//! Typed Git LFS runtime configuration assembly.
//!
//! This module is deliberately independent from process environment and from
//! transport I/O.  The command layer supplies the already loaded Git config,
//! the current branch/remote context, and the value of
//! `GIT_LFS_SKIP_SMUDGE`.  The result can then be handed to endpoint discovery
//! and the filter/store layers without re-reading configuration or guessing at
//! a remote URL.
//!
//! The relative `lfs.storage` rule follows the current Git LFS configuration
//! documentation: a non-absolute value is resolved inside the Git directory
//! (normally `.git`), while an absolute value is used as supplied.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_TYPE_DISK,
    GetFileInformationByHandle, GetFileType,
};

use super::{
    ConfigEntry, LfsEndpointError, LfsEndpointInputs, LfsHttpEnvironmentSnapshot, LfsHttpPolicy,
    LfsHttpPolicyError, LfsHttpUrl, LfsRemoteConfig, LfsTransferConcurrency, LfsUrlScope,
    SafeLfsConfig, parse_http_url, parse_safe_lfsconfig, parse_skip_smudge,
};

const DEFAULT_LFS_STORAGE_NAME: &str = "lfs";
const LFS_CONFIG_NAME: &str = ".lfsconfig";
const MAX_LFS_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_FETCH_PATTERN_BYTES: usize = 4 * 1024;
const MAX_FETCH_PATTERNS: usize = 256;
const MAX_FETCH_VALUE_BYTES: usize = 64 * 1024;
const MAX_FETCH_PATH_BYTES: usize = 4 * 1024;
// SystemCase uses one lower-case scalar for each input scalar, as Go's
// unicode.ToLower does.  A scalar can occupy at most four UTF-8 bytes, so this
// also bounds the two rolling rows on Darwin and Windows.
const MAX_FETCH_MATCH_TEXT_CELLS: usize = MAX_FETCH_PATH_BYTES * 4 + 1;
// A filter is rejected at construction when its worst-case match would exceed
// this amount of rolling-DP work.  This is deliberately below ten million
// state/cell visits per path, independent of how patterns are distributed
// between include and exclude.
const MAX_FETCH_MATCH_WORK: usize = 8_000_000;
// Each POSIX class is a fixed regular expression evaluated against at most
// one four-byte UTF-8 scalar.  Charging this many units per active cell is a
// conservative bound for dispatch and the fixed Unicode-property automaton.
const LFS_POSIX_CLASS_MATCH_WORK: usize = 64;
const MAX_REMOTE_NAME_BYTES: usize = 256;
const MAX_FETCH_HEAD_BYTES: usize = 1024 * 1024;
const MAX_URL_REWRITE_RULES: usize = 256;
const MAX_URL_REWRITE_BYTES: usize = 64 * 1024;

/// URL aliases are normally expanded by the Git config layer before this
/// module is called.  A typed callback is also accepted for callers that keep
/// that expansion in a separate runtime component.
pub(crate) trait LfsRemoteUrlRewriter {
    fn rewrite(&self, url: &str, push: bool) -> Result<String, LfsConfigError>;
}

/// Git's built-in URL alias policy.  A remote URL is rewritten by the
/// longest matching `insteadOf` prefix; push URLs first consider the
/// longest `pushInsteadOf` prefix and then fall back to `insteadOf`.
#[derive(Clone, Debug, Default)]
struct GitUrlInsteadOfRewriter {
    fetch: Vec<UrlRewriteRule>,
    push: Vec<UrlRewriteRule>,
}

#[derive(Clone, Debug)]
struct UrlRewriteRule {
    base: String,
    prefix: String,
}

impl GitUrlInsteadOfRewriter {
    fn from_entries(entries: &[ConfigEntry]) -> Result<Self, LfsConfigError> {
        let mut rewriter = Self::default();
        for entry in entries {
            if entry.section != "url"
                || !matches!(entry.key.as_str(), "insteadof" | "pushinsteadof")
            {
                continue;
            }
            validate_url_rewrite_text(&entry.subsection)?;
            validate_url_rewrite_text(&entry.value)?;
            if entry.subsection.is_empty() || entry.value.is_empty() {
                return Err(LfsConfigError::UrlRewriteFailed);
            }
            let rules = if entry.key == "pushinsteadof" {
                &mut rewriter.push
            } else {
                &mut rewriter.fetch
            };
            if rules.len() >= MAX_URL_REWRITE_RULES {
                return Err(LfsConfigError::UrlRewriteFailed);
            }
            rules.push(UrlRewriteRule {
                base: entry.subsection.clone(),
                prefix: entry.value.clone(),
            });
        }
        Ok(rewriter)
    }

    fn longest<'a>(rules: &'a [UrlRewriteRule], url: &str) -> Option<&'a UrlRewriteRule> {
        rules
            .iter()
            .filter(|rule| url.starts_with(&rule.prefix))
            .max_by_key(|rule| rule.prefix.len())
    }

    fn apply(rules: &[UrlRewriteRule], url: &str) -> Result<Option<String>, LfsConfigError> {
        let Some(rule) = Self::longest(rules, url) else {
            return Ok(None);
        };
        let suffix = &url[rule.prefix.len()..];
        let total = rule
            .base
            .len()
            .checked_add(suffix.len())
            .ok_or(LfsConfigError::UrlRewriteFailed)?;
        if total > MAX_URL_REWRITE_BYTES {
            return Err(LfsConfigError::UrlRewriteFailed);
        }
        Ok(Some(format!("{}{}", rule.base, suffix)))
    }
}

impl LfsRemoteUrlRewriter for GitUrlInsteadOfRewriter {
    fn rewrite(&self, url: &str, push: bool) -> Result<String, LfsConfigError> {
        validate_url_rewrite_text(url)?;
        if push && let Some(rewritten) = Self::apply(&self.push, url)? {
            return Ok(rewritten);
        }
        Ok(Self::apply(&self.fetch, url)?.unwrap_or_else(|| url.to_owned()))
    }
}

fn validate_url_rewrite_text(value: &str) -> Result<(), LfsConfigError> {
    if value.is_empty()
        || value.len() > MAX_URL_REWRITE_BYTES
        || value.bytes().any(|byte| byte == 0)
        || value.chars().any(char::is_control)
    {
        return Err(LfsConfigError::UrlRewriteFailed);
    }
    Ok(())
}

/// A validated, canonical worktree root trusted by the repository owner.
///
/// Construction rejects relative paths and non-directories, stores the
/// canonical path, and on Windows opens every addressable root/parent
/// component with `OPEN_REPARSE_POINT` to reject reparse traversal.  Windows
/// does not provide a documented `openat` equivalent here: the caller owns a
/// trusted repository ancestor and must prevent concurrent replacement of
/// that ancestor namespace until [`LfsRuntimeConfig::load`] returns.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TrustedWorktreeRoot {
    canonical: PathBuf,
}

impl fmt::Debug for TrustedWorktreeRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TrustedWorktreeRoot(<redacted>)")
    }
}

impl TrustedWorktreeRoot {
    pub(crate) fn new(path: &Path) -> Result<Self, LfsConfigError> {
        validate_base_path(path)?;
        #[cfg(windows)]
        validate_windows_trusted_worktree_components(path)?;
        let canonical =
            fs::canonicalize(path).map_err(|error| LfsConfigError::LfsConfigIo(error.kind()))?;
        validate_base_path(&canonical)?;
        #[cfg(windows)]
        validate_windows_trusted_worktree_components(&canonical)?;
        let metadata =
            fs::metadata(&canonical).map_err(|error| LfsConfigError::LfsConfigIo(error.kind()))?;
        if !metadata.is_dir() {
            return Err(LfsConfigError::UntrustedWorktreeRoot);
        }
        Ok(Self { canonical })
    }

    fn as_path(&self) -> &Path {
        &self.canonical
    }
}

/// The three sources Git LFS currently considers for `.lfsconfig`.
///
/// A non-bare checkout supplies a [`TrustedWorktreeRoot`].  The index and HEAD
/// are already-read blobs, so callers can hand us the exact object bytes
/// without allowing this module to reread a mutable repository path.  For a
/// bare repository, `worktree` is `None` and the index is deliberately
/// skipped; Git LFS reads HEAD only in that case.
#[derive(Clone, Default)]
pub(crate) struct LfsConfigSources<'a> {
    worktree: Option<TrustedWorktreeRoot>,
    pub(crate) index_blob: Option<&'a [u8]>,
    pub(crate) head_blob: Option<&'a [u8]>,
}

impl<'a> LfsConfigSources<'a> {
    pub(crate) fn new(
        worktree: Option<TrustedWorktreeRoot>,
        index_blob: Option<&'a [u8]>,
        head_blob: Option<&'a [u8]>,
    ) -> Self {
        Self {
            worktree,
            index_blob,
            head_blob,
        }
    }

    fn load(&self) -> Result<Option<LfsConfigSnapshot>, LfsConfigError> {
        if let Some(worktree) = &self.worktree {
            let path = worktree.as_path().join(LFS_CONFIG_NAME);
            if let Some(snapshot) = read_lfsconfig_snapshot(&path)? {
                return Ok(Some(snapshot));
            }
            if let Some(blob) = self.index_blob {
                return Ok(Some(LfsConfigSnapshot::from_blob(
                    LfsConfigSourceKind::Index,
                    blob,
                )?));
            }
        }
        self.head_blob
            .map(|blob| LfsConfigSnapshot::from_blob(LfsConfigSourceKind::Head, blob))
            .transpose()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfsConfigSourceKind {
    Worktree,
    Index,
    Head,
}

struct LfsConfigSnapshot {
    source: LfsConfigSourceKind,
    bytes: Vec<u8>,
}

impl LfsConfigSnapshot {
    fn from_blob(source: LfsConfigSourceKind, bytes: &[u8]) -> Result<Self, LfsConfigError> {
        if bytes.len() as u64 > MAX_LFS_CONFIG_BYTES {
            return Err(LfsConfigError::LfsConfigTooLarge);
        }
        validate_lfsconfig_bytes(bytes)?;
        Ok(Self {
            source,
            bytes: bytes.to_owned(),
        })
    }

    fn parse(self) -> Result<SafeLfsConfig, LfsConfigError> {
        let _source = self.source;
        parse_lfsconfig_bytes(&self.bytes)
    }
}

/// All context needed to assemble one immutable LFS runtime configuration.
///
/// `entries` must already be ordered in Git precedence order (lowest first,
/// highest last), as returned by the existing config reader.  The loader never
/// reads process environment; every environment value is caller-provided.
#[derive(Clone)]
pub(crate) struct LfsRuntimeConfigInput<'a> {
    pub(crate) git_dir: &'a Path,
    /// Common Git directory used for the implicit `lfs` storage path and as
    /// the anchor for an explicit relative `lfs.storage`. Absolute configured
    /// storage remains absolute after validation.
    pub(crate) default_storage_git_dir: &'a Path,
    pub(crate) lfsconfig: LfsConfigSources<'a>,
    pub(crate) entries: &'a [ConfigEntry],
    pub(crate) branch: Option<&'a str>,
    pub(crate) requested_remote: Option<&'a str>,
    pub(crate) skip_smudge: Option<&'a str>,
    pub(crate) skip_download_errors: Option<&'a str>,
    pub(crate) http_environment: LfsHttpEnvironmentSnapshot,
    pub(crate) fetch_head: Option<&'a str>,
    pub(crate) url_rewriter: Option<&'a dyn LfsRemoteUrlRewriter>,
}

/// Fully assembled configuration consumed by LFS endpoint/filter code.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsRuntimeConfig {
    storage: PathBuf,
    concurrent_transfers: LfsTransferConcurrency,
    endpoint_inputs: LfsEndpointInputs,
    fetch_filter: LfsFetchFilter,
    credential_use_http_path: LfsCredentialUseHttpPathPolicy,
    http_policy: Arc<LfsHttpPolicy>,
    skip_smudge: bool,
    remote_policy: LfsRemoteSelectionPolicy,
    allow_incomplete_push: bool,
    locks_verify: Option<bool>,
    skip_download_errors: bool,
    access_rules: LfsAccessRules,
    locks_rules: LfsLocksRules,
}

/// Effective Git LFS credential path policy.  URL-scoped rules use Git
/// LFS's URLConfig precedence: exact host beats wildcard host, then the
/// longest matching path, then an exact username, with the last value winning
/// for an otherwise equal config key.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct LfsCredentialUseHttpPathPolicy {
    global: bool,
    scoped: Vec<LfsCredentialUseHttpPathRule>,
}

