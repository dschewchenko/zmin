use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use zmin_git_core::{GitHashAlgorithm, RefStore, RefTarget};

use crate::runtime::{git_path_config_output, normalize_windows_input_path};

use super::{CliError, GitRepo, Result, bytes_eq, bytes_starts_with, wildcard_match_pathspec};

static GLOBAL_CONFIG_ENTRIES: OnceLock<Vec<ConfigEntry>> = OnceLock::new();

pub(crate) fn set_global_config_entries(entries: Vec<ConfigEntry>) {
    let _ = GLOBAL_CONFIG_ENTRIES.set(entries);
}

pub(crate) fn global_command_config_value(section: &str, key: &str) -> Option<String> {
    GLOBAL_CONFIG_ENTRIES.get().and_then(|entries| {
        entries
            .iter()
            .rev()
            .find(|entry| {
                entry.section == section && entry.subsection.is_empty() && entry.key == key
            })
            .map(|entry| entry.value.clone())
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigEntry {
    pub(crate) section: String,
    pub(crate) subsection: String,
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) implicit_bool: bool,
    pub(crate) scope: ConfigScope,
    pub(crate) origin: String,
}

#[derive(Debug)]
struct ConfigIncludeContext {
    git_dir: PathBuf,
    work_tree: PathBuf,
    branch: Option<String>,
    remote_urls: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

pub(crate) fn parse_global_config_entry(raw: &str) -> Result<ConfigEntry> {
    let (name, value, implicit_bool) = raw
        .split_once('=')
        .map(|(name, value)| (name, value, false))
        .unwrap_or((raw, "", true));
    let (section, subsection, key) = parse_config_name(name).map_err(|_| CliError::Stderr {
        code: 1,
        text: format!("error: key does not contain a section: {name}\n"),
    })?;
    if value.contains(['\n', '\r', '\0']) {
        return Err(CliError::Stderr {
            code: 1,
            text: format!("error: bogus config parameter: {raw}\n"),
        });
    }
    Ok(ConfigEntry {
        section,
        subsection,
        key,
        value: value.to_owned(),
        implicit_bool,
        scope: ConfigScope::Command,
        origin: "command line:".to_owned(),
    })
}

pub(crate) fn parse_global_config_env_entry(raw: &str) -> Result<ConfigEntry> {
    let Some((name, env_name)) = raw.split_once('=') else {
        return Err(CliError::Stderr {
            code: 129,
            text: "no config key given for --config-env\n".into(),
        });
    };
    let value = std::env::var(env_name).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("missing environment variable '{env_name}' for configuration '{name}'"),
    })?;
    let mut entry = parse_global_config_entry(&format!("{name}={value}"))?;
    entry.implicit_bool = false;
    Ok(entry)
}

pub(crate) fn read_config_value(repo: &GitRepo, name: &str) -> io::Result<Option<String>> {
    Ok(read_config_entry(repo, name)?.map(|entry| entry.value))
}

pub(crate) fn read_config_entry(repo: &GitRepo, name: &str) -> io::Result<Option<ConfigEntry>> {
    let (section, subsection, key) = parse_config_name(name)?;
    read_config_section_entry(repo, &section, &subsection, &key)
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
        )?);
    }
    entries.extend(read_local_config_entries_with_includes(repo)?);
    if let Some(global_entries) = GLOBAL_CONFIG_ENTRIES.get() {
        entries.extend(expand_config_entries_with_includes(
            global_entries.clone(),
            ConfigScope::Command,
            &std::env::current_dir()?,
            ".",
            Some(&include_context),
            0,
            false,
        )?);
    }
    Ok(entries)
}

