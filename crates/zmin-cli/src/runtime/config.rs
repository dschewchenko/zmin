use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use zmin_git_core::{GitHashAlgorithm, GitObjectKind, LooseObjectStore, RefStore, RefTarget};

use crate::runtime::{git_path_config_output, normalize_windows_input_path};

use super::{
    CliError, GitRepo, Result, bytes_eq, bytes_starts_with, read_common_git_dir, resolve_objectish,
    wildcard_match_pathspec,
};

static GLOBAL_CONFIG_ENTRIES: OnceLock<Mutex<Vec<ConfigEntry>>> = OnceLock::new();
static CONFIG_FILE_CACHE: OnceLock<Mutex<HashMap<ConfigFileCacheKey, Vec<ConfigEntry>>>> =
    OnceLock::new();
const REFTABLE_LOCK_TIMEOUT_ENV: &str = "ZMIN_REFTABLE_LOCK_TIMEOUT_MS";
const REFTABLE_BLOCK_SIZE_ENV: &str = "ZMIN_REFTABLE_BLOCK_SIZE";
const REFTABLE_INDEX_OBJECTS_ENV: &str = "ZMIN_REFTABLE_INDEX_OBJECTS";
const REFTABLE_RESTART_INTERVAL_ENV: &str = "ZMIN_REFTABLE_RESTART_INTERVAL";
pub(crate) const DEFAULT_CORE_BIG_FILE_THRESHOLD_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) fn set_global_config_entries(entries: Vec<ConfigEntry>) {
    GLOBAL_CONFIG_ENTRIES
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("global config lock poisoned")
        .extend(entries);
}

pub(crate) fn global_config_entries() -> Vec<ConfigEntry> {
    GLOBAL_CONFIG_ENTRIES
        .get()
        .map(|entries| entries.lock().expect("global config lock poisoned").clone())
        .unwrap_or_default()
}

pub(crate) fn global_command_config_value(section: &str, key: &str) -> Option<String> {
    global_config_entries().into_iter().rev().find_map(|entry| {
        (entry.section == section && entry.subsection.is_empty() && entry.key == key)
            .then_some(entry.value)
    })
}

pub(crate) fn propagate_reftable_lock_timeout_override() {
    let Some(value) = global_command_config_value("reftable", "locktimeout") else {
        return;
    };
    // SAFETY: command-line configuration is applied before command workers start.
    unsafe {
        std::env::set_var(REFTABLE_LOCK_TIMEOUT_ENV, value);
    }
}

pub(crate) fn propagate_reftable_write_options_overrides() {
    for (key, environment) in [
        ("blocksize", REFTABLE_BLOCK_SIZE_ENV),
        ("restartinterval", REFTABLE_RESTART_INTERVAL_ENV),
        ("indexobjects", REFTABLE_INDEX_OBJECTS_ENV),
    ] {
        let Some(value) = global_command_config_value("reftable", key) else {
            continue;
        };
        // SAFETY: command-line configuration is applied before command workers start.
        unsafe {
            std::env::set_var(environment, value);
        }
    }
}

pub(crate) fn validate_reftable_lock_timeout_override() -> Result<()> {
    let Some(raw) = global_command_config_value("reftable", "locktimeout") else {
        return Ok(());
    };
    let milliseconds =
        zmin_git_core::parse_reftable_lock_timeout_millis(&raw).map_err(|error| {
            CliError::Fatal {
                code: 128,
                message: error.to_string(),
            }
        })?;
    if milliseconds < -1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "reftable lock timeout does not support negative values other than -1"
                .to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigEntry {
    pub(crate) section: String,
    pub(crate) raw_section: String,
    pub(crate) subsection: String,
    pub(crate) key: String,
    pub(crate) raw_key: String,
    pub(crate) value: String,
    pub(crate) comment: Option<String>,
    pub(crate) implicit_bool: bool,
    pub(crate) scope: ConfigScope,
    pub(crate) origin: String,
    pub(crate) line: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyRemoteConfig {
    pub(crate) url: String,
    pub(crate) fetch_refspecs: Vec<String>,
}

#[derive(Debug)]
struct ConfigIncludeContext {
    git_dir: PathBuf,
    work_tree: PathBuf,
    branch: Option<String>,
    remote_urls: Vec<String>,
}

struct ConfigLogicalLine {
    text: String,
    line_no: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ConfigScope {
    System,
    Global,
    Local,
    Worktree,
    Command,
}

impl ConfigScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            ConfigScope::System => "system",
            ConfigScope::Global => "global",
            ConfigScope::Local => "local",
            ConfigScope::Worktree => "worktree",
            ConfigScope::Command => "command",
        }
    }
}

impl ConfigEntry {
    pub(crate) fn name(&self) -> String {
        if self.subsection.is_empty() {
            format!("{}.{}", self.section, self.key)
        } else {
            format!("{}.{}.{}", self.section, self.subsection, self.key)
        }
    }

    pub(crate) fn list_line(&self) -> String {
        if self.implicit_bool {
            self.name()
        } else {
            format!("{}={}", self.name(), self.value)
        }
    }

    pub(crate) fn bool_value(&self) -> Option<bool> {
        if self.implicit_bool {
            Some(true)
        } else {
            parse_git_bool(&self.value)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ConfigFileCacheKey {
    path: PathBuf,
    scope: ConfigScope,
    origin: String,
    fingerprint: ConfigFileFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConfigFileFingerprint {
    Missing,
    Present {
        len: u64,
        modified_nanos: u128,
        status_changed_nanos: u128,
    },
}

fn config_file_cache() -> &'static Mutex<HashMap<ConfigFileCacheKey, Vec<ConfigEntry>>> {
    CONFIG_FILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn config_file_cache_key(path: &Path, scope: ConfigScope, origin: String) -> ConfigFileCacheKey {
    ConfigFileCacheKey {
        path: path.to_path_buf(),
        scope,
        origin,
        fingerprint: config_file_fingerprint(path),
    }
}

fn config_file_fingerprint(path: &Path) -> ConfigFileFingerprint {
    match fs::metadata(path) {
        Ok(metadata) => ConfigFileFingerprint::Present {
            len: metadata.len(),
            modified_nanos: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or(0),
            status_changed_nanos: metadata_status_changed_nanos(&metadata),
        },
        Err(_) => ConfigFileFingerprint::Missing,
    }
}

#[cfg(unix)]
fn metadata_status_changed_nanos(metadata: &fs::Metadata) -> u128 {
    use std::os::unix::fs::MetadataExt;

    (metadata.ctime() as i128)
        .saturating_mul(1_000_000_000)
        .saturating_add(metadata.ctime_nsec() as i128)
        .max(0) as u128
}

#[cfg(not(unix))]
fn metadata_status_changed_nanos(_metadata: &fs::Metadata) -> u128 {
    0
}

pub(crate) fn parse_global_config_entry(raw: &str) -> Result<ConfigEntry> {
    let (name, value, implicit_bool) = raw
        .split_once('=')
        .map(|(name, value)| (name, value, false))
        .unwrap_or((raw, "", true));
    parse_global_config_parts(name, value, implicit_bool, raw)
}

fn parse_global_config_parts(
    name: &str,
    value: &str,
    implicit_bool: bool,
    raw: &str,
) -> Result<ConfigEntry> {
    let (section, subsection, key) = parse_config_name(name).map_err(|_| CliError::Stderr {
        code: 1,
        text: format!("error: key does not contain a section: {name}\n"),
    })?;
    let allows_newline = section == "core" && subsection.is_empty() && key == "commentchar";
    if value.contains('\0') || value.contains('\r') || (!allows_newline && value.contains('\n')) {
        return Err(CliError::Stderr {
            code: 1,
            text: format!("error: bogus config parameter: {raw}\n"),
        });
    }
    if section == "core"
        && subsection.is_empty()
        && key == "bare"
        && !implicit_bool
        && parse_git_bool(value).is_none()
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{value}' for 'core.bare'"),
        });
    }
    if section == "core"
        && subsection.is_empty()
        && key == "bigfilethreshold"
        && parse_git_unsigned_size(value).is_none()
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "bad numeric config value '{value}' for 'core.bigfilethreshold': invalid unit"
            ),
        });
    }
    Ok(ConfigEntry {
        section,
        raw_section: name.split('.').next().unwrap_or_default().to_owned(),
        subsection,
        raw_key: key.clone(),
        key,
        value: value.to_owned(),
        comment: None,
        implicit_bool,
        scope: ConfigScope::Command,
        origin: "command line:".to_owned(),
        line: None,
    })
}

pub(crate) fn core_big_file_threshold(repo: &GitRepo) -> Result<u64> {
    let entries = read_config_entries(repo)?;
    let Some(entry) = effective_config_entry(&entries, "core", "bigfilethreshold") else {
        return Ok(DEFAULT_CORE_BIG_FILE_THRESHOLD_BYTES);
    };
    parse_git_unsigned_size(&entry.value).ok_or_else(|| bad_numeric_config_value(entry))
}

pub(crate) fn bulk_checkin_compression_level(repo: &GitRepo) -> Result<u32> {
    let entries = read_config_entries(repo)?;
    let value = effective_config_entry(&entries, "pack", "compression")
        .or_else(|| effective_config_entry(&entries, "core", "compression"));
    let Some(entry) = value else {
        return Ok(6);
    };
    let level = entry
        .value
        .parse::<i32>()
        .map_err(|_| bad_compression_config_value(entry))?;
    match level {
        -1 => Ok(6),
        0..=9 => Ok(level as u32),
        _ => Err(bad_compression_config_value(entry)),
    }
}

