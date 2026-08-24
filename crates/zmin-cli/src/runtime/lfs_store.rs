//! Content-addressed local storage for Git LFS media objects.
//!
//! The store deliberately has a small API: callers provide the already parsed
//! SHA-256 object id, the expected byte count, and a reader.  The filesystem
//! layout is fixed to `aa/bb/full-hex-id`; no caller-controlled path is ever
//! joined to the configured root.
//!
//! The configured root is a trusted store boundary: callers must not permit
//! untrusted processes to replace its components concurrently. All writers use
//! the no-clobber protocol below, while Unix additionally traverses components
//! by descriptor with `openat(2)`/`O_NOFOLLOW`. Windows rejects symlink/reparse
//! components from metadata, but the standard library has no equivalent
//! portable no-follow directory traversal or portable DACL enforcement; that
//! residual metadata/open race and permission boundary are intentionally
//! covered by the trusted-root contract and explicit tests.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::sync::Arc as Shared;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

use zmin_git_core::{GitHashAlgorithm, GitObjectHash};

const IO_BUFFER_SIZE: usize = 128 * 1024;
const MAX_PUBLISH_ATTEMPTS: usize = 16;
const MAX_UNIQUE_PATH_ATTEMPTS: usize = 32;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
#[cfg(windows)]
const WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
static DURABILITY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
static DURABILITY_CRASH_ARMED: AtomicBool = AtomicBool::new(true);
#[cfg(test)]
static DURABILITY_OPERATIONS: OnceLock<Mutex<Vec<LfsDurabilityOperation>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LfsDurabilityOperationKind {
    CreateDirectory,
    SyncDirectory,
    CreateTemporary,
    SyncTemporaryFile,
    PublishObject,
    RemoveTemporary,
    QuarantineObject,
    RemoveQuarantine,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct LfsDurabilityOperation {
    kind: LfsDurabilityOperationKind,
    path: PathBuf,
}

/// Errors returned by the local LFS object store.
#[derive(Debug)]
pub(crate) enum LfsStoreError {
    InvalidRoot,
    UnsafePath,
    MissingObject,
    CorruptObject,
    ConcurrentPublication,
    HashMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
    },
    SizeMismatch {
        expected: u64,
        actual: u64,
    },
    Io {
        operation: LfsStoreOperation,
        kind: io::ErrorKind,
    },
}

/// Filesystem operation used in a sanitized I/O error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LfsStoreOperation {
    OpenRoot,
    CreateDirectory,
    CreateTemporary,
    Read,
    Write,
    Sync,
    Publish,
    Quarantine,
    RemoveTemporary,
}

pub(crate) type LfsStoreResult<T> = Result<T, LfsStoreError>;

impl fmt::Display for LfsStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoot => formatter.write_str("invalid LFS storage root"),
            Self::UnsafePath => formatter.write_str("unsafe LFS storage path"),
            Self::MissingObject => formatter.write_str("LFS object is not present"),
            Self::CorruptObject => formatter.write_str("LFS object failed verification"),
            Self::ConcurrentPublication => {
                formatter.write_str("concurrent LFS object publication did not settle")
            }
            Self::HashMismatch { .. } => formatter.write_str("LFS object hash does not match"),
            Self::SizeMismatch { .. } => formatter.write_str("LFS object size does not match"),
            Self::Io { operation, kind } => {
                write!(formatter, "LFS storage {operation:?} failed ({kind:?})")
            }
        }
    }
}

impl std::error::Error for LfsStoreError {}

/// How a store operation resolved the computed content-addressed object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsStoreOutcome {
    Published,
    Deduplicated,
}

/// A successfully ingested, content-addressed LFS object.
#[derive(Debug, Clone)]
pub(crate) struct LfsStoredObject {
    oid: [u8; 32],
    size: u64,
    path: PathBuf,
    outcome: LfsStoreOutcome,
}

impl LfsStoredObject {
    pub(crate) fn oid(&self) -> [u8; 32] {
        self.oid
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn outcome(&self) -> LfsStoreOutcome {
        self.outcome
    }
}

/// Existing callers use this shorter name; the public store result is the
/// typed [`LfsStoredObject`] above.
pub(crate) type LfsObject = LfsStoredObject;

/// A local Git LFS media store rooted at an explicitly configured directory.
#[derive(Debug, Clone)]
pub(crate) struct LfsStore {
    root: PathBuf,
    #[cfg(unix)]
    root_directory: Shared<File>,
}

struct PreparedDestination {
    path: PathBuf,
    #[cfg(unix)]
    directories: UnixDestinationDirectories,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicationBarrierPlan {
    SharedDirectory,
    SeparateDirectories,
}

#[cfg(unix)]
struct UnixDestinationDirectories {
    second: File,
}

impl LfsStore {
    /// Open (and, if necessary, create) the configured storage root.
    ///
    /// The root is a trusted boundary. On Windows the caller is responsible
    /// for preventing untrusted concurrent mutation and for configuring ACLs;
    /// this API does not provide a native no-follow or DACL guarantee there.
    pub(crate) fn new(root: PathBuf) -> LfsStoreResult<Self> {
        validate_root_path(&root)?;
        ensure_directory(&root, LfsStoreOperation::OpenRoot)?;
        let root = fs::canonicalize(&root)
            .map_err(|error| io_error(LfsStoreOperation::OpenRoot, error))?;
        #[cfg(unix)]
        let root_directory = {
            let directory = open_directory_nofollow(&root)
                .map_err(|error| io_error(LfsStoreOperation::OpenRoot, error))?;
            let permissions_changed = ensure_private_directory(&directory)
                .map_err(|error| io_error(LfsStoreOperation::OpenRoot, error))?;
            if permissions_changed {
                sync_directory_handle(&directory, &root)?;
            }
            Shared::new(directory)
        };
        Ok(Self {
            root,
            #[cfg(unix)]
            root_directory,
        })
    }

    /// Ingest bytes into the content-addressed store without buffering the
    /// object in memory.  Publication is atomic and never overwrites an
    /// existing pathname.
    pub(crate) fn ingest<R: Read>(
        &self,
        expected_oid: [u8; 32],
        expected_size: u64,
        mut source: R,
    ) -> LfsStoreResult<LfsObject> {
        let destination = self.prepare_destination_directory(&expected_oid)?;
        let mut temporary = TemporaryFile::create(&destination)?;
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        let mut buffer = io_buffer();
        let mut size = 0_u64;

        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| io_error(LfsStoreOperation::Read, error))?;
            if read == 0 {
                break;
            }
            let read_size = u64::try_from(read).expect("a read length fits in u64");
            let next_size = size
                .checked_add(read_size)
                .ok_or(LfsStoreError::SizeMismatch {
                    expected: expected_size,
                    actual: u64::MAX,
                })?;
            if next_size > expected_size {
                return Err(LfsStoreError::SizeMismatch {
                    expected: expected_size,
                    actual: next_size,
                });
            }
            temporary
                .file_mut()
                .write_all(&buffer[..read])
                .map_err(|error| io_error(LfsStoreOperation::Write, error))?;
            hasher.update(&buffer[..read]);
            size = next_size;
        }

        if size != expected_size {
            return Err(LfsStoreError::SizeMismatch {
                expected: expected_size,
                actual: size,
            });
        }
        let actual_oid = object_digest(hasher);
        if actual_oid != expected_oid {
            return Err(LfsStoreError::HashMismatch {
                expected: expected_oid,
                actual: actual_oid,
            });
        }

        temporary
            .file_mut()
            .flush()
            .map_err(|error| io_error(LfsStoreOperation::Write, error))?;
        temporary
            .file_mut()
            .sync_all()
            .map_err(|error| io_error(LfsStoreOperation::Sync, error))?;
        record_durability_operation(
            LfsDurabilityOperationKind::SyncTemporaryFile,
            &temporary.path,
        );
        temporary.close_file();

        let object = self.publish(
            temporary,
            destination,
            expected_oid,
            expected_size,
            PublicationBarrierPlan::SharedDirectory,
        )?;
        Ok(object)
    }

    /// Stream an object exactly once into a private store temporary while
    /// computing its SHA-256 OID and checked byte count.  The source and
    /// temporary are never reread; only an already-existing destination may
    /// be verified by the normal deduplication/corruption policy.
    pub(crate) fn ingest_computing<R: Read>(
        &self,
        mut source: R,
    ) -> LfsStoreResult<LfsStoredObject> {
        let staging = self.root_temporary_destination()?;
        let mut temporary = TemporaryFile::create(&staging)?;
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        let mut buffer = io_buffer();
        let mut size = 0_u64;

        loop {
            let read = source
                .read(&mut buffer)
                .map_err(|error| io_error(LfsStoreOperation::Read, error))?;
            if read == 0 {
                break;
            }
            let read_size = u64::try_from(read).expect("a read length fits in u64");
            size = size
                .checked_add(read_size)
                .ok_or(LfsStoreError::SizeMismatch {
                    expected: u64::MAX,
                    actual: u64::MAX,
                })?;
            temporary
                .file_mut()
                .write_all(&buffer[..read])
                .map_err(|error| io_error(LfsStoreOperation::Write, error))?;
            hasher.update(&buffer[..read]);
        }

        let oid = object_digest(hasher);
        temporary
            .file_mut()
            .flush()
            .map_err(|error| io_error(LfsStoreOperation::Write, error))?;
        temporary
            .file_mut()
            .sync_all()
            .map_err(|error| io_error(LfsStoreOperation::Sync, error))?;
        record_durability_operation(
            LfsDurabilityOperationKind::SyncTemporaryFile,
            &temporary.path,
        );
        temporary.close_file();

        let destination = self.prepare_destination_directory(&oid)?;
        self.publish(
            temporary,
            destination,
            oid,
            size,
            PublicationBarrierPlan::SeparateDirectories,
        )
    }

