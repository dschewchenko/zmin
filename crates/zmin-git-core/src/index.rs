use std::ffi::OsString;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use memmap2::Mmap;
use sha1::{Digest as Sha1Digest, Sha1};

use crate::object::{GitHashAlgorithm, ObjectId};
use crate::tree::TreeMode;

const INDEX_SIGNATURE: &[u8; 4] = b"DIRC";
const INDEX_VERSION_V2: u32 = 2;
const INDEX_VERSION_V3: u32 = 3;
const INDEX_VERSION_V4: u32 = 4;
const ENTRY_FIXED_LEN: usize = 62;
const CHECKSUM_LEN: usize = 20;
const ENTRY_FLAG_ASSUME_VALID: u16 = 0x8000;
const ENTRY_FLAG_EXTENDED: u16 = 0x4000;
const ENTRY_EXTENDED_SKIP_WORKTREE: u16 = 0x4000;
const ENTRY_EXTENDED_INTENT_TO_ADD: u16 = 0x2000;
const INDEX_ENTRY_ASSUME_VALID: u8 = 0b001;
const INDEX_ENTRY_SKIP_WORKTREE: u8 = 0b010;
const INDEX_ENTRY_INTENT_TO_ADD: u8 = 0b100;
const RESOLVE_UNDO_EXTENSION: &[u8; 4] = b"REUC";
const SPARSE_DIRECTORY_EXTENSION: &[u8; 4] = b"sdir";
const INDEX_ENTRY_INITIAL_CAPACITY_LIMIT: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexMode {
    File,
    Executable,
    Symlink,
    Tree,
    Gitlink,
}

impl IndexMode {
    pub const fn bits(self) -> u32 {
        match self {
            Self::File => 0o100644,
            Self::Executable => 0o100755,
            Self::Symlink => 0o120000,
            Self::Tree => 0o040000,
            Self::Gitlink => 0o160000,
        }
    }

    pub fn from_bits(bits: u32) -> io::Result<Self> {
        match bits {
            0o100644 => Ok(Self::File),
            0o100755 => Ok(Self::Executable),
            0o120000 => Ok(Self::Symlink),
            0o160000 => Ok(Self::Gitlink),
            0o100000..=0o100777 => {
                if bits & 0o111 != 0 {
                    Ok(Self::Executable)
                } else {
                    Ok(Self::File)
                }
            }
            0o120001..=0o120777 => Ok(Self::Symlink),
            0o160001..=0o160777 => Ok(Self::Gitlink),
            0o040000 => Ok(Self::Tree),
            _ => Ok(Self::File),
        }
    }

