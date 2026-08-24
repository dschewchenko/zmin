//! Runtime composition for one Git LFS network session.
//!
//! Endpoint discovery, authentication, and Basic Transfer remain independent
//! primitives.  This module owns their lifecycle: one immutable runtime
//! configuration, per-operation authentication generations, and at most one
//! authenticated Batch/action refetch after a rejected or expired action.
//! Repository reachability and CLI parsing deliberately remain outside this
//! boundary.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::{
    GitCredentialHelper, GitSshCommandRunner, LFS_AUTH_EXPIRY_SKEW, LFS_AUTH_MAX_CACHE_ENTRIES,
    LfsAccessMode, LfsActionCredentialProvider, LfsAuthCache, LfsAuthCredentials, LfsAuthError,
    LfsAuthOrigin, LfsAuthTransferPolicy, LfsBatchRef, LfsCredentialProvider, LfsEndpoint,
    LfsEndpointError, LfsEndpointResolution, LfsHttpUrl, LfsOid, LfsOperation, LfsRemoteConfig,
    LfsRuntimeConfig, LfsSshAuthenticationRequest, LfsSshAuthenticationResult, LfsSshCommandRunner,
    LfsStore, LfsTransferActionAuth, LfsTransferAuthHeaders, LfsTransferClient, LfsTransferConfig,
    LfsTransferError, LfsTransferFailure, LfsTransferHttpScope, LfsTransferObject,
    LfsTransferReport, authenticate_ssh, batch_url, resolve_lfs_endpoint,
    resolve_lfs_endpoint_for_remote,
};

const OPERATION_CACHE_ENTRIES: usize = 1;
const MAX_REMOTE_URL_OVERRIDE_BYTES: usize = 16 * 1024;

/// Clock boundary used for expiry decisions and deterministic tests.
pub(crate) trait LfsSessionClock {
    fn now(&self) -> SystemTime;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SystemLfsSessionClock;

impl LfsSessionClock for SystemLfsSessionClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Credential-provider lifecycle needed by a session.
///
/// Authentication rejection is intentionally explicit.  The production Git
/// adapter rejects by canonical origin without retaining raw user/password
/// material or invoking a shell.
pub(crate) trait LfsSessionCredentialProvider: LfsCredentialProvider {
    fn approve_authentication(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<(), LfsAuthError>;

    fn reject_authentication(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<(), LfsAuthError>;
}

impl LfsSessionCredentialProvider for GitCredentialHelper {
    fn approve_authentication(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<(), LfsAuthError> {
        self.approve_cached(endpoint, origin, operation)
    }

    fn reject_authentication(
        &self,
        endpoint: &LfsHttpUrl,
        origin: &LfsAuthOrigin,
        operation: LfsOperation,
    ) -> Result<(), LfsAuthError> {
        self.reject_cached(endpoint, origin, operation)
    }
}

/// Strict `git-lfs-authenticate` adapter.
///
/// The default HTTP endpoint is discovered and validated with the SSH remote,
/// matching Git LFS's endpoint finder.  `git-lfs-authenticate` may replace it
/// with an explicit `href`, but this adapter never derives a second URL.
pub(crate) struct GitLfsAuthenticateSsh<R> {
    runner: R,
    timeout: Duration,
}

impl<R> fmt::Debug for GitLfsAuthenticateSsh<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitLfsAuthenticateSsh")
            .field("timeout", &self.timeout)
            .field("runner", &"<redacted>")
            .finish()
    }
}

impl<R> GitLfsAuthenticateSsh<R> {
    pub(crate) fn new(runner: R, timeout: Duration) -> Result<Self, LfsAuthError> {
        if timeout.is_zero() {
            return Err(LfsAuthError::Timeout);
        }
        Ok(Self { runner, timeout })
    }
}

pub(crate) trait LfsSessionSshAuthenticator {
    fn authenticate(
        &self,
        request: &LfsSshAuthenticationRequest,
        now: SystemTime,
    ) -> Result<LfsSshAuthenticationResult, LfsSessionSshError>;
}

impl<R: LfsSshCommandRunner> LfsSessionSshAuthenticator for GitLfsAuthenticateSsh<R> {
    fn authenticate(
        &self,
        request: &LfsSshAuthenticationRequest,
        now: SystemTime,
    ) -> Result<LfsSshAuthenticationResult, LfsSessionSshError> {
        authenticate_ssh(
            &self.runner,
            request,
            &request.default_endpoint.url,
            self.timeout,
            now,
        )
        .map_err(LfsSessionSshError::Authentication)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsSessionSshError {
    Authentication(LfsAuthError),
}

impl fmt::Display for LfsSessionSshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authentication(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for LfsSessionSshError {}

/// HTTP/Basic Transfer dependency boundary.
///
/// Production constructs [`LfsTransferClient`]; tests can supply a fake that
/// records the validated endpoint and redacted credential boundary without a
/// TCP listener or environment mutation.
pub(crate) trait LfsNetworkHttpExecutor {
    fn execute(
        &self,
        endpoint: &LfsEndpoint,
        store: Arc<LfsStore>,
        auth_headers: LfsTransferAuthHeaders,
        action_credentials: Arc<dyn LfsActionCredentialProvider>,
        config: &LfsTransferConfig,
        objects: &[LfsTransferObject],
        reference: Option<LfsBatchRef>,
    ) -> LfsNetworkHttpExecution;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BasicLfsNetworkHttpExecutor;

impl LfsNetworkHttpExecutor for BasicLfsNetworkHttpExecutor {
    fn execute(
        &self,
        endpoint: &LfsEndpoint,
        store: Arc<LfsStore>,
        auth_headers: LfsTransferAuthHeaders,
        action_credentials: Arc<dyn LfsActionCredentialProvider>,
        config: &LfsTransferConfig,
        objects: &[LfsTransferObject],
        reference: Option<LfsBatchRef>,
    ) -> LfsNetworkHttpExecution {
        let client = match LfsTransferClient::new(
            endpoint,
            store,
            auth_headers,
            action_credentials,
            config.clone(),
        ) {
            Ok(client) => client,
            Err(error) => return classify_transfer_error(error, objects),
        };
        let result = match endpoint.operation {
            LfsOperation::Fetch => client.download(objects, reference),
            LfsOperation::Push => client.upload(objects, reference),
        };
        match result {
            Ok(report) => classify_transfer_report(report),
            Err(error) => classify_transfer_error(error, objects),
        }
    }
}

pub(crate) enum LfsNetworkHttpExecution {
    Complete(LfsTransferReport),
    Incomplete(LfsTransferReport),
    RefreshAuthentication {
        report: Option<LfsTransferReport>,
        objects: Vec<LfsTransferObject>,
        reason: LfsAuthRefreshReason,
    },
    Failed(LfsTransferError),
}

impl fmt::Debug for LfsNetworkHttpExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complete(report) => formatter.debug_tuple("Complete").field(report).finish(),
            Self::Incomplete(report) => formatter.debug_tuple("Incomplete").field(report).finish(),
            Self::RefreshAuthentication {
                report,
                objects,
                reason,
            } => formatter
                .debug_struct("RefreshAuthentication")
                .field("has_report", &report.is_some())
                .field("object_count", &objects.len())
                .field("reason", reason)
                .finish(),
            Self::Failed(error) => formatter.debug_tuple("Failed").field(error).finish(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfsAuthRefreshReason {
    Unauthorized,
    ExpiredAction,
    RejectedAction,
}

impl LfsAuthRefreshReason {
    fn is_action_refetch(self) -> bool {
        matches!(self, Self::ExpiredAction | Self::RejectedAction)
    }
}

fn classify_transfer_error(
    error: LfsTransferError,
    objects: &[LfsTransferObject],
) -> LfsNetworkHttpExecution {
    if matches!(
        &error,
        LfsTransferError::BatchHttp(response) if matches!(response.status(), 401 | 403)
    ) {
        return LfsNetworkHttpExecution::RefreshAuthentication {
            report: None,
            objects: objects.to_vec(),
            reason: LfsAuthRefreshReason::Unauthorized,
        };
    }
    LfsNetworkHttpExecution::Failed(error)
}

fn classify_transfer_report(report: LfsTransferReport) -> LfsNetworkHttpExecution {
    if !report.has_exact_accounting() {
        return LfsNetworkHttpExecution::Failed(LfsTransferError::IncompleteReport);
    }
    let mut reason = None;
    for (_, failure) in report.failed() {
        let candidate = match failure {
            LfsTransferFailure::ObjectError { code: 401 | 403 } => {
                Some(LfsAuthRefreshReason::Unauthorized)
            }
            LfsTransferFailure::HttpStatus {
                status: 401 | 403,
                auth:
                    LfsTransferActionAuth::Preauthenticated
                    | LfsTransferActionAuth::ActionHeader
                    | LfsTransferActionAuth::ConfiguredHeader
                    | LfsTransferActionAuth::CredentialHelper
                    | LfsTransferActionAuth::None,
                ..
            } => Some(LfsAuthRefreshReason::RejectedAction),
            LfsTransferFailure::ActionExpired(_) => Some(LfsAuthRefreshReason::ExpiredAction),
            _ => None,
        };
        if let Some(candidate) = candidate {
            if reason != Some(LfsAuthRefreshReason::Unauthorized) {
                reason = Some(candidate);
            }
        }
    }
    match reason {
        Some(reason) => {
            let objects = report.refresh_candidates();
            LfsNetworkHttpExecution::RefreshAuthentication {
                report: Some(report),
                objects,
                reason,
            }
        }
        None if report.is_complete() => LfsNetworkHttpExecution::Complete(report),
        None => LfsNetworkHttpExecution::Incomplete(report),
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum LfsNetworkRemote {
    Configured,
    Named(String),
    UrlOverride(LfsRemoteUrlContext),
    AnonymousUrl(LfsRemoteUrlOverride),
}

impl fmt::Debug for LfsNetworkRemote {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configured => formatter.write_str("Configured"),
            Self::Named(_) => formatter.write_str("Named(<redacted>)"),
            Self::UrlOverride(_) => formatter.write_str("UrlOverride(<redacted>)"),
            Self::AnonymousUrl(_) => formatter.write_str("AnonymousUrl(<redacted>)"),
        }
    }
}

/// Caller-supplied Git transport URL from the pre-push protocol.
///
/// Full transport validation and endpoint derivation are performed by the
/// shared endpoint resolver with the active `lfs.gitprotocol` policy.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsRemoteUrlOverride(String);

impl fmt::Debug for LfsRemoteUrlOverride {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LfsRemoteUrlOverride(<redacted>)")
    }
}

impl LfsRemoteUrlOverride {
    pub(crate) fn new(url: impl Into<String>) -> Result<Self, LfsEndpointError> {
        let url = url.into();
        if url.is_empty()
            || url.len() > MAX_REMOTE_URL_OVERRIDE_BYTES
            || url.contains('\\')
            || url.chars().any(char::is_control)
            || url.chars().any(char::is_whitespace)
        {
            return Err(LfsEndpointError::MalformedUrl);
        }
        Ok(Self(url))
    }
}

/// Named Git remote plus the transport URL supplied by Git's pre-push hook.
///
/// The URL replaces only that remote's Git transport URL. Configured global
/// and remote-specific LFS endpoints retain their normal precedence.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsRemoteUrlContext {
    remote: String,
    url: LfsRemoteUrlOverride,
}

impl fmt::Debug for LfsRemoteUrlContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsRemoteUrlContext")
            .field("remote", &"<redacted>")
            .field("url", &"<redacted>")
            .finish()
    }
}

impl Default for LfsNetworkRemote {
    fn default() -> Self {
        Self::Configured
    }
}

/// Object metadata and optional Batch context for one session call.
#[derive(Clone, Default)]
pub(crate) struct LfsNetworkRequest {
    objects: Vec<LfsTransferObject>,
    reference: Option<LfsBatchRef>,
    remote: LfsNetworkRemote,
}

impl fmt::Debug for LfsNetworkRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsNetworkRequest")
            .field("object_count", &self.objects.len())
            .field("has_reference", &self.reference.is_some())
            .field("remote", &self.remote)
            .finish()
    }
}

impl LfsNetworkRequest {
    pub(crate) fn new(objects: Vec<LfsTransferObject>) -> Self {
        Self {
            objects,
            ..Self::default()
        }
    }

