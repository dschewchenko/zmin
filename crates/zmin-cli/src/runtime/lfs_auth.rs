//! Typed Git LFS credential and SSH authentication primitives.
//!
//! This module deliberately stops before process and credential-helper I/O.
//! Adapters implement the two small runner/provider traits, while this file
//! owns validation, redaction, expiry handling, and secret lifetime hygiene.
//! The SSH command shape follows the Git LFS authentication API:
//! `git-lfs-authenticate <path-without-leading-slash> download|upload`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::DateTime;

use super::lfs_batch::LfsBatchHeaders;
#[cfg(test)]
use super::lfs_endpoint::{LfsEndpoint, LfsEndpointSource};
use super::lfs_endpoint::{
    LfsHttpUrl, LfsOperation, LfsSshAuthenticationRequest, decode_lfs_url_path, parse_http_url,
};

pub(crate) const LFS_AUTH_MAX_HEADERS: usize = 128;
pub(crate) const LFS_AUTH_MAX_HEADER_NAME_BYTES: usize = 256;
pub(crate) const LFS_AUTH_MAX_HEADER_VALUE_BYTES: usize = 16 * 1024;
pub(crate) const LFS_AUTH_MAX_HEADER_BYTES: usize = 64 * 1024;
pub(crate) const LFS_AUTH_MAX_SSH_STDOUT_BYTES: usize = 64 * 1024;
pub(crate) const LFS_AUTH_MAX_SSH_STDERR_BYTES: usize = 64 * 1024;
pub(crate) const LFS_AUTH_MAX_PATH_BYTES: usize = 8 * 1024;
pub(crate) const LFS_AUTH_MAX_CACHE_ENTRIES: usize = 64;
pub(crate) const LFS_AUTH_EXPIRY_SKEW: Duration = Duration::from_secs(30);
const MAX_JSON_BYTES: usize = LFS_AUTH_MAX_SSH_STDOUT_BYTES;
const MAX_JSON_DEPTH: usize = 16;
const MAX_JSON_MEMBERS: usize = 256;
const SECRET_STRING_INITIAL_CAPACITY: usize = 64;
const MAX_EXPIRY_SECONDS: u64 = 2_147_483_647;
const MAX_SSH_PATH_COMPONENT_BYTES: usize = 255;
const MAX_SSH_DESTINATION_FIELD_BYTES: usize = 4 * 1024;

#[cfg(test)]
static SECRET_WIPE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Sanitized authentication failures.  No variant stores a credential, URL,
/// command argument, or helper output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsAuthError {
    InvalidEndpoint,
    InvalidOrigin,
    InvalidHeader,
    DuplicateHeader,
    TooManyHeaders,
    HeaderTooLarge,
    InvalidJson,
    DuplicateField,
    MissingField,
    InvalidField,
    UnsupportedField,
    InvalidExpiry,
    InvalidCommandPath,
    OutputTooLarge,
    ProcessFailed,
    Timeout,
    CacheLimit,
}

impl fmt::Display for LfsAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidEndpoint => "invalid LFS authentication endpoint",
            Self::InvalidOrigin => "invalid LFS authentication origin",
            Self::InvalidHeader => "invalid LFS authentication header",
            Self::DuplicateHeader => "duplicate LFS authentication header",
            Self::TooManyHeaders => "too many LFS authentication headers",
            Self::HeaderTooLarge => "LFS authentication headers exceed bounds",
            Self::InvalidJson => "invalid LFS authentication JSON",
            Self::DuplicateField => "duplicate LFS authentication field",
            Self::MissingField => "missing LFS authentication field",
            Self::InvalidField => "invalid LFS authentication field",
            Self::UnsupportedField => "unsupported LFS authentication field",
            Self::InvalidExpiry => "invalid LFS authentication expiry",
            Self::InvalidCommandPath => "invalid LFS authentication command path",
            Self::OutputTooLarge => "LFS authentication output exceeds bounds",
            Self::ProcessFailed => "LFS authentication command failed",
            Self::Timeout => "LFS authentication command timed out",
            Self::CacheLimit => "invalid LFS authentication cache limit",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LfsAuthError {}

#[derive(PartialEq, Eq)]
struct SecretBytes(Vec<u8>);

impl SecretBytes {
    fn new(value: Vec<u8>) -> Self {
        Self(value)
    }

    fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        wipe_secret_bytes(&mut self.0);
    }
}

impl Clone for SecretBytes {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// An owned UTF-8 string whose backing bytes are wiped when it is dropped.
///
/// This is used for every decoded JSON string, including values that are only
/// temporarily needed while validating the response.  Keeping the wipe in a
/// named owner means parser errors and partially-built trees get the same
/// cleanup as successful responses.
struct SecretString(Vec<u8>);

impl SecretString {
    fn with_capacity(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }

    fn from_str(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("SecretString UTF-8 invariant")
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn push(&mut self, character: char) {
        let mut encoded = [0_u8; 4];
        let encoded_len = character.encode_utf8(&mut encoded).len();
        let required = self
            .0
            .len()
            .checked_add(encoded_len)
            .expect("SecretString length overflow");
        if required > self.0.capacity() {
            let capacity = self.0.capacity().max(1).saturating_mul(2).max(required);
            let mut replacement = Vec::with_capacity(capacity);
            replacement.extend_from_slice(&self.0);
            wipe_secret_bytes(&mut self.0);
            self.0 = replacement;
        }
        self.0.extend_from_slice(&encoded[..encoded_len]);
        wipe_secret_bytes(&mut encoded[..encoded_len]);
    }

    fn into_string(mut self) -> String {
        // Every constructor and push path maintains valid UTF-8.
        unsafe { String::from_utf8_unchecked(std::mem::take(&mut self.0)) }
    }

    fn clone_secret(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        wipe_secret_bytes(&mut self.0);
    }
}

fn wipe_secret_bytes(bytes: &mut [u8]) {
    bytes.fill(0);
    #[cfg(test)]
    SECRET_WIPE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// A bounded, case-insensitive-unique set of authentication headers.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsAuthHeaders {
    entries: Vec<LfsAuthHeader>,
}

#[derive(Clone, PartialEq, Eq)]
struct LfsAuthHeader {
    name: String,
    value: SecretBytes,
}

impl fmt::Debug for LfsAuthHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsAuthHeaders")
            .field("count", &self.entries.len())
            .field("headers", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for LfsAuthHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsAuthHeader")
            .field("name", &self.name)
            .field("value", &self.value)
            .finish()
    }
}

impl Default for LfsAuthHeaders {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl LfsAuthHeaders {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    fn from_secret_pairs(
        mut pairs: Vec<(SecretString, SecretBytes)>,
    ) -> Result<Self, LfsAuthError> {
        if pairs.len() > LFS_AUTH_MAX_HEADERS {
            return Err(LfsAuthError::TooManyHeaders);
        }
        let mut entries = Vec::with_capacity(pairs.len());
        while let Some((name, value)) = pairs.pop() {
            if let Err(error) = validate_header_name(name.as_str())
                .and_then(|_| validate_header_value(value.as_bytes()))
            {
                return Err(error);
            }
            entries.push(LfsAuthHeader {
                name: name.into_string(),
                value,
            });
        }
        let mut headers = Self { entries };
        headers.sort_and_check()?;
        Ok(headers)
    }

