//! Streaming Git LFS Basic Transfer orchestration.
//!
//! This module is intentionally the thin integration layer between endpoint
//! discovery, the bounded Batch codec, the content-addressed store, and the
//! generic HTTP transport.  Object bytes never enter a `Vec` here: downloads
//! are handed directly to [`LfsStore::ingest`] and uploads are reopened from
//! [`LfsStore::open_transport_file`] for every HTTP attempt.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, Cursor, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::DateTime;

use zmin_http_transport::{
    ClientConfig, ConfiguredRequestHeaderProvenance, ConfiguredRequestHeaders, HttpClient,
    HttpOrigin, HttpRequest, HttpUrl, RedirectPolicy, RequestBodyCancellation, RequestBodyFactory,
    RequestHeader, RequestHeaders, RequestPolicyResolver, ResolvedRequestPolicy, StreamingResponse,
    TransportError,
};

use super::{
    parse_batch_response, parse_http_url, serialize_batch_request, serialize_verify_request,
    split_batch_request, LfsAuthError, LfsBatchAction, LfsBatchActionEntry, LfsBatchActionKind,
    LfsBatchError, LfsBatchErrorResponse, LfsBatchHeader, LfsBatchHeaders, LfsBatchObject,
    LfsBatchOperation, LfsBatchRef, LfsBatchRequest, LfsBatchResponse, LfsBatchResponseObject,
    LfsBatchSuccessResponse, LfsEndpoint, LfsEndpointSource, LfsHttpPolicy, LfsHttpUrl, LfsOid,
    LfsOperation, LfsStore, LfsStoreError, LfsStoreOutcome,
};

const MAX_TRANSFER_CONCURRENCY: usize = 8;
const DEFAULT_BATCH_MAX_OBJECTS: usize = 1024;
const DEFAULT_BATCH_MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_BATCH_SPLIT_DEPTH: usize = 16;
const MAX_BATCH_ATTEMPTS: usize = 4096;
const LFS_TRANSFER_WORKER_STACK_BYTES: usize = 256 * 1024;
const ACTION_EXPIRY_SKEW: Duration = Duration::from_secs(30);
const LFS_BATCH_ACCEPT: &str = "application/vnd.git-lfs+json";
const LFS_MEDIA_CONTENT_TYPE: &str = "application/octet-stream";

struct LfsTransferWorkerJob {
    index: usize,
    operation: LfsBatchOperation,
    response: LfsBatchResponseObject,
    action_issued_at: SystemTime,
}

struct LfsTransferWorkerResult {
    index: usize,
    result: ObjectResult,
}

enum LfsTransferWorkerMessage {
    Completed(LfsTransferWorkerResult),
    Panicked,
}

#[cfg(test)]
#[derive(Default)]
struct LfsTransferWorkerTestObserver {
    spawned: std::sync::atomic::AtomicUsize,
    waves: std::sync::atomic::AtomicUsize,
    active: std::sync::atomic::AtomicUsize,
    max_active: std::sync::atomic::AtomicUsize,
    started: Mutex<Vec<usize>>,
    thread_names: Mutex<std::collections::HashSet<String>>,
    panic_index: Option<usize>,
}

#[cfg(test)]
impl LfsTransferWorkerTestObserver {
    fn panic_on(index: usize) -> Self {
        Self {
            panic_index: Some(index),
            ..Self::default()
        }
    }

    fn worker_started(&self) {
        self.spawned
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let name = thread::current().name().unwrap_or_default().to_owned();
        self.thread_names
            .lock()
            .expect("worker observer thread names")
            .insert(name);
    }

    fn wave_started(&self) {
        self.waves.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn job_started(&self, index: usize) -> LfsTransferWorkerActivity<'_> {
        self.started
            .lock()
            .expect("worker observer starts")
            .push(index);
        assert_ne!(self.panic_index, Some(index), "injected worker panic");
        let active = self
            .active
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        self.max_active
            .fetch_max(active, std::sync::atomic::Ordering::SeqCst);
        LfsTransferWorkerActivity { observer: self }
    }
}

#[cfg(test)]
struct LfsTransferWorkerActivity<'a> {
    observer: &'a LfsTransferWorkerTestObserver,
}

#[cfg(test)]
impl Drop for LfsTransferWorkerActivity<'_> {
    fn drop(&mut self) {
        self.observer
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Typed credentials/header material supplied by the credential or SSH auth
/// layer.  The future `lfs_auth` adapter can construct this from its validated
/// `(name, bytes)` iterator without exposing raw maps to the transfer client.
/// Values are retained only in the existing redacted `LfsBatchHeaders` type.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct LfsTransferAuthHeaders {
    headers: LfsBatchHeaders,
}

impl fmt::Debug for LfsTransferAuthHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsTransferAuthHeaders")
            .field("count", &self.headers.entries().len())
            .finish()
    }
}

impl LfsTransferAuthHeaders {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    /// Convert validated authentication bytes into wiping secret header
    /// storage. No intermediate or retained `String` owns the value.
    pub(crate) fn from_pairs(pairs: Vec<(String, Vec<u8>)>) -> Result<Self, LfsBatchError> {
        let mut pairs = pairs.into_iter();
        let mut headers = Vec::with_capacity(pairs.len());
        while let Some((name, value)) = pairs.next() {
            match LfsBatchHeader::new_secret(name, value) {
                Ok(header) => headers.push(header),
                Err(error) => {
                    for (_, mut remaining_value) in pairs {
                        remaining_value.fill(0);
                    }
                    return Err(error);
                }
            }
        }
        Ok(Self {
            headers: LfsBatchHeaders::new(headers)?,
        })
    }

    pub(crate) fn headers(&self) -> &LfsBatchHeaders {
        &self.headers
    }
}

/// Per-action Git credential boundary. Git LFS applies this only when the
/// Batch object is not marked `authenticated` and the actual action request
/// does not already carry Authorization or a token query parameter.
pub(crate) trait LfsActionCredentialProvider: Send + Sync {
    fn credentials(
        &self,
        action: &LfsHttpUrl,
        operation: LfsOperation,
    ) -> Result<Option<LfsTransferAuthHeaders>, LfsAuthError>;

    fn approve(&self, action: &LfsHttpUrl, operation: LfsOperation) -> Result<(), LfsAuthError>;

    fn reject(&self, action: &LfsHttpUrl, operation: LfsOperation) -> Result<(), LfsAuthError>;
}

/// A validated number of simultaneous object actions.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LfsTransferConcurrency(usize);

impl fmt::Debug for LfsTransferConcurrency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("LfsTransferConcurrency")
            .field(&self.0)
            .finish()
    }
}

impl LfsTransferConcurrency {
    pub(crate) fn new(value: usize) -> Result<Self, LfsTransferConfigError> {
        if (1..=MAX_TRANSFER_CONCURRENCY).contains(&value) {
            Ok(Self(value))
        } else {
            Err(LfsTransferConfigError::InvalidConcurrency)
        }
    }

    pub(crate) fn get(self) -> usize {
        self.0
    }
}

impl Default for LfsTransferConcurrency {
    fn default() -> Self {
        Self(MAX_TRANSFER_CONCURRENCY)
    }
}

/// Whether object failures stop after the current bounded worker wave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfsPartialResultPolicy {
    Continue,
    FailFast,
}

/// Transfer limits and HTTP safety policy.
///
/// `ClientConfig::redirect_policy` defaults to same-origin body replay and no
/// cross-origin caller headers.  `retry.allow_replayable_writes` is enabled
/// by default because every write body in this module is reopenable: Batch
/// JSON is bounded and uploads reopen a same-handle store file whose exact
/// length and SHA-256 are verified by the transport before completion.
#[derive(Clone)]
pub(crate) struct LfsTransferConfig {
    concurrency: LfsTransferConcurrency,
    partial_results: LfsPartialResultPolicy,
    batch_max_objects: usize,
    batch_max_bytes: usize,
    http: ClientConfig,
    http_policy: Arc<LfsHttpPolicy>,
    #[cfg(test)]
    worker_observer: Option<Arc<LfsTransferWorkerTestObserver>>,
}

impl fmt::Debug for LfsTransferConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsTransferConfig")
            .field("concurrency", &self.concurrency)
            .field("partial_results", &self.partial_results)
            .field("batch_max_objects", &self.batch_max_objects)
            .field("batch_max_bytes", &self.batch_max_bytes)
            .field("http_timeout_policy", &self.http.timeout_policy)
            .field("http_request_timeout", &self.http.request_timeout)
            .field("http_operation_timeout", &self.http.operation_timeout)
            .field("http_connection_policy", &"<resolver-owned>")
            .field("http_policy", &self.http_policy)
            .finish()
    }
}

impl Default for LfsTransferConfig {
    fn default() -> Self {
        let concurrency = LfsTransferConcurrency::default();
        let mut http = ClientConfig::default();
        http.retry.allow_replayable_writes = true;
        http.redirect_policy = RedirectPolicy::default();
        // Git LFS owns dial, TLS-handshake, and idle-activity timeout
        // semantics. The generic transport's fixed whole-request and
        // operation caps would incorrectly terminate valid long transfers.
        http.request_timeout = None;
        http.operation_timeout = None;
        // Object bytes remain bounded by the expected LFS size and are
        // streamed into the store.  A small fixed transport cap would reject
        // perfectly valid large media files while providing no RSS benefit.
        http.max_response_body_bytes = u64::MAX;
        Self {
            concurrency,
            partial_results: LfsPartialResultPolicy::Continue,
            batch_max_objects: DEFAULT_BATCH_MAX_OBJECTS,
            batch_max_bytes: DEFAULT_BATCH_MAX_BYTES,
            http,
            http_policy: Arc::new(LfsHttpPolicy::default()),
            #[cfg(test)]
            worker_observer: None,
        }
    }
}

impl LfsTransferConfig {
    pub(crate) fn new(concurrency: usize) -> Result<Self, LfsTransferConfigError> {
        let concurrency = LfsTransferConcurrency::new(concurrency)?;
        Ok(Self::from_concurrency(concurrency))
    }

    pub(crate) fn from_concurrency(concurrency: LfsTransferConcurrency) -> Self {
        Self {
            concurrency,
            ..Self::default()
        }
    }

    pub(crate) fn with_partial_results(mut self, policy: LfsPartialResultPolicy) -> Self {
        self.partial_results = policy;
        self
    }

    pub(crate) fn with_batch_limits(
        mut self,
        max_objects: usize,
        max_bytes: usize,
    ) -> Result<Self, LfsTransferConfigError> {
        if max_objects == 0
            || max_objects > DEFAULT_BATCH_MAX_OBJECTS
            || max_bytes == 0
            || max_bytes > DEFAULT_BATCH_MAX_BYTES
        {
            return Err(LfsTransferConfigError::InvalidBatchLimits);
        }
        self.batch_max_objects = max_objects;
        self.batch_max_bytes = max_bytes;
        Ok(self)
    }

    /// Replace transport limits and redirect/retry settings. The embedded
    /// static connection policy is ignored because every LFS request is
    /// resolved through `http_policy`; transport body/header safeguards remain
    /// enforced.
    pub(crate) fn with_http_config(mut self, http: ClientConfig) -> Self {
        let mut http = http;
        http.retry.allow_replayable_writes = true;
        http.request_timeout = None;
        http.operation_timeout = None;
        self.http = http;
        self
    }

    pub(crate) fn with_http_policy(mut self, policy: Arc<LfsHttpPolicy>) -> Self {
        self.http_policy = policy;
        self
    }

    #[cfg(test)]
    fn with_worker_observer(mut self, observer: Arc<LfsTransferWorkerTestObserver>) -> Self {
        self.worker_observer = Some(observer);
        self
    }

    pub(crate) fn concurrency(&self) -> LfsTransferConcurrency {
        self.concurrency
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsTransferConfigError {
    InvalidConcurrency,
    InvalidBatchLimits,
}

impl fmt::Display for LfsTransferConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConcurrency => {
                formatter.write_str("LFS transfer concurrency must be 1..=8")
            }
            Self::InvalidBatchLimits => formatter.write_str("invalid LFS Batch limits"),
        }
    }
}

impl std::error::Error for LfsTransferConfigError {}

/// An object request.  Only the oid and size are retained; media bytes stay
/// in the local store or in the HTTP stream.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LfsTransferObject {
    oid: LfsOid,
    size: u64,
}

impl LfsTransferObject {
    pub(crate) fn new(oid: LfsOid, size: u64) -> Result<Self, LfsBatchError> {
        LfsBatchObject::new(oid, size)?;
        Ok(Self { oid, size })
    }

    pub(crate) fn oid(&self) -> LfsOid {
        self.oid
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}

/// A successfully completed object action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LfsTransferSuccess {
    object: LfsTransferObject,
    outcome: LfsTransferSuccessKind,
}

impl LfsTransferSuccess {
    pub(crate) fn object(&self) -> LfsTransferObject {
        self.object
    }

    pub(crate) fn outcome(&self) -> LfsTransferSuccessKind {
        self.outcome
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsTransferSuccessKind {
    Downloaded(LfsStoreOutcome),
    Uploaded,
}

/// A per-object failure.  Server messages are deliberately not retained;
/// the Batch parser has already validated and sanitized them, but they can
/// still contain repository- or service-controlled sensitive text.
pub(crate) enum LfsTransferFailure {
    ObjectError {
        code: u16,
    },
    MissingAction(LfsBatchActionKind),
    ActionExpired(LfsBatchActionKind),
    HttpStatus {
        status: u16,
        scope: LfsTransferHttpScope,
        auth: LfsTransferActionAuth,
    },
    Authentication(LfsAuthError),
    Transport(TransportError),
    Batch(LfsBatchError),
    Store(LfsStoreError),
    Cancelled,
    InvalidAction,
}

impl fmt::Debug for LfsTransferFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectError { code } => formatter
                .debug_struct("ObjectError")
                .field("code", code)
                .finish(),
            Self::MissingAction(kind) => {
                formatter.debug_tuple("MissingAction").field(kind).finish()
            }
            Self::ActionExpired(kind) => {
                formatter.debug_tuple("ActionExpired").field(kind).finish()
            }
            Self::HttpStatus {
                status,
                scope,
                auth,
            } => formatter
                .debug_struct("HttpStatus")
                .field("status", status)
                .field("scope", scope)
                .field("auth", auth)
                .finish(),
            Self::Authentication(error) => formatter
                .debug_tuple("Authentication")
                .field(error)
                .finish(),
            Self::Transport(error) => formatter.debug_tuple("Transport").field(error).finish(),
            Self::Batch(error) => formatter.debug_tuple("Batch").field(error).finish(),
            Self::Store(error) => formatter.debug_tuple("Store").field(error).finish(),
            Self::Cancelled => formatter.write_str("Cancelled"),
            Self::InvalidAction => formatter.write_str("InvalidAction"),
        }
    }
}

