//! Git LFS endpoint discovery and normalization.
//!
//! This module deliberately contains discovery only.  HTTP batch transfers,
//! credentials, and the pure SSH transfer are integration concerns.  Keeping
//! the endpoint resolver independent of the transport makes it possible to
//! validate configuration before a request client is created.
//!
//! The precedence follows the current Git LFS endpoint finder:
//!
//! * `lfs.pushurl` (push only), then `lfs.url`;
//! * the selected remote's `remote.<name>.lfspushurl` (push only), then
//!   `remote.<name>.lfsurl`;
//! * the selected remote's Git URL, derived as an HTTP(S) LFS endpoint.
//!
//! `.lfsconfig` values are a lower-precedence configuration source.  Its
//! endpoint keys are accepted only after the corresponding ordinary Git
//! configuration value is absent.  This mirrors Git LFS's rule that normal
//! Git configuration overrides repository-provided `.lfsconfig` values.

use std::collections::BTreeMap;
use std::fmt;
use std::net::Ipv6Addr;
use std::str::FromStr;

const DEFAULT_REMOTE_NAME: &str = "origin";
const INFO_LFS_SUFFIX: &str = "/info/lfs";
pub(crate) const LFS_HTTP_URL_MAX_BYTES: usize = 8 * 1024;

/// The LFS operation changes both remote selection and URL precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsOperation {
    Fetch,
    Push,
}

impl LfsOperation {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Push => "push",
        }
    }
}

/// Identifies the configuration source that supplied an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsEndpointSource {
    GitConfigGlobal,
    GitConfigRemote,
    LfsConfigGlobal,
    LfsConfigRemote,
    DerivedRemote,
}

/// A validated HTTP(S) endpoint ready for a batch API client.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsEndpoint {
    pub(crate) operation: LfsOperation,
    pub(crate) source: LfsEndpointSource,
    pub(crate) url: LfsHttpUrl,
}

impl fmt::Debug for LfsEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsEndpoint")
            .field("operation", &self.operation)
            .field("source", &self.source)
            .field("url", &"<redacted>")
            .finish()
    }
}

/// A remote SSH URL requires `git-lfs-authenticate` before an HTTP endpoint
/// can be used.  Git LFS discovers the HTTP fallback from the same SSH URL
/// before authentication because a successful response may omit `href`.
/// Keeping that already-validated endpoint in the typed request prevents a
/// later caller from independently guessing or reparsing the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsSshAuthenticationRequest {
    pub(crate) operation: LfsOperation,
    pub(crate) user: Option<String>,
    pub(crate) host: String,
    pub(crate) port: Option<String>,
    pub(crate) path: String,
    pub(crate) default_endpoint: LfsEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LfsEndpointResolution {
    Http(LfsEndpoint),
    SshAuthenticationRequired(LfsSshAuthenticationRequest),
}

/// Ordinary Git remote configuration needed by endpoint discovery.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct LfsRemoteConfig {
    pub(crate) name: String,
    pub(crate) url: Option<String>,
    pub(crate) push_url: Option<String>,
    pub(crate) lfs_url: Option<String>,
    pub(crate) lfs_push_url: Option<String>,
}

impl fmt::Debug for LfsRemoteConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsRemoteConfig")
            .field("name", &self.name)
            .field("url", &self.url.as_ref().map(|_| "<redacted>"))
            .field("push_url", &self.push_url.as_ref().map(|_| "<redacted>"))
            .field("lfs_url", &self.lfs_url.as_ref().map(|_| "<redacted>"))
            .field(
                "lfs_push_url",
                &self.lfs_push_url.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl LfsRemoteConfig {
    pub(crate) fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }
}

/// The subset of `.lfsconfig` that can affect endpoint discovery.
///
/// The `values` map also retains the other official safe keys.  They are not
/// interpreted here, but retaining them lets the integration layer share one
/// parsed, security-checked representation with later LFS features.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct SafeLfsConfig {
    values: BTreeMap<String, String>,
}

impl fmt::Debug for SafeLfsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeLfsConfig")
            .field("key_count", &self.values.len())
            .finish()
    }
}

impl SafeLfsConfig {
    pub(crate) fn parse<I, K, V>(entries: I) -> Result<Self, LfsEndpointError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut values = BTreeMap::new();
        for (raw_key, raw_value) in entries {
            let key = raw_key.into();
            let value = raw_value.into();
            let canonical_key = canonical_lfsconfig_key(&key)?;
            values.insert(canonical_key, value);
        }
        Ok(Self { values })
    }

    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub(crate) fn lfs_url(&self) -> Option<&str> {
        self.value("lfs.url")
    }

    pub(crate) fn lfs_push_url(&self) -> Option<&str> {
        self.value("lfs.pushurl")
    }

    pub(crate) fn lfs_git_protocol(&self) -> Option<&str> {
        self.value("lfs.gitprotocol")
    }

    pub(crate) fn remote_lfs_url(&self, remote: &str) -> Option<&str> {
        let key = format!("remote.{remote}.lfsurl");
        self.value(&key)
    }

    pub(crate) fn access_values(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().filter_map(|(key, value)| {
            key.strip_prefix("lfs.")
                .and_then(|scope| scope.strip_suffix(".access"))
                .map(|scope| (scope, value.as_str()))
        })
    }
}

/// Inputs are intentionally owned so the resolver can be built directly from
/// Git config layers without tying its lifetime to a repository reader.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct LfsEndpointInputs {
    /// Ordinary Git configuration's global `lfs.url`.
    pub(crate) lfs_url: Option<String>,
    /// Ordinary Git configuration's global `lfs.pushurl`.
    pub(crate) lfs_push_url: Option<String>,
    /// Ordinary Git configuration's `lfs.gitprotocol` for `git://` remotes.
    pub(crate) lfs_git_protocol: Option<String>,
    /// Git remote URLs after any `url.*.insteadOf` and
    /// `url.*.pushInsteadOf` rewrites; alias expansion belongs to the caller.
    pub(crate) remotes: Vec<LfsRemoteConfig>,
    /// Explicit remote passed by the caller, equivalent to Git LFS's
    /// `Endpoint(operation, remote)` argument.
    pub(crate) requested_remote: Option<String>,
    /// Remote tracked by the current branch, if any.
    pub(crate) current_remote: Option<String>,
    /// Explicit default remote (`remote.lfsdefault`).
    pub(crate) default_remote: Option<String>,
    /// Remote used by the current branch for pushes, if any.
    pub(crate) current_push_remote: Option<String>,
    /// Explicit push default (`remote.lfspushdefault` or `remote.pushdefault`).
    pub(crate) push_default_remote: Option<String>,
    /// `.git/FETCH_HEAD` URL used only for the default fetch fallback.
    pub(crate) fetch_head_url: Option<String>,
    pub(crate) lfsconfig: SafeLfsConfig,
}

impl fmt::Debug for LfsEndpointInputs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsEndpointInputs")
            .field("has_lfs_url", &self.lfs_url.is_some())
            .field("has_lfs_push_url", &self.lfs_push_url.is_some())
            .field("has_lfs_git_protocol", &self.lfs_git_protocol.is_some())
            .field("remote_count", &self.remotes.len())
            .field("requested_remote", &self.requested_remote)
            .field("current_remote", &self.current_remote)
            .field("default_remote", &self.default_remote)
            .field("current_push_remote", &self.current_push_remote)
            .field("push_default_remote", &self.push_default_remote)
            .field("has_fetch_head_url", &self.fetch_head_url.is_some())
            .field("lfsconfig", &self.lfsconfig)
            .finish()
    }
}

impl LfsEndpointInputs {
    pub(crate) fn remote(&self, name: &str) -> Option<&LfsRemoteConfig> {
        self.remotes.iter().find(|remote| remote.name == name)
    }
}

