#![cfg(windows)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_INVALID_FUNCTION,
    ERROR_INVALID_HANDLE, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_WRITE_THROUGH, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_LIST_DIRECTORY, FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES, FileBasicInfo, FileDispositionInfo, FileDispositionInfoEx,
    FileRenameInfo, FlushFileBuffers, GetFileInformationByHandle, GetFileInformationByHandleEx,
    OPEN_EXISTING, SetFileInformationByHandle,
};

const MAX_TEMP_ATTEMPTS: u32 = 32;

pub(crate) struct WindowsDirectory {
    path: PathBuf,
    handle: fs::File,
    identity: (u64, u64),
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct WindowsFileIdentity {
    volume: u64,
    index: u64,
    size: u64,
    last_write: u64,
}

pub(crate) struct WindowsFileSnapshot {
    directory: WindowsDirectory,
    target_name: OsString,
    file: fs::File,
    identity: WindowsFileIdentity,
}

pub(crate) enum WindowsPublishMode {
    Absent,
    Checked(WindowsFileSnapshot),
    Replace,
}

pub(crate) struct WindowsTempFile {
    directory: WindowsDirectory,
    file: fs::File,
    path: PathBuf,
    published: bool,
}

#[derive(Eq, PartialEq)]
enum CheckedPublishState {
    Unchanged,
    Committed,
    CommittedWithQuarantine(PathBuf),
    Restored,
    RecoveryRequired(PathBuf),
}

struct CheckedPublishOutcome {
    state: CheckedPublishState,
}

struct CheckedPublishFailure {
    state: CheckedPublishState,
    publish_error: io::Error,
    restore_error: Option<io::Error>,
}

impl CheckedPublishOutcome {
    fn recovery_path(&self) -> Option<&Path> {
        match &self.state {
            CheckedPublishState::CommittedWithQuarantine(path) => Some(path),
            _ => None,
        }
    }
}

impl CheckedPublishFailure {
    fn recovery_path(&self) -> Option<&Path> {
        match &self.state {
            CheckedPublishState::RecoveryRequired(path) => Some(path),
            _ => None,
        }
    }

    fn into_io_error(self, description: &str) -> io::Error {
        match self.state {
            CheckedPublishState::RecoveryRequired(path) => io::Error::new(
                self.publish_error.kind(),
                format!(
                    "could not publish {description}; prior file is preserved at '{}'; restore failed: {}",
                    path.display(),
                    self.restore_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "unknown restore error".into())
                ),
            ),
            CheckedPublishState::Restored => io::Error::new(
                self.publish_error.kind(),
                format!(
                    "could not publish {description}; prior file was restored: {}",
                    self.publish_error
                ),
            ),
            _ => io::Error::new(
                self.publish_error.kind(),
                format!("could not publish {description}: {}", self.publish_error),
            ),
        }
    }
}

pub(crate) fn reject_reparse_components(path: &Path, allow_missing_leaf: bool) -> io::Result<()> {
    let mut current = PathBuf::new();
    let components = path.components().collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| matches!(component, Component::Prefix(_)))
        && !components
            .iter()
            .any(|component| matches!(component, Component::RootDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LFS path has a drive-relative prefix",
        ));
    }
    for component in &components {
        if matches!(component, Component::ParentDir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LFS path contains an unsafe component",
            ));
        }
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            // The prefix is path syntax rather than a filesystem object.  The
            // following RootDir/Normal component is checked through the same
            // handle-safe path validation below.
            continue;
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if allow_missing_leaf && error.kind() == io::ErrorKind::NotFound => {
                break;
            }
            Err(error) => return Err(error),
        };
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LFS path contains a reparse point",
            ));
        }
    }
    Ok(())
}