pub(crate) fn bulk_checkin_pack_size_limit(repo: &GitRepo) -> Result<Option<u64>> {
    let entries = read_config_entries(repo)?;
    let Some(entry) = effective_config_entry(&entries, "pack", "packsizelimit") else {
        return Ok(None);
    };
    parse_git_unsigned_size(&entry.value)
        .map(Some)
        .ok_or_else(|| bad_numeric_config_value(entry))
}

fn effective_config_entry<'a>(
    entries: &'a [ConfigEntry],
    section: &str,
    key: &str,
) -> Option<&'a ConfigEntry> {
    entries
        .iter()
        .rev()
        .find(|entry| entry.section == section && entry.subsection.is_empty() && entry.key == key)
}

fn bad_numeric_config_value(entry: &ConfigEntry) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "bad numeric config value '{}' for '{}.{}': invalid unit",
            entry.value, entry.section, entry.key
        ),
    }
}

fn bad_compression_config_value(entry: &ConfigEntry) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("bad zlib compression level {}", entry.value),
    }
}

pub(crate) fn parse_git_unsigned_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let digits_len = raw.bytes().take_while(u8::is_ascii_digit).count();
    if digits_len == 0 {
        return None;
    }
    let base = raw[..digits_len].parse::<u64>().ok()?;
    let multiplier = match &raw[digits_len..] {
        "" => 1,
        "k" | "K" => 1024,
        "m" | "M" => 1024 * 1024,
        "g" | "G" => 1024 * 1024 * 1024,
        _ => return None,
    };
    base.checked_mul(multiplier)
}

pub(crate) fn parse_global_config_env_entry(raw: &str) -> Result<ConfigEntry> {
    let Some((name, env_name)) = raw.rsplit_once('=') else {
        return Err(CliError::Stderr {
            code: 129,
            text: format!("invalid config format: {raw}\n"),
        });
    };
    if env_name.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("missing environment variable name for configuration '{name}'"),
        });
    }
    let value = std::env::var(env_name).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("missing environment variable '{env_name}' for configuration '{name}'"),
    })?;
    parse_global_config_parts(name, &value, false, &format!("{name}={value}"))
}

pub(crate) fn read_global_config_env_entries() -> Result<Vec<ConfigEntry>> {
    let mut entries = Vec::new();
    entries.extend(read_git_config_count_entries()?);
    entries.extend(read_git_config_parameters_entries()?);
    Ok(entries)
}

pub(crate) fn read_protected_config_entries() -> Result<Vec<ConfigEntry>> {
    let mut entries = Vec::new();
    for path in system_config_paths() {
        entries.extend(read_config_file_with_source(
            &path,
            ConfigScope::System,
            format!("file:{}", path.display()),
            None,
            0,
            false,
            None,
        )?);
    }
    for path in global_config_paths() {
        entries.extend(read_config_file_with_source(
            &path,
            ConfigScope::Global,
            format!("file:{}", path.display()),
            None,
            0,
            false,
            None,
        )?);
    }
    entries.extend(read_global_config_env_entries()?);
    entries.extend(global_config_entries());
    Ok(entries)
}

fn read_git_config_count_entries() -> Result<Vec<ConfigEntry>> {
    let Some(raw_count) = std::env::var_os("GIT_CONFIG_COUNT") else {
        return Ok(Vec::new());
    };
    let raw_count = raw_count.to_string_lossy();
    if raw_count.is_empty() {
        return Ok(Vec::new());
    }
    let count = raw_count.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: if raw_count.bytes().all(|byte| byte.is_ascii_digit()) {
            "too many entries in GIT_CONFIG_COUNT".into()
        } else {
            "bogus count in GIT_CONFIG_COUNT".into()
        },
    })?;
    if count > 1_000_000 {
        return Err(CliError::Fatal {
            code: 128,
            message: "too many entries in GIT_CONFIG_COUNT".into(),
        });
    }
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let key_name = format!("GIT_CONFIG_KEY_{index}");
        let value_name = format!("GIT_CONFIG_VALUE_{index}");
        let key = std::env::var(&key_name).map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("missing config key {key_name}"),
        })?;
        let value = std::env::var(&value_name).map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("missing config value {value_name}"),
        })?;
        entries.push(parse_global_config_parts(
            &key,
            &value,
            false,
            &format!("{key}={value}"),
        )?);
    }
    Ok(entries)
}

fn read_git_config_parameters_entries() -> Result<Vec<ConfigEntry>> {
    let Some(raw_parameters) = std::env::var_os("GIT_CONFIG_PARAMETERS") else {
        return Ok(Vec::new());
    };
    split_git_config_parameters(&raw_parameters.to_string_lossy())?
        .into_iter()
        .map(|word| {
            if let Some(separator) = word.separator {
                let name = &word.text[..separator];
                let value = &word.text[separator..];
                parse_global_config_parts(name, value, value.is_empty(), &format!("{name}={value}"))
            } else {
                parse_global_config_entry(&word.text)
            }
        })
        .collect()
}

struct ConfigParameterWord {
    text: String,
    separator: Option<usize>,
}

fn split_git_config_parameters(input: &str) -> Result<Vec<ConfigParameterWord>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote = None;
    let mut separator = None;
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (None, '\'') => quote = Some('\''),
            (None, '"') => quote = Some('"'),
            (None, '=') if separator.is_none() => separator = Some(current.len()),
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    if next == '\'' && chars.peek() != Some(&'\'') {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "bogus format in GIT_CONFIG_PARAMETERS".into(),
                        });
                    }
                    current.push(next);
                } else {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "bogus format in GIT_CONFIG_PARAMETERS".into(),
                    });
                }
            }
            (None, ch) if ch.is_whitespace() => {
                if !current.is_empty() || separator.is_some() {
                    words.push(ConfigParameterWord {
                        text: std::mem::take(&mut current),
                        separator: separator.take(),
                    });
                }
            }
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (_, ch) => current.push(ch),
        }
    }
    if quote.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "unterminated quote in GIT_CONFIG_PARAMETERS".into(),
        });
    }
    if !current.is_empty() || separator.is_some() {
        words.push(ConfigParameterWord {
            text: current,
            separator,
        });
    }
    Ok(words)
}

pub(crate) fn read_config_value(repo: &GitRepo, name: &str) -> io::Result<Option<String>> {
    Ok(read_config_entry(repo, name)?.map(|entry| entry.value))
}

pub(crate) fn read_global_config_value(name: &str) -> Result<Option<String>> {
    let (section, subsection, key) = parse_config_name(name)?;
    let entries = read_protected_config_entries()?;
    Ok(entries.into_iter().rev().find_map(|entry| {
        (entry.section == section && entry.subsection == subsection && entry.key == key)
            .then_some(entry.value)
    }))
}

pub(crate) fn read_config_entry(repo: &GitRepo, name: &str) -> io::Result<Option<ConfigEntry>> {
    let (section, subsection, key) = parse_config_name(name)?;
    read_config_section_entry(repo, &section, &subsection, &key)
}

pub(crate) fn validate_repository_format(repo: &GitRepo) -> Result<()> {
    let entries = read_common_config_entries(repo)?;
    validate_repository_format_entries(&entries)
}

pub(crate) fn read_validated_repository_format_entries(repo: &GitRepo) -> Result<Vec<ConfigEntry>> {
    let entries = read_common_config_entries(repo)?;
    validate_repository_format_entries(&entries)?;
    Ok(entries)
}

fn validate_repository_format_entries(entries: &[ConfigEntry]) -> Result<()> {
    let version = entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == "core"
                && entry.subsection.is_empty()
                && entry.key == "repositoryformatversion"
        })
        .map(|entry| entry.value.parse::<u32>())
        .transpose()
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: "invalid repository format version".into(),
        })?
        .unwrap_or(0);
    if version > 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("Expected git repo version <= 1, found {version}"),
        });
    }
    for entry in entries.iter().filter(|entry| entry.section == "extensions") {
        let known = matches!(
            entry.key.as_str(),
            "noop"
                | "noop-v1"
                | "objectformat"
                | "refstorage"
                | "worktreeconfig"
                | "preciousobjects"
                | "partialclone"
                | "compatobjectformat"
        );
        let requires_v1 = entry.key != "noop";
        if version == 0 && known && requires_v1 {
            return Err(CliError::Fatal {
                code: 128,
                message: "repo version is 0, but v1-only extension found".into(),
            });
        }
        if version == 1 && !known {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "unknown repository extension found: extensions.{}",
                    entry.key
                ),
            });
        }
    }
    Ok(())
}

pub(crate) fn repository_has_extension(repo: &GitRepo, key: &str) -> Result<bool> {
    Ok(read_common_config_entries(repo)?.into_iter().any(|entry| {
        entry.section == "extensions"
            && entry.subsection.is_empty()
            && entry.key == key
            && entry.bool_value().unwrap_or(true)
    }))
}

pub(crate) fn read_config_section_value(
    repo: &GitRepo,
    section: &str,
    subsection: &str,
    key: &str,
) -> io::Result<Option<String>> {
    Ok(read_config_section_entry(repo, section, subsection, key)?.map(|entry| entry.value))
}