/// Resolve and validate an endpoint for one operation.
pub(crate) fn resolve_lfs_endpoint(
    operation: LfsOperation,
    inputs: &LfsEndpointInputs,
) -> Result<LfsEndpointResolution, LfsEndpointError> {
    if operation == LfsOperation::Push {
        if let Some(url) = inputs.lfs_push_url.as_deref() {
            return explicit_endpoint(operation, LfsEndpointSource::GitConfigGlobal, url);
        }
    }

    if let Some(url) = inputs.lfs_url.as_deref() {
        return explicit_endpoint(operation, LfsEndpointSource::GitConfigGlobal, url);
    }

    // A safe .lfsconfig global URL is visible as the global endpoint once the
    // normal Git configuration layer has been exhausted.  It therefore wins
    // over remote-specific configuration, just like git-lfs's merged config.
    if operation == LfsOperation::Push {
        if let Some(url) = inputs.lfsconfig.lfs_push_url() {
            return explicit_endpoint(operation, LfsEndpointSource::LfsConfigGlobal, url);
        }
    }
    if let Some(url) = inputs.lfsconfig.lfs_url() {
        return explicit_endpoint(operation, LfsEndpointSource::LfsConfigGlobal, url);
    }

    // An explicitly named remote is an exact destination contract.  It never
    // falls through to another remote or FETCH_HEAD, even when it is absent or
    // has no endpoint material.
    if let Some(requested) = inputs.requested_remote.as_deref() {
        let remote = inputs
            .remote(requested)
            .ok_or_else(|| LfsEndpointError::MissingRemote {
                operation,
                remote: sanitize_token(requested),
            })?;
        return resolve_remote_endpoint(operation, remote, inputs);
    }

    if let Some(remote) = select_implicit_lfs_remote(operation, inputs) {
        return resolve_remote_endpoint(operation, remote, inputs);
    }

    if operation == LfsOperation::Fetch
        && let Some(fetch_head_url) = inputs.fetch_head_url.as_deref()
    {
        return derive_remote_resolution(operation, fetch_head_url, inputs);
    }

    Err(LfsEndpointError::MissingRemote {
        operation,
        remote: DEFAULT_REMOTE_NAME.to_owned(),
    })
}

/// Normalize an explicitly configured endpoint.  Explicit endpoint URLs are
/// already LFS API roots; no `.git/info/lfs` suffix is added.
pub(crate) fn normalize_lfs_endpoint(url: &str) -> Result<String, LfsEndpointError> {
    let parsed = parse_http_url(url)?;
    Ok(parsed.normalized(false))
}

pub(crate) fn resolve_lfs_endpoint_for_remote(
    operation: LfsOperation,
    remote: &str,
    inputs: &LfsEndpointInputs,
) -> Result<LfsEndpointResolution, LfsEndpointError> {
    let mut selected = inputs.clone();
    selected.requested_remote = Some(remote.to_owned());
    resolve_lfs_endpoint(operation, &selected)
}

/// Parse a `.lfsconfig` entry set and reject keys outside Git LFS's whitelist.
///
/// Git LFS currently ignores unsafe keys with a warning.  Zmin makes this
/// boundary strict so a repository-controlled file cannot silently alter an
/// unimplemented or execution-bearing setting.
pub(crate) fn parse_safe_lfsconfig<I, K, V>(entries: I) -> Result<SafeLfsConfig, LfsEndpointError>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    SafeLfsConfig::parse(entries)
}

fn explicit_endpoint(
    operation: LfsOperation,
    source: LfsEndpointSource,
    url: &str,
) -> Result<LfsEndpointResolution, LfsEndpointError> {
    if let Some(scheme_end) = url.find("://") {
        let scheme = &url[..scheme_end];
        if matches!(
            scheme.to_ascii_lowercase().as_str(),
            "ssh" | "git+ssh" | "ssh+git"
        ) {
            validate_remote_text(url)?;
            return Ok(LfsEndpointResolution::SshAuthenticationRequired(
                ssh_auth_from_authority_url(operation, source, url, false)?,
            ));
        }
    } else if url.contains(':') {
        validate_remote_text(url)?;
        return Ok(LfsEndpointResolution::SshAuthenticationRequired(
            ssh_auth_from_scp_url(operation, source, url, false)?,
        ));
    }
    let parsed = parse_http_url(url)?;
    Ok(LfsEndpointResolution::Http(LfsEndpoint {
        operation,
        source,
        url: parse_http_url(&parsed.normalized(false))?,
    }))
}

fn resolve_remote_endpoint(
    operation: LfsOperation,
    remote: &LfsRemoteConfig,
    inputs: &LfsEndpointInputs,
) -> Result<LfsEndpointResolution, LfsEndpointError> {
    if operation == LfsOperation::Push {
        if let Some(url) = remote.lfs_push_url.as_deref() {
            return explicit_endpoint(operation, LfsEndpointSource::GitConfigRemote, url);
        }
    }
    if let Some(url) = remote.lfs_url.as_deref() {
        return explicit_endpoint(operation, LfsEndpointSource::GitConfigRemote, url);
    }
    if let Some(url) = inputs.lfsconfig.remote_lfs_url(&remote.name) {
        return explicit_endpoint(operation, LfsEndpointSource::LfsConfigRemote, url);
    }
    derive_remote_endpoint(operation, remote, inputs)
}

/// Select the first usable implicit remote in the same order as Git LFS's
/// `Remote()` / `PushRemote()` chain.  A configured name that is absent, or a
/// remote with no operation-specific endpoint material, does not terminate
/// implicit discovery.  Malformed endpoint material is retained so the
/// resolver fails closed instead of silently retargeting the operation.
pub(crate) fn select_implicit_lfs_remote<'a>(
    operation: LfsOperation,
    inputs: &'a LfsEndpointInputs,
) -> Option<&'a LfsRemoteConfig> {
    let configured = match operation {
        LfsOperation::Fetch => [
            inputs.current_remote.as_deref(),
            inputs.default_remote.as_deref(),
            None,
            None,
        ],
        LfsOperation::Push => [
            inputs.current_push_remote.as_deref(),
            inputs.push_default_remote.as_deref(),
            inputs.current_remote.as_deref(),
            inputs.default_remote.as_deref(),
        ],
    };

    let mut visited = Vec::with_capacity(6);
    for name in configured
        .into_iter()
        .flatten()
        .chain(
            (inputs.remotes.len() == 1)
                .then(|| inputs.remotes[0].name.as_str())
                .into_iter(),
        )
        .chain(std::iter::once(DEFAULT_REMOTE_NAME))
    {
        if name == "." || visited.iter().any(|visited| *visited == name) {
            continue;
        }
        visited.push(name);
        if let Some(remote) = inputs.remote(name)
            && remote_has_endpoint_material(operation, remote, inputs)
        {
            return Some(remote);
        }
    }
    None
}

fn remote_has_endpoint_material(
    operation: LfsOperation,
    remote: &LfsRemoteConfig,
    inputs: &LfsEndpointInputs,
) -> bool {
    if operation == LfsOperation::Push && remote.lfs_push_url.is_some() {
        return true;
    }
    remote.lfs_url.is_some()
        || inputs.lfsconfig.remote_lfs_url(&remote.name).is_some()
        || match operation {
            LfsOperation::Fetch => remote.url.is_some(),
            LfsOperation::Push => remote.push_url.is_some() || remote.url.is_some(),
        }
}