    pub(crate) fn from_pairs(pairs: Vec<(String, Vec<u8>)>) -> Result<Self, LfsAuthError> {
        if pairs.len() > LFS_AUTH_MAX_HEADERS {
            wipe_header_pair_values(pairs);
            return Err(LfsAuthError::TooManyHeaders);
        }
        let mut entries = Vec::with_capacity(pairs.len());
        let mut pairs = pairs.into_iter();
        while let Some((name, mut value)) = pairs.next() {
            if let Err(error) =
                validate_header_name(&name).and_then(|_| validate_header_value(&value))
            {
                value.fill(0);
                wipe_header_pair_values(pairs);
                return Err(error);
            }
            entries.push(LfsAuthHeader {
                name,
                value: SecretBytes::new(value),
            });
        }
        let mut headers = Self { entries };
        headers.sort_and_check()?;
        Ok(headers)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.value.as_bytes()))
    }

    pub(crate) fn merge_action_headers(
        &self,
        action_headers: &LfsBatchHeaders,
    ) -> Result<Self, LfsAuthError> {
        let mut pairs = self
            .entries
            .iter()
            .map(|entry| (entry.name.clone(), entry.value.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        pairs.extend(
            action_headers
                .entries()
                .iter()
                .map(|entry| (entry.name().to_owned(), entry.value().as_bytes().to_vec())),
        );
        Self::from_pairs(pairs)
    }

    fn sort_and_check(&mut self) -> Result<(), LfsAuthError> {
        let mut total = 0_usize;
        for entry in &self.entries {
            total = total
                .checked_add(entry.name.len())
                .and_then(|value| value.checked_add(entry.value.as_bytes().len()))
                .ok_or(LfsAuthError::HeaderTooLarge)?;
        }
        if total > LFS_AUTH_MAX_HEADER_BYTES {
            return Err(LfsAuthError::HeaderTooLarge);
        }
        self.entries.sort_by(|left, right| {
            lower_ascii(&left.name)
                .cmp(&lower_ascii(&right.name))
                .then_with(|| left.name.cmp(&right.name))
        });
        if self
            .entries
            .windows(2)
            .any(|pair| lower_ascii(&pair[0].name) == lower_ascii(&pair[1].name))
        {
            return Err(LfsAuthError::DuplicateHeader);
        }
        Ok(())
    }
}

fn wipe_header_pair_values<I>(pairs: I)
where
    I: IntoIterator<Item = (String, Vec<u8>)>,
{
    for (_, mut value) in pairs {
        value.fill(0);
    }
}

fn validate_header_name(name: &str) -> Result<(), LfsAuthError> {
    if name.is_empty()
        || name.len() > LFS_AUTH_MAX_HEADER_NAME_BYTES
        || !name.bytes().all(is_header_name_byte)
        || is_forbidden_header(name)
    {
        return Err(LfsAuthError::InvalidHeader);
    }
    Ok(())
}

fn validate_header_value(value: &[u8]) -> Result<(), LfsAuthError> {
    if value.len() > LFS_AUTH_MAX_HEADER_VALUE_BYTES
        || !value
            .iter()
            .all(|byte| *byte == b'\t' || (0x20..=0x7e).contains(byte) || *byte >= 0x80)
    {
        return Err(LfsAuthError::InvalidHeader);
    }
    Ok(())
}

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn is_forbidden_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "connection"
            | "content-length"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "forwarded"
            | "x-forwarded-for"
            | "x-forwarded-host"
            | "x-forwarded-proto"
    ) || lower.starts_with("proxy-")
}

fn lower_ascii(value: &str) -> String {
    value.to_ascii_lowercase()
}

/// Canonical origin key used for credential isolation and cache lookup.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct LfsAuthOrigin {
    scheme: String,
    authority: String,
    /// Git LFS includes the URL path in the credential protocol only when
    /// credential.useHttpPath is enabled.  Keeping the optional value on the
    /// scope also makes the in-process cache and approval receipt path-safe.
    credential_path: Option<Vec<u8>>,
}

impl fmt::Debug for LfsAuthOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsAuthOrigin")
            .field("scheme", &self.scheme)
            .field("authority", &self.authority)
            .field("has_credential_path", &self.credential_path.is_some())
            .finish()
    }
}

impl LfsAuthOrigin {
    pub(crate) fn from_endpoint(endpoint: &LfsHttpUrl) -> Result<Self, LfsAuthError> {
        let raw = endpoint.as_str();
        parse_http_url(raw).map_err(|_| LfsAuthError::InvalidEndpoint)?;
        let scheme_end = raw.find("://").ok_or(LfsAuthError::InvalidOrigin)?;
        let scheme = raw[..scheme_end].to_ascii_lowercase();
        let remainder = &raw[scheme_end + 3..];
        let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
        let authority = &remainder[..authority_end];
        if authority.is_empty() || authority.contains('@') {
            return Err(LfsAuthError::InvalidOrigin);
        }
        let authority = canonical_authority(authority, &scheme)?;
        Ok(Self {
            scheme,
            authority,
            credential_path: None,
        })
    }

    pub(crate) fn from_endpoint_with_http_path(
        endpoint: &LfsHttpUrl,
        use_http_path: bool,
    ) -> Result<Self, LfsAuthError> {
        let mut origin = Self::from_endpoint(endpoint)?;
        if use_http_path {
            origin.credential_path = Some(endpoint_credential_path(endpoint)?);
        }
        Ok(origin)
    }

    pub(crate) fn scheme(&self) -> &str {
        &self.scheme
    }

    pub(crate) fn authority(&self) -> &str {
        &self.authority
    }

    pub(crate) fn credential_path(&self) -> Option<&[u8]> {
        self.credential_path.as_deref()
    }
}

fn endpoint_credential_path(endpoint: &LfsHttpUrl) -> Result<Vec<u8>, LfsAuthError> {
    let raw = endpoint.as_str();
    let authority_start = raw.find("://").ok_or(LfsAuthError::InvalidOrigin)? + 3;
    let remainder = &raw[authority_start..];
    let path_start = remainder.find('/').unwrap_or(remainder.len());
    let path_and_query = &remainder[path_start..];
    let path = path_and_query
        .split_once('?')
        .map_or(path_and_query, |(path, _query)| path);
    let decoded = decode_lfs_url_path(path).map_err(|_| LfsAuthError::InvalidOrigin)?;
    let path = decoded.strip_prefix(b"/").unwrap_or(&decoded);
    if path.len() > LFS_AUTH_MAX_PATH_BYTES {
        return Err(LfsAuthError::InvalidOrigin);
    }
    Ok(path.to_vec())
}

fn canonical_authority(raw: &str, scheme: &str) -> Result<String, LfsAuthError> {
    let default_port = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return Err(LfsAuthError::InvalidOrigin),
    };
    let (host, port) = if raw.starts_with('[') {
        let closing = raw.find(']').ok_or(LfsAuthError::InvalidOrigin)?;
        let host = &raw[1..closing];
        let port = raw[closing + 1..]
            .strip_prefix(':')
            .map(parse_port)
            .transpose()?
            .unwrap_or(default_port);
        (format!("[{}]", host.to_ascii_lowercase()), port)
    } else if let Some((host, port)) = raw.rsplit_once(':') {
        if host.contains(':') {
            return Err(LfsAuthError::InvalidOrigin);
        }
        (host.to_ascii_lowercase(), parse_port(port)?)
    } else {
        (raw.to_ascii_lowercase(), default_port)
    };
    if host.is_empty() || host.contains(['/', '?', '#', '\\']) {
        return Err(LfsAuthError::InvalidOrigin);
    }
    Ok(format!("{host}:{port}"))
}