pub(crate) fn read_config_section_entry(
    repo: &GitRepo,
    section: &str,
    subsection: &str,
    key: &str,
) -> io::Result<Option<ConfigEntry>> {
    Ok(read_config_entries(repo)?.into_iter().rev().find(|entry| {
        entry.section == section && entry.subsection == subsection && entry.key == key
    }))
}

pub(crate) fn read_config_entries(repo: &GitRepo) -> io::Result<Vec<ConfigEntry>> {
    let mut entries = Vec::new();
    let include_context = ConfigIncludeContext::new(repo)?;
    for path in system_config_paths() {
        entries.extend(read_config_file_with_source(
            &path,
            ConfigScope::System,
            format!("file:{}", path.display()),
            Some(&include_context),
            0,
            false,
            None,
        )?);
    }
    for global in global_config_paths() {
        entries.extend(read_config_file_with_source(
            &global,
            ConfigScope::Global,
            format!("file:{}", global.display()),
            Some(&include_context),
            0,
            false,
            None,
        )?);
    }
    entries.extend(read_local_config_entries_with_includes(repo)?);
    let global_entries = global_config_entries();
    if !global_entries.is_empty() {
        let command_include_dir = repo.root.clone();
        entries.extend(expand_config_entries_with_includes(
            global_entries,
            ConfigScope::Command,
            &command_include_dir,
            "command line",
            "command line",
            Some(&include_context),
            0,
            false,
        )?);
    }
    Ok(entries)
}

pub(crate) fn read_config_entries_no_includes(repo: &GitRepo) -> io::Result<Vec<ConfigEntry>> {
    let mut entries = Vec::new();
    for path in system_config_paths() {
        entries.extend(read_config_file_raw(
            &path,
            ConfigScope::System,
            format!("file:{}", path.display()),
        )?);
    }
    for global in global_config_paths() {
        entries.extend(read_config_file_raw(
            &global,
            ConfigScope::Global,
            format!("file:{}", global.display()),
        )?);
    }
    entries.extend(read_local_config_entries(repo)?);
    entries.extend(global_config_entries());
    Ok(entries)
}

pub(crate) fn read_config_entries_without_repo(no_includes: bool) -> io::Result<Vec<ConfigEntry>> {
    let mut entries = Vec::new();
    for (scope, paths) in [
        (ConfigScope::System, system_config_paths()),
        (ConfigScope::Global, global_config_paths()),
    ] {
        for path in paths {
            let origin = format!("file:{}", path.display());
            if no_includes {
                entries.extend(read_config_file_raw(&path, scope, origin)?);
            } else {
                entries.extend(read_config_file_with_source(
                    &path, scope, origin, None, 0, false, None,
                )?);
            }
        }
    }
    entries.extend(global_config_entries());
    Ok(entries)
}

pub(crate) fn global_config_paths() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("GIT_CONFIG_GLOBAL") {
        return vec![normalize_windows_input_path(PathBuf::from(path))];
    }
    let mut paths = Vec::new();
    for home in global_config_homes() {
        paths.push(xdg_config_home(&home).join("git/config"));
        paths.push(home.join(".gitconfig"));
    }
    paths
}

pub(crate) fn global_config_path_for_write() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GIT_CONFIG_GLOBAL") {
        return Some(normalize_windows_input_path(PathBuf::from(path)));
    }
    let home = global_config_homes().into_iter().next()?;
    let legacy = home.join(".gitconfig");
    if legacy.exists() {
        return Some(legacy);
    }
    let xdg = xdg_config_home(&home).join("git/config");
    if xdg.exists() {
        return Some(xdg);
    }
    Some(legacy)
}

pub(crate) fn global_config_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    let mut seen = BTreeSet::new();
    for home in global_config_home_candidates() {
        if seen.insert(home.clone()) {
            homes.push(home);
        }
    }
    homes
}

fn global_config_home_candidates() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        homes.push(PathBuf::from(home));
    }
    #[cfg(windows)]
    {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            homes.push(PathBuf::from(home));
        }
        if let (Some(drive), Some(path)) =
            (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
        {
            let mut home = PathBuf::from(drive);
            home.push(path);
            homes.push(home);
        }
    }
    homes
}

pub(crate) fn xdg_config_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
}

pub(crate) fn system_config_paths() -> Vec<PathBuf> {
    if std::env::var_os("GIT_CONFIG_NOSYSTEM").is_some_and(|value| {
        !matches!(
            value.to_string_lossy().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        )
    }) {
        return Vec::new();
    }
    if let Some(path) = explicit_system_config_path_from_env() {
        return vec![path];
    }
    #[cfg(target_os = "macos")]
    {
        for path in [
            "/Applications/Xcode.app/Contents/Developer/usr/share/git-core/gitconfig",
            "/Library/Developer/CommandLineTools/usr/share/git-core/gitconfig",
            "/opt/homebrew/etc/gitconfig",
            "/etc/gitconfig",
        ] {
            let path = PathBuf::from(path);
            if path.exists() {
                return vec![path];
            }
        }
        Vec::new()
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![PathBuf::from("/etc/gitconfig")]
    }
}

pub(crate) fn explicit_system_config_path() -> PathBuf {
    if let Some(path) = explicit_system_config_path_from_env() {
        return path;
    }
    #[cfg(target_os = "macos")]
    {
        for path in [
            "/Applications/Xcode.app/Contents/Developer/usr/share/git-core/gitconfig",
            "/Library/Developer/CommandLineTools/usr/share/git-core/gitconfig",
            "/opt/homebrew/etc/gitconfig",
            "/etc/gitconfig",
        ] {
            let path = PathBuf::from(path);
            if path.exists() {
                return path;
            }
        }
        PathBuf::from("/etc/gitconfig")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from("/etc/gitconfig")
    }
}

fn explicit_system_config_path_from_env() -> Option<PathBuf> {
    std::env::var_os("GIT_CONFIG_SYSTEM")
        .map(|path| normalize_windows_input_path(PathBuf::from(path)))
}

pub(crate) fn read_local_config_entries(repo: &GitRepo) -> io::Result<Vec<ConfigEntry>> {
    let mut entries = read_config_file_raw(
        &local_config_path(repo)?,
        ConfigScope::Local,
        "file:.git/config".to_owned(),
    )?;
    if worktree_config_enabled(&entries) {
        entries.extend(read_config_file_raw(
            &worktree_config_path(repo),
            ConfigScope::Worktree,
            "file:.git/config.worktree".to_owned(),
        )?);
    }
    Ok(entries)
}

pub(crate) fn read_local_config_entries_with_includes(
    repo: &GitRepo,
) -> io::Result<Vec<ConfigEntry>> {
    let include_context = ConfigIncludeContext::new(repo)?;
    let mut entries = read_config_file_with_source(
        &local_config_path(repo)?,
        ConfigScope::Local,
        "file:.git/config".to_owned(),
        Some(&include_context),
        0,
        false,
        None,
    )?;
    if worktree_config_enabled(&entries) {
        entries.extend(read_config_file_with_source(
            &worktree_config_path(repo),
            ConfigScope::Worktree,
            "file:.git/config.worktree".to_owned(),
            Some(&include_context),
            0,
            false,
            None,
        )?);
    }
    Ok(entries)
}

pub(crate) fn read_common_config_entries(repo: &GitRepo) -> io::Result<Vec<ConfigEntry>> {
    read_config_file(&local_config_path(repo)?)
}

pub(crate) fn read_worktree_config_entries(repo: &GitRepo) -> io::Result<Vec<ConfigEntry>> {
    read_config_file(&worktree_config_path(repo))
}

pub(crate) fn read_scoped_worktree_config_entries(repo: &GitRepo) -> Result<Vec<ConfigEntry>> {
    Ok(read_config_file(&worktree_config_path_for_scope(repo)?)?)
}

pub(crate) fn read_worktree_config_entry(
    repo: &GitRepo,
    name: &str,
) -> Result<Option<ConfigEntry>> {
    let (section, subsection, key) = parse_config_name(name)?;
    Ok(read_scoped_worktree_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == section && entry.subsection == subsection && entry.key == key
        }))
}

pub(crate) fn write_common_config_entries(
    repo: &GitRepo,
    entries: &[ConfigEntry],
) -> io::Result<()> {
    write_config_entries(&local_config_path(repo)?, entries)
}

pub(crate) fn read_config_file(path: &std::path::Path) -> io::Result<Vec<ConfigEntry>> {
    read_config_file_raw(path, ConfigScope::Local, String::new())
}

pub(crate) fn read_config_file_required(
    path: &Path,
    scope: ConfigScope,
) -> io::Result<Vec<ConfigEntry>> {
    let origin = format!(
        "file:{}",
        encode_config_origin_path(&path.to_string_lossy())
    );
    let content = fs::read_to_string(path)?;
    parse_config_text(&content, scope, origin, &path.to_string_lossy())
}

pub(crate) fn read_config_file_required_with_includes(
    path: &Path,
    scope: ConfigScope,
    repo: Option<&GitRepo>,
) -> io::Result<Vec<ConfigEntry>> {
    let origin = format!(
        "file:{}",
        encode_config_origin_path(&path.to_string_lossy())
    );
    let context = repo.map(ConfigIncludeContext::new).transpose()?;
    read_config_file_with_source(path, scope, origin, context.as_ref(), 0, false, None)
}

