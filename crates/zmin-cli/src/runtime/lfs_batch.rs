//! Typed, bounded Git LFS Batch and Basic Transfer messages.
//!
//! The wire format follows the current Git LFS Batch API and Basic Transfer
//! API specifications.  This module intentionally uses a small JSON reader
//! instead of adding a dependency: response bodies are bounded before parsing,
//! all values are validated into typed structures, and serialization has one
//! deterministic field order.
//!
//! Specifications:
//! <https://github.com/git-lfs/git-lfs/blob/main/docs/api/batch.md>
//! <https://github.com/git-lfs/git-lfs/blob/main/docs/api/basic-transfers.md>

use std::collections::HashSet;
use std::fmt;
use std::io::{self, Read};

use super::lfs_endpoint::{LFS_HTTP_URL_MAX_BYTES, LfsHttpUrl, parse_http_url};
use super::lfs_pointer::LfsOid;

pub(crate) const LFS_BATCH_MEDIA_TYPE: &str = "application/vnd.git-lfs+json";
pub(crate) const LFS_BATCH_MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const LFS_BATCH_MAX_OBJECTS: usize = 1_024;
pub(crate) const LFS_BATCH_MAX_HEADERS: usize = 128;
pub(crate) const LFS_BATCH_MAX_HEADER_NAME_BYTES: usize = 256;
pub(crate) const LFS_BATCH_MAX_HEADER_VALUE_BYTES: usize = 16 * 1024;
pub(crate) const LFS_BATCH_MAX_URL_BYTES: usize = LFS_HTTP_URL_MAX_BYTES;
pub(crate) const LFS_BATCH_MAX_REF_BYTES: usize = 4 * 1024;
pub(crate) const LFS_BATCH_MAX_STRING_BYTES: usize = 64 * 1024;

const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_FIELDS: usize = 256;
const MAX_JSON_ARRAY_ITEMS: usize = LFS_BATCH_MAX_OBJECTS * 2;
const MAX_EXPIRY_SECONDS: i64 = 2_147_483_647;

/// A sanitized Batch/Basic Transfer parsing or construction error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsBatchError {
    BodyTooLarge,
    Read(io::ErrorKind),
    InvalidJson,
    InvalidStructure,
    MissingField(&'static str),
    InvalidField(&'static str),
    DuplicateField(&'static str),
    DuplicateObject,
    DuplicateAction,
    DuplicateHeader,
    UnsupportedTransfer,
    UnsupportedAction,
    UnsupportedHashAlgorithm,
    InvalidOid,
    InvalidSize,
    InvalidExpiry,
    InvalidUrl,
    InvalidHeader,
    TooManyObjects,
    TooManyHeaders,
    InvalidBatchLimit,
    RequestTooLarge,
}

impl fmt::Display for LfsBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyTooLarge => formatter.write_str("LFS batch body is too large"),
            Self::Read(kind) => write!(formatter, "LFS batch body read failed ({kind:?})"),
            Self::InvalidJson => formatter.write_str("invalid LFS batch JSON"),
            Self::InvalidStructure => formatter.write_str("invalid LFS batch response structure"),
            Self::MissingField(field) => write!(formatter, "missing LFS batch field {field}"),
            Self::InvalidField(field) => write!(formatter, "invalid LFS batch field {field}"),
            Self::DuplicateField(field) => write!(formatter, "duplicate LFS batch field {field}"),
            Self::DuplicateObject => formatter.write_str("duplicate LFS batch object"),
            Self::DuplicateAction => formatter.write_str("duplicate LFS batch action"),
            Self::DuplicateHeader => formatter.write_str("duplicate LFS batch header"),
            Self::UnsupportedTransfer => formatter.write_str("unsupported LFS transfer adapter"),
            Self::UnsupportedAction => formatter.write_str("unsupported LFS transfer action"),
            Self::UnsupportedHashAlgorithm => formatter.write_str("unsupported LFS hash algorithm"),
            Self::InvalidOid => formatter.write_str("invalid LFS object id"),
            Self::InvalidSize => formatter.write_str("invalid LFS object size"),
            Self::InvalidExpiry => formatter.write_str("invalid LFS action expiry"),
            Self::InvalidUrl => formatter.write_str("invalid LFS action URL"),
            Self::InvalidHeader => formatter.write_str("invalid LFS action header"),
            Self::TooManyObjects => formatter.write_str("too many LFS batch objects"),
            Self::TooManyHeaders => formatter.write_str("too many LFS action headers"),
            Self::InvalidBatchLimit => formatter.write_str("invalid LFS batch split limit"),
            Self::RequestTooLarge => formatter.write_str("LFS batch request is too large"),
        }
    }
}

impl std::error::Error for LfsBatchError {}

/// The operation requested from an LFS server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsBatchOperation {
    Download,
    Upload,
}

impl LfsBatchOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Upload => "upload",
        }
    }
}

/// The only transfer adapter currently supported by Git LFS clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsTransferAdapter {
    Basic,
}

impl LfsTransferAdapter {
    fn as_str(self) -> &'static str {
        "basic"
    }
}

/// The hash algorithm named by the current Batch API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsBatchHashAlgorithm {
    Sha256,
}

impl LfsBatchHashAlgorithm {
    fn as_str(self) -> &'static str {
        "sha256"
    }
}

/// One object named in a Batch request or response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchObject {
    oid: LfsOid,
    size: u64,
}

impl LfsBatchObject {
    pub(crate) fn new(oid: LfsOid, size: u64) -> Result<Self, LfsBatchError> {
        if size > i64::MAX as u64 {
            return Err(LfsBatchError::InvalidSize);
        }
        Ok(Self { oid, size })
    }

    pub(crate) fn oid(&self) -> LfsOid {
        self.oid
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}

/// The JSON body sent by the Basic Transfer verify action after an upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsVerifyRequest {
    object: LfsBatchObject,
}

impl LfsVerifyRequest {
    pub(crate) fn new(object: LfsBatchObject) -> Self {
        Self { object }
    }

    pub(crate) fn object(&self) -> &LfsBatchObject {
        &self.object
    }

    pub(crate) fn to_json(&self) -> Vec<u8> {
        let mut writer = JsonWriter::new();
        writer.begin_object();
        writer.field_string("oid", self.object.oid.hex());
        writer.comma();
        writer.field_number("size", self.object.size);
        writer.end_object();
        writer.finish()
    }
}

/// Optional server ref context sent with a Batch request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchRef {
    name: String,
}

impl LfsBatchRef {
    pub(crate) fn new(name: impl Into<String>) -> Result<Self, LfsBatchError> {
        let name = name.into();
        validate_ref_name(&name)?;
        Ok(Self { name })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// A deterministic Batch request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchRequest {
    operation: LfsBatchOperation,
    transfers: Vec<LfsTransferAdapter>,
    reference: Option<LfsBatchRef>,
    objects: Vec<LfsBatchObject>,
    hash_algo: LfsBatchHashAlgorithm,
}

impl LfsBatchRequest {
    pub(crate) fn new(
        operation: LfsBatchOperation,
        objects: Vec<LfsBatchObject>,
    ) -> Result<Self, LfsBatchError> {
        if objects.is_empty() {
            return Err(LfsBatchError::InvalidStructure);
        }
        if objects.len() > LFS_BATCH_MAX_OBJECTS {
            return Err(LfsBatchError::TooManyObjects);
        }
        ensure_unique_objects(&objects)?;
        Ok(Self {
            operation,
            transfers: vec![LfsTransferAdapter::Basic],
            reference: None,
            objects,
            hash_algo: LfsBatchHashAlgorithm::Sha256,
        })
    }

    pub(crate) fn operation(&self) -> LfsBatchOperation {
        self.operation
    }