    pub const fn tree_mode(self) -> TreeMode {
        match self {
            Self::File => TreeMode::File,
            Self::Executable => TreeMode::Executable,
            Self::Symlink => TreeMode::Symlink,
            Self::Tree => TreeMode::Tree,
            Self::Gitlink => TreeMode::Gitlink,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub path: Vec<u8>,
    pub id: ObjectId,
    pub mode: IndexMode,
    pub(crate) mode_bits: u32,
    pub stage: u8,
    pub size: u32,
    pub ctime_seconds: u32,
    pub ctime_nanoseconds: u32,
    pub mtime_seconds: u32,
    pub mtime_nanoseconds: u32,
    pub dev: u32,
    pub ino: u32,
    pub uid: u32,
    pub gid: u32,
    pub(crate) flags: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveUndoStage {
    pub mode: IndexMode,
    pub id: ObjectId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveUndoEntry {
    pub path: Vec<u8>,
    pub stages: [Option<ResolveUndoStage>; 3],
}

impl IndexEntry {
    pub fn new(
        path: impl Into<Vec<u8>>,
        id: ObjectId,
        mode: IndexMode,
        size: u32,
    ) -> io::Result<Self> {
        let path = path.into();
        validate_index_path(&path)?;
        if id.algorithm() != GitHashAlgorithm::Sha1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git index v2 currently requires SHA-1 object ids",
            ));
        }
        Ok(Self {
            path,
            id,
            mode,
            mode_bits: mode.bits(),
            stage: 0,
            size,
            ctime_seconds: 0,
            ctime_nanoseconds: 0,
            mtime_seconds: 0,
            mtime_nanoseconds: 0,
            dev: 0,
            ino: 0,
            uid: 0,
            gid: 0,
            flags: 0,
        })
    }

    pub const fn mode_bits(&self) -> u32 {
        self.mode_bits
    }

    pub fn set_mode(&mut self, mode: IndexMode) {
        self.mode = mode;
        self.mode_bits = mode.bits();
    }

    pub const fn assume_valid(&self) -> bool {
        self.flags & INDEX_ENTRY_ASSUME_VALID != 0
    }

    pub fn set_assume_valid(&mut self, value: bool) {
        set_index_entry_flag(&mut self.flags, INDEX_ENTRY_ASSUME_VALID, value);
    }

    pub const fn skip_worktree(&self) -> bool {
        self.flags & INDEX_ENTRY_SKIP_WORKTREE != 0
    }

    pub fn set_skip_worktree(&mut self, value: bool) {
        set_index_entry_flag(&mut self.flags, INDEX_ENTRY_SKIP_WORKTREE, value);
    }

    pub const fn intent_to_add(&self) -> bool {
        self.flags & INDEX_ENTRY_INTENT_TO_ADD != 0
    }

    pub fn set_intent_to_add(&mut self, value: bool) {
        set_index_entry_flag(&mut self.flags, INDEX_ENTRY_INTENT_TO_ADD, value);
    }
}

const fn index_entry_flags(assume_valid: bool, skip_worktree: bool, intent_to_add: bool) -> u8 {
    (if assume_valid {
        INDEX_ENTRY_ASSUME_VALID
    } else {
        0
    }) | (if skip_worktree {
        INDEX_ENTRY_SKIP_WORKTREE
    } else {
        0
    }) | (if intent_to_add {
        INDEX_ENTRY_INTENT_TO_ADD
    } else {
        0
    })
}

fn set_index_entry_flag(flags: &mut u8, mask: u8, value: bool) {
    if value {
        *flags |= mask;
    } else {
        *flags &= !mask;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitIndex {
    entries: Vec<IndexEntry>,
    resolve_undo: Vec<ResolveUndoEntry>,
}

impl GitIndex {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            resolve_undo: Vec::new(),
        }
    }

    pub fn from_entries(entries: Vec<IndexEntry>) -> io::Result<Self> {
        Self::from_entries_and_resolve_undo(entries, Vec::new())
    }

    pub(crate) fn from_trusted_sorted_entries_unchecked(entries: Vec<IndexEntry>) -> Self {
        Self {
            entries,
            resolve_undo: Vec::new(),
        }
    }

    pub(crate) fn from_trusted_sorted_entries(entries: Vec<IndexEntry>) -> io::Result<Self> {
        validate_index_entries(&entries)?;
        validate_sorted_index_entries(&entries)?;
        Ok(Self::from_trusted_sorted_entries_unchecked(entries))
    }

    fn from_trusted_sorted_entries_and_resolve_undo(
        entries: Vec<IndexEntry>,
        resolve_undo: Vec<ResolveUndoEntry>,
    ) -> io::Result<Self> {
        validate_index_entries(&entries)?;
        validate_resolve_undo_entries(&resolve_undo)?;
        validate_sorted_index_entries(&entries)?;
        validate_sorted_resolve_undo_entries(&resolve_undo)?;
        Ok(Self {
            entries,
            resolve_undo,
        })
    }

    fn from_entries_and_resolve_undo(
        mut entries: Vec<IndexEntry>,
        mut resolve_undo: Vec<ResolveUndoEntry>,
    ) -> io::Result<Self> {
        validate_index_entries(&entries)?;
        validate_resolve_undo_entries(&resolve_undo)?;
        entries.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.stage.cmp(&right.stage))
        });
        resolve_undo.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Self {
            entries,
            resolve_undo,
        })
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    pub(crate) fn into_trusted_sorted_entries(self) -> Vec<IndexEntry> {
        self.entries
    }

    pub fn entry(&self, path: &[u8], stage: u8) -> Option<&IndexEntry> {
        self.entries
            .binary_search_by(|probe| {
                probe
                    .path
                    .as_slice()
                    .cmp(path)
                    .then(probe.stage.cmp(&stage))
            })
            .ok()
            .map(|idx| &self.entries[idx])
    }

    pub fn resolve_undo(&self) -> &[ResolveUndoEntry] {
        &self.resolve_undo
    }

    pub fn upsert(&mut self, entry: IndexEntry) -> io::Result<()> {
        validate_index_path(&entry.path)?;
        if entry.stage > 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git index stage must be 0..=3",
            ));
        }
        match self.entries.binary_search_by(|probe| {
            probe
                .path
                .cmp(&entry.path)
                .then(probe.stage.cmp(&entry.stage))
        }) {
            Ok(idx) => self.entries[idx] = entry,
            Err(idx) => self.entries.insert(idx, entry),
        }
        Ok(())
    }

    pub fn remove_path(&mut self, path: impl AsRef<[u8]>) -> io::Result<bool> {
        let path = path.as_ref();
        validate_index_path(path)?;
        let start = self
            .entries
            .partition_point(|entry| entry.path.as_slice() < path);
        let end =
            self.entries[start..].partition_point(|entry| entry.path.as_slice() == path) + start;
        if start == end {
            return Ok(false);
        }
        self.entries.drain(start..end);
        Ok(true)
    }

    pub fn remove_dir(&mut self, path: impl AsRef<[u8]>) -> io::Result<bool> {
        let path = path.as_ref();
        validate_index_path(path)?;
        let mut prefix = path.to_vec();
        prefix.push(b'/');
        let start = self
            .entries
            .partition_point(|entry| entry.path.as_slice() < prefix.as_slice());
        let end =
            self.entries[start..].partition_point(|entry| entry.path.starts_with(&prefix)) + start;
        if start == end {
            return Ok(false);
        }
        self.entries.drain(start..end);
        Ok(true)
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> io::Result<()> {
        write_index(path, self)
    }
}

pub fn write_index(path: impl AsRef<Path>, index: &GitIndex) -> io::Result<()> {
    write_index_to_path(path.as_ref(), index)
}

#[cfg(test)]
fn encode_index(index: &GitIndex) -> io::Result<Vec<u8>> {
    let mut encoded = Vec::with_capacity(estimated_index_size(index));
    encoded.extend_from_slice(INDEX_SIGNATURE);
    write_u32(&mut encoded, index_version_for_index(index));
    write_u32(&mut encoded, checked_entry_count(index.entries.len())?);

    for entry in &index.entries {
        encode_entry(&mut encoded, entry)?;
    }
    if !index.resolve_undo.is_empty() {
        encode_resolve_undo_extension(&mut encoded, &index.resolve_undo);
    }

    let digest = Sha1::digest(&encoded);
    encoded.extend_from_slice(&digest);
    Ok(encoded)
}