fn encode_config_origin_path(value: &str) -> String {
    let quoted = value.chars().any(|ch| ch.is_whitespace() || ch == '"');
    let mut encoded = String::with_capacity(value.len() + usize::from(quoted) * 2);
    if quoted {
        encoded.push('"');
    }
    for ch in value.chars() {
        match ch {
            '\\' => encoded.push_str("\\\\"),
            '"' => encoded.push_str("\\\""),
            _ => encoded.push(ch),
        }
    }
    if quoted {
        encoded.push('"');
    }
    encoded
}

pub(crate) fn read_config_file_scoped(
    path: &Path,
    scope: ConfigScope,
) -> io::Result<Vec<ConfigEntry>> {
    match read_config_file_required(path, scope) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        result => result,
    }
}

pub(crate) fn read_config_stdin() -> io::Result<Vec<ConfigEntry>> {
    let mut content = String::new();
    io::stdin().read_to_string(&mut content)?;
    parse_config_text(
        &content,
        ConfigScope::Local,
        "standard input:".to_owned(),
        "standard input",
    )
}

pub(crate) fn read_config_stdin_with_includes(
    repo: Option<&GitRepo>,
) -> io::Result<Vec<ConfigEntry>> {
    let entries = read_config_stdin()?;
    let context = repo.map(ConfigIncludeContext::new).transpose()?;
    let base_dir = std::env::current_dir()?;
    expand_config_entries_with_includes(
        entries,
        ConfigScope::Local,
        &base_dir,
        "standard input",
        "standard input",
        context.as_ref(),
        0,
        false,
    )
}

fn read_config_file_with_source(
    path: &std::path::Path,
    scope: ConfigScope,
    origin: String,
    include_context: Option<&ConfigIncludeContext>,
    include_depth: usize,
    hasconfig_included: bool,
    included_from: Option<&str>,
) -> io::Result<Vec<ConfigEntry>> {
    if include_depth > 10 {
        return Err(maximum_include_depth_error(
            &config_error_source(path, &origin),
            included_from,
        ));
    }
    let entries = read_config_file_raw(path, scope, origin.clone())?;
    if hasconfig_included && entries.iter().any(config_entry_is_remote_url) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "remote URLs cannot be configured in file directly or indirectly included by includeIf.hasconfig:remote.*.url",
        ));
    }
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let base_origin = origin
        .strip_prefix("file:")
        .unwrap_or(origin.as_str())
        .rsplit_once('/')
        .map(|(base, _)| base.to_owned())
        .unwrap_or_else(|| ".".to_owned());
    expand_config_entries_with_includes(
        entries,
        scope,
        base_dir,
        &base_origin,
        &config_error_source(path, &origin),
        include_context,
        include_depth,
        hasconfig_included,
    )
}

fn expand_config_entries_with_includes(
    entries: Vec<ConfigEntry>,
    scope: ConfigScope,
    base_dir: &Path,
    base_origin: &str,
    source: &str,
    include_context: Option<&ConfigIncludeContext>,
    include_depth: usize,
    hasconfig_included: bool,
) -> io::Result<Vec<ConfigEntry>> {
    if include_depth > 10 {
        return Err(maximum_include_depth_error(base_origin, Some(source)));
    }
    let mut with_includes = Vec::with_capacity(entries.len());
    for entry in entries {
        let plain_include =
            entry.section == "include" && entry.subsection.is_empty() && entry.key == "path";
        let hasconfig_include = entry.section == "includeif"
            && entry.key == "path"
            && include_context.is_some_and(|context| {
                config_include_hasconfig_condition_matches(&entry.subsection, context)
            });
        let conditional_include = entry.section == "includeif"
            && entry.key == "path"
            && include_context.is_some_and(|context| {
                config_include_condition_matches(&entry.subsection, base_dir, context)
            });
        let include_path = (plain_include || conditional_include || hasconfig_include)
            .then(|| entry.value.clone());
        with_includes.push(entry);
        if let Some(include_path) = include_path {
            if matches!(base_origin, "command line" | "blob" | "standard input")
                && !config_include_path_is_absolute(&include_path)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("relative config include is not allowed from {base_origin}"),
                ));
            }
            let Some((actual_path, origin_path)) =
                resolve_config_include_path(base_dir, &base_origin, &include_path)
            else {
                continue;
            };
            with_includes.extend(read_config_file_with_source(
                &actual_path,
                scope,
                format!("file:{origin_path}"),
                include_context,
                include_depth + 1,
                hasconfig_included || hasconfig_include,
                Some(source),
            )?);
        }
    }
    Ok(with_includes)
}

fn config_include_path_is_absolute(path: &str) -> bool {
    Path::new(path).is_absolute() || path == "~" || path.starts_with("~/")
}

fn maximum_include_depth_error(path: &str, included_from: Option<&str>) -> io::Error {
    let message = if let Some(included_from) = included_from {
        format!(
            "exceeded maximum include depth (10) while including\n\t{path}\nfrom\n\t{included_from}\nThis might be due to circular includes."
        )
    } else {
        "exceeded maximum include depth (10)".to_owned()
    };
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl ConfigIncludeContext {
    fn new(repo: &GitRepo) -> io::Result<Self> {
        let refs = RefStore::new(&repo.git_dir, repo_hash_algorithm_from_config(repo)?);
        let branch = match refs.read_head() {
            Ok(RefTarget::Symbolic(target)) => {
                target.strip_prefix("refs/heads/").map(str::to_owned)
            }
            Ok(RefTarget::Direct(_)) => None,
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            git_dir: fs::canonicalize(&repo.git_dir)?,
            work_tree: fs::canonicalize(&repo.root)?,
            branch,
            remote_urls: collect_config_remote_urls(repo)?,
        })
    }
}

pub(crate) fn repo_hash_algorithm_from_config(repo: &GitRepo) -> io::Result<GitHashAlgorithm> {
    let value = read_config_file(&local_config_path(repo)?)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "extensions"
                && entry.subsection.is_empty()
                && entry.key == "objectformat"
        })
        .map(|entry| entry.value);
    match value.as_deref() {
        None | Some("sha1") => Ok(GitHashAlgorithm::Sha1),
        Some("sha256") => Ok(GitHashAlgorithm::Sha256),
        Some(value) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid object format '{value}'"),
        )),
    }
}

fn config_include_condition_matches(
    condition: &str,
    base_dir: &Path,
    context: &ConfigIncludeContext,
) -> bool {
    if let Some(pattern) = condition.strip_prefix("gitdir:") {
        return gitdir_include_pattern_matches(pattern, false, base_dir, context);
    }
    if let Some(pattern) = condition.strip_prefix("gitdir/i:") {
        return gitdir_include_pattern_matches(pattern, true, base_dir, context);
    }
    if let Some(pattern) = condition.strip_prefix("onbranch:") {
        return onbranch_include_pattern_matches(pattern, context.branch.as_deref());
    }
    false
}

fn config_include_hasconfig_condition_matches(
    condition: &str,
    context: &ConfigIncludeContext,
) -> bool {
    let Some(pattern) = condition.strip_prefix("hasconfig:remote.*.url:") else {
        return false;
    };
    context
        .remote_urls
        .iter()
        .any(|url| hasconfig_remote_url_pattern_matches(pattern, url))
}

fn hasconfig_remote_url_pattern_matches(pattern: &str, url: &str) -> bool {
    let mut memo = vec![None; (pattern.len() + 1) * (url.len() + 1)];
    hasconfig_remote_url_pattern_matches_memo(pattern.as_bytes(), url.as_bytes(), 0, 0, &mut memo)
}

fn hasconfig_remote_url_pattern_matches_memo(
    pattern: &[u8],
    value: &[u8],
    pattern_index: usize,
    value_index: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let width = value.len() + 1;
    let memo_index = pattern_index * width + value_index;
    if let Some(result) = memo[memo_index] {
        return result;
    }
    let result = if pattern_index == pattern.len() {
        value_index == value.len()
    } else if pattern[pattern_index..].starts_with(b"**") {
        hasconfig_remote_url_pattern_matches_memo(
            pattern,
            value,
            pattern_index + 2,
            value_index,
            memo,
        ) || (value_index < value.len()
            && hasconfig_remote_url_pattern_matches_memo(
                pattern,
                value,
                pattern_index,
                value_index + 1,
                memo,
            ))
    } else {
        match pattern[pattern_index] {
            b'*' => {
                hasconfig_remote_url_pattern_matches_memo(
                    pattern,
                    value,
                    pattern_index + 1,
                    value_index,
                    memo,
                ) || (value_index < value.len()
                    && value[value_index] != b'/'
                    && hasconfig_remote_url_pattern_matches_memo(
                        pattern,
                        value,
                        pattern_index,
                        value_index + 1,
                        memo,
                    ))
            }
            b'?' => {
                value_index < value.len()
                    && value[value_index] != b'/'
                    && hasconfig_remote_url_pattern_matches_memo(
                        pattern,
                        value,
                        pattern_index + 1,
                        value_index + 1,
                        memo,
                    )
            }
            b'[' => {
                if let Some((class_end, matched)) = hasconfig_wildcard_class_matches(
                    &pattern[pattern_index + 1..],
                    value.get(value_index),
                ) {
                    matched
                        && value[value_index] != b'/'
                        && hasconfig_remote_url_pattern_matches_memo(
                            pattern,
                            value,
                            pattern_index + class_end + 2,
                            value_index + 1,
                            memo,
                        )
                } else {
                    value.get(value_index) == Some(&b'[')
                        && hasconfig_remote_url_pattern_matches_memo(
                            pattern,
                            value,
                            pattern_index + 1,
                            value_index + 1,
                            memo,
                        )
                }
            }
            literal => {
                value.get(value_index) == Some(&literal)
                    && hasconfig_remote_url_pattern_matches_memo(
                        pattern,
                        value,
                        pattern_index + 1,
                        value_index + 1,
                        memo,
                    )
            }
        }
    };
    memo[memo_index] = Some(result);
    result
}