fn derive_remote_endpoint(
    operation: LfsOperation,
    remote: &LfsRemoteConfig,
    inputs: &LfsEndpointInputs,
) -> Result<LfsEndpointResolution, LfsEndpointError> {
    let raw_url = match operation {
        LfsOperation::Fetch => remote.url.as_deref(),
        LfsOperation::Push => remote.push_url.as_deref().or(remote.url.as_deref()),
    }
    .ok_or_else(|| LfsEndpointError::MissingRemoteUrl {
        remote: sanitize_token(&remote.name),
        operation,
    })?;

    derive_remote_resolution(operation, raw_url, inputs)
}

fn derive_remote_resolution(
    operation: LfsOperation,
    raw_url: &str,
    inputs: &LfsEndpointInputs,
) -> Result<LfsEndpointResolution, LfsEndpointError> {
    validate_remote_text(raw_url)?;
    if let Some(scheme_end) = raw_url.find("://") {
        let scheme = &raw_url[..scheme_end];
        if !valid_scheme(scheme) {
            return Err(LfsEndpointError::MalformedUrl);
        }
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" => {
                let parsed = parse_http_url(raw_url)?;
                if parsed.query.is_some() {
                    return Err(LfsEndpointError::DerivedQueryNotAllowed);
                }
                Ok(LfsEndpointResolution::Http(LfsEndpoint {
                    operation,
                    source: LfsEndpointSource::DerivedRemote,
                    url: parse_http_url(&parsed.normalized(true))?,
                }))
            }
            "ssh" | "git+ssh" | "ssh+git" => Ok(LfsEndpointResolution::SshAuthenticationRequired(
                ssh_auth_from_authority_url(
                    operation,
                    LfsEndpointSource::DerivedRemote,
                    raw_url,
                    true,
                )?,
            )),
            "git" => Ok(LfsEndpointResolution::Http(LfsEndpoint {
                operation,
                source: LfsEndpointSource::DerivedRemote,
                url: parse_http_url(&derive_from_git_url(
                    raw_url,
                    configured_git_protocol(inputs)?,
                )?)?,
            })),
            _ => Err(LfsEndpointError::UnsupportedScheme {
                scheme: sanitize_token(&scheme.to_ascii_lowercase()),
            }),
        }
    } else {
        Ok(LfsEndpointResolution::SshAuthenticationRequired(
            ssh_auth_from_scp_url(operation, LfsEndpointSource::DerivedRemote, raw_url, true)?,
        ))
    }
}

fn configured_git_protocol(inputs: &LfsEndpointInputs) -> Result<&str, LfsEndpointError> {
    let protocol = inputs
        .lfs_git_protocol
        .as_deref()
        .or(inputs.lfsconfig.lfs_git_protocol())
        .unwrap_or("https");
    if protocol.eq_ignore_ascii_case("http") {
        return Ok("http");
    }
    if protocol.eq_ignore_ascii_case("https") {
        return Ok("https");
    }
    Err(LfsEndpointError::UnsupportedScheme {
        scheme: sanitize_token(protocol),
    })
}

fn derive_from_git_url(raw_url: &str, protocol: &str) -> Result<String, LfsEndpointError> {
    let scheme_end = raw_url.find("://").ok_or(LfsEndpointError::MalformedUrl)?;
    let remainder = &raw_url[scheme_end + 3..];
    let (authority, path_and_query) = split_authority(remainder)?;
    let (authority, userinfo) = split_remote_userinfo(authority)?;
    if userinfo.is_some() {
        return Err(LfsEndpointError::UserInfoNotAllowed);
    }
    let parsed_authority = parse_authority(authority, false)?;
    let (path, query) = split_path_query(path_and_query)?;
    if query.is_some() {
        return Err(LfsEndpointError::DerivedQueryNotAllowed);
    }
    if path.is_empty() || !path.starts_with('/') {
        return Err(LfsEndpointError::MalformedUrl);
    }
    let normalized_path = derive_path(path);
    let endpoint = LfsHttpUrl {
        raw: String::new(),
        scheme: protocol.to_owned(),
        authority: parsed_authority,
        path: normalized_path,
        query: None,
    };
    Ok(endpoint.normalized(false))
}

fn ssh_auth_from_authority_url(
    operation: LfsOperation,
    source: LfsEndpointSource,
    raw_url: &str,
    derive: bool,
) -> Result<LfsSshAuthenticationRequest, LfsEndpointError> {
    validate_remote_text(raw_url)?;
    let scheme_end = raw_url.find("://").ok_or(LfsEndpointError::MalformedUrl)?;
    let remainder = &raw_url[scheme_end + 3..];
    let (authority, path_and_query) = split_authority(remainder)?;
    let (authority, userinfo) = split_remote_userinfo(authority)?;
    let parsed_authority = parse_authority(authority, false)?;
    let (path, query) = split_path_query(path_and_query)?;
    if query.is_some() || path.is_empty() || !path.starts_with('/') {
        return Err(if query.is_some() {
            LfsEndpointError::DerivedQueryNotAllowed
        } else {
            LfsEndpointError::MalformedUrl
        });
    }
    let default_endpoint =
        ssh_default_http_endpoint(operation, source, &parsed_authority.host, path, derive)?;
    Ok(LfsSshAuthenticationRequest {
        operation,
        user: validate_ssh_user(userinfo)?,
        host: parsed_authority.host,
        port: parsed_authority.port,
        path: path.to_owned(),
        default_endpoint,
    })
}

fn ssh_auth_from_scp_url(
    operation: LfsOperation,
    source: LfsEndpointSource,
    raw_url: &str,
    derive: bool,
) -> Result<LfsSshAuthenticationRequest, LfsEndpointError> {
    validate_remote_text(raw_url)?;
    let (without_query, query) = split_raw_query(raw_url)?;
    if query.is_some() {
        return Err(LfsEndpointError::DerivedQueryNotAllowed);
    }
    validate_path_percent_encoding(without_query)?;
    let separator = if without_query.starts_with('[') {
        let closing = without_query
            .find(']')
            .ok_or(LfsEndpointError::MalformedUrl)?;
        closing + 1
    } else if let Some(opening) = without_query.find('[') {
        let closing = without_query[opening..]
            .find(']')
            .map(|offset| opening + offset)
            .ok_or(LfsEndpointError::MalformedUrl)?;
        closing + 1
    } else {
        without_query
            .find(':')
            .ok_or(LfsEndpointError::UnsupportedScheme {
                scheme: "schemeless".to_owned(),
            })?
    };
    if without_query.as_bytes().get(separator) != Some(&b':') {
        return Err(LfsEndpointError::MalformedUrl);
    }
    let host_part = &without_query[..separator];
    let path = &without_query[separator + 1..];
    if path.is_empty() || host_part.is_empty() || path.starts_with('/') {
        return Err(LfsEndpointError::MalformedUrl);
    }
    let (userinfo, authority) = parse_scp_user_and_authority(host_part)?;
    let default_endpoint =
        ssh_default_http_endpoint(operation, source, &authority.host, path, derive)?;
    Ok(LfsSshAuthenticationRequest {
        operation,
        user: validate_ssh_user(userinfo.as_deref())?,
        host: authority.host,
        port: authority.port,
        path: path.to_owned(),
        default_endpoint,
    })
}

/// Mirrors Git LFS v3.7.1's `EndpointFromSshUrl` followed, for a Git remote,
/// by `NewEndpointFromCloneURL`: user information and the SSH port do not
/// become part of the HTTPS endpoint, while the repository path is retained.
fn ssh_default_http_endpoint(
    operation: LfsOperation,
    source: LfsEndpointSource,
    host: &str,
    ssh_path: &str,
    derive: bool,
) -> Result<LfsEndpoint, LfsEndpointError> {
    let path = if ssh_path.starts_with('/') {
        ssh_path.to_owned()
    } else {
        format!("/{ssh_path}")
    };
    let parsed = parse_http_url(&format!("https://{host}{path}"))?;
    let normalized = parsed.normalized(derive);
    Ok(LfsEndpoint {
        operation,
        source,
        url: parse_http_url(&normalized)?,
    })
}