#[cfg(test)]
fn estimated_index_size(index: &GitIndex) -> usize {
    let entries_len = index
        .entries
        .iter()
        .map(|entry| {
            let fixed_len = ENTRY_FIXED_LEN
                + usize::from(entry.skip_worktree() || entry.intent_to_add())
                    * std::mem::size_of::<u16>();
            padded_index_entry_len(fixed_len + entry.path.len() + 1)
        })
        .sum::<usize>();
    let resolve_undo_len = if index.resolve_undo.is_empty() {
        0
    } else {
        8 + index
            .resolve_undo
            .iter()
            .map(|entry| {
                let modes_len = entry
                    .stages
                    .iter()
                    .map(|stage| {
                        stage
                            .as_ref()
                            .map(|stage| octal_u32_len(stage.mode.bits()))
                            .unwrap_or(1)
                            + 1
                    })
                    .sum::<usize>();
                let ids_len =
                    entry.stages.iter().flatten().count() * GitHashAlgorithm::Sha1.digest_len();
                entry.path.len() + 1 + modes_len + ids_len
            })
            .sum::<usize>()
    };
    12 + entries_len + resolve_undo_len + CHECKSUM_LEN
}

#[cfg(test)]
fn padded_index_entry_len(len: usize) -> usize {
    let remainder = len % 8;
    if remainder == 0 {
        len
    } else {
        len + (8 - remainder)
    }
}

#[cfg(test)]
fn encode_resolve_undo_extension(out: &mut Vec<u8>, entries: &[ResolveUndoEntry]) {
    let body_start = out.len() + 8;
    out.extend_from_slice(RESOLVE_UNDO_EXTENSION);
    write_u32(out, 0);
    for entry in entries {
        out.extend_from_slice(&entry.path);
        out.push(0);
        for stage in &entry.stages {
            if let Some(stage) = stage {
                write_octal_u32(out, stage.mode.bits());
            } else {
                out.push(b'0');
            }
            out.push(0);
        }
        for stage in entry.stages.iter().flatten() {
            out.extend_from_slice(stage.id.as_bytes());
        }
    }
    let body_len = (out.len() - body_start) as u32;
    let len_offset = body_start - 4;
    out[len_offset..len_offset + 4].copy_from_slice(&body_len.to_be_bytes());
}

fn octal_u32_len(mut value: u32) -> usize {
    let mut len = 1;
    while value >= 8 {
        value /= 8;
        len += 1;
    }
    len
}

#[cfg(test)]
fn write_octal_u32(out: &mut Vec<u8>, mut value: u32) {
    let mut buf = [0_u8; 11];
    let mut cursor = buf.len();
    if value == 0 {
        cursor -= 1;
        buf[cursor] = b'0';
    } else {
        while value > 0 {
            cursor -= 1;
            buf[cursor] = b'0' + (value & 0o7) as u8;
            value >>= 3;
        }
    }
    out.extend_from_slice(&buf[cursor..]);
}

fn write_index_to_path(path: &Path, index: &GitIndex) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = index_lock_path(path);
    let write_result = write_index_lock(&lock_path, index);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&lock_path);
        return Err(error);
    }
    if let Err(error) = replace_with_lock(&lock_path, path) {
        let _ = fs::remove_file(&lock_path);
        return Err(error);
    }
    Ok(())
}

fn write_index_lock(lock_path: &Path, index: &GitIndex) -> io::Result<()> {
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)?;
    let mut writer = IndexStreamingWriter::new(BufWriter::new(lock));
    write_index_stream(&mut writer, index)?;
    writer.finish()
}

struct IndexStreamingWriter<W> {
    inner: W,
    digest: Sha1,
    entry_start: usize,
    written: usize,
}

impl<W: Write> IndexStreamingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            digest: Sha1::new(),
            entry_start: 0,
            written: 0,
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.digest.update(bytes);
        self.inner.write_all(bytes)?;
        self.written += bytes.len();
        Ok(())
    }

    fn finish(mut self) -> io::Result<()> {
        let digest = self.digest.finalize();
        self.inner.write_all(&digest)?;
        self.inner.flush()
    }

    fn start_entry(&mut self) {
        self.entry_start = self.written;
    }

    fn entry_len(&self) -> usize {
        self.written - self.entry_start
    }
}

fn write_index_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    index: &GitIndex,
) -> io::Result<()> {
    out.write_all(INDEX_SIGNATURE)?;
    write_u32_stream(out, index_version_for_index(index))?;
    write_u32_stream(out, checked_entry_count(index.entries.len())?)?;

    for entry in &index.entries {
        encode_entry_stream(out, entry)?;
    }
    if !index.resolve_undo.is_empty() {
        encode_resolve_undo_extension_stream(out, &index.resolve_undo)?;
    }
    Ok(())
}

fn encode_resolve_undo_extension_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    entries: &[ResolveUndoEntry],
) -> io::Result<()> {
    out.write_all(RESOLVE_UNDO_EXTENSION)?;
    write_u32_stream(out, resolve_undo_extension_body_len(entries)?)?;
    for entry in entries {
        out.write_all(&entry.path)?;
        out.write_all(&[0])?;
        for stage in &entry.stages {
            if let Some(stage) = stage {
                write_octal_u32_stream(out, stage.mode.bits())?;
            } else {
                out.write_all(b"0")?;
            }
            out.write_all(&[0])?;
        }
        for stage in entry.stages.iter().flatten() {
            out.write_all(stage.id.as_bytes())?;
        }
    }
    Ok(())
}

fn resolve_undo_extension_body_len(entries: &[ResolveUndoEntry]) -> io::Result<u32> {
    let mut len = 0usize;
    for entry in entries {
        len = len
            .checked_add(entry.path.len() + 1)
            .ok_or_else(resolve_undo_extension_too_large)?;
        for stage in &entry.stages {
            let mode_len = stage
                .as_ref()
                .map(|stage| octal_u32_len(stage.mode.bits()))
                .unwrap_or(1);
            len = len
                .checked_add(mode_len + 1)
                .ok_or_else(resolve_undo_extension_too_large)?;
        }
        let ids_len = entry.stages.iter().flatten().count() * GitHashAlgorithm::Sha1.digest_len();
        len = len
            .checked_add(ids_len)
            .ok_or_else(resolve_undo_extension_too_large)?;
    }
    u32::try_from(len).map_err(|_| resolve_undo_extension_too_large())
}

