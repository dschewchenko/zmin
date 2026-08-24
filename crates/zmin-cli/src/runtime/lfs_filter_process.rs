//! Git LFS clean/smudge filters and the Git long-running process protocol.
//!
//! The process implementation intentionally advertises only `clean` and
//! `smudge`.  Git's `delay` capability is useful for a transfer queue, but it
//! also requires a correct `list_available_blobs` implementation; advertising
//! it without that queue would be a protocol violation.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use zmin_git_core::{GitHashAlgorithm, GitIgnore, GitObjectHash, ObjectId};

use super::{
    LFS_POINTER_MAX_BYTES, LfsFetchFilter, LfsOid, LfsPointer, LfsPointerError,
    LfsRemoteSelectionPolicy, LfsStore, LfsStoreError, VerifiedLfsReader,
};

const IO_BUFFER_SIZE: usize = 128 * 1024;
const PKT_HEADER_SIZE: usize = 4;
const PKT_MAX_PAYLOAD: usize = 65_516;
const PREFIX_LIMIT: usize = LFS_POINTER_MAX_BYTES;
const MAX_HEADER_BYTES: usize = 8 * 1024;
const MAX_PATH_BYTES: usize = 4 * 1024;
const MAX_REQUEST_HEADERS: usize = 16;
const MAX_PATH_RULES: usize = 256;
const MAX_TEMP_ATTEMPTS: usize = 32;
const TEMP_FILE_MODE: u32 = 0o600;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A filter operation used by [`LfsPathPolicy`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsFilterOperation {
    Clean,
    Smudge,
}

/// A validated Git blob object id carried by an optional filter-process
/// request `blob=` header.
///
/// The wrapper keeps the object kind explicit at the filter boundary while
/// reusing the repository's SHA-1/SHA-256 object-id representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitBlobOid(ObjectId);

impl GitBlobOid {
    fn parse(value: &str) -> LfsFilterResult<Self> {
        let algorithm = match value.len() {
            40 => GitHashAlgorithm::Sha1,
            64 => GitHashAlgorithm::Sha256,
            _ => return Err(invalid_header()),
        };
        if !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(invalid_header());
        }
        ObjectId::from_hex(algorithm, value)
            .map(Self)
            .map_err(|_| invalid_header())
    }

    pub(crate) fn algorithm(&self) -> GitHashAlgorithm {
        self.0.algorithm()
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub(crate) fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

/// Typed context supplied to a missing-object provider.
///
/// The pathname is repository-relative. `treeish` is the optional revision
/// supplied by Git's filter-process request and is borrowed for the duration
/// of the callback so the engine never needs to copy request metadata.
pub(crate) struct LfsFilterContext<'a> {
    pub(crate) pathname: &'a str,
    pub(crate) treeish: Option<&'a str>,
    pub(crate) blob: Option<&'a GitBlobOid>,
    pub(crate) operation: LfsFilterOperation,
}

/// A path rule for LFS include/exclude filtering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsPathRuleKind {
    Include,
    Exclude,
}

/// A validated wildcard path rule.
#[derive(Clone, Debug)]
pub(crate) struct LfsPathRule {
    pattern: String,
    kind: LfsPathRuleKind,
    matcher: GitIgnore,
}

impl LfsPathRule {
    pub(crate) fn new(pattern: String, kind: LfsPathRuleKind) -> LfsFilterResult<Self> {
        validate_pattern(&pattern)?;
        if pattern.starts_with('!') {
            return Err(LfsFilterError::InvalidPattern);
        }
        Ok(Self {
            matcher: GitIgnore::parse(&pattern),
            pattern,
            kind,
        })
    }

    pub(crate) fn include(pattern: String) -> LfsFilterResult<Self> {
        Self::new(pattern, LfsPathRuleKind::Include)
    }

    pub(crate) fn exclude(pattern: String) -> LfsFilterResult<Self> {
        Self::new(pattern, LfsPathRuleKind::Exclude)
    }

    pub(crate) fn pattern(&self) -> &str {
        &self.pattern
    }

    pub(crate) fn kind(&self) -> LfsPathRuleKind {
        self.kind
    }
}

/// Include/exclude and skip-smudge policy.
///
/// Rules use the GitIgnore matcher used by the LFS fetch include and exclude
/// lists (`*`, `?`, classes, and `**`).  The filter receives repository-
/// relative UTF-8 pathnames.
#[derive(Clone, Debug, Default)]
pub(crate) struct LfsPathPolicy {
    skip_smudge: bool,
    rules: Vec<LfsPathRule>,
    fetch_filter: Option<LfsFetchFilter>,
}

impl LfsPathPolicy {
    pub(crate) fn new(skip_smudge: bool) -> Self {
        Self {
            skip_smudge,
            rules: Vec::new(),
            fetch_filter: None,
        }
    }

    /// Build the filter-process policy from the same parsed matcher used by
    /// fetch reachability. This preserves Git LFS include/exclude semantics
    /// and prevents clean/smudge from reparsing configuration differently.
    pub(crate) fn from_fetch_filter(skip_smudge: bool, fetch_filter: LfsFetchFilter) -> Self {
        Self {
            skip_smudge,
            rules: Vec::new(),
            fetch_filter: Some(fetch_filter),
        }
    }

    pub(crate) fn from_environment(
        skip_flag: bool,
        include: Vec<String>,
        exclude: Vec<String>,
    ) -> LfsFilterResult<Self> {
        let environment_skip = parse_skip_smudge(std::env::var("GIT_LFS_SKIP_SMUDGE").ok())?;
        let mut policy = Self::new(skip_flag || environment_skip);
        for pattern in include {
            policy.add_rule(LfsPathRule::include(pattern)?)?;
        }
        for pattern in exclude {
            policy.add_rule(LfsPathRule::exclude(pattern)?)?;
        }
        Ok(policy)
    }

    pub(crate) fn add_rule(&mut self, rule: LfsPathRule) -> LfsFilterResult<()> {
        if self.rules.len() >= MAX_PATH_RULES {
            return Err(LfsFilterError::InvalidPattern);
        }
        self.rules.push(rule);
        Ok(())
    }

    pub(crate) fn skip_smudge(&self) -> bool {
        self.skip_smudge
    }

    fn should_process(&self, operation: LfsFilterOperation, path: &str) -> bool {
        if operation == LfsFilterOperation::Smudge && self.skip_smudge {
            return false;
        }
        // fetchinclude/fetchexclude are download policy.  Clean always turns
        // a non-empty worktree blob into a pointer, regardless of checkout
        // selection rules.
        if operation == LfsFilterOperation::Clean {
            return true;
        }

        if let Some(fetch_filter) = &self.fetch_filter {
            return fetch_filter.allows_bytes(path.as_bytes());
        }

        let has_include = self
            .rules
            .iter()
            .any(|rule| rule.kind == LfsPathRuleKind::Include);
        let included = !has_include
            || self.rules.iter().any(|rule| {
                rule.kind == LfsPathRuleKind::Include
                    && rule.matcher.is_ignored(path.as_bytes(), false)
            });
        let excluded = self.rules.iter().any(|rule| {
            rule.kind == LfsPathRuleKind::Exclude && rule.matcher.is_ignored(path.as_bytes(), false)
        });
        included && !excluded
    }
}

/// Parse the Git LFS skip-smudge environment convention.
pub(crate) fn parse_skip_smudge(value: Option<String>) -> LfsFilterResult<bool> {
    let Some(value) = value else {
        return Ok(false);
    };
    let value = value.trim();
    match value {
        "" | "0" => Ok(false),
        "1" => Ok(true),
        _ if value.eq_ignore_ascii_case("false")
            || value.eq_ignore_ascii_case("no")
            || value.eq_ignore_ascii_case("off") =>
        {
            Ok(false)
        }
        _ if value.eq_ignore_ascii_case("true")
            || value.eq_ignore_ascii_case("yes")
            || value.eq_ignore_ascii_case("on") =>
        {
            Ok(true)
        }
        _ => Err(LfsFilterError::InvalidSkipValue),
    }
}