fn derive_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    let base = if trimmed.ends_with(".git") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}.git")
    };
    format!("{base}{INFO_LFS_SUFFIX}")
}

/// A strict HTTP(S) URL value shared by endpoint discovery and Batch actions.
///
/// `as_str` returns the original URL bytes.  This is intentional: Basic
/// Transfer action URLs can contain signed query strings, so an action caller
/// must not normalize or reorder them before making the request.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsHttpUrl {
    raw: String,
    scheme: String,
    authority: HttpAuthority,
    path: String,
    query: Option<String>,
}

impl fmt::Debug for LfsHttpUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("LfsHttpUrl")
            .field(&"<redacted>")
            .finish()
    }
}

impl LfsHttpUrl {
    pub(crate) fn as_str(&self) -> &str {
        &self.raw
    }

    /// Whether this URL is covered by an endpoint-scoped LFS access rule.
    /// Origins use effective default ports; paths remain byte-exact and must
    /// end on a segment boundary. Query-bearing values are not access scopes.
    pub(crate) fn is_access_scope_for(&self, endpoint: &Self) -> bool {
        if self.query.is_some()
            || self.scheme != endpoint.scheme
            || self.authority.host != endpoint.authority.host
            || effective_http_port(&self.scheme, self.authority.port.as_deref())
                != effective_http_port(&endpoint.scheme, endpoint.authority.port.as_deref())
        {
            return false;
        }
        let scope_path = self.path.trim_end_matches('/');
        let endpoint_path = endpoint.path.trim_end_matches('/');
        scope_path.is_empty()
            || endpoint_path == scope_path
            || endpoint_path
                .strip_prefix(scope_path)
                .is_some_and(|suffix| suffix.starts_with('/'))
    }

    pub(crate) fn access_scope_len(&self) -> usize {
        self.path.trim_end_matches('/').len()
    }

    pub(crate) fn is_valid_access_scope(&self) -> bool {
        self.query.is_none()
    }

    fn normalized(&self, derive: bool) -> String {
        let path = if derive {
            derive_path(&self.path)
        } else {
            trim_endpoint_path(&self.path)
        };
        let mut result = format!("{}://{}{}", self.scheme, self.authority, path);
        if let Some(query) = &self.query {
            result.push('?');
            result.push_str(query);
        }
        result
    }
}

fn effective_http_port(scheme: &str, port: Option<&str>) -> Option<u16> {
    port.and_then(|value| value.parse::<u16>().ok())
        .or_else(|| match scheme {
            "http" => Some(80),
            "https" => Some(443),
            _ => None,
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpAuthority {
    host: String,
    port: Option<String>,
}

impl fmt::Display for HttpAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.host)?;
        if let Some(port) = &self.port {
            write!(formatter, ":{port}")?;
        }
        Ok(())
    }
}

pub(crate) fn parse_http_url(raw_url: &str) -> Result<LfsHttpUrl, LfsEndpointError> {
    validate_url_text(raw_url)?;
    if raw_url.len() > LFS_HTTP_URL_MAX_BYTES {
        return Err(LfsEndpointError::MalformedUrl);
    }
    let scheme_end = raw_url.find("://").ok_or(LfsEndpointError::MalformedUrl)?;
    let scheme = &raw_url[..scheme_end];
    if !valid_scheme(scheme) {
        return Err(LfsEndpointError::MalformedUrl);
    }
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(LfsEndpointError::UnsupportedScheme {
            scheme: scheme.to_ascii_lowercase(),
        });
    }

    let remainder = &raw_url[scheme_end + 3..];
    let (authority, path_and_query) = split_authority(remainder)?;
    if authority.contains('@') {
        return Err(LfsEndpointError::UserInfoNotAllowed);
    }
    let parsed_authority = parse_authority(authority, false)?;
    let (path, query) = split_path_query(path_and_query)?;
    if !path.is_empty() && !path.starts_with('/') {
        return Err(LfsEndpointError::MalformedUrl);
    }
    if let Some(query) = query.as_deref() {
        validate_percent_encoding(query)?;
    }
    Ok(LfsHttpUrl {
        raw: raw_url.to_owned(),
        scheme: scheme.to_ascii_lowercase(),
        authority: parsed_authority,
        path: path.to_owned(),
        query,
    })
}

fn parse_authority(raw: &str, allow_userinfo: bool) -> Result<HttpAuthority, LfsEndpointError> {
    if raw.is_empty() {
        return Err(LfsEndpointError::MissingHost);
    }
    validate_authority_escapes(raw)?;
    if !allow_userinfo && raw.contains('@') {
        return Err(LfsEndpointError::UserInfoNotAllowed);
    }
    let authority = if allow_userinfo {
        split_remote_userinfo(raw)?.0
    } else {
        raw
    };
    if authority.is_empty() {
        return Err(LfsEndpointError::MissingHost);
    }

    let (host, port) = if authority.starts_with('[') {
        let closing = authority.find(']').ok_or(LfsEndpointError::MalformedUrl)?;
        let host = &authority[..=closing];
        let remainder = &authority[closing + 1..];
        let port = if remainder.is_empty() {
            None
        } else if let Some(port) = remainder.strip_prefix(':') {
            Some(validate_port(port)?)
        } else {
            return Err(LfsEndpointError::MalformedUrl);
        };
        (host.to_ascii_lowercase(), port)
    } else {
        if authority.matches(':').count() > 1 {
            return Err(LfsEndpointError::MalformedUrl);
        }
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(validate_port(port)?)),
            None => (authority, None),
        };
        if host.is_empty() || host.contains(['[', ']']) {
            return Err(LfsEndpointError::MalformedUrl);
        }
        (host.to_ascii_lowercase(), port)
    };
    if host == "[]" || host.contains(['/', '?', '#', '@']) {
        return Err(LfsEndpointError::MalformedUrl);
    }
    validate_host(&host)?;
    Ok(HttpAuthority { host, port })
}

fn parse_scp_authority(raw: &str) -> Result<HttpAuthority, LfsEndpointError> {
    if raw.starts_with('[') && raw.ends_with(']') {
        let inner = &raw[1..raw.len() - 1];
        let (inner, _userinfo) = split_remote_userinfo(inner)?;
        let (host, port) = match inner.rsplit_once(':') {
            Some((host, port))
                if !host.is_empty()
                    && !port.is_empty()
                    && !host.contains(':')
                    && port.chars().all(|value| value.is_ascii_digit()) =>
            {
                (host, Some(validate_port(port)?))
            }
            _ => (inner, None),
        };
        if host.is_empty() {
            return Err(LfsEndpointError::MalformedUrl);
        }
        let host = if host.contains(':') {
            format!("[{}]", host.to_ascii_lowercase())
        } else {
            host.to_ascii_lowercase()
        };
        validate_host(&host)?;
        return Ok(HttpAuthority { host, port });
    }
    let (raw, _userinfo) = split_remote_userinfo(raw)?;
    parse_authority(raw, true)
}

fn parse_scp_user_and_authority(
    raw: &str,
) -> Result<(Option<String>, HttpAuthority), LfsEndpointError> {
    if raw.starts_with('[') && raw.ends_with(']') {
        let inner = &raw[1..raw.len() - 1];
        let (host, user) = match split_remote_userinfo(inner)? {
            (host, user) => (host, user.map(str::to_owned)),
        };
        let authority = parse_scp_authority(raw)?;
        if host.is_empty() {
            return Err(LfsEndpointError::MalformedUrl);
        }
        return Ok((user, authority));
    }
    let (host, user) = split_remote_userinfo(raw)?;
    Ok((user.map(str::to_owned), parse_authority(host, false)?))
}