fn parse_port(value: &str) -> Result<u16, LfsAuthError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LfsAuthError::InvalidOrigin);
    }
    let port = value
        .parse::<u32>()
        .map_err(|_| LfsAuthError::InvalidOrigin)?;
    if port == 0 || port > u16::MAX as u32 {
        return Err(LfsAuthError::InvalidOrigin);
    }
    Ok(port as u16)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LfsAuthExpiry {
    at: Option<SystemTime>,
}

impl LfsAuthExpiry {
    pub(crate) fn never() -> Self {
        Self { at: None }
    }

    pub(crate) fn at(at: SystemTime) -> Self {
        Self { at: Some(at) }
    }

    pub(crate) fn expires_at(self) -> Option<SystemTime> {
        self.at
    }

    fn usable(self, now: SystemTime) -> bool {
        self.at.is_none_or(|at| {
            at.duration_since(now)
                .is_ok_and(|remaining| remaining > LFS_AUTH_EXPIRY_SKEW)
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsAuthCredentials {
    headers: LfsAuthHeaders,
    expiry: LfsAuthExpiry,
}

impl fmt::Debug for LfsAuthCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsAuthCredentials")
            .field("headers", &self.headers)
            .field("expiry", &self.expiry)
            .finish()
    }
}

impl LfsAuthCredentials {
    pub(crate) fn new(headers: LfsAuthHeaders, expiry: LfsAuthExpiry) -> Self {
        Self { headers, expiry }
    }

    pub(crate) fn headers(&self) -> &LfsAuthHeaders {
        &self.headers
    }

    pub(crate) fn expiry(&self) -> LfsAuthExpiry {
        self.expiry
    }
}

/// Provider boundary for Git credential helpers or other credential stores.
pub(crate) trait LfsCredentialProvider {
    fn credentials(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<Option<LfsAuthCredentials>, LfsAuthError>;
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct LfsAuthCacheKey {
    origin: LfsAuthOrigin,
    operation: u8,
}

/// Small bounded cache.  Entries within the expiry skew are never returned.
pub(crate) struct LfsAuthCache {
    entries: BTreeMap<LfsAuthCacheKey, LfsAuthCredentials>,
    max_entries: usize,
}

impl fmt::Debug for LfsAuthCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsAuthCache")
            .field("entries", &self.entries.len())
            .field("max_entries", &self.max_entries)
            .finish()
    }
}

impl LfsAuthCache {
    pub(crate) fn new(max_entries: usize) -> Result<Self, LfsAuthError> {
        if max_entries > LFS_AUTH_MAX_CACHE_ENTRIES {
            return Err(LfsAuthError::CacheLimit);
        }
        Ok(Self {
            entries: BTreeMap::new(),
            max_entries,
        })
    }

    pub(crate) fn get_or_fetch<P: LfsCredentialProvider>(
        &mut self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
        now: SystemTime,
        provider: &P,
    ) -> Result<Option<LfsAuthCredentials>, LfsAuthError> {
        let expected = LfsAuthOrigin::from_endpoint_with_http_path(
            endpoint,
            origin.credential_path.is_some(),
        )?;
        if &expected != origin {
            return Err(LfsAuthError::InvalidOrigin);
        }
        let key = LfsAuthCacheKey {
            origin: origin.clone(),
            operation: operation_key(operation),
        };
        if let Some(credentials) = self.entries.get(&key) {
            if credentials.expiry.usable(now) {
                return Ok(Some(credentials.clone()));
            }
        }
        self.entries.remove(&key);
        let credentials = provider.credentials(endpoint, origin, operation)?;
        if let Some(credentials) = credentials.as_ref() {
            self.insert(endpoint, origin, operation, now, credentials.clone())?;
        }
        Ok(credentials)
    }

    pub(crate) fn insert(
        &mut self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
        now: SystemTime,
        credentials: LfsAuthCredentials,
    ) -> Result<(), LfsAuthError> {
        let expected = LfsAuthOrigin::from_endpoint_with_http_path(
            endpoint,
            origin.credential_path.is_some(),
        )?;
        if &expected != origin {
            return Err(LfsAuthError::InvalidOrigin);
        }
        if !credentials.expiry.usable(now) {
            return Err(LfsAuthError::InvalidExpiry);
        }
        if self.max_entries == 0 {
            return Ok(());
        }
        let key = LfsAuthCacheKey {
            origin: origin.clone(),
            operation: operation_key(operation),
        };
        if self.entries.len() >= self.max_entries
            && !self.entries.contains_key(&key)
            && let Some(first) = self.entries.keys().next().cloned()
        {
            self.entries.remove(&first);
        }
        self.entries.insert(key, credentials);
        Ok(())
    }

    pub(crate) fn invalidate(&mut self, origin: &LfsAuthOrigin, operation: LfsOperation) -> bool {
        self.entries
            .remove(&LfsAuthCacheKey {
                origin: origin.clone(),
                operation: operation_key(operation),
            })
            .is_some()
    }
}

fn operation_key(operation: LfsOperation) -> u8 {
    match operation {
        LfsOperation::Fetch => 0,
        LfsOperation::Push => 1,
    }
}

/// Bounded output returned by a platform SSH process adapter.
pub(crate) struct LfsSshCommandOutput {
    pub(crate) status: i32,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl Drop for LfsSshCommandOutput {
    fn drop(&mut self) {
        for byte in &mut self.stdout {
            *byte = 0;
        }
        for byte in &mut self.stderr {
            *byte = 0;
        }
    }
}

/// Full SSH destination context passed to the process adapter.
///
/// Keeping this separate from the command arguments prevents an adapter from
/// having to capture user, host, or port state elsewhere when constructing
/// the platform-specific SSH invocation.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsSshDestination {
    user: Option<String>,
    host: String,
    port: Option<u16>,
}

impl fmt::Debug for LfsSshDestination {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsSshDestination")
            .field("user", &self.user)
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

impl LfsSshDestination {
    fn from_request(request: &LfsSshAuthenticationRequest) -> Result<Self, LfsAuthError> {
        validate_destination_field(&request.host)?;
        if request.host.contains(['/', '\\', '@']) {
            return Err(LfsAuthError::InvalidCommandPath);
        }
        let user = request
            .user
            .as_deref()
            .map(|value| {
                validate_destination_field(value)?;
                if value.contains(['/', '\\', '@']) {
                    return Err(LfsAuthError::InvalidCommandPath);
                }
                Ok(value.to_owned())
            })
            .transpose()?;
        let port = request
            .port
            .as_deref()
            .map(|value| {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(LfsAuthError::InvalidCommandPath);
                }
                let port = value
                    .parse::<u16>()
                    .map_err(|_| LfsAuthError::InvalidCommandPath)?;
                if port == 0 {
                    return Err(LfsAuthError::InvalidCommandPath);
                }
                Ok(port)
            })
            .transpose()?;
        Ok(Self {
            user,
            host: request.host.clone(),
            port,
        })
    }

    pub(crate) fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn port(&self) -> Option<u16> {
        self.port
    }
}

fn validate_destination_field(value: &str) -> Result<(), LfsAuthError> {
    if value.is_empty()
        || value.len() > MAX_SSH_DESTINATION_FIELD_BYTES
        || value.chars().any(|character| character.is_control())
        || value.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(LfsAuthError::InvalidCommandPath);
    }
    Ok(())
}

/// Process boundary.  Implementations must enforce the supplied timeout and
/// output limits; the parser checks them again before handling any bytes.
pub(crate) trait LfsSshCommandRunner {
    fn run(
        &self,
        destination: &LfsSshDestination,
        program: &str,
        args: &[String],
        timeout: Duration,
        stdout_limit: usize,
        stderr_limit: usize,
    ) -> Result<LfsSshCommandOutput, LfsAuthError>;
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsSshAuthenticationResult {
    href: LfsHttpUrl,
    credentials: LfsAuthCredentials,
}

impl fmt::Debug for LfsSshAuthenticationResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsSshAuthenticationResult")
            .field("href", &"<redacted>")
            .field("credentials", &self.credentials)
            .finish()
    }
}