    pub(crate) fn objects(&self) -> &[LfsBatchObject] {
        &self.objects
    }

    pub(crate) fn transfers(&self) -> &[LfsTransferAdapter] {
        &self.transfers
    }

    pub(crate) fn set_reference(&mut self, reference: LfsBatchRef) {
        self.reference = Some(reference);
    }

    pub(crate) fn clear_transfers(&mut self) {
        self.transfers.clear();
    }

    pub(crate) fn set_transfers(
        &mut self,
        transfers: Vec<LfsTransferAdapter>,
    ) -> Result<(), LfsBatchError> {
        if transfers
            .iter()
            .filter(|transfer| **transfer == LfsTransferAdapter::Basic)
            .count()
            > 1
        {
            return Err(LfsBatchError::DuplicateField("transfers"));
        }
        self.transfers = transfers;
        Ok(())
    }

    pub(crate) fn to_json(&self) -> Result<Vec<u8>, LfsBatchError> {
        ensure_unique_objects(&self.objects)?;
        if self.objects.is_empty() {
            return Err(LfsBatchError::InvalidStructure);
        }
        if self.objects.len() > LFS_BATCH_MAX_OBJECTS {
            return Err(LfsBatchError::TooManyObjects);
        }

        let mut writer = JsonWriter::new();
        writer.begin_object();
        writer.field_string("operation", self.operation.as_str());
        if !self.transfers.is_empty() {
            writer.field_array_start("transfers");
            for (index, transfer) in self.transfers.iter().enumerate() {
                if index != 0 {
                    writer.comma();
                }
                writer.string(transfer.as_str());
            }
            writer.end_array();
        }
        if let Some(reference) = &self.reference {
            writer.comma();
            writer.key("ref");
            writer.begin_object();
            writer.field_string("name", reference.name());
            writer.end_object();
        }
        writer.comma();
        writer.key("objects");
        writer.begin_array();
        for (index, object) in self.objects.iter().enumerate() {
            if index != 0 {
                writer.comma();
            }
            writer.begin_object();
            writer.field_string("oid", object.oid.hex());
            writer.comma();
            writer.field_number("size", object.size);
            writer.end_object();
        }
        writer.end_array();
        writer.field_string("hash_algo", self.hash_algo.as_str());
        writer.end_object();
        Ok(writer.finish())
    }
}

/// A validated HTTP header attached to a Basic Transfer action.
#[derive(Clone)]
pub(crate) struct LfsBatchHeader {
    name: String,
    value: LfsBatchHeaderValue,
}

#[derive(Clone)]
enum LfsBatchHeaderValue {
    Public(String),
    Secret(SecretHeaderValue),
}

#[derive(Clone, PartialEq, Eq)]
struct SecretHeaderValue(Vec<u8>);

impl SecretHeaderValue {
    fn as_str(&self) -> &str {
        // Construction validates UTF-8 before the bytes enter this type.
        std::str::from_utf8(&self.0).expect("validated secret header UTF-8")
    }
}

impl Drop for SecretHeaderValue {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl LfsBatchHeaderValue {
    fn as_str(&self) -> &str {
        match self {
            Self::Public(value) => value,
            Self::Secret(value) => value.as_str(),
        }
    }
}

impl PartialEq for LfsBatchHeader {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.value() == other.value()
    }
}

impl Eq for LfsBatchHeader {}

impl fmt::Debug for LfsBatchHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsBatchHeader")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

impl LfsBatchHeader {
    pub(crate) fn new(
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, LfsBatchError> {
        let name = name.into();
        let value = value.into();
        validate_header_name(&name)?;
        validate_header_value(&value)?;
        Ok(Self {
            name,
            value: LfsBatchHeaderValue::Public(value),
        })
    }

    /// Construct an owned authentication header without converting its secret
    /// bytes into a non-wiping `String`.
    pub(crate) fn new_secret(
        name: impl Into<String>,
        mut value: Vec<u8>,
    ) -> Result<Self, LfsBatchError> {
        let name = name.into();
        let validation = std::str::from_utf8(&value)
            .map_err(|_| LfsBatchError::InvalidHeader)
            .and_then(|value| validate_header_value(value))
            .and_then(|_| validate_header_name(&name));
        if let Err(error) = validation {
            value.fill(0);
            return Err(error);
        }
        Ok(Self {
            name,
            value: LfsBatchHeaderValue::Secret(SecretHeaderValue(value)),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn value(&self) -> &str {
        self.value.as_str()
    }

    pub(crate) fn is_secret(&self) -> bool {
        matches!(self.value, LfsBatchHeaderValue::Secret(_))
    }
}

/// A bounded, case-insensitive-unique collection of action headers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LfsBatchHeaders {
    entries: Vec<LfsBatchHeader>,
}

impl LfsBatchHeaders {
    pub(crate) fn new(entries: Vec<LfsBatchHeader>) -> Result<Self, LfsBatchError> {
        if entries.len() > LFS_BATCH_MAX_HEADERS {
            return Err(LfsBatchError::TooManyHeaders);
        }
        let mut headers = Self { entries };
        headers.sort_and_check()?;
        Ok(headers)
    }

    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn entries(&self) -> &[LfsBatchHeader] {
        &self.entries
    }

    fn sort_and_check(&mut self) -> Result<(), LfsBatchError> {
        for header in &self.entries {
            validate_header_name(&header.name)?;
            validate_header_value(header.value())?;
        }
        self.entries.sort_by(|left, right| {
            lower_ascii(&left.name)
                .cmp(&lower_ascii(&right.name))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.value().cmp(right.value()))
        });
        for pair in self.entries.windows(2) {
            if lower_ascii(&pair[0].name) == lower_ascii(&pair[1].name) {
                return Err(LfsBatchError::DuplicateHeader);
            }
        }
        Ok(())
    }
}

/// One Basic Transfer action supplied by a server.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchAction {
    href: LfsHttpUrl,
    headers: LfsBatchHeaders,
    expires_in: Option<i64>,
    expires_at: Option<String>,
}

impl fmt::Debug for LfsBatchAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsBatchAction")
            .field("href", &"<redacted>")
            .field("headers", &self.headers)
            .field("expires_in", &self.expires_in)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl LfsBatchAction {
    pub(crate) fn href(&self) -> &str {
        self.href.as_str()
    }

    pub(crate) fn headers(&self) -> &LfsBatchHeaders {
        &self.headers
    }

    pub(crate) fn expires_in(&self) -> Option<i64> {
        self.expires_in
    }

    pub(crate) fn expires_at(&self) -> Option<&str> {
        self.expires_at.as_deref()
    }
}

/// The action name attached to a response object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsBatchActionKind {
    Download,
    Upload,
    Verify,
}

/// A named action, preserving the typed distinction between download/upload/verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchActionEntry {
    kind: LfsBatchActionKind,
    action: LfsBatchAction,
}

impl LfsBatchActionEntry {
    pub(crate) fn kind(&self) -> LfsBatchActionKind {
        self.kind
    }

    pub(crate) fn action(&self) -> &LfsBatchAction {
        &self.action
    }
}

/// An individual object error from an otherwise successful 200 Batch response.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchObjectError {
    code: u16,
    message: String,
}

impl fmt::Debug for LfsBatchObjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsBatchObjectError")
            .field("code", &self.code)
            .field("message", &"<redacted>")
            .finish()
    }
}

impl LfsBatchObjectError {
    pub(crate) fn code(&self) -> u16 {
        self.code
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

/// A response object containing actions or a per-object error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchResponseObject {
    object: LfsBatchObject,
    authenticated: bool,
    actions: Vec<LfsBatchActionEntry>,
    error: Option<LfsBatchObjectError>,
}

impl LfsBatchResponseObject {
    pub(crate) fn object(&self) -> &LfsBatchObject {
        &self.object
    }