fn hasconfig_wildcard_class_matches(class: &[u8], value: Option<&u8>) -> Option<(usize, bool)> {
    let value = *value?;
    let mut index = 0;
    let negated = matches!(class.first(), Some(b'!' | b'^'));
    if negated {
        index += 1;
    }
    let mut matched = false;
    let mut previous = None;
    while index < class.len() {
        let byte = class[index];
        if byte == b']' && previous.is_some() {
            return Some((index, if negated { !matched } else { matched }));
        }
        if byte == b'-'
            && let Some(start) = previous
            && let Some(end) = class.get(index + 1).copied()
            && end != b']'
        {
            if start <= value && value <= end {
                matched = true;
            }
            previous = Some(end);
            index += 2;
            continue;
        }
        if byte == value {
            matched = true;
        }
        previous = Some(byte);
        index += 1;
    }
    None
}

fn collect_config_remote_urls(repo: &GitRepo) -> io::Result<Vec<String>> {
    let mut urls = Vec::new();
    for path in system_config_paths() {
        collect_config_remote_urls_from_file(&path, ConfigScope::System, &mut urls)?;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        collect_config_remote_urls_from_file(
            &home.join(".gitconfig"),
            ConfigScope::Global,
            &mut urls,
        )?;
        let xdg = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("git/config");
        collect_config_remote_urls_from_file(&xdg, ConfigScope::Global, &mut urls)?;
    }
    collect_config_remote_urls_from_file(&local_config_path(repo)?, ConfigScope::Local, &mut urls)?;
    collect_config_remote_urls_from_file(
        &worktree_config_path(repo),
        ConfigScope::Worktree,
        &mut urls,
    )?;
    Ok(urls)
}

fn collect_config_remote_urls_from_file(
    path: &Path,
    scope: ConfigScope,
    urls: &mut Vec<String>,
) -> io::Result<()> {
    let entries = read_config_file_raw(path, scope, String::new())?;
    validate_hasconfig_include_targets(path, &entries)?;
    for entry in entries {
        if config_entry_is_remote_url(&entry) {
            urls.push(entry.value);
        }
    }
    Ok(())
}

fn validate_hasconfig_include_targets(path: &Path, entries: &[ConfigEntry]) -> io::Result<()> {
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let base_origin = path
        .to_str()
        .and_then(|value| value.rsplit_once('/').map(|(base, _)| base.to_owned()))
        .unwrap_or_else(|| ".".to_owned());
    for entry in entries {
        if entry.section != "includeif"
            || entry.key != "path"
            || !entry.subsection.starts_with("hasconfig:remote.*.url:")
        {
            continue;
        }
        let Some((actual_path, _origin_path)) =
            resolve_config_include_path(base_dir, &base_origin, &entry.value)
        else {
            continue;
        };
        let included = read_config_file_raw(&actual_path, entry.scope, String::new())?;
        if included.iter().any(config_entry_is_remote_url) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "remote URLs cannot be configured in file directly or indirectly included by includeIf.hasconfig:remote.*.url",
            ));
        }
    }
    Ok(())
}

fn config_entry_is_remote_url(entry: &ConfigEntry) -> bool {
    entry.section == "remote" && !entry.subsection.is_empty() && entry.key == "url"
}

fn onbranch_include_pattern_matches(pattern: &str, branch: Option<&str>) -> bool {
    let Some(branch) = branch else {
        return false;
    };
    if pattern
        .as_bytes()
        .iter()
        .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
    {
        return wildcard_match_pathspec(pattern, branch, false, true);
    }
    if let Some(prefix) = pattern.strip_suffix('/') {
        return branch == prefix || branch.starts_with(&format!("{prefix}/"));
    }
    branch == pattern
}

fn gitdir_include_pattern_matches(
    pattern: &str,
    icase: bool,
    base_dir: &Path,
    context: &ConfigIncludeContext,
) -> bool {
    let Some(pattern) = normalize_gitdir_include_pattern(pattern, base_dir) else {
        return false;
    };
    let git_dir = path_for_config_match(&context.git_dir);
    let work_tree = path_for_config_match(&context.work_tree);
    let mut candidates = vec![git_dir, work_tree];
    if let Some(pwd) = std::env::var_os("PWD") {
        let pwd = PathBuf::from(pwd);
        candidates.push(path_for_config_match(&pwd));
        candidates.push(path_for_config_match(&pwd.join(".git")));
    }
    if let Some(git_dir) = std::env::var_os("GIT_DIR") {
        candidates.push(path_for_config_match(&PathBuf::from(git_dir)));
    }
    candidates
        .iter()
        .any(|candidate| gitdir_pattern_matches_candidate(&pattern, candidate, icase))
}

fn normalize_gitdir_include_pattern(pattern: &str, base_dir: &Path) -> Option<String> {
    let trailing_directory = pattern.ends_with('/');
    let trimmed = pattern.trim_end_matches('/');
    let mut normalized = if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = fs::canonicalize(config_home_dir()?).ok()?;
        path_for_config_match(&home.join(rest))
    } else if let Some(rest) = trimmed.strip_prefix("./") {
        let base = fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
        path_for_config_match(&base.join(rest))
    } else {
        let path = Path::new(trimmed);
        if path.is_absolute() {
            path_for_config_match(path)
        } else {
            format!("**/{trimmed}")
        }
    };
    if trailing_directory {
        normalized.push_str("/**");
    }
    Some(normalized)
}

fn path_for_config_match(path: &Path) -> String {
    let mut value = normalize_windows_verbatim_path(path.display().to_string().replace('\\', "/"));
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    value
}

fn normalize_windows_verbatim_path(value: String) -> String {
    if let Some(rest) = value.strip_prefix("//?/UNC/") {
        return format!("//{rest}");
    }
    if let Some(rest) = value.strip_prefix("//?/") {
        return rest.to_owned();
    }
    value
}

fn gitdir_pattern_matches_candidate(pattern: &str, candidate: &str, icase: bool) -> bool {
    if pattern
        .as_bytes()
        .iter()
        .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
    {
        return gitdir_wildmatch(pattern, candidate, icase);
    }
    if bytes_eq(candidate.as_bytes(), pattern.as_bytes(), icase) {
        return true;
    }
    let mut prefix = pattern.as_bytes().to_vec();
    prefix.push(b'/');
    bytes_starts_with(candidate.as_bytes(), &prefix, icase)
}

fn gitdir_wildmatch(pattern: &str, candidate: &str, icase: bool) -> bool {
    let pattern = if icase {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_owned()
    };
    let candidate = if icase {
        candidate.to_ascii_lowercase()
    } else {
        candidate.to_owned()
    };
    let mut memo = vec![None; (pattern.len() + 1) * (candidate.len() + 1)];
    gitdir_wildmatch_memo(pattern.as_bytes(), candidate.as_bytes(), 0, 0, &mut memo)
}

fn gitdir_wildmatch_memo(
    pattern: &[u8],
    candidate: &[u8],
    pattern_index: usize,
    candidate_index: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let width = candidate.len() + 1;
    let memo_index = pattern_index * width + candidate_index;
    if let Some(result) = memo[memo_index] {
        return result;
    }
    let result = if pattern_index == pattern.len() {
        candidate_index == candidate.len()
    } else if pattern[pattern_index..].starts_with(b"**/") {
        gitdir_wildmatch_memo(pattern, candidate, pattern_index + 3, candidate_index, memo)
            || (candidate_index < candidate.len()
                && gitdir_wildmatch_memo(
                    pattern,
                    candidate,
                    pattern_index,
                    candidate_index + 1,
                    memo,
                ))
    } else if pattern[pattern_index..].starts_with(b"**") {
        gitdir_wildmatch_memo(pattern, candidate, pattern_index + 2, candidate_index, memo)
            || (candidate_index < candidate.len()
                && gitdir_wildmatch_memo(
                    pattern,
                    candidate,
                    pattern_index,
                    candidate_index + 1,
                    memo,
                ))
    } else {
        match pattern[pattern_index] {
            b'*' => {
                gitdir_wildmatch_memo(pattern, candidate, pattern_index + 1, candidate_index, memo)
                    || (candidate
                        .get(candidate_index)
                        .is_some_and(|byte| *byte != b'/')
                        && gitdir_wildmatch_memo(
                            pattern,
                            candidate,
                            pattern_index,
                            candidate_index + 1,
                            memo,
                        ))
            }
            b'?' => {
                candidate
                    .get(candidate_index)
                    .is_some_and(|byte| *byte != b'/')
                    && gitdir_wildmatch_memo(
                        pattern,
                        candidate,
                        pattern_index + 1,
                        candidate_index + 1,
                        memo,
                    )
            }
            literal => {
                candidate.get(candidate_index) == Some(&literal)
                    && gitdir_wildmatch_memo(
                        pattern,
                        candidate,
                        pattern_index + 1,
                        candidate_index + 1,
                        memo,
                    )
            }
        }
    };
    memo[memo_index] = Some(result);
    result
}