fn split_authority(raw: &str) -> Result<(&str, &str), LfsEndpointError> {
    let boundary = raw.find(['/', '?']).unwrap_or(raw.len());
    let authority = &raw[..boundary];
    if authority.is_empty() {
        return Err(LfsEndpointError::MissingHost);
    }
    Ok((authority, &raw[boundary..]))
}

fn split_path_query(raw: &str) -> Result<(&str, Option<String>), LfsEndpointError> {
    let (path, query) = match raw.split_once('?') {
        Some((path, query)) => (path, Some(query.to_owned())),
        None => (raw, None),
    };
    validate_path_percent_encoding(path)?;
    if let Some(query) = query.as_deref() {
        validate_percent_encoding(query)?;
    }
    Ok((path, query))
}

fn split_raw_query(raw: &str) -> Result<(&str, Option<String>), LfsEndpointError> {
    if raw.matches('?').count() > 1 {
        return Err(LfsEndpointError::MalformedUrl);
    }
    match raw.split_once('?') {
        Some((prefix, query)) => Ok((prefix, Some(query.to_owned()))),
        None => Ok((raw, None)),
    }
}

fn split_remote_userinfo(raw: &str) -> Result<(&str, Option<&str>), LfsEndpointError> {
    match raw.rsplit_once('@') {
        Some((userinfo, host)) => {
            if userinfo.is_empty() || host.is_empty() || userinfo.contains('@') {
                return Err(LfsEndpointError::MalformedUrl);
            }
            Ok((host, Some(userinfo)))
        }
        None => Ok((raw, None)),
    }
}

fn validate_port(raw: &str) -> Result<String, LfsEndpointError> {
    if raw.is_empty() || !raw.chars().all(|value| value.is_ascii_digit()) {
        return Err(LfsEndpointError::MalformedUrl);
    }
    let value = raw
        .parse::<u32>()
        .map_err(|_| LfsEndpointError::MalformedUrl)?;
    if value == 0 || value > u16::MAX as u32 {
        return Err(LfsEndpointError::MalformedUrl);
    }
    Ok(value.to_string())
}

fn validate_authority_escapes(value: &str) -> Result<(), LfsEndpointError> {
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'%' {
            continue;
        }
        if index + 2 >= bytes.len()
            || !bytes[index + 1].is_ascii_hexdigit()
            || !bytes[index + 2].is_ascii_hexdigit()
        {
            return Err(LfsEndpointError::MalformedUrl);
        }
        let decoded = (hex_value(bytes[index + 1]) << 4) | hex_value(bytes[index + 2]);
        if decoded < 0x20 || matches!(decoded, b'/' | b'?' | b'#' | b'@' | b'[' | b']' | b'\\') {
            return Err(LfsEndpointError::MalformedUrl);
        }
    }
    Ok(())
}

fn hex_value(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}

fn validate_host(host: &str) -> Result<(), LfsEndpointError> {
    if host.starts_with('[') {
        let Some(inner) = host
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            return Err(LfsEndpointError::MalformedUrl);
        };
        if Ipv6Addr::from_str(inner).is_err() {
            return Err(LfsEndpointError::MalformedUrl);
        }
        return Ok(());
    }
    if host.is_empty() || !host.is_ascii() || host.contains('%') {
        return Err(LfsEndpointError::MalformedUrl);
    }
    let name = host.trim_end_matches('.');
    if name.is_empty() {
        return Err(LfsEndpointError::MalformedUrl);
    }
    for label in name.split('.') {
        if label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
        {
            return Err(LfsEndpointError::MalformedUrl);
        }
    }
    Ok(())
}

fn trim_endpoint_path(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return path.to_owned();
    }
    path.trim_end_matches('/').to_owned()
}

fn valid_scheme(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.all(|character| character.is_ascii_alphanumeric() || ".+-".contains(character))
}

fn validate_url_text(value: &str) -> Result<(), LfsEndpointError> {
    if value.is_empty() {
        return Err(LfsEndpointError::MalformedUrl);
    }
    if has_control_or_space(value) {
        return Err(LfsEndpointError::ControlCharacter);
    }
    if value.contains('#') {
        return Err(LfsEndpointError::FragmentNotAllowed);
    }
    if value.contains('\\') {
        return Err(LfsEndpointError::MalformedUrl);
    }
    validate_percent_encoding(value)
}

fn validate_remote_text(value: &str) -> Result<(), LfsEndpointError> {
    if has_control_or_space(value) {
        return Err(LfsEndpointError::ControlCharacter);
    }
    if value.contains('#') {
        return Err(LfsEndpointError::FragmentNotAllowed);
    }
    if value.contains('\\') {
        return Err(LfsEndpointError::MalformedUrl);
    }
    validate_percent_encoding(value)
}

fn validate_ssh_user(value: Option<&str>) -> Result<Option<String>, LfsEndpointError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty()
        || value.starts_with('-')
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(LfsEndpointError::MalformedUrl);
    }
    Ok(Some(value.to_owned()))
}

fn has_control_or_space(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() || character.is_ascii_whitespace())
}

fn validate_percent_encoding(value: &str) -> Result<(), LfsEndpointError> {
    validate_percent_encoding_with_routing(value, false)
}

fn validate_path_percent_encoding(value: &str) -> Result<(), LfsEndpointError> {
    validate_percent_encoding_with_routing(value, true)
}

fn validate_percent_encoding_with_routing(
    value: &str,
    reject_routing_delimiters: bool,
) -> Result<(), LfsEndpointError> {
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(LfsEndpointError::MalformedUrl);
            }
            if reject_routing_delimiters {
                let decoded = (hex_value(bytes[index + 1]) << 4) | hex_value(bytes[index + 2]);
                if decoded < 0x20
                    || decoded == 0x7f
                    || matches!(decoded, b'/' | b'?' | b'#' | b'\\')
                {
                    return Err(LfsEndpointError::MalformedUrl);
                }
            }
        }
    }
    Ok(())
}

/// Decode a URL path using Go `net/url`'s byte-preserving `URL.Path`
/// semantics. Invalid escapes and NUL are rejected without converting
/// non-UTF-8 octets through a lossy Rust string.
pub(crate) fn decode_lfs_url_path(raw_path: &str) -> Result<Vec<u8>, LfsEndpointError> {
    let raw = raw_path.as_bytes();
    let mut decoded = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        let byte = if raw[index] == b'%' {
            if index + 2 >= raw.len()
                || !raw[index + 1].is_ascii_hexdigit()
                || !raw[index + 2].is_ascii_hexdigit()
            {
                return Err(LfsEndpointError::MalformedUrl);
            }
            let byte = (hex_value(raw[index + 1]) << 4) | hex_value(raw[index + 2]);
            index += 3;
            byte
        } else {
            let byte = raw[index];
            index += 1;
            byte
        };
        if byte == 0 {
            return Err(LfsEndpointError::MalformedUrl);
        }
        decoded.push(byte);
    }
    Ok(decoded)
}