fn resolve_undo_extension_too_large() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "git resolve-undo extension is too large",
    )
}

fn encode_entry_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    entry: &IndexEntry,
) -> io::Result<()> {
    validate_index_path(&entry.path)?;
    if entry.stage > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git index stage must be 0..=3",
        ));
    }

    out.start_entry();
    write_u32_stream(out, entry.ctime_seconds)?;
    write_u32_stream(out, entry.ctime_nanoseconds)?;
    write_u32_stream(out, entry.mtime_seconds)?;
    write_u32_stream(out, entry.mtime_nanoseconds)?;
    write_u32_stream(out, entry.dev)?;
    write_u32_stream(out, entry.ino)?;
    write_u32_stream(out, entry.mode_bits())?;
    write_u32_stream(out, entry.uid)?;
    write_u32_stream(out, entry.gid)?;
    write_u32_stream(out, entry.size)?;
    out.write_all(entry.id.as_bytes())?;
    write_u16_stream(out, flags_for_entry(entry))?;
    if entry.skip_worktree() || entry.intent_to_add() {
        write_u16_stream(out, extended_flags_for_entry(entry))?;
    }
    out.write_all(&entry.path)?;
    out.write_all(&[0])?;
    while !out.entry_len().is_multiple_of(8) {
        out.write_all(&[0])?;
    }
    Ok(())
}

fn write_octal_u32_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    mut value: u32,
) -> io::Result<()> {
    let mut buf = [0_u8; 11];
    let mut cursor = buf.len();
    if value == 0 {
        cursor -= 1;
        buf[cursor] = b'0';
    } else {
        while value > 0 {
            cursor -= 1;
            buf[cursor] = b'0' + (value & 0o7) as u8;
            value >>= 3;
        }
    }
    out.write_all(&buf[cursor..])
}

fn write_u32_stream<W: Write>(out: &mut IndexStreamingWriter<W>, value: u32) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

fn write_u16_stream<W: Write>(out: &mut IndexStreamingWriter<W>, value: u16) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}

fn index_lock_path(path: &Path) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(".lock");
    PathBuf::from(value)
}

#[cfg(unix)]
fn replace_with_lock(lock_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(lock_path, path)
}

#[cfg(windows)]
fn replace_with_lock(lock_path: &Path, path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let mut from = lock_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut to = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            from.as_mut_ptr(),
            to.as_mut_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_with_lock(lock_path: &Path, path: &Path) -> io::Result<()> {
    fs::rename(lock_path, path)
}

pub fn read_index(path: impl AsRef<Path>) -> io::Result<GitIndex> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() == 0 {
        return decode_index(&[]);
    }
    // The mmap is read-only and all parsing uses checked slice bounds. Git index
    // updates replace the file through index.lock, so readers see one immutable
    // snapshot for the lifetime of this parse.
    let bytes = unsafe { Mmap::map(&file)? };
    decode_index(&bytes)
}

fn decode_index(bytes: &[u8]) -> io::Result<GitIndex> {
    let (version, count, checksum_offset) = decode_index_header(bytes)?;
    let mut cursor = 12;
    let mut entries = Vec::with_capacity(index_entry_initial_capacity(count));
    let mut previous_path = Vec::new();
    for _ in 0..count {
        let (entry, next) = decode_entry(bytes, cursor, checksum_offset, version, &previous_path)?;
        previous_path.clone_from(&entry.path);
        entries.push(entry);
        cursor = next;
    }
    let resolve_undo = decode_index_extensions(bytes, cursor, checksum_offset)?;
    GitIndex::from_trusted_sorted_entries_and_resolve_undo(entries, resolve_undo)
}

fn index_entry_initial_capacity(count: usize) -> usize {
    count.min(INDEX_ENTRY_INITIAL_CAPACITY_LIMIT)
}

fn decode_index_header(bytes: &[u8]) -> io::Result<(u32, usize, usize)> {
    if bytes.len() < 12 + CHECKSUM_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index is too short",
        ));
    }
    let checksum_offset = bytes.len() - CHECKSUM_LEN;
    let expected = Sha1::digest(&bytes[..checksum_offset]);
    if expected.as_slice() != &bytes[checksum_offset..] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index checksum mismatch",
        ));
    }
    if &bytes[..4] != INDEX_SIGNATURE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index signature mismatch",
        ));
    }
    let version = read_u32(bytes, 4)?;
    if !matches!(
        version,
        INDEX_VERSION_V2 | INDEX_VERSION_V3 | INDEX_VERSION_V4
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("bad index version {version}"),
        ));
    }
    Ok((version, read_u32(bytes, 8)? as usize, checksum_offset))
}

fn decode_index_extensions(
    bytes: &[u8],
    mut cursor: usize,
    checksum_offset: usize,
) -> io::Result<Vec<ResolveUndoEntry>> {
    let mut resolve_undo = Vec::new();
    while cursor < checksum_offset {
        let header_end = cursor.checked_add(8).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension offset overflow",
            )
        })?;
        if header_end > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension header is truncated",
            ));
        }
        let signature = &bytes[cursor..cursor + 4];
        if signature.iter().any(u8::is_ascii_lowercase) && signature != SPARSE_DIRECTORY_EXTENSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index has an unsupported required extension",
            ));
        }
        let len = read_u32(bytes, cursor + 4)? as usize;
        cursor = header_end.checked_add(len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension length overflow",
            )
        })?;
        if cursor > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension body is truncated",
            ));
        }
        if signature == RESOLVE_UNDO_EXTENSION {
            resolve_undo = decode_resolve_undo_extension(&bytes[header_end..cursor])?;
        }
    }
    Ok(resolve_undo)
}