/// A callback used to fetch a missing local object.
///
/// The callback owns all network policy.  An available outcome means it has
/// published the object through the supplied store; the engine verifies it
/// again before exposing any bytes. `LeavePointer` implements the explicit
/// `lfs.skipdownloaderrors` policy without confusing a skipped transfer with
/// a successfully fetched object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsMissingObjectOutcome {
    Available,
    LeavePointer,
}

pub(crate) trait LfsMissingObjectHandler {
    fn fetch(
        &mut self,
        context: &LfsFilterContext<'_>,
        oid: [u8; 32],
        size: u64,
        store: &LfsStore,
    ) -> LfsFilterResult<LfsMissingObjectOutcome>;
}

/// Sanitized filter/protocol errors.  Input values are deliberately not
/// included in the error payload so callers cannot accidentally print
/// untrusted path or content data to stdout.
#[derive(Debug)]
pub(crate) enum LfsFilterError {
    Io {
        operation: LfsFilterIoOperation,
        kind: io::ErrorKind,
    },
    Pointer(LfsPointerError),
    Store(LfsStoreError),
    Protocol(LfsProtocolError),
    FatalAfterResponse,
    InvalidPath,
    InvalidPattern,
    InvalidSkipValue,
    MissingObject,
    UnsupportedRemoteSelectionPolicy(LfsRemoteSelectionPolicy),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsFilterIoOperation {
    Read,
    Write,
    CreateTemporary,
    SyncTemporary,
    SeekTemporary,
    RemoveTemporary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsProtocolError {
    UnexpectedEof,
    InvalidPacket,
    PacketTooLarge,
    InvalidHandshake,
    InvalidHeader,
    HeaderTooLarge,
    TooManyHeaders,
    MissingHeader,
    DuplicateHeader,
    UnsupportedCommand,
    UnsupportedCapability,
    DelayUnsupported,
    ExtensionsUnsupported,
}

pub(crate) type LfsFilterResult<T> = Result<T, LfsFilterError>;

impl std::fmt::Display for LfsFilterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { operation, kind } => {
                write!(formatter, "LFS filter {operation:?} failed ({kind:?})")
            }
            Self::Pointer(error) => write!(formatter, "invalid LFS pointer ({error})"),
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Protocol(error) => write!(formatter, "invalid LFS filter protocol ({error:?})"),
            Self::FatalAfterResponse => {
                formatter.write_str("LFS filter response failed after output started")
            }
            Self::InvalidPath => formatter.write_str("invalid LFS filter pathname"),
            Self::InvalidPattern => formatter.write_str("invalid LFS path pattern"),
            Self::InvalidSkipValue => formatter.write_str("invalid GIT_LFS_SKIP_SMUDGE value"),
            Self::MissingObject => formatter.write_str("LFS object is not present"),
            Self::UnsupportedRemoteSelectionPolicy(policy) => write!(
                formatter,
                "unsupported LFS multi-remote selection policy (autodetect={}, searchall={})",
                policy.autodetect(),
                policy.search_all()
            ),
        }
    }
}

impl std::error::Error for LfsFilterError {}

impl From<LfsPointerError> for LfsFilterError {
    fn from(error: LfsPointerError) -> Self {
        Self::Pointer(error)
    }
}

impl From<LfsStoreError> for LfsFilterError {
    fn from(error: LfsStoreError) -> Self {
        Self::Store(error)
    }
}

impl LfsFilterError {
    fn is_fatal(&self) -> bool {
        match self {
            Self::Io { .. } => true,
            Self::FatalAfterResponse => true,
            Self::Protocol(LfsProtocolError::DelayUnsupported)
            | Self::Protocol(LfsProtocolError::ExtensionsUnsupported) => false,
            Self::Protocol(_) => true,
            _ => false,
        }
    }
}

/// A reusable clean/smudge engine and Git filter-process server.
pub(crate) struct LfsFilterEngine {
    store: Arc<LfsStore>,
    policy: LfsPathPolicy,
    missing_handler: Option<Box<dyn LfsMissingObjectHandler>>,
}

impl LfsFilterEngine {
    pub(crate) fn new(store: Arc<LfsStore>, policy: LfsPathPolicy) -> Self {
        Self {
            store,
            policy,
            missing_handler: None,
        }
    }

    pub(crate) fn with_missing_handler(
        mut self,
        handler: impl LfsMissingObjectHandler + 'static,
    ) -> Self {
        self.missing_handler = Some(Box::new(handler));
        self
    }