fn read_config_file_raw(
    path: &std::path::Path,
    scope: ConfigScope,
    origin: String,
) -> io::Result<Vec<ConfigEntry>> {
    let cache_key = config_file_cache_key(path, scope, origin.clone());
    if let Some(entries) = config_file_cache()
        .lock()
        .expect("config file cache lock")
        .get(&cache_key)
        .cloned()
    {
        return Ok(entries);
    }
    let entries = match fs::read_to_string(path) {
        Ok(content) => {
            let source = config_error_source(path, &origin);
            parse_config_text(&content, scope, origin, &source)?
        }
        Err(_) => Vec::new(),
    };
    config_file_cache()
        .lock()
        .expect("config file cache lock")
        .insert(cache_key, entries.clone());
    Ok(entries)
}

pub(crate) fn parse_config_blob_entries(
    repo: &GitRepo,
    objectish: &str,
    includes: bool,
) -> Result<Vec<ConfigEntry>> {
    let object_id = resolve_objectish(repo, objectish)?;
    let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
    let object = store.read_object(&object_id)?;
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("object {objectish} is not a blob"),
        });
    }
    let origin = format!("blob:{objectish}");
    let entries = parse_config_text(
        &String::from_utf8_lossy(&object.content),
        ConfigScope::Command,
        origin.clone(),
        &origin,
    )?;
    if !includes {
        return Ok(entries);
    }
    let context = ConfigIncludeContext::new(repo)?;
    Ok(expand_config_entries_with_includes(
        entries,
        ConfigScope::Command,
        &repo.root,
        "blob",
        &origin,
        Some(&context),
        0,
        false,
    )?)
}

fn parse_config_text(
    content: &str,
    scope: ConfigScope,
    origin: String,
    source: &str,
) -> io::Result<Vec<ConfigEntry>> {
    let mut current_section = None::<(String, String)>;
    let mut entries = Vec::new();
    for logical_line in config_logical_lines(content) {
        let line_no = logical_line.line_no;
        let mut trimmed = trim_config_syntax(&logical_line.text);
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(after_open) = trimmed.strip_prefix('[') {
            let Some((section, rest)) = after_open.split_once(']') else {
                return Err(bad_config_line_error(line_no, &source));
            };
            current_section = Some(parse_config_section(section));
            trimmed = trim_config_syntax(rest);
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
        }
        let Some((section, subsection)) = current_section.as_ref() else {
            return Err(bad_config_line_error(line_no, &source));
        };
        let (key, raw_value, implicit_bool) = trimmed
            .split_once('=')
            .map(|(key, value)| {
                (
                    trim_config_key(key),
                    value.trim_start_matches([' ', '\t']),
                    false,
                )
            })
            .unwrap_or((trim_config_key(trimmed), "", true));
        if !config_key_is_valid(key) {
            return Err(bad_config_line_error(line_no, &source));
        }
        entries.push(ConfigEntry {
            section: section.to_ascii_lowercase(),
            raw_section: section.clone(),
            subsection: subsection.clone(),
            key: key.to_ascii_lowercase(),
            raw_key: key.to_owned(),
            value: parse_config_value(raw_value, line_no, source)?,
            comment: None,
            implicit_bool,
            scope,
            origin: origin.clone(),
            line: Some(line_no),
        });
    }
    Ok(entries)
}

fn config_key_is_valid(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn config_logical_lines(content: &str) -> Vec<ConfigLogicalLine> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut start_line = 1usize;
    let physical_count = content.split('\n').count();
    for (index, physical) in content.split('\n').enumerate() {
        if current.is_empty() {
            start_line = index + 1;
        }
        current.push_str(physical);
        if config_physical_line_continues(physical) && index + 1 < physical_count {
            current.pop();
            continue;
        }
        lines.push(ConfigLogicalLine {
            text: std::mem::take(&mut current),
            line_no: start_line,
        });
    }
    lines
}

fn config_physical_line_continues(line: &str) -> bool {
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

fn trim_config_syntax(value: &str) -> &str {
    value.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\r'))
}

fn trim_config_key(value: &str) -> &str {
    value.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\r'))
}

fn parse_config_value(value: &str, line_no: usize, source: &str) -> io::Result<String> {
    let mut parsed = String::with_capacity(value.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut keep_len = 0usize;
    for ch in value.chars() {
        if escaped {
            let decoded = match ch {
                'n' => '\n',
                't' => '\t',
                'b' => '\u{0008}',
                '"' => '"',
                '\\' => '\\',
                _ => return Err(bad_config_line_error(line_no, source)),
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
        return Err(bad_config_line_error(line_no, source));
    }
    parsed.truncate(keep_len);
    Ok(parsed)
}

fn config_error_source(path: &Path, origin: &str) -> String {
    origin
        .strip_prefix("file:")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| path.to_str().unwrap_or(""))
        .to_owned()
}

fn bad_config_line_error(line: usize, source: &str) -> io::Error {
    let location = if source == "standard input" {
        source.to_owned()
    } else {
        format!("file {source}")
    };
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("bad config line {line} in {location}"),
    )
}

fn resolve_config_include_path(
    base_dir: &Path,
    base_origin: &str,
    value: &str,
) -> Option<(PathBuf, String)> {
    let decoded = decode_config_value(value);
    if let Some(rest) = decoded.strip_prefix("~/") {
        let home = config_home_dir()?;
        let actual = home.join(rest);
        return Some((actual.clone(), actual.display().to_string()));
    }
    let include_path = normalize_windows_input_path(PathBuf::from(&decoded));
    if include_path.is_absolute() {
        return Some((include_path.clone(), git_path_config_output(&include_path)));
    }
    Some((
        base_dir.join(&include_path),
        format!("{base_origin}/{decoded}"),
    ))
}

fn config_home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        return Some(PathBuf::from(home));
    }
    #[cfg(windows)]
    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        return Some(PathBuf::from(user_profile));
    }
    None
}

pub(crate) fn local_config_path(repo: &GitRepo) -> io::Result<PathBuf> {
    Ok(common_git_dir_for_config(repo)?.join("config"))
}

pub(crate) fn worktree_config_path(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("config.worktree")
}

pub(crate) fn worktree_config_path_for_scope(repo: &GitRepo) -> Result<PathBuf> {
    let common_entries = read_common_config_entries(repo)?;
    if worktree_config_enabled(&common_entries) {
        Ok(worktree_config_path(repo))
    } else {
        Ok(local_config_path(repo)?)
    }
}

pub(crate) fn ensure_worktree_config_scope(repo: &GitRepo) -> Result<()> {
    let common_entries = read_common_config_entries(repo)?;
    if worktree_config_enabled(&common_entries) || !has_multiple_worktrees(repo)? {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: "--worktree cannot be used with multiple working trees unless the config\nextension worktreeConfig is enabled. Please read \"CONFIGURATION FILE\"\nsection in \"git help worktree\" for details".into(),
    })
}

pub(crate) fn worktree_config_enabled(entries: &[ConfigEntry]) -> bool {
    entries.iter().rev().any(|entry| {
        entry.section == "extensions"
            && entry.subsection.is_empty()
            && entry.key == "worktreeconfig"
            && entry.bool_value() == Some(true)
    })
}