    /// Open an object for streaming, verifying its SHA-256 and size as the
    /// caller reads through EOF.  A short read or a caller that stops before
    /// EOF does not claim successful verification.
    pub(crate) fn open_verified(
        &self,
        expected_oid: [u8; 32],
        expected_size: u64,
    ) -> LfsStoreResult<VerifiedLfsReader> {
        let file = self.open_transport_file(expected_oid, expected_size)?;
        Ok(VerifiedLfsReader::new(file, expected_oid, expected_size))
    }

    /// Open the exact store object as a same-handle regular file for the HTTP
    /// transport's closed verified-file body. The transport performs the
    /// single streaming SHA-256 pass on every retry; this boundary performs no
    /// path reopen or content pre-pass.
    pub(crate) fn open_transport_file(
        &self,
        expected_oid: [u8; 32],
        expected_size: u64,
    ) -> LfsStoreResult<File> {
        let destination = self.prepare_destination_directory(&expected_oid)?;
        let file = match open_destination_file(&destination)? {
            DestinationFile::Missing => return Err(LfsStoreError::MissingObject),
            DestinationFile::Unsafe => return Err(LfsStoreError::UnsafePath),
            DestinationFile::File(file) => file,
        };
        let metadata = file
            .metadata()
            .map_err(|error| io_error(LfsStoreOperation::Read, error))?;
        if !metadata.is_file() {
            return Err(LfsStoreError::CorruptObject);
        }
        if metadata.len() != expected_size {
            return Err(LfsStoreError::SizeMismatch {
                expected: expected_size,
                actual: metadata.len(),
            });
        }
        set_private_file_mode(&file).map_err(|error| io_error(LfsStoreOperation::Read, error))?;
        Ok(file)
    }

    /// Verify an object by streaming it to EOF without retaining its bytes.
    pub(crate) fn verify(&self, expected_oid: [u8; 32], expected_size: u64) -> LfsStoreResult<()> {
        let mut reader = self.open_verified(expected_oid, expected_size)?;
        let mut buffer = io_buffer();
        while reader.read_verified(&mut buffer)? != 0 {}
        reader.finish()
    }

    fn prepare_destination(&self, oid: &[u8; 32]) -> LfsStoreResult<PathBuf> {
        let hex = hex_oid(oid);
        let first = self.root.join(&hex[..2]);
        ensure_directory(&first, LfsStoreOperation::CreateDirectory)?;
        let second = first.join(&hex[2..4]);
        ensure_directory(&second, LfsStoreOperation::CreateDirectory)?;
        let destination = second.join(hex);
        Ok(destination)
    }

    fn prepare_destination_directory(&self, oid: &[u8; 32]) -> LfsStoreResult<PreparedDestination> {
        #[cfg(unix)]
        {
            let hex = hex_oid(oid);
            let root = self
                .root_directory
                .try_clone()
                .map_err(|error| io_error(LfsStoreOperation::CreateDirectory, error))?;
            let first = open_or_create_directory(
                &root,
                &hex.as_bytes()[..2],
                &self.root.join(&hex[..2]),
                LfsStoreOperation::CreateDirectory,
            )?;
            let second = open_or_create_directory(
                &first,
                &hex.as_bytes()[2..4],
                &self.root.join(&hex[..2]).join(&hex[2..4]),
                LfsStoreOperation::CreateDirectory,
            )?;
            return Ok(PreparedDestination {
                path: self.root.join(&hex[..2]).join(&hex[2..4]).join(&hex),
                directories: UnixDestinationDirectories { second },
            });
        }
        #[cfg(not(unix))]
        {
            Ok(PreparedDestination {
                path: self.prepare_destination(oid)?,
            })
        }
    }

    fn root_temporary_destination(&self) -> LfsStoreResult<PreparedDestination> {
        #[cfg(unix)]
        {
            let second = self
                .root_directory
                .try_clone()
                .map_err(|error| io_error(LfsStoreOperation::CreateTemporary, error))?;
            return Ok(PreparedDestination {
                path: self.root.join(".lfs-computed-temp"),
                directories: UnixDestinationDirectories { second },
            });
        }
        #[cfg(not(unix))]
        {
            Ok(PreparedDestination {
                path: self.root.join(".lfs-computed-temp"),
            })
        }
    }

    fn object_path(&self, oid: &[u8; 32]) -> LfsStoreResult<PathBuf> {
        self.prepare_destination(oid)
    }