    pub(crate) fn clean<R: Read, W: Write>(
        &mut self,
        pathname: &str,
        source: R,
        output: &mut W,
    ) -> LfsFilterResult<()> {
        validate_path(pathname)?;
        if !self
            .policy
            .should_process(LfsFilterOperation::Clean, pathname)
        {
            return copy_stream(source, output);
        }
        let pointer = self.clean_pointer(source)?;
        output
            .write_all(&pointer)
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))
    }

    pub(crate) fn smudge<R: Read, W: Write>(
        &mut self,
        pathname: &str,
        source: R,
        output: &mut W,
    ) -> LfsFilterResult<()> {
        validate_path(pathname)?;
        let mut spool = SpoolFile::from_reader(source)?;
        if !self
            .policy
            .should_process(LfsFilterOperation::Smudge, pathname)
        {
            spool.rewind()?;
            return copy_stream(spool.reader_mut(), output);
        }

        let (prefix, overflow) = read_prefix(spool.reader_mut())?;
        if !overflow {
            if let Ok(pointer) = LfsPointer::parse_current(&prefix) {
                if !pointer.extensions().is_empty() {
                    return Err(LfsFilterError::Protocol(
                        LfsProtocolError::ExtensionsUnsupported,
                    ));
                }
                let context = LfsFilterContext {
                    pathname,
                    treeish: None,
                    blob: None,
                    operation: LfsFilterOperation::Smudge,
                };
                match self.open_verified(&context, &pointer)? {
                    Some(mut reader) => stream_verified(&mut reader, output)?,
                    None => output
                        .write_all(&prefix)
                        .map_err(|error| io_error(LfsFilterIoOperation::Write, error))?,
                }
                return Ok(());
            }
        }
        spool.rewind()?;
        copy_stream(spool.reader_mut(), output)
    }

    /// Serve one Git long-running filter-process session.
    ///
    /// No delay capability is advertised. A client nevertheless sending
    /// `can-delay=1` or `list_available_blobs` receives a request error after
    /// its content has been drained, and the session remains usable.
    pub(crate) fn serve<R: Read, W: Write>(&mut self, input: R, output: W) -> LfsFilterResult<()> {
        let mut packets = PktReader::new(input);
        let mut writer = PktWriter::new(output);
        if !read_handshake(&mut packets)? {
            return Ok(());
        }
        write_server_greeting(&mut writer)?;
        let client_capabilities = read_capabilities(&mut packets)?;
        if !client_capabilities.clean && !client_capabilities.smudge {
            return Err(LfsFilterError::Protocol(
                LfsProtocolError::UnsupportedCapability,
            ));
        }
        write_server_capabilities(&mut writer, &client_capabilities)?;

        loop {
            let Some(request) = read_request(&mut packets)? else {
                return Ok(());
            };
            let result = self.serve_request(request, &mut packets, &mut writer);
            match result {
                Ok(true) => {}
                Ok(false) => return Ok(()),
                Err(error) => {
                    if error.is_fatal() {
                        return Err(error);
                    }
                    write_error_response(&mut writer)?;
                }
            }
        }
    }

    fn clean_pointer<R: Read>(&self, source: R) -> LfsFilterResult<Vec<u8>> {
        let mut chain = PrefixChainReader::new(source);
        let (prefix, overflow) = chain.detect()?;
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        if !overflow && LfsPointer::parse_current(&prefix).is_ok() {
            return Ok(prefix);
        }
        let object = self.store.ingest_computing(chain)?;
        let oid = LfsOid::from_hex(&hex_oid(&object.oid()))?;
        let pointer = LfsPointer::new(oid, object.size(), Vec::new())?;
        pointer.to_bytes().map_err(LfsFilterError::from)
    }

    fn open_verified(
        &mut self,
        context: &LfsFilterContext<'_>,
        pointer: &LfsPointer,
    ) -> LfsFilterResult<Option<VerifiedLfsReader>> {
        let oid = pointer.oid().bytes();
        let size = pointer.size();
        match self.store.open_verified(oid, size) {
            Ok(reader) => Ok(Some(reader)),
            Err(error) if matches!(&error, LfsStoreError::MissingObject) => {
                let Some(handler) = self.missing_handler.as_mut() else {
                    return Err(LfsFilterError::MissingObject);
                };
                match handler.fetch(context, oid, size, &self.store)? {
                    LfsMissingObjectOutcome::Available => self
                        .store
                        .open_verified(oid, size)
                        .map(Some)
                        .map_err(Into::into),
                    LfsMissingObjectOutcome::LeavePointer => Ok(None),
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    fn serve_request<R: Read, W: Write>(
        &mut self,
        request: Request,
        packets: &mut PktReader<R>,
        writer: &mut PktWriter<W>,
    ) -> LfsFilterResult<bool> {
        match request.command {
            RequestCommand::Abort => {
                write_status(writer, b"abort")?;
                Ok(false)
            }
            RequestCommand::ListAvailableBlobs => {
                Err(LfsFilterError::Protocol(LfsProtocolError::DelayUnsupported))
            }
            RequestCommand::Clean => {
                let mut content = PacketContentReader::new(packets);
                if validate_path(&request.pathname).is_err() {
                    let _drained = SpoolFile::from_reader(&mut content)?;
                    return Err(LfsFilterError::InvalidPath);
                }
                if request.can_delay {
                    let _drained = SpoolFile::from_reader(&mut content)?;
                    return Err(LfsFilterError::Protocol(LfsProtocolError::DelayUnsupported));
                }
                if !self
                    .policy
                    .should_process(LfsFilterOperation::Clean, &request.pathname)
                {
                    let mut spool = SpoolFile::from_reader(&mut content)?;
                    write_status(writer, b"success")?;
                    spool.rewind()?;
                    let mut content_writer = PacketContentWriter::new(writer);
                    copy_stream(spool.reader_mut(), &mut content_writer)?;
                    content_writer.finish()?;
                    write_empty_result(writer)?;
                } else {
                    let pointer = match self.clean_pointer(&mut content) {
                        Ok(pointer) => pointer,
                        Err(error) => {
                            if let Err(drain_error) = drain_content(&mut content) {
                                return Err(drain_error);
                            }
                            return Err(error);
                        }
                    };
                    write_status(writer, b"success")?;
                    if !pointer.is_empty() {
                        writer.write_data(&pointer)?;
                    }
                    writer.write_flush()?;
                    write_empty_result(writer)?;
                }
                Ok(true)
            }
            RequestCommand::Smudge => {
                let mut content = PacketContentReader::new(packets);
                let mut spool = SpoolFile::from_reader(&mut content)?;
                validate_path(&request.pathname)?;
                if request.can_delay {
                    return Err(LfsFilterError::Protocol(LfsProtocolError::DelayUnsupported));
                }
                if !self
                    .policy
                    .should_process(LfsFilterOperation::Smudge, &request.pathname)
                {
                    write_status(writer, b"success")?;
                    spool.rewind()?;
                    let mut content_writer = PacketContentWriter::new(writer);
                    copy_stream(spool.reader_mut(), &mut content_writer)?;
                    content_writer.finish()?;
                    write_empty_result(writer)?;
                    return Ok(true);
                }

                let (prefix, overflow) = read_prefix(spool.reader_mut())?;
                if !overflow {
                    if let Ok(pointer) = LfsPointer::parse_current(&prefix) {
                        if !pointer.extensions().is_empty() {
                            return Err(LfsFilterError::Protocol(
                                LfsProtocolError::ExtensionsUnsupported,
                            ));
                        }
                        let context = LfsFilterContext {
                            pathname: &request.pathname,
                            treeish: request.treeish.as_deref(),
                            blob: request.blob.as_ref(),
                            operation: LfsFilterOperation::Smudge,
                        };
                        let reader = self.open_verified(&context, &pointer)?;
                        write_status(writer, b"success")?;
                        if let Some(mut reader) = reader {
                            let mut content_writer = PacketContentWriter::new(writer);
                            if stream_verified(&mut reader, &mut content_writer).is_err() {
                                return Err(LfsFilterError::FatalAfterResponse);
                            }
                            content_writer.finish()?;
                        } else {
                            writer.write_data(&prefix)?;
                            writer.write_flush()?;
                        }
                        write_empty_result(writer)?;
                        return Ok(true);
                    }
                }

                write_status(writer, b"success")?;
                spool.rewind()?;
                let mut content_writer = PacketContentWriter::new(writer);
                copy_stream(spool.reader_mut(), &mut content_writer)?;
                content_writer.finish()?;
                write_empty_result(writer)?;
                Ok(true)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestCommand {
    Clean,
    Smudge,
    Abort,
    ListAvailableBlobs,
}

struct Request {
    command: RequestCommand,
    pathname: String,
    can_delay: bool,
    treeish: Option<String>,
    blob: Option<GitBlobOid>,
}

struct SpoolFile {
    file: Option<File>,
    path: std::path::PathBuf,
}

impl SpoolFile {
    fn from_reader<R: Read>(mut source: R) -> LfsFilterResult<Self> {
        let (file, path) = create_temp_file()?;
        let mut spool = Self {
            file: Some(file),
            path,
        };
        let mut buffer = [0_u8; IO_BUFFER_SIZE];
        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| io_error(LfsFilterIoOperation::Read, error))?;
            if read == 0 {
                break;
            }
            spool
                .file
                .as_mut()
                .expect("spool file remains open")
                .write_all(&buffer[..read])
                .map_err(|error| io_error(LfsFilterIoOperation::Write, error))?;
        }
        spool
            .file
            .as_mut()
            .expect("spool file remains open")
            .flush()
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))?;
        spool
            .file
            .as_mut()
            .expect("spool file remains open")
            .sync_all()
            .map_err(|error| io_error(LfsFilterIoOperation::SyncTemporary, error))?;
        spool
            .file
            .as_mut()
            .expect("spool file remains open")
            .seek(SeekFrom::Start(0))
            .map_err(|error| io_error(LfsFilterIoOperation::SeekTemporary, error))?;
        Ok(spool)
    }

    fn rewind(&mut self) -> LfsFilterResult<()> {
        self.file
            .as_mut()
            .expect("spool file remains open")
            .seek(SeekFrom::Start(0))
            .map(|_| ())
            .map_err(|error| io_error(LfsFilterIoOperation::SeekTemporary, error))
    }

    fn reader_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("spool file remains open")
    }
}

impl Drop for SpoolFile {
    fn drop(&mut self) {
        let _ = self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

fn create_temp_file() -> LfsFilterResult<(File, std::path::PathBuf)> {
    let directory = std::env::temp_dir();
    let process = std::process::id();
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(".zmin-lfs-filter-{process}-{counter}.tmp"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(TEMP_FILE_MODE);
        }
        match options.open(&path) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(io_error(LfsFilterIoOperation::CreateTemporary, error));
            }
        }
    }
    Err(LfsFilterError::Io {
        operation: LfsFilterIoOperation::CreateTemporary,
        kind: io::ErrorKind::AlreadyExists,
    })
}

struct PktReader<R> {
    reader: R,
}

enum Packet {
    Data(Vec<u8>),
    Flush,
}

impl<R: Read> PktReader<R> {
    fn new(reader: R) -> Self {
        Self { reader }
    }

    fn read_packet(&mut self) -> LfsFilterResult<Option<Packet>> {
        let mut header = [0_u8; PKT_HEADER_SIZE];
        let first = self
            .reader
            .read(&mut header[..1])
            .map_err(|error| io_error(LfsFilterIoOperation::Read, error))?;
        if first == 0 {
            return Ok(None);
        }
        self.reader.read_exact(&mut header[1..]).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                LfsFilterError::Protocol(LfsProtocolError::UnexpectedEof)
            } else {
                io_error(LfsFilterIoOperation::Read, error)
            }
        })?;
        let length = parse_packet_length(header)?;
        if length == 0 {
            return Ok(Some(Packet::Flush));
        }
        if length < PKT_HEADER_SIZE {
            return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidPacket));
        }
        let payload_length = length - PKT_HEADER_SIZE;
        if payload_length > PKT_MAX_PAYLOAD {
            return Err(LfsFilterError::Protocol(LfsProtocolError::PacketTooLarge));
        }
        let mut payload = vec![0_u8; payload_length];
        self.reader
            .read_exact(&mut payload)
            .map_err(|error| io_error(LfsFilterIoOperation::Read, error))?;
        Ok(Some(Packet::Data(payload)))
    }
}