fn decode_resolve_undo_extension(body: &[u8]) -> io::Result<Vec<ResolveUndoEntry>> {
    let mut cursor = 0;
    let mut entries = Vec::new();
    while cursor < body.len() {
        let path = read_nul_terminated(body, &mut cursor, "resolve-undo path")?.to_vec();
        validate_index_path(&path)?;
        let mut modes = [None, None, None];
        for mode in &mut modes {
            let raw = read_nul_terminated(body, &mut cursor, "resolve-undo mode")?;
            *mode = parse_resolve_undo_mode(raw)?;
        }
        let mut stages: [Option<ResolveUndoStage>; 3] = [None, None, None];
        for (idx, mode) in modes.into_iter().enumerate() {
            if let Some(mode) = mode {
                let end = cursor.checked_add(CHECKSUM_LEN).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "resolve-undo object id offset overflow",
                    )
                })?;
                if end > body.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "resolve-undo object id is truncated",
                    ));
                }
                stages[idx] = Some(ResolveUndoStage {
                    mode,
                    id: ObjectId::new(GitHashAlgorithm::Sha1, &body[cursor..end]),
                });
                cursor = end;
            }
        }
        entries.push(ResolveUndoEntry { path, stages });
    }
    Ok(entries)
}

fn read_nul_terminated<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    label: &str,
) -> io::Result<&'a [u8]> {
    let start = *cursor;
    let nul = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|offset| start + offset)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("{label} missing NUL"))
        })?;
    *cursor = nul + 1;
    Ok(&bytes[start..nul])
}

fn parse_resolve_undo_mode(raw: &[u8]) -> io::Result<Option<IndexMode>> {
    if raw.is_empty() || raw == b"0" {
        return Ok(None);
    }
    let raw = std::str::from_utf8(raw).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "resolve-undo mode is not utf-8")
    })?;
    let mode = u32::from_str_radix(raw, 8).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "resolve-undo mode is not octal")
    })?;
    Ok(Some(IndexMode::from_bits(mode)?))
}

#[cfg(test)]
fn encode_entry(out: &mut Vec<u8>, entry: &IndexEntry) -> io::Result<()> {
    validate_index_path(&entry.path)?;
    if entry.stage > 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git index stage must be 0..=3",
        ));
    }

    let entry_start = out.len();
    write_u32(out, entry.ctime_seconds);
    write_u32(out, entry.ctime_nanoseconds);
    write_u32(out, entry.mtime_seconds);
    write_u32(out, entry.mtime_nanoseconds);
    write_u32(out, entry.dev);
    write_u32(out, entry.ino);
    write_u32(out, entry.mode_bits());
    write_u32(out, entry.uid);
    write_u32(out, entry.gid);
    write_u32(out, entry.size);
    out.extend_from_slice(entry.id.as_bytes());
    write_u16(out, flags_for_entry(entry));
    if entry.skip_worktree() || entry.intent_to_add() {
        write_u16(out, extended_flags_for_entry(entry));
    }
    out.extend_from_slice(&entry.path);
    out.push(0);
    while !(out.len() - entry_start).is_multiple_of(8) {
        out.push(0);
    }
    Ok(())
}

fn index_version_for_index(index: &GitIndex) -> u32 {
    if index
        .entries()
        .iter()
        .any(|entry| entry.skip_worktree() || entry.intent_to_add())
    {
        INDEX_VERSION_V3
    } else {
        INDEX_VERSION_V2
    }
}

fn decode_entry(
    bytes: &[u8],
    start: usize,
    limit: usize,
    version: u32,
    previous_path: &[u8],
) -> io::Result<(IndexEntry, usize)> {
    let fixed_end = start.checked_add(ENTRY_FIXED_LEN).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "git index entry offset overflow",
        )
    })?;
    if fixed_end > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index entry is truncated",
        ));
    }

    let flags = read_u16(bytes, start + 60)?;
    let assume_valid = flags & ENTRY_FLAG_ASSUME_VALID != 0;
    let stage = ((flags >> 12) & 0b11) as u8;
    let declared_path_len = (flags & 0x0fff) as usize;
    let mut path_start = fixed_end;
    let (skip_worktree, intent_to_add) = if flags & ENTRY_FLAG_EXTENDED != 0 {
        let extended_end = fixed_end.checked_add(2).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extended flags offset overflow",
            )
        })?;
        if extended_end > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extended flags are truncated",
            ));
        }
        let extended_flags = read_u16(bytes, fixed_end)?;
        path_start = extended_end;
        (
            extended_flags & ENTRY_EXTENDED_SKIP_WORKTREE != 0,
            extended_flags & ENTRY_EXTENDED_INTENT_TO_ADD != 0,
        )
    } else {
        (false, false)
    };
    let (path, path_nul) = if version == INDEX_VERSION_V4 {
        decode_v4_entry_path(bytes, path_start, limit, previous_path)?
    } else {
        let path_nul = bytes[path_start..limit]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| path_start + offset)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "git index path missing NUL")
            })?;
        (bytes[path_start..path_nul].to_vec(), path_nul)
    };
    if declared_path_len != 0x0fff && declared_path_len != path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index path length mismatch",
        ));
    }
    validate_index_path(&path)?;

    let mut next = path_nul + 1;
    if version != INDEX_VERSION_V4 {
        while !(next - start).is_multiple_of(8) {
            next += 1;
        }
    }
    if next > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index entry padding is truncated",
        ));
    }

    let mode_bits = read_u32(bytes, start + 24)?;
    Ok((
        IndexEntry {
            ctime_seconds: read_u32(bytes, start)?,
            ctime_nanoseconds: read_u32(bytes, start + 4)?,
            mtime_seconds: read_u32(bytes, start + 8)?,
            mtime_nanoseconds: read_u32(bytes, start + 12)?,
            dev: read_u32(bytes, start + 16)?,
            ino: read_u32(bytes, start + 20)?,
            mode: IndexMode::from_bits(mode_bits)?,
            mode_bits,
            uid: read_u32(bytes, start + 28)?,
            gid: read_u32(bytes, start + 32)?,
            size: read_u32(bytes, start + 36)?,
            id: ObjectId::new(GitHashAlgorithm::Sha1, &bytes[start + 40..start + 60]),
            flags: index_entry_flags(assume_valid, skip_worktree, intent_to_add),
            stage,
            path,
        },
        next,
    ))
}