impl fmt::Debug for LfsCredentialUseHttpPathPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsCredentialUseHttpPathPolicy")
            .field("global", &self.global)
            .field("scoped_count", &self.scoped.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
struct LfsCredentialUseHttpPathRule {
    scope: LfsUrlScope,
    value: bool,
    order: usize,
}

impl LfsCredentialUseHttpPathPolicy {
    pub(crate) fn for_endpoint(&self, endpoint: &LfsHttpUrl) -> Result<bool, LfsEndpointError> {
        let endpoint =
            LfsUrlScope::parse(endpoint.as_str()).ok_or(LfsEndpointError::MalformedUrl)?;
        Ok(self
            .scoped
            .iter()
            .filter_map(|rule| {
                rule.scope
                    .match_score(&endpoint)
                    .map(|(host_score, path_score, user_score)| {
                        (host_score, path_score, user_score, rule.order, rule.value)
                    })
            })
            .max_by_key(|(host_score, path_score, user_score, order, _value)| {
                (*host_score, *path_score, *user_score, *order)
            })
            .map_or(self.global, |(_, _, _, _, value)| value))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LfsAccessMode {
    Basic,
    None,
    #[default]
    Unspecified,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct LfsAccessRules {
    ordinary: Vec<LfsAccessRule>,
    safe: Vec<LfsAccessRule>,
}

#[derive(Clone, PartialEq, Eq)]
struct LfsAccessRule {
    scope: LfsHttpUrl,
    mode: LfsAccessMode,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct LfsLocksRules {
    ordinary: Vec<LfsLocksRule>,
}

#[derive(Clone, PartialEq, Eq)]
struct LfsLocksRule {
    scope: LfsHttpUrl,
    verify: bool,
}

impl fmt::Debug for LfsLocksRules {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsLocksRules")
            .field("ordinary_count", &self.ordinary.len())
            .finish()
    }
}

impl fmt::Debug for LfsAccessRules {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsAccessRules")
            .field("ordinary_count", &self.ordinary.len())
            .field("safe_count", &self.safe.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LfsRemoteSelectionPolicy {
    autodetect: bool,
    search_all: bool,
}

impl LfsRemoteSelectionPolicy {
    pub(crate) fn autodetect(self) -> bool {
        self.autodetect
    }

    pub(crate) fn search_all(self) -> bool {
        self.search_all
    }
}

impl fmt::Debug for LfsRuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsRuntimeConfig")
            .field("storage", &"<redacted>")
            .field("concurrent_transfers", &self.concurrent_transfers)
            .field("endpoint_inputs", &self.endpoint_inputs)
            .field("fetch_filter", &self.fetch_filter)
            .field("credential_use_http_path", &self.credential_use_http_path)
            .field("http_policy", &self.http_policy)
            .field("skip_smudge", &self.skip_smudge)
            .field("remote_policy", &self.remote_policy)
            .field("allow_incomplete_push", &self.allow_incomplete_push)
            .field("locks_verify", &self.locks_verify)
            .field("skip_download_errors", &self.skip_download_errors)
            .field("access_rules", &self.access_rules)
            .field("locks_rules", &self.locks_rules)
            .finish()
    }
}

impl LfsRuntimeConfig {
    pub(crate) fn load(mut input: LfsRuntimeConfigInput<'_>) -> Result<Self, LfsConfigError> {
        validate_base_path(input.git_dir)?;

        let http_policy = Arc::new(
            LfsHttpPolicy::from_entries(input.entries, std::mem::take(&mut input.http_environment))
                .map_err(LfsConfigError::HttpPolicy)?,
        );

        let safe_lfsconfig = input
            .lfsconfig
            .load()?
            .map(LfsConfigSnapshot::parse)
            .transpose()?
            .unwrap_or_else(SafeLfsConfig::empty);
        validate_base_path(input.default_storage_git_dir)?;
        let storage = match last_value(input.entries, "lfs", "", "storage") {
            Some(configured) => resolve_storage_path(input.default_storage_git_dir, configured)?,
            None => resolve_storage_path(input.default_storage_git_dir, DEFAULT_LFS_STORAGE_NAME)?,
        };
        let concurrent_transfers = parse_concurrent_transfers(last_value(
            input.entries,
            "lfs",
            "",
            "concurrenttransfers",
        ))?;
        let builtin_rewriter = GitUrlInsteadOfRewriter::from_entries(input.entries)?;
        let input = LfsRuntimeConfigInput {
            url_rewriter: input.url_rewriter.or(Some(&builtin_rewriter)),
            ..input
        };
        let endpoint_inputs = assemble_endpoint_inputs(&input, &safe_lfsconfig)?;

        let include = effective_or_safe_value(input.entries, &safe_lfsconfig, "fetchinclude");
        let exclude = effective_or_safe_value(input.entries, &safe_lfsconfig, "fetchexclude");
        let fetch_filter = LfsFetchFilter::from_values(include, exclude)?;
        let skip_smudge = parse_skip_smudge(input.skip_smudge.map(str::to_owned))
            .map_err(|_| LfsConfigError::InvalidSkipSmudge)?;
        let remote_policy = LfsRemoteSelectionPolicy {
            autodetect: effective_bool(input.entries, &safe_lfsconfig, "remote.autodetect", false)?,
            search_all: effective_bool(input.entries, &safe_lfsconfig, "remote.searchall", false)?,
        };
        let allow_incomplete_push =
            effective_bool(input.entries, &safe_lfsconfig, "allowincompletepush", false)?;
        let locks_verify = effective_optional_bool(input.entries, &safe_lfsconfig, "locksverify")?;
        let skip_download_errors =
            effective_bool(input.entries, &safe_lfsconfig, "skipdownloaderrors", false)?
                || parse_optional_bool(input.skip_download_errors, LfsConfigError::InvalidInput)?;
        let credential_use_http_path = assemble_credential_use_http_path_policy(input.entries)?;
        let access_rules = assemble_access_rules(input.entries, &safe_lfsconfig)?;
        let locks_rules = assemble_locks_rules(input.entries)?;

        Ok(Self {
            storage,
            concurrent_transfers,
            endpoint_inputs,
            fetch_filter,
            credential_use_http_path,
            http_policy,
            skip_smudge,
            remote_policy,
            allow_incomplete_push,
            locks_verify,
            skip_download_errors,
            access_rules,
            locks_rules,
        })
    }

    pub(crate) fn storage(&self) -> &Path {
        &self.storage
    }

    pub(crate) fn concurrent_transfers(&self) -> LfsTransferConcurrency {
        self.concurrent_transfers
    }

    pub(crate) fn endpoint_inputs(&self) -> &LfsEndpointInputs {
        &self.endpoint_inputs
    }

    pub(crate) fn fetch_filter(&self) -> &LfsFetchFilter {
        &self.fetch_filter
    }

    pub(crate) fn credential_use_http_path_for(
        &self,
        endpoint: &LfsHttpUrl,
    ) -> Result<bool, LfsEndpointError> {
        self.credential_use_http_path.for_endpoint(endpoint)
    }

    pub(crate) fn http_policy(&self) -> &Arc<LfsHttpPolicy> {
        &self.http_policy
    }

    pub(crate) fn skip_smudge(&self) -> bool {
        self.skip_smudge
    }

    pub(crate) fn remote_policy(&self) -> LfsRemoteSelectionPolicy {
        self.remote_policy
    }

    pub(crate) fn allow_incomplete_push(&self) -> bool {
        self.allow_incomplete_push
    }

    pub(crate) fn locks_verify(&self) -> Option<bool> {
        self.locks_verify
    }

    pub(crate) fn skip_download_errors(&self) -> bool {
        self.skip_download_errors
    }

    /// Resolve endpoint-scoped access policy. Ordinary Git configuration has
    /// precedence over `.lfsconfig`; within one source the longest matching
    /// validated URL scope wins and equal scopes retain Git's last-value rule.
    pub(crate) fn access_for(&self, endpoint: &LfsHttpUrl) -> LfsAccessMode {
        best_access_rule(&self.access_rules.ordinary, endpoint)
            .or_else(|| best_access_rule(&self.access_rules.safe, endpoint))
            .unwrap_or_default()
    }

    /// Resolve URL-scoped lock verification. Ordinary configuration wins over
    /// `.lfsconfig`; the longest matching URL scope wins within one source.
    /// The unscoped value remains the fallback.
    pub(crate) fn locks_verify_for(&self, endpoint: &LfsHttpUrl) -> Option<bool> {
        best_locks_rule(&self.locks_rules.ordinary, endpoint).or(self.locks_verify)
    }
}

fn best_access_rule(rules: &[LfsAccessRule], endpoint: &LfsHttpUrl) -> Option<LfsAccessMode> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.scope.is_access_scope_for(endpoint))
        .max_by_key(|(index, rule)| (rule.scope.access_scope_len(), *index))
        .map(|(_, rule)| rule.mode)
}

fn assemble_access_rules(
    entries: &[ConfigEntry],
    safe: &SafeLfsConfig,
) -> Result<LfsAccessRules, LfsConfigError> {
    let mut rules = LfsAccessRules::default();
    for (scope, value) in safe.access_values() {
        if let Ok(scope) = parse_http_url(scope)
            && scope.is_valid_access_scope()
        {
            rules.safe.push(LfsAccessRule {
                scope,
                mode: parse_access_mode(value)?,
            });
        }
    }
    for entry in entries {
        if entry.section == "lfs" && entry.key == "access" && !entry.subsection.is_empty() {
            let scope = parse_http_url(&entry.subsection)
                .map_err(|_| LfsConfigError::InvalidAccessPolicy)?;
            if !scope.is_valid_access_scope() {
                return Err(LfsConfigError::InvalidAccessPolicy);
            }
            rules.ordinary.push(LfsAccessRule {
                scope,
                mode: parse_access_mode(&entry.value)?,
            });
        }
    }
    Ok(rules)
}

fn best_locks_rule(rules: &[LfsLocksRule], endpoint: &LfsHttpUrl) -> Option<bool> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.scope.is_access_scope_for(endpoint))
        .max_by_key(|(index, rule)| (rule.scope.access_scope_len(), *index))
        .map(|(_, rule)| rule.verify)
}

fn assemble_locks_rules(entries: &[ConfigEntry]) -> Result<LfsLocksRules, LfsConfigError> {
    let mut rules = LfsLocksRules::default();
    for entry in entries {
        if entry.section != "lfs" || entry.key != "locksverify" || entry.subsection.is_empty() {
            continue;
        }
        let verify = if entry.implicit_bool {
            true
        } else {
            parse_bool(&entry.value)?
        };
        let scope =
            parse_http_url(&entry.subsection).map_err(|_| LfsConfigError::InvalidLocksPolicy)?;
        if !scope.is_valid_access_scope() {
            return Err(LfsConfigError::InvalidLocksPolicy);
        }
        rules.ordinary.push(LfsLocksRule { scope, verify });
    }
    Ok(rules)
}

fn assemble_credential_use_http_path_policy(
    entries: &[ConfigEntry],
) -> Result<LfsCredentialUseHttpPathPolicy, LfsConfigError> {
    let mut policy = LfsCredentialUseHttpPathPolicy::default();
    for (order, entry) in entries.iter().enumerate() {
        if entry.section != "credential" || entry.key != "usehttppath" {
            continue;
        }
        let value = entry.bool_value().ok_or(LfsConfigError::InvalidBoolean)?;
        if entry.subsection.is_empty() {
            policy.global = value;
            continue;
        }
        let scope =
            LfsUrlScope::parse(&entry.subsection).ok_or(LfsConfigError::InvalidCredentialPolicy)?;
        policy.scoped.push(LfsCredentialUseHttpPathRule {
            scope,
            value,
            order,
        });
    }
    Ok(policy)
}

fn parse_access_mode(value: &str) -> Result<LfsAccessMode, LfsConfigError> {
    if value.eq_ignore_ascii_case("basic") {
        Ok(LfsAccessMode::Basic)
    } else if value.eq_ignore_ascii_case("none") {
        Ok(LfsAccessMode::None)
    } else {
        Err(LfsConfigError::InvalidAccessPolicy)
    }
}

/// Sanitized errors produced while assembling configuration.
///
/// Paths, config keys, URLs, and environment values are intentionally absent
/// from the payload and display text.  Callers can attach their own context at
/// a policy boundary without accidentally printing repository-controlled
/// secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsConfigError {
    InvalidInput,
    InvalidPath,
    UntrustedWorktreeRoot,
    InvalidStorage,
    InvalidSkipSmudge,
    InvalidBoolean,
    InvalidPattern,
    TooManyPatterns,
    LfsConfigIo(io::ErrorKind),
    LfsConfigTooLarge,
    LfsConfigSymlink,
    LfsConfigNotUtf8,
    LfsConfigControl,
    LfsConfigChanged,
    LfsConfigUnsafeKey,
    LfsConfigSyntax,
    InvalidFetchHead,
    UrlRewriteFailed,
    InvalidAccessPolicy,
    InvalidLocksPolicy,
    InvalidCredentialPolicy,
    InvalidConcurrentTransfers,
    HttpPolicy(LfsHttpPolicyError),
}

impl fmt::Display for LfsConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput => formatter.write_str("invalid LFS configuration input"),
            Self::InvalidPath => formatter.write_str("invalid LFS configuration path"),
            Self::UntrustedWorktreeRoot => formatter.write_str("untrusted LFS worktree root"),
            Self::InvalidStorage => formatter.write_str("invalid LFS storage path"),
            Self::InvalidSkipSmudge => formatter.write_str("invalid GIT_LFS_SKIP_SMUDGE value"),
            Self::InvalidBoolean => formatter.write_str("invalid LFS boolean value"),
            Self::InvalidPattern => formatter.write_str("invalid LFS fetch path pattern"),
            Self::TooManyPatterns => formatter.write_str("too many LFS fetch path patterns"),
            Self::LfsConfigIo(kind) => write!(formatter, "LFS config read failed ({kind:?})"),
            Self::LfsConfigTooLarge => formatter.write_str(".lfsconfig is too large"),
            Self::LfsConfigSymlink => formatter.write_str(".lfsconfig contains a symlink"),
            Self::LfsConfigNotUtf8 => formatter.write_str(".lfsconfig is not valid UTF-8"),
            Self::LfsConfigControl => formatter.write_str(".lfsconfig contains control characters"),
            Self::LfsConfigChanged => formatter.write_str(".lfsconfig changed while it was read"),
            Self::LfsConfigUnsafeKey => formatter.write_str(".lfsconfig contains an unsafe key"),
            Self::LfsConfigSyntax => formatter.write_str("invalid .lfsconfig syntax"),
            Self::InvalidFetchHead => formatter.write_str("invalid FETCH_HEAD input"),
            Self::UrlRewriteFailed => formatter.write_str("LFS remote URL rewrite failed"),
            Self::InvalidAccessPolicy => formatter.write_str("invalid LFS access policy"),
            Self::InvalidLocksPolicy => formatter.write_str("invalid LFS locks policy"),
            Self::InvalidCredentialPolicy => formatter.write_str("invalid LFS credential policy"),
            Self::InvalidConcurrentTransfers => {
                formatter.write_str("lfs.concurrenttransfers must be an integer in 1..=8")
            }
            Self::HttpPolicy(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LfsConfigError {}

/// Byte-preserving fetch include/exclude policy. Excludes always win, matching
/// Git LFS fetch behavior without converting repository paths through UTF-8.
#[derive(Clone)]
pub(crate) struct LfsFetchFilter {
    include: Vec<LfsFetchPattern>,
    exclude: Vec<LfsFetchPattern>,
    max_match_work: usize,
}

impl PartialEq for LfsFetchFilter {
    fn eq(&self, other: &Self) -> bool {
        self.include_patterns().eq(other.include_patterns())
            && self.exclude_patterns().eq(other.exclude_patterns())
    }
}

impl Eq for LfsFetchFilter {}

impl fmt::Debug for LfsFetchFilter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsFetchFilter")
            .field("include_count", &self.include.len())
            .field("exclude_count", &self.exclude.len())
            .finish()
    }
}

#[derive(Clone, Debug)]
struct LfsFetchPattern {
    pattern: String,
    matcher: LfsBytePattern,
}

#[derive(Clone, Debug)]
struct LfsBytePattern {
    wildcard: LfsWildcardPattern,
}

#[derive(Clone, Debug)]
struct LfsWildcardPattern {
    tokens: Vec<LfsWildcardToken>,
    work_per_text_cell: usize,
}

#[derive(Clone, Debug)]
enum LfsWildcardToken {
    Literal(u8),
    AnyByte,
    Star,
    GlobStar,
    GlobDirectories,
    TrailingContents,
    Never,
    Class(LfsWildcardClass),
}