pub(crate) fn authenticate_ssh<R: LfsSshCommandRunner>(
    runner: &R,
    request: &LfsSshAuthenticationRequest,
    default_href: &LfsHttpUrl,
    timeout: Duration,
    now: SystemTime,
) -> Result<LfsSshAuthenticationResult, LfsAuthError> {
    let destination = LfsSshDestination::from_request(request)?;
    let path = normalize_auth_repo_path(&request.path)?;
    let operation = match request.operation {
        LfsOperation::Fetch => "download",
        LfsOperation::Push => "upload",
    };
    let args = vec![path.to_owned(), operation.to_owned()];
    let output = runner.run(
        &destination,
        "git-lfs-authenticate",
        &args,
        timeout,
        LFS_AUTH_MAX_SSH_STDOUT_BYTES,
        LFS_AUTH_MAX_SSH_STDERR_BYTES,
    )?;
    if output.stdout.len() > LFS_AUTH_MAX_SSH_STDOUT_BYTES
        || output.stderr.len() > LFS_AUTH_MAX_SSH_STDERR_BYTES
    {
        return Err(LfsAuthError::OutputTooLarge);
    }
    if output.status != 0 {
        return Err(LfsAuthError::ProcessFailed);
    }
    parse_ssh_auth_response(&output.stdout, default_href, now)
}

fn normalize_auth_repo_path(path: &str) -> Result<String, LfsAuthError> {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() || path.len() > LFS_AUTH_MAX_PATH_BYTES || path.contains('\\') {
        return Err(LfsAuthError::InvalidCommandPath);
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.len() > MAX_SSH_PATH_COMPONENT_BYTES
            || component.starts_with('-')
            || component.chars().any(|character| character.is_control())
        {
            return Err(LfsAuthError::InvalidCommandPath);
        }
    }
    Ok(path.to_owned())
}

pub(crate) fn parse_ssh_auth_response(
    output: &[u8],
    default_href: &LfsHttpUrl,
    now: SystemTime,
) -> Result<LfsSshAuthenticationResult, LfsAuthError> {
    if output.len() > MAX_JSON_BYTES {
        return Err(LfsAuthError::OutputTooLarge);
    }
    let root = parse_json(output)?;
    let members = root.as_object().ok_or(LfsAuthError::InvalidJson)?;
    let mut href = None;
    let mut header_pairs = Vec::new();
    let mut expiry_at = None;
    let mut expiry_in = None;
    let mut seen = BTreeSet::new();
    for member in members {
        if !seen.insert(member.name.as_str()) {
            return Err(LfsAuthError::DuplicateField);
        }
        match member.name.as_str() {
            "href" => {
                href = Some(
                    member
                        .value
                        .as_str()
                        .ok_or(LfsAuthError::InvalidField)
                        .map(SecretString::from_str)?,
                )
            }
            "header" => {
                let headers = member.value.as_object().ok_or(LfsAuthError::InvalidField)?;
                for header in headers {
                    let value = header.value.as_str().ok_or(LfsAuthError::InvalidField)?;
                    header_pairs.push((
                        header.name.clone_secret(),
                        SecretBytes::new(value.as_bytes().to_vec()),
                    ));
                }
            }
            "expires_at" => {
                expiry_at = Some(
                    member
                        .value
                        .as_str()
                        .ok_or(LfsAuthError::InvalidExpiry)
                        .map(SecretString::from_str)?,
                );
            }
            "expires_in" => {
                expiry_in = Some(
                    member
                        .value
                        .as_number()
                        .ok_or(LfsAuthError::InvalidExpiry)
                        .map(SecretString::from_str)?,
                );
            }
            _ => return Err(LfsAuthError::UnsupportedField),
        }
    }
    let href = match href {
        Some(href) => parse_http_url(href.as_str()).map_err(|_| LfsAuthError::InvalidEndpoint)?,
        None => default_href.clone(),
    };
    let headers = LfsAuthHeaders::from_secret_pairs(header_pairs)?;
    let expiry = match (expiry_at, expiry_in) {
        (Some(_), Some(_)) => return Err(LfsAuthError::DuplicateField),
        (Some(value), None) => LfsAuthExpiry::at(parse_expiry_at(value.as_str())?),
        (None, Some(value)) => LfsAuthExpiry::at(parse_expiry_in(value.as_str(), now)?),
        (None, None) => LfsAuthExpiry::never(),
    };
    if !expiry.usable(now) {
        return Err(LfsAuthError::InvalidExpiry);
    }
    Ok(LfsSshAuthenticationResult {
        href,
        credentials: LfsAuthCredentials::new(headers, expiry),
    })
}

impl LfsSshAuthenticationResult {
    pub(crate) fn href(&self) -> &LfsHttpUrl {
        &self.href
    }

    pub(crate) fn credentials(&self) -> &LfsAuthCredentials {
        &self.credentials
    }
}

fn parse_expiry_at(value: &str) -> Result<SystemTime, LfsAuthError> {
    let date = DateTime::parse_from_rfc3339(value).map_err(|_| LfsAuthError::InvalidExpiry)?;
    let timestamp = date
        .timestamp()
        .try_into()
        .map_err(|_| LfsAuthError::InvalidExpiry)?;
    UNIX_EPOCH
        .checked_add(Duration::from_secs(timestamp))
        .ok_or(LfsAuthError::InvalidExpiry)
}

fn parse_expiry_in(value: &str, now: SystemTime) -> Result<SystemTime, LfsAuthError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LfsAuthError::InvalidExpiry);
    }
    let seconds = value
        .parse::<u64>()
        .map_err(|_| LfsAuthError::InvalidExpiry)?;
    if seconds > MAX_EXPIRY_SECONDS {
        return Err(LfsAuthError::InvalidExpiry);
    }
    now.checked_add(Duration::from_secs(seconds))
        .ok_or(LfsAuthError::InvalidExpiry)
}

enum AuthJsonValue {
    String(SecretString),
    Number(SecretString),
    Object(Vec<AuthJsonMember>),
    Boolean,
    Null,
}

impl AuthJsonValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<&str> {
        match self {
            Self::Number(value) => Some(value.as_str()),
            _ => None,
        }
    }

    fn as_object(&self) -> Option<&[AuthJsonMember]> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }
}

struct AuthJsonMember {
    name: SecretString,
    value: AuthJsonValue,
}

fn parse_json(input: &[u8]) -> Result<AuthJsonValue, LfsAuthError> {
    let mut parser = AuthJsonParser {
        input,
        position: 0,
        members: 0,
    };
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.position != input.len() {
        return Err(LfsAuthError::InvalidJson);
    }
    Ok(value)
}

struct AuthJsonParser<'a> {
    input: &'a [u8],
    position: usize,
    members: usize,
}