    fn publish(
        &self,
        temporary: TemporaryFile,
        destination: PreparedDestination,
        expected_oid: [u8; 32],
        expected_size: u64,
        barrier_plan: PublicationBarrierPlan,
    ) -> LfsStoreResult<LfsObject> {
        for _attempt in 0..MAX_PUBLISH_ATTEMPTS {
            match inspect_existing(&destination, expected_oid, expected_size)? {
                ExistingObject::Valid => {
                    temporary.remove()?;
                    return Ok(LfsStoredObject {
                        oid: expected_oid,
                        size: expected_size,
                        path: destination.path.clone(),
                        outcome: LfsStoreOutcome::Deduplicated,
                    });
                }
                ExistingObject::Missing => {}
                ExistingObject::Corrupt => {
                    self.quarantine_corrupt(&destination)?;
                    continue;
                }
                ExistingObject::Unsafe => return Err(LfsStoreError::UnsafePath),
            }

            match temporary.publish(&destination) {
                Ok(()) => {
                    record_durability_operation(
                        LfsDurabilityOperationKind::PublishObject,
                        &destination.path,
                    );
                    match barrier_plan {
                        PublicationBarrierPlan::SharedDirectory => {
                            temporary.remove_without_directory_sync()?;
                            self.sync_destination_parent(&destination)?;
                        }
                        PublicationBarrierPlan::SeparateDirectories => {
                            self.sync_destination_parent(&destination)?;
                            temporary.remove()?;
                        }
                    }
                    return Ok(LfsStoredObject {
                        oid: expected_oid,
                        size: expected_size,
                        path: destination.path.clone(),
                        outcome: LfsStoreOutcome::Published,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(io_error(LfsStoreOperation::Publish, error)),
            }
        }
        Err(LfsStoreError::ConcurrentPublication)
    }

    fn sync_destination_parent(&self, destination: &PreparedDestination) -> LfsStoreResult<()> {
        #[cfg(unix)]
        {
            let parent = destination.path.parent().ok_or(LfsStoreError::UnsafePath)?;
            sync_directory_handle(&destination.directories.second, parent)
        }
        #[cfg(not(unix))]
        {
            let parent = destination.path.parent().ok_or(LfsStoreError::UnsafePath)?;
            sync_directory(parent)
        }
    }

    fn quarantine_corrupt(&self, destination: &PreparedDestination) -> LfsStoreResult<()> {
        #[cfg(unix)]
        {
            return quarantine_corrupt_unix(destination);
        }
        #[cfg(not(unix))]
        {
            let path = &destination.path;
            let parent = path.parent().ok_or(LfsStoreError::UnsafePath)?;
            for _attempt in 0..MAX_UNIQUE_PATH_ATTEMPTS {
                let quarantine = quarantine_path(path);
                match fs::hard_link(path, &quarantine) {
                    Ok(()) => {
                        match fs::remove_file(path) {
                            Ok(()) => {}
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                            Err(error) => {
                                let _ = fs::remove_file(&quarantine);
                                return Err(io_error(LfsStoreOperation::Quarantine, error));
                            }
                        }
                        record_durability_operation(
                            LfsDurabilityOperationKind::QuarantineObject,
                            path,
                        );
                        let first_sync = sync_directory(parent);
                        let cleanup = fs::remove_file(&quarantine)
                            .map_err(|error| io_error(LfsStoreOperation::Quarantine, error));
                        if cleanup.is_ok() {
                            record_durability_operation(
                                LfsDurabilityOperationKind::RemoveQuarantine,
                                &quarantine,
                            );
                        }
                        let second_sync = if cleanup.is_ok() {
                            sync_directory(parent)
                        } else {
                            Ok(())
                        };
                        first_sync?;
                        cleanup?;
                        second_sync?;
                        return Ok(());
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(io_error(LfsStoreOperation::Quarantine, error)),
                }
            }
            Err(LfsStoreError::ConcurrentPublication)
        }
    }
}

/// A reader that verifies its object at EOF while retaining only a fixed-size
/// read buffer supplied by the caller.
pub(crate) struct VerifiedLfsReader {
    file: File,
    expected_oid: [u8; 32],
    expected_size: u64,
    hasher: GitObjectHash,
    size: u64,
    state: VerificationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerificationState {
    Pending,
    Verified,
    Failed,
}

impl VerifiedLfsReader {
    fn new(file: File, expected_oid: [u8; 32], expected_size: u64) -> Self {
        Self {
            file,
            expected_oid,
            expected_size,
            hasher: GitObjectHash::new(GitHashAlgorithm::Sha256),
            size: 0,
            state: VerificationState::Pending,
        }
    }

    pub(crate) fn finish(&mut self) -> LfsStoreResult<()> {
        match self.state {
            VerificationState::Verified => Ok(()),
            VerificationState::Pending | VerificationState::Failed => {
                Err(LfsStoreError::CorruptObject)
            }
        }
    }

    pub(crate) fn read_verified(&mut self, buffer: &mut [u8]) -> LfsStoreResult<usize> {
        self.read_inner(buffer)
    }

    fn finish_at_eof(&mut self) -> LfsStoreResult<()> {
        if self.state != VerificationState::Pending {
            return self.finish();
        }
        if self.size != self.expected_size {
            self.state = VerificationState::Failed;
            return Err(LfsStoreError::SizeMismatch {
                expected: self.expected_size,
                actual: self.size,
            });
        }
        let actual_oid = object_digest(std::mem::replace(
            &mut self.hasher,
            GitObjectHash::new(GitHashAlgorithm::Sha256),
        ));
        if actual_oid != self.expected_oid {
            self.state = VerificationState::Failed;
            return Err(LfsStoreError::HashMismatch {
                expected: self.expected_oid,
                actual: actual_oid,
            });
        }
        self.state = VerificationState::Verified;
        Ok(())
    }
}

impl Read for VerifiedLfsReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.read_inner(buffer)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

impl VerifiedLfsReader {
    fn read_inner(&mut self, buffer: &mut [u8]) -> LfsStoreResult<usize> {
        match self.state {
            VerificationState::Verified => return Ok(0),
            VerificationState::Failed => return Err(LfsStoreError::CorruptObject),
            VerificationState::Pending => {}
        }
        if buffer.is_empty() {
            return Ok(0);
        }
        let read = self.file.read(buffer).map_err(|error| {
            self.state = VerificationState::Failed;
            io_error(LfsStoreOperation::Read, error)
        })?;
        if read == 0 {
            self.finish_at_eof()?;
            return Ok(0);
        }
        let read_size = u64::try_from(read).expect("a read length fits in u64");
        self.size = match self.size.checked_add(read_size) {
            Some(size) => size,
            None => {
                self.state = VerificationState::Failed;
                return Err(LfsStoreError::SizeMismatch {
                    expected: self.expected_size,
                    actual: u64::MAX,
                });
            }
        };
        if self.size > self.expected_size {
            self.state = VerificationState::Failed;
            return Err(LfsStoreError::SizeMismatch {
                expected: self.expected_size,
                actual: self.size,
            });
        }
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingObject {
    Missing,
    Valid,
    Corrupt,
    Unsafe,
}

enum DestinationFile {
    Missing,
    Unsafe,
    File(File),
}

struct TemporaryFile {
    file: Option<File>,
    path: PathBuf,
    armed: bool,
    #[cfg(unix)]
    directory: File,
    #[cfg(unix)]
    name: Vec<u8>,
}

impl TemporaryFile {
    fn create(destination: &PreparedDestination) -> LfsStoreResult<Self> {
        #[cfg(unix)]
        {
            return Self::create_unix(destination);
        }
        #[cfg(not(unix))]
        for _attempt in 0..MAX_UNIQUE_PATH_ATTEMPTS {
            let path = temporary_path(&destination.path);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            set_creation_file_mode(&mut options);
            let file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(io_error(LfsStoreOperation::CreateTemporary, error));
                }
            };
            if let Err(error) = set_private_file_mode(&file) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(io_error(LfsStoreOperation::CreateTemporary, error));
            }
            record_durability_operation(LfsDurabilityOperationKind::CreateTemporary, &path);
            return Ok(Self {
                file: Some(file),
                path,
                armed: true,
            });
        }
        #[cfg(not(unix))]
        Err(LfsStoreError::ConcurrentPublication)
    }

    #[cfg(unix)]
    fn create_unix(destination: &PreparedDestination) -> LfsStoreResult<Self> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::{AsRawFd, FromRawFd};

        let directory = destination
            .directories
            .second
            .try_clone()
            .map_err(|error| io_error(LfsStoreOperation::CreateTemporary, error))?;
        for _attempt in 0..MAX_UNIQUE_PATH_ATTEMPTS {
            let path = temporary_path(&destination.path);
            let name = path
                .file_name()
                .ok_or(LfsStoreError::UnsafePath)?
                .as_bytes()
                .to_vec();
            let name_c = CString::new(name.as_slice()).map_err(|_| LfsStoreError::UnsafePath)?;
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    name_c.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    PRIVATE_FILE_MODE as libc::c_uint,
                )
            };
            if fd < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(io_error(LfsStoreOperation::CreateTemporary, error));
            }
            let file = unsafe { File::from_raw_fd(fd) };
            if let Err(error) = set_private_file_mode(&file) {
                let _ = unlinkat(&directory, &name);
                return Err(io_error(LfsStoreOperation::CreateTemporary, error));
            }
            record_durability_operation(LfsDurabilityOperationKind::CreateTemporary, &path);
            return Ok(Self {
                file: Some(file),
                path,
                armed: true,
                directory,
                name,
            });
        }
        Err(LfsStoreError::ConcurrentPublication)
    }

    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary file is open")
    }

    fn close_file(&mut self) {
        self.file.take();
    }

    fn publish(&self, destination: &PreparedDestination) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::io::AsRawFd;

            let source = CString::new(self.name.as_slice()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "temporary name contains NUL")
            })?;
            let target = CString::new(
                destination
                    .path
                    .file_name()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid object"))?
                    .as_bytes(),
            )
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "object name contains NUL"))?;
            let result = unsafe {
                libc::linkat(
                    self.directory.as_raw_fd(),
                    source.as_ptr(),
                    destination.directories.second.as_raw_fd(),
                    target.as_ptr(),
                    0,
                )
            };
            if result == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
        #[cfg(not(unix))]
        {
            fs::hard_link(&self.path, &destination.path)
        }
    }

    fn remove_entry(&mut self) -> LfsStoreResult<()> {
        self.close_file();
        #[cfg(unix)]
        {
            unlinkat(&self.directory, &self.name)
                .map_err(|error| io_error(LfsStoreOperation::RemoveTemporary, error))?;
            record_durability_operation(LfsDurabilityOperationKind::RemoveTemporary, &self.path);
        }
        #[cfg(not(unix))]
        {
            fs::remove_file(&self.path)
                .map_err(|error| io_error(LfsStoreOperation::RemoveTemporary, error))?;
            record_durability_operation(LfsDurabilityOperationKind::RemoveTemporary, &self.path);
        }
        self.armed = false;
        Ok(())
    }

    fn remove_without_directory_sync(mut self) -> LfsStoreResult<()> {
        self.remove_entry()
    }

    fn remove(mut self) -> LfsStoreResult<()> {
        self.remove_entry()?;
        let parent = self.path.parent().ok_or(LfsStoreError::UnsafePath)?;
        #[cfg(unix)]
        {
            sync_directory_handle(&self.directory, parent)?;
        }
        #[cfg(not(unix))]
        {
            sync_directory(parent)?;
        }
        Ok(())
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.armed {
            self.file.take();
            #[cfg(unix)]
            {
                if unlinkat(&self.directory, &self.name).is_ok() {
                    record_durability_operation(
                        LfsDurabilityOperationKind::RemoveTemporary,
                        &self.path,
                    );
                    if let Some(parent) = self.path.parent() {
                        let _ = sync_directory_handle(&self.directory, parent);
                    }
                }
            }
            #[cfg(not(unix))]
            if fs::remove_file(&self.path).is_ok() {
                record_durability_operation(
                    LfsDurabilityOperationKind::RemoveTemporary,
                    &self.path,
                );
                if let Some(parent) = self.path.parent() {
                    let _ = sync_directory(parent);
                }
            }
        }
    }
}

fn open_destination_file(destination: &PreparedDestination) -> LfsStoreResult<DestinationFile> {
    #[cfg(unix)]
    {
        return open_destination_file_unix(destination);
    }
    #[cfg(not(unix))]
    {
        let metadata = match symlink_metadata(&destination.path, LfsStoreOperation::Read) {
            Ok(metadata) => metadata,
            Err(LfsStoreError::Io {
                kind: io::ErrorKind::NotFound,
                ..
            }) => return Ok(DestinationFile::Missing),
            Err(error) => return Err(error),
        };
        if is_link_or_reparse(&metadata) {
            return Ok(DestinationFile::Unsafe);
        }
        let file = match open_read_file(&destination.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(DestinationFile::Missing);
            }
            Err(error) => return Err(io_error(LfsStoreOperation::Read, error)),
        };
        Ok(DestinationFile::File(file))
    }
}