#[derive(Clone, Debug)]
struct LfsWildcardClass {
    include: Vec<LfsWildcardClassAtom>,
    exclude: Vec<LfsWildcardClassAtom>,
}

#[derive(Clone, Debug)]
enum LfsWildcardClassAtom {
    Characters(Vec<char>),
    Range(LfsCharacterRange),
    Posix(LfsPosixClass),
}

#[derive(Clone, Copy, Debug)]
struct LfsCharacterRange {
    start: char,
    end: char,
}

#[derive(Clone, Copy, Debug)]
enum LfsPosixClass {
    Alnum,
    Alpha,
    Blank,
    Control,
    Digit,
    Graph,
    Lower,
    Print,
    Punctuation,
    Space,
    Upper,
    HexDigit,
}

#[derive(Debug, Default)]
struct LfsMatchScratch {
    previous: Vec<u8>,
    current: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
struct LfsMatchBudget {
    remaining: usize,
}

impl LfsFetchFilter {
    pub(crate) fn from_values(
        include: Option<&str>,
        exclude: Option<&str>,
    ) -> Result<Self, LfsConfigError> {
        let include = parse_patterns(include)?;
        let exclude = parse_patterns(exclude)?;
        let max_match_work = fetch_filter_match_work_bound(&include, &exclude)?;
        Ok(Self {
            include,
            exclude,
            max_match_work,
        })
    }

    pub(crate) fn allows(&self, repository_path: &str) -> bool {
        self.allows_bytes(repository_path.as_bytes())
    }

    pub(crate) fn allows_bytes(&self, repository_path: &[u8]) -> bool {
        if !valid_repository_path_bytes(repository_path) {
            return false;
        }
        let folded_path = system_case_path(repository_path);
        let repository_path = folded_path.as_deref().unwrap_or(repository_path);
        let mut budget = LfsMatchBudget {
            remaining: self.max_match_work,
        };
        let mut scratch = LfsMatchScratch::default();
        if !self.include.is_empty()
            && !matches_lfs_patterns(&self.include, repository_path, &mut budget, &mut scratch)
        {
            return false;
        }
        !matches_lfs_patterns(&self.exclude, repository_path, &mut budget, &mut scratch)
    }

    pub(crate) fn include_patterns(&self) -> impl Iterator<Item = &str> {
        self.include.iter().map(|pattern| pattern.pattern.as_str())
    }

    pub(crate) fn exclude_patterns(&self) -> impl Iterator<Item = &str> {
        self.exclude.iter().map(|pattern| pattern.pattern.as_str())
    }
}

impl LfsBytePattern {
    fn new(pattern: &str) -> Result<Self, LfsConfigError> {
        Ok(Self {
            wildcard: LfsWildcardPattern::compile(pattern)?,
        })
    }

    fn matches(
        &self,
        path: &[u8],
        budget: &mut LfsMatchBudget,
        scratch: &mut LfsMatchScratch,
    ) -> bool {
        lfs_wildcard_match(&self.wildcard, path, budget, scratch)
    }
}

fn valid_repository_path_bytes(path: &[u8]) -> bool {
    !path.is_empty()
        && path.len() <= MAX_FETCH_PATH_BYTES
        && !path.contains(&0)
        && !path.starts_with(b"/")
        && !path.ends_with(b"/")
        && !path.windows(2).any(|window| window == b"//")
}

fn matches_lfs_patterns(
    patterns: &[LfsFetchPattern],
    path: &[u8],
    budget: &mut LfsMatchBudget,
    scratch: &mut LfsMatchScratch,
) -> bool {
    patterns
        .iter()
        .any(|pattern| pattern.matcher.matches(path, budget, scratch))
}

fn system_case_path(path: &[u8]) -> Option<Vec<u8>> {
    #[cfg(any(target_os = "macos", windows))]
    {
        Some(lowercase_utf8_preserving_invalid(path))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = path;
        None
    }
}

fn lowercase_utf8_preserving_invalid(value: &[u8]) -> Vec<u8> {
    let mut lowered = Vec::with_capacity(value.len());
    let mut remaining = value;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                for character in valid.chars() {
                    let character = character.to_lowercase().next().unwrap_or(character);
                    let mut encoded = [0_u8; 4];
                    lowered.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                }
                break;
            }
            Err(error) => {
                let valid_length = error.valid_up_to();
                if valid_length != 0 {
                    let valid = std::str::from_utf8(&remaining[..valid_length])
                        .expect("validated UTF-8 prefix");
                    for character in valid.chars() {
                        let character = character.to_lowercase().next().unwrap_or(character);
                        let mut encoded = [0_u8; 4];
                        lowered.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                    }
                }
                // Go ranges over an ill-formed string as one U+FFFD rune per
                // invalid byte; strings.ToLower therefore canonicalizes the
                // byte before wildmatch sees it on SystemCase platforms.
                lowered.extend_from_slice("\u{fffd}".as_bytes());
                remaining = &remaining[valid_length + 1..];
            }
        }
    }
    lowered
}

fn slash_escape_lfs_pattern(pattern: &str) -> Result<String, LfsConfigError> {
    let bytes = pattern.as_bytes();
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if let Some(escaped) = bytes.get(index + 1)
                && matches!(*escaped, b'\\' | b'[' | b']' | b'*' | b'?' | b'#')
            {
                normalized.push(b'\\');
                normalized.push(*escaped);
                index += 2;
                continue;
            }
            normalized.push(b'/');
        } else {
            normalized.push(bytes[index]);
        }
        index += 1;
    }
    let normalized = String::from_utf8(normalized).map_err(|_| LfsConfigError::InvalidPattern)?;
    #[cfg(any(target_os = "macos", windows))]
    let normalized = normalized
        .chars()
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .collect();
    Ok(normalized)
}

impl LfsWildcardPattern {
    fn compile(pattern: &str) -> Result<Self, LfsConfigError> {
        let normalized = slash_escape_lfs_pattern(pattern)?;
        let separated_components = normalized.split('/').collect::<Vec<_>>();
        let component_count = separated_components
            .len()
            .saturating_sub(usize::from(normalized.ends_with('/')));
        let components = normalized
            .split('/')
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();
        if components.is_empty() {
            return Self::from_tokens(vec![LfsWildcardToken::Never]);
        }
        let unanchored = component_count == 1;
        let normalized = components.join("/");
        let pattern = normalized.as_bytes();
        let mut tokens = Vec::with_capacity(pattern.len().saturating_add(2));
        if unanchored {
            tokens.push(LfsWildcardToken::GlobDirectories);
        }
        let mut index = 0;
        while index < pattern.len() {
            let left_boundary = index == 0 || pattern[index - 1] == b'/';
            let right_boundary = index + 2 == pattern.len()
                || pattern.get(index + 2).is_some_and(|byte| *byte == b'/');
            if pattern[index] == b'\\'
                && let Some(literal) = pattern.get(index + 1)
            {
                tokens.push(LfsWildcardToken::Literal(*literal));
                index += 2;
            } else if left_boundary && pattern[index..].starts_with(b"**/") {
                tokens.push(LfsWildcardToken::GlobDirectories);
                index += 3;
            } else if left_boundary && right_boundary && pattern[index..].starts_with(b"**") {
                tokens.push(LfsWildcardToken::GlobStar);
                index += 2;
            } else {
                match pattern[index] {
                    b'*' => tokens.push(LfsWildcardToken::Star),
                    b'?' => tokens.push(LfsWildcardToken::AnyByte),
                    b'[' => {
                        let (class, next) = LfsWildcardClass::parse(pattern, index)?;
                        tokens.push(LfsWildcardToken::Class(class));
                        index = next;
                        continue;
                    }
                    literal => tokens.push(LfsWildcardToken::Literal(literal)),
                }
                index += 1;
            }
        }
        tokens.push(LfsWildcardToken::TrailingContents);
        Self::from_tokens(tokens)
    }

    fn from_tokens(tokens: Vec<LfsWildcardToken>) -> Result<Self, LfsConfigError> {
        // Two full-row units account for initialization and scratch clearing;
        // every token then charges its worst-case work for every text cell.
        let work_per_text_cell = tokens.iter().try_fold(2_usize, |work, token| {
            work.checked_add(token.work_per_text_cell()?)
        });
        let work_per_text_cell = work_per_text_cell.ok_or(LfsConfigError::InvalidPattern)?;
        Ok(Self {
            tokens,
            work_per_text_cell,
        })
    }

    fn work_per_text_cell(&self) -> usize {
        self.work_per_text_cell
    }

    fn work_for_text(&self, text_len: usize) -> Option<usize> {
        self.work_per_text_cell
            .checked_mul(text_len.checked_add(1)?)
    }
}

impl LfsWildcardClass {
    fn parse(pattern: &[u8], start: usize) -> Result<(Self, usize), LfsConfigError> {
        let mut parsed = Self {
            include: Vec::new(),
            exclude: Vec::new(),
        };
        let mut run = Vec::new();
        let mut negated = false;
        let mut index = start + 1;
        while index < pattern.len() {
            match pattern[index] {
                b']' => {
                    parsed.push_characters(&mut run, negated);
                    return Ok((parsed, index + 1));
                }
                b'!' | b'^' => {
                    negated = !negated;
                    index += 1;
                }
                b'[' if pattern[index..].starts_with(b"[:") => {
                    let class = &pattern[index..];
                    let close = class
                        .windows(2)
                        .position(|window| window == b":]")
                        .ok_or(LfsConfigError::InvalidPattern)?;
                    if close == 1 {
                        run.extend(['[', ':', ']']);
                        index += 2;
                        continue;
                    }
                    let name = std::str::from_utf8(&class[2..close])
                        .map_err(|_| LfsConfigError::InvalidPattern)?;
                    let class = LfsPosixClass::parse(name)?;
                    parsed.push_atom(LfsWildcardClassAtom::Posix(class), negated);
                    index += close + 2;
                }
                b'-' => {
                    let next = next_utf8_character(pattern, index + 1)
                        .ok_or(LfsConfigError::InvalidPattern)?;
                    if run.is_empty() || next.0 == ']' {
                        run.push('-');
                        index += 1;
                    } else {
                        let start = run.pop().expect("checked non-empty class run");
                        parsed.push_characters(&mut run, negated);
                        // wildmatch/v2.0.1 forms range endpoints from the
                        // adjacent pattern bytes even though it tests the
                        // subject as a Unicode rune.  Preserve that pinned
                        // behavior (notably `[α-ω]` does not contain `β`).
                        let start = char::from(
                            *start
                                .encode_utf8(&mut [0_u8; 4])
                                .as_bytes()
                                .last()
                                .expect("encoded class endpoint"),
                        );
                        let end = char::from(
                            *next
                                .0
                                .encode_utf8(&mut [0_u8; 4])
                                .as_bytes()
                                .first()
                                .expect("encoded class endpoint"),
                        );
                        parsed.push_atom(
                            LfsWildcardClassAtom::Range(LfsCharacterRange {
                                start: start.min(end),
                                end: start.max(end),
                            }),
                            negated,
                        );
                        index += 1 + next.1;
                    }
                }
                b'\\' => {
                    let (literal, length) = next_utf8_character(pattern, index + 1)
                        .ok_or(LfsConfigError::InvalidPattern)?;
                    run.push(literal);
                    index += 1 + length;
                }
                _ => {
                    let (literal, length) = next_utf8_character(pattern, index)
                        .ok_or(LfsConfigError::InvalidPattern)?;
                    run.push(literal);
                    index += length;
                }
            }
        }
        Err(LfsConfigError::InvalidPattern)
    }

    fn push_characters(&mut self, run: &mut Vec<char>, negated: bool) {
        if !run.is_empty() {
            self.push_atom(
                LfsWildcardClassAtom::Characters(std::mem::take(run)),
                negated,
            );
        }
    }

    fn push_atom(&mut self, atom: LfsWildcardClassAtom, negated: bool) {
        if negated {
            self.exclude.push(atom);
        } else {
            self.include.push(atom);
        }
    }

    fn matches(&self, value: char) -> bool {
        (self.include.is_empty() || self.include.iter().any(|atom| atom.matches(value)))
            && !self.exclude.iter().any(|atom| atom.matches(value))
    }
}

fn lfs_wildcard_match(
    pattern: &LfsWildcardPattern,
    text: &[u8],
    budget: &mut LfsMatchBudget,
    scratch: &mut LfsMatchScratch,
) -> bool {
    budget.charge(pattern.work_per_text_cell(), text.len());
    scratch.previous.resize(text.len() + 1, 0);
    scratch.current.resize(text.len() + 1, 0);
    scratch.previous.fill(0);
    scratch.current.fill(0);
    scratch.previous[0] = 1;
    for token in &pattern.tokens {
        scratch.current.fill(0);
        match token {
            LfsWildcardToken::Star | LfsWildcardToken::GlobStar => {
                scratch.current[0] = scratch.previous[0];
                let crosses_slash = matches!(token, LfsWildcardToken::GlobStar);
                for index in 1..=text.len() {
                    scratch.current[index] = u8::from(
                        scratch.previous[index] != 0
                            || scratch.current[index - 1] != 0
                                && (crosses_slash || text[index - 1] != b'/'),
                    );
                }
            }
            LfsWildcardToken::GlobDirectories => {
                scratch.current[0] = scratch.previous[0];
                let mut consuming = false;
                for index in 1..=text.len() {
                    consuming |= scratch.previous[index - 1] != 0;
                    scratch.current[index] = u8::from(
                        scratch.previous[index] != 0 || consuming && text[index - 1] == b'/',
                    );
                }
            }
            LfsWildcardToken::TrailingContents => {
                for index in 0..=text.len() {
                    if scratch.previous[index] != 0 && (index == text.len() || text[index] == b'/')
                    {
                        scratch.current[text.len()] = 1;
                        break;
                    }
                }
            }
            LfsWildcardToken::Never => {}
            _ => {
                for index in 0..text.len() {
                    if scratch.previous[index] == 0 {
                        continue;
                    }
                    if let Some(width) = token.match_width(&text[index..]) {
                        scratch.current[index + width] = 1;
                    }
                }
            }
        }
        std::mem::swap(&mut scratch.previous, &mut scratch.current);
    }
    scratch.previous[text.len()] != 0
}

impl LfsWildcardToken {
    fn work_per_text_cell(&self) -> Option<usize> {
        match self {
            Self::Class(class) => class.match_work()?.checked_add(1),
            Self::Literal(_)
            | Self::AnyByte
            | Self::Star
            | Self::GlobStar
            | Self::GlobDirectories
            | Self::TrailingContents
            | Self::Never => Some(1),
        }
    }