fn global_config_paths() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("GIT_CONFIG_GLOBAL") {
        return vec![normalize_windows_input_path(PathBuf::from(path))];
    }
    let mut paths = Vec::new();
    for home in global_config_homes() {
        paths.push(home.join(".gitconfig"));
        paths.push(xdg_config_home(&home).join("git/config"));
    }
    paths
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
    if std::env::var_os("GIT_CONFIG_NOSYSTEM").is_some() {
        return Vec::new();
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

pub(crate) fn read_local_config_entries(repo: &GitRepo) -> io::Result<Vec<ConfigEntry>> {
    let mut entries = read_common_config_entries(repo)?;
    if worktree_config_enabled(&entries) {
        entries.extend(read_worktree_config_entries(repo)?);
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
    )?;
    if worktree_config_enabled(&entries) {
        entries.extend(read_config_file_with_source(
            &worktree_config_path(repo),
            ConfigScope::Worktree,
            "file:.git/config.worktree".to_owned(),
            Some(&include_context),
            0,
            false,
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

fn read_config_file_with_source(
    path: &std::path::Path,
    scope: ConfigScope,
    origin: String,
    include_context: Option<&ConfigIncludeContext>,
    include_depth: usize,
    hasconfig_included: bool,
) -> io::Result<Vec<ConfigEntry>> {
    if include_depth > 10 {
        return Ok(Vec::new());
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
    include_context: Option<&ConfigIncludeContext>,
    include_depth: usize,
    hasconfig_included: bool,
) -> io::Result<Vec<ConfigEntry>> {
    if include_depth > 10 {
        return Ok(Vec::new());
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
            )?);
        }
    }
    Ok(with_includes)
}

impl ConfigIncludeContext {
    fn new(repo: &GitRepo) -> io::Result<Self> {
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
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
        .any(|url| wildcard_match_pathspec(pattern, url, false, true))
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
    for entry in read_config_file_raw(path, scope, String::new())? {
        if config_entry_is_remote_url(&entry) {
            urls.push(entry.value);
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
    let candidates = [git_dir.as_str(), work_tree.as_str()];
    candidates
        .into_iter()
        .any(|candidate| gitdir_pattern_matches_candidate(&pattern, candidate, icase))
}

fn normalize_gitdir_include_pattern(pattern: &str, base_dir: &Path) -> Option<String> {
    let raw = if let Some(rest) = pattern.strip_prefix("~/") {
        config_home_dir()?.join(rest)
    } else {
        let path = Path::new(pattern);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            base_dir.join(path)
        }
    };
    Some(path_for_config_match(&raw))
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
        return wildcard_match_pathspec(pattern, candidate, icase, true);
    }
    if bytes_eq(candidate.as_bytes(), pattern.as_bytes(), icase) {
        return true;
    }
    let mut prefix = pattern.as_bytes().to_vec();
    prefix.push(b'/');
    bytes_starts_with(candidate.as_bytes(), &prefix, icase)
}

fn read_config_file_raw(
    path: &std::path::Path,
    scope: ConfigScope,
    origin: String,
) -> io::Result<Vec<ConfigEntry>> {
    let Ok(content) = fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let mut current_section = None::<(String, String)>;
    let mut entries = Vec::new();
    let source = config_error_source(path, &origin);
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let mut trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(after_open) = trimmed.strip_prefix('[') {
            let Some((section, rest)) = after_open.split_once(']') else {
                return Err(bad_config_line_error(line_no, &source));
            };
            current_section = Some(parse_config_section(section));
            trimmed = rest.trim();
            if trimmed.is_empty() {
                continue;
            }
        }
        let Some((section, subsection)) = current_section.as_ref() else {
            return Err(bad_config_line_error(line_no, &source));
        };
        let (key, value, implicit_bool) = trimmed
            .split_once('=')
            .map(|(key, value)| (key.trim(), value.trim(), false))
            .unwrap_or((trimmed, "", true));
        if key.is_empty() {
            return Err(bad_config_line_error(line_no, &source));
        }
        entries.push(ConfigEntry {
            section: section.to_ascii_lowercase(),
            subsection: subsection.clone(),
            key: key.to_ascii_lowercase(),
            value: decode_config_value(value),
            implicit_bool,
            scope,
            origin: origin.clone(),
        });
    }
    Ok(entries)
}

fn config_error_source(path: &Path, origin: &str) -> String {
    origin
        .strip_prefix("file:")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| path.to_str().unwrap_or(""))
        .to_owned()
}

fn bad_config_line_error(line: usize, source: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("bad config line {line} in file {source}"),
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
    Ok(names.into_iter().collect())
}

pub(crate) fn remote_url(repo: &GitRepo, name: &str) -> Result<String> {
    ensure_remote_exists(repo, name)?;
    read_config_section_value(repo, "remote", name, "url")?.ok_or_else(|| CliError::Fatal {
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
    Ok(read_config_entries(repo)?
        .into_iter()
        .any(|entry| entry.section == "remote" && entry.subsection == name))
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
    if value.contains(['\n', '\r', '\0']) {
        return Err(CliError::Fatal {
            code: 1,
            message: "config value cannot contain control separators".into(),
        });
    }
    Ok(ConfigEntry {
        section,
        subsection,
        key,
        value: value.to_owned(),
        implicit_bool: false,
        scope: ConfigScope::Local,
        origin: String::new(),
    })
}

pub(crate) fn parse_config_name(name: &str) -> io::Result<(String, String, String)> {
    let parts = name.split('.').collect::<Vec<_>>();
    if parts.len() < 2
        || parts
            .iter()
            .any(|part| part.is_empty() || part.contains(['\n', '\r', '\0']))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid config key",
        ));
    }
    let section = parts[0].to_ascii_lowercase();
    let key = parts[parts.len() - 1].to_ascii_lowercase();
    let subsection = if parts.len() > 2 {
        parts[1..parts.len() - 1].join(".")
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
    let mut current = None::<(&str, &str)>;
    for entry in entries {
        let section = (entry.section.as_str(), entry.subsection.as_str());
        if current != Some(section) {
            if !out.is_empty() {
                out.push('\n');
            }
            if entry.subsection.is_empty() {
                out.push_str(&format!("[{}]\n", entry.section));
            } else {
                out.push_str(&format!("[{} \"{}\"]\n", entry.section, entry.subsection));
            }
            current = Some(section);
        }
        if entry.implicit_bool {
            out.push_str(&format!("\t{}\n", entry.key));
        } else {
            out.push_str(&format!(
                "\t{} = {}\n",
                entry.key,
                encode_config_value(&entry.value)
            ));
        }
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

fn encode_config_value(value: &str) -> String {
    let quoted = value.chars().next().is_some_and(char::is_whitespace)
        || value.chars().last().is_some_and(char::is_whitespace);
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
    } else {
        (raw.to_owned(), String::new())
    }
}