impl<'a> AuthJsonParser<'a> {
    fn value(&mut self, depth: usize) -> Result<AuthJsonValue, LfsAuthError> {
        if depth > MAX_JSON_DEPTH {
            return Err(LfsAuthError::InvalidJson);
        }
        self.whitespace();
        match self.input.get(self.position).copied() {
            Some(b'"') => Ok(AuthJsonValue::String(self.string()?)),
            Some(b'{') => self.object(depth + 1),
            Some(b't') => {
                self.literal(b"true")?;
                Ok(AuthJsonValue::Boolean)
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Ok(AuthJsonValue::Boolean)
            }
            Some(b'n') => {
                self.literal(b"null")?;
                Ok(AuthJsonValue::Null)
            }
            Some(b'-' | b'0'..=b'9') => Ok(AuthJsonValue::Number(self.number()?)),
            _ => Err(LfsAuthError::InvalidJson),
        }
    }

    fn object(&mut self, depth: usize) -> Result<AuthJsonValue, LfsAuthError> {
        self.position += 1;
        let mut members = Vec::new();
        self.whitespace();
        if self.input.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Ok(AuthJsonValue::Object(members));
        }
        loop {
            self.whitespace();
            if self.input.get(self.position) != Some(&b'"') {
                return Err(LfsAuthError::InvalidJson);
            }
            let name = self.string()?;
            self.whitespace();
            if self.input.get(self.position) != Some(&b':') {
                return Err(LfsAuthError::InvalidJson);
            }
            self.position += 1;
            let value = self.value(depth)?;
            self.members += 1;
            if self.members > MAX_JSON_MEMBERS {
                return Err(LfsAuthError::InvalidJson);
            }
            members.push(AuthJsonMember { name, value });
            self.whitespace();
            match self.input.get(self.position) {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(AuthJsonValue::Object(members));
                }
                _ => return Err(LfsAuthError::InvalidJson),
            }
        }
    }

    fn string(&mut self) -> Result<SecretString, LfsAuthError> {
        if self.input.get(self.position) != Some(&b'"') {
            return Err(LfsAuthError::InvalidJson);
        }
        self.position += 1;
        let mut value = SecretString::with_capacity(
            self.input
                .len()
                .saturating_sub(self.position)
                .min(SECRET_STRING_INITIAL_CAPACITY),
        );
        loop {
            let byte = *self
                .input
                .get(self.position)
                .ok_or(LfsAuthError::InvalidJson)?;
            self.position += 1;
            match byte {
                b'"' => return Ok(value),
                b'\\' => self.escape(&mut value)?,
                0..=0x1f => return Err(LfsAuthError::InvalidJson),
                0x80..=0xff => {
                    let start = self.position - 1;
                    let tail = self.input.get(start..).ok_or(LfsAuthError::InvalidJson)?;
                    let text = std::str::from_utf8(tail).map_err(|_| LfsAuthError::InvalidJson)?;
                    let character = text.chars().next().ok_or(LfsAuthError::InvalidJson)?;
                    value.push(character);
                    self.position = start + character.len_utf8();
                }
                _ => value.push(byte as char),
            }
            if value.len() > MAX_JSON_BYTES {
                return Err(LfsAuthError::OutputTooLarge);
            }
        }
    }

    fn escape(&mut self, value: &mut SecretString) -> Result<(), LfsAuthError> {
        let byte = *self
            .input
            .get(self.position)
            .ok_or(LfsAuthError::InvalidJson)?;
        self.position += 1;
        match byte {
            b'"' => value.push('"'),
            b'\\' => value.push('\\'),
            b'/' => value.push('/'),
            b'b' => value.push('\u{0008}'),
            b'f' => value.push('\u{000c}'),
            b'n' => value.push('\n'),
            b'r' => value.push('\r'),
            b't' => value.push('\t'),
            b'u' => self.unicode_escape(value)?,
            _ => return Err(LfsAuthError::InvalidJson),
        }
        Ok(())
    }

    fn unicode_escape(&mut self, value: &mut SecretString) -> Result<(), LfsAuthError> {
        let high = self.hex_quad()?;
        let code = match high {
            0xd800..=0xdbff => {
                if self.input.get(self.position..self.position + 2) != Some(b"\\u") {
                    return Err(LfsAuthError::InvalidJson);
                }
                self.position += 2;
                let low = self.hex_quad()?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err(LfsAuthError::InvalidJson);
                }
                0x1_0000 + (u32::from(high) - 0xd800) * 0x400 + (u32::from(low) - 0xdc00)
            }
            0xdc00..=0xdfff => return Err(LfsAuthError::InvalidJson),
            value => u32::from(value),
        };
        value.push(char::from_u32(code).ok_or(LfsAuthError::InvalidJson)?);
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u16, LfsAuthError> {
        let mut value = 0_u16;
        for _ in 0..4 {
            value = (value << 4)
                | u16::from(hex_value(
                    *self
                        .input
                        .get(self.position)
                        .ok_or(LfsAuthError::InvalidJson)?,
                )?);
            self.position += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<SecretString, LfsAuthError> {
        let start = self.position;
        if self.input.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        match self.input.get(self.position) {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                self.position += 1;
                while self
                    .input
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    self.position += 1;
                }
            }
            _ => return Err(LfsAuthError::InvalidJson),
        }
        if self.input.get(self.position) == Some(&b'.') {
            self.position += 1;
            let before = self.position;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if before == self.position {
                return Err(LfsAuthError::InvalidJson);
            }
        }
        if self
            .input
            .get(self.position)
            .is_some_and(|byte| *byte == b'e' || *byte == b'E')
        {
            self.position += 1;
            if self
                .input
                .get(self.position)
                .is_some_and(|byte| *byte == b'+' || *byte == b'-')
            {
                self.position += 1;
            }
            let before = self.position;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if before == self.position {
                return Err(LfsAuthError::InvalidJson);
            }
        }
        let mut value = SecretString::with_capacity(self.position.saturating_sub(start));
        for byte in &self.input[start..self.position] {
            value.push(*byte as char);
        }
        Ok(value)
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), LfsAuthError> {
        let end = self
            .position
            .checked_add(literal.len())
            .ok_or(LfsAuthError::InvalidJson)?;
        if self.input.get(self.position..end) != Some(literal) {
            return Err(LfsAuthError::InvalidJson);
        }
        self.position = end;
        Ok(())
    }

    fn whitespace(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.position += 1;
        }
    }
}