    fn match_width(&self, value: &[u8]) -> Option<usize> {
        let first = *value.first()?;
        match self {
            Self::Literal(literal) => (*literal == first).then_some(1),
            Self::AnyByte => (first != b'/').then_some(1),
            Self::Class(class) if first != b'/' => {
                let (character, width) = decode_first_lfs_rune(value);
                class.matches(character).then_some(width)
            }
            Self::Star
            | Self::GlobStar
            | Self::GlobDirectories
            | Self::TrailingContents
            | Self::Never
            | Self::Class(_) => None,
        }
    }
}

fn next_utf8_character(value: &[u8], index: usize) -> Option<(char, usize)> {
    let value = std::str::from_utf8(value.get(index..)?).ok()?;
    let character = value.chars().next()?;
    Some((character, character.len_utf8()))
}

fn decode_first_lfs_rune(value: &[u8]) -> (char, usize) {
    match std::str::from_utf8(value) {
        Ok(value) => {
            let character = value.chars().next().expect("non-empty UTF-8 string");
            (character, character.len_utf8())
        }
        Err(error) if error.valid_up_to() != 0 => {
            let valid =
                std::str::from_utf8(&value[..error.valid_up_to()]).expect("validated UTF-8 prefix");
            let character = valid.chars().next().expect("non-empty UTF-8 prefix");
            (character, character.len_utf8())
        }
        Err(error) => ('\u{fffd}', error.error_len().unwrap_or(1)),
    }
}

impl LfsWildcardClassAtom {
    fn match_work(&self) -> usize {
        match self {
            Self::Characters(characters) => characters.len(),
            Self::Range(_) => 1,
            Self::Posix(_) => LFS_POSIX_CLASS_MATCH_WORK,
        }
    }

    fn matches(&self, value: char) -> bool {
        match self {
            Self::Characters(characters) => characters.contains(&value),
            Self::Range(range) => range.start <= value && value <= range.end,
            Self::Posix(class) => class.matches(value),
        }
    }
}

impl LfsWildcardClass {
    fn match_work(&self) -> Option<usize> {
        // One unit covers the include/exclude decision. Each atom adds one
        // dispatch unit plus its worst-case scan/evaluation cost; both lists
        // may be exhausted for a single active DP cell.
        self.include
            .iter()
            .chain(&self.exclude)
            .try_fold(1_usize, |work, atom| {
                work.checked_add(1)?.checked_add(atom.match_work())
            })
    }
}

impl LfsPosixClass {
    fn parse(name: &str) -> Result<Self, LfsConfigError> {
        match name.to_ascii_lowercase().as_str() {
            "alnum" => Ok(Self::Alnum),
            "alpha" => Ok(Self::Alpha),
            "blank" => Ok(Self::Blank),
            "cntrl" => Ok(Self::Control),
            "digit" => Ok(Self::Digit),
            "graph" => Ok(Self::Graph),
            "lower" => Ok(Self::Lower),
            "print" => Ok(Self::Print),
            "punct" => Ok(Self::Punctuation),
            "space" => Ok(Self::Space),
            "upper" => Ok(Self::Upper),
            "xdigit" => Ok(Self::HexDigit),
            _ => Err(LfsConfigError::InvalidPattern),
        }
    }

    fn matches(self, value: char) -> bool {
        match self {
            Self::Alnum => unicode_class_matches(&UNICODE_ALNUM, r"\A(?:\p{N}|\p{L})\z", value),
            Self::Alpha => unicode_class_matches(&UNICODE_ALPHA, r"\A\p{L}\z", value),
            Self::Blank => matches!(value, ' ' | '\t'),
            Self::Control => unicode_class_matches(&UNICODE_CONTROL, r"\A\p{Cc}\z", value),
            Self::Digit => unicode_class_matches(&UNICODE_DIGIT, r"\A\p{Nd}\z", value),
            Self::Graph => unicode_class_matches(
                &UNICODE_GRAPH,
                r"\A[\p{L}\p{M}\p{N}\p{P}\p{S}\p{Zs}]\z",
                value,
            ),
            Self::Lower => unicode_class_matches(&UNICODE_LOWER, r"\A\p{Ll}\z", value),
            Self::Print => {
                value == ' '
                    || unicode_class_matches(
                        &UNICODE_PRINT,
                        r"\A[\p{L}\p{M}\p{N}\p{P}\p{S}]\z",
                        value,
                    )
            }
            Self::Punctuation => unicode_class_matches(&UNICODE_PUNCTUATION, r"\A\p{P}\z", value),
            Self::Space => unicode_class_matches(&UNICODE_SPACE, r"\A\p{White_Space}\z", value),
            Self::Upper => unicode_class_matches(&UNICODE_UPPER, r"\A\p{Lu}\z", value),
            Self::HexDigit => {
                unicode_class_matches(&UNICODE_DIGIT, r"\A\p{Nd}\z", value)
                    || value.is_ascii_hexdigit()
            }
        }
    }
}

static UNICODE_ALNUM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_ALPHA: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_CONTROL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_DIGIT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_GRAPH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_LOWER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_PRINT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_PUNCTUATION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_SPACE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static UNICODE_UPPER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

fn unicode_class_matches(
    regex: &'static std::sync::OnceLock<regex::Regex>,
    expression: &'static str,
    value: char,
) -> bool {
    let regex = regex.get_or_init(|| regex::Regex::new(expression).expect("valid Unicode class"));
    let mut encoded = [0_u8; 4];
    regex.is_match(value.encode_utf8(&mut encoded))
}

impl LfsMatchBudget {
    fn charge(&mut self, work_per_text_cell: usize, text_len: usize) {
        let text_cells = text_len
            .checked_add(1)
            .expect("validated LFS fetch path length fits usize");
        let work = work_per_text_cell
            .checked_mul(text_cells)
            .expect("validated LFS fetch matcher work fits usize");
        self.remaining = self
            .remaining
            .checked_sub(work)
            .expect("validated LFS fetch matcher work stays within its construction-time bound");
    }
}

fn fetch_filter_match_work_bound(
    include: &[LfsFetchPattern],
    exclude: &[LfsFetchPattern],
) -> Result<usize, LfsConfigError> {
    let work_per_text_cell = include
        .iter()
        .chain(exclude)
        .try_fold(0_usize, |total, pattern| {
            total.checked_add(pattern.matcher.wildcard_work_per_text_cell())
        })
        .ok_or(LfsConfigError::InvalidPattern)?;
    let work = work_per_text_cell
        .checked_mul(MAX_FETCH_MATCH_TEXT_CELLS)
        .ok_or(LfsConfigError::InvalidPattern)?;
    if work > MAX_FETCH_MATCH_WORK {
        return Err(LfsConfigError::InvalidPattern);
    }
    Ok(work)
}

impl LfsBytePattern {
    fn wildcard_work_per_text_cell(&self) -> usize {
        self.wildcard.work_per_text_cell()
    }
}

fn assemble_endpoint_inputs(
    input: &LfsRuntimeConfigInput<'_>,
    safe_lfsconfig: &SafeLfsConfig,
) -> Result<LfsEndpointInputs, LfsConfigError> {
    let branch = normalize_branch(input.branch)?;
    let requested_remote = validate_optional_name(input.requested_remote)?;
    let current_remote = branch
        .as_deref()
        .map(|name| last_remote_value(input.entries, "branch", name, "remote"))
        .transpose()?
        .flatten();
    let current_push_remote = branch
        .as_deref()
        .map(|name| last_remote_value(input.entries, "branch", name, "pushremote"))
        .transpose()?
        .flatten();

    let configured_default_remote = last_remote_value(input.entries, "remote", "", "lfsdefault")?;
    let push_default_remote = last_remote_value(input.entries, "remote", "", "lfspushdefault")?.or(
        last_remote_value(input.entries, "remote", "", "pushdefault")?,
    );
    let fetch_head_url = input
        .fetch_head
        .map(parse_fetch_head_url)
        .transpose()?
        .flatten()
        .map(|url| rewrite_remote_url(input.url_rewriter, &url, false))
        .transpose()?;

    let mut remotes = Vec::new();
    let mut names = BTreeSet::new();
    for entry in input.entries {
        if entry.section == "remote" && !entry.subsection.is_empty() {
            validate_remote_name(&entry.subsection)?;
            names.insert(entry.subsection.clone());
        }
    }
    for name in names {
        let raw_url = last_value(input.entries, "remote", &name, "url");
        let url = raw_url
            .map(|value| rewrite_remote_url(input.url_rewriter, value, false))
            .transpose()?;
        let push_url = last_value(input.entries, "remote", &name, "pushurl")
            .or(raw_url)
            .map(|value| rewrite_remote_url(input.url_rewriter, value, true))
            .transpose()?;
        let lfs_url = last_value(input.entries, "remote", &name, "lfsurl").map(str::to_owned);
        let lfs_push_url =
            last_value(input.entries, "remote", &name, "lfspushurl").map(str::to_owned);
        remotes.push(LfsRemoteConfig {
            name,
            url,
            push_url,
            lfs_url,
            lfs_push_url,
        });
    }

    Ok(LfsEndpointInputs {
        lfs_url: last_value(input.entries, "lfs", "", "url").map(str::to_owned),
        lfs_push_url: last_value(input.entries, "lfs", "", "pushurl").map(str::to_owned),
        lfs_git_protocol: last_value(input.entries, "lfs", "", "gitprotocol").map(str::to_owned),
        remotes,
        requested_remote,
        current_remote,
        default_remote: configured_default_remote,
        current_push_remote,
        push_default_remote,
        fetch_head_url,
        lfsconfig: safe_lfsconfig.clone(),
    })
}

fn rewrite_remote_url(
    rewriter: Option<&dyn LfsRemoteUrlRewriter>,
    url: &str,
    push: bool,
) -> Result<String, LfsConfigError> {
    rewriter
        .map(|rewriter| rewriter.rewrite(url, push))
        .unwrap_or_else(|| Ok(url.to_owned()))
}

fn effective_or_safe_value<'a>(
    entries: &'a [ConfigEntry],
    safe: &'a SafeLfsConfig,
    key: &str,
) -> Option<&'a str> {
    let safe_key = match key {
        "fetchinclude" => "lfs.fetchinclude",
        "fetchexclude" => "lfs.fetchexclude",
        _ => return last_value(entries, "lfs", "", key),
    };
    last_value(entries, "lfs", "", key).or_else(|| safe.value(safe_key))
}

fn effective_bool(
    entries: &[ConfigEntry],
    safe: &SafeLfsConfig,
    key: &str,
    default: bool,
) -> Result<bool, LfsConfigError> {
    let ordinary = if let Some((subsection, key)) = key.split_once('.') {
        last_entry(entries, "lfs", subsection, key)
    } else {
        last_entry(entries, "lfs", "", key)
    };
    if let Some(entry) = ordinary {
        return if entry.implicit_bool {
            Ok(true)
        } else {
            parse_bool(&entry.value)
        };
    }
    match safe.value(&format!("lfs.{key}")) {
        Some(value) => parse_bool(value),
        None => Ok(default),
    }
}

fn effective_optional_bool(
    entries: &[ConfigEntry],
    safe: &SafeLfsConfig,
    key: &str,
) -> Result<Option<bool>, LfsConfigError> {
    let ordinary = last_entry(entries, "lfs", "", key);
    if let Some(entry) = ordinary {
        return if entry.implicit_bool {
            Ok(Some(true))
        } else {
            parse_bool(&entry.value).map(Some)
        };
    }
    safe.value(&format!("lfs.{key}"))
        .map(parse_bool)
        .transpose()
}

fn parse_optional_bool(value: Option<&str>, error: LfsConfigError) -> Result<bool, LfsConfigError> {
    value
        .map(parse_bool)
        .transpose()
        .map_err(|_| error)
        .map(|value| value.unwrap_or(false))
}

fn parse_bool(value: &str) -> Result<bool, LfsConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(LfsConfigError::InvalidBoolean),
    }
}

fn parse_concurrent_transfers(
    value: Option<&str>,
) -> Result<LfsTransferConcurrency, LfsConfigError> {
    let Some(value) = value else {
        return Ok(LfsTransferConcurrency::default());
    };
    let value = value
        .parse::<u64>()
        .map_err(|_| LfsConfigError::InvalidConcurrentTransfers)?;
    let value = usize::try_from(value).map_err(|_| LfsConfigError::InvalidConcurrentTransfers)?;
    LfsTransferConcurrency::new(value).map_err(|_| LfsConfigError::InvalidConcurrentTransfers)
}

fn last_value<'a>(
    entries: &'a [ConfigEntry],
    section: &str,
    subsection: &str,
    key: &str,
) -> Option<&'a str> {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == section && entry.subsection == subsection && entry.key == key
        })
        .map(|entry| entry.value.as_str())
}

fn last_entry<'a>(
    entries: &'a [ConfigEntry],
    section: &str,
    subsection: &str,
    key: &str,
) -> Option<&'a ConfigEntry> {
    entries.iter().rev().find(|entry| {
        entry.section == section && entry.subsection == subsection && entry.key == key
    })
}

fn last_remote_value(
    entries: &[ConfigEntry],
    section: &str,
    subsection: &str,
    key: &str,
) -> Result<Option<String>, LfsConfigError> {
    last_value(entries, section, subsection, key)
        .map(|value| {
            validate_remote_name(value)?;
            Ok(value.to_owned())
        })
        .transpose()
}

fn normalize_branch(branch: Option<&str>) -> Result<Option<String>, LfsConfigError> {
    let Some(branch) = branch else {
        return Ok(None);
    };
    if branch.is_empty()
        || branch.len() > MAX_REMOTE_NAME_BYTES
        || branch.bytes().any(|byte| byte == 0)
        || branch.chars().any(|character| character.is_control())
    {
        return Err(LfsConfigError::InvalidInput);
    }
    Ok(Some(
        branch
            .strip_prefix("refs/heads/")
            .unwrap_or(branch)
            .to_owned(),
    ))
}

fn validate_optional_name(name: Option<&str>) -> Result<Option<String>, LfsConfigError> {
    let Some(name) = name else {
        return Ok(None);
    };
    validate_remote_name(name)?;
    Ok(Some(name.to_owned()))
}

fn validate_remote_name(name: &str) -> Result<(), LfsConfigError> {
    if name.is_empty()
        || name.len() > MAX_REMOTE_NAME_BYTES
        || name.bytes().any(|byte| byte == 0)
        || name.chars().any(|character| character.is_control())
    {
        return Err(LfsConfigError::InvalidInput);
    }
    Ok(())
}