struct PktWriter<W> {
    writer: W,
}

impl<W: Write> PktWriter<W> {
    fn new(writer: W) -> Self {
        Self { writer }
    }

    fn write_data(&mut self, payload: &[u8]) -> LfsFilterResult<()> {
        if payload.len() > PKT_MAX_PAYLOAD {
            return Err(LfsFilterError::Protocol(LfsProtocolError::PacketTooLarge));
        }
        let length = payload
            .len()
            .checked_add(PKT_HEADER_SIZE)
            .ok_or(LfsFilterError::Protocol(LfsProtocolError::PacketTooLarge))?;
        let header = [
            hex_digit(length >> 12),
            hex_digit(length >> 8),
            hex_digit(length >> 4),
            hex_digit(length),
        ];
        self.writer
            .write_all(&header)
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))?;
        self.writer
            .write_all(payload)
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))
    }

    fn write_flush(&mut self) -> LfsFilterResult<()> {
        self.writer
            .write_all(b"0000")
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))?;
        self.writer
            .flush()
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))
    }
}

struct PacketContentReader<'a, R> {
    packets: &'a mut PktReader<R>,
    current: Vec<u8>,
    offset: usize,
    done: bool,
}

impl<'a, R> PacketContentReader<'a, R> {
    fn new(packets: &'a mut PktReader<R>) -> Self {
        Self {
            packets,
            current: Vec::new(),
            offset: 0,
            done: false,
        }
    }
}

impl<R: Read> Read for PacketContentReader<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            if self.offset < self.current.len() {
                let count = (self.current.len() - self.offset).min(output.len());
                output[..count].copy_from_slice(&self.current[self.offset..self.offset + count]);
                self.offset += count;
                return Ok(count);
            }
            if self.done {
                return Ok(0);
            }
            match self.packets.read_packet() {
                Ok(Some(Packet::Data(payload))) => {
                    self.current = payload;
                    self.offset = 0;
                }
                Ok(Some(Packet::Flush)) => {
                    self.done = true;
                    return Ok(0);
                }
                Ok(None) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "LFS content ended before its flush packet",
                    ));
                }
                Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidData, error)),
            }
        }
    }
}

struct PacketContentWriter<'a, W> {
    packets: &'a mut PktWriter<W>,
}

impl<'a, W: Write> PacketContentWriter<'a, W> {
    fn new(packets: &'a mut PktWriter<W>) -> Self {
        Self { packets }
    }

    fn finish(self) -> LfsFilterResult<()> {
        self.packets.write_flush()
    }
}

impl<W: Write> Write for PacketContentWriter<'_, W> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        for chunk in input.chunks(PKT_MAX_PAYLOAD) {
            self.packets
                .write_data(chunk)
                .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        }
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_handshake<R: Read>(packets: &mut PktReader<R>) -> LfsFilterResult<bool> {
    let first = match packets.read_packet()? {
        None => return Ok(false),
        Some(Packet::Data(value)) => value,
        _ => return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHandshake)),
    };
    if strip_optional_lf(&first) != b"git-filter-client" {
        return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHandshake));
    }
    let mut selected_v2 = false;
    let mut count = 0_usize;
    let mut bytes = 0_usize;
    loop {
        match packets.read_packet()? {
            Some(Packet::Data(value)) => {
                count = count
                    .checked_add(1)
                    .ok_or(LfsFilterError::Protocol(LfsProtocolError::TooManyHeaders))?;
                bytes = bytes
                    .checked_add(value.len())
                    .ok_or(LfsFilterError::Protocol(LfsProtocolError::HeaderTooLarge))?;
                if count > MAX_REQUEST_HEADERS || bytes > MAX_HEADER_BYTES {
                    return Err(LfsFilterError::Protocol(LfsProtocolError::HeaderTooLarge));
                }
                if strip_optional_lf(&value) == b"version=2" {
                    selected_v2 = true;
                }
            }
            Some(Packet::Flush) => {
                return if selected_v2 {
                    Ok(true)
                } else {
                    Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHandshake))
                };
            }
            None => {
                return Err(LfsFilterError::Protocol(LfsProtocolError::UnexpectedEof));
            }
        }
    }
}

fn write_server_greeting<W: Write>(writer: &mut PktWriter<W>) -> LfsFilterResult<()> {
    writer.write_data(b"git-filter-server\n")?;
    writer.write_data(b"version=2\n")?;
    writer.write_flush()
}

struct ClientCapabilities {
    clean: bool,
    smudge: bool,
}

fn read_capabilities<R: Read>(packets: &mut PktReader<R>) -> LfsFilterResult<ClientCapabilities> {
    let mut capabilities = ClientCapabilities {
        clean: false,
        smudge: false,
    };
    let mut count = 0_usize;
    let mut bytes = 0_usize;
    loop {
        match packets.read_packet()? {
            Some(Packet::Flush) => return Ok(capabilities),
            Some(Packet::Data(value)) => {
                count = count
                    .checked_add(1)
                    .ok_or(LfsFilterError::Protocol(LfsProtocolError::TooManyHeaders))?;
                bytes = bytes
                    .checked_add(value.len())
                    .ok_or(LfsFilterError::Protocol(LfsProtocolError::HeaderTooLarge))?;
                if count > MAX_REQUEST_HEADERS || bytes > MAX_HEADER_BYTES {
                    return Err(LfsFilterError::Protocol(LfsProtocolError::HeaderTooLarge));
                }
                let value = std::str::from_utf8(&value)
                    .map_err(|_| LfsFilterError::Protocol(LfsProtocolError::InvalidHandshake))?;
                let value = strip_optional_lf(value.as_bytes());
                let value = std::str::from_utf8(value)
                    .map_err(|_| LfsFilterError::Protocol(LfsProtocolError::InvalidHandshake))?;
                match value {
                    "capability=clean" if !capabilities.clean => capabilities.clean = true,
                    "capability=smudge" if !capabilities.smudge => capabilities.smudge = true,
                    "capability=delay" => {}
                    _ => {}
                }
            }
            None => {
                return Err(LfsFilterError::Protocol(LfsProtocolError::UnexpectedEof));
            }
        }
    }
}