impl fmt::Display for LfsTransferFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectError { code } => write!(formatter, "LFS server rejected object ({code})"),
            Self::MissingAction(kind) => write!(formatter, "LFS response omitted {kind:?} action"),
            Self::ActionExpired(kind) => {
                write!(formatter, "LFS {kind:?} action expired; refetch required")
            }
            Self::HttpStatus { status, .. } => {
                write!(formatter, "LFS action returned HTTP status {status}")
            }
            Self::Authentication(error) => error.fmt(formatter),
            Self::Transport(error) => error.fmt(formatter),
            Self::Batch(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::Cancelled => formatter.write_str("LFS transfer cancelled"),
            Self::InvalidAction => formatter.write_str("invalid LFS action"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfsTransferHttpScope {
    SameOrigin,
    CrossOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfsTransferActionAuth {
    Preauthenticated,
    ActionHeader,
    ConfiguredHeader,
    CredentialHelper,
    None,
}

impl std::error::Error for LfsTransferFailure {}

/// Deterministic partial result, ordered as the de-duplicated input.
#[derive(Default)]
pub(crate) struct LfsTransferReport {
    requested: Vec<LfsTransferObject>,
    succeeded: Vec<LfsTransferSuccess>,
    failed: Vec<(LfsTransferObject, LfsTransferFailure)>,
    pending: Vec<LfsTransferPending>,
    deduplicated: usize,
    batch_requests: usize,
    split_requests: usize,
    refetch_required: bool,
    complete: bool,
}

impl fmt::Debug for LfsTransferReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsTransferReport")
            .field("requested", &self.requested.len())
            .field("succeeded", &self.succeeded.len())
            .field("failed", &self.failed.len())
            .field("pending", &self.pending.len())
            .field("deduplicated", &self.deduplicated)
            .field("batch_requests", &self.batch_requests)
            .field("split_requests", &self.split_requests)
            .field("refetch_required", &self.refetch_required)
            .field("complete", &self.complete)
            .finish()
    }
}

impl LfsTransferReport {
    pub(crate) fn requested(&self) -> &[LfsTransferObject] {
        &self.requested
    }

    pub(crate) fn succeeded(&self) -> &[LfsTransferSuccess] {
        &self.succeeded
    }

    pub(crate) fn failed(&self) -> &[(LfsTransferObject, LfsTransferFailure)] {
        &self.failed
    }

    pub(crate) fn pending(&self) -> &[LfsTransferPending] {
        &self.pending
    }

    pub(crate) fn deduplicated(&self) -> usize {
        self.deduplicated
    }

    pub(crate) fn batch_requests(&self) -> usize {
        self.batch_requests
    }

    pub(crate) fn split_requests(&self) -> usize {
        self.split_requests
    }

    pub(crate) fn requires_refetch(&self) -> bool {
        self.refetch_required
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.complete && self.pending.is_empty() && self.has_exact_accounting()
    }

    pub(crate) fn has_exact_accounting(&self) -> bool {
        let mut accounted = HashMap::with_capacity(self.requested.len());
        for object in &self.requested {
            if accounted.insert(*object, false).is_some() {
                return false;
            }
        }
        for object in self
            .succeeded
            .iter()
            .map(LfsTransferSuccess::object)
            .chain(self.failed.iter().map(|(object, _)| *object))
            .chain(self.pending.iter().map(LfsTransferPending::object))
        {
            let Some(seen) = accounted.get_mut(&object) else {
                return false;
            };
            if *seen {
                return false;
            }
            *seen = true;
        }
        accounted.values().all(|seen| *seen)
    }

    pub(crate) fn accounts_for(&self, objects: &[LfsTransferObject]) -> bool {
        let Ok((expected, _)) = deduplicate_objects(objects) else {
            return false;
        };
        if expected.len() != self.requested.len() {
            return false;
        }
        let expected = expected
            .into_iter()
            .map(|object| (object, ()))
            .collect::<HashMap<_, _>>();
        self.requested
            .iter()
            .all(|object| expected.contains_key(object))
            && self.has_exact_accounting()
    }

    pub(crate) fn unresolved_count(&self) -> usize {
        self.failed.len().saturating_add(self.pending.len())
    }

    pub(crate) fn refresh_candidates(&self) -> Vec<LfsTransferObject> {
        let mut retryable = HashMap::with_capacity(self.failed.len() + self.pending.len());
        for (object, failure) in &self.failed {
            if failure.is_authentication_refreshable()
                || matches!(failure, LfsTransferFailure::Cancelled)
            {
                retryable.insert(*object, ());
            }
        }
        for pending in &self.pending {
            retryable.insert(pending.object(), ());
        }
        self.requested
            .iter()
            .filter(|object| retryable.contains_key(object))
            .copied()
            .collect()
    }

    fn finalize(&mut self, attempted_all: bool) {
        let mut accounted = HashMap::with_capacity(self.requested.len());
        for success in &self.succeeded {
            accounted.insert(success.object(), ());
        }
        for (object, _) in &self.failed {
            accounted.insert(*object, ());
        }
        let pending_reason = if attempted_all {
            LfsTransferPendingReason::MissingObjectResult
        } else {
            LfsTransferPendingReason::NotStartedAfterFailFast
        };
        self.pending = self
            .requested
            .iter()
            .filter(|object| !accounted.contains_key(object))
            .copied()
            .map(|object| LfsTransferPending::new(object, pending_reason))
            .collect();
        self.complete = attempted_all && self.pending.is_empty() && self.has_exact_accounting();
    }

    #[cfg(test)]
    pub(crate) fn with_test_failures(failed: Vec<(LfsTransferObject, LfsTransferFailure)>) -> Self {
        let requested = failed.iter().map(|(object, _)| *object).collect();
        Self {
            requested,
            failed,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_outcomes(
        requested: Vec<LfsTransferObject>,
        succeeded: Vec<LfsTransferObject>,
        failed: Vec<(LfsTransferObject, LfsTransferFailure)>,
        pending: Vec<LfsTransferObject>,
        complete: bool,
    ) -> Self {
        Self {
            requested,
            succeeded: succeeded
                .into_iter()
                .map(|object| LfsTransferSuccess {
                    object,
                    outcome: LfsTransferSuccessKind::Uploaded,
                })
                .collect(),
            failed,
            pending: pending
                .into_iter()
                .map(|object| {
                    LfsTransferPending::new(
                        object,
                        LfsTransferPendingReason::NotStartedAfterFailFast,
                    )
                })
                .collect(),
            complete,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn complete_for_test() -> Self {
        Self {
            complete: true,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn complete_for_test_objects(requested: &[LfsTransferObject]) -> Self {
        Self::with_test_outcomes(
            requested.to_vec(),
            requested.to_vec(),
            Vec::new(),
            Vec::new(),
            true,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LfsTransferPending {
    object: LfsTransferObject,
    reason: LfsTransferPendingReason,
}

impl LfsTransferPending {
    fn new(object: LfsTransferObject, reason: LfsTransferPendingReason) -> Self {
        Self { object, reason }
    }

    pub(crate) fn object(&self) -> LfsTransferObject {
        self.object
    }

    pub(crate) fn reason(&self) -> LfsTransferPendingReason {
        self.reason
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfsTransferPendingReason {
    NotStartedAfterFailFast,
    MissingObjectResult,
}

/// A Basic Transfer client bound to one already-resolved HTTP endpoint.
pub(crate) struct LfsTransferClient {
    endpoint: LfsEndpoint,
    endpoint_origin: HttpOrigin,
    store: Arc<LfsStore>,
    auth_headers: LfsTransferAuthHeaders,
    action_credentials: Arc<dyn LfsActionCredentialProvider>,
    http: HttpClient,
    config: LfsTransferConfig,
}

impl fmt::Debug for LfsTransferClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsTransferClient")
            .field("operation", &self.endpoint.operation)
            .field("source", &self.endpoint.source)
            .field(
                "auth_header_count",
                &self.auth_headers.headers().entries().len(),
            )
            .field("action_credentials", &"<redacted>")
            .field("config", &self.config)
            .finish()
    }
}

impl LfsTransferClient {
    pub(crate) fn new(
        endpoint: &LfsEndpoint,
        store: Arc<LfsStore>,
        auth_headers: LfsTransferAuthHeaders,
        action_credentials: Arc<dyn LfsActionCredentialProvider>,
        config: LfsTransferConfig,
    ) -> Result<Self, LfsTransferError> {
        let endpoint_origin = batch_url(&endpoint.url)?.origin().clone();
        let resolver: Arc<dyn RequestPolicyResolver> = Arc::new(LfsEndpointBoundHttpPolicy {
            endpoint_origin: endpoint_origin.clone(),
            policy: config.http_policy.clone(),
        });
        let http = HttpClient::with_policy_resolver(config.http.clone(), resolver)
            .map_err(LfsTransferError::Transport)?;
        Ok(Self {
            endpoint: endpoint.clone(),
            endpoint_origin,
            store,
            auth_headers,
            action_credentials,
            http,
            config,
        })
    }

    pub(crate) fn download(
        &self,
        objects: &[LfsTransferObject],
        reference: Option<LfsBatchRef>,
    ) -> Result<LfsTransferReport, LfsTransferError> {
        self.transfer(LfsBatchOperation::Download, objects, reference)
    }

    pub(crate) fn upload(
        &self,
        objects: &[LfsTransferObject],
        reference: Option<LfsBatchRef>,
    ) -> Result<LfsTransferReport, LfsTransferError> {
        self.transfer(LfsBatchOperation::Upload, objects, reference)
    }

    fn transfer(
        &self,
        operation: LfsBatchOperation,
        objects: &[LfsTransferObject],
        reference: Option<LfsBatchRef>,
    ) -> Result<LfsTransferReport, LfsTransferError> {
        self.ensure_operation(operation)?;
        let (unique, deduplicated) = deduplicate_objects(objects)?;
        if unique.is_empty() {
            return Err(LfsTransferError::EmptyRequest);
        }
        let batch_objects = unique
            .iter()
            .map(|object| LfsBatchObject::new(object.oid(), object.size()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(LfsTransferError::Batch)?;
        let mut request =
            LfsBatchRequest::new(operation, batch_objects).map_err(LfsTransferError::Batch)?;
        if let Some(reference) = reference {
            request.set_reference(reference);
        }
        let requests = split_batch_request(
            &request,
            self.config.batch_max_objects,
            self.config.batch_max_bytes,
        )
        .map_err(LfsTransferError::Batch)?;
        let requested_indices = unique
            .iter()
            .copied()
            .enumerate()
            .map(|(index, object)| (object, index))
            .collect::<HashMap<_, _>>();
        let mut queue = requests
            .into_iter()
            .map(|request| (request, 0_usize))
            .collect::<VecDeque<_>>();
        let mut jobs = Vec::with_capacity(unique.len());
        let mut report = LfsTransferReport {
            requested: unique.clone(),
            deduplicated,
            ..LfsTransferReport::default()
        };
        let cancellation = LfsTransferCancellation::new();

        while let Some((request, split_depth)) = queue.pop_front() {
            if report.batch_requests >= MAX_BATCH_ATTEMPTS {
                return Err(LfsTransferError::SplitLimitExceeded);
            }
            report.batch_requests += 1;
            let response = self.post_batch(&request, &cancellation)?;
            let action_issued_at = SystemTime::now();
            let success = match response {
                LfsBatchResponse::Success(success) => success,
                LfsBatchResponse::Error(error)
                    if error.status() == 413 && request.objects().len() > 1 =>
                {
                    if split_depth >= MAX_BATCH_SPLIT_DEPTH {
                        return Err(LfsTransferError::SplitLimitExceeded);
                    }
                    let half = request.objects().len().div_ceil(2);
                    let split = split_batch_request(&request, half, self.config.batch_max_bytes)
                        .map_err(LfsTransferError::Batch)?;
                    if split.len() < 2 {
                        return Err(LfsTransferError::RequestTooLarge);
                    }
                    report.split_requests += 1;
                    for part in split.into_iter().rev() {
                        queue.push_front((part, split_depth + 1));
                    }
                    continue;
                }
                LfsBatchResponse::Error(error) if error.status() == 413 => {
                    return Err(LfsTransferError::RequestTooLarge);
                }
                LfsBatchResponse::Error(error) => {
                    return Err(LfsTransferError::BatchHttp(error));
                }
            };

            jobs.extend(collect_transfer_jobs(
                operation,
                &success,
                request.objects(),
                action_issued_at,
                &requested_indices,
            ));
        }

        let (results, cancelled) = self.execute_jobs(jobs, cancellation)?;
        let failed = results.iter().any(|result| result.is_failure());
        for result in results {
            match result {
                ObjectResult::Success(success) => report.succeeded.push(success),
                ObjectResult::Failure(object, error) => {
                    report.refetch_required |= error.needs_refetch();
                    report.failed.push((object, error));
                }
            }
        }
        if (failed || cancelled) && self.config.partial_results == LfsPartialResultPolicy::FailFast
        {
            report.finalize(false);
            return Ok(report);
        }
        report.finalize(true);
        Ok(report)
    }

    fn ensure_operation(&self, operation: LfsBatchOperation) -> Result<(), LfsTransferError> {
        let expected = match self.endpoint.operation {
            LfsOperation::Fetch => LfsBatchOperation::Download,
            LfsOperation::Push => LfsBatchOperation::Upload,
        };
        if operation == expected {
            Ok(())
        } else {
            Err(LfsTransferError::OperationMismatch)
        }
    }

    fn post_batch(
        &self,
        request: &LfsBatchRequest,
        cancellation: &LfsTransferCancellation,
    ) -> Result<LfsBatchResponse, LfsTransferError> {
        let body = serialize_batch_request(request).map_err(LfsTransferError::Batch)?;
        let request_body = bytes_body(body);
        let headers = merged_request_headers(
            Some(self.auth_headers.headers()),
            &LfsBatchHeaders::empty(),
            &[
                ("Accept", LFS_BATCH_ACCEPT.as_bytes()),
                ("Content-Type", LFS_BATCH_ACCEPT.as_bytes()),
            ],
        )
        .map_err(LfsTransferError::Transport)?;
        let url = batch_url(&self.endpoint.url)?;
        let http_request = HttpRequest::new("POST".parse().expect("static HTTP method"), url)
            .with_headers(headers)
            .with_cancellation(cancellation.clone())
            .with_body(request_body);
        let mut response = self
            .http
            .execute(&http_request)
            .map_err(LfsTransferError::Transport)?;
        let status = response.status();
        let parsed = parse_batch_response(status, &mut response, Some(request))
            .map_err(LfsTransferError::Batch);
        if status == 413 {
            // The Batch parser intentionally does not consume a potentially
            // unbounded 413 body.  Close the response before the caller
            // recursively enqueues split requests.
            drop(response);
        }
        parsed
    }

    fn execute_jobs(
        &self,
        jobs: Vec<LfsTransferWorkerJob>,
        cancellation: LfsTransferCancellation,
    ) -> Result<(Vec<ObjectResult>, bool), LfsTransferError> {
        if jobs.is_empty() {
            return Ok((Vec::new(), false));
        }
        let job_count = jobs.len();
        let worker_count = effective_transfer_workers(self.config.concurrency(), job_count);
        let fail_fast = self.config.partial_results == LfsPartialResultPolicy::FailFast;
        let (job_sender, job_receiver) = mpsc::sync_channel(worker_count);
        let job_receiver = Arc::new(LfsTransferJobReceiver::new(job_receiver));
        let (result_sender, result_receiver) = mpsc::sync_channel(worker_count);
        let output = thread::scope(|scope| {
            let mut workers = Vec::with_capacity(worker_count);
            for worker_index in 0..worker_count {
                let worker_receiver = Arc::clone(&job_receiver);
                let worker_sender = result_sender.clone();
                let worker_cancellation = cancellation.clone();
                let worker = thread::Builder::new()
                    .name(format!("zmin-lfs-transfer-{worker_index}"))
                    .stack_size(LFS_TRANSFER_WORKER_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        #[cfg(test)]
                        if let Some(observer) = &self.config.worker_observer {
                            observer.worker_started();
                        }
                        let outcome = catch_unwind(AssertUnwindSafe(|| {
                            run_transfer_worker(
                                self,
                                &worker_receiver,
                                &worker_sender,
                                &worker_cancellation,
                                fail_fast,
                            );
                        }));
                        if outcome.is_err() {
                            worker_cancellation.cancel();
                            let _ = worker_sender.send(LfsTransferWorkerMessage::Panicked);
                        }
                    });
                match worker {
                    Ok(worker) => workers.push(worker),
                    Err(error) => {
                        cancellation.cancel();
                        drop(job_sender);
                        drop(result_sender);
                        for worker in workers {
                            let _ = worker.join();
                        }
                        return Err(LfsTransferError::WorkerSpawn(error.kind()));
                    }
                }
            }
            drop(result_sender);
            let mut output = Vec::with_capacity(job_count);
            let mut jobs = jobs.into_iter();
            let mut worker_panicked = false;
            loop {
                let wave = jobs.by_ref().take(worker_count).collect::<Vec<_>>();
                if wave.is_empty() {
                    break;
                }
                let wave_len = wave.len();
                #[cfg(test)]
                if let Some(observer) = &self.config.worker_observer {
                    observer.wave_started();
                }
                for job in wave {
                    if job_sender.send(job).is_err() {
                        worker_panicked = true;
                        cancellation.cancel();
                        break;
                    }
                }
                if worker_panicked {
                    break;
                }
                for _ in 0..wave_len {
                    match result_receiver.recv() {
                        Ok(LfsTransferWorkerMessage::Completed(result)) => output.push(result),
                        Ok(LfsTransferWorkerMessage::Panicked) | Err(_) => {
                            worker_panicked = true;
                            cancellation.cancel();
                            break;
                        }
                    }
                }
                if worker_panicked || (fail_fast && cancellation.is_cancelled()) {
                    break;
                }
            }
            drop(job_sender);
            for worker in workers {
                if worker.join().is_err() {
                    return Err(LfsTransferError::WorkerPanicked);
                }
            }
            if worker_panicked {
                Err(LfsTransferError::WorkerPanicked)
            } else {
                Ok(output)
            }
        });
        let mut output = output?;
        output.sort_by_key(|result| result.index);
        Ok((
            output.into_iter().map(|result| result.result).collect(),
            cancellation.is_cancelled(),
        ))
    }

    fn execute_object(
        &self,
        operation: LfsBatchOperation,
        response: &LfsBatchResponseObject,
        action_issued_at: SystemTime,
        cancellation: &LfsTransferCancellation,
    ) -> ObjectResult {
        let object = LfsTransferObject {
            oid: response.object().oid(),
            size: response.object().size(),
        };
        if let Some(error) = response.error() {
            return ObjectResult::Failure(
                object,
                LfsTransferFailure::ObjectError { code: error.code() },
            );
        }
        if cancellation.is_cancelled() {
            return ObjectResult::Failure(object, LfsTransferFailure::Cancelled);
        }
        let result = match operation {
            LfsBatchOperation::Download => {
                self.download_object(object, response, action_issued_at, cancellation)
            }
            LfsBatchOperation::Upload => {
                self.upload_object(object, response, action_issued_at, cancellation)
            }
        };
        match result {
            Ok(outcome) => ObjectResult::Success(LfsTransferSuccess { object, outcome }),
            Err(error) => ObjectResult::Failure(object, error),
        }
    }

    fn download_object(
        &self,
        object: LfsTransferObject,
        response: &LfsBatchResponseObject,
        action_issued_at: SystemTime,
        cancellation: &LfsTransferCancellation,
    ) -> Result<LfsTransferSuccessKind, LfsTransferFailure> {
        let action = find_action(response, LfsBatchActionKind::Download)?;
        ensure_action_usable(
            action.action(),
            LfsBatchActionKind::Download,
            action_issued_at,
            SystemTime::now(),
        )?;
        let mut http_response = self
            .execute_action(
                "GET",
                action.action(),
                response.authenticated(),
                cancellation,
                None,
                &[],
            )
            .map_err(|error| {
                if cancellation.is_cancelled() {
                    LfsTransferFailure::Cancelled
                } else {
                    error
                }
            })?;
        if !is_success(http_response.response.status()) {
            let status = http_response.response.status();
            let scope = http_response.scope;
            let auth = http_response.auth;
            drop(http_response);
            return Err(LfsTransferFailure::HttpStatus {
                status,
                scope,
                auth,
            });
        }
        if cancellation.is_cancelled() {
            return Err(LfsTransferFailure::Cancelled);
        }
        let mut reader =
            LfsTransferCancellationReader::new(&mut http_response.response, cancellation.clone());
        let stored = self
            .store
            .ingest(object.oid().bytes(), object.size(), &mut reader)
            .map_err(|error| {
                if cancellation.is_cancelled() {
                    LfsTransferFailure::Cancelled
                } else {
                    LfsTransferFailure::Store(error)
                }
            })?;
        Ok(LfsTransferSuccessKind::Downloaded(stored.outcome()))
    }

    fn upload_object(
        &self,
        object: LfsTransferObject,
        response: &LfsBatchResponseObject,
        action_issued_at: SystemTime,
        cancellation: &LfsTransferCancellation,
    ) -> Result<LfsTransferSuccessKind, LfsTransferFailure> {
        let upload = find_optional_action(response, LfsBatchActionKind::Upload);
        let verify = find_optional_action(response, LfsBatchActionKind::Verify);
        let Some(upload) = upload else {
            return if verify.is_none() && response.actions().is_empty() {
                // The Batch API omits all actions when the server already has
                // an uploaded object. No local object read or PUT is needed.
                Ok(LfsTransferSuccessKind::Uploaded)
            } else {
                Err(LfsTransferFailure::MissingAction(
                    LfsBatchActionKind::Upload,
                ))
            };
        };
        ensure_action_usable(
            upload.action(),
            LfsBatchActionKind::Upload,
            action_issued_at,
            SystemTime::now(),
        )?;
        if cancellation.is_cancelled() {
            return Err(LfsTransferFailure::Cancelled);
        }
        let body = self.upload_body(object, cancellation.clone());
        let http_response = self
            .execute_action(
                "PUT",
                upload.action(),
                response.authenticated(),
                cancellation,
                Some(body),
                &[("Content-Type", LFS_MEDIA_CONTENT_TYPE.as_bytes())],
            )
            .map_err(|error| {
                if cancellation.is_cancelled() {
                    LfsTransferFailure::Cancelled
                } else {
                    error
                }
            })?;
        if !is_success(http_response.response.status()) {
            let status = http_response.response.status();
            let scope = http_response.scope;
            let auth = http_response.auth;
            drop(http_response);
            return Err(LfsTransferFailure::HttpStatus {
                status,
                scope,
                auth,
            });
        }
        if let Some(verify) = verify {
            self.verify_object(
                object,
                verify,
                response.authenticated(),
                action_issued_at,
                cancellation,
            )?;
        }
        Ok(LfsTransferSuccessKind::Uploaded)
    }

    fn verify_object(
        &self,
        object: LfsTransferObject,
        action: &LfsBatchActionEntry,
        authenticated: bool,
        action_issued_at: SystemTime,
        cancellation: &LfsTransferCancellation,
    ) -> Result<(), LfsTransferFailure> {
        if cancellation.is_cancelled() {
            return Err(LfsTransferFailure::Cancelled);
        }
        let request =
            LfsBatchObject::new(object.oid(), object.size()).map_err(LfsTransferFailure::Batch)?;
        let bytes = serialize_verify_request(&super::LfsVerifyRequest::new(request));
        ensure_action_usable(
            action.action(),
            LfsBatchActionKind::Verify,
            action_issued_at,
            SystemTime::now(),
        )?;
        let response = self
            .execute_action(
                "POST",
                action.action(),
                authenticated,
                cancellation,
                Some(bytes_body(bytes)),
                &[
                    ("Accept", LFS_BATCH_ACCEPT.as_bytes()),
                    ("Content-Type", LFS_BATCH_ACCEPT.as_bytes()),
                ],
            )
            .map_err(|error| {
                if cancellation.is_cancelled() {
                    LfsTransferFailure::Cancelled
                } else {
                    error
                }
            })?;
        if is_success(response.response.status()) {
            Ok(())
        } else {
            let status = response.response.status();
            let scope = response.scope;
            let auth = response.auth;
            drop(response);
            Err(LfsTransferFailure::HttpStatus {
                status,
                scope,
                auth,
            })
        }
    }

    fn upload_body(
        &self,
        object: LfsTransferObject,
        cancellation: LfsTransferCancellation,
    ) -> RequestBodyFactory {
        let store = Arc::clone(&self.store);
        RequestBodyFactory::verified_regular_file(
            object.size(),
            object.oid().bytes(),
            cancellation.clone(),
            move || {
                if cancellation.is_cancelled() {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "LFS transfer cancelled",
                    ));
                }
                store
                    .open_transport_file(object.oid().bytes(), object.size())
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
            },
        )
    }

    fn execute_action(
        &self,
        method: &str,
        action: &LfsBatchAction,
        authenticated: bool,
        cancellation: &LfsTransferCancellation,
        body: Option<RequestBodyFactory>,
        defaults: &[(&str, &[u8])],
    ) -> Result<LfsActionHttpResponse, LfsTransferFailure> {
        let url = HttpUrl::parse(action.href()).map_err(LfsTransferFailure::Transport)?;
        let scope = if action_uses_endpoint_auth(&self.endpoint_origin, &url) {
            LfsTransferHttpScope::SameOrigin
        } else {
            LfsTransferHttpScope::CrossOrigin
        };
        let action_url =
            parse_http_url(action.href()).map_err(|_| LfsTransferFailure::InvalidAction)?;
        let configured_authorization = self
            .config
            .http_policy
            .configured_authorization_provenance(&url)
            .is_some_and(|provenance| {
                scope == LfsTransferHttpScope::SameOrigin
                    || provenance == ConfiguredRequestHeaderProvenance::UrlScoped
            });
        let action_authorization = action_has_authorization(action.headers());

        let (helper_headers, auth) = if authenticated {
            (None, LfsTransferActionAuth::Preauthenticated)
        } else if action_authorization || action_url_has_token(&action_url) {
            (None, LfsTransferActionAuth::ActionHeader)
        } else if configured_authorization {
            (None, LfsTransferActionAuth::ConfiguredHeader)
        } else {
            let credentials = self
                .action_credentials
                .credentials(&action_url, self.endpoint.operation)
                .map_err(LfsTransferFailure::Authentication)?;
            let auth = if credentials.is_some() {
                LfsTransferActionAuth::CredentialHelper
            } else {
                LfsTransferActionAuth::None
            };
            (credentials, auth)
        };
        let headers = merged_request_headers(
            helper_headers.as_ref().map(LfsTransferAuthHeaders::headers),
            action.headers(),
            defaults,
        )
        .map_err(LfsTransferFailure::Transport)?;
        let method = method
            .parse()
            .map_err(|_| LfsTransferFailure::InvalidAction)?;
        let mut request = HttpRequest::new(method, url)
            .with_headers(headers)
            .with_cancellation(cancellation.clone());
        if let Some(body) = body {
            request = request.with_body(body);
        }
        let response = self
            .http
            .execute(&request)
            .map_err(LfsTransferFailure::Transport)?;
        if auth == LfsTransferActionAuth::CredentialHelper {
            if is_success(response.status()) {
                self.action_credentials
                    .approve(&action_url, self.endpoint.operation)
                    .map_err(LfsTransferFailure::Authentication)?;
            } else if matches!(response.status(), 401 | 403) {
                self.action_credentials
                    .reject(&action_url, self.endpoint.operation)
                    .map_err(LfsTransferFailure::Authentication)?;
            }
        }
        Ok(LfsActionHttpResponse {
            response,
            scope,
            auth,
        })
    }
}

struct LfsActionHttpResponse {
    response: StreamingResponse,
    scope: LfsTransferHttpScope,
    auth: LfsTransferActionAuth,
}

struct LfsEndpointBoundHttpPolicy {
    endpoint_origin: HttpOrigin,
    policy: Arc<LfsHttpPolicy>,
}

impl RequestPolicyResolver for LfsEndpointBoundHttpPolicy {
    fn resolve(&self, url: &HttpUrl) -> Result<ResolvedRequestPolicy, TransportError> {
        let mut resolved = self.policy.resolve(url)?;
        if url.origin() != &self.endpoint_origin
            && resolved.configured_headers.provenance()
                == ConfiguredRequestHeaderProvenance::Generic
        {
            resolved.configured_headers = ConfiguredRequestHeaders::none();
        }
        Ok(resolved)
    }
}

fn action_has_authorization(headers: &LfsBatchHeaders) -> bool {
    headers.entries().iter().any(|header| {
        header.name().eq_ignore_ascii_case("authorization") && !header.value().is_empty()
    })
}

fn action_url_has_token(url: &LfsHttpUrl) -> bool {
    let Some(query) = url
        .as_str()
        .split_once('?')
        .map(|(_, query)| query.split_once('#').map_or(query, |(query, _)| query))
    else {
        return false;
    };
    query.split('&').any(|parameter| {
        let (name, value) = parameter
            .split_once('=')
            .map_or((parameter, ""), |(name, value)| (name, value));
        decode_query_component(name).as_deref() == Some(b"token")
            && decode_query_component(value).is_some_and(|value| !value.is_empty())
    })
}

fn decode_query_component(raw: &str) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut bytes = raw.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => decoded.push(b' '),
            b'%' => {
                let high = query_hex(bytes.next()?)?;
                let low = query_hex(bytes.next()?)?;
                decoded.push((high << 4) | low);
            }
            byte => decoded.push(byte),
        }
    }
    Some(decoded)
}

fn query_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug)]
enum ObjectResult {
    Success(LfsTransferSuccess),
    Failure(LfsTransferObject, LfsTransferFailure),
}

impl ObjectResult {
    fn is_failure(&self) -> bool {
        matches!(self, Self::Failure(..))
    }
}

impl LfsTransferFailure {
    fn needs_refetch(&self) -> bool {
        matches!(self, Self::ActionExpired(_))
    }

    fn is_authentication_refreshable(&self) -> bool {
        matches!(
            self,
            Self::HttpStatus {
                status: 401 | 403,
                scope: LfsTransferHttpScope::SameOrigin | LfsTransferHttpScope::CrossOrigin,
                ..
            } | Self::ObjectError { code: 401 | 403 }
                | Self::ActionExpired(_)
        )
    }
}

type LfsTransferCancellation = RequestBodyCancellation;

struct LfsTransferJobReceiver {
    receiver: Mutex<mpsc::Receiver<LfsTransferWorkerJob>>,
}

impl LfsTransferJobReceiver {
    fn new(receiver: mpsc::Receiver<LfsTransferWorkerJob>) -> Self {
        Self {
            receiver: Mutex::new(receiver),
        }
    }

    fn recv(&self) -> Option<LfsTransferWorkerJob> {
        self.receiver
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .recv()
            .ok()
    }
}

fn collect_transfer_jobs(
    operation: LfsBatchOperation,
    response: &LfsBatchSuccessResponse,
    request_objects: &[LfsBatchObject],
    action_issued_at: SystemTime,
    requested_indices: &HashMap<LfsTransferObject, usize>,
) -> Vec<LfsTransferWorkerJob> {
    let mut by_oid = HashMap::with_capacity(response.objects().len());
    for object in response.objects() {
        by_oid.insert(object.object().oid(), object);
    }
    request_objects
        .iter()
        .filter_map(|object| by_oid.get(&object.oid()).copied())
        .map(|response| {
            let object = LfsTransferObject {
                oid: response.object().oid(),
                size: response.object().size(),
            };
            LfsTransferWorkerJob {
                index: *requested_indices
                    .get(&object)
                    .expect("Batch parser retained a requested LFS object"),
                operation,
                response: response.clone(),
                action_issued_at,
            }
        })
        .collect()
}

fn run_transfer_worker(
    client: &LfsTransferClient,
    receiver: &LfsTransferJobReceiver,
    sender: &mpsc::SyncSender<LfsTransferWorkerMessage>,
    cancellation: &LfsTransferCancellation,
    fail_fast: bool,
) {
    while let Some(job) = receiver.recv() {
        #[cfg(test)]
        let _activity = client
            .config
            .worker_observer
            .as_ref()
            .map(|observer| observer.job_started(job.index));
        let result = client.execute_object(
            job.operation,
            &job.response,
            job.action_issued_at,
            cancellation,
        );
        let failed = result.is_failure();
        if fail_fast && failed {
            cancellation.cancel();
        }
        if sender
            .send(LfsTransferWorkerMessage::Completed(
                LfsTransferWorkerResult {
                    index: job.index,
                    result,
                },
            ))
            .is_err()
        {
            cancellation.cancel();
            return;
        }
    }
}

struct LfsTransferCancellationReader<R> {
    inner: R,
    cancellation: LfsTransferCancellation,
}

impl<R> LfsTransferCancellationReader<R> {
    fn new(inner: R, cancellation: LfsTransferCancellation) -> Self {
        Self {
            inner,
            cancellation,
        }
    }
}

impl<R: Read> Read for LfsTransferCancellationReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancellation.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "LFS transfer cancelled",
            ));
        }
        self.inner.read(buffer)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum LfsTransferError {
    EmptyRequest,
    OperationMismatch,
    RequestTooLarge,
    SplitLimitExceeded,
    IncompleteReport,
    WorkerSpawn(io::ErrorKind),
    WorkerPanicked,
    Transport(TransportError),
    Batch(LfsBatchError),
    BatchHttp(LfsBatchErrorResponse),
}

impl fmt::Display for LfsTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequest => formatter.write_str("LFS transfer request has no objects"),
            Self::OperationMismatch => {
                formatter.write_str("LFS endpoint operation does not match transfer")
            }
            Self::RequestTooLarge => formatter.write_str("LFS Batch request is too large to split"),
            Self::SplitLimitExceeded => formatter.write_str("LFS Batch split limit exceeded"),
            Self::IncompleteReport => {
                formatter.write_str("LFS transfer returned an incomplete object report")
            }
            Self::WorkerSpawn(kind) => {
                write!(formatter, "LFS transfer worker could not start ({kind:?})")
            }
            Self::WorkerPanicked => formatter.write_str("LFS transfer worker failed"),
            Self::Transport(error) => error.fmt(formatter),
            Self::Batch(error) => error.fmt(formatter),
            Self::BatchHttp(error) => write!(
                formatter,
                "LFS Batch request returned HTTP status {}",
                error.status()
            ),
        }
    }
}