    pub(crate) fn with_reference(mut self, reference: LfsBatchRef) -> Self {
        self.reference = Some(reference);
        self
    }

    pub(crate) fn with_remote(mut self, remote: impl Into<String>) -> Self {
        self.remote = LfsNetworkRemote::Named(remote.into());
        self
    }

    pub(crate) fn with_remote_url(
        mut self,
        remote: impl Into<String>,
        remote_url: LfsRemoteUrlOverride,
    ) -> Self {
        self.remote = LfsNetworkRemote::UrlOverride(LfsRemoteUrlContext {
            remote: remote.into(),
            url: remote_url,
        });
        self
    }

    pub(crate) fn with_anonymous_remote_url(mut self, remote_url: LfsRemoteUrlOverride) -> Self {
        self.remote = LfsNetworkRemote::AnonymousUrl(remote_url);
        self
    }

    pub(crate) fn objects(&self) -> &[LfsTransferObject] {
        &self.objects
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LfsRemoteSelectionRequirement {
    operation: LfsOperation,
    autodetect: bool,
    search_all: bool,
}

impl LfsRemoteSelectionRequirement {
    pub(crate) fn operation(self) -> LfsOperation {
        self.operation
    }

    pub(crate) fn autodetect(self) -> bool {
        self.autodetect
    }

    pub(crate) fn search_all(self) -> bool {
        self.search_all
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfsNetworkDisposition {
    Complete,
    FailureRequired,
    SkipDownloadErrors,
    AllowIncompletePush,
}

pub(crate) struct LfsNetworkTransferOutcome {
    reports: Vec<LfsTransferReport>,
    disposition: LfsNetworkDisposition,
    authentication_refreshes: u8,
    action_refetches: u8,
    unresolved_failures: usize,
}

impl fmt::Debug for LfsNetworkTransferOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsNetworkTransferOutcome")
            .field("report_count", &self.reports.len())
            .field("disposition", &self.disposition)
            .field("authentication_refreshes", &self.authentication_refreshes)
            .field("action_refetches", &self.action_refetches)
            .field("unresolved_failures", &self.unresolved_failures)
            .finish()
    }
}

impl LfsNetworkTransferOutcome {
    pub(crate) fn reports(&self) -> &[LfsTransferReport] {
        &self.reports
    }

    pub(crate) fn disposition(&self) -> LfsNetworkDisposition {
        self.disposition
    }

    pub(crate) fn authentication_refreshes(&self) -> u8 {
        self.authentication_refreshes
    }

    pub(crate) fn action_refetches(&self) -> u8 {
        self.action_refetches
    }

    pub(crate) fn unresolved_failures(&self) -> usize {
        self.unresolved_failures
    }
}

#[derive(Debug)]
pub(crate) enum LfsNetworkOutcome {
    RemoteSelectionRequired(LfsRemoteSelectionRequirement),
    Transfer(LfsNetworkTransferOutcome),
    DownloadFailureSkipped(LfsSkippedDownloadFailure),
}

pub(crate) enum LfsSkippedDownloadFailure {
    Transfer(LfsTransferError),
    AuthenticationRefreshExhausted(LfsAuthRefreshReason),
}

impl fmt::Debug for LfsSkippedDownloadFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transfer(_) => formatter.write_str("Transfer(<redacted>)"),
            Self::AuthenticationRefreshExhausted(reason) => formatter
                .debug_tuple("AuthenticationRefreshExhausted")
                .field(reason)
                .finish(),
        }
    }
}

pub(crate) enum LfsNetworkSessionError {
    Endpoint(LfsEndpointError),
    Authentication(LfsAuthError),
    SshAuthentication(LfsSessionSshError),
    Transfer(LfsTransferError),
    AuthenticationRetryExhausted(LfsAuthRefreshReason),
    LocksVerificationRequired,
}

impl fmt::Debug for LfsNetworkSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Endpoint(error) => formatter.debug_tuple("Endpoint").field(error).finish(),
            Self::Authentication(error) => formatter
                .debug_tuple("Authentication")
                .field(error)
                .finish(),
            Self::SshAuthentication(error) => formatter
                .debug_tuple("SshAuthentication")
                .field(error)
                .finish(),
            Self::Transfer(error) => formatter.debug_tuple("Transfer").field(error).finish(),
            Self::AuthenticationRetryExhausted(reason) => formatter
                .debug_tuple("AuthenticationRetryExhausted")
                .field(reason)
                .finish(),
            Self::LocksVerificationRequired => formatter.write_str("LocksVerificationRequired"),
        }
    }
}

impl fmt::Display for LfsNetworkSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Endpoint(error) => error.fmt(formatter),
            Self::Authentication(error) => error.fmt(formatter),
            Self::SshAuthentication(error) => error.fmt(formatter),
            Self::Transfer(error) => error.fmt(formatter),
            Self::AuthenticationRetryExhausted(reason) => {
                write!(
                    formatter,
                    "LFS {reason:?} authentication refresh was exhausted"
                )
            }
            Self::LocksVerificationRequired => {
                formatter.write_str("LFS lock verification requires the unsupported locking API")
            }
        }
    }
}

impl std::error::Error for LfsNetworkSessionError {}

struct LfsOperationAuthCache {
    http: LfsAuthCache,
    ssh: Option<LfsSshCachedAuthentication>,
}

impl LfsOperationAuthCache {
    fn new() -> Result<Self, LfsAuthError> {
        Ok(Self {
            http: LfsAuthCache::new(OPERATION_CACHE_ENTRIES)?,
            ssh: None,
        })
    }
}

struct LfsSessionAuthCaches {
    fetch: LfsOperationAuthCache,
    push: LfsOperationAuthCache,
}

struct LfsSessionActionCredentials<C, K> {
    config: LfsRuntimeConfig,
    provider: C,
    clock: K,
    cache: Mutex<LfsAuthCache>,
}

impl<C, K> LfsSessionActionCredentials<C, K> {
    fn new(config: LfsRuntimeConfig, provider: C, clock: K) -> Result<Self, LfsAuthError> {
        Ok(Self {
            config,
            provider,
            clock,
            cache: Mutex::new(LfsAuthCache::new(LFS_AUTH_MAX_CACHE_ENTRIES)?),
        })
    }

    fn origin_for(&self, action: &LfsHttpUrl) -> Result<LfsAuthOrigin, LfsAuthError> {
        let use_http_path = self
            .config
            .credential_use_http_path_for(action)
            .map_err(|_| LfsAuthError::InvalidOrigin)?;
        LfsAuthOrigin::from_endpoint_with_http_path(action, use_http_path)
    }
}

impl<C, K> LfsActionCredentialProvider for LfsSessionActionCredentials<C, K>
where
    C: LfsSessionCredentialProvider + Send + Sync,
    K: LfsSessionClock + Send + Sync,
{
    fn credentials(
        &self,
        action: &LfsHttpUrl,
        operation: LfsOperation,
    ) -> Result<Option<LfsTransferAuthHeaders>, LfsAuthError> {
        if self.config.access_for(action) != LfsAccessMode::Basic {
            return Ok(None);
        }
        let origin = self.origin_for(action)?;
        let credentials = self
            .cache
            .lock()
            .map_err(|_| LfsAuthError::ProcessFailed)?
            .get_or_fetch(action, &origin, operation, self.clock.now(), &self.provider)?;
        credentials
            .as_ref()
            .map(LfsAuthTransferPolicy::to_transfer)
            .transpose()
    }

    fn approve(&self, action: &LfsHttpUrl, operation: LfsOperation) -> Result<(), LfsAuthError> {
        let origin = self.origin_for(action)?;
        self.provider
            .approve_authentication(action, &origin, operation)
    }

    fn reject(&self, action: &LfsHttpUrl, operation: LfsOperation) -> Result<(), LfsAuthError> {
        let origin = self.origin_for(action)?;
        self.provider
            .reject_authentication(action, &origin, operation)?;
        self.cache
            .lock()
            .map_err(|_| LfsAuthError::ProcessFailed)?
            .invalidate(&origin, operation);
        Ok(())
    }
}