    pub(crate) fn authenticated(&self) -> bool {
        self.authenticated
    }

    pub(crate) fn actions(&self) -> &[LfsBatchActionEntry] {
        &self.actions
    }

    pub(crate) fn error(&self) -> Option<&LfsBatchObjectError> {
        self.error.as_ref()
    }
}

/// A successful 200 Batch response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchSuccessResponse {
    transfer: LfsTransferAdapter,
    objects: Vec<LfsBatchResponseObject>,
    hash_algo: LfsBatchHashAlgorithm,
}

impl LfsBatchSuccessResponse {
    pub(crate) fn transfer(&self) -> LfsTransferAdapter {
        self.transfer
    }

    pub(crate) fn objects(&self) -> &[LfsBatchResponseObject] {
        &self.objects
    }

    pub(crate) fn hash_algo(&self) -> LfsBatchHashAlgorithm {
        self.hash_algo
    }
}

/// Whether a response may be retried by splitting the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsBatchRetry {
    None,
    SplitRequest,
}

/// A sanitized top-level HTTP/API error response.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct LfsBatchErrorResponse {
    status: u16,
    message: Option<String>,
    request_id: Option<String>,
    retry: LfsBatchRetry,
    body: LfsBatchBodyDisposition,
}

impl fmt::Debug for LfsBatchErrorResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LfsBatchErrorResponse")
            .field("status", &self.status)
            .field("message", &"<redacted>")
            .field("request_id", &"<redacted>")
            .field("retry", &self.retry)
            .field("body", &self.body)
            .finish()
    }
}

impl LfsBatchErrorResponse {
    pub(crate) fn status(&self) -> u16 {
        self.status
    }

    pub(crate) fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub(crate) fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub(crate) fn retry(&self) -> LfsBatchRetry {
        self.retry
    }

    pub(crate) fn body_disposition(&self) -> LfsBatchBodyDisposition {
        self.body
    }
}

/// How the HTTP response body was handled before returning a typed result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsBatchBodyDisposition {
    Consumed,
    /// The parser deliberately did not read an unbounded 413 body.  The HTTP
    /// owner must drain it under its own transport limit or close the
    /// connection before retrying a split request.
    ConnectionMustClose,
}

/// Parsed Batch result. HTTP errors remain typed data so callers can decide
/// whether to authenticate, retry, or split a 413 request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LfsBatchResponse {
    Success(LfsBatchSuccessResponse),
    Error(LfsBatchErrorResponse),
}

/// Serialize a request with deterministic field and object order.
pub(crate) fn serialize_batch_request(request: &LfsBatchRequest) -> Result<Vec<u8>, LfsBatchError> {
    request.to_json()
}

/// Serialize the Basic Transfer verify body deterministically.
pub(crate) fn serialize_verify_request(request: &LfsVerifyRequest) -> Vec<u8> {
    request.to_json()
}

/// Split one request deterministically by object count and encoded byte size.
pub(crate) fn split_batch_request(
    request: &LfsBatchRequest,
    max_objects: usize,
    max_bytes: usize,
) -> Result<Vec<LfsBatchRequest>, LfsBatchError> {
    if max_objects == 0 || max_bytes == 0 {
        return Err(LfsBatchError::InvalidBatchLimit);
    }
    let (prefix_bytes, suffix_bytes) = request_object_scaffold(request)?;
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let scaffold_bytes = prefix_bytes
        .checked_add(suffix_bytes)
        .ok_or(LfsBatchError::RequestTooLarge)?;
    let mut current_bytes = scaffold_bytes;
    for object in &request.objects {
        let object_bytes = encoded_object_size(object);
        let separator_bytes = usize::from(!current.is_empty());
        let exceeds_limit = current.len() >= max_objects
            || current_bytes
                .checked_add(separator_bytes)
                .and_then(|size| size.checked_add(object_bytes))
                .map_or(true, |size| size > max_bytes);
        if exceeds_limit {
            if current.is_empty() {
                return Err(LfsBatchError::RequestTooLarge);
            }
            batches.push(request.with_objects(std::mem::take(&mut current))?);
            current_bytes = scaffold_bytes;
            if current_bytes
                .checked_add(object_bytes)
                .map_or(true, |size| size > max_bytes)
            {
                return Err(LfsBatchError::RequestTooLarge);
            }
        }
        let separator_bytes = usize::from(!current.is_empty());
        current.push(object.clone());
        current_bytes = current_bytes
            .checked_add(object_bytes)
            .and_then(|size| size.checked_add(separator_bytes))
            .ok_or(LfsBatchError::RequestTooLarge)?;
    }
    if !current.is_empty() {
        batches.push(request.with_objects(current)?);
    }
    Ok(batches)
}

impl LfsBatchRequest {
    fn with_objects(&self, objects: Vec<LfsBatchObject>) -> Result<Self, LfsBatchError> {
        let mut request = self.clone();
        request.objects = objects;
        ensure_unique_objects(&request.objects)?;
        Ok(request)
    }
}

fn request_object_scaffold(request: &LfsBatchRequest) -> Result<(usize, usize), LfsBatchError> {
    let mut writer = JsonWriter::new();
    writer.begin_object();
    writer.field_string("operation", request.operation.as_str());
    if !request.transfers.is_empty() {
        writer.field_array_start("transfers");
        for (index, transfer) in request.transfers.iter().enumerate() {
            if index != 0 {
                writer.comma();
            }
            writer.string(transfer.as_str());
        }
        writer.end_array();
    }
    if let Some(reference) = &request.reference {
        writer.comma();
        writer.key("ref");
        writer.begin_object();
        writer.field_string("name", reference.name());
        writer.end_object();
    }
    writer.comma();
    writer.key("objects");
    writer.begin_array();
    let prefix = writer.len();
    writer.end_array();
    writer.field_string("hash_algo", request.hash_algo.as_str());
    writer.end_object();
    let total = writer.finish().len();
    Ok((prefix, total - prefix))
}

fn encoded_object_size(object: &LfsBatchObject) -> usize {
    let mut writer = JsonWriter::new();
    writer.begin_object();
    writer.field_string("oid", object.oid.hex());
    writer.comma();
    writer.field_number("size", object.size);
    writer.end_object();
    writer.finish().len()
}

/// Parse one HTTP response body with a hard byte bound.
pub(crate) fn parse_batch_response<R: Read>(
    status: u16,
    mut reader: R,
    request: Option<&LfsBatchRequest>,
) -> Result<LfsBatchResponse, LfsBatchError> {
    if status == 413 {
        return Ok(LfsBatchResponse::Error(LfsBatchErrorResponse {
            status,
            message: None,
            request_id: None,
            retry: LfsBatchRetry::SplitRequest,
            body: LfsBatchBodyDisposition::ConnectionMustClose,
        }));
    }
    let body = read_bounded(&mut reader)?;
    if status != 200 {
        return Ok(LfsBatchResponse::Error(parse_error_response(status, &body)));
    }
    let root = parse_json(&body).map_err(|_| LfsBatchError::InvalidJson)?;
    let object = root.as_object().ok_or(LfsBatchError::InvalidStructure)?;
    if field(object, "objects")?.is_none() {
        return Err(LfsBatchError::InvalidStructure);
    }
    let success = parse_success_response(object, request)?;
    Ok(LfsBatchResponse::Success(success))
}