fn write_server_capabilities<W: Write>(
    writer: &mut PktWriter<W>,
    client: &ClientCapabilities,
) -> LfsFilterResult<()> {
    if client.clean {
        writer.write_data(b"capability=clean\n")?;
    }
    if client.smudge {
        writer.write_data(b"capability=smudge\n")?;
    }
    writer.write_flush()
}

fn read_request<R: Read>(packets: &mut PktReader<R>) -> LfsFilterResult<Option<Request>> {
    let first = match packets.read_packet()? {
        None => return Ok(None),
        Some(Packet::Flush) => {
            return Err(LfsFilterError::Protocol(LfsProtocolError::MissingHeader));
        }
        Some(Packet::Data(value)) => value,
    };
    let mut command = None;
    let mut pathname = None;
    let mut can_delay = false;
    let mut can_delay_seen = false;
    let mut treeish = None;
    let mut blob = None;
    let mut count = 0_usize;
    let mut header_bytes = 0_usize;
    let mut packet = Some(first);
    loop {
        let value = if let Some(value) = packet.take() {
            PacketRead::Data(value)
        } else {
            match packets.read_packet() {
                Ok(Some(Packet::Data(value))) => PacketRead::Data(value),
                Ok(Some(Packet::Flush)) => PacketRead::Flush,
                Ok(None) => PacketRead::Eof,
                Err(error) => PacketRead::Error(error),
            }
        };
        let value = match value {
            PacketRead::Data(value) => value,
            PacketRead::Flush => break,
            PacketRead::Eof => {
                return Err(LfsFilterError::Protocol(LfsProtocolError::UnexpectedEof));
            }
            PacketRead::Error(error) => return Err(error),
        };
        count = count
            .checked_add(1)
            .ok_or(LfsFilterError::Protocol(LfsProtocolError::TooManyHeaders))?;
        header_bytes = header_bytes
            .checked_add(value.len())
            .ok_or(LfsFilterError::Protocol(LfsProtocolError::HeaderTooLarge))?;
        if count > MAX_REQUEST_HEADERS || header_bytes > MAX_HEADER_BYTES {
            return Err(LfsFilterError::Protocol(LfsProtocolError::HeaderTooLarge));
        }
        let value = strip_optional_lf(&value);
        let text = std::str::from_utf8(value)
            .map_err(|_| LfsFilterError::Protocol(LfsProtocolError::InvalidHeader))?;
        let (key, value) = text
            .split_once('=')
            .ok_or(LfsFilterError::Protocol(LfsProtocolError::InvalidHeader))?;
        if key.is_empty() {
            return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHeader));
        }
        match key {
            "command" if command.is_none() => {
                command = Some(parse_command(value)?);
            }
            "pathname" if pathname.is_none() => {
                pathname = Some(value.to_owned());
            }
            "can-delay" if !can_delay_seen => {
                can_delay_seen = true;
                can_delay = match value {
                    "0" => false,
                    "1" => true,
                    _ => {
                        return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHeader));
                    }
                };
            }
            "treeish" if treeish.is_none() => {
                if value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n')) {
                    return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHeader));
                }
                treeish = Some(value.to_owned());
            }
            "blob" if blob.is_none() => {
                blob = Some(GitBlobOid::parse(value)?);
            }
            "command" | "pathname" | "can-delay" | "treeish" | "blob" => {
                return Err(LfsFilterError::Protocol(LfsProtocolError::DuplicateHeader));
            }
            _ => return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHeader)),
        }
        packet = None;
    }
    let command = command.ok_or(LfsFilterError::Protocol(LfsProtocolError::MissingHeader))?;
    if matches!(command, RequestCommand::Clean | RequestCommand::Smudge) && pathname.is_none() {
        return Err(LfsFilterError::Protocol(LfsProtocolError::MissingHeader));
    }
    Ok(Some(Request {
        command,
        pathname: pathname.unwrap_or_default(),
        can_delay,
        treeish,
        blob,
    }))
}

enum PacketRead {
    Data(Vec<u8>),
    Flush,
    Eof,
    Error(LfsFilterError),
}

fn parse_command(value: &str) -> LfsFilterResult<RequestCommand> {
    match value {
        "clean" => Ok(RequestCommand::Clean),
        "smudge" => Ok(RequestCommand::Smudge),
        "abort" => Ok(RequestCommand::Abort),
        "list_available_blobs" => Ok(RequestCommand::ListAvailableBlobs),
        _ => Err(LfsFilterError::Protocol(
            LfsProtocolError::UnsupportedCommand,
        )),
    }
}

fn invalid_header() -> LfsFilterError {
    LfsFilterError::Protocol(LfsProtocolError::InvalidHeader)
}

fn write_status<W: Write>(writer: &mut PktWriter<W>, status: &[u8]) -> LfsFilterResult<()> {
    let mut value = [0_u8; 16];
    let prefix = b"status=";
    let total = prefix.len() + status.len() + 1;
    if total > value.len() {
        return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidPacket));
    }
    value[..prefix.len()].copy_from_slice(prefix);
    value[prefix.len()..prefix.len() + status.len()].copy_from_slice(status);
    value[prefix.len() + status.len()] = b'\n';
    writer.write_data(&value[..total])?;
    writer.write_flush()
}

fn write_error_response<W: Write>(writer: &mut PktWriter<W>) -> LfsFilterResult<()> {
    write_status(writer, b"error")
}

fn write_empty_result<W: Write>(writer: &mut PktWriter<W>) -> LfsFilterResult<()> {
    writer.write_flush()
}

fn parse_packet_length(header: [u8; PKT_HEADER_SIZE]) -> LfsFilterResult<usize> {
    let mut length = 0_usize;
    for digit in header {
        let value = match digit {
            b'0'..=b'9' => usize::from(digit - b'0'),
            b'a'..=b'f' => usize::from(digit - b'a' + 10),
            _ => return Err(LfsFilterError::Protocol(LfsProtocolError::InvalidPacket)),
        };
        length = (length << 4) | value;
    }
    Ok(length)
}

fn hex_digit(value: usize) -> u8 {
    match value & 0x0f {
        value @ 0..=9 => b'0' + value as u8,
        value => b'a' + (value as u8 - 10),
    }
}

fn read_prefix<R: Read>(source: &mut R) -> LfsFilterResult<(Vec<u8>, bool)> {
    let mut prefix = Vec::with_capacity(PREFIX_LIMIT);
    let mut buffer = [0_u8; PREFIX_LIMIT];
    while prefix.len() < PREFIX_LIMIT {
        let remaining = PREFIX_LIMIT - prefix.len();
        let read = source
            .read(&mut buffer[..remaining])
            .map_err(|error| io_error(LfsFilterIoOperation::Read, error))?;
        if read == 0 {
            return Ok((prefix, false));
        }
        prefix.extend_from_slice(&buffer[..read]);
    }
    Ok((prefix, true))
}

/// A bounded look-ahead reader. Detection consumes at most one pointer-sized
/// prefix and then replays that prefix before reading the underlying source,
/// allowing clean to hash/store non-pointer input in one pass.
struct PrefixChainReader<R> {
    source: R,
    prefix: Vec<u8>,
    offset: usize,
    detected: bool,
    overflow: bool,
}

impl<R> PrefixChainReader<R> {
    fn new(source: R) -> Self {
        Self {
            source,
            prefix: Vec::with_capacity(PREFIX_LIMIT),
            offset: 0,
            detected: false,
            overflow: false,
        }
    }