fn has_multiple_worktrees(repo: &GitRepo) -> io::Result<bool> {
    if repo.git_dir.join("commondir").exists() {
        return Ok(true);
    }
    let worktrees = common_git_dir_for_config(repo)?.join("worktrees");
    match fs::read_dir(worktrees) {
        Ok(mut entries) => Ok(entries.next().transpose()?.is_some()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(crate) fn common_git_dir_for_config(repo: &GitRepo) -> io::Result<PathBuf> {
    match fs::read_to_string(repo.git_dir.join("commondir")) {
        Ok(raw) => {
            let value = PathBuf::from(raw.trim());
            if value.is_absolute() {
                Ok(value)
            } else {
                Ok(repo.git_dir.join(value))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(repo.git_dir.clone()),
        Err(error) => Err(error),
    }
}

pub(crate) fn decode_config_value(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value);
    decode_config_escapes(inner)
}

fn decode_config_escapes(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{0008}'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(next) => {
                out.push('\\');
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub(crate) fn set_config_value(repo: &GitRepo, name: &str, value: &str) -> Result<()> {
    let path = local_config_path(repo)?;
    set_config_value_in_file(&path, name, value)
}

pub(crate) fn append_config_value(repo: &GitRepo, name: &str, value: &str) -> Result<()> {
    let path = local_config_path(repo)?;
    append_config_value_in_file(&path, name, value)
}

pub(crate) fn set_config_values(repo: &GitRepo, values: &[(String, String)]) -> Result<()> {
    let path = local_config_path(repo)?;
    let new_entries = values
        .iter()
        .map(|(name, value)| parse_config_entry(name, value))
        .collect::<Result<Vec<_>>>()?;
    let mut entries = read_config_file(&path)?;
    for new_entry in new_entries {
        let mut replaced = false;
        entries.retain_mut(|entry| {
            if config_entry_key_matches(entry, &new_entry) {
                if replaced {
                    false
                } else {
                    entry.value.clone_from(&new_entry.value);
                    replaced = true;
                    true
                }
            } else {
                true
            }
        });
        if !replaced {
            let insert_at = entries
                .iter()
                .rposition(|entry| {
                    entry.section == new_entry.section && entry.subsection == new_entry.subsection
                })
                .map(|idx| idx + 1)
                .unwrap_or(entries.len());
            entries.insert(insert_at, new_entry);
        }
    }
    write_config_entries(&path, &entries)?;
    Ok(())
}

pub(crate) fn set_worktree_config_value(repo: &GitRepo, name: &str, value: &str) -> Result<()> {
    let path = worktree_config_path_for_scope(repo)?;
    set_config_value_in_file(&path, name, value)
}

pub(crate) fn set_ordered_worktree_config_values(
    repo: &GitRepo,
    values: &[(String, String)],
) -> Result<()> {
    let path = worktree_config_path_for_scope(repo)?;
    let new_entries = values
        .iter()
        .map(|(name, value)| parse_config_entry(name, value))
        .collect::<Result<Vec<_>>>()?;
    let mut entries = read_config_file(&path)?;
    entries.retain(|entry| {
        !new_entries
            .iter()
            .any(|new_entry| config_entry_key_matches(entry, new_entry))
    });
    entries.extend(new_entries);
    write_config_entries(&path, &entries)?;
    Ok(())
}

pub(crate) fn append_worktree_config_value(repo: &GitRepo, name: &str, value: &str) -> Result<()> {
    let path = worktree_config_path_for_scope(repo)?;
    append_config_value_in_file(&path, name, value)
}

pub(crate) fn set_config_value_in_file(
    path: &std::path::Path,
    name: &str,
    value: &str,
) -> Result<()> {
    let new_entry = parse_config_entry(name, value)?;
    let mut entries = read_config_file(path)?;
    let mut replaced = false;
    entries.retain_mut(|entry| {
        if config_entry_key_matches(entry, &new_entry) {
            if replaced {
                false
            } else {
                entry.value.clone_from(&new_entry.value);
                replaced = true;
                true
            }
        } else {
            true
        }
    });
    if !replaced {
        let insert_at = entries
            .iter()
            .rposition(|entry| {
                entry.section == new_entry.section && entry.subsection == new_entry.subsection
            })
            .map(|idx| idx + 1)
            .unwrap_or(entries.len());
        entries.insert(insert_at, new_entry);
    }
    write_config_entries(path, &entries)?;
    Ok(())
}

pub(crate) fn add_config_value_in_file_if_missing(
    path: &std::path::Path,
    name: &str,
    value: &str,
) -> Result<()> {
    let new_entry = parse_config_entry(name, value)?;
    let mut entries = read_config_file(path)?;
    if entries
        .iter()
        .any(|entry| config_entry_key_matches(entry, &new_entry) && entry.value == new_entry.value)
    {
        return Ok(());
    }
    let insert_at = entries
        .iter()
        .rposition(|entry| {
            entry.section == new_entry.section && entry.subsection == new_entry.subsection
        })
        .map(|idx| idx + 1)
        .unwrap_or(entries.len());
    entries.insert(insert_at, new_entry);
    write_config_entries(path, &entries)?;
    Ok(())
}

pub(crate) fn append_config_value_in_file(
    path: &std::path::Path,
    name: &str,
    value: &str,
) -> Result<()> {
    let new_entry = parse_config_entry(name, value)?;
    let mut entries = read_config_file(path)?;
    let insert_at = entries
        .iter()
        .rposition(|entry| {
            entry.section == new_entry.section && entry.subsection == new_entry.subsection
        })
        .map(|idx| idx + 1)
        .unwrap_or(entries.len());
    entries.insert(insert_at, new_entry);
    write_config_entries(path, &entries)?;
    Ok(())
}

pub(crate) fn unset_config_value(repo: &GitRepo, name: &str) -> Result<()> {
    let path = local_config_path(repo)?;
    unset_config_value_in_file(&path, name)
}

pub(crate) fn unset_worktree_config_value(repo: &GitRepo, name: &str) -> Result<()> {
    let path = worktree_config_path_for_scope(repo)?;
    unset_config_value_in_file(&path, name)
}

pub(crate) fn unset_config_value_in_file(path: &std::path::Path, name: &str) -> Result<()> {
    let target = parse_config_entry(name, "")?;
    let mut entries = read_config_file(path)?;
    let before = entries.len();
    entries.retain(|entry| !config_entry_key_matches(entry, &target));
    if entries.len() == before {
        return Err(CliError::Exit(5));
    }
    write_config_entries(path, &entries)?;
    Ok(())
}

pub(crate) fn remove_config_value_from_file(
    path: &std::path::Path,
    name: &str,
    value: &str,
) -> Result<()> {
    let target = parse_config_entry(name, value)?;
    let mut entries = read_config_file(path)?;
    let before = entries.len();
    entries
        .retain(|entry| !(config_entry_key_matches(entry, &target) && entry.value == target.value));
    if entries.len() == before {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("repository '{}' is not registered", target.value),
        });
    }
    write_config_entries(path, &entries)?;
    Ok(())
}

pub(crate) fn parse_git_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" | "" => Some(false),
        _ => None,
    }
}

pub(crate) fn remote_names(repo: &GitRepo) -> io::Result<Vec<String>> {
    let mut names = BTreeSet::new();
    for entry in read_config_entries(repo)? {
        if entry.section == "remote" && !entry.subsection.is_empty() {
            names.insert(entry.subsection);
        }
    }
    let remotes_dir = read_common_git_dir(&repo.git_dir)
        .map_err(config_cli_error_to_io)?
        .join("remotes");
    match fs::read_dir(remotes_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_file()
                    && let Some(name) = entry.file_name().to_str()
                {
                    names.insert(name.to_owned());
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(names.into_iter().collect())
}

pub(crate) fn remote_url(repo: &GitRepo, name: &str) -> Result<String> {
    ensure_remote_exists(repo, name)?;
    if modern_remote_exists(repo, name)? {
        return read_config_entries(repo)?
            .into_iter()
            .find(|entry| {
                entry.section == "remote" && entry.subsection == name && entry.key == "url"
            })
            .map(|entry| entry.value)
            .ok_or_else(|| CliError::Fatal {
                code: 2,
                message: format!("No URL configured for remote '{name}'"),
            });
    }
    legacy_remote_config(repo, name)?
        .map(|remote| remote.url)
        .ok_or_else(|| CliError::Fatal {
            code: 2,
            message: format!("No URL configured for remote '{name}'"),
        })
}

pub(crate) fn ensure_remote_exists(repo: &GitRepo, name: &str) -> Result<()> {
    if remote_exists(repo, name)? {
        Ok(())
    } else {
        Err(CliError::Stderr {
            code: 2,
            text: format!("error: No such remote: '{name}'\n"),
        })
    }
}

pub(crate) fn remote_exists(repo: &GitRepo, name: &str) -> io::Result<bool> {
    Ok(modern_remote_exists(repo, name)? || legacy_remote_config(repo, name)?.is_some())
}

pub(crate) fn modern_remote_exists(repo: &GitRepo, name: &str) -> io::Result<bool> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .any(|entry| entry.section == "remote" && entry.subsection == name))
}

pub(crate) fn legacy_remote_config(
    repo: &GitRepo,
    name: &str,
) -> io::Result<Option<LegacyRemoteConfig>> {
    if !legacy_remote_name_is_safe(name) {
        return Ok(None);
    }
    let path = read_common_git_dir(&repo.git_dir)
        .map_err(config_cli_error_to_io)?
        .join("remotes")
        .join(name);
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut url = None;
    let mut fetch_refspecs = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("URL:") {
            if url.is_none() {
                url = Some(value.trim().to_owned());
            }
        } else if let Some(value) = line.strip_prefix("Pull:") {
            fetch_refspecs.push(value.trim().to_owned());
        }
    }
    Ok(url.map(|url| LegacyRemoteConfig {
        url,
        fetch_refspecs,
    }))
}

fn legacy_remote_name_is_safe(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains(['/', '\\', '\n', '\r', '\0'])
        && name != "."
        && name != ".."
}

fn config_cli_error_to_io(error: CliError) -> io::Error {
    io::Error::other(format!("{error:?}"))
}

pub(crate) fn validate_remote_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('-')
        || name.contains(['\n', '\r', '\0'])
        || name.contains('/')
        || name.contains("..")
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{name}' is not a valid remote name"),
        });
    }
    Ok(())
}

pub(crate) fn remove_remote_config_entries(entries: &mut Vec<ConfigEntry>, name: &str) {
    let branches = entries
        .iter()
        .filter(|entry| entry.section == "branch" && entry.key == "remote" && entry.value == name)
        .map(|entry| entry.subsection.clone())
        .collect::<HashSet<_>>();
    entries.retain(|entry| {
        !(entry.section == "remote" && entry.subsection == name
            || entry.section == "branch" && branches.contains(&entry.subsection))
    });
}

pub(crate) fn rename_branch_config(repo: &GitRepo, old_name: &str, new_name: &str) -> Result<()> {
    let path = local_config_path(repo)?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let old_header = format!("[branch \"{old_name}\"]");
    let new_header = format!("[branch \"{new_name}\"]");
    let renamed = contents.replace(&old_header, &new_header);
    if renamed != contents {
        fs::write(path, renamed)?;
    }
    Ok(())
}