#[cfg(unix)]
fn open_destination_file_unix(
    destination: &PreparedDestination,
) -> LfsStoreResult<DestinationFile> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let name = destination
        .path
        .file_name()
        .ok_or(LfsStoreError::UnsafePath)?;
    let name = CString::new(name.as_bytes()).map_err(|_| LfsStoreError::UnsafePath)?;
    let fd = unsafe {
        libc::openat(
            destination.directories.second.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(DestinationFile::Missing);
        }
        if error.raw_os_error() == Some(libc::ELOOP) {
            return Ok(DestinationFile::Unsafe);
        }
        return Err(io_error(LfsStoreOperation::Read, error));
    }
    Ok(DestinationFile::File(unsafe { File::from_raw_fd(fd) }))
}

fn inspect_existing(
    destination: &PreparedDestination,
    expected_oid: [u8; 32],
    expected_size: u64,
) -> LfsStoreResult<ExistingObject> {
    let mut file = match open_destination_file(destination)? {
        DestinationFile::Missing => return Ok(ExistingObject::Missing),
        DestinationFile::Unsafe => return Ok(ExistingObject::Unsafe),
        DestinationFile::File(file) => file,
    };
    let metadata = file
        .metadata()
        .map_err(|error| io_error(LfsStoreOperation::Read, error))?;
    if !metadata.is_file() {
        return Ok(ExistingObject::Unsafe);
    }
    set_private_file_mode(&file).map_err(|error| io_error(LfsStoreOperation::Read, error))?;
    match verify_file(&mut file, expected_oid, expected_size) {
        Ok(()) => Ok(ExistingObject::Valid),
        Err(LfsStoreError::HashMismatch { .. } | LfsStoreError::SizeMismatch { .. }) => {
            Ok(ExistingObject::Corrupt)
        }
        Err(error) => Err(error),
    }
}

fn verify_file(file: &mut File, expected_oid: [u8; 32], expected_size: u64) -> LfsStoreResult<()> {
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
    let mut buffer = io_buffer();
    let mut size = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(LfsStoreOperation::Read, error))?;
        if read == 0 {
            break;
        }
        let read_size = u64::try_from(read).expect("a read length fits in u64");
        size = size
            .checked_add(read_size)
            .ok_or(LfsStoreError::SizeMismatch {
                expected: expected_size,
                actual: u64::MAX,
            })?;
        hasher.update(&buffer[..read]);
        if size > expected_size {
            return Err(LfsStoreError::SizeMismatch {
                expected: expected_size,
                actual: size,
            });
        }
    }
    if size != expected_size {
        return Err(LfsStoreError::SizeMismatch {
            expected: expected_size,
            actual: size,
        });
    }
    let actual_oid = object_digest(hasher);
    if actual_oid != expected_oid {
        return Err(LfsStoreError::HashMismatch {
            expected: expected_oid,
            actual: actual_oid,
        });
    }
    Ok(())
}

fn ensure_directory(path: &Path, operation: LfsStoreOperation) -> LfsStoreResult<()> {
    let mut current = PathBuf::new();
    let mut saw_normal_component = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return Err(LfsStoreError::UnsafePath),
            Component::Normal(name) => {
                current.push(name);
                ensure_directory_component(&current, operation)?;
                saw_normal_component = true;
            }
        }
    }
    if !saw_normal_component {
        ensure_directory_component(path, operation)?;
    }
    Ok(())
}

fn ensure_directory_component(path: &Path, operation: LfsStoreOperation) -> LfsStoreResult<()> {
    match symlink_metadata(path, operation) {
        Ok(metadata) => {
            // macOS exposes the system temporary directory through the
            // root-owned `/var` and `/tmp` symlink aliases.  These aliases
            // are part of the platform path contract, whereas arbitrary
            // symlinked store ancestors remain unsafe and are rejected.
            let alias = is_link_or_reparse(&metadata) && is_system_directory_alias(path, &metadata);
            let is_directory = if alias {
                fs::metadata(path)
                    .map_err(|error| io_error(operation, error))?
                    .is_dir()
            } else {
                metadata.is_dir()
            };
            if (is_link_or_reparse(&metadata) && !alias) || !is_directory {
                return Err(LfsStoreError::UnsafePath);
            }
            Ok(())
        }
        Err(LfsStoreError::Io {
            kind: io::ErrorKind::NotFound,
            ..
        }) => {
            let created = match create_private_directory(path) {
                Ok(()) => true,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
                Err(error) => return Err(io_error(operation, error)),
            };
            let metadata = symlink_metadata(path, operation)?;
            let alias = is_link_or_reparse(&metadata) && is_system_directory_alias(path, &metadata);
            let is_directory = if alias {
                fs::metadata(path)
                    .map_err(|error| io_error(operation, error))?
                    .is_dir()
            } else {
                metadata.is_dir()
            };
            if (is_link_or_reparse(&metadata) && !alias) || !is_directory {
                return Err(LfsStoreError::UnsafePath);
            }
            set_private_directory_mode(path).map_err(|error| io_error(operation, error))?;
            if created {
                record_durability_operation(LfsDurabilityOperationKind::CreateDirectory, path);
                let parent = path.parent().ok_or(LfsStoreError::UnsafePath)?;
                let parent = if parent.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    parent
                };
                sync_directory(parent)?;
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn is_system_directory_alias(path: &Path, metadata: &fs::Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;

        let expected = if path == Path::new("/var") {
            Path::new("/private/var")
        } else if path == Path::new("/tmp") {
            Path::new("/private/tmp")
        } else {
            return false;
        };
        if !metadata.file_type().is_symlink() || metadata.uid() != 0 {
            return false;
        }
        let Ok(resolved) = fs::canonicalize(path) else {
            return false;
        };
        if resolved != expected {
            return false;
        }
        fs::metadata(expected).is_ok_and(|target| target.is_dir() && target.uid() == 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, metadata);
        false
    }
}

#[cfg(unix)]
fn open_directory_nofollow(path: &Path) -> io::Result<File> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let mut current = File::open("/")?;
    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::Prefix(_) | Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LFS storage path contains an unsafe component",
                ));
            }
        };
        let name = CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "LFS storage path contains NUL")
        })?;
        let fd = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        current = unsafe { File::from_raw_fd(fd) };
    }
    Ok(current)
}

#[cfg(unix)]
fn open_or_create_directory(
    parent: &File,
    name: &[u8],
    path: &Path,
    operation: LfsStoreOperation,
) -> LfsStoreResult<File> {
    use std::ffi::CString;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let name = CString::new(name).map_err(|_| LfsStoreError::UnsafePath)?;
    let open = || {
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
        };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    };

    let directory = match open() {
        Ok(directory) => directory,
        Err(error)
            if error
                .raw_os_error()
                .is_some_and(|code| code == libc::ELOOP || code == libc::ENOTDIR) =>
        {
            return Err(LfsStoreError::UnsafePath);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let result = unsafe {
                libc::mkdirat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    PRIVATE_DIRECTORY_MODE as libc::mode_t,
                )
            };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(io_error(operation, error));
                }
            } else {
                record_durability_operation(LfsDurabilityOperationKind::CreateDirectory, path);
                let parent_path = path.parent().ok_or(LfsStoreError::UnsafePath)?;
                sync_directory_handle(parent, parent_path)?;
            }
            open().map_err(|error| io_error(operation, error))?
        }
        Err(error) => return Err(io_error(operation, error)),
    };
    let permissions_changed =
        ensure_private_directory(&directory).map_err(|error| io_error(operation, error))?;
    if permissions_changed {
        sync_directory_handle(&directory, path)?;
    }
    Ok(directory)
}

#[cfg(unix)]
fn ensure_private_directory(directory: &File) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    let mode = directory.metadata()?.permissions().mode() & 0o777;
    if mode & !PRIVATE_DIRECTORY_MODE != 0 || mode != PRIVATE_DIRECTORY_MODE {
        directory.set_permissions(fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE))?;
        return Ok(true);
    }
    Ok(false)
}

fn symlink_metadata(path: &Path, operation: LfsStoreOperation) -> LfsStoreResult<fs::Metadata> {
    fs::symlink_metadata(path).map_err(|error| io_error(operation, error))
}

fn open_read_file(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        return open_read_file_unix(path);
    }
    #[cfg(not(unix))]
    {
        // Windows component checks below reject symlinks/reparse points, but
        // std does not expose a portable no-follow directory traversal.  The
        // remaining metadata/open race is covered by the trusted store-root
        // contract and called out as a platform limitation in the tests.
        let mut options = OpenOptions::new();
        options.read(true);
        options.open(path)
    }
}

#[cfg(unix)]
fn open_read_file_unix(path: &Path) -> io::Result<File> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    let mut current = File::open("/")?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::Prefix(_) | Component::ParentDir => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LFS storage path contains an unsafe component",
                ));
            }
        };
        let name = CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "LFS storage path contains NUL")
        })?;
        let flags = if components.peek().is_some() {
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW
        } else {
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW
        };
        let fd = unsafe { libc::openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        current = unsafe { File::from_raw_fd(fd) };
    }
    Ok(current)
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        return metadata.file_attributes() & WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn validate_root_path(root: &Path) -> LfsStoreResult<()> {
    if root.as_os_str().is_empty()
        || root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(LfsStoreError::InvalidRoot);
    }
    if is_filesystem_root(root) || is_system_store_root(root) {
        return Err(LfsStoreError::InvalidRoot);
    }
    match fs::canonicalize(root) {
        Ok(canonical) if is_system_store_root(&canonical) => Err(LfsStoreError::InvalidRoot),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(LfsStoreOperation::OpenRoot, error)),
    }
}