fn parse_success_response(
    object: &[JsonMember],
    request: Option<&LfsBatchRequest>,
) -> Result<LfsBatchSuccessResponse, LfsBatchError> {
    let transfer = match field(object, "transfer")? {
        Some(value) => parse_transfer(value)?,
        None => LfsTransferAdapter::Basic,
    };
    if let Some(request) = request {
        if !request.transfers.is_empty() && !request.transfers.contains(&transfer) {
            return Err(LfsBatchError::UnsupportedTransfer);
        }
    }
    let hash_algo = match field(object, "hash_algo")? {
        Some(value) => parse_hash_algorithm(value)?,
        None => LfsBatchHashAlgorithm::Sha256,
    };
    let objects = field(object, "objects")?
        .ok_or(LfsBatchError::MissingField("objects"))?
        .as_array()
        .ok_or(LfsBatchError::InvalidField("objects"))?;
    if objects.len() > LFS_BATCH_MAX_OBJECTS {
        return Err(LfsBatchError::TooManyObjects);
    }
    let mut parsed = Vec::with_capacity(objects.len());
    for value in objects {
        parsed.push(parse_response_object(value)?);
    }
    ensure_unique_response_objects(&parsed)?;
    validate_response_objects(request, &parsed)?;
    Ok(LfsBatchSuccessResponse {
        transfer,
        objects: parsed,
        hash_algo,
    })
}

fn parse_response_object(value: &JsonValue) -> Result<LfsBatchResponseObject, LfsBatchError> {
    let object = value.as_object().ok_or(LfsBatchError::InvalidStructure)?;
    let oid = parse_oid(field(object, "oid")?.ok_or(LfsBatchError::MissingField("oid"))?)?;
    let size = parse_size(field(object, "size")?.ok_or(LfsBatchError::MissingField("size"))?)?;
    let authenticated = match field(object, "authenticated")? {
        Some(value) => value
            .as_bool()
            .ok_or(LfsBatchError::InvalidField("authenticated"))?,
        None => false,
    };
    let actions = match field(object, "actions")? {
        Some(value) => parse_actions(value)?,
        None => Vec::new(),
    };
    let error = match field(object, "error")? {
        Some(value) => Some(parse_object_error(value)?),
        None => None,
    };
    if error.is_some() && !actions.is_empty() {
        return Err(LfsBatchError::InvalidField("actions"));
    }
    Ok(LfsBatchResponseObject {
        object: LfsBatchObject::new(oid, size)?,
        authenticated,
        actions,
        error,
    })
}