fn canonical_lfsconfig_key(raw: &str) -> Result<String, LfsEndpointError> {
    if raw.is_empty() || has_control_or_space(raw) {
        return Err(LfsEndpointError::UnsafeLfsConfigKey {
            key: "<redacted>".to_owned(),
        });
    }
    let parts: Vec<&str> = raw.split('.').collect();
    let middle = (parts.len() >= 3).then(|| parts[1..parts.len() - 1].join("."));
    let is_access_key = parts.len() >= 3
        && parts[0].eq_ignore_ascii_case("lfs")
        && parts[parts.len() - 1].eq_ignore_ascii_case("access");
    let is_safe = match parts.as_slice() {
        [section, key] => {
            section.eq_ignore_ascii_case("lfs")
                && matches!(
                    key.to_ascii_lowercase().as_str(),
                    "allowincompletepush"
                        | "fetchexclude"
                        | "fetchinclude"
                        | "gitprotocol"
                        | "locksverify"
                        | "pushurl"
                        | "skipdownloaderrors"
                        | "url"
                )
        }
        _ if parts.len() >= 3 => {
            let section = parts[0];
            let final_key = parts[parts.len() - 1];
            (section.eq_ignore_ascii_case("lfs")
                && final_key.eq_ignore_ascii_case("access")
                && middle.as_deref().is_some_and(|value| !value.is_empty()))
                || (section.eq_ignore_ascii_case("remote")
                    && final_key.eq_ignore_ascii_case("lfsurl")
                    && middle.as_deref().is_some_and(|value| !value.is_empty()))
        }
        _ => false,
    };

    if !is_safe || (is_access_key && !validate_access_key_scope(middle.as_deref().unwrap_or(""))) {
        return Err(LfsEndpointError::UnsafeLfsConfigKey {
            key: "<redacted>".to_owned(),
        });
    }

    let canonical = if parts.len() == 2 {
        format!("lfs.{}", parts[1].to_ascii_lowercase())
    } else {
        let middle = parts[1..parts.len() - 1].join(".");
        format!(
            "{}.{}.{}",
            parts[0].to_ascii_lowercase(),
            middle,
            parts[parts.len() - 1].to_ascii_lowercase()
        )
    };
    Ok(canonical)
}

fn validate_access_key_scope(scope: &str) -> bool {
    if scope.is_empty() {
        return false;
    }
    if let Some(scheme_end) = scope.find("://") {
        let scheme = &scope[..scheme_end];
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return false;
        }
        return parse_http_url(scope).is_ok_and(|url| url.is_valid_access_scope());
    }
    validate_remote_text(scope).is_ok() && validate_path_percent_encoding(scope).is_ok()
}

fn sanitize_token(value: &str) -> String {
    let mut result = String::with_capacity(value.len().min(80));
    for character in value.chars().take(80) {
        if character.is_ascii_alphanumeric() || "._/-".contains(character) {
            result.push(character);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "unknown".to_owned()
    } else {
        result
    }
}

/// Endpoint errors intentionally never contain the untrusted URL value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LfsEndpointError {
    ControlCharacter,
    DerivedQueryNotAllowed,
    FragmentNotAllowed,
    MalformedUrl,
    MissingHost,
    MissingRemote {
        operation: LfsOperation,
        remote: String,
    },
    MissingRemoteUrl {
        operation: LfsOperation,
        remote: String,
    },
    UnsupportedScheme {
        scheme: String,
    },
    UnsafeLfsConfigKey {
        key: String,
    },
    UserInfoNotAllowed,
}