impl fmt::Debug for LfsTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequest => formatter.write_str("EmptyRequest"),
            Self::OperationMismatch => formatter.write_str("OperationMismatch"),
            Self::RequestTooLarge => formatter.write_str("RequestTooLarge"),
            Self::SplitLimitExceeded => formatter.write_str("SplitLimitExceeded"),
            Self::IncompleteReport => formatter.write_str("IncompleteReport"),
            Self::WorkerSpawn(kind) => formatter.debug_tuple("WorkerSpawn").field(kind).finish(),
            Self::WorkerPanicked => formatter.write_str("WorkerPanicked"),
            Self::Transport(error) => formatter.debug_tuple("Transport").field(error).finish(),
            Self::Batch(error) => formatter.debug_tuple("Batch").field(error).finish(),
            Self::BatchHttp(error) => formatter
                .debug_struct("BatchHttp")
                .field("status", &error.status())
                .finish(),
        }
    }
}

impl std::error::Error for LfsTransferError {}

fn deduplicate_objects(
    objects: &[LfsTransferObject],
) -> Result<(Vec<LfsTransferObject>, usize), LfsTransferError> {
    let mut seen = HashMap::with_capacity(objects.len());
    let mut unique = Vec::with_capacity(objects.len());
    let mut deduplicated = 0_usize;
    for object in objects {
        let key = object.oid();
        if let Some(previous_size) = seen.insert(key, object.size()) {
            if previous_size != object.size() {
                return Err(LfsTransferError::Batch(LfsBatchError::InvalidSize));
            }
            deduplicated += 1;
        } else {
            unique.push(*object);
        }
    }
    Ok((unique, deduplicated))
}