fn hex_value(byte: u8) -> Result<u8, LfsAuthError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(LfsAuthError::InvalidJson),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    fn endpoint(raw: &str) -> LfsHttpUrl {
        parse_http_url(raw).expect("endpoint")
    }

    fn ssh_request(path: &str, operation: LfsOperation) -> LfsSshAuthenticationRequest {
        LfsSshAuthenticationRequest {
            operation,
            user: Some("git".to_owned()),
            host: "example.test".to_owned(),
            port: None,
            path: path.to_owned(),
            default_endpoint: LfsEndpoint {
                operation,
                source: LfsEndpointSource::DerivedRemote,
                url: endpoint("https://example.test/repo.git/info/lfs"),
            },
        }
    }

    struct Runner {
        output: RefCell<Option<LfsSshCommandOutput>>,
        expected_port: Option<u16>,
    }

    impl LfsSshCommandRunner for Runner {
        fn run(
            &self,
            destination: &LfsSshDestination,
            program: &str,
            args: &[String],
            _timeout: Duration,
            _stdout_limit: usize,
            _stderr_limit: usize,
        ) -> Result<LfsSshCommandOutput, LfsAuthError> {
            assert_eq!(destination.user(), Some("git"));
            assert_eq!(destination.host(), "example.test");
            assert_eq!(destination.port(), self.expected_port);
            assert_eq!(program, "git-lfs-authenticate");
            assert_eq!(args, &["team/repo.git".to_owned(), "download".to_owned()]);
            Ok(self.output.borrow_mut().take().expect("output"))
        }
    }

    #[test]
    fn parses_official_ssh_vector_and_normalizes_leading_slash() {
        let runner = Runner {
            output: RefCell::new(Some(LfsSshCommandOutput {
                status: 0,
                stdout: br#"{"href":"https://objects.example.test/a?sig=secret","header":{"Authorization":"Bearer secret"},"expires_in":3600}"#.to_vec(),
                stderr: Vec::new(),
            })),
            expected_port: None,
        };
        let result = authenticate_ssh(
            &runner,
            &ssh_request("/team/repo.git", LfsOperation::Fetch),
            &endpoint("https://default.example.test/info/lfs"),
            Duration::from_secs(5),
            now(),
        )
        .expect("auth");
        assert_eq!(
            result.href().as_str(),
            "https://objects.example.test/a?sig=secret"
        );
        assert_eq!(result.credentials().headers().iter().count(), 1);
        assert_eq!(
            result.credentials().expiry().expires_at(),
            Some(now() + Duration::from_secs(3600))
        );
    }

    #[test]
    fn parser_wipes_owned_strings_on_success_and_error_paths() {
        let default = endpoint("https://default.example.test/info/lfs");
        let before_success = SECRET_WIPE_COUNT.load(Ordering::Relaxed);
        {
            let result = parse_ssh_auth_response(
                br#"{"href":"https://objects.example.test/a?sig=secret","header":{"Authorization":"Bearer secret"},"expires_in":3600}"#,
                &default,
                now(),
            )
            .expect("auth");
            assert_eq!(result.credentials().headers().iter().count(), 1);
        }
        assert!(SECRET_WIPE_COUNT.load(Ordering::Relaxed) > before_success);

        let malformed = br#"{"header":{"Authorization":"unterminated}"#;
        let before_malformed = SECRET_WIPE_COUNT.load(Ordering::Relaxed);
        assert_eq!(
            parse_ssh_auth_response(malformed, &default, now()),
            Err(LfsAuthError::InvalidJson)
        );
        assert!(SECRET_WIPE_COUNT.load(Ordering::Relaxed) > before_malformed);

        let duplicate_headers = br#"{"header":{"Authorization":"one","authorization":"two"}}"#;
        let before_duplicate_headers = SECRET_WIPE_COUNT.load(Ordering::Relaxed);
        assert_eq!(
            parse_ssh_auth_response(duplicate_headers, &default, now()),
            Err(LfsAuthError::DuplicateHeader)
        );
        assert!(SECRET_WIPE_COUNT.load(Ordering::Relaxed) > before_duplicate_headers);

        let duplicate = br#"{"href":"https://objects.example.test/one","href":"https://objects.example.test/two"}"#;
        let before_duplicate = SECRET_WIPE_COUNT.load(Ordering::Relaxed);
        assert_eq!(
            parse_ssh_auth_response(duplicate, &default, now()),
            Err(LfsAuthError::DuplicateField)
        );
        assert!(SECRET_WIPE_COUNT.load(Ordering::Relaxed) > before_duplicate);
    }

    #[test]
    fn rejects_malicious_json_url_headers_and_expiry() {
        for json in [
            br#"{"href":"ftp://example.test/a"}"#.as_slice(),
            br#"{"href":"https://user:secret@example.test/a"}"#.as_slice(),
            br#"{"href":"https://example.test/a","header":{"Host":"evil"}}"#.as_slice(),
            br#"{"href":"https://example.test/a","header":{"Authorization":"a\r\nb"}}"#.as_slice(),
            br#"{"href":"https://example.test/a","expires_in":2147483648}"#.as_slice(),
            br#"{"href":"https://example.test/a","expires_at":"not-a-date"}"#.as_slice(),
            br#"{"href":"https://example.test/a","unknown":"x"}"#.as_slice(),
        ] {
            assert!(
                parse_ssh_auth_response(
                    json,
                    &endpoint("https://default.example.test/info/lfs"),
                    now()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn optional_href_uses_discovered_default_endpoint() {
        let default = endpoint("https://default.example.test/team/info/lfs");
        let result = parse_ssh_auth_response(br#"{"expires_in":3600}"#, &default, now())
            .expect("default href");
        assert_eq!(result.href().as_str(), default.as_str());
    }

    #[test]
    fn ssh_destination_context_preserves_user_host_and_port() {
        let runner = Runner {
            output: RefCell::new(Some(LfsSshCommandOutput {
                status: 0,
                stdout: br#"{"expires_in":3600}"#.to_vec(),
                stderr: Vec::new(),
            })),
            expected_port: Some(2222),
        };
        let mut request = ssh_request("/team/repo.git", LfsOperation::Fetch);
        request.port = Some("2222".to_owned());
        let result = authenticate_ssh(
            &runner,
            &request,
            &endpoint("https://default.example.test/info/lfs"),
            Duration::from_secs(5),
            now(),
        )
        .expect("auth");
        assert_eq!(
            result.href().as_str(),
            "https://default.example.test/info/lfs"
        );
    }

    #[test]
    fn rejects_unsafe_auth_repo_paths_before_runner_invocation() {
        let runner = Runner {
            output: RefCell::new(None),
            expected_port: None,
        };
        let oversized_component = format!("/team/{}.git", "x".repeat(256));
        let paths = [
            "",
            ".",
            "..",
            "/",
            "/.",
            "/..",
            "/team/../repo.git",
            "/team/./repo.git",
            "/team//repo.git",
            "/team\\repo.git",
            "/-team/repo.git",
            "/team/-repo.git",
            "/team/line\nrepo.git",
        ];
        for path in paths {
            assert_eq!(
                authenticate_ssh(
                    &runner,
                    &ssh_request(path, LfsOperation::Fetch),
                    &endpoint("https://default.example.test/info/lfs"),
                    Duration::from_secs(1),
                    now(),
                ),
                Err(LfsAuthError::InvalidCommandPath)
            );
        }
        assert_eq!(
            authenticate_ssh(
                &runner,
                &ssh_request(&oversized_component, LfsOperation::Fetch),
                &endpoint("https://default.example.test/info/lfs"),
                Duration::from_secs(1),
                now(),
            ),
            Err(LfsAuthError::InvalidCommandPath)
        );
    }

    #[test]
    fn parses_surrogate_pairs_and_rejects_invalid_pairs() {
        let default = endpoint("https://default.example.test/info/lfs");
        let result =
            parse_ssh_auth_response(br#"{"header":{"X-Emoji":"\uD83D\uDE00"}}"#, &default, now())
                .expect("surrogate pair");
        let (_, value) = result
            .credentials()
            .headers()
            .iter()
            .next()
            .expect("header");
        assert_eq!(value, "😀".as_bytes());
        for json in [
            br#"{"header":{"X":"\uD83D"}}"#.as_slice(),
            br#"{"header":{"X":"\uDE00"}}"#.as_slice(),
            br#"{"header":{"X":"\uD83D\u0041"}}"#.as_slice(),
        ] {
            assert_eq!(
                parse_ssh_auth_response(json, &default, now()),
                Err(LfsAuthError::InvalidJson)
            );
        }
    }

    #[test]
    fn expiry_within_skew_is_rejected() {
        let default = endpoint("https://default.example.test/info/lfs");
        assert_eq!(
            parse_ssh_auth_response(br#"{"expires_in":30}"#, &default, now()),
            Err(LfsAuthError::InvalidExpiry)
        );
    }

    #[test]
    fn rejects_duplicate_headers_and_fields_without_secret_in_errors() {
        let error = parse_ssh_auth_response(
            br#"{"href":"https://example.test/a","header":{"Authorization":"one","authorization":"two"}}"#,
            &endpoint("https://default.example.test/info/lfs"),
            now(),
        )
        .expect_err("duplicate headers");
        assert_eq!(error, LfsAuthError::DuplicateHeader);
        let error = parse_ssh_auth_response(
            br#"{"href":"https://example.test/a","expires_in":1,"expires_in":2}"#,
            &endpoint("https://default.example.test/info/lfs"),
            now(),
        )
        .expect_err("duplicate field");
        assert_eq!(error, LfsAuthError::DuplicateField);
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn auth_header_constructor_rejects_late_invalid_and_oversized_inputs() {
        let late_invalid = vec![
            ("Authorization".to_owned(), b"Bearer first-secret".to_vec()),
            ("X-Invalid".to_owned(), b"second\nsecret".to_vec()),
            ("X-Unprocessed".to_owned(), b"third-secret".to_vec()),
        ];
        assert_eq!(
            LfsAuthHeaders::from_pairs(late_invalid),
            Err(LfsAuthError::InvalidHeader)
        );

        let oversized = (0..=LFS_AUTH_MAX_HEADERS)
            .map(|index| (format!("X-Secret-{index}"), b"secret".to_vec()))
            .collect();
        assert_eq!(
            LfsAuthHeaders::from_pairs(oversized),
            Err(LfsAuthError::TooManyHeaders)
        );
    }

    #[test]
    fn credentials_and_action_headers_never_override_each_other() {
        let credentials = LfsAuthHeaders::from_pairs(vec![(
            "Authorization".to_owned(),
            b"Bearer secret".to_vec(),
        )])
        .expect("credentials");
        let action = LfsBatchHeaders::new(vec![
            super::super::lfs_batch::LfsBatchHeader::new("X-Signed", "value").expect("header"),
        ])
        .expect("action");
        let merged = credentials.merge_action_headers(&action).expect("merge");
        assert_eq!(merged.iter().count(), 2);
        let duplicate = LfsBatchHeaders::new(vec![
            super::super::lfs_batch::LfsBatchHeader::new("authorization", "other").expect("header"),
        ])
        .expect("action");
        assert_eq!(
            credentials.merge_action_headers(&duplicate),
            Err(LfsAuthError::DuplicateHeader)
        );
    }

    struct Provider {
        calls: RefCell<usize>,
        credentials: Option<LfsAuthCredentials>,
    }

    impl LfsCredentialProvider for Provider {
        fn credentials(
            &self,
            _endpoint: &LfsHttpUrl,
            _origin: &LfsAuthOrigin,
            _operation: LfsOperation,
        ) -> Result<Option<LfsAuthCredentials>, LfsAuthError> {
            *self.calls.borrow_mut() += 1;
            Ok(self.credentials.clone())
        }
    }

    #[test]
    fn cache_is_origin_and_operation_scoped_and_expires_with_skew() {
        let ep = endpoint("https://example.test/info/lfs");
        let origin = LfsAuthOrigin::from_endpoint(&ep).expect("origin");
        assert_eq!(
            origin,
            LfsAuthOrigin::from_endpoint(&endpoint("HTTPS://EXAMPLE.TEST:443/other"))
                .expect("same origin")
        );
        let provider = Provider {
            calls: RefCell::new(0),
            credentials: Some(LfsAuthCredentials::new(
                LfsAuthHeaders::from_pairs(vec![("Authorization".to_owned(), b"secret".to_vec())])
                    .expect("headers"),
                LfsAuthExpiry::at(now() + Duration::from_secs(60)),
            )),
        };
        let mut cache = LfsAuthCache::new(2).expect("cache");
        cache
            .get_or_fetch(&ep, &origin, LfsOperation::Fetch, now(), &provider)
            .expect("fetch");
        cache
            .get_or_fetch(
                &ep,
                &origin,
                LfsOperation::Fetch,
                now() + Duration::from_secs(1),
                &provider,
            )
            .expect("cached");
        assert_eq!(*provider.calls.borrow(), 1);
        assert_eq!(
            cache.get_or_fetch(
                &ep,
                &origin,
                LfsOperation::Fetch,
                now() + Duration::from_secs(31),
                &provider,
            ),
            Err(LfsAuthError::InvalidExpiry)
        );
        assert_eq!(*provider.calls.borrow(), 2);
        assert!(matches!(
            cache.get_or_fetch(
                &ep,
                &LfsAuthOrigin::from_endpoint(&endpoint("https://other.test/info/lfs"))
                    .expect("origin"),
                LfsOperation::Fetch,
                now(),
                &provider
            ),
            Err(LfsAuthError::InvalidOrigin)
        ));
    }

    #[test]
    fn cache_rejects_expired_credentials_from_any_provider() {
        let ep = endpoint("https://example.test/info/lfs");
        let origin = LfsAuthOrigin::from_endpoint(&ep).expect("origin");
        let request_now = now();
        let provider = Provider {
            calls: RefCell::new(0),
            credentials: Some(LfsAuthCredentials::new(
                LfsAuthHeaders::empty(),
                LfsAuthExpiry::at(request_now + LFS_AUTH_EXPIRY_SKEW),
            )),
        };
        let mut cache = LfsAuthCache::new(1).expect("cache");
        assert_eq!(
            cache.get_or_fetch(&ep, &origin, LfsOperation::Fetch, request_now, &provider,),
            Err(LfsAuthError::InvalidExpiry)
        );
        assert_eq!(*provider.calls.borrow(), 1);
        assert_eq!(
            cache.get_or_fetch(&ep, &origin, LfsOperation::Fetch, request_now, &provider,),
            Err(LfsAuthError::InvalidExpiry)
        );
        assert_eq!(*provider.calls.borrow(), 2);
    }

    #[test]
    fn cache_insert_and_invalidate_are_origin_operation_scoped() {
        let ep = endpoint("https://example.test/info/lfs");
        let origin = LfsAuthOrigin::from_endpoint(&ep).expect("origin");
        let credentials = LfsAuthCredentials::new(
            LfsAuthHeaders::from_pairs(vec![("Authorization".to_owned(), b"secret".to_vec())])
                .expect("headers"),
            LfsAuthExpiry::never(),
        );
        let provider = Provider {
            calls: RefCell::new(0),
            credentials: Some(credentials.clone()),
        };
        let mut cache = LfsAuthCache::new(2).expect("cache");
        cache
            .insert(&ep, &origin, LfsOperation::Fetch, now(), credentials)
            .expect("insert");
        cache
            .get_or_fetch(&ep, &origin, LfsOperation::Fetch, now(), &provider)
            .expect("cached fetch");
        assert_eq!(*provider.calls.borrow(), 0);
        assert!(!cache.invalidate(&origin, LfsOperation::Push));
        assert!(cache.invalidate(&origin, LfsOperation::Fetch));
        cache
            .get_or_fetch(&ep, &origin, LfsOperation::Fetch, now(), &provider)
            .expect("refetched");
        assert_eq!(*provider.calls.borrow(), 1);
    }

    #[test]
    fn http_path_scope_is_opt_in_and_isolated_for_same_origin() {
        let first_endpoint = endpoint("https://example.test/team/one/info/lfs");
        let second_endpoint = endpoint("https://example.test/team/two/info/lfs");
        let origin = LfsAuthOrigin::from_endpoint(&first_endpoint).expect("origin");
        let first = LfsAuthOrigin::from_endpoint_with_http_path(&first_endpoint, true)
            .expect("first scoped origin");
        let second = LfsAuthOrigin::from_endpoint_with_http_path(&second_endpoint, true)
            .expect("second scoped origin");
        assert_eq!(
            origin,
            LfsAuthOrigin::from_endpoint(&second_endpoint).expect("origin")
        );
        assert_ne!(first, second);
        assert_eq!(
            first.credential_path(),
            Some(b"team/one/info/lfs".as_slice())
        );
        assert_eq!(
            second.credential_path(),
            Some(b"team/two/info/lfs".as_slice())
        );

        struct ScopedProvider {
            calls: RefCell<Vec<Option<Vec<u8>>>>,
        }

        impl LfsCredentialProvider for ScopedProvider {
            fn credentials(
                &self,
                _endpoint: &LfsHttpUrl,
                origin: &LfsAuthOrigin,
                _operation: LfsOperation,
            ) -> Result<Option<LfsAuthCredentials>, LfsAuthError> {
                self.calls
                    .borrow_mut()
                    .push(origin.credential_path().map(<[u8]>::to_vec));
                Ok(Some(LfsAuthCredentials::new(
                    LfsAuthHeaders::empty(),
                    LfsAuthExpiry::never(),
                )))
            }
        }

        let provider = ScopedProvider {
            calls: RefCell::new(Vec::new()),
        };
        let mut cache = LfsAuthCache::new(2).expect("cache");
        cache
            .get_or_fetch(
                &first_endpoint,
                &first,
                LfsOperation::Fetch,
                now(),
                &provider,
            )
            .expect("first fetch");
        cache
            .get_or_fetch(
                &second_endpoint,
                &second,
                LfsOperation::Fetch,
                now(),
                &provider,
            )
            .expect("second fetch");
        cache
            .get_or_fetch(
                &first_endpoint,
                &first,
                LfsOperation::Fetch,
                now(),
                &provider,
            )
            .expect("first cached fetch");
        assert_eq!(
            *provider.calls.borrow(),
            vec![
                Some(b"team/one/info/lfs".to_vec()),
                Some(b"team/two/info/lfs".to_vec()),
            ]
        );
    }

    #[test]
    fn http_path_scope_decodes_percent_octets_without_utf8_loss() {
        let encoded_endpoint = endpoint("https://example.test/team/%72epo%20one/%FF/info/lfs");
        let equivalent_endpoint = endpoint("https://example.test/team/repo%20one/%ff/info/lfs");
        let encoded = LfsAuthOrigin::from_endpoint_with_http_path(&encoded_endpoint, true)
            .expect("encoded origin");
        let equivalent = LfsAuthOrigin::from_endpoint_with_http_path(&equivalent_endpoint, true)
            .expect("equivalent origin");
        assert_eq!(encoded, equivalent);
        assert_eq!(
            encoded.credential_path(),
            Some(b"team/repo one/\xff/info/lfs".as_slice())
        );

        let provider = Provider {
            calls: RefCell::new(0),
            credentials: Some(LfsAuthCredentials::new(
                LfsAuthHeaders::empty(),
                LfsAuthExpiry::never(),
            )),
        };
        let mut cache = LfsAuthCache::new(1).expect("cache");
        cache
            .get_or_fetch(
                &encoded_endpoint,
                &encoded,
                LfsOperation::Fetch,
                now(),
                &provider,
            )
            .expect("encoded fetch");
        cache
            .get_or_fetch(
                &equivalent_endpoint,
                &equivalent,
                LfsOperation::Fetch,
                now(),
                &provider,
            )
            .expect("equivalent cached fetch");
        assert_eq!(*provider.calls.borrow(), 1);

        assert_eq!(
            parse_http_url("https://example.test/team/%00private/info/lfs"),
            Err(super::super::lfs_endpoint::LfsEndpointError::MalformedUrl)
        );
    }

    #[test]
    fn auth_origin_debug_never_discloses_credential_path() {
        let endpoint = endpoint(
            "https://example.test/private-path/api-token/info/lfs?signature=private-query",
        );
        let origin =
            LfsAuthOrigin::from_endpoint_with_http_path(&endpoint, true).expect("scoped origin");
        let debug = format!("{origin:?}");
        for secret in ["private-path", "api-token", "private-query"] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("has_credential_path: true"));
    }

    #[test]
    fn timeout_nonzero_and_oversized_process_results_are_typed_and_redacted() {
        struct ErrorRunner(LfsAuthError);
        impl LfsSshCommandRunner for ErrorRunner {
            fn run(
                &self,
                _destination: &LfsSshDestination,
                _program: &str,
                _args: &[String],
                _timeout: Duration,
                _stdout_limit: usize,
                _stderr_limit: usize,
            ) -> Result<LfsSshCommandOutput, LfsAuthError> {
                Err(self.0)
            }
        }
        assert_eq!(
            authenticate_ssh(
                &ErrorRunner(LfsAuthError::Timeout),
                &ssh_request("/team/repo.git", LfsOperation::Fetch),
                &endpoint("https://default.example.test/info/lfs"),
                Duration::from_secs(1),
                now()
            ),
            Err(LfsAuthError::Timeout)
        );
        let runner = Runner {
            output: RefCell::new(Some(LfsSshCommandOutput {
                status: 1,
                stdout: b"secret stdout".to_vec(),
                stderr: b"secret stderr".to_vec(),
            })),
            expected_port: None,
        };
        let error = authenticate_ssh(
            &runner,
            &ssh_request("/team/repo.git", LfsOperation::Fetch),
            &endpoint("https://default.example.test/info/lfs"),
            Duration::from_secs(1),
            now(),
        )
        .expect_err("nonzero");
        assert_eq!(error, LfsAuthError::ProcessFailed);
        assert!(!error.to_string().contains("secret"));
        let oversized = Runner {
            output: RefCell::new(Some(LfsSshCommandOutput {
                status: 0,
                stdout: vec![b'x'; LFS_AUTH_MAX_SSH_STDOUT_BYTES + 1],
                stderr: Vec::new(),
            })),
            expected_port: None,
        };
        assert_eq!(
            authenticate_ssh(
                &oversized,
                &ssh_request("/team/repo.git", LfsOperation::Fetch),
                &endpoint("https://default.example.test/info/lfs"),
                Duration::from_secs(1),
                now()
            ),
            Err(LfsAuthError::OutputTooLarge)
        );
        assert!(matches!(
            LfsAuthCache::new(LFS_AUTH_MAX_CACHE_ENTRIES + 1),
            Err(LfsAuthError::CacheLimit)
        ));
    }

    #[test]
    fn cache_headers_drop_and_debug_are_redacted() {
        let headers =
            LfsAuthHeaders::from_pairs(vec![("Authorization".to_owned(), b"secret".to_vec())])
                .expect("headers");
        let credentials = LfsAuthCredentials::new(headers, LfsAuthExpiry::never());
        let text = format!("{credentials:?}");
        assert!(!text.contains("secret"));
    }
}