pub(crate) fn open_regular_snapshot(path: &Path) -> io::Result<Option<WindowsFileSnapshot>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "LFS snapshot has no parent"))?;
    let directory = WindowsDirectory::open(parent)?;
    let target_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing snapshot target"))?
        .to_os_string();
    let file = match open_path_shared(
        path,
        FILE_GENERIC_READ | FILE_WRITE_ATTRIBUTES | DELETE,
        OPEN_EXISTING,
        FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_ATTRIBUTE_NORMAL,
        FILE_SHARE_READ,
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    require_regular(&metadata)?;
    let identity = file_identity(&file)?;
    Ok(Some(WindowsFileSnapshot {
        directory,
        target_name,
        file,
        identity,
    }))
}

pub(crate) fn atomic_write(
    path: &Path,
    contents: &[u8],
    mode: WindowsPublishMode,
    description: &str,
) -> io::Result<()> {
    let mut temporary = WindowsTempFile::create(path, description)?;
    temporary.write_all(contents)?;
    temporary.sync_all()?;
    temporary.publish(path, mode, description)
}

impl WindowsFileSnapshot {
    pub(crate) fn read_bounded(&mut self, limit: u64) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        (&mut self.file)
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)?;
        self.validate_unchanged()?;
        Ok(bytes)
    }

    fn validate_unchanged(&self) -> io::Result<()> {
        let metadata = self.file.metadata()?;
        require_regular(&metadata)?;
        if file_identity(&self.file)? != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "LFS target changed while its snapshot was held",
            ));
        }
        Ok(())
    }

    fn matches_target(&self, path: &Path) -> bool {
        path.parent() == Some(self.directory.path.as_path())
            && path.file_name() == Some(self.target_name.as_os_str())
    }
}

impl WindowsTempFile {
    pub(crate) fn create(target: &Path, description: &str) -> io::Result<Self> {
        let parent = target.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{description} path has no parent directory"),
            )
        })?;
        let directory = WindowsDirectory::open(parent)?;
        let (file, path) = create_temp(target, description)?;
        Ok(Self {
            directory,
            file,
            path,
            published: false,
        })
    }

    pub(crate) fn sync_all(&mut self) -> io::Result<()> {
        self.file.flush()?;
        flush_file(&self.file)
    }

    pub(crate) fn publish(
        mut self,
        target: &Path,
        mode: WindowsPublishMode,
        description: &str,
    ) -> io::Result<()> {
        if self.path.parent() != target.parent() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LFS temporary file has a different parent",
            ));
        }
        let target_name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing target name"))?;
        match mode {
            WindowsPublishMode::Checked(snapshot) => {
                if !snapshot.matches_target(target) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "LFS snapshot does not match publication target",
                    ));
                }
                if snapshot.directory.identity != self.directory.identity {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "LFS temporary file and snapshot have different parent directories",
                    ));
                }
                snapshot.validate_unchanged()?;
                let outcome = publish_checked_no_replace(
                    &snapshot.directory,
                    target_name,
                    &mut self,
                    &snapshot,
                    description,
                )
                .map_err(|failure| failure.into_io_error(description))?;
                let _ = outcome.recovery_path();
                let _ = snapshot.directory.flush();
                Ok(())
            }
            WindowsPublishMode::Absent | WindowsPublishMode::Replace => {
                self.directory.still_current()?;
                let replace = matches!(mode, WindowsPublishMode::Replace);
                rename_handle(target, &self.file, replace)?;
                self.published = true;
                let _ = self.directory.flush();
                Ok(())
            }
        }
    }
}

impl Write for WindowsTempFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Drop for WindowsTempFile {
    fn drop(&mut self) {
        if !self.published {
            let _ = mark_delete(&self.file);
        }
    }
}

impl WindowsDirectory {
    fn open(path: &Path) -> io::Result<Self> {
        reject_reparse_components(path, false)?;
        let handle = open_path_shared(
            path,
            FILE_GENERIC_READ | FILE_LIST_DIRECTORY,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )?;
        let metadata = handle.metadata()?;
        require_directory(&metadata)?;
        let identity = directory_identity(&handle)?;
        Ok(Self {
            path: path.to_owned(),
            handle,
            identity,
        })
    }