fn effective_transfer_workers(concurrency: LfsTransferConcurrency, object_count: usize) -> usize {
    concurrency.get().min(object_count)
}

fn find_action<'a>(
    object: &'a LfsBatchResponseObject,
    kind: LfsBatchActionKind,
) -> Result<&'a LfsBatchActionEntry, LfsTransferFailure> {
    find_optional_action(object, kind).ok_or(LfsTransferFailure::MissingAction(kind))
}

fn find_optional_action<'a>(
    object: &'a LfsBatchResponseObject,
    kind: LfsBatchActionKind,
) -> Option<&'a LfsBatchActionEntry> {
    object.actions().iter().find(|entry| entry.kind() == kind)
}

fn merged_request_headers(
    auth_headers: Option<&LfsBatchHeaders>,
    action_headers: &LfsBatchHeaders,
    defaults: &[(&str, &[u8])],
) -> Result<RequestHeaders, TransportError> {
    let mut result = RequestHeaders::empty();
    let mut names = Vec::<&str>::with_capacity(
        auth_headers.map_or(0, |headers| headers.entries().len())
            + action_headers.entries().len()
            + defaults.len(),
    );
    for header in auth_headers
        .into_iter()
        .flat_map(LfsBatchHeaders::entries)
        .chain(action_headers.entries())
    {
        if names
            .iter()
            .any(|name: &&str| name.eq_ignore_ascii_case(header.name()))
        {
            // An auth header and a server-signed action header with the same
            // name must never silently overwrite one another.
            return Err(TransportError::InvalidHeader);
        }
        let request_header = if header.is_secret() {
            RequestHeader::new_secret(header.name(), header.value().as_bytes().to_vec())?
        } else {
            RequestHeader::new(header.name(), header.value().as_bytes())?
        };
        result.push(request_header)?;
        names.push(header.name());
    }
    for (name, value) in defaults {
        if names
            .iter()
            .any(|current| current.eq_ignore_ascii_case(name))
        {
            continue;
        }
        result.push(RequestHeader::new(name, value)?)?;
        names.push(*name);
    }
    Ok(result)
}

fn action_uses_endpoint_auth(endpoint_origin: &HttpOrigin, action_url: &HttpUrl) -> bool {
    endpoint_origin == action_url.origin()
}

fn ensure_action_usable(
    action: &LfsBatchAction,
    kind: LfsBatchActionKind,
    issued_at: SystemTime,
    now: SystemTime,
) -> Result<(), LfsTransferFailure> {
    let deadline = now
        .checked_add(ACTION_EXPIRY_SKEW)
        .ok_or(LfsTransferFailure::InvalidAction)?;
    if let Some(seconds) = action.expires_in() {
        let expiration = if seconds >= 0 {
            issued_at.checked_add(Duration::from_secs(seconds as u64))
        } else {
            issued_at.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
        }
        .ok_or(LfsTransferFailure::InvalidAction)?;
        if expiration <= deadline {
            return Err(LfsTransferFailure::ActionExpired(kind));
        }
    }
    if let Some(expires_at) = action.expires_at() {
        let expiration = DateTime::parse_from_rfc3339(expires_at)
            .ok()
            .and_then(system_time_from_datetime)
            .ok_or(LfsTransferFailure::InvalidAction)?;
        if expiration <= deadline {
            return Err(LfsTransferFailure::ActionExpired(kind));
        }
    }
    Ok(())
}

fn system_time_from_datetime(datetime: DateTime<chrono::FixedOffset>) -> Option<SystemTime> {
    let seconds = datetime.timestamp();
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
    }
}

pub(crate) fn batch_url(endpoint: &LfsHttpUrl) -> Result<HttpUrl, LfsTransferError> {
    let (base, query) = endpoint
        .as_str()
        .split_once('?')
        .map_or((endpoint.as_str(), None), |(base, query)| {
            (base, Some(query))
        });
    let mut raw = String::with_capacity(endpoint.as_str().len() + "/objects/batch".len());
    raw.push_str(base.trim_end_matches('/'));
    raw.push_str("/objects/batch");
    if let Some(query) = query {
        raw.push('?');
        raw.push_str(query);
    }
    HttpUrl::parse(&raw).map_err(LfsTransferError::Transport)
}

fn bytes_body(bytes: Vec<u8>) -> RequestBodyFactory {
    RequestBodyFactory::bytes(bytes)
}