fn decode_v4_entry_path(
    bytes: &[u8],
    path_start: usize,
    limit: usize,
    previous_path: &[u8],
) -> io::Result<(Vec<u8>, usize)> {
    let (remove_len, suffix_start) = read_index_v4_path_remove_len(bytes, path_start, limit)?;
    if remove_len > previous_path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index v4 path prefix underflow",
        ));
    }
    let path_nul = bytes[suffix_start..limit]
        .iter()
        .position(|byte| *byte == 0)
        .map(|offset| suffix_start + offset)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "git index path missing NUL"))?;
    let prefix_len = previous_path.len() - remove_len;
    let mut path = Vec::with_capacity(prefix_len + path_nul - suffix_start);
    path.extend_from_slice(&previous_path[..prefix_len]);
    path.extend_from_slice(&bytes[suffix_start..path_nul]);
    Ok((path, path_nul))
}

fn read_index_v4_path_remove_len(
    bytes: &[u8],
    start: usize,
    limit: usize,
) -> io::Result<(usize, usize)> {
    let first = *bytes.get(start).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "git index v4 path compression is truncated",
        )
    })?;
    let mut cursor = start + 1;
    let mut value = (first & 0x7f) as usize;
    let mut byte = first;
    while byte & 0x80 != 0 {
        byte = *bytes.get(cursor).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git index v4 path compression is truncated",
            )
        })?;
        cursor += 1;
        value = value
            .checked_add(1)
            .and_then(|next| next.checked_shl(7))
            .and_then(|next| next.checked_add((byte & 0x7f) as usize))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "git index v4 path compression overflow",
                )
            })?;
    }
    if cursor > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index v4 path compression is truncated",
        ));
    }
    Ok((value, cursor))
}

fn flags_for_entry(entry: &IndexEntry) -> u16 {
    let path_len = entry.path.len().min(0x0fff) as u16;
    let assume_valid = if entry.assume_valid() {
        ENTRY_FLAG_ASSUME_VALID
    } else {
        0
    };
    let extended = if entry.skip_worktree() || entry.intent_to_add() {
        ENTRY_FLAG_EXTENDED
    } else {
        0
    };
    assume_valid | extended | ((entry.stage as u16) << 12) | path_len
}

fn extended_flags_for_entry(entry: &IndexEntry) -> u16 {
    let skip_worktree = if entry.skip_worktree() {
        ENTRY_EXTENDED_SKIP_WORKTREE
    } else {
        0
    };
    let intent_to_add = if entry.intent_to_add() {
        ENTRY_EXTENDED_INTENT_TO_ADD
    } else {
        0
    };
    skip_worktree | intent_to_add
}

fn checked_entry_count(count: usize) -> io::Result<u32> {
    u32::try_from(count).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "git index has too many entries",
        )
    })
}

fn validate_index_entries(entries: &[IndexEntry]) -> io::Result<()> {
    for entry in entries {
        validate_index_path(&entry.path)?;
        if entry.stage > 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git index stage must be 0..=3",
            ));
        }
    }
    Ok(())
}

fn validate_resolve_undo_entries(entries: &[ResolveUndoEntry]) -> io::Result<()> {
    for entry in entries {
        validate_index_path(&entry.path)?;
        for stage in entry.stages.iter().flatten() {
            if stage.id.algorithm() != GitHashAlgorithm::Sha1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "git resolve-undo entries currently require SHA-1 object ids",
                ));
            }
        }
    }
    Ok(())
}

fn validate_sorted_index_entries(entries: &[IndexEntry]) -> io::Result<()> {
    for pair in entries.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if !left
            .path
            .cmp(&right.path)
            .then(left.stage.cmp(&right.stage))
            .is_lt()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index entries are not sorted",
            ));
        }
    }
    Ok(())
}

fn validate_sorted_resolve_undo_entries(entries: &[ResolveUndoEntry]) -> io::Result<()> {
    for pair in entries.windows(2) {
        if !pair[0].path.cmp(&pair[1].path).is_lt() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git resolve-undo entries are not sorted",
            ));
        }
    }
    Ok(())
}

fn validate_index_path(path: &[u8]) -> io::Result<()> {
    if path.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git index path is empty",
        ));
    }
    if path.contains(&0) || path.starts_with(b"/") || path.windows(3).any(|w| w == b"/../") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git index path is invalid",
        ));
    }
    if path == b".." || path.starts_with(b"../") || path.ends_with(b"/..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git index path escapes repository",
        ));
    }
    Ok(())
}