impl LfsSessionAuthCaches {
    fn new() -> Result<Self, LfsAuthError> {
        Ok(Self {
            fetch: LfsOperationAuthCache::new()?,
            push: LfsOperationAuthCache::new()?,
        })
    }

    fn operation_mut(&mut self, operation: LfsOperation) -> &mut LfsOperationAuthCache {
        match operation {
            LfsOperation::Fetch => &mut self.fetch,
            LfsOperation::Push => &mut self.push,
        }
    }
}

#[derive(Clone)]
struct LfsSshCachedAuthentication {
    request: LfsSshAuthenticationRequest,
    result: LfsSshAuthenticationResult,
}

#[derive(Clone)]
enum LfsAuthenticationKind {
    Http {
        origin: LfsAuthOrigin,
        access_mode: LfsAccessMode,
        credentials_attempted: bool,
    },
    Ssh,
}

struct LfsAuthenticatedEndpoint {
    endpoint: LfsEndpoint,
    headers: LfsTransferAuthHeaders,
    kind: LfsAuthenticationKind,
}

/// One immutable configuration and bounded authentication lifecycle.
pub(crate) struct LfsNetworkSession<C, S, H, K> {
    config: LfsRuntimeConfig,
    store: Arc<LfsStore>,
    credential_provider: C,
    ssh_authenticator: S,
    http_executor: H,
    clock: K,
    transfer_config: LfsTransferConfig,
    auth_caches: LfsSessionAuthCaches,
}

impl<C, S, H, K> fmt::Debug for LfsNetworkSession<C, S, H, K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsNetworkSession")
            .field("config", &self.config)
            .field("store", &"<redacted>")
            .field("credential_provider", &"<redacted>")
            .field("ssh_authenticator", &"<redacted>")
            .field("http_executor", &"<redacted>")
            .field("transfer_config", &self.transfer_config)
            .finish()
    }
}

impl<C, S, H, K> LfsNetworkSession<C, S, H, K> {
    pub(crate) fn new(
        config: LfsRuntimeConfig,
        store: Arc<LfsStore>,
        credential_provider: C,
        ssh_authenticator: S,
        http_executor: H,
        clock: K,
        transfer_config: LfsTransferConfig,
    ) -> Result<Self, LfsNetworkSessionError> {
        Ok(Self {
            config,
            store,
            credential_provider,
            ssh_authenticator,
            http_executor,
            clock,
            transfer_config,
            auth_caches: LfsSessionAuthCaches::new()
                .map_err(LfsNetworkSessionError::Authentication)?,
        })
    }
}

pub(crate) type SystemLfsNetworkSession = LfsNetworkSession<
    GitCredentialHelper,
    GitLfsAuthenticateSsh<GitSshCommandRunner>,
    BasicLfsNetworkHttpExecutor,
    SystemLfsSessionClock,
>;

impl SystemLfsNetworkSession {
    pub(crate) fn system(
        config: LfsRuntimeConfig,
        store: Arc<LfsStore>,
        transfer_config: LfsTransferConfig,
        process_timeout: Duration,
    ) -> Result<Self, LfsNetworkSessionError> {
        let credential_provider = GitCredentialHelper::system(process_timeout)
            .map_err(LfsNetworkSessionError::Authentication)?;
        let runner =
            GitSshCommandRunner::system().map_err(LfsNetworkSessionError::Authentication)?;
        let ssh_authenticator = GitLfsAuthenticateSsh::new(runner, process_timeout)
            .map_err(LfsNetworkSessionError::Authentication)?;
        Self::new(
            config,
            store,
            credential_provider,
            ssh_authenticator,
            BasicLfsNetworkHttpExecutor,
            SystemLfsSessionClock,
            transfer_config,
        )
    }
}