/// Parse the URL-bearing portion of a Git `FETCH_HEAD` file.
pub(crate) fn parse_fetch_head_url(content: &str) -> Result<Option<String>, LfsConfigError> {
    if content.len() > MAX_FETCH_HEAD_BYTES
        || content.bytes().any(|byte| byte == 0)
        || content.chars().any(|character| {
            character.is_control() && character != '\n' && character != '\r' && character != '\t'
        })
    {
        return Err(LfsConfigError::InvalidFetchHead);
    }
    let mut found = None;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 3 || !valid_fetch_head_oid(fields[0]) {
            return Err(LfsConfigError::InvalidFetchHead);
        }
        if !fields[1].is_empty() && fields[1] != "not-for-merge" {
            return Err(LfsConfigError::InvalidFetchHead);
        }
        let candidate = parse_fetch_head_description(fields[2])?;
        if found.is_none() {
            found = Some(candidate);
        }
    }
    Ok(found)
}

fn valid_fetch_head_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_fetch_head_description(value: &str) -> Result<String, LfsConfigError> {
    let prefixes = ["branch '", "remote-tracking branch '", "tag '"];
    let prefix = prefixes
        .iter()
        .find(|prefix| value.starts_with(*prefix))
        .ok_or(LfsConfigError::InvalidFetchHead)?;
    let remainder = &value[prefix.len()..];
    let (reference, url) = remainder
        .split_once("' of ")
        .ok_or(LfsConfigError::InvalidFetchHead)?;
    if reference.is_empty()
        || reference.contains('\'')
        || reference.chars().any(|character| character.is_control())
        || url.is_empty()
        || url
            .chars()
            .any(|character| character.is_control() || character.is_ascii_whitespace())
    {
        return Err(LfsConfigError::InvalidFetchHead);
    }
    Ok(url.to_owned())
}

fn resolve_storage_path(git_dir: &Path, configured: &str) -> Result<PathBuf, LfsConfigError> {
    validate_text(configured, LfsConfigError::InvalidStorage)?;
    let path = Path::new(configured);
    if path.is_absolute() {
        validate_no_symlink_components(path, LfsConfigError::InvalidStorage)?;
        return Ok(path.to_path_buf());
    }
    let mut resolved = git_dir.to_path_buf();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => resolved.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(LfsConfigError::InvalidStorage);
            }
        }
    }
    validate_no_symlink_components(&resolved, LfsConfigError::InvalidStorage)?;
    Ok(resolved)
}

fn parse_patterns(value: Option<&str>) -> Result<Vec<LfsFetchPattern>, LfsConfigError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.len() > MAX_FETCH_VALUE_BYTES {
        return Err(LfsConfigError::InvalidPattern);
    }
    let mut patterns = Vec::new();
    for raw in value.split(',') {
        let raw_pattern = raw.trim();
        if raw_pattern.is_empty() {
            continue;
        }
        if raw_pattern.len() > MAX_FETCH_PATTERN_BYTES
            || raw_pattern.bytes().any(|byte| byte == 0)
            || raw_pattern.chars().any(|character| character.is_control())
        {
            return Err(LfsConfigError::InvalidPattern);
        }
        if patterns.len() >= MAX_FETCH_PATTERNS {
            return Err(LfsConfigError::TooManyPatterns);
        }
        // Git LFS tools.CleanPaths trims exactly one terminal separator
        // before constructing its wildmatch.  It does not interpret `!` as
        // negation; include and exclude are two independent OR lists.
        let pattern = raw_pattern
            .strip_suffix('/')
            .or_else(|| raw_pattern.strip_suffix('\\'))
            .unwrap_or(raw_pattern);
        patterns.push(LfsFetchPattern {
            pattern: pattern.to_owned(),
            matcher: LfsBytePattern::new(pattern)?,
        });
    }
    Ok(patterns)
}

#[cfg(unix)]
fn read_lfsconfig_snapshot(path: &Path) -> Result<Option<LfsConfigSnapshot>, LfsConfigError> {
    // The directory descriptor pins the trusted worktree while openat resolves
    // the final name.  The final O_NOFOLLOW prevents a swapped `.lfsconfig`
    // symlink from redirecting the read after any lexical/path validation.
    let parent = path.parent().ok_or(LfsConfigError::InvalidPath)?;
    validate_no_symlink_components(parent, LfsConfigError::LfsConfigSymlink)?;
    let parent_bytes =
        CString::new(parent.as_os_str().as_bytes()).map_err(|_| LfsConfigError::InvalidPath)?;
    let parent_fd = unsafe {
        libc::open(
            parent_bytes.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if parent_fd < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else if is_symlink_open_error(&error) {
            Err(LfsConfigError::LfsConfigSymlink)
        } else {
            Err(LfsConfigError::LfsConfigIo(error.kind()))
        };
    }
    let parent_file = unsafe { fs::File::from_raw_fd(parent_fd) };
    let name = CString::new(LFS_CONFIG_NAME).expect("static filename has no NUL");
    let file_fd = unsafe {
        libc::openat(
            parent_file.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if file_fd < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else if is_symlink_open_error(&error) {
            Err(LfsConfigError::LfsConfigSymlink)
        } else {
            Err(LfsConfigError::LfsConfigIo(error.kind()))
        };
    }
    let mut file = unsafe { fs::File::from_raw_fd(file_fd) };
    let metadata = file
        .metadata()
        .map_err(|error| LfsConfigError::LfsConfigIo(error.kind()))?;
    if !metadata.file_type().is_file() {
        return Err(LfsConfigError::LfsConfigIo(io::ErrorKind::InvalidInput));
    }
    if metadata.len() > MAX_LFS_CONFIG_BYTES {
        return Err(LfsConfigError::LfsConfigTooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_LFS_CONFIG_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| LfsConfigError::LfsConfigIo(error.kind()))?;
    if bytes.len() as u64 > MAX_LFS_CONFIG_BYTES {
        return Err(LfsConfigError::LfsConfigTooLarge);
    }
    validate_lfsconfig_bytes(&bytes)?;
    Ok(Some(LfsConfigSnapshot {
        source: LfsConfigSourceKind::Worktree,
        bytes,
    }))
}

#[cfg(windows)]
fn read_lfsconfig_snapshot(path: &Path) -> Result<Option<LfsConfigSnapshot>, LfsConfigError> {
    // OPEN_REPARSE_POINT makes the final path component resolve to its own
    // handle instead of traversing it.  Inspecting that same handle's
    // attributes closes the path-check/read TOCTOU gap for reparse files.
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // Some Windows compatibility layers report a reparse entry as
            // not found even with OPEN_REPARSE_POINT.  A path lookup is used
            // only to classify that failed open; bytes are still accepted
            // exclusively from the successfully opened, inspected handle.
            return match windows_path_is_reparse(path) {
                Ok(true) => Err(LfsConfigError::LfsConfigSymlink),
                Ok(false) => Err(LfsConfigError::LfsConfigIo(error.kind())),
                Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(metadata_error) => Err(LfsConfigError::LfsConfigIo(metadata_error.kind())),
            };
        }
        Err(error) if is_symlink_open_error(&error) => {
            return Err(LfsConfigError::LfsConfigSymlink);
        }
        Err(error) => return Err(LfsConfigError::LfsConfigIo(error.kind())),
    };
    let opened_information = windows_handle_information(&file)?;
    if opened_information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(LfsConfigError::LfsConfigSymlink);
    }
    if opened_information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
        || unsafe { GetFileType(file.as_raw_handle() as _) } != FILE_TYPE_DISK
    {
        return Err(LfsConfigError::LfsConfigIo(io::ErrorKind::InvalidInput));
    }
    let opened_size = (u64::from(opened_information.nFileSizeHigh) << u32::BITS)
        | u64::from(opened_information.nFileSizeLow);
    if opened_size > MAX_LFS_CONFIG_BYTES {
        return Err(LfsConfigError::LfsConfigTooLarge);
    }
    let mut bytes = Vec::with_capacity(opened_size as usize);
    file.take(MAX_LFS_CONFIG_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| LfsConfigError::LfsConfigIo(error.kind()))?;
    if bytes.len() as u64 > MAX_LFS_CONFIG_BYTES {
        return Err(LfsConfigError::LfsConfigTooLarge);
    }
    validate_lfsconfig_bytes(&bytes)?;
    Ok(Some(LfsConfigSnapshot {
        source: LfsConfigSourceKind::Worktree,
        bytes,
    }))
}

#[cfg(windows)]
fn windows_handle_information(
    file: &fs::File,
) -> Result<BY_HANDLE_FILE_INFORMATION, LfsConfigError> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) };
    if result == 0 {
        return Err(LfsConfigError::LfsConfigIo(
            io::Error::last_os_error().kind(),
        ));
    }
    Ok(information)
}

#[cfg(windows)]
fn windows_path_is_reparse(path: &Path) -> io::Result<bool> {
    use std::os::windows::fs::MetadataExt;

    Ok(fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(all(not(unix), not(windows)))]
fn read_lfsconfig_snapshot(_path: &Path) -> Result<Option<LfsConfigSnapshot>, LfsConfigError> {
    // No supported no-follow handle primitive exists on this target.  Do not
    // fall back to a path metadata check followed by an ordinary open.
    Err(LfsConfigError::LfsConfigIo(io::ErrorKind::Unsupported))
}

#[cfg(unix)]
fn is_symlink_open_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_symlink_open_error(_error: &io::Error) -> bool {
    false
}

fn validate_lfsconfig_bytes(bytes: &[u8]) -> Result<(), LfsConfigError> {
    let text = std::str::from_utf8(bytes).map_err(|_| LfsConfigError::LfsConfigNotUtf8)?;
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(LfsConfigError::LfsConfigControl);
    }
    Ok(())
}

fn parse_lfsconfig_bytes(bytes: &[u8]) -> Result<SafeLfsConfig, LfsConfigError> {
    validate_lfsconfig_bytes(bytes)?;
    let text = std::str::from_utf8(bytes).map_err(|_| LfsConfigError::LfsConfigNotUtf8)?;
    let mut section = None::<(String, String)>;
    let mut logical = String::new();
    let mut pairs = Vec::new();
    let mut continued_at_eof = false;

    for physical in text.split('\n') {
        let physical = physical.strip_suffix('\r').unwrap_or(physical);
        logical.push_str(physical);
        if config_line_continues(&logical) {
            logical.pop();
            continued_at_eof = true;
            continue;
        }
        continued_at_eof = false;
        let line = std::mem::take(&mut logical);
        let mut trimmed = trim_config_line(&line);
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(after_open) = trimmed.strip_prefix('[') {
            let Some((raw_section, rest)) = after_open.split_once(']') else {
                return Err(LfsConfigError::LfsConfigSyntax);
            };
            section = Some(parse_lfsconfig_section(raw_section)?);
            trimmed = trim_config_line(rest);
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
        }
        let Some((section_name, subsection)) = section.as_ref() else {
            return Err(LfsConfigError::LfsConfigSyntax);
        };
        let (raw_key, raw_value, implicit_bool) = trimmed
            .split_once('=')
            .map(|(key, value)| (key.trim(), value.trim_start_matches([' ', '\t']), false))
            .unwrap_or((trimmed.trim(), "", true));
        if !valid_lfsconfig_key(raw_key) {
            return Err(LfsConfigError::LfsConfigSyntax);
        }
        let value = if implicit_bool {
            "true".to_owned()
        } else {
            parse_lfsconfig_value(raw_value)?
        };
        if value.len() > MAX_FETCH_VALUE_BYTES {
            return Err(LfsConfigError::LfsConfigTooLarge);
        }
        let name = if subsection.is_empty() {
            format!("{section_name}.{raw_key}")
        } else {
            format!("{section_name}.{subsection}.{raw_key}")
        };
        pairs.push((name, value));
    }
    if continued_at_eof || !logical.is_empty() {
        return Err(LfsConfigError::LfsConfigSyntax);
    }
    parse_safe_lfsconfig(pairs).map_err(|_| LfsConfigError::LfsConfigUnsafeKey)
}

fn config_line_continues(line: &str) -> bool {
    let mut backslashes = 0;
    for byte in line.as_bytes().iter().rev() {
        if *byte != b'\\' {
            break;
        }
        backslashes += 1;
    }
    backslashes % 2 == 1
}

fn trim_config_line(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\r'])
}

fn parse_lfsconfig_section(raw: &str) -> Result<(String, String), LfsConfigError> {
    let raw = raw.trim();
    let (raw_name, raw_subsection) = raw
        .find(|character: char| character.is_whitespace())
        .map_or((raw, None), |index| {
            (&raw[..index], Some(raw[index..].trim()))
        });
    let name = raw_name.to_ascii_lowercase();
    if !matches!(name.as_str(), "lfs" | "remote") {
        return Err(LfsConfigError::LfsConfigSyntax);
    }
    let Some(raw_subsection) = raw_subsection else {
        return Ok((name, String::new()));
    };
    let subsection = parse_quoted_subsection(raw_subsection)?;
    if subsection.is_empty() {
        return Err(LfsConfigError::LfsConfigSyntax);
    }
    Ok((name, subsection))
}

fn parse_quoted_subsection(raw: &str) -> Result<String, LfsConfigError> {
    let Some(mut chars) = raw.strip_prefix('"').map(str::chars) else {
        return Err(LfsConfigError::LfsConfigSyntax);
    };
    let mut subsection = String::new();
    let mut escaped = false;
    let mut closed = false;
    while let Some(character) = chars.next() {
        if escaped {
            subsection.push(match character {
                '"' => '"',
                '\\' => '\\',
                _ => return Err(LfsConfigError::LfsConfigSyntax),
            });
            escaped = false;
        } else {
            match character {
                '\\' => escaped = true,
                '"' => {
                    closed = true;
                    break;
                }
                _ => subsection.push(character),
            }
        }
    }
    if escaped || !closed || !chars.all(|character| character.is_ascii_whitespace()) {
        return Err(LfsConfigError::LfsConfigSyntax);
    }
    if subsection.chars().any(|character| character.is_control()) {
        return Err(LfsConfigError::LfsConfigControl);
    }
    Ok(subsection)
}