    fn still_current(&self) -> io::Result<()> {
        let metadata = self.handle.metadata()?;
        require_directory(&metadata)?;
        if directory_identity(&self.handle)? != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LFS parent directory was replaced",
            ));
        }
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        let result = unsafe { FlushFileBuffers(self.handle.as_raw_handle() as HANDLE) };
        if result != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(value)
                if value == ERROR_ACCESS_DENIED as i32
                    || value == ERROR_INVALID_FUNCTION as i32 =>
            {
                // Win32 has no portable directory-fsync equivalent.  These
                // filesystems report that limitation explicitly; publication
                // remains atomic, but this path cannot claim directory-entry
                // durability stronger than the filesystem provides.
                Ok(())
            }
            _ => Err(error),
        }
    }
}

fn create_temp(target: &Path, description: &str) -> io::Result<(fs::File, PathBuf)> {
    let name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("lfs");
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing temp parent"))?;
    for attempt in 0..MAX_TEMP_ATTEMPTS {
        let path = parent.join(format!(
            ".{name}.zmin-lfs-tmp-{}-{attempt}",
            std::process::id()
        ));
        match open_path_shared(
            &path,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ,
        ) {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("could not create temporary {description}: {error}"),
                ));
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("could not create temporary {description}"),
    ))
}

fn publish_checked_no_replace(
    directory: &WindowsDirectory,
    target_name: &OsStr,
    temporary: &mut WindowsTempFile,
    expected: &WindowsFileSnapshot,
    description: &str,
) -> Result<CheckedPublishOutcome, CheckedPublishFailure> {
    directory
        .still_current()
        .and_then(|()| expected.validate_unchanged())
        .map_err(|publish_error| CheckedPublishFailure {
            state: CheckedPublishState::Unchanged,
            publish_error,
            restore_error: None,
        })?;
    for attempt in 0..MAX_TEMP_ATTEMPTS {
        let mut backup_name = target_name.to_os_string();
        backup_name.push(format!(".zmin-lfs-old-{}-{attempt}", std::process::id()));
        let backup_path = directory.path.join(&backup_name);
        let target_path = directory.path.join(target_name);
        match rename_handle(&backup_path, &expected.file, false) {
            Ok(()) => {
                let outcome = finish_checked_publish_transaction(
                    backup_path,
                    || rename_handle(&target_path, &temporary.file, false),
                    || rename_handle(&target_path, &expected.file, false),
                    || mark_delete(&expected.file),
                );
                if outcome.is_ok() {
                    temporary.published = true;
                }
                return outcome;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CheckedPublishFailure {
                    state: CheckedPublishState::Unchanged,
                    publish_error: io::Error::new(
                        error.kind(),
                        format!("could not reserve prior {description}: {error}"),
                    ),
                    restore_error: None,
                });
            }
        }
    }
    Err(CheckedPublishFailure {
        state: CheckedPublishState::Unchanged,
        publish_error: io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("could not reserve a backup name for {description}"),
        ),
        restore_error: None,
    })
}

fn finish_checked_publish_transaction<Publish, Restore, Cleanup>(
    backup_path: PathBuf,
    publish: Publish,
    restore: Restore,
    cleanup: Cleanup,
) -> Result<CheckedPublishOutcome, CheckedPublishFailure>
where
    Publish: FnOnce() -> io::Result<()>,
    Restore: FnOnce() -> io::Result<()>,
    Cleanup: FnOnce() -> io::Result<()>,
{
    match publish() {
        Ok(()) => match cleanup() {
            Ok(()) => Ok(CheckedPublishOutcome {
                state: CheckedPublishState::Committed,
            }),
            Err(_) => Ok(CheckedPublishOutcome {
                state: CheckedPublishState::CommittedWithQuarantine(backup_path),
            }),
        },
        Err(publish_error) => match restore() {
            Ok(()) => Err(CheckedPublishFailure {
                state: CheckedPublishState::Restored,
                publish_error,
                restore_error: None,
            }),
            Err(restore_error) => Err(CheckedPublishFailure {
                state: CheckedPublishState::RecoveryRequired(backup_path),
                publish_error,
                restore_error: Some(restore_error),
            }),
        },
    }
}