fn is_success(status: u16) -> bool {
    (200..=299).contains(&status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;
    use std::thread::{self, JoinHandle};

    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    use crate::runtime::{ConfigEntry, ConfigScope, LfsHttpEnvironmentSnapshot};

    struct NoActionCredentials;

    impl LfsActionCredentialProvider for NoActionCredentials {
        fn credentials(
            &self,
            _action: &LfsHttpUrl,
            _operation: LfsOperation,
        ) -> Result<Option<LfsTransferAuthHeaders>, LfsAuthError> {
            Ok(None)
        }

        fn approve(
            &self,
            _action: &LfsHttpUrl,
            _operation: LfsOperation,
        ) -> Result<(), LfsAuthError> {
            Ok(())
        }

        fn reject(
            &self,
            _action: &LfsHttpUrl,
            _operation: LfsOperation,
        ) -> Result<(), LfsAuthError> {
            Ok(())
        }
    }

    fn no_action_credentials() -> Arc<dyn LfsActionCredentialProvider> {
        Arc::new(NoActionCredentials)
    }

    #[derive(Default)]
    struct ActionCredentialState {
        fills: Vec<String>,
        approves: Vec<String>,
        rejects: Vec<String>,
    }

    struct RecordingActionCredentials {
        state: Arc<Mutex<ActionCredentialState>>,
    }

    impl LfsActionCredentialProvider for RecordingActionCredentials {
        fn credentials(
            &self,
            action: &LfsHttpUrl,
            _operation: LfsOperation,
        ) -> Result<Option<LfsTransferAuthHeaders>, LfsAuthError> {
            self.state
                .lock()
                .expect("action credential state")
                .fills
                .push(action.as_str().to_owned());
            LfsTransferAuthHeaders::from_pairs(vec![(
                "Authorization".to_owned(),
                b"Basic action-helper".to_vec(),
            )])
            .map(Some)
            .map_err(|_| LfsAuthError::InvalidHeader)
        }

        fn approve(
            &self,
            action: &LfsHttpUrl,
            _operation: LfsOperation,
        ) -> Result<(), LfsAuthError> {
            self.state
                .lock()
                .expect("action credential state")
                .approves
                .push(action.as_str().to_owned());
            Ok(())
        }

        fn reject(
            &self,
            action: &LfsHttpUrl,
            _operation: LfsOperation,
        ) -> Result<(), LfsAuthError> {
            self.state
                .lock()
                .expect("action credential state")
                .rejects
                .push(action.as_str().to_owned());
            Ok(())
        }
    }

    fn recording_action_credentials() -> (
        Arc<dyn LfsActionCredentialProvider>,
        Arc<Mutex<ActionCredentialState>>,
    ) {
        let state = Arc::new(Mutex::new(ActionCredentialState::default()));
        (
            Arc::new(RecordingActionCredentials {
                state: Arc::clone(&state),
            }),
            state,
        )
    }

    fn oid() -> LfsOid {
        LfsOid::from_hex("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
            .expect("test oid")
    }

    fn object(size: u64) -> LfsTransferObject {
        LfsTransferObject::new(oid(), size).expect("test object")
    }

    #[test]
    fn concurrency_is_strictly_bounded() {
        assert_eq!(LfsTransferConcurrency::default().get(), 8);
        assert_eq!(LfsTransferConfig::default().concurrency().get(), 8);
        assert!(LfsTransferConcurrency::new(1).is_ok());
        assert!(LfsTransferConcurrency::new(8).is_ok());
        assert_eq!(
            LfsTransferConcurrency::new(0),
            Err(LfsTransferConfigError::InvalidConcurrency)
        );
        assert_eq!(
            LfsTransferConcurrency::new(9),
            Err(LfsTransferConfigError::InvalidConcurrency)
        );
        assert_eq!(
            effective_transfer_workers(LfsTransferConcurrency::default(), 1),
            1
        );
        assert_eq!(
            effective_transfer_workers(LfsTransferConcurrency::default(), 8),
            8
        );
        assert_eq!(LFS_TRANSFER_WORKER_STACK_BYTES, 256 * 1024);
    }

    #[test]
    fn fixed_worker_pool_is_persistent_bounded_and_deterministically_ordered() {
        for concurrency in [1_usize, 2, 8] {
            run_fixed_worker_pool_case(concurrency, 64);
        }
    }

    #[test]
    fn eight_large_downloads_preserve_every_hash_and_oid() {
        run_fixed_worker_pool_case(8, 16 * 1024 * 1024);
    }

    #[test]
    fn worker_panic_is_typed_and_all_workers_are_joined() {
        run_worker_panic_case(1);
        run_worker_panic_case(8);
    }

    fn run_worker_panic_case(job_count: usize) {
        let payloads = (0..job_count)
            .map(|index| format!("panic-target-{index}").into_bytes())
            .collect::<Vec<_>>();
        let objects = payloads
            .iter()
            .map(|payload| {
                LfsTransferObject::new(oid_for_bytes(payload), payload.len() as u64)
                    .expect("object")
            })
            .collect::<Vec<_>>();
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let endpoint = format!("http://{address}/info/lfs");
        let server_objects = objects.clone();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("batch connection");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/info/lfs/objects/batch");
            let entries = server_objects
                .iter()
                .enumerate()
                .map(|(index, object)| {
                    format!(
                        "{{\"oid\":\"{}\",\"size\":{},\"actions\":{{\"download\":{{\"href\":\"http://{address}/panic/{index}\"}}}}}}",
                        object.oid().hex(),
                        object.size()
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let response = format!("{{\"objects\":[{entries}]}}");
            write_http_response(&mut stream, 200, response.as_bytes());
        });
        let observer = Arc::new(LfsTransferWorkerTestObserver::panic_on(0));
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let client = fixture_client_with_concurrency(
            LfsOperation::Fetch,
            &endpoint,
            store,
            8,
            Arc::clone(&observer),
        );
        let error = client.download(&objects, None).expect_err("worker panic");
        assert!(matches!(error, LfsTransferError::WorkerPanicked));
        assert_eq!(observer.spawned.load(Ordering::SeqCst), job_count);
        assert_eq!(observer.active.load(Ordering::SeqCst), 0);
        assert_eq!(
            observer.thread_names.lock().expect("thread names").len(),
            job_count
        );
        server.join().expect("fixture");
    }

    fn run_fixed_worker_pool_case(concurrency: usize, payload_bytes: usize) {
        const OBJECT_COUNT: usize = 8;

        let payloads = (0..OBJECT_COUNT)
            .map(|index| Arc::new(vec![index as u8; payload_bytes]))
            .collect::<Vec<_>>();
        let objects = payloads
            .iter()
            .map(|payload| {
                LfsTransferObject::new(oid_for_bytes(payload.as_slice()), payload.len() as u64)
                    .expect("object")
            })
            .collect::<Vec<_>>();
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let endpoint = format!("http://{address}/info/lfs");
        let server_objects = objects.clone();
        let server_payloads = payloads.clone();
        let server = thread::spawn(move || {
            let (mut batch_stream, _) = listener.accept().expect("batch connection");
            let request = read_captured_http_request(&mut batch_stream);
            assert_eq!(request.path, "/info/lfs/objects/batch");
            let entries = server_objects
                .iter()
                .enumerate()
                .map(|(index, object)| {
                    format!(
                        "{{\"oid\":\"{}\",\"size\":{},\"actions\":{{\"download\":{{\"href\":\"http://{address}/download/{index}\"}}}}}}",
                        object.oid().hex(),
                        object.size()
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let response = format!("{{\"objects\":[{entries}]}}");
            write_http_response(&mut batch_stream, 200, response.as_bytes());

            let payloads = Arc::new(server_payloads);
            let mut handlers = Vec::with_capacity(OBJECT_COUNT);
            for _ in 0..OBJECT_COUNT {
                let (mut stream, _) = listener.accept().expect("download connection");
                let payloads = Arc::clone(&payloads);
                handlers.push(thread::spawn(move || {
                    let request = read_captured_http_request(&mut stream);
                    let index = request
                        .path
                        .strip_prefix("/download/")
                        .and_then(|value| value.parse::<usize>().ok())
                        .expect("download index");
                    thread::sleep(Duration::from_millis(
                        (OBJECT_COUNT.saturating_sub(index)) as u64,
                    ));
                    write_http_response(&mut stream, 200, payloads[index].as_slice());
                }));
            }
            for handler in handlers {
                handler.join().expect("download handler");
            }
        });
        let observer = Arc::new(LfsTransferWorkerTestObserver::default());
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let client = fixture_client_with_concurrency(
            LfsOperation::Fetch,
            &endpoint,
            store,
            concurrency,
            Arc::clone(&observer),
        );
        let report = client.download(&objects, None).expect("download report");
        assert!(report.is_complete());
        assert_eq!(
            report
                .succeeded()
                .iter()
                .map(LfsTransferSuccess::object)
                .collect::<Vec<_>>(),
            objects
        );
        assert_eq!(observer.spawned.load(Ordering::SeqCst), concurrency);
        assert_eq!(
            observer.waves.load(Ordering::SeqCst),
            OBJECT_COUNT.div_ceil(concurrency)
        );
        assert_eq!(observer.max_active.load(Ordering::SeqCst), concurrency);
        assert_eq!(observer.active.load(Ordering::SeqCst), 0);
        let mut started = observer.started.lock().expect("starts").clone();
        started.sort_unstable();
        assert_eq!(started, (0..OBJECT_COUNT).collect::<Vec<_>>());
        assert_eq!(
            observer.thread_names.lock().expect("thread names").len(),
            concurrency
        );
        server.join().expect("fixture");
    }

    fn oid_for_bytes(bytes: &[u8]) -> LfsOid {
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        let encoded = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        LfsOid::from_hex(&encoded).expect("SHA-256 OID")
    }

    #[test]
    fn duplicate_oids_are_deduplicated_without_media_buffering() {
        let (objects, duplicates) = deduplicate_objects(&[object(5), object(5)]).expect("dedup");
        assert_eq!(objects, vec![object(5)]);
        assert_eq!(duplicates, 1);
        assert!(deduplicate_objects(&[object(5), object(6)]).is_err());
    }

    #[test]
    fn duplicate_oid_mismatch_is_not_silently_accepted() {
        let error = deduplicate_objects(&[object(1), object(2)]).expect_err("size mismatch");
        assert_eq!(error, LfsTransferError::Batch(LfsBatchError::InvalidSize));
    }

    #[test]
    fn bounded_batch_split_is_deterministic() {
        let batch = LfsBatchRequest::new(
            LfsBatchOperation::Download,
            vec![
                LfsBatchObject::new(oid(), 5).expect("object"),
                LfsBatchObject::new(
                    LfsOid::from_hex(
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    )
                    .expect("oid"),
                    5,
                )
                .expect("object"),
            ],
        )
        .expect("batch");
        let parts = split_batch_request(&batch, 1, DEFAULT_BATCH_MAX_BYTES).expect("split");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].objects()[0].oid(), oid());
    }

    #[test]
    fn batch_path_is_appended_before_an_endpoint_query() {
        let endpoint =
            parse_http_url("https://example.test/repo.git/info/lfs/?signed=1").expect("endpoint");
        let batch = batch_url(&endpoint).expect("batch endpoint");
        assert_eq!(
            batch.as_str(),
            "https://example.test/repo.git/info/lfs/objects/batch?signed=1"
        );
    }

    #[test]
    fn fixed_json_bodies_are_reopenable_for_retries() {
        let body = bytes_body(vec![1, 2, 3]);
        assert_eq!(format!("{body:?}"), "RequestBodyFactory(<reopenable>)");
    }

    #[test]
    fn transfer_debug_does_not_include_action_or_batch_header_values() {
        let auth = LfsTransferAuthHeaders::from_pairs(vec![(
            "Authorization".to_owned(),
            b"Bearer secret".to_vec(),
        )])
        .expect("headers");
        let debug = format!("{auth:?}");
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn auth_header_values_remain_secret_owned_and_redacted() {
        let auth = LfsTransferAuthHeaders::from_pairs(vec![(
            "Authorization".to_owned(),
            b"Basic sensitive-value".to_vec(),
        )])
        .expect("secret auth header");
        let header = &auth.headers().entries()[0];
        assert!(header.is_secret());
        assert_eq!(header.value(), "Basic sensitive-value");
        assert!(!format!("{auth:?}").contains("sensitive-value"));
        assert!(!format!("{header:?}").contains("sensitive-value"));
    }

    #[test]
    fn action_headers_keep_server_values_and_add_safe_defaults() {
        let auth =
            LfsBatchHeaders::new(vec![
                LfsBatchHeader::new("Authorization", "Bearer secret").expect("auth")
            ])
            .expect("auth headers");
        let action = LfsBatchHeaders::new(vec![
            LfsBatchHeader::new("X-Signed", "signature").expect("signed"),
            LfsBatchHeader::new("Content-Type", "application/custom").expect("content type"),
        ])
        .expect("action headers");
        let headers = merged_request_headers(
            Some(&auth),
            &action,
            &[
                ("Accept", LFS_BATCH_ACCEPT.as_bytes()),
                ("Content-Type", LFS_MEDIA_CONTENT_TYPE.as_bytes()),
            ],
        )
        .expect("merged headers");
        let names = headers.iter().map(RequestHeader::name).collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["authorization", "content-type", "x-signed", "accept"]
        );
        let upload_defaults = merged_request_headers(
            Some(&auth),
            &LfsBatchHeaders::empty(),
            &[("Content-Type", LFS_MEDIA_CONTENT_TYPE.as_bytes())],
        )
        .expect("upload defaults");
        assert_eq!(
            upload_defaults
                .iter()
                .map(RequestHeader::name)
                .collect::<Vec<_>>(),
            vec!["authorization", "content-type"]
        );
        assert!(merged_request_headers(
            Some(&auth),
            &LfsBatchHeaders::new(vec![
                LfsBatchHeader::new("authorization", "other").expect("duplicate"),
            ])
            .expect("action headers"),
            &[],
        )
        .is_err());
    }

    #[test]
    fn canonical_origins_cover_host_case_default_ports_and_ipv6() {
        let endpoint = HttpUrl::parse("http://EXAMPLE.test:80/info/lfs").expect("endpoint");
        let action = HttpUrl::parse("http://example.TEST/object").expect("action");
        assert!(action_uses_endpoint_auth(endpoint.origin(), &action));

        let endpoint = HttpUrl::parse("https://[2001:db8::1]/info/lfs").expect("IPv6 endpoint");
        let action =
            HttpUrl::parse("https://[2001:0db8:0:0:0:0:0:1]:443/object").expect("IPv6 action");
        assert!(action_uses_endpoint_auth(endpoint.origin(), &action));

        let other_port = HttpUrl::parse("https://[2001:db8::1]:444/object").expect("other port");
        assert!(!action_uses_endpoint_auth(endpoint.origin(), &other_port));
        let other_scheme = HttpUrl::parse("http://[2001:db8::1]:443/object").expect("other scheme");
        assert!(!action_uses_endpoint_auth(endpoint.origin(), &other_scheme));
    }

    #[test]
    fn configured_authorization_is_sent_to_the_matching_batch_url() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("endpoint listener");
        let address = listener.local_addr().expect("endpoint address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("endpoint request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/info/lfs/objects/batch");
            assert_eq!(
                request.header_values("authorization"),
                vec!["Bearer configured"]
            );
            write_http_status_response(&mut stream, 401, br#"{"message":"unauthorized"}"#);
        });
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let endpoint = format!("http://{address}/info/lfs");
        let policy = LfsHttpPolicy::from_entries(
            &[http_policy_entry(
                &endpoint,
                "Authorization: Bearer configured",
            )],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("HTTP policy");
        let client = fixture_client_with_auth_and_policy(
            LfsOperation::Fetch,
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            Arc::new(policy),
        );

        assert!(matches!(
            client.download(&[object(5)], None),
            Err(LfsTransferError::BatchHttp(error)) if error.status() == 401
        ));
        server.join().expect("endpoint server");
    }

    #[test]
    fn cross_origin_action_drops_generic_secrets_but_keeps_scoped_and_signed_headers() {
        let endpoint_listener = TcpListener::bind("127.0.0.1:0").expect("endpoint listener");
        let endpoint_address = endpoint_listener.local_addr().expect("endpoint address");
        let action_listener = TcpListener::bind("127.0.0.1:0").expect("action listener");
        let action_address = action_listener.local_addr().expect("action address");
        let object_oid = oid().hex().to_owned();
        let endpoint_server = thread::spawn(move || {
            let (mut stream, _) = endpoint_listener.accept().expect("endpoint request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/info/lfs/objects/batch");
            assert_eq!(
                request.header_values("authorization"),
                vec!["Bearer generic-endpoint"]
            );
            assert_eq!(request.header_values("cookie"), vec!["session=generic"]);
            assert_eq!(
                request.header_values("x-generic-secret"),
                vec!["endpoint-only"]
            );
            let response = format!(
                "{{\"objects\":[{{\"oid\":\"{object_oid}\",\"size\":5,\"actions\":{{\"download\":{{\"href\":\"http://{action_address}/download\",\"header\":{{\"Authorization\":\"Bearer action-signed\"}}}}}}}}]}}"
            );
            write_http_response(&mut stream, 200, response.as_bytes());
        });
        let action_server = thread::spawn(move || {
            let (mut stream, _) = action_listener.accept().expect("action request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/download");
            assert_eq!(
                request.header_values("authorization"),
                vec!["Bearer action-signed"]
            );
            assert!(request.header_values("cookie").is_empty());
            assert!(request.header_values("x-generic-secret").is_empty());
            assert_eq!(request.header_values("x-configured-target"), vec!["action"]);
            write_http_response(&mut stream, 200, b"hello");
        });
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let endpoint = format!("http://{endpoint_address}/info/lfs");
        let policy = LfsHttpPolicy::from_entries(
            &[
                http_policy_entry("", "Authorization: Bearer generic-endpoint"),
                http_policy_entry("", "Cookie: session=generic"),
                http_policy_entry("", "X-Generic-Secret: endpoint-only"),
                http_policy_entry(
                    &format!("http://{action_address}/download"),
                    "X-Configured-Target: action",
                ),
            ],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("HTTP policy");
        let client = fixture_client_with_auth_and_policy(
            LfsOperation::Fetch,
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            Arc::new(policy),
        );

        let report = client.download(&[object(5)], None).expect("download");
        assert!(report.is_complete());
        endpoint_server.join().expect("endpoint server");
        action_server.join().expect("action server");
    }

    #[test]
    fn cross_origin_generic_authorization_does_not_suppress_action_helper() {
        let endpoint_listener = TcpListener::bind("127.0.0.1:0").expect("endpoint listener");
        let endpoint_address = endpoint_listener.local_addr().expect("endpoint address");
        let action_listener = TcpListener::bind("127.0.0.1:0").expect("action listener");
        let action_address = action_listener.local_addr().expect("action address");
        let object_oid = oid().hex().to_owned();
        let endpoint_server = thread::spawn(move || {
            let (mut stream, _) = endpoint_listener.accept().expect("batch request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(
                request.header_values("authorization"),
                vec!["Bearer generic-endpoint"]
            );
            let response = format!(
                "{{\"objects\":[{{\"oid\":\"{object_oid}\",\"size\":5,\"actions\":{{\"download\":{{\"href\":\"http://{action_address}/download\"}}}}}}]}}"
            );
            write_http_response(&mut stream, 200, response.as_bytes());
        });
        let action_server = thread::spawn(move || {
            let (mut stream, _) = action_listener.accept().expect("action request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(
                request.header_values("authorization"),
                vec!["Basic action-helper"]
            );
            write_http_response(&mut stream, 200, b"hello");
        });
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let endpoint = format!("http://{endpoint_address}/info/lfs");
        let policy = LfsHttpPolicy::from_entries(
            &[http_policy_entry(
                "",
                "Authorization: Bearer generic-endpoint",
            )],
            LfsHttpEnvironmentSnapshot::default(),
        )
        .expect("HTTP policy");
        let (credentials, state) = recording_action_credentials();
        let client = fixture_client_with_auth_policy_and_action_credentials(
            LfsOperation::Fetch,
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            Arc::new(policy),
            credentials,
        );
        assert!(client
            .download(&[object(5)], None)
            .expect("download")
            .is_complete());
        let state = state.lock().expect("action credential state");
        assert_eq!(state.fills.len(), 1);
        assert_eq!(state.approves.len(), 1);
        assert!(state.rejects.is_empty());
        endpoint_server.join().expect("endpoint server");
        action_server.join().expect("action server");
    }

    #[test]
    fn batch_authenticated_flag_controls_action_credential_helper() {
        for authenticated in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
            let address = listener.local_addr().expect("fixture address");
            let object_oid = oid().hex().to_owned();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("batch request");
                let request = read_captured_http_request(&mut stream);
                assert_eq!(request.path, "/info/lfs/objects/batch");
                let response = format!(
                    "{{\"objects\":[{{\"oid\":\"{object_oid}\",\"size\":5,\"authenticated\":{authenticated},\"actions\":{{\"download\":{{\"href\":\"http://{address}/download\",\"header\":{{\"X-Signed\":\"action-value\"}}}}}}}}]}}"
                );
                write_http_response(&mut stream, 200, response.as_bytes());

                let (mut stream, _) = listener.accept().expect("action request");
                let request = read_captured_http_request(&mut stream);
                assert_eq!(request.path, "/download");
                assert_eq!(request.header_values("x-signed"), vec!["action-value"]);
                if authenticated {
                    assert!(request.header_values("authorization").is_empty());
                } else {
                    assert_eq!(
                        request.header_values("authorization"),
                        vec!["Basic action-helper"]
                    );
                }
                write_http_response(&mut stream, 200, b"hello");
            });
            let root = tempdir().expect("store root");
            let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
            let endpoint = format!("http://{address}/info/lfs");
            let (credentials, state) = recording_action_credentials();
            let client = fixture_client_with_auth_policy_and_action_credentials(
                LfsOperation::Fetch,
                &endpoint,
                store,
                LfsTransferAuthHeaders::empty(),
                Arc::new(LfsHttpPolicy::default()),
                credentials,
            );
            let report = client.download(&[object(5)], None).expect("download");
            assert!(report.is_complete());
            let state = state.lock().expect("action credential state");
            assert_eq!(state.fills.len(), usize::from(!authenticated));
            assert_eq!(state.approves.len(), usize::from(!authenticated));
            assert!(state.rejects.is_empty());
            server.join().expect("fixture server");
        }
    }

    #[test]
    fn signed_or_url_scoped_authorization_suppresses_action_helper() {
        for configured in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
            let address = listener.local_addr().expect("fixture address");
            let object_oid = oid().hex().to_owned();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("batch request");
                let request = read_captured_http_request(&mut stream);
                assert_eq!(request.path, "/info/lfs/objects/batch");
                let action_header = if configured {
                    String::new()
                } else {
                    ",\"header\":{\"Authorization\":\"Bearer action-signed\"}".to_owned()
                };
                let response = [
                    "{\"objects\":[{\"oid\":\"",
                    &object_oid,
                    "\",\"size\":5,\"actions\":{\"download\":{\"href\":\"http://",
                    &address.to_string(),
                    "/download\"",
                    &action_header,
                    "}}}]}",
                ]
                .concat();
                write_http_response(&mut stream, 200, response.as_bytes());

                let (mut stream, _) = listener.accept().expect("action request");
                let request = read_captured_http_request(&mut stream);
                let expected = if configured {
                    "Bearer configured-action"
                } else {
                    "Bearer action-signed"
                };
                assert_eq!(request.header_values("authorization"), vec![expected]);
                write_http_response(&mut stream, 200, b"hello");
            });
            let root = tempdir().expect("store root");
            let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
            let endpoint = format!("http://{address}/info/lfs");
            let entries = configured.then(|| {
                http_policy_entry(
                    &format!("http://{address}/download"),
                    "Authorization: Bearer configured-action",
                )
            });
            let policy = LfsHttpPolicy::from_entries(
                entries.as_slice(),
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("HTTP policy");
            let (credentials, state) = recording_action_credentials();
            let client = fixture_client_with_auth_policy_and_action_credentials(
                LfsOperation::Fetch,
                &endpoint,
                store,
                LfsTransferAuthHeaders::empty(),
                Arc::new(policy),
                credentials,
            );
            assert!(client
                .download(&[object(5)], None)
                .expect("download")
                .is_complete());
            let state = state.lock().expect("action credential state");
            assert!(state.fills.is_empty());
            assert!(state.approves.is_empty());
            assert!(state.rejects.is_empty());
            server.join().expect("fixture server");
        }
    }

    #[test]
    fn rejected_action_credentials_are_rejected_without_an_internal_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let object_oid = oid().hex().to_owned();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("batch request");
            let response = [
                "{\"objects\":[{\"oid\":\"",
                &object_oid,
                "\",\"size\":5,\"actions\":{\"download\":{\"href\":\"http://",
                &address.to_string(),
                "/download\"}}}]}",
            ]
            .concat();
            write_http_response(&mut stream, 200, response.as_bytes());
            let (mut stream, _) = listener.accept().expect("action request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(
                request.header_values("authorization"),
                vec!["Basic action-helper"]
            );
            write_http_response(&mut stream, 401, b"");
        });
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let endpoint = format!("http://{address}/info/lfs");
        let (credentials, state) = recording_action_credentials();
        let client = fixture_client_with_auth_policy_and_action_credentials(
            LfsOperation::Fetch,
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            Arc::new(LfsHttpPolicy::default()),
            credentials,
        );
        let report = client
            .download(&[object(5)], None)
            .expect("typed action failure");
        assert_eq!(report.failed().len(), 1);
        assert!(matches!(
            report.failed()[0].1,
            LfsTransferFailure::HttpStatus {
                status: 401,
                auth: LfsTransferActionAuth::CredentialHelper,
                ..
            }
        ));
        let state = state.lock().expect("action credential state");
        assert_eq!(state.fills.len(), 1);
        assert!(state.approves.is_empty());
        assert_eq!(state.rejects.len(), 1);
        server.join().expect("fixture server");
    }

    #[test]
    fn token_query_suppression_matches_git_lfs_nonempty_decoded_value() {
        for (url, expected) in [
            ("https://example.test/object?token=signed", true),
            ("https://example.test/object?token=", false),
            ("https://example.test/object?to%6ben=signed", true),
            ("https://example.test/object?Token=signed", false),
            ("https://example.test/object?other=1", false),
        ] {
            assert_eq!(
                action_url_has_token(&parse_http_url(url).expect("URL")),
                expected,
                "{url}"
            );
        }
    }

    #[test]
    fn batch_action_header_is_secret_across_an_allowlisted_redirect() {
        let target_listener = TcpListener::bind("127.0.0.1:0").expect("target listener");
        let target_address = target_listener.local_addr().expect("target address");
        let target_server = thread::spawn(move || {
            let (mut stream, _) = target_listener.accept().expect("target request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/object");
            assert!(!request
                .header_values("accept")
                .contains(&"signed-action-value"));
            write_http_response(&mut stream, 200, b"hello");
        });

        let endpoint_listener = TcpListener::bind("127.0.0.1:0").expect("endpoint listener");
        let endpoint_address = endpoint_listener.local_addr().expect("endpoint address");
        let object_oid = oid().hex().to_owned();
        let endpoint_server = thread::spawn(move || {
            let (mut stream, _) = endpoint_listener.accept().expect("batch request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/info/lfs/objects/batch");
            let response = format!(
                "{{\"objects\":[{{\"oid\":\"{object_oid}\",\"size\":5,\"actions\":{{\"download\":{{\"href\":\"http://{endpoint_address}/redirect\",\"header\":{{\"Accept\":\"signed-action-value\"}}}}}}}}]}}"
            );
            write_http_response(&mut stream, 200, response.as_bytes());

            let (mut stream, _) = endpoint_listener.accept().expect("redirect request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/redirect");
            assert_eq!(request.header_values("accept"), vec!["signed-action-value"]);
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{target_address}/object\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("redirect response");
        });

        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let endpoint = LfsEndpoint {
            operation: LfsOperation::Fetch,
            source: LfsEndpointSource::DerivedRemote,
            url: parse_http_url(&format!("http://{endpoint_address}/info/lfs"))
                .expect("endpoint URL"),
        };
        let mut http = ClientConfig::default();
        http.connection_policy.proxy = zmin_http_transport::ProxyPolicy::Disabled;
        http.retry.max_attempts = 1;
        http.redirect_policy = RedirectPolicy::default()
            .allow_cross_origin_header("Accept")
            .expect("safe public redirect header");
        let config = LfsTransferConfig::new(1)
            .expect("config")
            .with_http_config(http);
        let client = LfsTransferClient::new(
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            no_action_credentials(),
            config,
        )
        .expect("client");

        let report = client.download(&[object(5)], None).expect("download");
        assert!(report.is_complete());
        endpoint_server.join().expect("endpoint server");
        target_server.join().expect("target server");
    }

    #[test]
    fn same_origin_action_does_not_reuse_endpoint_credentials() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let object_oid = oid().hex().to_owned();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("batch request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/info/lfs/objects/batch");
            let response = format!(
                "{{\"objects\":[{{\"oid\":\"{object_oid}\",\"size\":5,\"actions\":{{\"download\":{{\"href\":\"http://{address}/download\"}}}}}}]}}"
            );
            write_http_response(&mut stream, 200, response.as_bytes());

            let (mut stream, _) = listener.accept().expect("action request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/download");
            assert!(request.header_values("authorization").is_empty());
            assert!(request.header_values("cookie").is_empty());
            write_http_response(&mut stream, 200, b"hello");
        });
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let auth = LfsTransferAuthHeaders::from_pairs(vec![
            (
                "Authorization".to_owned(),
                b"Bearer endpoint-secret".to_vec(),
            ),
            ("Cookie".to_owned(), b"session=endpoint".to_vec()),
        ])
        .expect("endpoint auth");
        let endpoint = format!("http://{address}/info/lfs");
        let client = fixture_client_with_auth(LfsOperation::Fetch, &endpoint, store, auth);

        let report = client.download(&[object(5)], None).expect("download");
        assert!(report.is_complete());
        server.join().expect("fixture server");
    }

    #[test]
    fn expired_actions_return_a_refetch_signal_with_skew() {
        let request = LfsBatchRequest::new(
            LfsBatchOperation::Download,
            vec![LfsBatchObject::new(oid(), 5).expect("object")],
        )
        .expect("request");
        let body = br#"{"objects":[{"oid":"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824","size":5,"actions":{"download":{"href":"https://example.test/object","expires_in":10}}}]}"#;
        let response = parse_batch_response(200, Cursor::new(body), Some(&request)).expect("batch");
        let LfsBatchResponse::Success(response) = response else {
            panic!("expected success")
        };
        let action = response.objects()[0].actions()[0].action().clone();
        let issued = UNIX_EPOCH + Duration::from_secs(1_000);
        let now = issued + Duration::from_secs(1);
        let error = ensure_action_usable(&action, LfsBatchActionKind::Download, issued, now)
            .expect_err("action should be expired inside skew");
        assert!(error.needs_refetch());
        assert!(matches!(error, LfsTransferFailure::ActionExpired(_)));
    }

    #[test]
    fn fail_fast_cancellation_is_shared_by_queued_workers() {
        let token = LfsTransferCancellation::new();
        let queued = token.clone();
        assert!(!queued.is_cancelled());
        token.cancel();
        assert!(queued.is_cancelled());
    }

    #[test]
    fn cancellation_reader_stops_std_io_copy_without_retrying() {
        let token = LfsTransferCancellation::new();
        let mut reader = LfsTransferCancellationReader::new(Cursor::new(b"payload"), token.clone());
        let mut prefix = [0_u8; 3];
        reader.read_exact(&mut prefix).expect("initial read");
        assert_eq!(&prefix, b"pay");
        token.cancel();
        let error = io::copy(&mut reader, &mut io::sink())
            .expect_err("cancelled reader must fail closed without an Interrupted retry loop");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
    }

    #[test]
    fn fail_fast_wakes_a_stalled_download_and_never_starts_the_next_wave() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let slow_oid = oid();
        let failed_oid =
            LfsOid::from_hex("486ea46224d1bb4fb680f34f7c9ad96a8f24ec88be73ea8e5a6c65260e9cb8a7")
                .expect("failed oid");
        let queued_oid =
            LfsOid::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .expect("queued oid");
        let slow_oid_hex = slow_oid.hex().to_owned();
        let failed_oid_hex = failed_oid.hex().to_owned();
        let queued_oid_hex = queued_oid.hex().to_owned();
        let (client_done_sender, client_done_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut batch_stream, _) = listener.accept().expect("batch request");
            let batch = read_captured_http_request(&mut batch_stream);
            assert_eq!(batch.path, "/info/lfs/objects/batch");
            let response = format!(
                "{{\"objects\":[{{\"oid\":\"{slow_oid_hex}\",\"size\":5,\"actions\":{{\"download\":{{\"href\":\"http://{address}/slow\"}}}}}},{{\"oid\":\"{failed_oid_hex}\",\"size\":5,\"actions\":{{\"download\":{{\"href\":\"http://{address}/fail\"}}}}}},{{\"oid\":\"{queued_oid_hex}\",\"size\":1,\"actions\":{{\"download\":{{\"href\":\"http://{address}/queued\"}}}}}}]}}"
            );
            write_http_response(&mut batch_stream, 200, response.as_bytes());

            let mut slow_stream = None;
            let mut fail_stream = None;
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("first worker wave");
                let request = read_captured_http_request(&mut stream);
                match request.path.as_str() {
                    "/slow" => slow_stream = Some(stream),
                    "/fail" => fail_stream = Some(stream),
                    path => panic!("unexpected first-wave action: {path}"),
                }
            }
            let mut slow_stream = slow_stream.expect("slow request");
            write!(
                slow_stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n"
            )
            .expect("slow response header");
            slow_stream.flush().expect("flush slow response header");
            write_http_response(&mut fail_stream.expect("failed request"), 500, b"");

            client_done_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("request-wide cancellation wakes stalled response");
            listener.set_nonblocking(true).expect("nonblocking probe");
            match listener.accept() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Ok(_) => panic!("queued wave started after fail-fast cancellation"),
                Err(error) => panic!("queued-wave probe: {error}"),
            }
        });

        let root = tempdir().expect("store root");
        let store_root = root.path().join("store");
        let store = Arc::new(LfsStore::new(store_root.clone()).expect("store"));
        let endpoint = LfsEndpoint {
            operation: LfsOperation::Fetch,
            source: LfsEndpointSource::DerivedRemote,
            url: parse_http_url(&format!("http://{address}/info/lfs")).expect("endpoint URL"),
        };
        let mut http = ClientConfig::default();
        http.connection_policy.proxy = zmin_http_transport::ProxyPolicy::Disabled;
        http.retry.max_attempts = 1;
        let http_policy = Arc::new(
            LfsHttpPolicy::from_entries(
                &[lfs_activity_timeout_entry("", "0")],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("disabled activity timeout"),
        );
        let worker_observer = Arc::new(LfsTransferWorkerTestObserver::default());
        let config = LfsTransferConfig::new(2)
            .expect("config")
            .with_partial_results(LfsPartialResultPolicy::FailFast)
            .with_http_config(http)
            .with_http_policy(http_policy)
            .with_worker_observer(Arc::clone(&worker_observer));
        let client = LfsTransferClient::new(
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            no_action_credentials(),
            config,
        )
        .expect("client");
        let objects = [
            LfsTransferObject::new(slow_oid, 5).expect("slow object"),
            LfsTransferObject::new(failed_oid, 5).expect("failed object"),
            LfsTransferObject::new(queued_oid, 1).expect("queued object"),
        ];
        let report = client.download(&objects, None).expect("partial report");
        client_done_sender
            .send(())
            .expect("signal client completion");
        assert!(!report.is_complete());
        assert!(report.succeeded().is_empty());
        assert_eq!(report.failed().len(), 2);
        assert!(report
            .failed()
            .iter()
            .any(|(_, failure)| matches!(failure, LfsTransferFailure::Cancelled)));
        assert_eq!(report.pending().len(), 1);
        assert_eq!(report.pending()[0].object(), objects[2]);
        assert_eq!(
            report.pending()[0].reason(),
            LfsTransferPendingReason::NotStartedAfterFailFast
        );
        assert!(report.accounts_for(&objects));
        let refresh_candidates = report.refresh_candidates();
        assert_eq!(refresh_candidates.len(), 2);
        assert!(refresh_candidates.contains(&objects[2]));
        assert!(report.failed().iter().any(|(object, failure)| {
            matches!(failure, LfsTransferFailure::Cancelled) && refresh_candidates.contains(object)
        }));
        assert_eq!(count_transfer_temporary_files(&store_root), 0);
        assert_eq!(worker_observer.spawned.load(Ordering::SeqCst), 2);
        assert_eq!(worker_observer.waves.load(Ordering::SeqCst), 1);
        assert_eq!(
            worker_observer.started.lock().expect("started jobs").len(),
            2
        );
        assert_eq!(
            worker_observer
                .thread_names
                .lock()
                .expect("worker names")
                .len(),
            2
        );
        server.join().expect("fixture server");
    }

    #[test]
    fn fail_fast_wakes_an_upload_waiting_for_response_and_leaves_no_next_wave() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let stalled_oid = oid();
        let failed_oid =
            LfsOid::from_hex("486ea46224d1bb4fb680f34f7c9ad96a8f24ec88be73ea8e5a6c65260e9cb8a7")
                .expect("failed oid");
        let queued_oid =
            LfsOid::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .expect("queued oid");
        let stalled_oid_hex = stalled_oid.hex().to_owned();
        let failed_oid_hex = failed_oid.hex().to_owned();
        let queued_oid_hex = queued_oid.hex().to_owned();
        let (client_done_sender, client_done_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut batch_stream, _) = listener.accept().expect("batch request");
            let batch = read_captured_http_request(&mut batch_stream);
            assert_eq!(batch.path, "/info/lfs/objects/batch");
            let response = format!(
                "{{\"objects\":[{{\"oid\":\"{stalled_oid_hex}\",\"size\":5,\"actions\":{{\"upload\":{{\"href\":\"http://{address}/stall-upload\"}}}}}},{{\"oid\":\"{failed_oid_hex}\",\"size\":5,\"actions\":{{\"upload\":{{\"href\":\"http://{address}/fail-upload\"}}}}}},{{\"oid\":\"{queued_oid_hex}\",\"size\":1,\"actions\":{{\"upload\":{{\"href\":\"http://{address}/queued-upload\"}}}}}}]}}"
            );
            write_http_response(&mut batch_stream, 200, response.as_bytes());

            let mut stalled_stream = None;
            let mut failed_stream = None;
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("first upload wave");
                let request = read_captured_http_request(&mut stream);
                match request.path.as_str() {
                    "/stall-upload" => {
                        assert_eq!(request.body, b"hello");
                        stalled_stream = Some(stream);
                    }
                    "/fail-upload" => {
                        assert_eq!(request.body, b"world");
                        failed_stream = Some(stream);
                    }
                    path => panic!("unexpected first-wave upload: {path}"),
                }
            }
            let _stalled_stream = stalled_stream.expect("fully read stalled upload");
            write_http_response(&mut failed_stream.expect("failed upload"), 500, b"");

            client_done_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("request-wide cancellation wakes response wait");
            listener.set_nonblocking(true).expect("nonblocking probe");
            match listener.accept() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Ok(_) => panic!("queued upload started after fail-fast cancellation"),
                Err(error) => panic!("queued-upload probe: {error}"),
            }
        });

        let root = tempdir().expect("store root");
        let store_root = root.path().join("store");
        let store = Arc::new(LfsStore::new(store_root.clone()).expect("store"));
        store
            .ingest(stalled_oid.bytes(), 5, Cursor::new(b"hello"))
            .expect("seed stalled object");
        store
            .ingest(failed_oid.bytes(), 5, Cursor::new(b"world"))
            .expect("seed failed object");
        let endpoint = LfsEndpoint {
            operation: LfsOperation::Push,
            source: LfsEndpointSource::DerivedRemote,
            url: parse_http_url(&format!("http://{address}/info/lfs")).expect("endpoint URL"),
        };
        let mut http = ClientConfig::default();
        http.connection_policy.proxy = zmin_http_transport::ProxyPolicy::Disabled;
        http.retry.max_attempts = 1;
        let http_policy = Arc::new(
            LfsHttpPolicy::from_entries(
                &[lfs_activity_timeout_entry("", "0")],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("disabled activity timeout"),
        );
        let worker_observer = Arc::new(LfsTransferWorkerTestObserver::default());
        let config = LfsTransferConfig::new(2)
            .expect("config")
            .with_partial_results(LfsPartialResultPolicy::FailFast)
            .with_http_config(http)
            .with_http_policy(http_policy)
            .with_worker_observer(Arc::clone(&worker_observer));
        let client = LfsTransferClient::new(
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            no_action_credentials(),
            config,
        )
        .expect("client");
        let objects = [
            LfsTransferObject::new(stalled_oid, 5).expect("stalled object"),
            LfsTransferObject::new(failed_oid, 5).expect("failed object"),
            LfsTransferObject::new(queued_oid, 1).expect("queued object"),
        ];
        let report = client.upload(&objects, None).expect("partial report");
        client_done_sender
            .send(())
            .expect("signal client completion");
        assert!(!report.is_complete());
        assert!(report.succeeded().is_empty());
        assert_eq!(report.failed().len(), 2);
        assert!(report
            .failed()
            .iter()
            .any(|(_, failure)| matches!(failure, LfsTransferFailure::Cancelled)));
        assert_eq!(report.pending().len(), 1);
        assert_eq!(report.pending()[0].object(), objects[2]);
        assert_eq!(
            report.pending()[0].reason(),
            LfsTransferPendingReason::NotStartedAfterFailFast
        );
        assert!(report.accounts_for(&objects));
        assert_eq!(count_transfer_temporary_files(&store_root), 0);
        assert_eq!(worker_observer.spawned.load(Ordering::SeqCst), 2);
        assert_eq!(worker_observer.waves.load(Ordering::SeqCst), 1);
        assert_eq!(
            worker_observer.started.lock().expect("started jobs").len(),
            2
        );
        assert_eq!(
            worker_observer
                .thread_names
                .lock()
                .expect("worker names")
                .len(),
            2
        );
        server.join().expect("fixture server");
    }

    #[test]
    fn download_fixture_streams_batch_action_into_store() {
        let (endpoint, server) = fixture(false, b"hello");
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let client = fixture_client(LfsOperation::Fetch, &endpoint, Arc::clone(&store));
        let report = client
            .download(&[object(5)], None)
            .expect("download report");
        assert!(report.is_complete());
        assert_eq!(report.succeeded().len(), 1);
        assert!(report.failed().is_empty());
        server.join().expect("fixture");
    }

    #[test]
    fn upload_fixture_reopens_verified_reader_and_verifies_action() {
        let (endpoint, server) = fixture(true, b"hello");
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        store
            .ingest(oid().bytes(), 5, Cursor::new(b"hello"))
            .expect("seed object");
        let client = fixture_client(LfsOperation::Push, &endpoint, Arc::clone(&store));
        let report = client.upload(&[object(5)], None).expect("upload report");
        assert!(report.is_complete());
        assert_eq!(report.succeeded().len(), 1);
        assert!(report.failed().is_empty());
        server.join().expect("fixture");
    }

    #[test]
    fn upload_without_actions_means_the_server_already_has_the_object() {
        use std::sync::mpsc;

        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let object_oid = oid().hex().to_owned();
        let (client_done_sender, client_done_receiver) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("batch request");
            let request = read_captured_http_request(&mut stream);
            assert_eq!(request.path, "/info/lfs/objects/batch");
            let response = format!("{{\"objects\":[{{\"oid\":\"{object_oid}\",\"size\":5}}]}}");
            write_http_response(&mut stream, 200, response.as_bytes());

            client_done_receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("client completion");
            listener
                .set_nonblocking(true)
                .expect("nonblocking listener");
            match listener.accept() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Ok(_) => panic!("unexpected object PUT after no-action batch response"),
                Err(error) => panic!("probe object PUT: {error}"),
            }
        });

        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let store_path = root.path().to_owned();
        drop(root);
        assert!(!store_path.exists(), "fixture store must be unavailable");
        let endpoint = format!("http://{address}/info/lfs");
        let client = fixture_client(LfsOperation::Push, &endpoint, store);

        let result = client.upload(&[object(5)], None);
        client_done_sender
            .send(())
            .expect("signal client completion");
        let report = result.expect("already-present upload report");
        assert!(report.is_complete());
        assert!(report.failed().is_empty());
        assert_eq!(report.succeeded().len(), 1);
        assert_eq!(
            report.succeeded()[0].outcome(),
            LfsTransferSuccessKind::Uploaded
        );
        server.join().expect("fixture server");
    }

    #[test]
    fn upload_verify_action_without_upload_action_is_rejected() {
        let request = LfsBatchRequest::new(
            LfsBatchOperation::Upload,
            vec![LfsBatchObject::new(oid(), 5).expect("object")],
        )
        .expect("request");
        let body = br#"{"objects":[{"oid":"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824","size":5,"actions":{"verify":{"href":"https://example.test/verify"}}}]}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(body), Some(&request)),
            Err(LfsBatchError::MissingField("actions.upload"))
        );
    }

    #[test]
    fn batch_413_is_closed_before_bounded_recursive_split() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let first_oid = oid().hex().to_owned();
        let second_oid =
            "486ea46224d1bb4fb680f34f7c9ad96a8f24ec88be73ea8e5a6c65260e9cb8a7".to_owned();
        let server_second_oid = second_oid.clone();
        let endpoint = format!("http://{address}/info/lfs");
        let server = thread::spawn(move || {
            let mut batch_requests = 0;
            let mut downloads = 0;
            while batch_requests < 3 || downloads < 2 {
                let (mut stream, _) = listener.accept().expect("fixture connection");
                let (path, body) = read_http_request(&mut stream);
                if path == "/info/lfs/objects/batch" {
                    batch_requests += 1;
                    if batch_requests == 1 {
                        write_http_status_response(&mut stream, 413, b"too large");
                        continue;
                    }
                    let text = String::from_utf8(body).expect("batch body");
                    let (object_oid, download_path) = if text.contains(&server_second_oid) {
                        (&server_second_oid, "/download/world")
                    } else {
                        (&first_oid, "/download/hello")
                    };
                    let response = format!(
                        "{{\"objects\":[{{\"oid\":\"{object_oid}\",\"size\":5,\"actions\":{{\"download\":{{\"href\":\"http://{address}{download_path}\"}}}}}}]}}"
                    );
                    write_http_status_response(&mut stream, 200, response.as_bytes());
                } else if path == "/download/hello" {
                    downloads += 1;
                    write_http_status_response(&mut stream, 200, b"hello");
                } else if path == "/download/world" {
                    downloads += 1;
                    write_http_status_response(&mut stream, 200, b"world");
                } else {
                    write_http_status_response(&mut stream, 404, b"");
                }
            }
        });
        let root = tempdir().expect("store root");
        let store = Arc::new(LfsStore::new(root.path().to_owned()).expect("store"));
        let client = fixture_client(LfsOperation::Fetch, &endpoint, Arc::clone(&store));
        let second = LfsTransferObject::new(LfsOid::from_hex(&second_oid).expect("second oid"), 5)
            .expect("second object");
        let report = client
            .download(&[object(5), second], None)
            .expect("split report");
        assert!(report.is_complete());
        assert_eq!(report.batch_requests(), 3);
        assert_eq!(report.split_requests(), 1);
        server.join().expect("fixture");
    }

    #[test]
    fn lfs_transfer_removes_generic_whole_request_and_operation_caps() {
        let defaults = LfsTransferConfig::default();
        assert_eq!(defaults.http.request_timeout, None);
        assert_eq!(defaults.http.operation_timeout, None);

        let mut generic = ClientConfig::default();
        generic.request_timeout = Some(Duration::from_secs(120));
        generic.operation_timeout = Some(Duration::from_secs(120));
        let configured = LfsTransferConfig::default().with_http_config(generic);
        assert_eq!(configured.http.request_timeout, None);
        assert_eq!(configured.http.operation_timeout, None);
    }

    #[test]
    fn endpoint_bound_policy_resolves_activity_for_batch_action_and_redirect_urls() {
        let policy = Arc::new(
            LfsHttpPolicy::from_entries(
                &[
                    lfs_activity_timeout_entry("", "5"),
                    lfs_activity_timeout_entry("https://endpoint.example/info/lfs/objects", "11"),
                    lfs_activity_timeout_entry("https://objects.example/team", "7"),
                    lfs_activity_timeout_entry("https://redirect.example/final", "9"),
                ],
                LfsHttpEnvironmentSnapshot::default(),
            )
            .expect("timeout policy"),
        );
        let endpoint = HttpUrl::parse("https://endpoint.example/info/lfs")
            .expect("endpoint URL")
            .origin()
            .clone();
        let resolver = LfsEndpointBoundHttpPolicy {
            endpoint_origin: endpoint,
            policy,
        };
        for (url, seconds) in [
            ("https://endpoint.example/info/lfs/objects/batch", 11_u64),
            ("https://objects.example/team/object", 7),
            ("https://objects.example/team/redirected", 7),
            ("https://redirect.example/final/object", 9),
            ("https://redirect.example/other", 5),
        ] {
            let resolved = resolver
                .resolve(&HttpUrl::parse(url).expect("actual request URL"))
                .expect("resolved actual-URL policy");
            assert_eq!(
                resolved.timeouts.activity_timeout,
                Some(Duration::from_secs(seconds)),
                "{url}"
            );
        }
    }

    fn fixture_client(
        operation: LfsOperation,
        endpoint: &str,
        store: Arc<LfsStore>,
    ) -> LfsTransferClient {
        fixture_client_with_auth(operation, endpoint, store, LfsTransferAuthHeaders::empty())
    }

    fn fixture_client_with_auth(
        operation: LfsOperation,
        endpoint: &str,
        store: Arc<LfsStore>,
        auth: LfsTransferAuthHeaders,
    ) -> LfsTransferClient {
        fixture_client_with_auth_and_policy(
            operation,
            endpoint,
            store,
            auth,
            Arc::new(LfsHttpPolicy::default()),
        )
    }

    fn fixture_client_with_auth_and_policy(
        operation: LfsOperation,
        endpoint: &str,
        store: Arc<LfsStore>,
        auth: LfsTransferAuthHeaders,
        policy: Arc<LfsHttpPolicy>,
    ) -> LfsTransferClient {
        fixture_client_with_auth_policy_and_action_credentials(
            operation,
            endpoint,
            store,
            auth,
            policy,
            no_action_credentials(),
        )
    }

    fn fixture_client_with_auth_policy_and_action_credentials(
        operation: LfsOperation,
        endpoint: &str,
        store: Arc<LfsStore>,
        auth: LfsTransferAuthHeaders,
        policy: Arc<LfsHttpPolicy>,
        action_credentials: Arc<dyn LfsActionCredentialProvider>,
    ) -> LfsTransferClient {
        let endpoint = LfsEndpoint {
            operation,
            source: LfsEndpointSource::DerivedRemote,
            url: parse_http_url(endpoint).expect("endpoint URL"),
        };
        let mut http = ClientConfig::default();
        http.connection_policy.proxy = zmin_http_transport::ProxyPolicy::Disabled;
        http.retry.max_attempts = 1;
        let config = LfsTransferConfig::new(1)
            .expect("config")
            .with_http_config(http)
            .with_http_policy(policy);
        LfsTransferClient::new(&endpoint, store, auth, action_credentials, config).expect("client")
    }

    fn fixture_client_with_concurrency(
        operation: LfsOperation,
        endpoint: &str,
        store: Arc<LfsStore>,
        concurrency: usize,
        observer: Arc<LfsTransferWorkerTestObserver>,
    ) -> LfsTransferClient {
        let endpoint = LfsEndpoint {
            operation,
            source: LfsEndpointSource::DerivedRemote,
            url: parse_http_url(endpoint).expect("endpoint URL"),
        };
        let mut http = ClientConfig::default();
        http.connection_policy.proxy = zmin_http_transport::ProxyPolicy::Disabled;
        http.retry.max_attempts = 1;
        let config = LfsTransferConfig::new(concurrency)
            .expect("concurrency")
            .with_http_config(http)
            .with_http_policy(Arc::new(LfsHttpPolicy::default()))
            .with_worker_observer(observer);
        LfsTransferClient::new(
            &endpoint,
            store,
            LfsTransferAuthHeaders::empty(),
            no_action_credentials(),
            config,
        )
        .expect("client")
    }

    fn http_policy_entry(subsection: &str, value: &str) -> ConfigEntry {
        ConfigEntry {
            section: "http".to_owned(),
            raw_section: "http".to_owned(),
            subsection: subsection.to_owned(),
            key: "extraheader".to_owned(),
            raw_key: "extraHeader".to_owned(),
            value: value.to_owned(),
            comment: None,
            implicit_bool: false,
            scope: ConfigScope::Local,
            origin: "test".to_owned(),
            line: None,
        }
    }

    fn lfs_activity_timeout_entry(subsection: &str, value: &str) -> ConfigEntry {
        ConfigEntry {
            section: "lfs".to_owned(),
            raw_section: "lfs".to_owned(),
            subsection: subsection.to_owned(),
            key: "activitytimeout".to_owned(),
            raw_key: "activityTimeout".to_owned(),
            value: value.to_owned(),
            comment: None,
            implicit_bool: false,
            scope: ConfigScope::Local,
            origin: "test".to_owned(),
            line: None,
        }
    }

    fn fixture(upload: bool, payload: &'static [u8]) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let oid = oid().hex().to_owned();
        let endpoint = format!("http://{address}/info/lfs");
        let server = thread::spawn(move || {
            let expected_requests = if upload { 3 } else { 2 };
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().expect("fixture connection");
                let (path, body) = read_http_request(&mut stream);
                if path == "/info/lfs/objects/batch" {
                    let action = if upload {
                        format!(
                            "\"upload\":{{\"href\":\"http://{address}/upload\"}},\"verify\":{{\"href\":\"http://{address}/verify\"}}"
                        )
                    } else {
                        format!("\"download\":{{\"href\":\"http://{address}/download\"}}")
                    };
                    let response = format!(
                        "{{\"objects\":[{{\"oid\":\"{oid}\",\"size\":5,\"actions\":{{{action}}}}}]}}"
                    );
                    write_http_response(&mut stream, 200, response.as_bytes());
                } else if upload && path == "/upload" {
                    assert_eq!(body, payload);
                    write_http_response(&mut stream, 200, b"");
                } else if upload && path == "/verify" {
                    assert!(body.starts_with(b"{\"oid\":\""));
                    write_http_response(&mut stream, 200, b"");
                } else if !upload && path == "/download" {
                    write_http_response(&mut stream, 200, payload);
                } else {
                    write_http_response(&mut stream, 404, b"");
                }
            }
        });
        (endpoint, server)
    }

    fn count_transfer_temporary_files(path: &std::path::Path) -> usize {
        let Ok(entries) = std::fs::read_dir(path) else {
            return 0;
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    count_transfer_temporary_files(&path)
                } else {
                    usize::from(
                        path.file_name()
                            .is_some_and(|name| name.to_string_lossy().contains(".tmp-")),
                    )
                }
            })
            .sum()
    }

    struct CapturedHttpRequest {
        path: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl CapturedHttpRequest {
        fn header_values(&self, name: &str) -> Vec<&str> {
            self.headers
                .iter()
                .filter_map(|(current, value)| current.eq_ignore_ascii_case(name).then_some(value))
                .map(String::as_str)
                .collect()
        }
    }

    fn read_http_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
        let request = read_captured_http_request(stream);
        (request.path, request.body)
    }

    fn read_captured_http_request(stream: &mut TcpStream) -> CapturedHttpRequest {
        let mut headers = Vec::new();
        let mut byte = [0_u8; 1];
        while !headers.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).expect("fixture headers");
            headers.push(byte[0]);
            assert!(headers.len() < 64 * 1024, "fixture header bound");
        }
        let text = String::from_utf8(headers).expect("fixture header UTF-8");
        let mut lines = text.split("\r\n");
        let path = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .expect("fixture request line")
            .to_owned();
        let headers = lines
            .filter(|line| !line.is_empty())
            .map(|line| {
                let (name, value) = line.split_once(':').expect("fixture header");
                (name.to_owned(), value.trim().to_owned())
            })
            .collect::<Vec<_>>();
        let length = headers
            .iter()
            .find_map(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then_some(value.as_str())
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0_u8; length];
        stream.read_exact(&mut body).expect("fixture body");
        CapturedHttpRequest {
            path,
            headers,
            body,
        }
    }

    fn write_http_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
        write_http_status_response(stream, status, body);
    }

    fn write_http_status_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
        let reason = match status {
            200 => "OK",
            404 => "Not Found",
            413 => "Payload Too Large",
            _ => "Error",
        };
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).expect("fixture response");
        stream.write_all(body).expect("fixture body");
    }
}