fn valid_lfsconfig_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn parse_lfsconfig_value(value: &str) -> Result<String, LfsConfigError> {
    let mut parsed = String::with_capacity(value.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut keep_len = 0;
    for character in value.chars() {
        if escaped {
            parsed.push(match character {
                'n' => '\n',
                't' => '\t',
                'b' => '\u{0008}',
                '"' => '"',
                '\\' => '\\',
                _ => return Err(LfsConfigError::LfsConfigSyntax),
            });
            keep_len = parsed.len();
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => break,
            _ => {
                parsed.push(character);
                if quoted || !matches!(character, ' ' | '\t' | '\r') {
                    keep_len = parsed.len();
                }
            }
        }
    }
    if quoted || escaped {
        return Err(LfsConfigError::LfsConfigSyntax);
    }
    parsed.truncate(keep_len);
    if parsed.chars().any(char::is_control) {
        return Err(LfsConfigError::LfsConfigControl);
    }
    Ok(parsed)
}

fn validate_base_path(path: &Path) -> Result<(), LfsConfigError> {
    if !path.is_absolute() || path.as_os_str().is_empty() {
        return Err(LfsConfigError::InvalidPath);
    }
    validate_path_text(path, LfsConfigError::InvalidPath)
}

fn validate_text(value: &str, error: LfsConfigError) -> Result<(), LfsConfigError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(error);
    }
    Ok(())
}

fn validate_path_text(path: &Path, error: LfsConfigError) -> Result<(), LfsConfigError> {
    if path.to_string_lossy().chars().any(char::is_control) {
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn validate_windows_trusted_worktree_components(path: &Path) -> Result<(), LfsConfigError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        let inspect = match component {
            Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
                false
            }
            Component::RootDir => {
                current.push(Path::new(std::path::MAIN_SEPARATOR_STR));
                true
            }
            Component::CurDir => false,
            Component::ParentDir => return Err(LfsConfigError::UntrustedWorktreeRoot),
            Component::Normal(value) => {
                current.push(value);
                true
            }
        };
        if !inspect {
            continue;
        }
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        let directory = match options.open(&current) {
            Ok(directory) => directory,
            Err(error) => {
                if matches!(windows_path_is_reparse(&current), Ok(true)) {
                    return Err(LfsConfigError::UntrustedWorktreeRoot);
                }
                return Err(LfsConfigError::LfsConfigIo(error.kind()));
            }
        };
        let information = windows_handle_information(&directory)?;
        if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || unsafe { GetFileType(directory.as_raw_handle() as _) } != FILE_TYPE_DISK
        {
            return Err(LfsConfigError::UntrustedWorktreeRoot);
        }
    }
    Ok(())
}