fn rename_handle(
    target_path: &Path,
    temporary: &fs::File,
    replace_existing: bool,
) -> io::Result<()> {
    let target = target_path.as_os_str().encode_wide().collect::<Vec<_>>();
    if target.iter().any(|value| *value == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in target name",
        ));
    }
    let bytes = size_of::<FILE_RENAME_INFO>().saturating_add(
        target
            .len()
            .saturating_sub(1)
            .saturating_mul(size_of::<u16>()),
    );
    let mut storage = vec![0_u64; bytes.saturating_add(7) / 8];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = replace_existing;
        (*info).RootDirectory = ptr::null_mut();
        (*info).FileNameLength = (target.len() * size_of::<u16>()) as u32;
        ptr::copy_nonoverlapping(target.as_ptr(), (*info).FileName.as_mut_ptr(), target.len());
    }
    let result = unsafe {
        SetFileInformationByHandle(
            temporary.as_raw_handle() as HANDLE,
            FileRenameInfo,
            info.cast(),
            bytes as u32,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(map_windows_error(io::Error::last_os_error()))
    }
}

fn mark_delete(file: &fs::File) -> io::Result<()> {
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as HANDLE,
            FileDispositionInfoEx,
            (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if result != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(value)
            if value == ERROR_INVALID_FUNCTION as i32
                || value == ERROR_INVALID_PARAMETER as i32
                || value == ERROR_NOT_SUPPORTED as i32 => {}
        _ => return Err(error),
    }

    clear_readonly(file)?;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as HANDLE,
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn clear_readonly(file: &fs::File) -> io::Result<()> {
    set_readonly_attribute(file, false)
}

fn set_readonly_attribute(file: &fs::File, readonly: bool) -> io::Result<()> {
    let mut information = FILE_BASIC_INFO::default();
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            FileBasicInfo,
            (&mut information as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    let was_readonly = information.FileAttributes & FILE_ATTRIBUTE_READONLY != 0;
    if was_readonly == readonly {
        return Ok(());
    }
    if readonly {
        information.FileAttributes &= !FILE_ATTRIBUTE_NORMAL;
        information.FileAttributes |= FILE_ATTRIBUTE_READONLY;
    } else {
        information.FileAttributes &= !FILE_ATTRIBUTE_READONLY;
    }
    if information.FileAttributes == 0 {
        information.FileAttributes = FILE_ATTRIBUTE_NORMAL;
    }
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as HANDLE,
            FileBasicInfo,
            (&information as *const FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn flush_file(file: &fs::File) -> io::Result<()> {
    let result = unsafe { FlushFileBuffers(file.as_raw_handle() as HANDLE) };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn open_path(
    path: &Path,
    access: u32,
    disposition: u32,
    flags: u32,
    attributes: u32,
) -> io::Result<fs::File> {
    open_path_shared(
        path,
        access,
        disposition,
        flags,
        attributes,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
}

fn open_path_shared(
    path: &Path,
    access: u32,
    disposition: u32,
    flags: u32,
    attributes: u32,
    share: u32,
) -> io::Result<fs::File> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            share,
            ptr::null_mut(),
            disposition,
            flags | attributes,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(map_windows_error(io::Error::last_os_error()));
    }
    if handle == ptr::null_mut() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Windows returned an invalid LFS file handle",
        ));
    }
    Ok(unsafe { fs::File::from_raw_handle(handle) })
}

fn map_windows_error(error: io::Error) -> io::Error {
    match error.raw_os_error() {
        Some(value)
            if value == ERROR_FILE_EXISTS as i32 || value == ERROR_ALREADY_EXISTS as i32 =>
        {
            io::Error::new(io::ErrorKind::AlreadyExists, error)
        }
        Some(value) if value == ERROR_INVALID_HANDLE as i32 => {
            io::Error::new(io::ErrorKind::InvalidInput, error)
        }
        _ => error,
    }
}

fn require_directory(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_dir() || is_reparse(metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LFS parent is not a regular directory",
        ));
    }
    Ok(())
}