    fn detect(&mut self) -> LfsFilterResult<(Vec<u8>, bool)>
    where
        R: Read,
    {
        if !self.detected {
            let mut buffer = [0_u8; PREFIX_LIMIT];
            while self.prefix.len() < PREFIX_LIMIT {
                let remaining = PREFIX_LIMIT - self.prefix.len();
                let read = self
                    .source
                    .read(&mut buffer[..remaining])
                    .map_err(|error| io_error(LfsFilterIoOperation::Read, error))?;
                if read == 0 {
                    break;
                }
                self.prefix.extend_from_slice(&buffer[..read]);
            }
            self.overflow = self.prefix.len() == PREFIX_LIMIT;
            self.detected = true;
        }
        Ok((self.prefix.clone(), self.overflow))
    }
}

impl<R: Read> Read for PrefixChainReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.offset < self.prefix.len() {
            let count = (self.prefix.len() - self.offset).min(output.len());
            output[..count].copy_from_slice(&self.prefix[self.offset..self.offset + count]);
            self.offset += count;
            return Ok(count);
        }
        self.source.read(output)
    }
}

fn strip_optional_lf(value: &[u8]) -> &[u8] {
    value.strip_suffix(b"\n").unwrap_or(value)
}

fn copy_stream<R: Read, W: Write>(mut source: R, output: &mut W) -> LfsFilterResult<()> {
    let mut buffer = [0_u8; IO_BUFFER_SIZE];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|error| io_error(LfsFilterIoOperation::Read, error))?;
        if read == 0 {
            return Ok(());
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))?;
    }
}

fn drain_content<R: Read>(source: &mut R) -> LfsFilterResult<()> {
    let mut sink = io::sink();
    copy_stream(source, &mut sink)
}

fn stream_verified<W: Write>(
    reader: &mut VerifiedLfsReader,
    output: &mut W,
) -> LfsFilterResult<()> {
    let mut buffer = [0_u8; IO_BUFFER_SIZE];
    loop {
        let read = reader.read_verified(&mut buffer)?;
        if read == 0 {
            reader.finish()?;
            return Ok(());
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| io_error(LfsFilterIoOperation::Write, error))?;
    }
}

fn object_digest(hasher: GitObjectHash) -> [u8; 32] {
    let object_id = hasher.finalize();
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(object_id.as_bytes());
    digest
}

fn hex_oid(oid: &[u8; 32]) -> String {
    let mut value = String::with_capacity(64);
    for byte in oid {
        value.push(char::from(hex_digit(usize::from(byte >> 4))));
        value.push(char::from(hex_digit(usize::from(byte & 0x0f))));
    }
    value
}

fn validate_path(path: &str) -> LfsFilterResult<()> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(LfsFilterError::InvalidPath);
    }
    Ok(())
}

fn validate_pattern(pattern: &str) -> LfsFilterResult<()> {
    if pattern.is_empty()
        || pattern.len() > MAX_PATH_BYTES
        || pattern
            .bytes()
            .any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(LfsFilterError::InvalidPattern);
    }
    Ok(())
}