impl fmt::Display for LfsEndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ControlCharacter => {
                formatter.write_str("LFS endpoint contains control or whitespace characters")
            }
            Self::DerivedQueryNotAllowed => {
                formatter.write_str("derived Git remote URLs cannot contain query strings")
            }
            Self::FragmentNotAllowed => {
                formatter.write_str("LFS endpoint fragments are not supported")
            }
            Self::MalformedUrl => formatter.write_str("malformed LFS endpoint URL"),
            Self::MissingHost => formatter.write_str("LFS endpoint URL has no host"),
            Self::MissingRemote { operation, remote } => {
                write!(
                    formatter,
                    "no LFS remote '{remote}' is configured for {}",
                    operation.label()
                )
            }
            Self::MissingRemoteUrl { operation, remote } => {
                write!(
                    formatter,
                    "remote '{remote}' has no Git URL for {}",
                    operation.label()
                )
            }
            Self::UnsupportedScheme { scheme } => {
                write!(
                    formatter,
                    "LFS HTTP core does not support URL scheme '{scheme}'"
                )
            }
            Self::UnsafeLfsConfigKey { key } => {
                write!(formatter, "unsafe .lfsconfig key '{key}'")
            }
            Self::UserInfoNotAllowed => {
                formatter.write_str("LFS endpoint URL userinfo is not allowed")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str, url: &str) -> LfsRemoteConfig {
        let mut remote = LfsRemoteConfig::named(name);
        remote.url = Some(url.to_owned());
        remote
    }

    fn inputs() -> LfsEndpointInputs {
        LfsEndpointInputs {
            remotes: vec![remote("origin", "https://github.com/acme/repo.git")],
            lfsconfig: SafeLfsConfig::empty(),
            ..LfsEndpointInputs::default()
        }
    }

    fn http_url(resolution: LfsEndpointResolution) -> String {
        match resolution {
            LfsEndpointResolution::Http(endpoint) => endpoint.url.as_str().to_owned(),
            LfsEndpointResolution::SshAuthenticationRequired(_) => {
                panic!("expected HTTP endpoint")
            }
        }
    }

    fn ssh_request(resolution: LfsEndpointResolution) -> LfsSshAuthenticationRequest {
        match resolution {
            LfsEndpointResolution::SshAuthenticationRequired(request) => request,
            LfsEndpointResolution::Http(_) => panic!("expected SSH authentication request"),
        }
    }

    #[test]
    fn fetch_precedence_is_global_then_lfsconfig_then_remote_specific_then_derived() {
        let mut config = inputs();
        config.lfs_url = Some("https://normal.example/lfs/".to_owned());
        config.lfsconfig =
            parse_safe_lfsconfig([("lfs.url", "https://repo.example/lfs")]).expect("safe config");
        config.remotes[0].lfs_url = Some("https://remote.example/lfs".to_owned());
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://normal.example/lfs"
        );

        config.lfs_url = None;
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://repo.example/lfs"
        );

        config.lfsconfig = SafeLfsConfig::empty();
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://remote.example/lfs"
        );

        config.remotes[0].lfs_url = None;
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://github.com/acme/repo.git/info/lfs"
        );
    }

    #[test]
    fn push_uses_push_urls_and_keeps_fetch_endpoint_independent() {
        let mut config = inputs();
        config.lfs_url = Some("https://read.example/lfs".to_owned());
        config.lfs_push_url = Some("https://write-global.example/lfs".to_owned());
        config.remotes[0].lfs_push_url = Some("https://write-remote.example/lfs".to_owned());
        config.remotes[0].push_url = Some("https://write-git.example/repo.git".to_owned());

        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://read.example/lfs"
        );
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Push, &config).expect("endpoint")),
            "https://write-global.example/lfs"
        );

        config.lfs_push_url = None;
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Push, &config).expect("endpoint")),
            "https://read.example/lfs"
        );

        config.lfs_url = None;
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Push, &config).expect("endpoint")),
            "https://write-remote.example/lfs"
        );
    }

    #[test]
    fn github_and_gitlab_remote_forms_derive_https_endpoints() {
        let github = remote("origin", "git@github.com:acme/repo.git");
        let gitlab = remote("origin", "https://gitlab.example:8443/group/sub/repo");
        let mut config = LfsEndpointInputs {
            remotes: vec![github],
            ..LfsEndpointInputs::default()
        };
        let request =
            ssh_request(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint"));
        assert_eq!(request.user.as_deref(), Some("git"));
        assert_eq!(request.host, "github.com");
        assert_eq!(request.path, "acme/repo.git");
        assert_eq!(
            request.default_endpoint.url.as_str(),
            "https://github.com/acme/repo.git/info/lfs"
        );
        config.remotes = vec![gitlab];
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://gitlab.example:8443/group/sub/repo.git/info/lfs"
        );
    }

    #[test]
    fn ports_and_ipv6_are_preserved() {
        assert_eq!(
            normalize_lfs_endpoint("HTTPS://[2001:DB8::1]:9443/lfs/?token=abc").expect("endpoint"),
            "https://[2001:db8::1]:9443/lfs?token=abc"
        );
        let config = LfsEndpointInputs {
            remotes: vec![remote(
                "origin",
                "ssh://git@[2001:db8::1]:2222/team/repo.git",
            )],
            ..LfsEndpointInputs::default()
        };
        let request =
            ssh_request(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint"));
        assert_eq!(request.user.as_deref(), Some("git"));
        assert_eq!(request.host, "[2001:db8::1]");
        assert_eq!(request.port.as_deref(), Some("2222"));
        assert_eq!(request.path, "/team/repo.git");
        assert_eq!(
            request.default_endpoint.url.as_str(),
            "https://[2001:db8::1]/team/repo.git/info/lfs"
        );
    }

    #[test]
    fn safe_lfsconfig_rejects_execution_and_push_remote_keys() {
        let error = parse_safe_lfsconfig([("lfs.customtransfer.foo.args", "secret")])
            .expect_err("unsafe key");
        assert_eq!(error.to_string(), "unsafe .lfsconfig key '<redacted>'");
        let error = parse_safe_lfsconfig([("lfs.customtransfer.password.args", "secret")])
            .expect_err("unsafe key");
        assert!(!error.to_string().contains("password"));
        let error = parse_safe_lfsconfig([("remote.origin.lfspushurl", "https://secret")])
            .expect_err("unsafe key");
        assert_eq!(error.to_string(), "unsafe .lfsconfig key '<redacted>'");
    }

    #[test]
    fn endpoint_errors_never_echo_credentials_or_fragments() {
        let error =
            normalize_lfs_endpoint("https://user:password@example.test/lfs").expect_err("userinfo");
        assert_eq!(
            error.to_string(),
            "LFS endpoint URL userinfo is not allowed"
        );
        let error =
            normalize_lfs_endpoint("https://example.test/lfs#password").expect_err("fragment");
        assert_eq!(
            error.to_string(),
            "LFS endpoint fragments are not supported"
        );
        assert!(!error.to_string().contains("password"));
    }

    #[test]
    fn credential_path_decoder_preserves_net_url_bytes() {
        assert_eq!(
            decode_lfs_url_path("/team/%72epo%20one/%fF").expect("decoded path"),
            b"/team/repo one/\xff"
        );
        assert_eq!(
            decode_lfs_url_path("/team/%00private"),
            Err(LfsEndpointError::MalformedUrl)
        );
        assert_eq!(
            decode_lfs_url_path("/team/%x0private"),
            Err(LfsEndpointError::MalformedUrl)
        );
    }

    #[test]
    fn unsupported_config_schemes_are_explicit() {
        let error =
            normalize_lfs_endpoint("ssh://git@example.test/repo").expect_err("unsupported scheme");
        assert_eq!(
            error.to_string(),
            "LFS HTTP core does not support URL scheme 'ssh'"
        );
        let error = normalize_lfs_endpoint("https://").expect_err("missing host");
        assert_eq!(error.to_string(), "LFS endpoint URL has no host");
    }

    #[test]
    fn git_protocol_is_validated_and_applies_only_to_git_remotes() {
        let mut config = LfsEndpointInputs {
            remotes: vec![remote("origin", "git://example.test/team/repo")],
            lfs_git_protocol: Some("http".to_owned()),
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "http://example.test/team/repo.git/info/lfs"
        );
        config.lfs_git_protocol = Some("file".to_owned());
        let error =
            resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect_err("invalid git protocol");
        assert_eq!(
            error.to_string(),
            "LFS HTTP core does not support URL scheme 'file'"
        );
    }

    #[test]
    fn ssh_and_scp_remotes_require_authentication_discovery() {
        let mut config = LfsEndpointInputs {
            remotes: vec![remote(
                "origin",
                "ssh://git@example.test:2222/team/repo.git",
            )],
            ..LfsEndpointInputs::default()
        };
        let request =
            ssh_request(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint"));
        assert_eq!(request.operation, LfsOperation::Fetch);
        assert_eq!(request.user.as_deref(), Some("git"));
        assert_eq!(request.host, "example.test");
        assert_eq!(request.port.as_deref(), Some("2222"));
        assert_eq!(request.path, "/team/repo.git");
        assert_eq!(
            request.default_endpoint.url.as_str(),
            "https://example.test/team/repo.git/info/lfs"
        );

        config.remotes[0].url = Some("git@example.test:team/repo.git".to_owned());
        let request =
            ssh_request(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint"));
        assert_eq!(request.path, "team/repo.git");
        assert_eq!(
            request.default_endpoint.url.as_str(),
            "https://example.test/team/repo.git/info/lfs"
        );

        config.lfs_url = Some("ssh://git@example.test/team/repo.git".to_owned());
        let request =
            ssh_request(resolve_lfs_endpoint(LfsOperation::Push, &config).expect("endpoint"));
        assert_eq!(request.operation, LfsOperation::Push);
        assert_eq!(request.path, "/team/repo.git");
        assert_eq!(
            request.default_endpoint.url.as_str(),
            "https://example.test/team/repo.git"
        );
        assert_eq!(
            request.default_endpoint.source,
            LfsEndpointSource::GitConfigGlobal
        );
    }

    #[test]
    fn git_lfs_v3_7_1_ssh_discovery_vectors_keep_ssh_metadata_separate() {
        let vectors = [
            (
                "git@github.com:git-lfs/git-lfs.git",
                Some("git"),
                "github.com",
                None,
                "git-lfs/git-lfs.git",
                "https://github.com/git-lfs/git-lfs.git/info/lfs",
            ),
            (
                "ssh://deploy@example.test:2222/team/repo",
                Some("deploy"),
                "example.test",
                Some("2222"),
                "/team/repo",
                "https://example.test/team/repo.git/info/lfs",
            ),
            (
                "git+ssh://git@example.test/team/repo.git/",
                Some("git"),
                "example.test",
                None,
                "/team/repo.git/",
                "https://example.test/team/repo.git/info/lfs",
            ),
            (
                "[git@example.test:2022]:team/repo.git",
                Some("git"),
                "example.test",
                Some("2022"),
                "team/repo.git",
                "https://example.test/team/repo.git/info/lfs",
            ),
        ];
        for (raw, user, host, port, path, endpoint) in vectors {
            let config = LfsEndpointInputs {
                remotes: vec![remote("origin", raw)],
                ..LfsEndpointInputs::default()
            };
            let request =
                ssh_request(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint"));
            assert_eq!(request.user.as_deref(), user);
            assert_eq!(request.host, host);
            assert_eq!(request.port.as_deref(), port);
            assert_eq!(request.path, path);
            assert_eq!(request.default_endpoint.url.as_str(), endpoint);
            assert!(!request.default_endpoint.url.as_str().contains('@'));
            assert!(!request.default_endpoint.url.as_str().contains(":2222"));
        }
    }

    #[test]
    fn derived_remote_queries_are_rejected_but_explicit_queries_are_kept() {
        let config = LfsEndpointInputs {
            remotes: vec![remote(
                "origin",
                "https://example.test/team/repo.git?token=secret",
            )],
            ..LfsEndpointInputs::default()
        };
        let error = resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect_err("derived query");
        assert_eq!(
            error.to_string(),
            "derived Git remote URLs cannot contain query strings"
        );
        assert_eq!(
            normalize_lfs_endpoint("https://example.test/lfs?token=secret").expect("endpoint"),
            "https://example.test/lfs?token=secret"
        );
    }

    #[test]
    fn dotted_remote_and_url_scoped_access_keys_are_safe() {
        let config = parse_safe_lfsconfig([
            ("remote.team.origin.lfsurl", "https://lfs.example/team"),
            ("lfs.https://lfs.example/team.access", "basic"),
            ("lfs.opaque-scope.access", "basic"),
        ])
        .expect("safe config");
        assert_eq!(
            config.remote_lfs_url("team.origin"),
            Some("https://lfs.example/team")
        );
        assert_eq!(
            config.value("lfs.https://lfs.example/team.access"),
            Some("basic")
        );
        assert_eq!(config.value("lfs.opaque-scope.access"), Some("basic"));
        assert!(
            parse_safe_lfsconfig([("lfs.https://lfs.example/%2Fteam.access", "basic")]).is_err()
        );
        assert!(parse_safe_lfsconfig([("lfs.opaque%2Fscope.access", "basic")]).is_err());
        assert!(parse_safe_lfsconfig([("lfs.ftp://lfs.example/team.access", "basic")]).is_err());
    }

    #[test]
    fn fetch_head_is_only_a_default_fetch_fallback() {
        let config = LfsEndpointInputs {
            fetch_head_url: Some("https://example.test/team/repo".to_owned()),
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://example.test/team/repo.git/info/lfs"
        );
        assert!(resolve_lfs_endpoint(LfsOperation::Push, &config).is_err());
    }

    #[test]
    fn authority_rejects_backslashes_encoded_delimiters_and_invalid_ipv6() {
        for value in [
            "https://example.test\\evil/lfs",
            "https://example%40.test/lfs",
            "https://[not-an-ipv6]/lfs",
            "https://example.test/team%2Frepo",
            "https://example.test/team%3Frepo",
            "https://example.test/team%23repo",
            "https://example.test/team%5Crepo",
        ] {
            assert!(normalize_lfs_endpoint(value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn current_and_default_remote_selection_matches_git_lfs_precedence() {
        let mut config = LfsEndpointInputs {
            remotes: vec![
                remote("origin", "https://example.test/origin.git"),
                remote("tracking", "https://example.test/tracking.git"),
                remote("branch-push", "https://example.test/branch-push.git"),
                remote("lfs-default", "https://example.test/lfs-default.git"),
                remote(
                    "lfs-push-default",
                    "https://example.test/lfs-push-default.git",
                ),
            ],
            current_remote: Some("tracking".to_owned()),
            default_remote: Some("lfs-default".to_owned()),
            current_push_remote: Some("branch-push".to_owned()),
            push_default_remote: Some("lfs-push-default".to_owned()),
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://example.test/tracking.git/info/lfs"
        );
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Push, &config).expect("endpoint")),
            "https://example.test/branch-push.git/info/lfs"
        );
        config.current_push_remote = None;
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Push, &config).expect("endpoint")),
            "https://example.test/lfs-push-default.git/info/lfs"
        );
        config.push_default_remote = None;
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Push, &config).expect("endpoint")),
            "https://example.test/tracking.git/info/lfs"
        );
    }

    #[test]
    fn explicit_named_remote_overrides_branch_selection() {
        let config = LfsEndpointInputs {
            remotes: vec![
                remote("origin", "https://example.test/origin.git"),
                remote("team.origin", "https://example.test/team.git"),
            ],
            current_remote: Some("origin".to_owned()),
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(
                resolve_lfs_endpoint_for_remote(LfsOperation::Fetch, "team.origin", &config)
                    .expect("endpoint")
            ),
            "https://example.test/team.git/info/lfs"
        );
    }

    #[test]
    fn explicit_named_remote_without_endpoint_is_not_retargeted() {
        let config = LfsEndpointInputs {
            remotes: vec![
                remote("origin", "https://example.test/origin.git"),
                LfsRemoteConfig::named("team"),
            ],
            requested_remote: Some("team".to_owned()),
            ..LfsEndpointInputs::default()
        };
        assert!(matches!(
            resolve_lfs_endpoint(LfsOperation::Fetch, &config),
            Err(LfsEndpointError::MissingRemoteUrl { .. })
        ));
        let mut missing = config;
        missing.requested_remote = Some("missing".to_owned());
        assert!(matches!(
            resolve_lfs_endpoint(LfsOperation::Fetch, &missing),
            Err(LfsEndpointError::MissingRemote { remote, .. }) if remote == "missing"
        ));
    }

    #[test]
    fn implicit_single_remote_is_selected_before_origin() {
        let config = LfsEndpointInputs {
            remotes: vec![remote("upstream", "https://example.test/upstream.git")],
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &config).expect("endpoint")),
            "https://example.test/upstream.git/info/lfs"
        );
    }

    #[test]
    fn implicit_selection_skips_absent_and_endpointless_candidates_in_order() {
        let fetch = LfsEndpointInputs {
            current_remote: Some("missing-branch".to_owned()),
            default_remote: Some("endpointless-default".to_owned()),
            remotes: vec![
                LfsRemoteConfig::named("endpointless-default"),
                remote("origin", "https://example.test/origin.git"),
            ],
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Fetch, &fetch).expect("origin fallback")),
            "https://example.test/origin.git/info/lfs"
        );

        let push = LfsEndpointInputs {
            current_push_remote: Some("missing-branch-push".to_owned()),
            push_default_remote: Some("missing-push-default".to_owned()),
            current_remote: Some("missing-branch".to_owned()),
            default_remote: Some("missing-default".to_owned()),
            remotes: vec![remote("upstream", "https://example.test/upstream.git")],
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(resolve_lfs_endpoint(LfsOperation::Push, &push).expect("sole fallback")),
            "https://example.test/upstream.git/info/lfs"
        );

        let fetch_head = LfsEndpointInputs {
            current_remote: Some("endpointless".to_owned()),
            remotes: vec![LfsRemoteConfig::named("endpointless")],
            fetch_head_url: Some("https://example.test/fetched.git".to_owned()),
            ..LfsEndpointInputs::default()
        };
        assert_eq!(
            http_url(
                resolve_lfs_endpoint(LfsOperation::Fetch, &fetch_head)
                    .expect("FETCH_HEAD fallback")
            ),
            "https://example.test/fetched.git/info/lfs"
        );

        let malformed = LfsEndpointInputs {
            current_remote: Some("broken".to_owned()),
            remotes: vec![
                remote("broken", "ftp://example.test/repo.git"),
                remote("origin", "https://example.test/origin.git"),
            ],
            ..LfsEndpointInputs::default()
        };
        assert!(matches!(
            resolve_lfs_endpoint(LfsOperation::Fetch, &malformed),
            Err(LfsEndpointError::UnsupportedScheme { .. })
        ));
    }

    #[test]
    fn explicit_origin_never_uses_fetch_head() {
        let config = LfsEndpointInputs {
            requested_remote: Some("origin".to_owned()),
            remotes: vec![LfsRemoteConfig::named("origin")],
            fetch_head_url: Some("https://example.test/fetch-head".to_owned()),
            ..LfsEndpointInputs::default()
        };
        assert!(matches!(
            resolve_lfs_endpoint(LfsOperation::Fetch, &config),
            Err(LfsEndpointError::MissingRemoteUrl { .. })
        ));
    }

    #[test]
    fn access_scopes_canonicalize_origin_and_require_path_boundaries() {
        let scope = parse_http_url("HTTPS://[2001:db8::1]/team").expect("scope");
        let covered =
            parse_http_url("https://[2001:db8::1]:443/team/repo/info/lfs").expect("covered");
        let sibling =
            parse_http_url("https://[2001:db8::1]/team-secret/info/lfs").expect("sibling");
        let query = parse_http_url("https://[2001:db8::1]/team?secret=signed").expect("query");
        assert!(scope.is_access_scope_for(&covered));
        assert!(!scope.is_access_scope_for(&sibling));
        assert!(!query.is_access_scope_for(&covered));
    }
}