fn validate_no_symlink_components(
    path: &Path,
    error: LfsConfigError,
) -> Result<(), LfsConfigError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::ParentDir => return Err(error),
            Component::Normal(value) => {
                current.push(value);
                match fs::symlink_metadata(&current) {
                    Ok(metadata)
                        if metadata.file_type().is_symlink()
                            && !is_trusted_system_symlink(&current, &metadata) =>
                    {
                        return Err(error);
                    }
                    Ok(_) => {}
                    Err(error_value) if error_value.kind() == io::ErrorKind::NotFound => break,
                    Err(_) => return Err(error),
                }
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn is_trusted_system_symlink(path: &Path, metadata: &fs::Metadata) -> bool {
    if metadata.uid() != 0 {
        return false;
    }
    let target = match fs::read_link(path) {
        Ok(target) => target,
        Err(_) => return false,
    };
    (path == Path::new("/var") && matches!(target.to_str(), Some("/private/var" | "private/var")))
        || (path == Path::new("/tmp")
            && matches!(target.to_str(), Some("/private/tmp" | "private/tmp")))
}

#[cfg(not(target_os = "macos"))]
fn is_trusted_system_symlink(_path: &Path, _metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn entry(name: &str, value: &str) -> ConfigEntry {
        let (section, subsection, key) = super::super::parse_config_name(name).expect("config key");
        ConfigEntry {
            raw_section: section.clone(),
            section,
            subsection,
            raw_key: key.clone(),
            key,
            value: value.to_owned(),
            comment: None,
            implicit_bool: false,
            scope: super::super::ConfigScope::Local,
            origin: "test".to_owned(),
            line: None,
        }
    }

    fn input<'a>(
        git_dir: &'a Path,
        repository_root: &'a Path,
        entries: &'a [ConfigEntry],
    ) -> LfsRuntimeConfigInput<'a> {
        LfsRuntimeConfigInput {
            git_dir,
            default_storage_git_dir: git_dir,
            lfsconfig: LfsConfigSources::new(
                Some(TrustedWorktreeRoot::new(repository_root).expect("trusted worktree")),
                None,
                None,
            ),
            entries,
            branch: Some("main"),
            requested_remote: None,
            skip_smudge: None,
            skip_download_errors: None,
            http_environment: LfsHttpEnvironmentSnapshot::default(),
            fetch_head: None,
            url_rewriter: None,
        }
    }

    fn temporary_directory() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("zmin-lfs-config-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("temporary directory");
        path
    }

    #[test]
    fn concurrent_transfers_defaults_to_eight_and_accepts_git_integer_forms() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let default = LfsRuntimeConfig::load(input(&git_dir, &root, &[])).expect("default config");
        assert_eq!(default.concurrent_transfers().get(), 8);

        for value in ["1", "2", "8", "+2", "02"] {
            let entries = vec![entry("lfs.concurrentTransfers", value)];
            let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries))
                .expect("accepted concurrency");
            assert_eq!(
                config.concurrent_transfers().get(),
                value.parse::<usize>().expect("test integer")
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_transfers_rejects_every_explicit_invalid_value_without_echo() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        for value in ["0", "-1", "9", "not-a-number", "184467440737095516160"] {
            let entries = vec![entry("lfs.concurrentTransfers", value)];
            let error = LfsRuntimeConfig::load(input(&git_dir, &root, &entries))
                .expect_err("invalid concurrency");
            assert_eq!(error, LfsConfigError::InvalidConcurrentTransfers);
            let rendered = format!("{error:?} {error}");
            assert!(!rendered.contains(value));
        }

        let entries = vec![
            entry("lfs.concurrentTransfers", "9"),
            entry("lfs.concurrentTransfers", "2"),
        ];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries))
            .expect("last ordinary value wins");
        assert_eq!(config.concurrent_transfers().get(), 2);
        let entries = vec![
            entry("lfs.concurrentTransfers", "2"),
            entry("lfs.concurrentTransfers", "9"),
        ];
        assert_eq!(
            LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect_err("invalid winner"),
            LfsConfigError::InvalidConcurrentTransfers
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configured_transport_timeouts_are_assembled_into_http_policy() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![
            entry("lfs.dialTimeout", "30"),
            entry("lfs.tlsTimeout", "31"),
            entry("lfs.activityTimeout", "32"),
            entry("lfs.https://example.test/repo.activityTimeout", "33"),
        ];
        LfsRuntimeConfig::load(input(&git_dir, &root, &entries))
            .expect("supported transport timeout policy");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_transfers_is_not_accepted_from_lfsconfig() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[lfs]\nconcurrenttransfers = 2\n",
        )
        .expect("lfsconfig");
        assert_eq!(
            LfsRuntimeConfig::load(input(&git_dir, &root, &[])).expect_err("unsafe lfsconfig key"),
            LfsConfigError::LfsConfigUnsafeKey
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn credential_use_http_path_uses_global_and_longest_url_scope_precedence() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![
            entry("credential.useHttpPath", "false"),
            entry("credential.https://example.test.useHttpPath", "true"),
            entry("credential.https://example.test/team.useHttpPath", "false"),
            entry(
                "credential.https://example.test/team/repo.useHttpPath",
                "true",
            ),
        ];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect("config");
        let root_endpoint = parse_http_url("https://example.test/other/info/lfs").expect("url");
        let team_endpoint =
            parse_http_url("https://example.test/team/other/info/lfs").expect("url");
        let repo_endpoint = parse_http_url("https://example.test/team/repo/info/lfs").expect("url");
        assert!(
            config
                .credential_use_http_path_for(&root_endpoint)
                .expect("root policy")
        );
        assert!(
            !config
                .credential_use_http_path_for(&team_endpoint)
                .expect("team policy")
        );
        assert!(
            config
                .credential_use_http_path_for(&repo_endpoint)
                .expect("repository policy")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn credential_use_http_path_matches_default_git_lfs_endpoint_without_git_suffix() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![entry(
            "credential.https://example.test/team/repo.useHttpPath",
            "true",
        )];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect("config");
        let endpoint =
            parse_http_url("https://example.test/team/repo.git/info/lfs").expect("endpoint");
        assert!(
            config
                .credential_use_http_path_for(&endpoint)
                .expect("credential policy")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn credential_use_http_path_matches_decoded_percent_path_bytes() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![
            entry("credential.useHttpPath", "false"),
            entry(
                "credential.https://example.test/team/%72epo.useHttpPath",
                "true",
            ),
            entry(
                "credential.https://bytes.example/team/%FF.useHttpPath",
                "true",
            ),
        ];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect("config");
        let repository =
            parse_http_url("https://example.test/team/repo.git/info/lfs").expect("repository");
        let non_utf8 =
            parse_http_url("https://bytes.example/team/%ff/info/lfs").expect("byte path");
        assert!(
            config
                .credential_use_http_path_for(&repository)
                .expect("repository policy")
        );
        assert!(
            config
                .credential_use_http_path_for(&non_utf8)
                .expect("byte path policy")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn credential_use_http_path_rejects_malformed_and_nul_paths_without_echo() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        for scope in ["%x0-private-secret", "%00-private-secret"] {
            let entries = vec![entry(
                &format!("credential.https://example.test/team/{scope}.useHttpPath"),
                "true",
            )];
            let error = LfsRuntimeConfig::load(input(&git_dir, &root, &entries))
                .expect_err("invalid credential scope");
            assert_eq!(error, LfsConfigError::InvalidCredentialPolicy);
            let rendered = format!("{error:?} {error}");
            assert!(!rendered.contains("private-secret"));
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn credential_policy_and_runtime_debug_redact_scoped_url_secrets() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![entry(
            "credential.https://private-user:private-password@example.test/private-path.useHttpPath",
            "true",
        )];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect("config");
        let rendered = format!("{config:?}");
        for secret in ["private-user", "private-password", "private-path"] {
            assert!(!rendered.contains(secret));
        }
        assert!(rendered.contains("scoped_count: 1"));
        let _ = fs::remove_dir_all(root);
    }

    struct TestUrlRewriter;

    impl LfsRemoteUrlRewriter for TestUrlRewriter {
        fn rewrite(&self, url: &str, push: bool) -> Result<String, LfsConfigError> {
            Ok(format!(
                "rewritten:{}:{url}",
                if push { "push" } else { "fetch" }
            ))
        }
    }

    #[test]
    fn built_in_url_rewriter_uses_longest_match_and_push_priority() {
        let entries = vec![
            entry("url.https://mirror.test/.insteadof", "gh:"),
            entry("url.https://repo.test/.insteadof", "gh:org/"),
            entry("url.ssh://push.test/.pushinsteadof", "gh:org/"),
        ];
        let rewriter = GitUrlInsteadOfRewriter::from_entries(&entries).expect("rules");
        assert_eq!(
            rewriter.rewrite("gh:org/repository.git", false),
            Ok("https://repo.test/repository.git".to_owned())
        );
        assert_eq!(
            rewriter.rewrite("gh:org/repository.git", true),
            Ok("ssh://push.test/repository.git".to_owned())
        );
        assert_eq!(
            rewriter.rewrite("other:repository.git", true),
            Ok("other:repository.git".to_owned())
        );
    }

    #[test]
    fn runtime_load_applies_url_aliases_when_no_callback_is_supplied() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![
            entry("remote.origin.url", "gh:org/repository.git"),
            entry("url.https://mirror.test/.insteadof", "gh:"),
        ];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect("config");
        assert_eq!(
            config.endpoint_inputs().remotes[0].url.as_deref(),
            Some("https://mirror.test/org/repository.git")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn normal_config_overrides_lfsconfig_and_assembles_remote_context() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[lfs]\nurl = https://repo.example/lfs\nfetchinclude = src/**\n",
        )
        .expect("lfsconfig");
        let entries = vec![
            entry("lfs.url", "https://normal.example/lfs"),
            entry("remote.origin.url", "https://github.example/repo.git"),
            entry("branch.main.remote", "origin"),
            entry("remote.pushDefault", "origin"),
            entry("lfs.fetchExclude", "src/private/**"),
        ];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect("config");
        assert_eq!(
            config.endpoint_inputs().lfs_url.as_deref(),
            Some("https://normal.example/lfs")
        );
        assert_eq!(
            config.endpoint_inputs().current_remote.as_deref(),
            Some("origin")
        );
        assert_eq!(
            config.endpoint_inputs().push_default_remote.as_deref(),
            Some("origin")
        );
        assert!(config.fetch_filter().allows("src/main.c"));
        assert_eq!(config.fetch_filter().allows("src/private/secret.c"), false);
        assert_eq!(
            config.storage(),
            git_dir.join(DEFAULT_LFS_STORAGE_NAME).as_path()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relative_storage_cannot_escape_git_directory() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![entry("lfs.storage", "../outside")];
        assert!(matches!(
            LfsRuntimeConfig::load(input(&git_dir, &root, &entries)),
            Err(LfsConfigError::InvalidStorage)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn linked_worktree_relative_storage_uses_common_git_dir() {
        let root = temporary_directory();
        let common_git_dir = root.join(".git");
        let worktree_git_dir = common_git_dir.join("worktrees").join("linked");
        let worktree = root.join("linked");
        fs::create_dir_all(&worktree_git_dir).expect("worktree admin directory");
        fs::create_dir_all(&worktree).expect("linked worktree");

        let mut default_input = input(&worktree_git_dir, &worktree, &[]);
        default_input.default_storage_git_dir = &common_git_dir;
        let default = LfsRuntimeConfig::load(default_input).expect("default storage");
        assert_eq!(default.storage(), common_git_dir.join("lfs"));

        let entries = [entry("lfs.storage", "custom-lfs")];
        let mut custom_input = input(&worktree_git_dir, &worktree, &entries);
        custom_input.default_storage_git_dir = &common_git_dir;
        let custom = LfsRuntimeConfig::load(custom_input).expect("custom storage");
        assert_eq!(custom.storage(), common_git_dir.join("custom-lfs"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_precedence_and_url_alias_callback_are_typed() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![
            entry("remote.origin.url", "alias:read"),
            entry("remote.origin.pushurl", "alias:write"),
            entry("remote.origin.lfsurl", "https://read-lfs.example/"),
            entry("remote.origin.lfspushurl", "https://write-lfs.example/"),
            entry("branch.main.remote", "origin"),
            entry("branch.main.pushRemote", "publish"),
            entry("remote.lfsdefault", "origin"),
            entry("remote.lfspushdefault", "publish"),
        ];
        let rewriter = TestUrlRewriter;
        let mut config_input = input(&git_dir, &root, &entries);
        config_input.fetch_head =
            Some("1111111111111111111111111111111111111111\t\tbranch 'main' of alias:fetch\n");
        config_input.url_rewriter = Some(&rewriter);
        let config = LfsRuntimeConfig::load(config_input).expect("config");
        let remote = config.endpoint_inputs().remote("origin").expect("origin");
        assert_eq!(remote.url.as_deref(), Some("rewritten:fetch:alias:read"));
        assert_eq!(
            remote.push_url.as_deref(),
            Some("rewritten:push:alias:write")
        );
        assert_eq!(remote.lfs_url.as_deref(), Some("https://read-lfs.example/"));
        assert_eq!(
            remote.lfs_push_url.as_deref(),
            Some("https://write-lfs.example/")
        );
        assert_eq!(
            config.endpoint_inputs().current_push_remote.as_deref(),
            Some("publish")
        );
        assert_eq!(
            config.endpoint_inputs().fetch_head_url.as_deref(),
            Some("rewritten:fetch:alias:fetch")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fetch_filter_uses_upstream_include_exclude_or_semantics() {
        let filter = LfsFetchFilter::from_values(
            Some("src/**,docs/*.md,!literal/**"),
            Some("vendor/**,!blocked/**"),
        )
        .expect("filter");
        assert!(filter.allows("src/main.c"));
        assert!(filter.allows("src/private/key.txt"));
        assert!(filter.allows("docs/readme.md"));
        assert!(!filter.allows("docs/readme.txt"));
        assert!(!filter.allows("vendor/drop.txt"));
        assert!(!filter.allows("vendor/keep/file.txt"));
        assert!(filter.allows("!literal/file.bin"));
        assert!(!filter.allows("src\\main.c"));
    }

    #[test]
    fn forbidden_lfsconfig_keys_are_rejected_without_redaction_leaks() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[core]\neditor = https://user:secret@example.test/token\n",
        )
        .expect("lfsconfig");
        let error = LfsRuntimeConfig::load(input(&git_dir, &root, &[])).expect_err("unsafe key");
        let text = error.to_string();
        assert!(!text.contains("editor"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("example.test"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lfs_access_scope_sections_are_case_insensitive_but_preserve_scope() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[LfS \"HTTPS://example.test/lfs\"]\nAccess = basic\n",
        )
        .expect("lfsconfig");
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &[])).expect("config");
        assert_eq!(
            config
                .endpoint_inputs()
                .lfsconfig
                .value("lfs.HTTPS://example.test/lfs.access"),
            Some("basic")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lfs_access_scope_rejects_invalid_url_without_echoing_scope() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[lfs \"https://example.test/#secret\"]\naccess = basic\n",
        )
        .expect("lfsconfig");
        let error = LfsRuntimeConfig::load(input(&git_dir, &root, &[])).expect_err("invalid scope");
        assert_eq!(error, LfsConfigError::LfsConfigUnsafeKey);
        assert!(!error.to_string().contains("secret"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lfsconfig_size_utf8_and_control_bounds_are_enforced() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        fs::write(
            root.join(LFS_CONFIG_NAME),
            vec![b'x'; (MAX_LFS_CONFIG_BYTES + 1) as usize],
        )
        .expect("large lfsconfig");
        assert!(matches!(
            LfsRuntimeConfig::load(input(&git_dir, &root, &[])),
            Err(LfsConfigError::LfsConfigTooLarge)
        ));
        fs::write(root.join(LFS_CONFIG_NAME), [0xff, 0xfe]).expect("invalid utf8");
        assert!(matches!(
            LfsRuntimeConfig::load(input(&git_dir, &root, &[])),
            Err(LfsConfigError::LfsConfigNotUtf8)
        ));
        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[lfs]\nurl = https://safe.example/\x01\n",
        )
        .expect("control lfsconfig");
        assert!(matches!(
            LfsRuntimeConfig::load(input(&git_dir, &root, &[])),
            Err(LfsConfigError::LfsConfigControl)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn lfsconfig_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let secret = root.join("secret-lfsconfig");
        fs::write(&secret, b"[lfs]\nurl = https://secret.example/\n").expect("secret");
        symlink(&secret, root.join(LFS_CONFIG_NAME)).expect("symlink");
        assert!(matches!(
            LfsRuntimeConfig::load(input(&git_dir, &root, &[])),
            Err(LfsConfigError::LfsConfigSymlink)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn lfsconfig_windows_uses_same_guarded_handle_for_regular_and_reparse_files() {
        use std::os::windows::fs::symlink_file;

        let root = temporary_directory();
        let regular = root.join(LFS_CONFIG_NAME);
        fs::write(&regular, b"[lfs]\nurl = https://safe.example/\n").expect("lfsconfig");
        let snapshot = read_lfsconfig_snapshot(&regular)
            .expect("guarded read")
            .expect("regular file");
        assert_eq!(snapshot.bytes, b"[lfs]\nurl = https://safe.example/\n");

        let target = root.join("real-lfsconfig");
        let link = root.join("reparse-lfsconfig");
        fs::write(&target, b"[lfs]\nurl = https://secret.example/\n").expect("target");
        // Wine can report a successful symlink call without materializing an
        // entry.  Exercise the guard whenever the reparse fixture is visible.
        if symlink_file(&target, &link).is_ok() && fs::symlink_metadata(&link).is_ok() {
            assert!(matches!(
                read_lfsconfig_snapshot(&link),
                Err(LfsConfigError::LfsConfigSymlink)
            ));
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fetch_head_parser_validates_oid_status_and_description() {
        let content = "1111111111111111111111111111111111111111\t\tbranch 'main' of https://example.test/repo.git\n";
        assert_eq!(
            parse_fetch_head_url(content)
                .expect("fetch head")
                .as_deref(),
            Some("https://example.test/repo.git")
        );
        let sha256 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\tnot-for-merge\ttag 'v1' of ssh://example.test/repo.git\n";
        assert_eq!(
            parse_fetch_head_url(sha256).expect("fetch head").as_deref(),
            Some("ssh://example.test/repo.git")
        );
        assert_eq!(parse_fetch_head_url("\n\r\n").expect("no URL"), None);
        assert!(matches!(
            parse_fetch_head_url(
                "1111111111111111111111111111111111111111\tmaybe\tbranch 'main' of https://example.test/\n"
            ),
            Err(LfsConfigError::InvalidFetchHead)
        ));
        assert!(matches!(
            parse_fetch_head_url("not-for-merge"),
            Err(LfsConfigError::InvalidFetchHead)
        ));
        assert!(matches!(
            parse_fetch_head_url(
                "1111111111111111111111111111111111111111\t\tbranch of https://example.test/\x01\n"
            ),
            Err(LfsConfigError::InvalidFetchHead)
        ));
    }

    #[test]
    fn skip_smudge_is_caller_supplied_and_strict() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let mut config_input = input(&git_dir, &root, &[]);
        config_input.skip_smudge = Some("yes");
        assert!(
            LfsRuntimeConfig::load(config_input.clone())
                .expect("config")
                .skip_smudge()
        );
        config_input.skip_smudge = Some("maybe");
        assert!(matches!(
            LfsRuntimeConfig::load(config_input),
            Err(LfsConfigError::InvalidSkipSmudge)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lfsconfig_sources_follow_worktree_index_head_and_bare_precedence() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let index = b"[lfs]\nurl = https://index.example/\n";
        let head = b"[lfs]\nurl = https://head.example/\n";

        let mut config_input = input(&git_dir, &root, &[]);
        config_input.lfsconfig = LfsConfigSources::new(None, Some(index), Some(head));
        let bare = LfsRuntimeConfig::load(config_input.clone()).expect("bare config");
        assert_eq!(
            bare.endpoint_inputs().lfsconfig.lfs_url(),
            Some("https://head.example/")
        );
        assert_eq!(
            bare.endpoint_inputs().lfsconfig.value("lfs.url"),
            Some("https://head.example/")
        );

        config_input.lfsconfig = LfsConfigSources::new(
            Some(TrustedWorktreeRoot::new(&root).expect("trusted worktree")),
            Some(index),
            Some(head),
        );
        let from_index = LfsRuntimeConfig::load(config_input.clone()).expect("index config");
        assert_eq!(
            from_index.endpoint_inputs().lfsconfig.value("lfs.url"),
            Some("https://index.example/")
        );

        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[lfs]\nurl = https://worktree.example/\n",
        )
        .expect("worktree lfsconfig");
        let from_worktree = LfsRuntimeConfig::load(config_input.clone()).expect("worktree config");
        assert_eq!(
            from_worktree.endpoint_inputs().lfsconfig.value("lfs.url"),
            Some("https://worktree.example/")
        );

        fs::write(root.join(LFS_CONFIG_NAME), b"").expect("empty worktree lfsconfig");
        let empty = LfsRuntimeConfig::load(config_input).expect("empty worktree config");
        assert_eq!(empty.endpoint_inputs().lfsconfig.value("lfs.url"), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trusted_worktree_root_is_canonical_typed_and_redacted() {
        let root = temporary_directory();
        let trusted = TrustedWorktreeRoot::new(&root.join(".")).expect("trusted worktree");
        assert_eq!(
            trusted.as_path(),
            fs::canonicalize(&root).expect("canonical worktree")
        );
        assert_eq!(format!("{trusted:?}"), "TrustedWorktreeRoot(<redacted>)");
        assert!(matches!(
            TrustedWorktreeRoot::new(Path::new("relative-worktree")),
            Err(LfsConfigError::InvalidPath)
        ));

        let regular = root.join("regular");
        fs::write(&regular, b"not a directory").expect("regular file");
        assert!(matches!(
            TrustedWorktreeRoot::new(&regular),
            Err(LfsConfigError::UntrustedWorktreeRoot)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn trusted_worktree_root_rejects_reparse_parent() {
        use std::os::windows::fs::symlink_dir;

        let root = temporary_directory();
        let real_parent = root.join("real-parent");
        let repository = real_parent.join("repository");
        fs::create_dir_all(&repository).expect("repository");
        let reparse_parent = root.join("reparse-parent");
        // As above, require the compatibility layer to have materialized the
        // entry before treating this as a usable reparse fixture.
        match symlink_dir(&real_parent, &reparse_parent) {
            Ok(()) if fs::symlink_metadata(&reparse_parent).is_ok() => {
                assert!(matches!(
                    TrustedWorktreeRoot::new(&reparse_parent.join("repository")),
                    Err(LfsConfigError::UntrustedWorktreeRoot)
                ));
            }
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                ) => {}
            Err(error) => panic!("create directory reparse point: {error}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn current_network_policies_use_normal_config_then_safe_lfsconfig() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        fs::write(
            root.join(LFS_CONFIG_NAME),
            b"[lfs]\nallowincompletepush = true\nlocksverify = true\nskipdownloaderrors = true\n",
        )
        .expect("lfsconfig");
        let entries = vec![
            entry("lfs.remote.autodetect", "yes"),
            entry("lfs.remote.searchall", "on"),
            entry("lfs.locksverify", "false"),
        ];
        let mut config_input = input(&git_dir, &root, &entries);
        config_input.skip_download_errors = Some("yes");
        let config = LfsRuntimeConfig::load(config_input).expect("config");
        assert!(config.remote_policy().autodetect());
        assert!(config.remote_policy().search_all());
        assert!(config.allow_incomplete_push());
        assert_eq!(config.locks_verify(), Some(false));
        assert!(config.skip_download_errors());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lfsconfig_blob_is_parsed_as_one_exact_snapshot() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let blob = b"[lfs]\nfetchinclude = src/**\n";
        let config_input = LfsRuntimeConfigInput {
            git_dir: &git_dir,
            default_storage_git_dir: &git_dir,
            lfsconfig: LfsConfigSources::new(None, None, Some(blob)),
            entries: &[],
            branch: None,
            requested_remote: None,
            skip_smudge: None,
            skip_download_errors: None,
            http_environment: LfsHttpEnvironmentSnapshot::default(),
            fetch_head: None,
            url_rewriter: None,
        };
        let config = LfsRuntimeConfig::load(config_input).expect("blob config");
        assert!(config.fetch_filter().allows("src/file.bin"));
        assert!(!config.fetch_filter().allows("docs/file.bin"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn same_config_is_independent_of_repository_hash_algorithm() {
        let sha1_root = temporary_directory();
        let sha256_root = temporary_directory();
        let sha1_git = sha1_root.join(".git");
        let sha256_git = sha256_root.join(".git");
        fs::create_dir_all(&sha1_git).expect("sha1 git directory");
        fs::create_dir_all(&sha256_git).expect("sha256 git directory");
        let entries = vec![entry("remote.origin.url", "https://example.test/repo.git")];
        let sha1 = LfsRuntimeConfig::load(input(&sha1_git, &sha1_root, &entries)).expect("sha1");
        let sha256 =
            LfsRuntimeConfig::load(input(&sha256_git, &sha256_root, &entries)).expect("sha256");
        assert_eq!(sha1.endpoint_inputs(), sha256.endpoint_inputs());
        assert_eq!(sha1.fetch_filter(), sha256.fetch_filter());
        let _ = fs::remove_dir_all(sha1_root);
        let _ = fs::remove_dir_all(sha256_root);
    }

    #[test]
    fn access_policy_uses_normal_precedence_and_longest_scope() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let blob = b"[lfs \"https://example.test/team\"]\naccess = none\n";
        let entries = vec![
            entry("lfs.https://example.test.access", "none"),
            entry("lfs.https://example.test/team/repo.access", "basic"),
        ];
        let config = LfsRuntimeConfig::load(LfsRuntimeConfigInput {
            git_dir: &git_dir,
            default_storage_git_dir: &git_dir,
            lfsconfig: LfsConfigSources::new(None, None, Some(blob)),
            entries: &entries,
            branch: None,
            requested_remote: None,
            skip_smudge: None,
            skip_download_errors: None,
            http_environment: LfsHttpEnvironmentSnapshot::default(),
            fetch_head: None,
            url_rewriter: None,
        })
        .expect("config");
        assert_eq!(
            config.access_for(
                &parse_http_url("HTTPS://EXAMPLE.TEST:443/team/repo/info/lfs").expect("endpoint")
            ),
            LfsAccessMode::Basic
        );
        assert_eq!(
            config.access_for(
                &parse_http_url("https://example.test/team/other/info/lfs").expect("endpoint")
            ),
            LfsAccessMode::None
        );
        assert_eq!(
            config.access_for(
                &parse_http_url("https://unmatched.example/info/lfs").expect("endpoint")
            ),
            LfsAccessMode::Unspecified
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn locks_policy_uses_longest_url_scope_then_global_fallback() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let entries = vec![
            entry("lfs.locksverify", "false"),
            entry("lfs.https://example.test/team.locksverify", "true"),
            entry("lfs.https://example.test/team/repo.locksverify", "false"),
        ];
        let config = LfsRuntimeConfig::load(input(&git_dir, &root, &entries)).expect("config");
        let team =
            parse_http_url("https://example.test/team/other/objects/batch").expect("team endpoint");
        let repo =
            parse_http_url("https://example.test/team/repo/objects/batch").expect("repo endpoint");
        let other = parse_http_url("https://other.test/objects/batch").expect("other endpoint");
        assert_eq!(config.locks_verify_for(&team), Some(true));
        assert_eq!(config.locks_verify_for(&repo), Some(false));
        assert_eq!(config.locks_verify_for(&other), Some(false));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_locks_scope_fails_closed_without_echoing_url() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        let secret = "https://user:secret@example.test/team";
        let entries = [entry(&format!("lfs.{secret}.locksverify"), "true")];
        let error = LfsRuntimeConfig::load(input(&git_dir, &root, &entries))
            .expect_err("unsafe lock scope");
        assert!(matches!(error, LfsConfigError::InvalidLocksPolicy));
        assert!(!error.to_string().contains("secret"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_access_policy_and_scope_fail_closed() {
        let root = temporary_directory();
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        assert!(matches!(
            LfsRuntimeConfig::load(input(
                &git_dir,
                &root,
                &[entry("lfs.https://example.test.access", "ntlm")]
            )),
            Err(LfsConfigError::InvalidAccessPolicy)
        ));
        assert!(matches!(
            LfsRuntimeConfig::load(input(
                &git_dir,
                &root,
                &[entry("lfs.https://user@example.test.access", "basic")]
            )),
            Err(LfsConfigError::InvalidAccessPolicy)
        ));
        assert!(matches!(
            LfsRuntimeConfig::load(input(
                &git_dir,
                &root,
                &[entry("lfs.https://example.test/path?token.access", "basic")]
            )),
            Err(LfsConfigError::InvalidAccessPolicy)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fetch_filter_accepts_non_utf8_repository_path_bytes() {
        let filter = LfsFetchFilter::from_values(None, Some("private/**")).expect("filter");
        assert!(filter.allows_bytes(b"assets/\xff.bin"));
        assert!(!filter.allows_bytes(b"private/\xff.bin"));
        assert!(filter.allows_bytes(b"assets/bad\\name"));
        assert!(filter.allows_bytes(b"assets/tab\tname"));
        assert!(filter.allows_bytes(b"assets/line\nname"));
    }

    #[test]
    fn fetch_filter_matches_raw_bytes_without_utf8_replacement() {
        let one_byte = LfsFetchFilter::from_values(Some("assets/?.bin"), None).expect("filter");
        #[cfg(any(target_os = "macos", windows))]
        assert!(!one_byte.allows_bytes(b"assets/\xff.bin"));
        #[cfg(not(any(target_os = "macos", windows)))]
        assert!(one_byte.allows_bytes(b"assets/\xff.bin"));
        assert!(!one_byte.allows_bytes("assets/�.bin".as_bytes()));

        let any_bytes = LfsFetchFilter::from_values(Some("assets/*.bin"), None).expect("filter");
        assert!(any_bytes.allows_bytes(b"assets/\xff.bin"));
        let replacement =
            LfsFetchFilter::from_values(Some("assets/[�].bin"), None).expect("filter");
        #[cfg(any(target_os = "macos", windows))]
        assert!(replacement.allows_bytes(b"assets/\xff.bin"));
        #[cfg(not(any(target_os = "macos", windows)))]
        assert!(replacement.allows_bytes("assets/�.bin".as_bytes()));

        let empty = LfsFetchFilter::from_values(None, None).expect("empty filter");
        for valid in [
            b"back\\slash.bin".as_slice(),
            b"tab\tname.bin".as_slice(),
            b"line\nname.bin".as_slice(),
            b"non-utf8-\xff.bin".as_slice(),
        ] {
            assert!(empty.allows_bytes(valid));
        }
        for invalid in [
            b"".as_slice(),
            b"/leading".as_slice(),
            b"trailing/".as_slice(),
            b"two//components".as_slice(),
            b"nul\0name".as_slice(),
        ] {
            assert!(!empty.allows_bytes(invalid));
        }
    }

    struct WildcardVector<'a> {
        pattern: &'a str,
        path: &'a [u8],
        expected: bool,
    }

    #[test]
    fn fetch_filter_iterative_wildcard_vectors_cover_gitignore_tokens() {
        let vectors = [
            WildcardVector {
                pattern: "**/foo",
                path: b"foo",
                expected: true,
            },
            WildcardVector {
                pattern: "**/foo",
                path: b"deep/tree/foo",
                expected: true,
            },
            WildcardVector {
                pattern: "**/foo",
                path: b"deep/tree/xfoo",
                expected: false,
            },
            WildcardVector {
                pattern: "a/**/b",
                path: b"a/b",
                expected: true,
            },
            WildcardVector {
                pattern: "a/**/b",
                path: b"a/x/y/b",
                expected: true,
            },
            WildcardVector {
                pattern: "a/**/b",
                path: b"a/xb",
                expected: false,
            },
            WildcardVector {
                pattern: "a/**",
                path: b"a/x/y",
                expected: true,
            },
            WildcardVector {
                pattern: r"literal\*.bin",
                path: b"literal*.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "[a-c][!0-9].bin",
                path: b"bq.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "[a-c][!0-9].bin",
                path: b"b7.bin",
                expected: false,
            },
            WildcardVector {
                pattern: "?.bin",
                path: b"\xff.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "foo*",
                path: b"foo/bar.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "foo*",
                path: b"top/foo/bar.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "foo",
                path: b"foobar/file.bin",
                expected: false,
            },
            WildcardVector {
                pattern: "a/*",
                path: b"a/b/file.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "big/b",
                path: b"big/b/b1",
                expected: true,
            },
            WildcardVector {
                pattern: "b",
                path: b"big/b/b1",
                expected: true,
            },
            WildcardVector {
                pattern: r"foo\bar",
                path: b"foo/bar/file.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "[[:alpha:]][[:digit:]].bin",
                path: "ж٥.bin".as_bytes(),
                expected: true,
            },
            WildcardVector {
                pattern: "[[:punct:]].bin",
                path: "。.bin".as_bytes(),
                expected: true,
            },
            WildcardVector {
                pattern: "*[al]?",
                path: b"ball",
                expected: true,
            },
            WildcardVector {
                pattern: "**[!te]",
                path: b"ten",
                expected: true,
            },
            WildcardVector {
                pattern: "**[!ten]",
                path: b"ten",
                expected: false,
            },
            WildcardVector {
                pattern: "[[:]ab]",
                path: b"[ab]",
                expected: true,
            },
            WildcardVector {
                pattern: "[[]ab]",
                path: b"[ab]",
                expected: true,
            },
            WildcardVector {
                pattern: "[a-c[:digit:]x-z]",
                path: b"5",
                expected: true,
            },
            WildcardVector {
                pattern: "[a-c[:digit:]x-z]",
                path: b"q",
                expected: false,
            },
            WildcardVector {
                pattern: r"back\\slash.bin",
                path: b"back\\slash.bin",
                expected: true,
            },
            WildcardVector {
                pattern: "**/*a*b*g*n*t",
                path: b"abcd/abcdefg/abcdefghijk/abcdefghijklmnop.txt",
                expected: true,
            },
            WildcardVector {
                pattern: "**/*a*b*g*n*t",
                path: b"abcd/abcdefg/abcdefghijk/abcdefghijklmnop.txtz",
                expected: false,
            },
        ];
        for vector in vectors {
            let pattern = LfsWildcardPattern::compile(vector.pattern).expect("official vector");
            let work = pattern
                .work_for_text(vector.path.len())
                .expect("bounded official vector work");
            let mut budget = LfsMatchBudget { remaining: work };
            let mut scratch = LfsMatchScratch::default();
            assert_eq!(
                lfs_wildcard_match(&pattern, vector.path, &mut budget, &mut scratch),
                vector.expected,
                "pattern={:?}, path={:?}",
                vector.pattern,
                vector.path
            );
            assert_eq!(budget.remaining, 0);
        }
    }

    #[test]
    fn fetch_filter_worst_case_has_bounded_work_and_two_reused_rows() {
        let pattern = "*".repeat(480);
        let path = vec![b'a'; MAX_FETCH_PATH_BYTES];
        let compiled = LfsWildcardPattern::compile(&pattern).expect("bounded pattern");
        let work = compiled
            .work_for_text(path.len())
            .expect("bounded wildcard work");
        let mut budget = LfsMatchBudget { remaining: work };
        let mut scratch = LfsMatchScratch::default();

        assert!(lfs_wildcard_match(
            &compiled,
            &path,
            &mut budget,
            &mut scratch
        ));
        assert_eq!(budget.remaining, 0);
        assert!(work < MAX_FETCH_MATCH_WORK);
        assert_eq!(
            scratch.previous.len() + scratch.current.len(),
            2 * (path.len() + 1)
        );
        assert_eq!(2 * (path.len() + 1) * size_of::<u8>(), 8_194);

        let filter = LfsFetchFilter::from_values(Some(&pattern), None).expect("bounded filter");
        assert!(filter.allows_bytes(&path));

        let too_expensive = "?".repeat(MAX_FETCH_PATTERN_BYTES);
        assert!(matches!(
            LfsFetchFilter::from_values(Some(&too_expensive), None),
            Err(LfsConfigError::InvalidPattern)
        ));
    }

    #[test]
    fn fetch_filter_class_scan_cost_is_bounded_at_construction() {
        let maximum_factor = MAX_FETCH_MATCH_WORK / MAX_FETCH_MATCH_TEXT_CELLS;
        let empty_class = LfsWildcardPattern::compile("*[]*").expect("empty class");
        let fixed_factor = empty_class.work_per_text_cell();
        let maximum_characters = maximum_factor
            .checked_sub(fixed_factor)
            .and_then(|remaining| remaining.checked_sub(1))
            .expect("class boundary has room for characters");

        let accepted = format!("*[{}]*", "a".repeat(maximum_characters));
        let rejected = format!("*[{}]*", "a".repeat(maximum_characters + 1));
        let accepted_filter =
            LfsFetchFilter::from_values(Some(&accepted), None).expect("boundary class");
        assert!(accepted_filter.max_match_work <= MAX_FETCH_MATCH_WORK);
        let compiled = LfsWildcardPattern::compile(&accepted).expect("compiled boundary class");
        let path = vec![b'z'; MAX_FETCH_PATH_BYTES];
        let work = compiled
            .work_for_text(path.len())
            .expect("bounded class match work");
        let mut budget = LfsMatchBudget { remaining: work };
        let mut scratch = LfsMatchScratch::default();
        assert!(!lfs_wildcard_match(
            &compiled,
            &path,
            &mut budget,
            &mut scratch
        ));
        assert_eq!(budget.remaining, 0);
        assert!(matches!(
            LfsFetchFilter::from_values(Some(&rejected), None),
            Err(LfsConfigError::InvalidPattern)
        ));

        let adversarial = format!("*[{}]*", "a".repeat(MAX_FETCH_PATTERN_BYTES - 5));
        assert!(matches!(
            LfsFetchFilter::from_values(Some(&adversarial), None),
            Err(LfsConfigError::InvalidPattern)
        ));
    }

    #[test]
    fn fetch_filter_class_budget_counts_characters_ranges_and_posix() {
        let empty = LfsWildcardPattern::compile("*[]*")
            .expect("empty class")
            .work_per_text_cell();
        let characters = LfsWildcardPattern::compile("*[abcd]*")
            .expect("characters")
            .work_per_text_cell();
        let range = LfsWildcardPattern::compile("*[a-z]*")
            .expect("range")
            .work_per_text_cell();
        let posix = LfsWildcardPattern::compile("*[[:alpha:]]*")
            .expect("POSIX")
            .work_per_text_cell();

        assert_eq!(characters, empty + 1 + 4);
        assert_eq!(range, empty + 1 + 1);
        assert_eq!(posix, empty + 1 + LFS_POSIX_CLASS_MATCH_WORK);
    }

    #[test]
    fn fetch_filter_rejects_malformed_wildmatch_classes_at_construction() {
        for pattern in ["[", "[!", "[[:unknown:]]", "[[:alpha]"] {
            assert!(matches!(
                LfsFetchFilter::from_values(Some(pattern), None),
                Err(LfsConfigError::InvalidPattern)
            ));
        }
    }

    #[test]
    fn fetch_filter_system_case_matches_pinned_wildmatch_policy() {
        let filter = LfsFetchFilter::from_values(Some("foo/*.bin"), None).expect("filter");
        #[cfg(any(target_os = "macos", windows))]
        assert!(filter.allows("FOO/UPPER.BIN"));
        #[cfg(not(any(target_os = "macos", windows)))]
        assert!(!filter.allows("FOO/UPPER.BIN"));

        let dotted_i = LfsFetchFilter::from_values(Some("i.bin"), None).expect("filter");
        #[cfg(any(target_os = "macos", windows))]
        assert!(dotted_i.allows("İ.bin"));
        #[cfg(not(any(target_os = "macos", windows)))]
        assert!(!dotted_i.allows("İ.bin"));
    }

    #[test]
    fn fetch_filter_preserves_v2_unicode_range_endpoint_quirk() {
        let filter = LfsFetchFilter::from_values(Some("[α-ω].bin"), None).expect("filter");
        assert!(!filter.allows("β.bin"));
    }

    #[test]
    fn fetch_filter_clean_paths_and_root_anchoring_match_git_lfs() {
        let unanchored = LfsFetchFilter::from_values(Some("foo//"), None).expect("filter");
        assert!(unanchored.allows("top/foo/file.bin"));

        let anchored = LfsFetchFilter::from_values(Some("/foo/"), None).expect("filter");
        assert!(anchored.allows("foo/file.bin"));
        assert!(!anchored.allows("top/foo/file.bin"));

        let root_only = LfsFetchFilter::from_values(Some("/"), None).expect("filter");
        assert!(!root_only.allows("foo/file.bin"));
    }
}