fn io_error(operation: LfsFilterIoOperation, error: io::Error) -> LfsFilterError {
    LfsFilterError::Io {
        operation,
        kind: error.kind(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zmin_git_core::GitHashAlgorithm;

    fn store() -> (Arc<LfsStore>, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            ".zmin-lfs-filter-test-{}-{}",
            std::process::id(),
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let store = Arc::new(LfsStore::new(path.clone()).expect("test store"));
        (store, path)
    }

    fn packet(payload: &[u8]) -> Vec<u8> {
        let length = payload.len() + PKT_HEADER_SIZE;
        let mut output = Vec::with_capacity(length);
        output.extend_from_slice(&[
            hex_digit(length >> 12),
            hex_digit(length >> 8),
            hex_digit(length >> 4),
            hex_digit(length),
        ]);
        output.extend_from_slice(payload);
        output
    }

    fn handshake() -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(&packet(b"git-filter-client\n"));
        output.extend_from_slice(&packet(b"version=1\n"));
        output.extend_from_slice(&packet(b"version=2\n"));
        output.extend_from_slice(b"0000");
        output.extend_from_slice(&packet(b"capability=unknown\n"));
        output.extend_from_slice(&packet(b"capability=clean\n"));
        output.extend_from_slice(&packet(b"capability=smudge\n"));
        output.extend_from_slice(b"0000");
        output
    }

    fn request(command: &[u8], pathname: &[u8], content: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(&packet(command));
        output.extend_from_slice(&packet(pathname));
        output.extend_from_slice(b"0000");
        if !content.is_empty() {
            output.extend_from_slice(&packet(content));
        }
        output.extend_from_slice(b"0000");
        output
    }

    fn request_headers(headers: &[&[u8]]) -> LfsFilterResult<Request> {
        let mut input = Vec::new();
        for header in headers {
            input.extend_from_slice(&packet(header));
        }
        input.extend_from_slice(b"0000");
        read_request(&mut PktReader::new(Cursor::new(input)))?
            .ok_or(LfsFilterError::Protocol(LfsProtocolError::MissingHeader))
    }

    #[test]
    fn request_blob_metadata_accepts_sha1_and_sha256() {
        let sha1 = format!("blob={}\n", "a".repeat(40));
        let request =
            request_headers(&[b"command=smudge\n", b"pathname=sha1.bin\n", sha1.as_bytes()])
                .expect("sha1 blob");
        let blob = request.blob.expect("sha1 metadata");
        assert_eq!(blob.algorithm(), GitHashAlgorithm::Sha1);
        assert_eq!(blob.as_bytes().len(), 20);
        assert_eq!(blob.to_hex(), "a".repeat(40));

        let sha256 = format!("blob={}\n", "b".repeat(64));
        let request = request_headers(&[
            b"command=smudge\n",
            b"pathname=sha256.bin\n",
            sha256.as_bytes(),
        ])
        .expect("sha256 blob");
        let blob = request.blob.expect("sha256 metadata");
        assert_eq!(blob.algorithm(), GitHashAlgorithm::Sha256);
        assert_eq!(blob.as_bytes().len(), 32);
        assert_eq!(blob.to_hex(), "b".repeat(64));
    }

    #[test]
    fn request_blob_metadata_rejects_duplicate_header() {
        let first = format!("blob={}\n", "a".repeat(40));
        let second = format!("blob={}\n", "b".repeat(40));
        assert!(matches!(
            request_headers(&[
                b"command=smudge\n",
                b"pathname=duplicate.bin\n",
                first.as_bytes(),
                second.as_bytes(),
            ]),
            Err(LfsFilterError::Protocol(LfsProtocolError::DuplicateHeader))
        ));
    }

    #[test]
    fn request_blob_metadata_rejects_uppercase_wrong_length_nonhex_and_control() {
        let values = [
            format!("blob={}\n", "A".repeat(40)),
            format!("blob={}\n", "a".repeat(39)),
            format!("blob={}\n", "g".repeat(40)),
            format!("blob={}\n", "a".repeat(39) + "\r"),
        ];
        for value in values {
            assert!(matches!(
                request_headers(&[
                    b"command=smudge\n",
                    b"pathname=invalid.bin\n",
                    value.as_bytes(),
                ]),
                Err(LfsFilterError::Protocol(LfsProtocolError::InvalidHeader))
            ));
        }
    }

    #[test]
    fn clean_and_smudge_round_trip_streams_store_object() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let source = b"hello LFS\0binary";
        let mut pointer = Vec::new();
        engine
            .clean("file.bin", Cursor::new(source), &mut pointer)
            .expect("clean");
        assert!(pointer.len() < PREFIX_LIMIT);
        assert!(LfsPointer::parse_strict(&pointer).is_ok());
        let mut output = Vec::new();
        engine
            .smudge("file.bin", Cursor::new(pointer), &mut output)
            .expect("smudge");
        assert_eq!(output, source);
        let mut empty = Vec::new();
        engine
            .clean("empty", Cursor::new(Vec::<u8>::new()), &mut empty)
            .expect("empty clean");
        assert!(empty.is_empty());
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn malformed_and_binary_smudge_inputs_passthrough() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        for input in [b"not a pointer".to_vec(), vec![0, 1, 2, 255], Vec::new()] {
            let mut output = Vec::new();
            engine
                .smudge("file.bin", Cursor::new(input.clone()), &mut output)
                .expect("passthrough");
            assert_eq!(output, input);
        }
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn clean_is_idempotent_for_a_current_pointer() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let input = b"size 7\nversion https://git-lfs.github.com/spec/v1\noid sha256:0000000000000000000000000000000000000000000000000000000000000000";
        let mut output = Vec::new();
        engine
            .clean("already-pointer", Cursor::new(input), &mut output)
            .expect("current pointer");
        assert_eq!(output, input);
        let _ = fs::remove_dir_all(path);
    }

    struct TestMissingHandler {
        source: Vec<u8>,
        called: bool,
    }

    struct SkipMissingHandler;

    impl LfsMissingObjectHandler for TestMissingHandler {
        fn fetch(
            &mut self,
            context: &LfsFilterContext<'_>,
            oid: [u8; 32],
            size: u64,
            store: &LfsStore,
        ) -> LfsFilterResult<LfsMissingObjectOutcome> {
            assert_eq!(context.pathname, "file.bin");
            assert_eq!(context.treeish, None);
            assert_eq!(context.operation, LfsFilterOperation::Smudge);
            self.called = true;
            store
                .ingest(oid, size, Cursor::new(self.source.clone()))
                .map(|_| LfsMissingObjectOutcome::Available)
                .map_err(Into::into)
        }
    }

    impl LfsMissingObjectHandler for SkipMissingHandler {
        fn fetch(
            &mut self,
            _context: &LfsFilterContext<'_>,
            _oid: [u8; 32],
            _size: u64,
            _store: &LfsStore,
        ) -> LfsFilterResult<LfsMissingObjectOutcome> {
            Ok(LfsMissingObjectOutcome::LeavePointer)
        }
    }

    #[test]
    fn missing_object_handler_must_publish_before_smudge() {
        let (store, path) = store();
        let source = b"fetched later".to_vec();
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        hasher.update(&source);
        let oid_bytes = object_digest(hasher);
        let pointer = LfsPointer::new(
            LfsOid::from_hex(&hex_oid(&oid_bytes)).expect("oid"),
            source.len() as u64,
            Vec::new(),
        )
        .expect("pointer")
        .to_bytes()
        .expect("pointer bytes");
        let handler = TestMissingHandler {
            source: source.clone(),
            called: false,
        };
        let mut engine =
            LfsFilterEngine::new(store, LfsPathPolicy::new(false)).with_missing_handler(handler);
        let mut output = Vec::new();
        engine
            .smudge("file.bin", Cursor::new(pointer), &mut output)
            .expect("fetch and smudge");
        assert_eq!(output, source);
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn missing_object_without_handler_fails_closed() {
        let (store, path) = store();
        let source = b"not locally present";
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        hasher.update(source);
        let oid = object_digest(hasher);
        let pointer = LfsPointer::new(
            LfsOid::from_hex(&hex_oid(&oid)).expect("oid"),
            source.len() as u64,
            Vec::new(),
        )
        .expect("pointer")
        .to_bytes()
        .expect("pointer bytes");
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let mut output = Vec::new();
        let error = engine
            .smudge("missing.bin", Cursor::new(pointer), &mut output)
            .expect_err("missing object");
        assert!(matches!(error, LfsFilterError::MissingObject));
        assert!(output.is_empty());
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn skipped_missing_download_leaves_pointer_in_direct_and_process_smudge() {
        let (store, path) = store();
        let pointer = LfsPointer::new(
            LfsOid::from_hex(&"1".repeat(64)).expect("oid"),
            42,
            Vec::new(),
        )
        .expect("pointer")
        .to_bytes()
        .expect("pointer bytes");
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false))
            .with_missing_handler(SkipMissingHandler);

        let mut direct = Vec::new();
        engine
            .smudge("missing.bin", Cursor::new(&pointer), &mut direct)
            .expect("skip direct smudge");
        assert_eq!(direct, pointer);

        let mut input = handshake();
        input.extend_from_slice(&request(
            b"command=smudge\n",
            b"pathname=missing.bin\n",
            &pointer,
        ));
        let mut output = Vec::new();
        engine
            .serve(Cursor::new(input), &mut output)
            .expect("skip process smudge");
        assert!(
            output
                .windows(pointer.len())
                .any(|window| window == pointer)
        );
        assert!(
            output
                .windows(b"status=error\n".len())
                .all(|window| window != b"status=error\n")
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn skip_and_include_exclude_policy_are_typed() {
        assert!(parse_skip_smudge(Some("true".to_owned())).expect("true"));
        assert!(!parse_skip_smudge(Some("0".to_owned())).expect("false"));
        assert!(parse_skip_smudge(Some("maybe".to_owned())).is_err());
        let skip_policy = LfsPathPolicy::new(true);
        assert!(!skip_policy.should_process(LfsFilterOperation::Smudge, "src/a.bin"));
        let mut policy = LfsPathPolicy::new(false);
        policy
            .add_rule(LfsPathRule::include("src/**".to_owned()).expect("include"))
            .expect("include rule");
        policy
            .add_rule(LfsPathRule::exclude("src/private/**".to_owned()).expect("exclude"))
            .expect("exclude rule");
        assert!(policy.should_process(LfsFilterOperation::Smudge, "src/a.bin"));
        assert!(!policy.should_process(LfsFilterOperation::Smudge, "src/private/a.bin"));
        assert!(!policy.should_process(LfsFilterOperation::Smudge, "other/a.bin"));
        assert!(policy.should_process(LfsFilterOperation::Clean, "src/private/a.bin"));
        assert!(policy.should_process(LfsFilterOperation::Clean, "other/a.bin"));
    }

    #[test]
    fn parsed_fetch_filter_preserves_lfs_include_exclude_semantics() {
        let include =
            LfsFetchFilter::from_values(Some("src/**"), Some("src/private/**")).expect("include");
        let include_policy = LfsPathPolicy::from_fetch_filter(false, include);
        assert!(include_policy.should_process(LfsFilterOperation::Smudge, "src/a.bin"));
        assert!(!include_policy.should_process(LfsFilterOperation::Smudge, "src/private/a.bin"));

        let exclude = LfsFetchFilter::from_values(None, Some("vendor/**")).expect("exclude");
        let exclude_policy = LfsPathPolicy::from_fetch_filter(false, exclude);
        assert!(!exclude_policy.should_process(LfsFilterOperation::Smudge, "vendor/a.bin"));
        assert!(!exclude_policy.should_process(LfsFilterOperation::Smudge, "vendor/keep/a.bin"));
        assert!(exclude_policy.should_process(LfsFilterOperation::Clean, "vendor/a.bin"));
    }

    #[test]
    fn process_handshake_and_multiple_requests_use_exact_framing() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let source = b"process content";
        let mut pointer = Vec::new();
        engine
            .clean("file.bin", Cursor::new(source), &mut pointer)
            .expect("seed object");
        let mut input = handshake();
        input.extend_from_slice(&request(b"command=clean\n", b"pathname=file.bin\n", source));
        input.extend_from_slice(&request(
            b"command=smudge\n",
            b"pathname=file.bin\n",
            &pointer,
        ));
        let mut output = Vec::new();
        engine
            .serve(Cursor::new(input), &mut output)
            .expect("filter process");
        assert!(output.starts_with(&packet(b"git-filter-server\n")));
        assert!(
            output
                .windows(b"capability=delay\n".len())
                .all(|window| { window != b"capability=delay\n" })
        );
        assert!(
            output
                .windows(b"status=success\n".len())
                .any(|window| { window == b"status=success\n" })
        );
        assert!(output.windows(source.len()).any(|window| window == source));
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn can_delay_is_rejected_and_error_is_framed() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let mut input = handshake();
        input.extend_from_slice(&packet(b"command=smudge\n"));
        input.extend_from_slice(&packet(b"pathname=file.bin\n"));
        input.extend_from_slice(&packet(b"can-delay=1\n"));
        input.extend_from_slice(b"0000");
        input.extend_from_slice(b"0000");
        let mut output = Vec::new();
        engine
            .serve(Cursor::new(input), &mut output)
            .expect("delay request is recoverable");
        assert!(
            output
                .windows(b"status=error\n".len())
                .any(|window| { window == b"status=error\n" })
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn initial_eof_is_a_graceful_noop_and_uppercase_pkt_hex_is_rejected() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let mut output = Vec::new();
        engine
            .serve(Cursor::new(Vec::<u8>::new()), &mut output)
            .expect("initial EOF");
        assert!(output.is_empty());
        assert!(matches!(
            parse_packet_length(*b"00AF"),
            Err(LfsFilterError::Protocol(LfsProtocolError::InvalidPacket))
        ));
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn recoverable_request_error_does_not_desynchronize_next_request() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let mut input = handshake();
        input.extend_from_slice(&request(
            b"command=smudge\n",
            b"pathname=missing.bin\n",
            &LfsPointer::new(
                LfsOid::from_hex(&"0".repeat(64)).expect("oid"),
                0,
                Vec::new(),
            )
            .expect("pointer")
            .to_bytes()
            .expect("pointer bytes"),
        ));
        input.extend_from_slice(&request(
            b"command=clean",
            b"pathname=next.bin",
            b"next content",
        ));
        let mut output = Vec::new();
        engine
            .serve(Cursor::new(input), &mut output)
            .expect("error is request-scoped");
        assert!(
            output
                .windows(b"status=error\n".len())
                .any(|window| window == b"status=error\n")
        );
        assert_eq!(
            output
                .windows(b"status=success\n".len())
                .filter(|window| *window == b"status=success\n")
                .count(),
            1
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn clean_store_error_after_content_drain_recovers_next_request() {
        let (store, path) = store();
        let first = b"content whose publication is unsafe";
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        hasher.update(first);
        let oid = object_digest(hasher);
        let hex = hex_oid(&oid);
        let unsafe_destination = path.join(&hex[..2]).join(&hex[2..4]).join(&hex);
        fs::create_dir_all(&unsafe_destination).expect("unsafe destination");

        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let mut input = handshake();
        input.extend_from_slice(&request(b"command=clean", b"pathname=unsafe.bin", first));
        input.extend_from_slice(&request(
            b"command=clean",
            b"pathname=after-store-error.bin",
            b"after store error",
        ));
        let mut output = Vec::new();
        engine
            .serve(Cursor::new(input), &mut output)
            .expect("store error is request-scoped");
        assert_eq!(
            output
                .windows(b"status=error\n".len())
                .filter(|window| *window == b"status=error\n")
                .count(),
            1
        );
        assert_eq!(
            output
                .windows(b"status=success\n".len())
                .filter(|window| *window == b"status=success\n")
                .count(),
            1
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn content_drain_io_failure_is_fatal() {
        struct FailingReader;

        impl Read for FailingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "test failure"))
            }
        }

        let error = drain_content(&mut FailingReader).expect_err("drain failure");
        assert!(matches!(
            error,
            LfsFilterError::Io {
                operation: LfsFilterIoOperation::Read,
                ..
            }
        ));
        assert!(error.is_fatal());
    }

    #[test]
    fn smudge_extension_is_rejected_after_content_drain() {
        let (store, path) = store();
        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let pointer = b"version https://git-lfs.github.com/spec/v1\noid sha256:0000000000000000000000000000000000000000000000000000000000000000\nsize 0\next-1-test sha256:0000000000000000000000000000000000000000000000000000000000000000\n";
        let mut input = handshake();
        input.extend_from_slice(&request(
            b"command=smudge\n",
            b"pathname=extended.bin\n",
            pointer,
        ));
        input.extend_from_slice(&request(
            b"command=clean\n",
            b"pathname=after-extension.bin\n",
            b"after extension",
        ));
        let mut output = Vec::new();
        engine
            .serve(Cursor::new(input), &mut output)
            .expect("extension error is request-scoped");
        assert_eq!(
            output
                .windows(b"status=error\n".len())
                .filter(|window| *window == b"status=error\n")
                .count(),
            1
        );
        assert_eq!(
            output
                .windows(b"status=success\n".len())
                .filter(|window| *window == b"status=success\n")
                .count(),
            1
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn smudge_verification_failure_after_output_is_fatal_without_final_flush() {
        let (store, path) = store();
        let source = vec![b'a'; IO_BUFFER_SIZE + 1];
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        hasher.update(&source);
        let oid = object_digest(hasher);
        let object = store
            .ingest(oid, source.len() as u64, Cursor::new(source.clone()))
            .expect("seed object");
        let mut corrupted = source;
        *corrupted.last_mut().expect("non-empty object") = b'b';
        fs::write(object.path(), corrupted).expect("corrupt object");
        let pointer = LfsPointer::new(
            LfsOid::from_hex(&hex_oid(&oid)).expect("oid"),
            (IO_BUFFER_SIZE + 1) as u64,
            Vec::new(),
        )
        .expect("pointer")
        .to_bytes()
        .expect("pointer bytes");

        let mut engine = LfsFilterEngine::new(store, LfsPathPolicy::new(false));
        let mut input = handshake();
        input.extend_from_slice(&request(
            b"command=smudge\n",
            b"pathname=corrupt.bin\n",
            &pointer,
        ));
        input.extend_from_slice(&request(
            b"command=clean\n",
            b"pathname=must-not-run.bin\n",
            b"next request",
        ));
        let mut output = Vec::new();
        let error = engine
            .serve(Cursor::new(input), &mut output)
            .expect_err("late verification failure");
        assert!(matches!(error, LfsFilterError::FatalAfterResponse));
        assert_eq!(
            output
                .windows(b"status=success\n".len())
                .filter(|window| *window == b"status=success\n")
                .count(),
            1
        );
        assert!(
            !output
                .windows(b"status=error\n".len())
                .any(|window| window == b"status=error\n")
        );
        assert!(!output.ends_with(b"0000"));
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn pkt_flush_flushes_underlying_transport() {
        struct FlushWriter {
            bytes: Vec<u8>,
            flushes: usize,
        }

        impl Write for FlushWriter {
            fn write(&mut self, input: &[u8]) -> io::Result<usize> {
                self.bytes.extend_from_slice(input);
                Ok(input.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }

        let mut output = FlushWriter {
            bytes: Vec::new(),
            flushes: 0,
        };
        let mut writer = PktWriter::new(&mut output);
        writer.write_data(b"x").expect("data");
        writer.write_flush().expect("flush");
        drop(writer);
        assert_eq!(output.bytes, b"0005x0000");
        assert_eq!(output.flushes, 1);
    }

    #[test]
    fn packet_and_prefix_bounds_are_fixed() {
        assert_eq!(IO_BUFFER_SIZE, 128 * 1024);
        assert_eq!(PREFIX_LIMIT, 1024);
        assert_eq!(PKT_MAX_PAYLOAD, 65_516);
        let oversized = vec![b'x'; PKT_MAX_PAYLOAD + 1];
        let mut writer = PktWriter::new(Vec::<u8>::new());
        assert!(matches!(
            writer.write_data(&oversized),
            Err(LfsFilterError::Protocol(LfsProtocolError::PacketTooLarge))
        ));
    }
}