fn validate_response_objects(
    request: Option<&LfsBatchRequest>,
    objects: &[LfsBatchResponseObject],
) -> Result<(), LfsBatchError> {
    let Some(request) = request else {
        return Ok(());
    };
    if request.objects.len() != objects.len() {
        return Err(LfsBatchError::InvalidStructure);
    }
    for response in objects {
        let Some(expected) = request
            .objects
            .iter()
            .find(|object| object.oid == response.object.oid)
        else {
            return Err(LfsBatchError::InvalidOid);
        };
        if expected.size != response.object.size {
            return Err(LfsBatchError::InvalidSize);
        }
        if response.error.is_none() {
            for action in &response.actions {
                match request.operation {
                    LfsBatchOperation::Download if action.kind != LfsBatchActionKind::Download => {
                        return Err(LfsBatchError::InvalidField("actions"));
                    }
                    LfsBatchOperation::Upload if action.kind == LfsBatchActionKind::Download => {
                        return Err(LfsBatchError::InvalidField("actions"));
                    }
                    _ => {}
                }
            }
            match request.operation {
                LfsBatchOperation::Download => {
                    if !response
                        .actions
                        .iter()
                        .any(|entry| entry.kind == LfsBatchActionKind::Download)
                    {
                        return Err(LfsBatchError::MissingField("actions.download"));
                    }
                }
                LfsBatchOperation::Upload => {
                    if response
                        .actions
                        .iter()
                        .any(|entry| matches!(entry.kind, LfsBatchActionKind::Verify))
                        && !response
                            .actions
                            .iter()
                            .any(|entry| matches!(entry.kind, LfsBatchActionKind::Upload))
                    {
                        return Err(LfsBatchError::MissingField("actions.upload"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn parse_actions(value: &JsonValue) -> Result<Vec<LfsBatchActionEntry>, LfsBatchError> {
    let object = value
        .as_object()
        .ok_or(LfsBatchError::InvalidField("actions"))?;
    let mut actions = Vec::with_capacity(object.len());
    for member in object {
        let kind = match member.key.as_str() {
            "download" => LfsBatchActionKind::Download,
            "upload" => LfsBatchActionKind::Upload,
            "verify" => LfsBatchActionKind::Verify,
            _ => return Err(LfsBatchError::UnsupportedAction),
        };
        if actions
            .iter()
            .any(|entry: &LfsBatchActionEntry| entry.kind == kind)
        {
            return Err(LfsBatchError::DuplicateAction);
        }
        actions.push(LfsBatchActionEntry {
            kind,
            action: parse_action(&member.value)?,
        });
    }
    Ok(actions)
}

fn parse_action(value: &JsonValue) -> Result<LfsBatchAction, LfsBatchError> {
    let object = value.as_object().ok_or(LfsBatchError::InvalidStructure)?;
    let href =
        parse_http_url(required_string(object, "href")?).map_err(|_| LfsBatchError::InvalidUrl)?;
    let headers = match field(object, "header")? {
        Some(value) => parse_headers(value)?,
        None => LfsBatchHeaders::empty(),
    };
    let expires_in = match field(object, "expires_in")? {
        Some(value) => Some(parse_expiry_seconds(value)?),
        None => None,
    };
    let expires_at = match field(object, "expires_at")? {
        Some(value) => {
            let expiry = value.as_str().ok_or(LfsBatchError::InvalidExpiry)?;
            validate_expires_at(expiry)?;
            Some(expiry.to_owned())
        }
        None => None,
    };
    Ok(LfsBatchAction {
        href,
        headers,
        expires_in,
        expires_at,
    })
}

fn parse_headers(value: &JsonValue) -> Result<LfsBatchHeaders, LfsBatchError> {
    let object = value
        .as_object()
        .ok_or(LfsBatchError::InvalidField("header"))?;
    if object.len() > LFS_BATCH_MAX_HEADERS {
        return Err(LfsBatchError::TooManyHeaders);
    }
    let mut headers = Vec::with_capacity(object.len());
    for member in object {
        let value = member.value.as_str().ok_or(LfsBatchError::InvalidHeader)?;
        headers.push(LfsBatchHeader::new_secret(
            member.key.clone(),
            value.as_bytes().to_vec(),
        )?);
    }
    LfsBatchHeaders::new(headers)
}

fn parse_object_error(value: &JsonValue) -> Result<LfsBatchObjectError, LfsBatchError> {
    let object = value
        .as_object()
        .ok_or(LfsBatchError::InvalidField("error"))?;
    let code =
        parse_integer(field(object, "code")?.ok_or(LfsBatchError::MissingField("error.code"))?)
            .map_err(|_| LfsBatchError::InvalidField("error.code"))?;
    let code = u16::try_from(code).map_err(|_| LfsBatchError::InvalidField("error.code"))?;
    let message = required_string(object, "message")?;
    let message = sanitize_server_text(message);
    Ok(LfsBatchObjectError { code, message })
}

fn parse_error_response(status: u16, body: &[u8]) -> LfsBatchErrorResponse {
    let (message, request_id) = parse_json(body)
        .ok()
        .and_then(|root| root.as_object().map(parse_error_object_unchecked))
        .and_then(|result| result.ok())
        .map(|error| (error.message, error.request_id))
        .unwrap_or((None, None));
    LfsBatchErrorResponse {
        status,
        message,
        request_id,
        retry: if status == 413 {
            LfsBatchRetry::SplitRequest
        } else {
            LfsBatchRetry::None
        },
        body: LfsBatchBodyDisposition::Consumed,
    }
}

fn parse_error_object(
    status: u16,
    object: &[JsonMember],
) -> Result<LfsBatchErrorResponse, LfsBatchError> {
    let parsed = parse_error_object_unchecked(object)?;
    Ok(LfsBatchErrorResponse {
        status,
        message: parsed.message,
        request_id: parsed.request_id,
        retry: LfsBatchRetry::None,
        body: LfsBatchBodyDisposition::Consumed,
    })
}

struct ParsedErrorObject {
    message: Option<String>,
    request_id: Option<String>,
}

fn parse_error_object_unchecked(object: &[JsonMember]) -> Result<ParsedErrorObject, LfsBatchError> {
    let message = match field(object, "message")? {
        Some(value) => Some(sanitize_server_text(
            value
                .as_str()
                .ok_or(LfsBatchError::InvalidField("message"))?,
        )),
        None => None,
    };
    let request_id = match field(object, "request_id")? {
        Some(value) => {
            let value = value
                .as_str()
                .ok_or(LfsBatchError::InvalidField("request_id"))?;
            validate_bounded_text(value, 1024, true)
                .map_err(|_| LfsBatchError::InvalidField("request_id"))?;
            Some(value.to_owned())
        }
        None => None,
    };
    if message.is_none() && request_id.is_none() && field(object, "documentation_url")?.is_none() {
        return Err(LfsBatchError::InvalidStructure);
    }
    Ok(ParsedErrorObject {
        message,
        request_id,
    })
}

fn parse_transfer(value: &JsonValue) -> Result<LfsTransferAdapter, LfsBatchError> {
    match value.as_str() {
        Some("basic") => Ok(LfsTransferAdapter::Basic),
        _ => Err(LfsBatchError::UnsupportedTransfer),
    }
}

fn parse_hash_algorithm(value: &JsonValue) -> Result<LfsBatchHashAlgorithm, LfsBatchError> {
    match value.as_str() {
        Some("sha256") => Ok(LfsBatchHashAlgorithm::Sha256),
        _ => Err(LfsBatchError::UnsupportedHashAlgorithm),
    }
}

fn parse_oid(value: &JsonValue) -> Result<LfsOid, LfsBatchError> {
    let value = value.as_str().ok_or(LfsBatchError::InvalidOid)?;
    LfsOid::from_hex(value).map_err(|_| LfsBatchError::InvalidOid)
}

fn parse_size(value: &JsonValue) -> Result<u64, LfsBatchError> {
    let value = parse_integer(value)?;
    if value < 0 || value > i64::MAX {
        return Err(LfsBatchError::InvalidSize);
    }
    Ok(value as u64)
}

fn parse_expiry_seconds(value: &JsonValue) -> Result<i64, LfsBatchError> {
    let value = parse_integer(value).map_err(|_| LfsBatchError::InvalidExpiry)?;
    if !(-MAX_EXPIRY_SECONDS..=MAX_EXPIRY_SECONDS).contains(&value) {
        return Err(LfsBatchError::InvalidExpiry);
    }
    Ok(value)
}

fn parse_integer(value: &JsonValue) -> Result<i64, LfsBatchError> {
    let number = value.as_number().ok_or(LfsBatchError::InvalidSize)?;
    number
        .parse::<i64>()
        .map_err(|_| LfsBatchError::InvalidSize)
}

fn validate_expires_at(value: &str) -> Result<(), LfsBatchError> {
    validate_bounded_text(value, 64, false).map_err(|_| LfsBatchError::InvalidExpiry)?;
    let bytes = value.as_bytes();
    let (date_end, offset_start) = if bytes.len() == 20 {
        if bytes[10] != b'T' || bytes[19] != b'Z' {
            return Err(LfsBatchError::InvalidExpiry);
        }
        (19, None)
    } else if bytes.len() == 25 {
        if bytes[10] != b'T' || (bytes[19] != b'+' && bytes[19] != b'-') || bytes[22] != b':' {
            return Err(LfsBatchError::InvalidExpiry);
        }
        (19, Some(19))
    } else {
        return Err(LfsBatchError::InvalidExpiry);
    };
    for (index, byte) in bytes.iter().enumerate() {
        let is_digit = byte.is_ascii_digit();
        let allowed = if index < date_end {
            is_digit
                || matches!(index, 4 | 7) && *byte == b'-'
                || index == 10
                || matches!(index, 13 | 16) && *byte == b':'
        } else if let Some(offset) = offset_start {
            (index == offset && (*byte == b'+' || *byte == b'-'))
                || index == 22 && *byte == b':'
                || is_digit
        } else {
            index == 19 && *byte == b'Z'
        };
        if !allowed {
            return Err(LfsBatchError::InvalidExpiry);
        }
    }
    let year = decimal_component(&bytes[0..4]);
    let month = decimal_component(&bytes[5..7]);
    let day = decimal_component(&bytes[8..10]);
    let hour = decimal_component(&bytes[11..13]);
    let minute = decimal_component(&bytes[14..16]);
    let second = decimal_component(&bytes[17..19]);
    if year.is_none()
        || !matches!(month, Some(1..=12))
        || !matches!(day, Some(1..=31))
        || !matches!(hour, Some(0..=23))
        || !matches!(minute, Some(0..=59))
        || !matches!(second, Some(0..=60))
    {
        return Err(LfsBatchError::InvalidExpiry);
    }
    if let (Some(year), Some(month), Some(day)) = (year, month, day) {
        if day > days_in_month(year, month) {
            return Err(LfsBatchError::InvalidExpiry);
        }
    }
    if let Some(offset) = offset_start {
        let hours = decimal_component(&bytes[offset + 1..offset + 3]);
        let minutes = decimal_component(&bytes[offset + 4..offset + 6]);
        if !matches!(hours, Some(0..=23)) || !matches!(minutes, Some(0..=59)) {
            return Err(LfsBatchError::InvalidExpiry);
        }
    }
    Ok(())
}

fn decimal_component(bytes: &[u8]) -> Option<u32> {
    if bytes.iter().all(|byte| byte.is_ascii_digit()) {
        Some(
            bytes
                .iter()
                .fold(0_u32, |value, byte| value * 10 + u32::from(byte - b'0')),
        )
    } else {
        None
    }
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn validate_header_name(value: &str) -> Result<(), LfsBatchError> {
    if value.is_empty() || value.len() > LFS_BATCH_MAX_HEADER_NAME_BYTES {
        return Err(LfsBatchError::InvalidHeader);
    }
    if !value.bytes().all(is_header_name_byte) {
        return Err(LfsBatchError::InvalidHeader);
    }
    if is_forbidden_action_header(value) {
        return Err(LfsBatchError::InvalidHeader);
    }
    Ok(())
}

fn is_forbidden_action_header(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
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

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn validate_header_value(value: &str) -> Result<(), LfsBatchError> {
    if value.len() > LFS_BATCH_MAX_HEADER_VALUE_BYTES
        || !value
            .bytes()
            .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte))
    {
        return Err(LfsBatchError::InvalidHeader);
    }
    Ok(())
}

fn validate_bounded_text(value: &str, max_bytes: usize, allow_empty: bool) -> Result<(), ()> {
    if (!allow_empty && value.is_empty()) || value.len() > max_bytes {
        return Err(());
    }
    if value.chars().any(char::is_control) {
        return Err(());
    }
    Ok(())
}

fn validate_ref_name(value: &str) -> Result<(), LfsBatchError> {
    validate_bounded_text(value, LFS_BATCH_MAX_REF_BYTES, false)
        .map_err(|_| LfsBatchError::InvalidField("ref.name"))?;
    if matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Ok(());
    }
    if value == "@" || !value.starts_with("refs/") {
        return Err(LfsBatchError::InvalidField("ref.name"));
    }
    if value.ends_with('/')
        || value.ends_with('.')
        || value.contains("..")
        || value.contains("@{")
        || value.contains(['~', '^', ':', '?', '*', '[', '\\'])
        || value.contains("//")
        || value.chars().any(|character| character == ' ')
    {
        return Err(LfsBatchError::InvalidField("ref.name"));
    }

    // Git checks each slash-delimited component independently.  In
    // particular, `.hidden` and `release.lock` are invalid even when they
    // are not the final component; a dot before another component (for
    // example `feature./topic`) is otherwise accepted by git-check-ref-format.
    if value.split('/').any(|component| {
        component.is_empty()
            || component == "."
            || component == ".."
            || component.starts_with('.')
            || component.ends_with(".lock")
    }) {
        return Err(LfsBatchError::InvalidField("ref.name"));
    }
    Ok(())
}

fn sanitize_server_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if value.contains("://")
        || [
            "authorization",
            "bearer",
            "basic ",
            "credential",
            "cookie",
            "password",
            "secret",
            "token",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return "server rejected the LFS batch request".to_owned();
    }
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

fn lower_ascii(value: &str) -> String {
    value
        .bytes()
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect()
}

fn ensure_unique_objects(objects: &[LfsBatchObject]) -> Result<(), LfsBatchError> {
    let mut seen = HashSet::with_capacity(objects.len());
    for object in objects {
        if !seen.insert(object.oid) {
            return Err(LfsBatchError::DuplicateObject);
        }
    }
    Ok(())
}

fn ensure_unique_response_objects(objects: &[LfsBatchResponseObject]) -> Result<(), LfsBatchError> {
    let mut seen = HashSet::with_capacity(objects.len());
    for object in objects {
        if !seen.insert(object.object.oid) {
            return Err(LfsBatchError::DuplicateObject);
        }
    }
    Ok(())
}

fn read_bounded<R: Read>(reader: &mut R) -> Result<Vec<u8>, LfsBatchError> {
    let mut body = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| LfsBatchError::Read(error.kind()))?;
        if read == 0 {
            return Ok(body);
        }
        if body.len().saturating_add(read) > LFS_BATCH_MAX_BODY_BYTES {
            return Err(LfsBatchError::BodyTooLarge);
        }
        body.extend_from_slice(&buffer[..read]);
    }
}

#[derive(Debug, Clone, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<JsonMember>),
}

#[derive(Debug, Clone, PartialEq)]
struct JsonMember {
    key: String,
    value: JsonValue,
}

impl JsonValue {
    fn as_object(&self) -> Option<&[JsonMember]> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<&str> {
        match self {
            Self::Number(value) => Some(value),
            _ => None,
        }
    }
}

fn field<'a>(
    object: &'a [JsonMember],
    key: &'static str,
) -> Result<Option<&'a JsonValue>, LfsBatchError> {
    let mut result = None;
    for member in object {
        if member.key == key {
            if result.is_some() {
                return Err(LfsBatchError::DuplicateField(key));
            }
            result = Some(&member.value);
        }
    }
    Ok(result)
}

fn required_string<'a>(
    object: &'a [JsonMember],
    key: &'static str,
) -> Result<&'a str, LfsBatchError> {
    field(object, key)?
        .ok_or(LfsBatchError::MissingField(key))?
        .as_str()
        .ok_or(LfsBatchError::InvalidField(key))
}