impl<C, S, H, K> LfsNetworkSession<C, S, H, K>
where
    C: LfsSessionCredentialProvider + Clone + Send + Sync + 'static,
    S: LfsSessionSshAuthenticator,
    H: LfsNetworkHttpExecutor,
    K: LfsSessionClock + Clone + Send + Sync + 'static,
{
    /// Download objects already identified as absent by the caller.
    ///
    /// The store intentionally has no metadata-only presence oracle.  This
    /// method therefore does not add a full `verify` pass before the network
    /// request: response bytes stream once into `LfsStore::ingest`, and the
    /// filter layer reopens the stored object for verified output afterwards.
    pub(crate) fn download_missing(
        &mut self,
        request: &LfsNetworkRequest,
    ) -> Result<LfsNetworkOutcome, LfsNetworkSessionError> {
        self.execute(LfsOperation::Fetch, request)
    }

    pub(crate) fn upload(
        &mut self,
        request: &LfsNetworkRequest,
    ) -> Result<LfsNetworkOutcome, LfsNetworkSessionError> {
        self.execute(LfsOperation::Push, request)
    }

    fn execute(
        &mut self,
        operation: LfsOperation,
        request: &LfsNetworkRequest,
    ) -> Result<LfsNetworkOutcome, LfsNetworkSessionError> {
        if let Some(requirement) = self.remote_selection_requirement(operation, &request.remote) {
            return Ok(LfsNetworkOutcome::RemoteSelectionRequired(requirement));
        }

        let resolution = self.resolve_endpoint(operation, &request.remote)?;
        let authenticated = self.authenticate_resolution(resolution, operation, false)?;
        self.enforce_locks_policy(operation, &authenticated.endpoint)?;
        let action_credentials = self.action_credentials()?;
        let first = self.http_executor.execute(
            &authenticated.endpoint,
            Arc::clone(&self.store),
            authenticated.headers.clone(),
            Arc::clone(&action_credentials),
            &self.transfer_config,
            &request.objects,
            request.reference.clone(),
        );
        match first {
            LfsNetworkHttpExecution::Complete(report) => {
                if !report.is_complete() || !report.accounts_for(&request.objects) {
                    return self
                        .transfer_error_outcome(operation, LfsTransferError::IncompleteReport);
                }
                self.approve_authenticated(&authenticated, operation)?;
                Ok(LfsNetworkOutcome::Transfer(self.finish_outcome(
                    operation,
                    vec![report],
                    0,
                    0,
                    0,
                    true,
                )))
            }
            LfsNetworkHttpExecution::Incomplete(report) => {
                if !report.accounts_for(&request.objects) {
                    return self
                        .transfer_error_outcome(operation, LfsTransferError::IncompleteReport);
                }
                Ok(LfsNetworkOutcome::Transfer(self.finish_outcome(
                    operation,
                    vec![report],
                    0,
                    0,
                    0,
                    false,
                )))
            }
            LfsNetworkHttpExecution::Failed(error) => self.transfer_error_outcome(operation, error),
            LfsNetworkHttpExecution::RefreshAuthentication {
                report,
                objects,
                reason,
            } => {
                if !refresh_execution_accounts_for(report.as_ref(), &objects, &request.objects) {
                    return self
                        .transfer_error_outcome(operation, LfsTransferError::IncompleteReport);
                }
                self.refresh_once(
                    operation,
                    &request.remote,
                    request.reference.clone(),
                    authenticated,
                    report,
                    objects,
                    reason,
                    action_credentials,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn refresh_once(
        &mut self,
        operation: LfsOperation,
        remote: &LfsNetworkRemote,
        reference: Option<LfsBatchRef>,
        authenticated: LfsAuthenticatedEndpoint,
        initial_report: Option<LfsTransferReport>,
        objects: Vec<LfsTransferObject>,
        reason: LfsAuthRefreshReason,
        action_credentials: Arc<dyn LfsActionCredentialProvider>,
    ) -> Result<LfsNetworkOutcome, LfsNetworkSessionError> {
        if reason.is_action_refetch() {
            self.approve_authenticated(&authenticated, operation)?;
        }
        if reason == LfsAuthRefreshReason::Unauthorized
            && let LfsAuthenticationKind::Http {
                origin,
                access_mode,
                credentials_attempted,
            } = &authenticated.kind
        {
            if *access_mode == LfsAccessMode::None {
                return if operation == LfsOperation::Fetch && self.config.skip_download_errors() {
                    Ok(LfsNetworkOutcome::DownloadFailureSkipped(
                        LfsSkippedDownloadFailure::AuthenticationRefreshExhausted(reason),
                    ))
                } else {
                    Err(LfsNetworkSessionError::AuthenticationRetryExhausted(reason))
                };
            }
            if *credentials_attempted {
                self.credential_provider
                    .reject_authentication(&authenticated.endpoint.url, origin, operation)
                    .map_err(LfsNetworkSessionError::Authentication)?;
            }
        }
        if reason == LfsAuthRefreshReason::Unauthorized {
            let cache = self.auth_caches.operation_mut(operation);
            match &authenticated.kind {
                LfsAuthenticationKind::Http { origin, .. } => {
                    cache.http.invalidate(origin, operation);
                }
                LfsAuthenticationKind::Ssh => cache.ssh = None,
            }
        }
        let resolution = self.resolve_endpoint(operation, remote)?;
        let refreshed = self.authenticate_resolution(
            resolution,
            operation,
            reason == LfsAuthRefreshReason::Unauthorized,
        )?;
        self.enforce_locks_policy(operation, &refreshed.endpoint)?;
        let retry = self.http_executor.execute(
            &refreshed.endpoint,
            Arc::clone(&self.store),
            refreshed.headers.clone(),
            action_credentials,
            &self.transfer_config,
            &objects,
            reference,
        );
        match retry {
            LfsNetworkHttpExecution::Complete(report) => {
                if !report.is_complete() || !report.accounts_for(&objects) {
                    return self
                        .transfer_error_outcome(operation, LfsTransferError::IncompleteReport);
                }
                self.approve_authenticated(&refreshed, operation)?;
                let unresolved_before = initial_report
                    .as_ref()
                    .map(|report| count_non_refresh_failures(report))
                    .unwrap_or(0);
                let initial_complete = initial_report
                    .as_ref()
                    .is_none_or(LfsTransferReport::has_exact_accounting)
                    && unresolved_before == 0;
                let mut reports = Vec::with_capacity(2);
                if let Some(initial_report) = initial_report {
                    reports.push(initial_report);
                }
                reports.push(report);
                Ok(LfsNetworkOutcome::Transfer(self.finish_outcome(
                    operation,
                    reports,
                    u8::from(reason == LfsAuthRefreshReason::Unauthorized),
                    u8::from(reason.is_action_refetch()),
                    unresolved_before,
                    initial_complete,
                )))
            }
            LfsNetworkHttpExecution::Incomplete(report) => {
                if !report.accounts_for(&objects) {
                    return self
                        .transfer_error_outcome(operation, LfsTransferError::IncompleteReport);
                }
                let unresolved_before = initial_report
                    .as_ref()
                    .map(count_non_refresh_failures)
                    .unwrap_or(0);
                let mut reports = Vec::with_capacity(2);
                if let Some(initial_report) = initial_report {
                    reports.push(initial_report);
                }
                reports.push(report);
                Ok(LfsNetworkOutcome::Transfer(self.finish_outcome(
                    operation,
                    reports,
                    u8::from(reason == LfsAuthRefreshReason::Unauthorized),
                    u8::from(reason.is_action_refetch()),
                    unresolved_before,
                    false,
                )))
            }
            LfsNetworkHttpExecution::Failed(error) => self.transfer_error_outcome(operation, error),
            LfsNetworkHttpExecution::RefreshAuthentication {
                report,
                objects: retry_objects,
                reason,
            } => {
                if !refresh_execution_accounts_for(report.as_ref(), &retry_objects, &objects) {
                    return self
                        .transfer_error_outcome(operation, LfsTransferError::IncompleteReport);
                }
                if reason == LfsAuthRefreshReason::Unauthorized
                    && let LfsAuthenticationKind::Http {
                        origin,
                        credentials_attempted: true,
                        ..
                    } = &refreshed.kind
                {
                    self.credential_provider
                        .reject_authentication(&refreshed.endpoint.url, origin, operation)
                        .map_err(LfsNetworkSessionError::Authentication)?;
                }
                if operation == LfsOperation::Fetch && self.config.skip_download_errors() {
                    Ok(LfsNetworkOutcome::DownloadFailureSkipped(
                        LfsSkippedDownloadFailure::AuthenticationRefreshExhausted(reason),
                    ))
                } else {
                    Err(LfsNetworkSessionError::AuthenticationRetryExhausted(reason))
                }
            }
        }
    }

    fn transfer_error_outcome(
        &self,
        operation: LfsOperation,
        error: LfsTransferError,
    ) -> Result<LfsNetworkOutcome, LfsNetworkSessionError> {
        if operation == LfsOperation::Fetch && self.config.skip_download_errors() {
            Ok(LfsNetworkOutcome::DownloadFailureSkipped(
                LfsSkippedDownloadFailure::Transfer(error),
            ))
        } else {
            Err(LfsNetworkSessionError::Transfer(error))
        }
    }

    fn approve_authenticated(
        &self,
        authenticated: &LfsAuthenticatedEndpoint,
        operation: LfsOperation,
    ) -> Result<(), LfsNetworkSessionError> {
        if let LfsAuthenticationKind::Http {
            origin,
            credentials_attempted: true,
            ..
        } = &authenticated.kind
        {
            self.credential_provider
                .approve_authentication(&authenticated.endpoint.url, origin, operation)
                .map_err(LfsNetworkSessionError::Authentication)?;
        }
        Ok(())
    }

    fn action_credentials(
        &self,
    ) -> Result<Arc<dyn LfsActionCredentialProvider>, LfsNetworkSessionError> {
        Ok(Arc::new(
            LfsSessionActionCredentials::new(
                self.config.clone(),
                self.credential_provider.clone(),
                self.clock.clone(),
            )
            .map_err(LfsNetworkSessionError::Authentication)?,
        ))
    }

    fn finish_outcome(
        &self,
        operation: LfsOperation,
        reports: Vec<LfsTransferReport>,
        authentication_refreshes: u8,
        action_refetches: u8,
        unresolved_before: usize,
        preceding_attempt_complete: bool,
    ) -> LfsNetworkTransferOutcome {
        let final_report = reports.last().expect("a completed execution has a report");
        let unresolved_failures = unresolved_before.saturating_add(final_report.unresolved_count());
        let complete = preceding_attempt_complete
            && final_report.is_complete()
            && final_report.has_exact_accounting()
            && unresolved_failures == 0;
        let disposition = if complete {
            LfsNetworkDisposition::Complete
        } else {
            match operation {
                LfsOperation::Fetch if self.config.skip_download_errors() => {
                    LfsNetworkDisposition::SkipDownloadErrors
                }
                LfsOperation::Push if self.config.allow_incomplete_push() => {
                    LfsNetworkDisposition::AllowIncompletePush
                }
                _ => LfsNetworkDisposition::FailureRequired,
            }
        };
        LfsNetworkTransferOutcome {
            reports,
            disposition,
            authentication_refreshes,
            action_refetches,
            unresolved_failures,
        }
    }

    fn remote_selection_requirement(
        &self,
        operation: LfsOperation,
        remote: &LfsNetworkRemote,
    ) -> Option<LfsRemoteSelectionRequirement> {
        if !matches!(remote, LfsNetworkRemote::Configured)
            || self.config.endpoint_inputs().requested_remote.is_some()
            || self.has_global_endpoint(operation)
        {
            return None;
        }
        let policy = self.config.remote_policy();
        (policy.autodetect() || policy.search_all()).then_some(LfsRemoteSelectionRequirement {
            operation,
            autodetect: policy.autodetect(),
            search_all: policy.search_all(),
        })
    }

    fn has_global_endpoint(&self, operation: LfsOperation) -> bool {
        let inputs = self.config.endpoint_inputs();
        inputs.lfs_url.is_some()
            || inputs.lfsconfig.lfs_url().is_some()
            || (operation == LfsOperation::Push
                && (inputs.lfs_push_url.is_some() || inputs.lfsconfig.lfs_push_url().is_some()))
    }

    fn resolve_endpoint(
        &self,
        operation: LfsOperation,
        remote: &LfsNetworkRemote,
    ) -> Result<LfsEndpointResolution, LfsNetworkSessionError> {
        let result = match remote {
            LfsNetworkRemote::Configured => {
                resolve_lfs_endpoint(operation, self.config.endpoint_inputs())
            }
            LfsNetworkRemote::Named(remote) => {
                resolve_lfs_endpoint_for_remote(operation, remote, self.config.endpoint_inputs())
            }
            LfsNetworkRemote::UrlOverride(context) => {
                let mut inputs = self.config.endpoint_inputs().clone();
                let remote = inputs
                    .remotes
                    .iter_mut()
                    .find(|remote| remote.name == context.remote)
                    .ok_or_else(|| {
                        LfsNetworkSessionError::Endpoint(LfsEndpointError::MissingRemote {
                            operation,
                            remote: "configured".to_owned(),
                        })
                    })?;
                remote.url = Some(context.url.0.clone());
                remote.push_url = Some(context.url.0.clone());
                resolve_lfs_endpoint_for_remote(operation, &context.remote, &inputs)
            }
            LfsNetworkRemote::AnonymousUrl(url) => {
                const ANONYMOUS_REMOTE: &str = "zmin-anonymous-lfs";
                let mut inputs = self.config.endpoint_inputs().clone();
                inputs
                    .remotes
                    .retain(|remote| remote.name != ANONYMOUS_REMOTE);
                let mut remote = LfsRemoteConfig::named(ANONYMOUS_REMOTE);
                remote.url = Some(url.0.clone());
                remote.push_url = Some(url.0.clone());
                inputs.remotes.push(remote);
                resolve_lfs_endpoint_for_remote(operation, ANONYMOUS_REMOTE, &inputs)
            }
        };
        result.map_err(LfsNetworkSessionError::Endpoint)
    }

    fn enforce_locks_policy(
        &self,
        operation: LfsOperation,
        endpoint: &LfsEndpoint,
    ) -> Result<(), LfsNetworkSessionError> {
        if operation == LfsOperation::Push
            && self.config.locks_verify_for(&endpoint.url) == Some(true)
        {
            return Err(LfsNetworkSessionError::LocksVerificationRequired);
        }
        Ok(())
    }

    fn authenticate_resolution(
        &mut self,
        resolution: LfsEndpointResolution,
        operation: LfsOperation,
        force_credentials: bool,
    ) -> Result<LfsAuthenticatedEndpoint, LfsNetworkSessionError> {
        let now = self.clock.now();
        match resolution {
            LfsEndpointResolution::Http(endpoint) => {
                let batch_request_url =
                    batch_url(&endpoint.url).map_err(LfsNetworkSessionError::Transfer)?;
                let configured_authorization = self
                    .config
                    .http_policy()
                    .has_configured_authorization(&batch_request_url);
                let use_http_path = self
                    .config
                    .credential_use_http_path_for(&endpoint.url)
                    .map_err(LfsNetworkSessionError::Endpoint)?;
                let origin =
                    LfsAuthOrigin::from_endpoint_with_http_path(&endpoint.url, use_http_path)
                        .map_err(LfsNetworkSessionError::Authentication)?;
                let access_mode = self.config.access_for(&endpoint.url);
                let credentials_attempted = !configured_authorization
                    && (access_mode == LfsAccessMode::Basic
                        || (force_credentials && access_mode == LfsAccessMode::Unspecified));
                let credentials = if credentials_attempted {
                    self.auth_caches
                        .operation_mut(operation)
                        .http
                        .get_or_fetch(
                            &endpoint.url,
                            &origin,
                            operation,
                            now,
                            &self.credential_provider,
                        )
                        .map_err(LfsNetworkSessionError::Authentication)?
                } else {
                    None
                };
                let headers = credentials
                    .as_ref()
                    .map(LfsAuthTransferPolicy::to_transfer)
                    .transpose()
                    .map_err(LfsNetworkSessionError::Authentication)?
                    .unwrap_or_else(LfsTransferAuthHeaders::empty);
                Ok(LfsAuthenticatedEndpoint {
                    endpoint,
                    headers,
                    kind: LfsAuthenticationKind::Http {
                        origin,
                        access_mode,
                        credentials_attempted,
                    },
                })
            }
            LfsEndpointResolution::SshAuthenticationRequired(request) => {
                let default_endpoint = request.default_endpoint.clone();
                let cached = self
                    .auth_caches
                    .operation_mut(operation)
                    .ssh
                    .as_ref()
                    .filter(|cached| {
                        cached.request == request
                            && credentials_are_usable(cached.result.credentials(), now)
                    })
                    .cloned();
                let result = match cached {
                    Some(cached) => cached.result,
                    None => {
                        let result = self
                            .ssh_authenticator
                            .authenticate(&request, now)
                            .map_err(LfsNetworkSessionError::SshAuthentication)?;
                        self.auth_caches.operation_mut(operation).ssh =
                            Some(LfsSshCachedAuthentication {
                                request,
                                result: result.clone(),
                            });
                        result
                    }
                };
                let headers = LfsAuthTransferPolicy::to_transfer(result.credentials())
                    .map_err(LfsNetworkSessionError::Authentication)?;
                Ok(LfsAuthenticatedEndpoint {
                    endpoint: LfsEndpoint {
                        operation,
                        source: default_endpoint.source,
                        url: result.href().clone(),
                    },
                    headers,
                    kind: LfsAuthenticationKind::Ssh,
                })
            }
        }
    }
}

fn credentials_are_usable(credentials: &LfsAuthCredentials, now: SystemTime) -> bool {
    credentials.expiry().expires_at().is_none_or(|at| {
        at.duration_since(now)
            .is_ok_and(|remaining| remaining > LFS_AUTH_EXPIRY_SKEW)
    })
}

fn refresh_execution_accounts_for(
    report: Option<&LfsTransferReport>,
    retry_objects: &[LfsTransferObject],
    requested_objects: &[LfsTransferObject],
) -> bool {
    match report {
        Some(report) => {
            report.accounts_for(requested_objects)
                && same_transfer_objects(&report.refresh_candidates(), retry_objects)
        }
        None => same_transfer_objects(requested_objects, retry_objects),
    }
}

fn same_transfer_objects(left: &[LfsTransferObject], right: &[LfsTransferObject]) -> bool {
    fn indexed(objects: &[LfsTransferObject]) -> Option<HashMap<LfsOid, u64>> {
        let mut indexed = HashMap::with_capacity(objects.len());
        for object in objects {
            if let Some(size) = indexed.insert(object.oid(), object.size())
                && size != object.size()
            {
                return None;
            }
        }
        Some(indexed)
    }

    matches!((indexed(left), indexed(right)), (Some(left), Some(right)) if left == right)
}

fn count_non_refresh_failures(report: &LfsTransferReport) -> usize {
    report
        .failed()
        .iter()
        .filter(|(_, failure)| {
            !matches!(
                failure,
                LfsTransferFailure::HttpStatus {
                    status: 401 | 403,
                    scope: LfsTransferHttpScope::SameOrigin | LfsTransferHttpScope::CrossOrigin,
                    ..
                } | LfsTransferFailure::ObjectError { code: 401 | 403 }
                    | LfsTransferFailure::ActionExpired(_)
                    | LfsTransferFailure::Cancelled
            )
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::super::{
        ConfigEntry, ConfigScope, LfsAuthExpiry, LfsAuthHeaders, LfsBatchActionKind,
        LfsConfigSources, LfsHttpEnvironmentSnapshot, LfsOid, LfsRuntimeConfigInput,
        LfsSshCommandOutput, LfsSshDestination, parse_config_name, parse_http_url,
    };
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone, Copy)]
    struct FixedClock(SystemTime);

    impl LfsSessionClock for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    #[derive(Default)]
    struct CredentialState {
        fetch_calls: AtomicUsize,
        push_calls: AtomicUsize,
        approves: AtomicUsize,
        rejects: AtomicUsize,
        paths: Mutex<Vec<Option<Vec<u8>>>>,
    }

    #[derive(Clone)]
    struct FakeCredentials(Arc<CredentialState>);

    impl LfsCredentialProvider for FakeCredentials {
        fn credentials(
            &self,
            _endpoint: &LfsHttpUrl,
            origin: &LfsAuthOrigin,
            operation: LfsOperation,
        ) -> Result<Option<LfsAuthCredentials>, LfsAuthError> {
            let calls = match operation {
                LfsOperation::Fetch => &self.0.fetch_calls,
                LfsOperation::Push => &self.0.push_calls,
            };
            calls.fetch_add(1, Ordering::Relaxed);
            self.0
                .paths
                .lock()
                .expect("credential paths")
                .push(origin.credential_path().map(<[u8]>::to_vec));
            Ok(Some(LfsAuthCredentials::new(
                LfsAuthHeaders::from_pairs(vec![(
                    "Authorization".to_owned(),
                    format!("Basic {}", operation.label()).into_bytes(),
                )])?,
                LfsAuthExpiry::never(),
            )))
        }
    }

    impl LfsSessionCredentialProvider for FakeCredentials {
        fn approve_authentication(
            &self,
            _endpoint: &LfsHttpUrl,
            _origin: &LfsAuthOrigin,
            _operation: LfsOperation,
        ) -> Result<(), LfsAuthError> {
            self.0.approves.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn reject_authentication(
            &self,
            _endpoint: &LfsHttpUrl,
            _origin: &LfsAuthOrigin,
            _operation: LfsOperation,
        ) -> Result<(), LfsAuthError> {
            self.0.rejects.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct HttpRecord {
        operation: LfsOperation,
        url: String,
        objects: Vec<LfsTransferObject>,
        headers: Vec<(String, String)>,
        concurrency: usize,
    }

    #[derive(Default)]
    struct HttpState {
        executions: RefCell<VecDeque<LfsNetworkHttpExecution>>,
        records: RefCell<Vec<HttpRecord>>,
    }

    #[derive(Clone)]
    struct FakeHttp(Rc<HttpState>);

    impl FakeHttp {
        fn new(executions: Vec<LfsNetworkHttpExecution>) -> (Self, Rc<HttpState>) {
            let state = Rc::new(HttpState {
                executions: RefCell::new(executions.into()),
                records: RefCell::new(Vec::new()),
            });
            (Self(Rc::clone(&state)), state)
        }
    }

    impl LfsNetworkHttpExecutor for FakeHttp {
        fn execute(
            &self,
            endpoint: &LfsEndpoint,
            _store: Arc<LfsStore>,
            auth_headers: LfsTransferAuthHeaders,
            _action_credentials: Arc<dyn LfsActionCredentialProvider>,
            config: &LfsTransferConfig,
            objects: &[LfsTransferObject],
            _reference: Option<LfsBatchRef>,
        ) -> LfsNetworkHttpExecution {
            let headers = auth_headers
                .headers()
                .entries()
                .iter()
                .map(|header| (header.name().to_owned(), header.value().to_owned()))
                .collect();
            self.0.records.borrow_mut().push(HttpRecord {
                operation: endpoint.operation,
                url: endpoint.url.as_str().to_owned(),
                objects: objects.to_vec(),
                headers,
                concurrency: config.concurrency().get(),
            });
            let execution = self
                .0
                .executions
                .borrow_mut()
                .pop_front()
                .expect("scripted HTTP execution");
            match execution {
                LfsNetworkHttpExecution::Complete(report)
                    if report.requested().is_empty() && !objects.is_empty() =>
                {
                    LfsNetworkHttpExecution::Complete(LfsTransferReport::complete_for_test_objects(
                        objects,
                    ))
                }
                execution => execution,
            }
        }
    }

    #[derive(Default)]
    struct SshState {
        outputs: RefCell<VecDeque<LfsSshCommandOutput>>,
        calls: Cell<usize>,
    }

    #[derive(Clone)]
    struct FakeSshRunner(Rc<SshState>);

    impl LfsSshCommandRunner for FakeSshRunner {
        fn run(
            &self,
            _destination: &LfsSshDestination,
            program: &str,
            args: &[String],
            _timeout: Duration,
            _stdout_limit: usize,
            _stderr_limit: usize,
        ) -> Result<LfsSshCommandOutput, LfsAuthError> {
            assert_eq!(program, "git-lfs-authenticate");
            assert_eq!(args.len(), 2);
            self.0.calls.set(self.0.calls.get() + 1);
            self.0
                .outputs
                .borrow_mut()
                .pop_front()
                .ok_or(LfsAuthError::ProcessFailed)
        }
    }

    fn temporary_directory() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zmin-lfs-network-session-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary directory");
        path
    }

    fn entry(name: &str, value: &str) -> ConfigEntry {
        let (section, subsection, key) = parse_config_name(name).expect("config name");
        ConfigEntry {
            raw_section: section.clone(),
            section,
            subsection,
            raw_key: key.clone(),
            key,
            value: value.to_owned(),
            comment: None,
            implicit_bool: false,
            scope: ConfigScope::Local,
            origin: "test".to_owned(),
            line: Some(1),
        }
    }

    fn config(root: &Path, entries: &[ConfigEntry]) -> LfsRuntimeConfig {
        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("git directory");
        LfsRuntimeConfig::load(LfsRuntimeConfigInput {
            git_dir: &git_dir,
            default_storage_git_dir: &git_dir,
            lfsconfig: LfsConfigSources::new(None, None, None),
            entries,
            branch: Some("main"),
            requested_remote: None,
            skip_smudge: None,
            skip_download_errors: None,
            http_environment: LfsHttpEnvironmentSnapshot::default(),
            fetch_head: None,
            url_rewriter: None,
        })
        .expect("runtime config")
    }

    fn object(byte: u8, size: u64) -> LfsTransferObject {
        let hex = format!("{byte:02x}").repeat(32);
        LfsTransferObject::new(LfsOid::from_hex(&hex).expect("oid"), size).expect("object")
    }

    fn session(
        root: &Path,
        entries: &[ConfigEntry],
        executions: Vec<LfsNetworkHttpExecution>,
    ) -> (
        LfsNetworkSession<
            FakeCredentials,
            GitLfsAuthenticateSsh<FakeSshRunner>,
            FakeHttp,
            FixedClock,
        >,
        Arc<CredentialState>,
        Rc<HttpState>,
        Rc<SshState>,
    ) {
        session_with_transfer_config(root, entries, executions, LfsTransferConfig::default())
    }

    fn session_with_transfer_config(
        root: &Path,
        entries: &[ConfigEntry],
        executions: Vec<LfsNetworkHttpExecution>,
        transfer_config: LfsTransferConfig,
    ) -> (
        LfsNetworkSession<
            FakeCredentials,
            GitLfsAuthenticateSsh<FakeSshRunner>,
            FakeHttp,
            FixedClock,
        >,
        Arc<CredentialState>,
        Rc<HttpState>,
        Rc<SshState>,
    ) {
        let credential_state = Arc::new(CredentialState::default());
        let ssh_state = Rc::new(SshState::default());
        let (http, http_state) = FakeHttp::new(executions);
        let session = LfsNetworkSession::new(
            config(root, entries),
            Arc::new(LfsStore::new(root.join("objects")).expect("store")),
            FakeCredentials(Arc::clone(&credential_state)),
            GitLfsAuthenticateSsh::new(
                FakeSshRunner(Rc::clone(&ssh_state)),
                Duration::from_secs(5),
            )
            .expect("SSH adapter"),
            http,
            FixedClock(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000)),
            transfer_config,
        )
        .expect("session");
        (session, credential_state, http_state, ssh_state)
    }

    #[test]
    fn action_credential_cache_is_scoped_to_actual_url_path_and_reject_invalidates() {
        let root = temporary_directory();
        let runtime = config(
            &root,
            &[
                entry("credential.usehttppath", "true"),
                entry("lfs.https://storage.example/team.access", "basic"),
            ],
        );
        let state = Arc::new(CredentialState::default());
        let credentials = LfsSessionActionCredentials::new(
            runtime,
            FakeCredentials(Arc::clone(&state)),
            FixedClock(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000)),
        )
        .expect("action credentials");
        let action =
            parse_http_url("https://storage.example/team/object").expect("actual action URL");

        assert!(
            credentials
                .credentials(&action, LfsOperation::Fetch)
                .expect("first fill")
                .is_some()
        );
        assert!(
            credentials
                .credentials(&action, LfsOperation::Fetch)
                .expect("cached fill")
                .is_some()
        );
        assert_eq!(state.fetch_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            *state.paths.lock().expect("credential paths"),
            vec![Some(b"team/object".to_vec())]
        );

        credentials
            .reject(&action, LfsOperation::Fetch)
            .expect("reject exact action credentials");
        assert!(
            credentials
                .credentials(&action, LfsOperation::Fetch)
                .expect("fill after reject")
                .is_some()
        );
        assert_eq!(state.fetch_calls.load(Ordering::Relaxed), 2);
        assert_eq!(state.rejects.load(Ordering::Relaxed), 1);
        let _ = fs::remove_dir_all(root);
    }

    fn complete_execution() -> LfsNetworkHttpExecution {
        LfsNetworkHttpExecution::Complete(LfsTransferReport::complete_for_test())
    }

    fn recoverable_fail_fast_execution(
        requested: &[LfsTransferObject],
        authenticated_failure: LfsTransferObject,
        completed_peer: LfsTransferObject,
        queued: LfsTransferObject,
        cancelled_peer: LfsTransferObject,
    ) -> LfsNetworkHttpExecution {
        classify_transfer_report(LfsTransferReport::with_test_outcomes(
            requested.to_vec(),
            vec![completed_peer],
            vec![
                (
                    authenticated_failure,
                    LfsTransferFailure::HttpStatus {
                        status: 401,
                        scope: LfsTransferHttpScope::SameOrigin,
                        auth: LfsTransferActionAuth::CredentialHelper,
                    },
                ),
                (cancelled_peer, LfsTransferFailure::Cancelled),
            ],
            vec![queued],
            false,
        ))
    }

    fn assert_fail_fast_authentication_recovery(operation: LfsOperation) {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
        ];
        let authenticated_failure = object(0x71, 1);
        let completed_peer = object(0x72, 2);
        let queued = object(0x73, 3);
        let cancelled_peer = object(0x74, 4);
        let requested = vec![
            authenticated_failure,
            completed_peer,
            queued,
            cancelled_peer,
        ];
        let retry_objects = vec![authenticated_failure, queued, cancelled_peer];
        let first = recoverable_fail_fast_execution(
            &requested,
            authenticated_failure,
            completed_peer,
            queued,
            cancelled_peer,
        );
        let retry = classify_transfer_report(LfsTransferReport::with_test_outcomes(
            retry_objects.clone(),
            retry_objects.clone(),
            Vec::new(),
            Vec::new(),
            true,
        ));
        let (mut session, credentials, http, _) = session(&root, &entries, vec![first, retry]);
        let request = LfsNetworkRequest::new(requested);
        let outcome = match operation {
            LfsOperation::Fetch => session.download_missing(&request),
            LfsOperation::Push => session.upload(&request),
        }
        .expect("one bounded authentication refresh");
        let LfsNetworkOutcome::Transfer(outcome) = outcome else {
            panic!("transfer outcome")
        };
        assert_eq!(outcome.disposition(), LfsNetworkDisposition::Complete);
        assert_eq!(outcome.unresolved_failures(), 0);
        assert_eq!(outcome.authentication_refreshes(), 0);
        assert_eq!(outcome.action_refetches(), 1);
        assert_eq!(outcome.reports().len(), 2);
        assert_eq!(outcome.reports()[0].pending().len(), 1);
        assert_eq!(outcome.reports()[0].pending()[0].object(), queued);
        let records = http.records.borrow();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].objects, retry_objects);
        assert!(!records[1].objects.contains(&completed_peer));
        assert!(records[1].objects.contains(&cancelled_peer));
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn system_session_reuses_the_exact_configured_store() {
        let root = temporary_directory();
        let runtime_config = config(&root, &[]);
        let store =
            Arc::new(LfsStore::new(root.join(".git").join("lfs").join("objects")).expect("store"));
        let session = SystemLfsNetworkSession::system(
            runtime_config,
            Arc::clone(&store),
            LfsTransferConfig::default(),
            Duration::from_secs(30),
        )
        .expect("system session");
        assert!(Arc::ptr_eq(&session.store, &store));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_propagates_one_immutable_transfer_concurrency() {
        let root = temporary_directory();
        let entries = vec![entry(
            "remote.origin.url",
            "https://example.test/team/repository.git",
        )];
        let transfer_config = LfsTransferConfig::new(2).expect("transfer config");
        let (mut session, _, http, _) = session_with_transfer_config(
            &root,
            &entries,
            vec![complete_execution()],
            transfer_config,
        );
        session
            .download_missing(&LfsNetworkRequest::new(vec![object(0x18, 1)]))
            .expect("download");
        assert_eq!(http.records.borrow()[0].concurrency, 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn complete_classification_rejects_missing_object_accounting() {
        let first = object(0x1b, 1);
        let omitted = object(0x1c, 2);
        let report = LfsTransferReport::with_test_outcomes(
            vec![first, omitted],
            vec![first],
            Vec::new(),
            Vec::new(),
            true,
        );
        assert!(matches!(
            classify_transfer_report(report),
            LfsNetworkHttpExecution::Failed(LfsTransferError::IncompleteReport)
        ));
    }

    #[test]
    fn session_passes_path_scoped_credentials_for_same_origin_endpoints() {
        let root = temporary_directory();
        let entries = vec![
            entry("remote.origin.url", "https://example.test/team/r%65po.git"),
            entry("credential.useHttpPath", "false"),
            entry("credential.https://example.test/team.useHttpPath", "true"),
            entry(
                "lfs.https://example.test/team/r%65po.git/info/lfs.access",
                "basic",
            ),
            entry(
                "lfs.https://example.test/team/%FF.git/info/lfs.access",
                "basic",
            ),
        ];
        let (mut session, credentials, _, _) = session(
            &root,
            &entries,
            vec![complete_execution(), complete_execution()],
        );
        session
            .download_missing(&LfsNetworkRequest::new(vec![object(0x19, 1)]))
            .expect("first endpoint");
        session
            .download_missing(
                &LfsNetworkRequest::new(vec![object(0x1a, 1)]).with_anonymous_remote_url(
                    LfsRemoteUrlOverride::new("https://example.test/team/%FF.git")
                        .expect("remote URL"),
                ),
            )
            .expect("second endpoint");
        assert_eq!(
            *credentials.paths.lock().expect("credential paths"),
            vec![
                Some(b"team/repo.git/info/lfs".to_vec()),
                Some(b"team/\xff.git/info/lfs".to_vec()),
            ]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn url_scoped_locks_policy_fails_closed_before_http_transfer() {
        let root = temporary_directory();
        let entries = vec![
            entry("remote.origin.url", "https://example.test/team/repo.git"),
            entry("lfs.locksverify", "false"),
            entry("lfs.https://example.test/team.locksverify", "true"),
        ];
        let (mut session, _, http, _) = session(&root, &entries, vec![complete_execution()]);
        let result = session.upload(&LfsNetworkRequest::new(vec![object(0x09, 1)]));
        assert!(matches!(
            result,
            Err(LfsNetworkSessionError::LocksVerificationRequired)
        ));
        assert!(http.records.borrow().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn url_scoped_locks_false_overrides_global_true() {
        let root = temporary_directory();
        let entries = vec![
            entry("remote.origin.url", "https://example.test/team/repo.git"),
            entry("lfs.locksverify", "true"),
            entry(
                "lfs.https://example.test/team/repo.git.locksverify",
                "false",
            ),
        ];
        let (mut session, _, http, _) = session(&root, &entries, vec![complete_execution()]);
        session
            .upload(&LfsNetworkRequest::new(vec![object(0x0a, 1)]))
            .expect("scoped false permits push");
        assert_eq!(http.records.borrow().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ssh_authenticated_endpoint_enforces_scoped_locks_policy() {
        let root = temporary_directory();
        let entries = vec![
            entry("remote.origin.url", "git@example.test:org/repo.git"),
            entry(
                "lfs.https://example.test/org/repo.git/info/lfs.locksverify",
                "true",
            ),
        ];
        let (mut session, _, http, ssh) = session(&root, &entries, vec![complete_execution()]);
        ssh.outputs.borrow_mut().push_back(LfsSshCommandOutput {
            status: 0,
            stdout: br#"{"expires_in":3600}"#.to_vec(),
            stderr: Vec::new(),
        });
        assert!(matches!(
            session.upload(&LfsNetworkRequest::new(vec![object(0x0c, 1)])),
            Err(LfsNetworkSessionError::LocksVerificationRequired)
        ));
        assert_eq!(ssh.calls.get(), 1);
        assert!(http.records.borrow().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn anonymous_transport_url_resolves_without_configured_remote() {
        let root = temporary_directory();
        let (mut session, _, http, _) = session(&root, &[], vec![complete_execution()]);
        let request = LfsNetworkRequest::new(vec![object(0x0b, 1)]).with_anonymous_remote_url(
            LfsRemoteUrlOverride::new("https://example.test/team/repo.git").expect("anonymous URL"),
        );
        session.upload(&request).expect("anonymous upload");
        let records = http.records.borrow();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].url,
            "https://example.test/team/repo.git/info/lfs"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fetch_push_rewrites_and_auth_caches_are_operation_scoped() {
        let root = temporary_directory();
        let entries = vec![
            entry("remote.origin.url", "gh:org/repository.git"),
            entry("branch.main.remote", "origin"),
            entry("url.https://fetch.example/.insteadof", "gh:"),
            entry("url.https://push.example/.pushinsteadof", "gh:"),
            entry("lfs.https://fetch.example.access", "basic"),
            entry("lfs.https://push.example.access", "basic"),
        ];
        let (mut session, credentials, http, _) = session(
            &root,
            &entries,
            vec![
                complete_execution(),
                complete_execution(),
                complete_execution(),
            ],
        );
        let request = LfsNetworkRequest::new(vec![object(0x11, 3)]);
        session.download_missing(&request).expect("fetch one");
        session.download_missing(&request).expect("fetch two");
        session.upload(&request).expect("push");
        let records = http.records.borrow();
        assert_eq!(records[0].operation, LfsOperation::Fetch);
        assert_eq!(
            records[0].url,
            "https://fetch.example/org/repository.git/info/lfs"
        );
        assert_eq!(
            records[2].url,
            "https://push.example/org/repository.git/info/lfs"
        );
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.push_calls.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.approves.load(Ordering::Relaxed), 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unauthorized_rejects_and_reauthenticates_exactly_once() {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
        ];
        let wanted = object(0x22, 7);
        let (mut session, credentials, http, _) = session(
            &root,
            &entries,
            vec![
                LfsNetworkHttpExecution::RefreshAuthentication {
                    report: None,
                    objects: vec![wanted],
                    reason: LfsAuthRefreshReason::Unauthorized,
                },
                complete_execution(),
            ],
        );
        let outcome = session
            .download_missing(&LfsNetworkRequest::new(vec![wanted]))
            .expect("bounded refresh");
        let LfsNetworkOutcome::Transfer(outcome) = outcome else {
            panic!("transfer outcome");
        };
        assert_eq!(outcome.authentication_refreshes(), 1);
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 2);
        assert_eq!(credentials.approves.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 1);
        assert_eq!(http.records.borrow().len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fetch_fail_fast_refresh_recovers_cancelled_and_queued_objects() {
        assert_fail_fast_authentication_recovery(LfsOperation::Fetch);
    }

    #[test]
    fn push_fail_fast_refresh_recovers_cancelled_and_queued_objects() {
        assert_fail_fast_authentication_recovery(LfsOperation::Push);
    }

    #[test]
    fn incomplete_refresh_report_can_never_be_a_complete_outcome() {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
        ];
        let authenticated_failure = object(0x75, 1);
        let completed_peer = object(0x76, 2);
        let queued = object(0x77, 3);
        let cancelled_peer = object(0x78, 4);
        let requested = vec![
            authenticated_failure,
            completed_peer,
            queued,
            cancelled_peer,
        ];
        let retry_objects = vec![authenticated_failure, queued, cancelled_peer];
        let first = recoverable_fail_fast_execution(
            &requested,
            authenticated_failure,
            completed_peer,
            queued,
            cancelled_peer,
        );
        let retry = classify_transfer_report(LfsTransferReport::with_test_outcomes(
            retry_objects.clone(),
            vec![authenticated_failure],
            vec![(
                queued,
                LfsTransferFailure::MissingAction(LfsBatchActionKind::Download),
            )],
            vec![cancelled_peer],
            false,
        ));
        let (mut session, _, http, _) = session(&root, &entries, vec![first, retry]);
        let outcome = session
            .download_missing(&LfsNetworkRequest::new(requested))
            .expect("typed incomplete transfer outcome");
        let LfsNetworkOutcome::Transfer(outcome) = outcome else {
            panic!("transfer outcome")
        };
        assert_eq!(
            outcome.disposition(),
            LfsNetworkDisposition::FailureRequired
        );
        assert_eq!(outcome.unresolved_failures(), 2);
        assert_eq!(http.records.borrow()[1].objects, retry_objects);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unspecified_access_waits_for_challenge_before_credential_fill() {
        let root = temporary_directory();
        let entries = vec![entry("lfs.url", "https://lfs.example/team")];
        let wanted = object(0x23, 7);
        let (mut session, credentials, http, _) = session(
            &root,
            &entries,
            vec![
                LfsNetworkHttpExecution::RefreshAuthentication {
                    report: None,
                    objects: vec![wanted],
                    reason: LfsAuthRefreshReason::Unauthorized,
                },
                complete_execution(),
            ],
        );
        let outcome = session
            .download_missing(&LfsNetworkRequest::new(vec![wanted]))
            .expect("challenge refresh");
        let LfsNetworkOutcome::Transfer(outcome) = outcome else {
            panic!("transfer outcome");
        };
        assert_eq!(outcome.authentication_refreshes(), 1);
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 0);
        let records = http.records.borrow();
        assert!(records[0].headers.is_empty());
        assert!(!records[1].headers.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn expired_action_refetches_only_failed_objects_without_reject() {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
        ];
        let first = object(0x31, 5);
        let expired = object(0x32, 6);
        let (mut session, credentials, http, _) = session(
            &root,
            &entries,
            vec![
                LfsNetworkHttpExecution::RefreshAuthentication {
                    report: Some(LfsTransferReport::with_test_outcomes(
                        vec![first, expired],
                        vec![first],
                        vec![(
                            expired,
                            LfsTransferFailure::ActionExpired(LfsBatchActionKind::Download),
                        )],
                        Vec::new(),
                        true,
                    )),
                    objects: vec![expired],
                    reason: LfsAuthRefreshReason::ExpiredAction,
                },
                complete_execution(),
            ],
        );
        let outcome = session
            .download_missing(&LfsNetworkRequest::new(vec![first, expired]))
            .expect("refetch");
        let LfsNetworkOutcome::Transfer(outcome) = outcome else {
            panic!("transfer outcome");
        };
        assert_eq!(outcome.disposition(), LfsNetworkDisposition::Complete);
        assert_eq!(outcome.authentication_refreshes(), 0);
        assert_eq!(outcome.action_refetches(), 1);
        let records = http.records.borrow();
        assert_eq!(records[0].objects, vec![first, expired]);
        assert_eq!(records[1].objects, vec![expired]);
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.approves.load(Ordering::Relaxed), 2);
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn second_authentication_refresh_is_never_retried() {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
        ];
        let wanted = object(0x41, 7);
        let refresh = || LfsNetworkHttpExecution::RefreshAuthentication {
            report: None,
            objects: vec![wanted],
            reason: LfsAuthRefreshReason::Unauthorized,
        };
        let (mut session, credentials, http, _) =
            session(&root, &entries, vec![refresh(), refresh()]);
        assert!(matches!(
            session.download_missing(&LfsNetworkRequest::new(vec![wanted])),
            Err(LfsNetworkSessionError::AuthenticationRetryExhausted(
                LfsAuthRefreshReason::Unauthorized
            ))
        ));
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 2);
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 2);
        assert_eq!(http.records.borrow().len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cross_origin_action_rejection_refetches_only_failed_objects_without_endpoint_reauth() {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
        ];
        let completed = object(0x48, 6);
        let rejected = object(0x49, 7);
        let first = classify_transfer_report(LfsTransferReport::with_test_outcomes(
            vec![completed, rejected],
            vec![completed],
            vec![(
                rejected,
                LfsTransferFailure::HttpStatus {
                    status: 401,
                    scope: LfsTransferHttpScope::CrossOrigin,
                    auth: LfsTransferActionAuth::ActionHeader,
                },
            )],
            Vec::new(),
            true,
        ));
        let (mut session, credentials, http, _) =
            session(&root, &entries, vec![first, complete_execution()]);

        let outcome = session
            .download_missing(&LfsNetworkRequest::new(vec![completed, rejected]))
            .expect("action-only refetch");
        let LfsNetworkOutcome::Transfer(outcome) = outcome else {
            panic!("transfer outcome");
        };
        assert_eq!(outcome.disposition(), LfsNetworkDisposition::Complete);
        assert_eq!(outcome.authentication_refreshes(), 0);
        assert_eq!(outcome.action_refetches(), 1);
        assert_eq!(outcome.unresolved_failures(), 0);
        let records = http.records.borrow();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].objects, vec![completed, rejected]);
        assert_eq!(records[1].objects, vec![rejected]);
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.approves.load(Ordering::Relaxed), 2);
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repeated_cross_origin_action_rejection_stops_after_one_refetch() {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
        ];
        let rejected = object(0x4a, 8);
        let rejection = || {
            classify_transfer_report(LfsTransferReport::with_test_failures(vec![(
                rejected,
                LfsTransferFailure::HttpStatus {
                    status: 403,
                    scope: LfsTransferHttpScope::CrossOrigin,
                    auth: LfsTransferActionAuth::ActionHeader,
                },
            )]))
        };
        let (mut session, credentials, http, _) =
            session(&root, &entries, vec![rejection(), rejection()]);

        assert!(matches!(
            session.download_missing(&LfsNetworkRequest::new(vec![rejected])),
            Err(LfsNetworkSessionError::AuthenticationRetryExhausted(
                LfsAuthRefreshReason::RejectedAction
            ))
        ));
        assert_eq!(http.records.borrow().len(), 2);
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.approves.load(Ordering::Relaxed), 1);
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn access_none_never_calls_the_credential_provider() {
        let root = temporary_directory();
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "none"),
        ];
        let (mut session, credentials, http, _) =
            session(&root, &entries, vec![complete_execution()]);
        session
            .download_missing(&LfsNetworkRequest::new(vec![object(0x51, 1)]))
            .expect("anonymous fetch");
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 0);
        assert!(http.records.borrow()[0].headers.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn configured_batch_authorization_suppresses_helper_on_initial_and_refresh_attempts() {
        let root = temporary_directory();
        let wanted = object(0x53, 1);
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "basic"),
            entry(
                "http.https://lfs.example/team.extraheader",
                "Authorization: Bearer configured",
            ),
        ];
        let refresh = || LfsNetworkHttpExecution::RefreshAuthentication {
            report: None,
            objects: vec![wanted],
            reason: LfsAuthRefreshReason::Unauthorized,
        };
        let (mut session, credentials, http, _) =
            session(&root, &entries, vec![refresh(), refresh()]);

        assert!(matches!(
            session.download_missing(&LfsNetworkRequest::new(vec![wanted])),
            Err(LfsNetworkSessionError::AuthenticationRetryExhausted(
                LfsAuthRefreshReason::Unauthorized
            ))
        ));
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 0);
        assert_eq!(credentials.approves.load(Ordering::Relaxed), 0);
        assert_eq!(credentials.rejects.load(Ordering::Relaxed), 0);
        let records = http.records.borrow();
        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.headers.is_empty()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn access_none_unauthorized_honors_typed_skip_without_retry() {
        let root = temporary_directory();
        let wanted = object(0x52, 1);
        let entries = vec![
            entry("lfs.url", "https://lfs.example/team"),
            entry("lfs.https://lfs.example/team.access", "none"),
            entry("lfs.skipdownloaderrors", "true"),
        ];
        let (mut session, credentials, http, _) = session(
            &root,
            &entries,
            vec![LfsNetworkHttpExecution::RefreshAuthentication {
                report: None,
                objects: vec![wanted],
                reason: LfsAuthRefreshReason::Unauthorized,
            }],
        );
        assert!(matches!(
            session.download_missing(&LfsNetworkRequest::new(vec![wanted])),
            Ok(LfsNetworkOutcome::DownloadFailureSkipped(
                LfsSkippedDownloadFailure::AuthenticationRefreshExhausted(
                    LfsAuthRefreshReason::Unauthorized
                )
            ))
        ));
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 0);
        assert_eq!(http.records.borrow().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ssh_authentication_uses_discovered_default_and_is_cached_per_operation() {
        let root = temporary_directory();
        let entries = vec![entry("remote.origin.url", "git@example.test:org/repo.git")];
        let (mut network_session, credentials, http, ssh) = session(
            &root,
            &entries,
            vec![complete_execution(), complete_execution()],
        );
        ssh.outputs.borrow_mut().push_back(LfsSshCommandOutput {
            status: 0,
            stdout: br#"{"href":"https://lfs.example/org/repo","header":{"Authorization":"Remote token"},"expires_in":3600}"#.to_vec(),
            stderr: Vec::new(),
        });
        let request = LfsNetworkRequest::new(vec![object(0x61, 9)]);
        network_session
            .download_missing(&request)
            .expect("SSH fetch one");
        network_session
            .download_missing(&request)
            .expect("SSH fetch two");
        assert_eq!(ssh.calls.get(), 1);
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 0);
        assert_eq!(http.records.borrow()[0].url, "https://lfs.example/org/repo");
        assert_eq!(http.records.borrow()[0].headers[0].1, "Remote token");

        let missing_href_root = temporary_directory();
        let (mut missing_href, _, missing_http, missing_ssh) =
            session(&missing_href_root, &entries, vec![complete_execution()]);
        missing_ssh
            .outputs
            .borrow_mut()
            .push_back(LfsSshCommandOutput {
                status: 0,
                stdout: br#"{"expires_in":3600}"#.to_vec(),
                stderr: Vec::new(),
            });
        missing_href
            .download_missing(&request)
            .expect("optional href uses the discovered SSH fallback endpoint");
        assert_eq!(missing_ssh.calls.get(), 1);
        assert_eq!(
            missing_http.records.borrow()[0].url,
            "https://example.test/org/repo.git/info/lfs"
        );
        assert!(missing_http.records.borrow()[0].headers.is_empty());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(missing_href_root);
    }

    #[test]
    fn remote_policy_override_and_download_skip_are_typed() {
        let root = temporary_directory();
        let entries = vec![
            entry("remote.origin.url", "https://origin.example/repo.git"),
            entry("lfs.remote.autodetect", "true"),
            entry("lfs.remote.searchall", "true"),
            entry("lfs.skipdownloaderrors", "true"),
        ];
        let (mut session, credentials, http, _) = session(
            &root,
            &entries,
            vec![LfsNetworkHttpExecution::Failed(
                LfsTransferError::EmptyRequest,
            )],
        );
        let request = LfsNetworkRequest::new(vec![object(0x71, 2)]);
        let LfsNetworkOutcome::RemoteSelectionRequired(requirement) = session
            .download_missing(&request)
            .expect("remote selection")
        else {
            panic!("remote selection outcome");
        };
        assert!(requirement.autodetect());
        assert!(requirement.search_all());
        assert_eq!(credentials.fetch_calls.load(Ordering::Relaxed), 0);

        let override_url =
            LfsRemoteUrlOverride::new("https://override.example/repo.git").expect("override");
        let outcome = session
            .download_missing(&request.clone().with_remote_url("origin", override_url))
            .expect("skipped transfer failure");
        assert!(matches!(
            outcome,
            LfsNetworkOutcome::DownloadFailureSkipped(LfsSkippedDownloadFailure::Transfer(
                LfsTransferError::EmptyRequest
            ))
        ));
        assert_eq!(
            http.records.borrow()[0].url,
            "https://override.example/repo.git/info/lfs"
        );

        let global_root = temporary_directory();
        let global_entries = vec![
            entry("lfs.url", "https://global.example/lfs"),
            entry("lfs.remote.autodetect", "true"),
            entry("lfs.remote.searchall", "true"),
        ];
        let (mut global, _, global_http, _) =
            self::session(&global_root, &global_entries, vec![complete_execution()]);
        assert!(matches!(
            global
                .download_missing(&LfsNetworkRequest::new(vec![object(0x72, 3)]))
                .expect("global endpoint does not require remote probing"),
            LfsNetworkOutcome::Transfer(_)
        ));
        assert_eq!(
            global_http.records.borrow()[0].url,
            "https://global.example/lfs"
        );
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(global_root);
    }

    #[test]
    fn hook_transport_url_preserves_remote_specific_lfs_endpoints() {
        let root = temporary_directory();
        let entries = vec![
            entry("remote.origin.url", "https://git.example/repository.git"),
            entry("remote.origin.lfsurl", "https://lfs.example/fetch"),
            entry("remote.origin.lfspushurl", "https://lfs.example/push"),
        ];
        let (mut session, _, http, _) = session(
            &root,
            &entries,
            vec![complete_execution(), complete_execution()],
        );
        let transport =
            LfsRemoteUrlOverride::new("https://hook.example/repository.git").expect("transport");
        let request =
            LfsNetworkRequest::new(vec![object(0x81, 2)]).with_remote_url("origin", transport);
        session.download_missing(&request).expect("fetch");
        session.upload(&request).expect("push");
        let records = http.records.borrow();
        assert_eq!(records[0].url, "https://lfs.example/fetch");
        assert_eq!(records[1].url, "https://lfs.example/push");
        let _ = fs::remove_dir_all(root);
    }
}