#[cfg(test)]
fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn read_u32(bytes: &[u8], offset: usize) -> io::Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "git index offset overflow"))?;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "git index u32 is truncated"))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u16(bytes: &[u8], offset: usize) -> io::Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "git index offset overflow"))?;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "git index u16 is truncated"))?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::{GitObjectKind, LooseObjectStore};

    #[test]
    fn writes_index_readable_by_stock_git() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"hello index\n")
            .expect("write blob");
        let entry =
            IndexEntry::new("README.md", blob.clone(), IndexMode::File, 12).expect("index entry");
        let index = GitIndex::from_entries(vec![entry]).expect("index");
        write_index(repo.path().join(".git/index"), &index).expect("write index");

        let staged = git(&repo, ["ls-files", "--stage"]);

        assert_eq!(staged, format!("100644 {} 0\tREADME.md", blob.to_hex()));
    }

    #[test]
    fn write_index_refuses_existing_lock_and_preserves_index() {
        let repo = git_init();
        let index_path = repo.path().join(".git/index");
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        let original = GitIndex::from_entries(vec![
            IndexEntry::new("first.txt", first, IndexMode::File, 0).expect("first entry"),
        ])
        .expect("original index");
        write_index(&index_path, &original).expect("write original index");
        let before = std::fs::read(&index_path).expect("read original index");
        std::fs::write(index_path.with_file_name("index.lock"), b"locked").expect("write lock");

        let replacement = GitIndex::from_entries(vec![
            IndexEntry::new("second.txt", second, IndexMode::File, 0).expect("second entry"),
        ])
        .expect("replacement index");
        let error = write_index(&index_path, &replacement).expect_err("write should fail");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(&index_path).expect("read preserved index"),
            before
        );
    }

    #[test]
    fn reads_index_written_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"from git index\n").expect("write file");
        git(&repo, ["add", "README.md"]);
        let index = read_index(repo.path().join(".git/index")).expect("read index");

        assert_eq!(index.entries().len(), 1);
        let entry = &index.entries()[0];
        assert_eq!(entry.path, b"README.md");
        assert_eq!(entry.mode, IndexMode::File);
        assert_eq!(entry.stage, 0);
        assert_eq!(entry.id.to_hex(), git(&repo, ["hash-object", "README.md"]));
    }

    #[test]
    fn reads_and_preserves_raw_regular_index_mode_bits() {
        let repo = TempDir::new().expect("temp repo");
        let index_path = repo.path().join("index");
        let id = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("a.txt", id.clone(), IndexMode::File, 6).expect("entry"),
        ])
        .expect("index");
        let mut encoded = encode_index(&index).expect("encode index");
        encoded[12 + 24..12 + 28].copy_from_slice(&0o100640u32.to_be_bytes());
        let checksum_offset = encoded.len() - CHECKSUM_LEN;
        let digest = Sha1::digest(&encoded[..checksum_offset]);
        encoded[checksum_offset..].copy_from_slice(&digest);
        std::fs::write(&index_path, encoded).expect("write raw index");

        let index = read_index(&index_path).expect("read index");
        assert_eq!(index.entries()[0].mode, IndexMode::File);
        assert_eq!(index.entries()[0].mode_bits(), 0o100640);
        write_index(&index_path, &index).expect("rewrite index");

        let rewritten = std::fs::read(&index_path).expect("read rewritten");
        assert_eq!(
            u32::from_be_bytes([
                rewritten[12 + 24],
                rewritten[12 + 25],
                rewritten[12 + 26],
                rewritten[12 + 27],
            ]),
            0o100640
        );
    }

    #[test]
    fn streaming_index_writer_matches_buffer_encoder() {
        let repo = TempDir::new().expect("temp repo");
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        let third = ObjectId::new(GitHashAlgorithm::Sha1, &[3; 20]);
        let mut extended =
            IndexEntry::new("b.txt", second.clone(), IndexMode::Executable, 7).expect("entry");
        extended.set_assume_valid(true);
        extended.set_skip_worktree(true);
        let index = GitIndex {
            entries: vec![
                IndexEntry::new("a.txt", first.clone(), IndexMode::File, 3).expect("entry"),
                extended,
            ],
            resolve_undo: vec![ResolveUndoEntry {
                path: b"a.txt".to_vec(),
                stages: [
                    Some(ResolveUndoStage {
                        mode: IndexMode::File,
                        id: first,
                    }),
                    None,
                    Some(ResolveUndoStage {
                        mode: IndexMode::Executable,
                        id: third,
                    }),
                ],
            }],
        };
        let index_path = repo.path().join("index");

        write_index(&index_path, &index).expect("stream index");

        assert_eq!(
            std::fs::read(&index_path).expect("read streamed index"),
            encode_index(&index).expect("encode index")
        );
        assert_eq!(read_index(&index_path).expect("read index"), index);
    }

    #[test]
    fn index_entry_initial_capacity_is_bounded() {
        assert_eq!(index_entry_initial_capacity(0), 0);
        assert_eq!(index_entry_initial_capacity(2), 2);
        assert_eq!(
            index_entry_initial_capacity(usize::MAX),
            INDEX_ENTRY_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn resolve_undo_octal_helpers_match_format_output() {
        for value in [0, 1, 7, 8, 0o100644, 0o100755, 0o120000, 0o160000, u32::MAX] {
            let expected = format!("{value:o}");
            let mut encoded = Vec::new();
            write_octal_u32(&mut encoded, value);

            assert_eq!(octal_u32_len(value), expected.len());
            assert_eq!(encoded, expected.as_bytes());
        }
    }

    #[test]
    fn reads_and_preserves_resolve_undo_extension_written_by_stock_git() {
        let repo = git_init();
        git(&repo, ["config", "user.name", "Bench"]);
        git(&repo, ["config", "user.email", "bench@example.test"]);
        git(&repo, ["config", "commit.gpgsign", "false"]);
        let base_branch = git(&repo, ["symbolic-ref", "--short", "HEAD"]);
        std::fs::write(repo.path().join("f.txt"), b"base\n").expect("write base");
        git(&repo, ["add", "f.txt"]);
        git(&repo, ["commit", "-m", "base"]);
        git(&repo, ["checkout", "-b", "left"]);
        std::fs::write(repo.path().join("f.txt"), b"left\n").expect("write left");
        git(&repo, ["commit", "-am", "left"]);
        git(&repo, ["checkout", &base_branch]);
        git(&repo, ["checkout", "-b", "right"]);
        std::fs::write(repo.path().join("f.txt"), b"right\n").expect("write right");
        git(&repo, ["commit", "-am", "right"]);
        let merge = Command::new("git")
            .args(["merge", "left"])
            .current_dir(repo.path())
            .output()
            .expect("run merge");
        assert!(!merge.status.success(), "merge should conflict");
        std::fs::write(repo.path().join("f.txt"), b"resolved\n").expect("write resolved");
        git(&repo, ["add", "f.txt"]);

        let index_path = repo.path().join(".git/index");
        let before = git(&repo, ["ls-files", "--resolve-undo"]);
        let index = read_index(&index_path).expect("read index");
        assert_eq!(index.resolve_undo().len(), 1);
        assert_eq!(index.resolve_undo()[0].path, b"f.txt");
        assert!(index.resolve_undo()[0].stages.iter().all(Option::is_some));
        write_index(&index_path, &index).expect("rewrite index");

        assert_eq!(git(&repo, ["ls-files", "--resolve-undo"]), before);
    }

    #[test]
    fn preserves_assume_valid_index_flag_written_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"from git index\n").expect("write file");
        git(&repo, ["add", "README.md"]);
        git(&repo, ["update-index", "--assume-unchanged", "README.md"]);

        let index_path = repo.path().join(".git/index");
        let index = read_index(&index_path).expect("read index");
        assert!(index.entries()[0].assume_valid());
        write_index(&index_path, &index).expect("rewrite index");

        assert_eq!(git(&repo, ["ls-files", "-v"]), "h README.md");
    }

    #[test]
    fn preserves_skip_worktree_index_flag_written_by_stock_git() {
        let repo = git_init();
        std::fs::write(repo.path().join("README.md"), b"from git index\n").expect("write file");
        git(&repo, ["add", "README.md"]);
        git(&repo, ["update-index", "--skip-worktree", "README.md"]);

        let index_path = repo.path().join(".git/index");
        let index = read_index(&index_path).expect("read index");
        assert!(index.entries()[0].skip_worktree());
        write_index(&index_path, &index).expect("rewrite index");

        assert_eq!(git(&repo, ["ls-files", "-v"]), "S README.md");
    }

    #[test]
    fn removes_exact_paths_and_directories() {
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        let third = ObjectId::new(GitHashAlgorithm::Sha1, &[3; 20]);
        let mut index = GitIndex::from_entries(vec![
            IndexEntry::new("README.md", first, IndexMode::File, 0).expect("entry"),
            IndexEntry::new("docs/a.md", second, IndexMode::File, 0).expect("entry"),
            IndexEntry::new("docs/nested/b.md", third, IndexMode::File, 0).expect("entry"),
        ])
        .expect("index");

        assert!(index.remove_path(b"README.md").expect("remove file"));
        assert!(index.remove_dir(b"docs").expect("remove dir"));
        assert!(index.entries().is_empty());
    }

    #[test]
    fn remove_path_and_dir_use_sorted_ranges_precisely() {
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        let third = ObjectId::new(GitHashAlgorithm::Sha1, &[3; 20]);
        let fourth = ObjectId::new(GitHashAlgorithm::Sha1, &[4; 20]);
        let fifth = ObjectId::new(GitHashAlgorithm::Sha1, &[5; 20]);
        let mut conflicted =
            IndexEntry::new("docs/a.md", second, IndexMode::File, 0).expect("conflicted entry");
        conflicted.stage = 2;
        let mut index = GitIndex::from_entries(vec![
            IndexEntry::new("docs", first, IndexMode::File, 0).expect("docs file"),
            conflicted,
            IndexEntry::new("docs/a.md", third, IndexMode::File, 0).expect("docs entry"),
            IndexEntry::new("docs/nested/b.md", fourth, IndexMode::File, 0).expect("nested entry"),
            IndexEntry::new("docs2/a.md", fifth, IndexMode::File, 0).expect("sibling entry"),
        ])
        .expect("index");

        assert!(index.remove_path(b"docs/a.md").expect("remove path"));
        assert_eq!(
            index
                .entries()
                .iter()
                .map(|entry| (entry.path.as_slice(), entry.stage))
                .collect::<Vec<_>>(),
            vec![
                (b"docs".as_slice(), 0),
                (b"docs/nested/b.md".as_slice(), 0),
                (b"docs2/a.md".as_slice(), 0)
            ]
        );
        assert!(index.remove_dir(b"docs").expect("remove dir"));
        assert_eq!(
            index
                .entries()
                .iter()
                .map(|entry| entry.path.as_slice())
                .collect::<Vec<_>>(),
            vec![b"docs".as_slice(), b"docs2/a.md".as_slice()]
        );
        assert!(!index.remove_path(b"missing").expect("missing path"));
        assert!(!index.remove_dir(b"missing").expect("missing dir"));
    }

    fn git_init() -> TempDir {
        let repo = TempDir::new().expect("temp repo");
        let output = Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(repo.path())
            .output()
            .expect("run git init");
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        repo
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git stdout utf8")
            .trim_end_matches('\n')
            .to_owned()
    }
}