pub(crate) fn copy_branch_config(repo: &GitRepo, old_name: &str, new_name: &str) -> Result<()> {
    let path = local_config_path(repo)?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let lines = config_lines_with_endings(&contents);
    let source_header = format!("[branch \"{old_name}\"]");
    let destination_header = format!("[branch \"{new_name}\"]");
    let lines = remove_config_sections(lines, &destination_header);
    let copied = copy_config_sections(lines, &source_header, &destination_header);
    if copied.changed {
        fs::write(path, copied.contents)?;
    }
    Ok(())
}

struct CopiedConfigSections {
    contents: String,
    changed: bool,
}

fn copy_config_sections(
    lines: Vec<String>,
    source_header: &str,
    destination_header: &str,
) -> CopiedConfigSections {
    let mut out = String::new();
    let mut changed = false;
    let mut index = 0usize;
    while index < lines.len() {
        if !config_line_has_header(&lines[index], source_header) {
            out.push_str(&lines[index]);
            index += 1;
            continue;
        }
        let next_header = next_config_section_header(&lines, index + 1).unwrap_or(lines.len());
        for line in &lines[index..next_header] {
            out.push_str(line);
        }
        for line in &lines[index..next_header] {
            out.push_str(&line.replacen(source_header, destination_header, 1));
        }
        changed = true;
        index = next_header;
    }
    CopiedConfigSections {
        contents: out,
        changed,
    }
}

fn remove_config_sections(lines: Vec<String>, header: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        if !config_line_has_header(&lines[index], header) {
            out.push(lines[index].clone());
            index += 1;
            continue;
        }
        index = next_config_section_header(&lines, index + 1).unwrap_or(lines.len());
    }
    out
}

fn config_lines_with_endings(contents: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (index, ch) in contents.char_indices() {
        if ch == '\n' {
            lines.push(contents[start..=index].to_owned());
            start = index + 1;
        }
    }
    if start < contents.len() {
        lines.push(contents[start..].to_owned());
    }
    lines
}

fn next_config_section_header(lines: &[String], start: usize) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| config_line_is_section_header(line).then_some(index))
}

fn config_line_has_header(line: &str, header: &str) -> bool {
    line.trim() == header
}

fn config_line_is_section_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

pub(crate) fn remove_branch_upstream_config(repo: &GitRepo, branch: &str) -> Result<()> {
    let mut entries = read_common_config_entries(repo)?;
    let before = entries.len();
    entries.retain(|entry| {
        !(entry.section == "branch"
            && entry.subsection == branch
            && (entry.key == "remote" || entry.key == "merge"))
    });
    if entries.len() != before {
        write_common_config_entries(repo, &entries)?;
    }
    Ok(())
}

pub(crate) fn parse_config_entry(name: &str, value: &str) -> Result<ConfigEntry> {
    let (section, subsection, key) = parse_config_name(name).map_err(|_| CliError::Fatal {
        code: 1,
        message: format!("invalid config key: {name}"),
    })?;
    if value.contains('\0') {
        return Err(CliError::Fatal {
            code: 1,
            message: "config value cannot contain control separators".into(),
        });
    }
    Ok(ConfigEntry {
        section,
        raw_section: name.split('.').next().unwrap_or_default().to_owned(),
        subsection,
        raw_key: name.rsplit('.').next().unwrap_or(&key).to_owned(),
        key,
        value: value.to_owned(),
        comment: None,
        implicit_bool: false,
        scope: ConfigScope::Local,
        origin: String::new(),
        line: None,
    })
}

pub(crate) fn parse_config_section_name(name: &str) -> Result<(String, String)> {
    let (section, subsection, _key) = parse_config_name(&format!("{name}.zmin-section-sentinel"))
        .map_err(|_| CliError::Fatal {
        code: 1,
        message: format!("invalid config section name: {name}"),
    })?;
    if !section
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("invalid config section name: {name}"),
        });
    }
    Ok((section, subsection))
}

pub(crate) fn parse_config_name(name: &str) -> io::Result<(String, String, String)> {
    let parts = name.split('.').collect::<Vec<_>>();
    let section_valid = parts.first().is_some_and(|section| {
        !section.is_empty()
            && section
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    let key_valid = parts.last().is_some_and(|key| {
        key.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    if parts.len() < 2
        || !section_valid
        || !key_valid
        || parts.iter().any(|part| part.contains(['\n', '\r', '\0']))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid config key",
        ));
    }
    let section = parts[0].to_ascii_lowercase();
    let key = parts[parts.len() - 1].to_ascii_lowercase();
    let subsection = if parts.len() > 2 {
        let middle = &parts[1..parts.len() - 1];
        if middle.iter().all(|part| part.is_empty()) {
            String::new()
        } else if middle.first() == Some(&"") {
            let suffix = middle
                .iter()
                .skip_while(|part| part.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(".");
            format!(".{suffix}")
        } else {
            middle.join(".")
        }
    } else {
        String::new()
    };
    Ok((section, subsection, key))
}

pub(crate) fn config_entry_key_matches(left: &ConfigEntry, right: &ConfigEntry) -> bool {
    left.section == right.section && left.subsection == right.subsection && left.key == right.key
}

pub(crate) fn write_config_entries(
    path: &std::path::Path,
    entries: &[ConfigEntry],
) -> io::Result<()> {
    reject_locked_config(path)?;
    let mut out = String::new();
    let mut current = None::<(String, String)>;
    for entry in entries {
        let section = (entry.section.clone(), entry.subsection.clone());
        if current != Some(section.clone()) {
            if entry.subsection.is_empty() {
                out.push_str(&format!("[{}]\n", entry.raw_section));
            } else {
                out.push_str(&format!(
                    "[{} \"{}\"]\n",
                    entry.raw_section, entry.subsection
                ));
            }
            current = Some(section);
        }
        if entry.implicit_bool {
            out.push_str(&format!("\t{}", entry.raw_key));
        } else {
            out.push_str(&format!(
                "\t{} = {}",
                entry.raw_key,
                encode_config_value(&entry.value)
            ));
        }
        if let Some(comment) = entry.comment.as_deref() {
            out.push_str(comment);
        }
        out.push('\n');
    }
    fs::write(path, out)
}

fn reject_locked_config(path: &Path) -> io::Result<()> {
    let lock_path = PathBuf::from(format!("{}.lock", path.display()));
    if lock_path.exists() {
        return Err(io::Error::other(format!(
            "could not lock config file {}",
            display_config_path_for_error(path)
        )));
    }
    Ok(())
}

fn display_config_path_for_error(path: &Path) -> String {
    if path.file_name().and_then(|name| name.to_str()) == Some("config")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some(".git")
    {
        ".git/config".to_owned()
    } else {
        path.display().to_string()
    }
}

pub(crate) fn encode_config_value(value: &str) -> String {
    let quoted = value.chars().next().is_some_and(char::is_whitespace)
        || value.chars().last().is_some_and(char::is_whitespace)
        || value.contains(['#', ';']);
    let mut encoded = String::with_capacity(value.len() + usize::from(quoted) * 2);
    if quoted {
        encoded.push('"');
    }
    for ch in value.chars() {
        match ch {
            '\\' => encoded.push_str("\\\\"),
            '"' => encoded.push_str("\\\""),
            '\n' => encoded.push_str("\\n"),
            '\t' => encoded.push_str("\\t"),
            '\u{08}' => encoded.push_str("\\b"),
            _ => encoded.push(ch),
        }
    }
    if quoted {
        encoded.push('"');
    }
    encoded
}

pub(crate) fn parse_config_section(raw: &str) -> (String, String) {
    let raw = raw.trim();
    if let Some((section, rest)) = raw.split_once(' ') {
        (
            section.trim().to_owned(),
            rest.trim().trim_matches('"').to_owned(),
        )
    } else if let Some((section, subsection)) = raw.split_once('.') {
        (section.to_owned(), subsection.to_ascii_lowercase())
    } else {
        (raw.to_owned(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigScope, parse_config_name, read_config_file_raw};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn read_config_file_raw_refreshes_when_file_changes() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("config");
        fs::write(&path, "[core]\n\tpager = less\n").expect("write initial config");

        let first = read_config_file_raw(&path, ConfigScope::Local, "file:test".to_owned())
            .expect("read initial config");
        assert_eq!(first[0].value, "less");

        fs::write(&path, "[core]\n\tpager = cat\n").expect("rewrite config");

        let second = read_config_file_raw(&path, ConfigScope::Local, "file:test".to_owned())
            .expect("read updated config");
        assert_eq!(second[0].value, "cat");
    }

    #[test]
    fn parse_config_name_allows_empty_alias_subsection_like_stock_git() {
        assert_eq!(
            parse_config_name("alias..something").expect("empty subsection"),
            ("alias".to_owned(), String::new(), "something".to_owned())
        );
        assert_eq!(
            parse_config_name("alias..something.command")
                .expect("leading dot subsection via shell-quoted CLI"),
            (
                "alias".to_owned(),
                ".something".to_owned(),
                "command".to_owned()
            )
        );
        assert_eq!(
            parse_config_name("alias...something.command").expect("leading dot subsection"),
            (
                "alias".to_owned(),
                ".something".to_owned(),
                "command".to_owned()
            )
        );
    }
}