struct JsonParser<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    fn parse(mut self) -> Result<JsonValue, JsonParseError> {
        let value = self.value(0)?;
        self.whitespace();
        if self.position != self.input.len() {
            return Err(JsonParseError);
        }
        Ok(value)
    }

    fn value(&mut self, depth: usize) -> Result<JsonValue, JsonParseError> {
        if depth > MAX_JSON_DEPTH {
            return Err(JsonParseError);
        }
        self.whitespace();
        let Some(byte) = self.input.get(self.position).copied() else {
            return Err(JsonParseError);
        };
        match byte {
            b'n' => self.literal(b"null", JsonValue::Null),
            b't' => self.literal(b"true", JsonValue::Bool(true)),
            b'f' => self.literal(b"false", JsonValue::Bool(false)),
            b'"' => self.string().map(JsonValue::String),
            b'[' => self.array(depth + 1),
            b'{' => self.object(depth + 1),
            b'-' | b'0'..=b'9' => self.number().map(JsonValue::Number),
            _ => Err(JsonParseError),
        }
    }

    fn literal(&mut self, literal: &[u8], value: JsonValue) -> Result<JsonValue, JsonParseError> {
        if self.input.get(self.position..self.position + literal.len()) != Some(literal) {
            return Err(JsonParseError);
        }
        self.position += literal.len();
        Ok(value)
    }

    fn string(&mut self) -> Result<String, JsonParseError> {
        if self.input.get(self.position) != Some(&b'"') {
            return Err(JsonParseError);
        }
        self.position += 1;
        let mut value = String::new();
        loop {
            let Some(byte) = self.input.get(self.position).copied() else {
                return Err(JsonParseError);
            };
            match byte {
                b'"' => {
                    self.position += 1;
                    return Ok(value);
                }
                b'\\' => {
                    self.position += 1;
                    self.escape(&mut value)?;
                }
                byte if byte < 0x20 => return Err(JsonParseError),
                _ => {
                    let start = self.position;
                    while let Some(byte) = self.input.get(self.position).copied() {
                        if byte == b'"' || byte == b'\\' || byte < 0x20 {
                            break;
                        }
                        self.position += 1;
                    }
                    let chunk = std::str::from_utf8(&self.input[start..self.position])
                        .map_err(|_| JsonParseError)?;
                    value.push_str(chunk);
                }
            }
            if value.len() > LFS_BATCH_MAX_STRING_BYTES {
                return Err(JsonParseError);
            }
        }
    }

    fn escape(&mut self, value: &mut String) -> Result<(), JsonParseError> {
        let Some(byte) = self.input.get(self.position).copied() else {
            return Err(JsonParseError);
        };
        self.position += 1;
        let character = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{0008}',
            b'f' => '\u{000c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(value),
            _ => return Err(JsonParseError),
        };
        value.push(character);
        Ok(())
    }

    fn unicode_escape(&mut self, value: &mut String) -> Result<(), JsonParseError> {
        let high = self.hex_quad()?;
        let codepoint = if (0xd800..=0xdbff).contains(&high) {
            if self.input.get(self.position..self.position + 2) != Some(b"\\u") {
                return Err(JsonParseError);
            }
            self.position += 2;
            let low = self.hex_quad()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err(JsonParseError);
            }
            0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(low) - 0xdc00)
        } else if (0xdc00..=0xdfff).contains(&high) {
            return Err(JsonParseError);
        } else {
            u32::from(high)
        };
        let Some(character) = char::from_u32(codepoint) else {
            return Err(JsonParseError);
        };
        value.push(character);
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u16, JsonParseError> {
        let end = self.position.checked_add(4).ok_or(JsonParseError)?;
        let bytes = self.input.get(self.position..end).ok_or(JsonParseError)?;
        let mut value = 0_u16;
        for byte in bytes {
            value = value
                .checked_mul(16)
                .and_then(|value| value.checked_add(hex_value(*byte)?))
                .ok_or(JsonParseError)?;
        }
        self.position = end;
        Ok(value)
    }

    fn number(&mut self) -> Result<String, JsonParseError> {
        let start = self.position;
        if self.input.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        match self.input.get(self.position).copied() {
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
            _ => return Err(JsonParseError),
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
                return Err(JsonParseError);
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
                return Err(JsonParseError);
            }
        }
        let number = std::str::from_utf8(&self.input[start..self.position])
            .map_err(|_| JsonParseError)?
            .to_owned();
        if number.len() > 64 {
            return Err(JsonParseError);
        }
        Ok(number)
    }

    fn array(&mut self, depth: usize) -> Result<JsonValue, JsonParseError> {
        self.position += 1;
        let mut values = Vec::new();
        self.whitespace();
        if self.input.get(self.position) == Some(&b']') {
            self.position += 1;
            return Ok(JsonValue::Array(values));
        }
        loop {
            if values.len() >= MAX_JSON_ARRAY_ITEMS {
                return Err(JsonParseError);
            }
            values.push(self.value(depth)?);
            self.whitespace();
            match self.input.get(self.position).copied() {
                Some(b',') => {
                    self.position += 1;
                    self.whitespace();
                }
                Some(b']') => {
                    self.position += 1;
                    return Ok(JsonValue::Array(values));
                }
                _ => return Err(JsonParseError),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsonValue, JsonParseError> {
        self.position += 1;
        let mut members = Vec::new();
        self.whitespace();
        if self.input.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Ok(JsonValue::Object(members));
        }
        loop {
            if members.len() >= MAX_JSON_FIELDS {
                return Err(JsonParseError);
            }
            let key = self.string()?;
            self.whitespace();
            if self.input.get(self.position) != Some(&b':') {
                return Err(JsonParseError);
            }
            self.position += 1;
            let value = self.value(depth)?;
            if members.iter().any(|member| member.key == key) {
                return Err(JsonParseError);
            }
            members.push(JsonMember { key, value });
            self.whitespace();
            match self.input.get(self.position).copied() {
                Some(b',') => {
                    self.position += 1;
                    self.whitespace();
                }
                Some(b'}') => {
                    self.position += 1;
                    return Ok(JsonValue::Object(members));
                }
                _ => return Err(JsonParseError),
            }
        }
    }

    fn whitespace(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        {
            self.position += 1;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct JsonParseError;

fn parse_json(input: &[u8]) -> Result<JsonValue, JsonParseError> {
    JsonParser::new(input).parse()
}

fn hex_value(byte: u8) -> Option<u16> {
    match byte {
        b'0'..=b'9' => Some(u16::from(byte - b'0')),
        b'a'..=b'f' => Some(u16::from(byte - b'a' + 10)),
        b'A'..=b'F' => Some(u16::from(byte - b'A' + 10)),
        _ => None,
    }
}

struct JsonWriter {
    output: Vec<u8>,
}

impl JsonWriter {
    fn new() -> Self {
        Self { output: Vec::new() }
    }

    fn len(&self) -> usize {
        self.output.len()
    }

    fn begin_object(&mut self) {
        self.output.push(b'{');
    }

    fn end_object(&mut self) {
        self.output.push(b'}');
    }

    fn begin_array(&mut self) {
        self.output.push(b'[');
    }

    fn end_array(&mut self) {
        self.output.push(b']');
    }

    fn comma(&mut self) {
        self.output.push(b',');
    }

    fn key(&mut self, key: &str) {
        self.string(key);
        self.output.push(b':');
    }

    fn field_string(&mut self, key: &str, value: &str) {
        if !self.output.ends_with(b"{") {
            self.comma();
        }
        self.key(key);
        self.string(value);
    }

    fn field_array_start(&mut self, key: &str) {
        if !self.output.ends_with(b"{") {
            self.comma();
        }
        self.key(key);
        self.begin_array();
    }

    fn field_number(&mut self, key: &str, value: u64) {
        self.key(key);
        self.output.extend_from_slice(value.to_string().as_bytes());
    }

    fn string(&mut self, value: &str) {
        self.output.push(b'"');
        for byte in value.bytes() {
            match byte {
                b'"' => self.output.extend_from_slice(b"\\\""),
                b'\\' => self.output.extend_from_slice(b"\\\\"),
                b'\n' => self.output.extend_from_slice(b"\\n"),
                b'\r' => self.output.extend_from_slice(b"\\r"),
                b'\t' => self.output.extend_from_slice(b"\\t"),
                byte if byte < 0x20 => {
                    let escaped = format!("\\u{byte:04x}");
                    self.output.extend_from_slice(escaped.as_bytes());
                }
                byte => self.output.push(byte),
            }
        }
        self.output.push(b'"');
    }

    fn finish(self) -> Vec<u8> {
        self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn oid(byte: u8) -> LfsOid {
        LfsOid::from_hex(&String::from_utf8(vec![byte; 64]).expect("hex")).expect("oid")
    }

    fn request(operation: LfsBatchOperation) -> LfsBatchRequest {
        LfsBatchRequest::new(
            operation,
            vec![LfsBatchObject::new(oid(b'a'), 123).expect("object")],
        )
        .expect("request")
    }

    #[test]
    fn request_serialization_is_deterministic() {
        let request = request(LfsBatchOperation::Download);
        let expected = br#"{"operation":"download","transfers":["basic"],"objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":123}],"hash_algo":"sha256"}"#;
        assert_eq!(serialize_batch_request(&request).expect("json"), expected);
        assert_eq!(
            serialize_batch_request(&request),
            serialize_batch_request(&request)
        );
    }

    #[test]
    fn parses_download_action_and_headers() {
        let request = request(LfsBatchOperation::Download);
        let body = br#"{"transfer":"basic","objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":123,"authenticated":true,"actions":{"download":{"href":"https://example.test/object","header":{"z":"last","Authorization":"Bearer token"},"expires_in":86400,"expires_at":"2026-08-22T12:00:00Z"}}}],"hash_algo":"sha256"}"#;
        let parsed = parse_batch_response(200, Cursor::new(body), Some(&request)).expect("parse");
        let LfsBatchResponse::Success(success) = parsed else {
            panic!("expected success")
        };
        assert_eq!(success.transfer(), LfsTransferAdapter::Basic);
        let object = &success.objects()[0];
        assert!(object.authenticated());
        assert_eq!(object.actions()[0].action().headers().entries().len(), 2);
        assert!(
            object.actions()[0]
                .action()
                .headers()
                .entries()
                .iter()
                .all(LfsBatchHeader::is_secret)
        );
        assert_eq!(object.actions()[0].action().expires_in(), Some(86400));
    }

    #[test]
    fn parses_upload_and_optional_verify() {
        let request = request(LfsBatchOperation::Upload);
        let body = br#"{"objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":123,"actions":{"upload":{"href":"https://upload.test/u"},"verify":{"href":"https://upload.test/v"}}}]}"#;
        let parsed = parse_batch_response(200, Cursor::new(body), Some(&request)).expect("parse");
        let LfsBatchResponse::Success(success) = parsed else {
            panic!("expected success")
        };
        assert_eq!(success.objects()[0].actions().len(), 2);
        let verify = LfsVerifyRequest::new(success.objects()[0].object().clone());
        assert_eq!(
            serialize_verify_request(&verify),
            br#"{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":123}"#
        );
    }

    #[test]
    fn preserves_typed_per_object_error_without_raw_url() {
        let request = request(LfsBatchOperation::Download);
        let body = br#"{"objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":123,"error":{"code":404,"message":"missing object"}}]}"#;
        let parsed = parse_batch_response(200, Cursor::new(body), Some(&request)).expect("parse");
        let LfsBatchResponse::Success(success) = parsed else {
            panic!("expected success")
        };
        let error = success.objects()[0].error().expect("object error");
        assert_eq!(error.code(), 404);
        assert_eq!(error.message(), "missing object");
    }

    #[test]
    fn rejects_duplicate_case_insensitive_headers() {
        let request = request(LfsBatchOperation::Download);
        let body = br#"{"objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":123,"actions":{"download":{"href":"https://example.test","header":{"Authorization":"one","authorization":"two"}}}}]}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(body), Some(&request)),
            Err(LfsBatchError::DuplicateHeader)
        );
    }

    #[test]
    fn rejects_duplicate_actions_and_unknown_transfer() {
        let request = request(LfsBatchOperation::Download);
        let duplicate = br#"{"objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":123,"actions":{"download":{"href":"https://example.test/a"},"download":{"href":"https://example.test/b"}}}]}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(duplicate), Some(&request)),
            Err(LfsBatchError::InvalidJson)
        );
        let unknown = br#"{"transfer":"tus","objects":[]}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(unknown), None),
            Err(LfsBatchError::UnsupportedTransfer)
        );
    }

    #[test]
    fn rejects_url_credentials_fragment_and_non_http() {
        for url in [
            "https://user@example.test/object",
            "https://example.test/object#fragment",
            "file:///tmp/object",
        ] {
            let body = format!(
                "{{\"objects\":[{{\"oid\":\"{}\",\"size\":123,\"actions\":{{\"download\":{{\"href\":\"{}\"}}}}}}]}}",
                "a".repeat(64),
                url
            );
            assert_eq!(
                parse_batch_response(200, Cursor::new(body.into_bytes()), None),
                Err(LfsBatchError::InvalidUrl)
            );
        }
    }

    #[test]
    fn preserves_signed_query_bytes_through_typed_action_url() {
        let request = request(LfsBatchOperation::Download);
        let href = "HTTPS://[2001:DB8::1]:9443/object?X=1%2f2&sig=a%2Bb";
        let body = format!(
            "{{\"objects\":[{{\"oid\":\"{}\",\"size\":123,\"actions\":{{\"download\":{{\"href\":\"{}\"}}}}}}]}}",
            "a".repeat(64),
            href
        );
        let parsed = parse_batch_response(200, Cursor::new(body.into_bytes()), Some(&request))
            .expect("parse");
        let LfsBatchResponse::Success(success) = parsed else {
            panic!("expected success")
        };
        assert_eq!(success.objects()[0].actions()[0].action().href(), href);
    }

    #[test]
    fn rejects_hop_by_hop_and_authority_action_headers() {
        for header in [
            "Connection",
            "Content-Length",
            "Host",
            "Proxy-Authorization",
            "Transfer-Encoding",
            "X-Forwarded-Host",
        ] {
            assert_eq!(
                LfsBatchHeader::new(header, "value").expect_err("forbidden header"),
                LfsBatchError::InvalidHeader
            );
        }
        assert!(LfsBatchHeader::new("Authorization", "Bearer signed").is_ok());
        assert!(LfsBatchHeader::new("X-Amz-Signature", "signed").is_ok());
    }

    #[test]
    fn accepts_utf8_fully_qualified_refs_and_rejects_invalid_grammar() {
        let reference = LfsBatchRef::new("refs/heads/функція").expect("UTF-8 ref");
        assert_eq!(reference.name(), "refs/heads/функція");
        assert!(LfsBatchRef::new("refs/heads/feature./topic").is_ok());
        assert!(LfsBatchRef::new("1".repeat(40)).is_ok());
        assert!(LfsBatchRef::new("a".repeat(64)).is_ok());
        for invalid in [
            "heads/main",
            "A111111111111111111111111111111111111111",
            "111111111111111111111111111111111111111",
            "11111111111111111111111111111111111111111",
            "refs/heads/with space",
            "refs/heads/.hidden/main",
            "refs/heads/release.lock/main",
            "refs/heads/./main",
            "refs/heads/a..b",
            "refs/heads/a.lock",
            "refs/heads/a@{b}",
            "refs//heads/main",
        ] {
            assert!(LfsBatchRef::new(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn bounds_body_and_exposes_413_split_signal() {
        let oversized = vec![b' '; LFS_BATCH_MAX_BODY_BYTES + 1];
        assert_eq!(
            parse_batch_response(200, Cursor::new(oversized), None),
            Err(LfsBatchError::BodyTooLarge)
        );
        let response =
            parse_batch_response(413, Cursor::new(Vec::<u8>::new()), None).expect("413 response");
        let LfsBatchResponse::Error(error) = response else {
            panic!("expected error")
        };
        assert_eq!(error.retry(), LfsBatchRetry::SplitRequest);
        assert_eq!(
            error.body_disposition(),
            LfsBatchBodyDisposition::ConnectionMustClose
        );
    }

    #[test]
    fn malformed_success_without_objects_is_not_an_error_response() {
        let body = br#"{"message":"not a valid success"}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(body), None),
            Err(LfsBatchError::InvalidStructure)
        );
    }

    #[test]
    fn duplicate_unknown_json_members_are_rejected() {
        let body = br#"{"objects":[],"future":1,"future":2}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(body), None),
            Err(LfsBatchError::InvalidJson)
        );
    }

    #[test]
    fn split_is_stable_by_count_and_encoded_bytes() {
        let objects = (b'a'..=b'c')
            .map(|byte| LfsBatchObject::new(oid(byte), 1).expect("object"))
            .collect();
        let request = LfsBatchRequest::new(LfsBatchOperation::Upload, objects).expect("request");
        let batches = split_batch_request(&request, 2, LFS_BATCH_MAX_BODY_BYTES).expect("split");
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].objects().len(), 2);
        assert_eq!(batches[1].objects().len(), 1);
        assert_eq!(
            split_batch_request(&request, 2, LFS_BATCH_MAX_BODY_BYTES).expect("split"),
            batches
        );
    }

    #[test]
    fn split_respects_exact_encoded_byte_limit_without_reencoding_prefixes() {
        let objects = vec![
            LfsBatchObject::new(oid(b'a'), 1).expect("object"),
            LfsBatchObject::new(oid(b'b'), 1).expect("object"),
        ];
        let request =
            LfsBatchRequest::new(LfsBatchOperation::Upload, objects.clone()).expect("request");
        let one = LfsBatchRequest::new(LfsBatchOperation::Upload, vec![objects[0].clone()])
            .expect("one object request");
        let one_size = serialize_batch_request(&one)
            .expect("one object JSON")
            .len();
        let batches = split_batch_request(&request, 2, one_size).expect("exact split");
        assert_eq!(batches.len(), 2);
        assert_eq!(
            serialize_batch_request(&batches[0])
                .expect("first JSON")
                .len(),
            one_size
        );
    }

    #[test]
    fn rejects_duplicate_objects_with_linear_seen_set() {
        let object = LfsBatchObject::new(oid(b'a'), 1).expect("object");
        assert_eq!(
            LfsBatchRequest::new(LfsBatchOperation::Upload, vec![object.clone(), object],),
            Err(LfsBatchError::DuplicateObject)
        );

        let body = br#"{"objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1},{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1}]}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(body), None),
            Err(LfsBatchError::DuplicateObject)
        );
    }

    #[test]
    fn server_error_redacts_url_and_authorization_text() {
        let body = br#"{"message":"Authorization failed at https://secret.example/token","request_id":"req-1","documentation_url":"https://secret.example/docs"}"#;
        let response = parse_batch_response(401, Cursor::new(body), None).expect("error");
        let LfsBatchResponse::Error(error) = response else {
            panic!("expected error")
        };
        assert_eq!(
            error.message(),
            Some("server rejected the LFS batch request")
        );
        assert_eq!(error.request_id(), Some("req-1"));
        assert!(!format!("{error:?}").contains("https://secret.example"));
    }

    #[test]
    fn rejects_expiry_and_size_overflows() {
        let request = request(LfsBatchOperation::Download);
        let body = br#"{"objects":[{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":9223372036854775808,"actions":{"download":{"href":"https://example.test","expires_in":2147483648}}}]}"#;
        assert_eq!(
            parse_batch_response(200, Cursor::new(body), Some(&request)),
            Err(LfsBatchError::InvalidSize)
        );
    }
}