fn require_regular(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() || is_reparse(metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LFS path is not a regular non-reparse file",
        ));
    }
    Ok(())
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn file_identity(file: &fs::File) -> io::Result<WindowsFileIdentity> {
    let mut information =
        windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION::default();
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut information) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(WindowsFileIdentity {
        volume: u64::from(information.dwVolumeSerialNumber),
        index: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        size: (u64::from(information.nFileSizeHigh) << 32) | u64::from(information.nFileSizeLow),
        last_write: (u64::from(information.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(information.ftLastWriteTime.dwLowDateTime),
    })
}

fn directory_identity(file: &fs::File) -> io::Result<(u64, u64)> {
    let identity = file_identity(file)?;
    Ok((identity.volume, identity.index))
}

#[cfg(test)]
mod tests {
    use super::{
        CheckedPublishState, WindowsPublishMode, WindowsTempFile, atomic_write,
        finish_checked_publish_transaction, open_regular_snapshot, reject_reparse_components,
    };
    use std::cell::Cell;
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    #[test]
    fn windows_policy_rejects_parent_components() {
        assert!(reject_reparse_components(Path::new(".."), false).is_err());
    }

    #[test]
    fn windows_policy_rejects_drive_relative_paths() {
        assert!(reject_reparse_components(Path::new(r"C:hooks"), true).is_err());
    }

    #[test]
    fn checked_publish_pins_target_against_substitution() {
        let directory = tempfile::TempDir::new().expect("temp directory");
        let target = directory.path().join("attributes");
        let replacement = directory.path().join("replacement");
        fs::write(&target, b"old").expect("write target");
        let mut snapshot = open_regular_snapshot(&target)
            .expect("open snapshot")
            .expect("snapshot exists");
        assert_eq!(snapshot.read_bounded(16).expect("read snapshot"), b"old");

        assert!(fs::rename(&target, &replacement).is_err());
        atomic_write(
            &target,
            b"new",
            WindowsPublishMode::Checked(snapshot),
            "test target",
        )
        .expect("checked publish");
        assert_eq!(fs::read(&target).expect("read target"), b"new");
        assert!(!replacement.exists());
    }

    #[test]
    fn publication_uses_the_still_open_temporary_handle() {
        let directory = tempfile::TempDir::new().expect("temp directory");
        let target = directory.path().join("checkout");
        let moved = directory.path().join("moved-temp");
        let mut temporary = WindowsTempFile::create(&target, "test target").expect("create temp");
        temporary.write_all(b"trusted").expect("write temp");
        temporary.sync_all().expect("sync temp");
        let original_name = temporary.path.clone();

        assert!(fs::rename(&original_name, &moved).is_err());
        assert!(fs::write(&original_name, b"attacker").is_err());
        temporary
            .publish(&target, WindowsPublishMode::Absent, "test target")
            .expect("publish open handle");

        assert_eq!(fs::read(&target).expect("read target"), b"trusted");
        assert!(!original_name.exists());
        assert!(!moved.exists());
    }

    #[test]
    fn absent_publish_never_replaces_a_concurrent_target() {
        let directory = tempfile::TempDir::new().expect("temp directory");
        let target = directory.path().join("attributes");
        let mut temporary = WindowsTempFile::create(&target, "test target").expect("create temp");
        temporary.write_all(b"ours").expect("write temp");
        temporary.sync_all().expect("sync temp");
        fs::write(&target, b"theirs").expect("create concurrent target");

        let error = temporary
            .publish(&target, WindowsPublishMode::Absent, "test target")
            .expect_err("must not replace target");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).expect("read target"), b"theirs");
    }

    #[test]
    fn temporary_file_pins_parent_namespace_until_publish_finishes() {
        let root = tempfile::TempDir::new().expect("temp directory");
        let parent = root.path().join("pinned");
        let renamed = root.path().join("renamed");
        fs::create_dir(&parent).expect("create parent");
        let target = parent.join("target");
        let temporary = WindowsTempFile::create(&target, "test target").expect("create temp");

        assert!(fs::rename(&parent, &renamed).is_err());
        drop(temporary);
        fs::rename(&parent, &renamed).expect("parent becomes movable after temp closes");
    }

    #[test]
    fn cleanup_failure_commits_with_recoverable_quarantine() {
        let restore_called = Cell::new(false);
        let backup = PathBuf::from(r"C:\repo\old.quarantine");
        let outcome = finish_checked_publish_transaction(
            backup.clone(),
            || Ok(()),
            || {
                restore_called.set(true);
                Ok(())
            },
            || Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected")),
        )
        .unwrap_or_else(|_| panic!("committed cleanup failure must remain successful"));

        assert_eq!(outcome.recovery_path(), Some(backup.as_path()));
        assert!(!restore_called.get());
    }

    #[test]
    fn failed_restore_preserves_original_at_recovery_path() {
        let cleanup_called = Cell::new(false);
        let backup = PathBuf::from(r"C:\repo\old.quarantine");
        let failure = finish_checked_publish_transaction(
            backup.clone(),
            || Err(io::Error::new(io::ErrorKind::AlreadyExists, "publish")),
            || Err(io::Error::new(io::ErrorKind::PermissionDenied, "restore")),
            || {
                cleanup_called.set(true);
                Ok(())
            },
        )
        .err()
        .expect("restore failure must be reported");

        assert_eq!(failure.recovery_path(), Some(backup.as_path()));
        assert!(matches!(
            failure.state,
            CheckedPublishState::RecoveryRequired(_)
        ));
        assert!(!cleanup_called.get());
    }

    #[test]
    fn failed_publish_with_successful_restore_reports_restored_state() {
        let cleanup_called = Cell::new(false);
        let failure = finish_checked_publish_transaction(
            PathBuf::from(r"C:\repo\old.quarantine"),
            || Err(io::Error::new(io::ErrorKind::AlreadyExists, "publish")),
            || Ok(()),
            || {
                cleanup_called.set(true);
                Ok(())
            },
        )
        .err()
        .expect("publish failure must be reported");

        assert!(matches!(failure.state, CheckedPublishState::Restored));
        assert!(failure.recovery_path().is_none());
        assert!(!cleanup_called.get());
    }

    #[test]
    fn checked_publish_removes_readonly_backup_after_commit() {
        let directory = tempfile::TempDir::new().expect("temp directory");
        let target = directory.path().join("readonly");
        fs::write(&target, b"old").expect("write target");
        let mut snapshot = open_regular_snapshot(&target)
            .expect("open snapshot")
            .expect("snapshot exists");
        super::set_readonly_attribute(&snapshot.file, true).expect("make target readonly");
        assert_eq!(snapshot.read_bounded(16).expect("read snapshot"), b"old");

        atomic_write(
            &target,
            b"new",
            WindowsPublishMode::Checked(snapshot),
            "test target",
        )
        .expect("checked publish");

        assert_eq!(fs::read(&target).expect("read target"), b"new");
        let orphaned_entries = fs::read_dir(directory.path())
            .expect("read parent")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter(|name| name != "readonly")
            .collect::<Vec<_>>();
        assert!(
            orphaned_entries.is_empty(),
            "successful cleanup left orphaned entries: {orphaned_entries:?}"
        );
    }
}