fn is_filesystem_root(path: &Path) -> bool {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::RootDir), None) => true,
        (Some(Component::Prefix(_)), Some(Component::RootDir)) => components.next().is_none(),
        _ => false,
    }
}

fn is_system_store_root(path: &Path) -> bool {
    if path == Path::new("/tmp") || path == Path::new("/var") {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        path == Path::new("/private/tmp") || path == Path::new("/private/var")
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn object_digest(hasher: GitObjectHash) -> [u8; 32] {
    let object_id = hasher.finalize();
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(object_id.as_bytes());
    digest
}

fn io_buffer() -> Box<[u8]> {
    vec![0_u8; IO_BUFFER_SIZE].into_boxed_slice()
}

fn hex_oid(oid: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in oid {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
fn record_durability_operation(kind: LfsDurabilityOperationKind, path: &Path) {
    let normalized_path = fs::canonicalize(path).or_else(|_| {
        let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
        let name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        fs::canonicalize(parent).map(|parent| parent.join(name))
    });
    DURABILITY_OPERATIONS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("durability recorder lock")
        .push(LfsDurabilityOperation {
            kind,
            path: normalized_path.unwrap_or_else(|_| path.to_path_buf()),
        });
    let sequence = DURABILITY_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
    let crash_sequence = std::env::var("ZMIN_LFS_TEST_CRASH_SEQUENCE")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    if DURABILITY_CRASH_ARMED.load(Ordering::SeqCst) && crash_sequence == Some(sequence) {
        std::process::exit(86);
    }
}

#[cfg(not(test))]
#[inline]
fn record_durability_operation(_kind: LfsDurabilityOperationKind, _path: &Path) {}

#[cfg(test)]
fn durability_operations() -> Vec<LfsDurabilityOperation> {
    DURABILITY_OPERATIONS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("durability recorder lock")
        .clone()
}

#[cfg(test)]
fn durability_operations_under(root: &Path) -> Vec<LfsDurabilityOperation> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    durability_operations()
        .iter()
        .filter(|operation| operation.path.starts_with(&root))
        .cloned()
        .collect()
}

fn temporary_path(destination: &Path) -> PathBuf {
    let mut path = destination.to_path_buf();
    path.set_file_name(format!(
        ".{}.tmp-{}",
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        unique_nonce()
    ));
    path
}

fn quarantine_path(path: &Path) -> PathBuf {
    let mut quarantine = path.to_path_buf();
    quarantine.set_file_name(format!(
        ".{}.corrupt-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        unique_nonce()
    ));
    quarantine
}

fn unique_nonce() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::thread;

    let mut hasher = DefaultHasher::new();
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    thread::current().id().hash(&mut hasher);
    TEMP_FILE_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn io_error(operation: LfsStoreOperation, error: io::Error) -> LfsStoreError {
    LfsStoreError::Io {
        operation,
        kind: error.kind(),
    }
}

fn set_private_file_mode(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

fn set_creation_file_mode(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(PRIVATE_FILE_MODE);
    }
    #[cfg(not(unix))]
    let _ = options;
}

fn create_private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(PRIVATE_DIRECTORY_MODE);
        builder.create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

fn set_private_directory_mode(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(unix)]
fn unlinkat(directory: &File, name: &[u8]) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::io::AsRawFd;

    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains NUL"))?;
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn quarantine_corrupt_unix(destination: &PreparedDestination) -> LfsStoreResult<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;

    let target = destination
        .path
        .file_name()
        .ok_or(LfsStoreError::UnsafePath)?
        .as_bytes()
        .to_vec();
    let target = CString::new(target.as_slice()).map_err(|_| LfsStoreError::UnsafePath)?;
    for _attempt in 0..MAX_UNIQUE_PATH_ATTEMPTS {
        let quarantine_path = quarantine_path(&destination.path);
        let quarantine_name = quarantine_path
            .file_name()
            .ok_or(LfsStoreError::UnsafePath)?
            .as_bytes()
            .to_vec();
        let quarantine =
            CString::new(quarantine_name.as_slice()).map_err(|_| LfsStoreError::UnsafePath)?;
        let placeholder = unsafe {
            libc::openat(
                destination.directories.second.as_raw_fd(),
                quarantine.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                PRIVATE_FILE_MODE as libc::c_uint,
            )
        };
        if placeholder < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::AlreadyExists {
                continue;
            }
            return Err(io_error(LfsStoreOperation::Quarantine, error));
        }
        unsafe {
            libc::close(placeholder);
        }

        let result = unsafe {
            libc::renameat(
                destination.directories.second.as_raw_fd(),
                target.as_ptr(),
                destination.directories.second.as_raw_fd(),
                quarantine.as_ptr(),
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if unlinkat(&destination.directories.second, quarantine_name.as_slice()).is_ok() {
                let parent = destination.path.parent().ok_or(LfsStoreError::UnsafePath)?;
                sync_directory_handle(&destination.directories.second, parent)?;
            }
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(());
            }
            return Err(io_error(LfsStoreOperation::Quarantine, error));
        }
        record_durability_operation(
            LfsDurabilityOperationKind::QuarantineObject,
            &destination.path,
        );
        let parent = destination.path.parent().ok_or(LfsStoreError::UnsafePath)?;
        let first_sync = sync_directory_handle(&destination.directories.second, parent);
        let cleanup = unlinkat(&destination.directories.second, quarantine_name.as_slice())
            .map_err(|error| io_error(LfsStoreOperation::Quarantine, error));
        if cleanup.is_ok() {
            record_durability_operation(
                LfsDurabilityOperationKind::RemoveQuarantine,
                &quarantine_path,
            );
        }
        let second_sync = if cleanup.is_ok() {
            sync_directory_handle(&destination.directories.second, parent)
        } else {
            Ok(())
        };
        first_sync?;
        cleanup?;
        second_sync?;
        return Ok(());
    }
    Err(LfsStoreError::ConcurrentPublication)
}

fn sync_directory(path: &Path) -> LfsStoreResult<()> {
    #[cfg(unix)]
    {
        File::open(path)
            .map_err(|error| io_error(LfsStoreOperation::Sync, error))?
            .sync_all()
            .map_err(|error| io_error(LfsStoreOperation::Sync, error))?;
    }
    #[cfg(not(unix))]
    {
        // The existing project convention is a no-op on Windows: std does not
        // expose a portable directory handle sync, while the object file is
        // flushed before the no-clobber hard-link publication.
        let _ = path;
    }
    record_durability_operation(LfsDurabilityOperationKind::SyncDirectory, path);
    Ok(())
}

#[cfg(unix)]
fn sync_directory_file(file: &File) -> io::Result<()> {
    file.sync_all()
}

#[cfg(unix)]
fn sync_directory_handle(file: &File, path: &Path) -> LfsStoreResult<()> {
    sync_directory_file(file).map_err(|error| io_error(LfsStoreOperation::Sync, error))?;
    record_durability_operation(LfsDurabilityOperationKind::SyncDirectory, path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::process::Command;
    use std::sync::Arc;
    use std::thread;
    use tempfile::TempDir;

    #[derive(Clone, Copy, Debug)]
    enum DurabilityCrashMode {
        Expected,
        Computed,
        Quarantine,
    }

    impl DurabilityCrashMode {
        fn from_environment() -> Self {
            match std::env::var("ZMIN_LFS_TEST_CRASH_MODE").as_deref() {
                Ok("expected") => Self::Expected,
                Ok("computed") => Self::Computed,
                Ok("quarantine") => Self::Quarantine,
                _ => panic!("invalid durability crash probe mode"),
            }
        }

        fn as_str(self) -> &'static str {
            match self {
                Self::Expected => "expected",
                Self::Computed => "computed",
                Self::Quarantine => "quarantine",
            }
        }

        fn operation_count(self) -> u64 {
            match self {
                Self::Expected => 11,
                Self::Computed => 12,
                Self::Quarantine => 9,
            }
        }

        fn publication_operation(self) -> u64 {
            match self {
                Self::Expected | Self::Computed => 9,
                Self::Quarantine => 7,
            }
        }
    }

    fn oid(bytes: &[u8]) -> [u8; 32] {
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        hasher.update(bytes);
        object_digest(hasher)
    }

    fn store() -> (TempDir, LfsStore) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("objects");
        let store = LfsStore::new(root).expect("LFS store");
        (directory, store)
    }

    fn operation(kind: LfsDurabilityOperationKind, path: &Path) -> LfsDurabilityOperation {
        LfsDurabilityOperation {
            kind,
            path: path.to_path_buf(),
        }
    }

    fn run_exact_test_probe(test_name: &str, environment: &[(&str, &str)]) {
        let executable = std::env::current_exe().expect("current test executable");
        let mut command = Command::new(executable);
        command.arg("--exact").arg(test_name).arg("--nocapture");
        command.envs(environment.iter().copied());
        let output = command.output().expect("isolated durability probe");
        assert!(
            output.stdout.len() <= 64 * 1024 && output.stderr.len() <= 64 * 1024,
            "isolated durability probe emitted excessive output"
        );
        assert!(
            output.status.success(),
            "isolated durability probe failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn sha256_stream_matches_known_block_boundaries_and_large_input() {
        let cases = [
            (
                0,
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                63,
                "7f963102afccacb03458e0e3ee8f51a234359aed99cb7ebfd33d5e47a095fcba",
            ),
            (
                64,
                "30a1b375b49e0f1d5e00b6868bbfd471cbe5387806456cc11fd153c689bfe747",
            ),
            (
                65,
                "ce4c5c9097270aaca5630ec6ca4b48c145a1bb24462589a0ec3f69cd5c9b2a5c",
            ),
            (
                127,
                "fc7d260501b641289fe7df7d943df65d93c782f2f3a1e2a4e69c6f763d88f8c5",
            ),
            (
                128,
                "5a878aab98a988c5479b48c00d8e2bef988c946dc648583ffe7ac598d76bacd5",
            ),
            (
                129,
                "a1253e220a0a60177ba22d6f1ae079f6b8f0b8fa598b0adc1390b0d7c81756fa",
            ),
        ];
        for (length, expected) in cases {
            let bytes: Vec<u8> = (0..length).map(|index| (index * 17 + 3) as u8).collect();
            let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
            for chunk in bytes.chunks(17) {
                hasher.update(chunk);
            }
            assert_eq!(hex_oid(&object_digest(hasher)), expected);
        }

        let chunk: Vec<u8> = (0..4096).map(|index| (index % 251) as u8).collect();
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha256);
        for _ in 0..4096 {
            hasher.update(&chunk);
        }
        assert_eq!(
            hex_oid(&object_digest(hasher)),
            "67454e4c52729b6fbdde0d68afd711229c015d1790408c896b93a2986108c282"
        );
    }

    #[test]
    fn ingest_and_read_are_chunked_and_verified() {
        let (_directory, store) = store();
        let bytes = vec![b'x'; IO_BUFFER_SIZE * 2 + 17];
        let object = store
            .ingest(oid(&bytes), bytes.len() as u64, Cursor::new(bytes.clone()))
            .expect("ingest");
        assert_eq!(object.size(), bytes.len() as u64);
        assert!(object.path().is_file());
        store.verify(object.oid(), object.size()).expect("verify");
        let mut reader = store
            .open_verified(object.oid(), object.size())
            .expect("open");
        let mut result = Vec::new();
        reader.read_to_end(&mut result).expect("read");
        assert_eq!(result, bytes);
    }

    #[test]
    fn durability_order_syncs_only_changed_directory_entries() {
        if std::env::var_os("ZMIN_LFS_TEST_DURABILITY_ORDER_PROBE").is_none() {
            run_exact_test_probe(
                "runtime::lfs_store::tests::durability_order_syncs_only_changed_directory_entries",
                &[("ZMIN_LFS_TEST_DURABILITY_ORDER_PROBE", "1")],
            );
            return;
        }

        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("objects");
        let store = LfsStore::new(root.clone()).expect("store");
        let canonical_directory = fs::canonicalize(directory.path()).expect("canonical temp root");
        let canonical_root = fs::canonicalize(&root).expect("canonical store root");
        let initial_operations = durability_operations().len();
        let reopened = LfsStore::new(root).expect("reopened existing store");
        assert_eq!(
            durability_operations().len(),
            initial_operations,
            "opening an existing store must not sync unchanged ancestors"
        );
        drop(reopened);
        let bytes = b"durability order";
        let object_oid = oid(bytes);
        store
            .ingest(object_oid, bytes.len() as u64, Cursor::new(bytes))
            .expect("first ingest");
        let oid_hex = hex_oid(&object_oid);
        let first = canonical_root.join(&oid_hex[..2]);
        let second = first.join(&oid_hex[2..4]);
        let object_path = second.join(&oid_hex);
        let operations = durability_operations();
        let temporary = operations
            .get(6)
            .expect("temporary create operation")
            .path
            .clone();
        assert_eq!(
            operations,
            vec![
                operation(LfsDurabilityOperationKind::CreateDirectory, &canonical_root),
                operation(
                    LfsDurabilityOperationKind::SyncDirectory,
                    &canonical_directory,
                ),
                operation(LfsDurabilityOperationKind::CreateDirectory, &first),
                operation(LfsDurabilityOperationKind::SyncDirectory, &canonical_root),
                operation(LfsDurabilityOperationKind::CreateDirectory, &second),
                operation(LfsDurabilityOperationKind::SyncDirectory, &first),
                operation(LfsDurabilityOperationKind::CreateTemporary, &temporary),
                operation(LfsDurabilityOperationKind::SyncTemporaryFile, &temporary),
                operation(LfsDurabilityOperationKind::PublishObject, &object_path),
                operation(LfsDurabilityOperationKind::RemoveTemporary, &temporary),
                operation(LfsDurabilityOperationKind::SyncDirectory, &second),
            ]
        );

        let prior_operations = operations.len();
        store
            .ingest(object_oid, bytes.len() as u64, Cursor::new(bytes))
            .expect("deduplicated ingest");
        let operations = durability_operations();
        let deduplicated_temporary = operations
            .get(prior_operations)
            .expect("deduplicated temporary create")
            .path
            .clone();
        assert_eq!(
            operations[prior_operations..],
            vec![
                operation(
                    LfsDurabilityOperationKind::CreateTemporary,
                    &deduplicated_temporary,
                ),
                operation(
                    LfsDurabilityOperationKind::SyncTemporaryFile,
                    &deduplicated_temporary,
                ),
                operation(
                    LfsDurabilityOperationKind::RemoveTemporary,
                    &deduplicated_temporary,
                ),
                operation(LfsDurabilityOperationKind::SyncDirectory, &second),
            ]
        );
        assert!(
            operations
                .iter()
                .all(|operation| operation.path.starts_with(&canonical_directory)),
            "every recorded durability operation must stay inside the isolated test root: {operations:?}"
        );

        let computed_bytes = b"computed durability order";
        let prior_operations = operations.len();
        let computed = store
            .ingest_computing(Cursor::new(computed_bytes))
            .expect("computed ingest");
        assert_eq!(computed.oid(), oid(computed_bytes));
        let operations = durability_operations();
        assert_eq!(
            operations[operations.len() - 4..],
            [
                operation(LfsDurabilityOperationKind::PublishObject, computed.path(),),
                operation(
                    LfsDurabilityOperationKind::SyncDirectory,
                    computed.path().parent().expect("computed object parent"),
                ),
                operation(
                    LfsDurabilityOperationKind::RemoveTemporary,
                    &operations[operations.len() - 2].path,
                ),
                operation(LfsDurabilityOperationKind::SyncDirectory, &canonical_root),
            ]
        );
        assert!(
            operations.len() >= prior_operations + 6,
            "computed publication must retain separate source and destination barriers"
        );
    }

    #[test]
    fn durability_crash_probe() {
        let Some(root) = std::env::var_os("ZMIN_LFS_TEST_CRASH_ROOT").map(PathBuf::from) else {
            return;
        };
        let mode = DurabilityCrashMode::from_environment();
        if matches!(mode, DurabilityCrashMode::Quarantine) {
            DURABILITY_CRASH_ARMED.store(false, Ordering::SeqCst);
        }
        DURABILITY_SEQUENCE.store(0, Ordering::SeqCst);
        let bytes = b"crash-safe object";
        let store = LfsStore::new(root).expect("crash probe store");
        match mode {
            DurabilityCrashMode::Expected => {
                store
                    .ingest(oid(bytes), bytes.len() as u64, Cursor::new(bytes))
                    .expect("expected-OID crash probe ingest");
            }
            DurabilityCrashMode::Computed => {
                store
                    .ingest_computing(Cursor::new(bytes))
                    .expect("computed-OID crash probe ingest");
            }
            DurabilityCrashMode::Quarantine => {
                let destination = store.prepare_destination(&oid(bytes)).expect("destination");
                fs::write(destination, b"corrupt").expect("corrupt destination");
                DURABILITY_SEQUENCE.store(0, Ordering::SeqCst);
                DURABILITY_CRASH_ARMED.store(true, Ordering::SeqCst);
                store
                    .ingest(oid(bytes), bytes.len() as u64, Cursor::new(bytes))
                    .expect("quarantine crash probe ingest");
            }
        }
    }

    #[test]
    fn every_process_restart_failpoint_recovers_without_partial_objects() {
        let executable = std::env::current_exe().expect("current test executable");
        for mode in [
            DurabilityCrashMode::Expected,
            DurabilityCrashMode::Computed,
            DurabilityCrashMode::Quarantine,
        ] {
            for crash_sequence in 1..=mode.operation_count() {
                let directory = tempfile::tempdir().expect("temporary directory");
                let root = directory.path().join("objects");
                let output = Command::new(&executable)
                    .arg("--exact")
                    .arg("runtime::lfs_store::tests::durability_crash_probe")
                    .arg("--nocapture")
                    .env("ZMIN_LFS_TEST_CRASH_ROOT", &root)
                    .env("ZMIN_LFS_TEST_CRASH_SEQUENCE", crash_sequence.to_string())
                    .env("ZMIN_LFS_TEST_CRASH_MODE", mode.as_str())
                    .output()
                    .expect("crash probe process");
                assert!(
                    output.stdout.len() <= 64 * 1024 && output.stderr.len() <= 64 * 1024,
                    "crash probe emitted excessive output"
                );
                assert_eq!(
                    output.status.code(),
                    Some(86),
                    "{mode:?} crash operation {crash_sequence}: stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );

                let bytes = b"crash-safe object";
                let object_oid = oid(bytes);
                let store = LfsStore::new(root).expect("reopened store");
                let verification = store.verify(object_oid, bytes.len() as u64);
                if crash_sequence < mode.publication_operation() {
                    assert!(
                        verification.is_err(),
                        "pre-publication {mode:?} restart {crash_sequence} exposed a valid object"
                    );
                } else {
                    verification.unwrap_or_else(|error| {
                        panic!("published {mode:?} object after restart {crash_sequence}: {error}")
                    });
                }

                // This models abrupt process death (which skips Rust destructors), not a
                // storage-device power loss. The exact fsync order is asserted separately.
                match mode {
                    DurabilityCrashMode::Computed => {
                        store
                            .ingest_computing(Cursor::new(bytes))
                            .unwrap_or_else(|error| {
                                panic!("computed recovery after restart {crash_sequence}: {error}")
                            });
                    }
                    DurabilityCrashMode::Expected | DurabilityCrashMode::Quarantine => {
                        store
                            .ingest(object_oid, bytes.len() as u64, Cursor::new(bytes))
                            .unwrap_or_else(|error| {
                                panic!("{mode:?} recovery after restart {crash_sequence}: {error}")
                            });
                    }
                }
                store
                    .verify(object_oid, bytes.len() as u64)
                    .unwrap_or_else(|error| {
                        panic!("{mode:?} verification after restart {crash_sequence}: {error}")
                    });
            }
        }
    }

    #[test]
    fn transport_file_is_same_handle_regular_and_exact_length() {
        let (_directory, store) = store();
        let bytes = b"transport body";
        let object = store
            .ingest(oid(bytes), bytes.len() as u64, Cursor::new(bytes))
            .expect("ingest");
        let mut file = store
            .open_transport_file(object.oid(), object.size())
            .expect("transport file");
        assert!(file.metadata().expect("metadata").is_file());
        let mut result = Vec::new();
        file.read_to_end(&mut result).expect("same-handle read");
        assert_eq!(result, bytes);
        assert!(matches!(
            store.open_transport_file(object.oid(), object.size() + 1),
            Err(LfsStoreError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn ingest_computing_streams_a_chunked_source_once() {
        let (_directory, store) = store();
        let bytes = vec![b'c'; IO_BUFFER_SIZE * 2 + 17];
        let object = store
            .ingest_computing(OnePassReader::chunked(bytes.clone(), 173))
            .expect("computed ingest");
        assert_eq!(object.oid(), oid(&bytes));
        assert_eq!(object.size(), bytes.len() as u64);
        assert_eq!(object.outcome(), LfsStoreOutcome::Published);
        assert_eq!(fs::read(object.path()).expect("stored bytes"), bytes);
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[test]
    fn ingest_computing_publishes_empty_objects() {
        let (_directory, store) = store();
        let object = store
            .ingest_computing(OnePassReader::new(Vec::new()))
            .expect("empty computed ingest");
        assert_eq!(object.oid(), oid(&[]));
        assert_eq!(object.size(), 0);
        assert_eq!(object.outcome(), LfsStoreOutcome::Published);
        assert!(fs::read(object.path()).expect("empty object").is_empty());
    }

    #[test]
    fn zero_length_read_does_not_verify_pending_reader() {
        let (_directory, store) = store();
        let bytes = b"pending verification";
        let object = store
            .ingest(oid(bytes), bytes.len() as u64, Cursor::new(bytes))
            .expect("ingest");
        let mut reader = store
            .open_verified(object.oid(), object.size())
            .expect("open");
        let mut empty = [];
        assert_eq!(reader.read_verified(&mut empty).expect("empty read"), 0);
        assert!(matches!(reader.finish(), Err(LfsStoreError::CorruptObject)));
        let mut output = Vec::new();
        reader.read_to_end(&mut output).expect("complete read");
        assert_eq!(output, bytes);
        reader.finish().expect("verified after EOF");
    }

    #[test]
    fn failed_verification_is_terminal() {
        let (_directory, store) = store();
        let bytes = b"terminal verification";
        let object = store
            .ingest(oid(bytes), bytes.len() as u64, Cursor::new(bytes))
            .expect("ingest");
        let file = open_read_file(object.path()).expect("open object");
        let mut reader = VerifiedLfsReader::new(file, [0_u8; 32], object.size());
        let mut output = [0_u8; 64];
        assert_eq!(
            reader.read_verified(&mut output).expect("object bytes"),
            bytes.len()
        );
        assert!(matches!(
            reader.read_verified(&mut output),
            Err(LfsStoreError::HashMismatch { .. })
        ));
        assert!(matches!(
            reader.read_verified(&mut output),
            Err(LfsStoreError::CorruptObject)
        ));
        let mut empty = [];
        assert!(matches!(
            reader.read_verified(&mut empty),
            Err(LfsStoreError::CorruptObject)
        ));
        assert!(matches!(reader.finish(), Err(LfsStoreError::CorruptObject)));
    }

    #[test]
    fn wrong_hash_and_size_never_publish() {
        let (_directory, store) = store();
        let bytes = b"payload";
        let error = store
            .ingest([0_u8; 32], bytes.len() as u64, Cursor::new(bytes))
            .expect_err("wrong hash");
        assert!(matches!(error, LfsStoreError::HashMismatch { .. }));
        let error = store
            .ingest(oid(bytes), bytes.len() as u64 + 1, Cursor::new(bytes))
            .expect_err("wrong size");
        assert!(matches!(error, LfsStoreError::SizeMismatch { .. }));
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[cfg(unix)]
    #[test]
    fn new_directories_and_temp_publication_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let (_directory, store) = store();
        assert_eq!(
            fs::metadata(&store.root)
                .expect("root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let bytes = b"private object";
        let object = store
            .ingest(oid(bytes), bytes.len() as u64, Cursor::new(bytes))
            .expect("ingest");
        assert_eq!(
            fs::metadata(object.path())
                .expect("object metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let computed = store
            .ingest_computing(OnePassReader::new(b"computed private".to_vec()))
            .expect("computed ingest");
        assert_eq!(
            fs::metadata(computed.path())
                .expect("computed object metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn system_roots_are_rejected_without_mode_changes() {
        use std::os::unix::fs::PermissionsExt;

        let mut paths = vec![
            PathBuf::from("/"),
            PathBuf::from("/tmp"),
            PathBuf::from("/var"),
        ];
        #[cfg(target_os = "macos")]
        paths.extend([PathBuf::from("/private/tmp"), PathBuf::from("/private/var")]);

        for path in paths {
            let before = fs::symlink_metadata(&path)
                .expect("system root metadata")
                .permissions()
                .mode()
                & 0o7777;
            assert!(matches!(
                LfsStore::new(path.clone()),
                Err(LfsStoreError::InvalidRoot)
            ));
            let after = fs::symlink_metadata(&path)
                .expect("system root metadata after rejection")
                .permissions()
                .mode()
                & 0o7777;
            assert_eq!(
                after,
                before,
                "system root mode changed: {}",
                path.display()
            );
        }
    }

    #[test]
    fn regular_file_cannot_be_configured_as_root() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root_file = directory.path().join("root-file");
        fs::write(&root_file, b"not a directory").expect("root file");
        assert!(matches!(
            LfsStore::new(root_file),
            Err(LfsStoreError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn special_file_cannot_be_configured_as_destination() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let (_directory, store) = store();
        let bytes = b"fifo destination";
        let expected = oid(bytes);
        let destination = store.prepare_destination(&expected).expect("destination");
        let name = CString::new(destination.as_os_str().as_bytes()).expect("fifo path");
        let result = unsafe { libc::mkfifo(name.as_ptr(), PRIVATE_FILE_MODE as libc::mode_t) };
        assert_eq!(result, 0, "create fifo: {}", io::Error::last_os_error());

        assert!(matches!(
            store.ingest(expected, bytes.len() as u64, Cursor::new(bytes)),
            Err(LfsStoreError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn existing_store_boundaries_are_tightened_before_use() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("objects");
        fs::create_dir(&root).expect("root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).expect("root mode");
        let store = LfsStore::new(root).expect("store");
        let canonical_root = fs::canonicalize(&store.root).expect("canonical root");
        assert_eq!(
            fs::metadata(&store.root)
                .expect("root metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            durability_operations_under(directory.path()),
            vec![operation(
                LfsDurabilityOperationKind::SyncDirectory,
                &canonical_root,
            )],
            "permission tightening must sync exactly the changed root"
        );

        let bytes = b"existing private object";
        let expected = oid(bytes);
        let path = store.prepare_destination(&expected).expect("path");
        let shard = path.parent().expect("shard");
        let canonical_shard = fs::canonicalize(shard).expect("canonical shard");
        fs::set_permissions(shard, fs::Permissions::from_mode(0o755)).expect("shard mode");
        fs::write(&path, bytes).expect("object");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("object mode");
        let prior_operations = durability_operations_under(directory.path()).len();

        store
            .ingest(expected, bytes.len() as u64, Cursor::new(bytes))
            .expect("reuse existing object");
        let operations = durability_operations_under(directory.path());
        assert_eq!(
            operations
                .get(prior_operations)
                .expect("permission sync operation"),
            &operation(LfsDurabilityOperationKind::SyncDirectory, &canonical_shard,),
            "permission tightening must sync the exact changed shard before use"
        );
        assert_eq!(
            fs::metadata(shard)
                .expect("shard metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("object metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn concurrent_writers_deduplicate_without_clobbering() {
        let (_directory, store) = store();
        let store = Arc::new(store);
        let bytes = b"same object from every writer".to_vec();
        let expected = oid(&bytes);
        let mut workers = Vec::new();
        for _ in 0..8 {
            let store = Arc::clone(&store);
            let bytes = bytes.clone();
            workers.push(thread::spawn(move || {
                store
                    .ingest(expected, bytes.len() as u64, Cursor::new(bytes))
                    .expect("concurrent ingest")
            }));
        }
        for worker in workers {
            worker.join().expect("writer thread");
        }
        store.verify(expected, bytes.len() as u64).expect("verify");
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[test]
    fn concurrent_computed_writers_publish_once_and_deduplicate() {
        let (_directory, store) = store();
        let store = Arc::new(store);
        let bytes = vec![b'p'; IO_BUFFER_SIZE + 31];
        let mut workers = Vec::new();
        for _ in 0..8 {
            let store = Arc::clone(&store);
            let bytes = bytes.clone();
            workers.push(thread::spawn(move || {
                store
                    .ingest_computing(OnePassReader::chunked(bytes, 97))
                    .expect("concurrent computed ingest")
                    .outcome()
            }));
        }
        let outcomes = workers
            .into_iter()
            .map(|worker| worker.join().expect("writer thread"))
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == LfsStoreOutcome::Published)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == LfsStoreOutcome::Deduplicated)
                .count(),
            7
        );
        let expected = oid(&bytes);
        store.verify(expected, bytes.len() as u64).expect("verify");
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[test]
    fn symlinked_shard_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("objects");
        fs::create_dir(&root).expect("root");
        let target = directory.path().join("outside");
        fs::create_dir(&target).expect("outside");
        let expected = oid(b"symlink");
        let shard = root.join(hex_oid(&expected)[..2].to_owned());
        if !symlink_directory(&target, &shard) {
            return;
        }
        let store = LfsStore::new(root).expect("store");
        assert!(matches!(
            store.ingest(expected, 7, Cursor::new(b"symlink")),
            Err(LfsStoreError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn computed_ingest_rejects_symlinked_shard_and_cleans_temp() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("objects");
        fs::create_dir(&root).expect("root");
        let target = directory.path().join("outside");
        fs::create_dir(&target).expect("outside");
        let bytes = b"computed symlink";
        let expected = oid(bytes);
        let shard = root.join(hex_oid(&expected)[..2].to_owned());
        std::os::unix::fs::symlink(&target, &shard).expect("directory symlink");
        let store = LfsStore::new(root).expect("store");
        assert!(matches!(
            store.ingest_computing(OnePassReader::new(bytes.to_vec())),
            Err(LfsStoreError::UnsafePath)
        ));
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[test]
    fn symlinked_root_ancestor_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let target = directory.path().join("outside");
        fs::create_dir(&target).expect("outside");
        let linked = directory.path().join("linked");
        if !symlink_directory(&target, &linked) {
            return;
        }
        let root = linked.join("objects");
        assert!(matches!(
            LfsStore::new(root),
            Err(LfsStoreError::UnsafePath)
        ));
    }

    #[test]
    fn interrupted_ingest_cleans_private_temp_file() {
        let (_directory, store) = store();
        let expected = oid(b"never complete");
        let error = store
            .ingest(expected, 13, FailingReader::new(b"partial"))
            .expect_err("reader failure");
        assert!(matches!(
            error,
            LfsStoreError::Io {
                operation: LfsStoreOperation::Read,
                ..
            }
        ));
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[test]
    fn interrupted_computed_ingest_cleans_private_temp_file() {
        let (_directory, store) = store();
        let error = store
            .ingest_computing(FailingReader::new(b"computed partial"))
            .expect_err("reader failure");
        assert!(matches!(
            error,
            LfsStoreError::Io {
                operation: LfsStoreOperation::Read,
                ..
            }
        ));
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[test]
    fn corrupt_existing_object_is_quarantined_then_replaced() {
        let (directory, store) = store();
        let bytes = b"replacement bytes";
        let expected = oid(bytes);
        let path = store.prepare_destination(&expected).expect("path");
        fs::write(&path, b"corrupt").expect("corrupt object");
        let prior_operations = durability_operations_under(directory.path()).len();
        let object = store
            .ingest(expected, bytes.len() as u64, Cursor::new(bytes))
            .expect("replace corrupt object");
        assert_eq!(fs::read(object.path()).expect("read object"), bytes);
        assert_eq!(count_named_files(&store.root, ".corrupt-"), 0);
        let operations = durability_operations_under(directory.path());
        assert_eq!(
            operations[prior_operations..]
                .iter()
                .map(|operation| operation.kind)
                .collect::<Vec<_>>(),
            vec![
                LfsDurabilityOperationKind::CreateTemporary,
                LfsDurabilityOperationKind::SyncTemporaryFile,
                LfsDurabilityOperationKind::QuarantineObject,
                LfsDurabilityOperationKind::SyncDirectory,
                LfsDurabilityOperationKind::RemoveQuarantine,
                LfsDurabilityOperationKind::SyncDirectory,
                LfsDurabilityOperationKind::PublishObject,
                LfsDurabilityOperationKind::RemoveTemporary,
                LfsDurabilityOperationKind::SyncDirectory,
            ]
        );
    }

    #[test]
    fn computed_ingest_quarantines_corrupt_existing_destination() {
        let (_directory, store) = store();
        let bytes = b"computed replacement bytes";
        let expected = oid(bytes);
        let path = store.prepare_destination(&expected).expect("path");
        fs::write(&path, b"corrupt").expect("corrupt object");
        let object = store
            .ingest_computing(OnePassReader::chunked(bytes.to_vec(), 3))
            .expect("replace corrupt object");
        assert_eq!(object.outcome(), LfsStoreOutcome::Published);
        assert_eq!(fs::read(object.path()).expect("read object"), bytes);
        assert_eq!(count_named_files(&store.root, ".corrupt-"), 0);
        assert_eq!(count_temporary_files(&store.root), 0);
    }

    #[test]
    fn root_parent_traversal_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("objects").join("..").join("escape");
        assert!(matches!(
            LfsStore::new(root),
            Err(LfsStoreError::InvalidRoot)
        ));
    }

    fn count_temporary_files(root: &Path) -> usize {
        let mut count = 0;
        let Ok(entries) = fs::read_dir(root) else {
            return count;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_temporary_files(&path);
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(".tmp-"))
            {
                count += 1;
            }
        }
        count
    }

    fn count_named_files(root: &Path, marker: &str) -> usize {
        let mut count = 0;
        let Ok(entries) = fs::read_dir(root) else {
            return count;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_named_files(&path, marker);
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(marker))
            {
                count += 1;
            }
        }
        count
    }

    fn symlink_directory(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).expect("directory symlink");
            true
        }
        #[cfg(windows)]
        {
            match std::os::windows::fs::symlink_dir(target, link) {
                Ok(()) => true,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                    ) =>
                {
                    eprintln!("SKIP symlink capability unavailable: {error}");
                    false
                }
                Err(error) => panic!("directory symlink capability probe failed: {error}"),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            eprintln!("SKIP symlink capability unavailable on this platform");
            false
        }
    }

    struct FailingReader {
        bytes: Vec<u8>,
        offset: usize,
    }

    struct OnePassReader {
        bytes: Vec<u8>,
        offset: usize,
        max_chunk: usize,
        reached_eof: bool,
    }

    impl OnePassReader {
        fn new(bytes: Vec<u8>) -> Self {
            Self::chunked(bytes, usize::MAX)
        }

        fn chunked(bytes: Vec<u8>, max_chunk: usize) -> Self {
            Self {
                bytes,
                offset: 0,
                max_chunk: max_chunk.max(1),
                reached_eof: false,
            }
        }
    }

    impl Read for OnePassReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if output.is_empty() {
                return Ok(0);
            }
            if self.offset == self.bytes.len() {
                if self.reached_eof {
                    return Err(io::Error::other("one-pass reader was reread"));
                }
                self.reached_eof = true;
                return Ok(0);
            }
            let amount = output
                .len()
                .min(self.max_chunk)
                .min(self.bytes.len() - self.offset);
            output[..amount].copy_from_slice(&self.bytes[self.offset..self.offset + amount]);
            self.offset += amount;
            Ok(amount)
        }
    }

    impl FailingReader {
        fn new(bytes: &[u8]) -> Self {
            Self {
                bytes: bytes.to_vec(),
                offset: 0,
            }
        }
    }

    impl Read for FailingReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.offset == self.bytes.len() {
                return Err(io::Error::other("injected reader failure"));
            }
            let amount = output.len().min(self.bytes.len() - self.offset);
            output[..amount].copy_from_slice(&self.bytes[self.offset..self.offset + amount]);
            self.offset += amount;
            Ok(amount)
        }
    }
}
