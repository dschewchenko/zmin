use std::ffi::OsString;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::{cmp::Ordering, collections::BTreeMap};

use memmap2::Mmap;

use crate::object::{GitHashAlgorithm, GitObjectHash, GitObjectKind, ObjectId};
use crate::tree::{TreeEntry, TreeMode, encode_tree};

const INDEX_SIGNATURE: &[u8; 4] = b"DIRC";
const INDEX_VERSION_V2: u32 = 2;
const INDEX_VERSION_V3: u32 = 3;
const INDEX_VERSION_V4: u32 = 4;
const ENTRY_METADATA_LEN: usize = 40;
const ENTRY_FLAGS_LEN: usize = 2;
const ENTRY_FLAG_ASSUME_VALID: u16 = 0x8000;
const ENTRY_FLAG_EXTENDED: u16 = 0x4000;
const ENTRY_EXTENDED_SKIP_WORKTREE: u16 = 0x4000;
const ENTRY_EXTENDED_INTENT_TO_ADD: u16 = 0x2000;
const INDEX_ENTRY_ASSUME_VALID: u8 = 0b001;
const INDEX_ENTRY_SKIP_WORKTREE: u8 = 0b010;
const INDEX_ENTRY_INTENT_TO_ADD: u8 = 0b100;
const CACHE_TREE_MAX_COMPONENT_DEPTH: usize = 128;
const RESOLVE_UNDO_EXTENSION: &[u8; 4] = b"REUC";
const CACHE_TREE_EXTENSION: &[u8; 4] = b"TREE";
const SPARSE_DIRECTORY_EXTENSION: &[u8; 4] = b"sdir";
const SPLIT_INDEX_LINK_EXTENSION: &[u8; 4] = b"link";
const SPLIT_INDEX_EMPTY_BITMAP: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, // bit_size
    0x00, 0x00, 0x00, 0x01, // buffer_size
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // single zero word
    0x00, 0x00, 0x00, 0x00, // rlw index
];
const SPLIT_INDEX_SINGLE_ENTRY_TAIL: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
];
const INDEX_ENTRY_INITIAL_CAPACITY_LIMIT: usize = 8192;

fn index_phase_trace_enabled() -> bool {
    std::env::var_os("ZMIN_PHASE_TRACE").is_some()
}

fn index_phase_started() -> Option<std::time::Instant> {
    index_phase_trace_enabled().then(std::time::Instant::now)
}

fn index_phase_emit(name: &str, started: Option<std::time::Instant>) {
    if let Some(started) = started {
        eprintln!(
            "zmin-phase\t{name}\tseconds={:.6}",
            started.elapsed().as_secs_f64()
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitIndexVersion {
    V2,
    V3,
    V4,
}

impl GitIndexVersion {
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::V2 => INDEX_VERSION_V2,
            Self::V3 => INDEX_VERSION_V3,
            Self::V4 => INDEX_VERSION_V4,
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndex {
    hash_algorithm: GitHashAlgorithm,
    entries: Vec<IndexEntry>,
    resolve_undo: Vec<ResolveUndoEntry>,
    cache_tree: Option<IndexCacheTree>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexCacheTree {
    entry_count: i32,
    oid: ObjectId,
    subtrees: Vec<IndexCacheTreeChild>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexCacheTreeChild {
    name: Vec<u8>,
    cache_tree: IndexCacheTree,
}

impl GitIndex {
    pub fn new() -> Self {
        Self::new_with_algorithm(GitHashAlgorithm::Sha1)
    }

    pub fn new_with_algorithm(hash_algorithm: GitHashAlgorithm) -> Self {
        Self {
            hash_algorithm,
            entries: Vec::new(),
            resolve_undo: Vec::new(),
            cache_tree: build_cache_tree(hash_algorithm, &[]),
        }
    }

    pub fn from_entries(entries: Vec<IndexEntry>) -> io::Result<Self> {
        Self::from_entries_and_resolve_undo(entries, Vec::new())
    }

    pub(crate) fn from_trusted_sorted_entries_unchecked(entries: Vec<IndexEntry>) -> Self {
        let hash_algorithm = index_entries_algorithm(&entries).unwrap_or(GitHashAlgorithm::Sha1);
        Self {
            hash_algorithm,
            cache_tree: build_cache_tree(hash_algorithm, &entries),
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
            hash_algorithm: index_entries_algorithm(&entries).unwrap_or(GitHashAlgorithm::Sha1),
            cache_tree: build_cache_tree(
                index_entries_algorithm(&entries).unwrap_or(GitHashAlgorithm::Sha1),
                &entries,
            ),
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
            hash_algorithm: index_entries_algorithm(&entries).unwrap_or(GitHashAlgorithm::Sha1),
            cache_tree: build_cache_tree(
                index_entries_algorithm(&entries).unwrap_or(GitHashAlgorithm::Sha1),
                &entries,
            ),
            entries,
            resolve_undo,
        })
    }

    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    pub const fn hash_algorithm(&self) -> GitHashAlgorithm {
        self.hash_algorithm
    }

    pub fn cached_root_tree_id(&self) -> Option<&ObjectId> {
        self.cache_tree
            .as_ref()
            .filter(|root| root.entry_count >= 0)
            .map(|root| &root.oid)
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

    pub fn take_resolve_undo(
        &mut self,
        path: impl AsRef<[u8]>,
    ) -> io::Result<Option<ResolveUndoEntry>> {
        let path = path.as_ref();
        validate_index_path(path)?;
        match self
            .resolve_undo
            .binary_search_by(|probe| probe.path.as_slice().cmp(path))
        {
            Ok(idx) => Ok(Some(self.resolve_undo.remove(idx))),
            Err(_) => Ok(None),
        }
    }

    pub fn upsert_resolve_undo(&mut self, entry: ResolveUndoEntry) -> io::Result<()> {
        validate_index_path(&entry.path)?;
        validate_resolve_undo_entries(std::slice::from_ref(&entry))?;
        match self
            .resolve_undo
            .binary_search_by(|probe| probe.path.as_slice().cmp(entry.path.as_slice()))
        {
            Ok(idx) => self.resolve_undo[idx] = entry,
            Err(idx) => self.resolve_undo.insert(idx, entry),
        }
        Ok(())
    }

    pub fn upsert(&mut self, entry: IndexEntry) -> io::Result<()> {
        validate_index_path(&entry.path)?;
        if entry.id.algorithm() != self.hash_algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git index entry object format does not match the index",
            ));
        }
        if entry.stage > 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git index stage must be 0..=3",
            ));
        }
        let invalidate_path = entry.path.clone();
        match self.entries.binary_search_by(|probe| {
            probe
                .path
                .cmp(&entry.path)
                .then(probe.stage.cmp(&entry.stage))
        }) {
            Ok(idx) => self.entries[idx] = entry,
            Err(idx) => self.entries.insert(idx, entry),
        }
        invalidate_cache_tree_path(self.cache_tree.as_mut(), &invalidate_path);
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
        invalidate_cache_tree_path(self.cache_tree.as_mut(), path);
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
        invalidate_cache_tree_path(self.cache_tree.as_mut(), path);
        Ok(true)
    }

    pub fn write_to_path(&self, path: impl AsRef<Path>) -> io::Result<()> {
        write_index(path, self)
    }

    pub fn write_to_path_with_version(
        &self,
        path: impl AsRef<Path>,
        version: GitIndexVersion,
    ) -> io::Result<()> {
        write_index_with_version(path, self, version)
    }

    pub fn write_to_path_without_split(&self, path: impl AsRef<Path>) -> io::Result<()> {
        write_index_to_path_without_split(path.as_ref(), self, None)
    }

    pub fn write_to_path_without_split_with_version(
        &self,
        path: impl AsRef<Path>,
        version: GitIndexVersion,
    ) -> io::Result<()> {
        write_index_to_path_without_split(path.as_ref(), self, Some(version))
    }

    pub fn refresh_cache_tree(&mut self) {
        self.cache_tree = build_cache_tree(self.hash_algorithm, &self.entries);
    }

    pub fn clear_cache_tree(&mut self) {
        self.cache_tree = None;
    }
}

impl Default for GitIndex {
    fn default() -> Self {
        Self::new()
    }
}

pub fn write_index(path: impl AsRef<Path>, index: &GitIndex) -> io::Result<()> {
    write_index_to_path(path.as_ref(), index, None)
}

pub fn write_index_with_version(
    path: impl AsRef<Path>,
    index: &GitIndex,
    version: GitIndexVersion,
) -> io::Result<()> {
    write_index_to_path(path.as_ref(), index, Some(version))
}

fn encode_index(index: &GitIndex) -> io::Result<Vec<u8>> {
    encode_index_with_version(index, None)
}

fn encode_index_with_version(
    index: &GitIndex,
    forced_version: Option<GitIndexVersion>,
) -> io::Result<Vec<u8>> {
    let mut encoded = Vec::with_capacity(estimated_index_size(index));
    encoded.extend_from_slice(INDEX_SIGNATURE);
    write_u32(
        &mut encoded,
        forced_version
            .map(GitIndexVersion::as_u32)
            .unwrap_or_else(|| index_version_for_index(index)),
    );
    write_u32(&mut encoded, checked_entry_count(index.entries.len())?);

    for entry in &index.entries {
        encode_entry(&mut encoded, entry)?;
    }
    if !index.resolve_undo.is_empty() {
        encode_resolve_undo_extension(&mut encoded, &index.resolve_undo);
    }
    if let Some(cache_tree) = index
        .cache_tree
        .as_ref()
        .filter(|tree| tree.entry_count > 0)
    {
        encode_cache_tree_extension(&mut encoded, cache_tree)?;
    }

    let mut hasher = GitObjectHash::new(index.hash_algorithm);
    hasher.update(&encoded);
    let digest = hasher.finalize();
    encoded.extend_from_slice(digest.as_bytes());
    Ok(encoded)
}

fn estimated_index_size(index: &GitIndex) -> usize {
    let entries_len = index
        .entries
        .iter()
        .map(|entry| {
            let fixed_len = entry_fixed_len(index.hash_algorithm)
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
                    entry.stages.iter().flatten().count() * index.hash_algorithm.digest_len();
                entry.path.len() + 1 + modes_len + ids_len
            })
            .sum::<usize>()
    };
    let cache_tree_len = index
        .cache_tree
        .as_ref()
        .filter(|tree| tree.entry_count > 0)
        .map(estimated_cache_tree_extension_len)
        .unwrap_or(0);
    12 + entries_len + resolve_undo_len + cache_tree_len + index.hash_algorithm.digest_len()
}

const fn entry_fixed_len(algorithm: GitHashAlgorithm) -> usize {
    ENTRY_METADATA_LEN + algorithm.digest_len() + ENTRY_FLAGS_LEN
}

fn padded_index_entry_len(len: usize) -> usize {
    let remainder = len % 8;
    if remainder == 0 {
        len
    } else {
        len + (8 - remainder)
    }
}

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

fn encode_cache_tree_extension(out: &mut Vec<u8>, root: &IndexCacheTree) -> io::Result<()> {
    let body_start = out.len() + 8;
    out.extend_from_slice(CACHE_TREE_EXTENSION);
    write_u32(out, 0);
    encode_cache_tree_node(out, root, &[])?;
    let body_len = (out.len() - body_start) as u32;
    let len_offset = body_start - 4;
    out[len_offset..len_offset + 4].copy_from_slice(&body_len.to_be_bytes());
    Ok(())
}

fn encode_cache_tree_node(out: &mut Vec<u8>, node: &IndexCacheTree, path: &[u8]) -> io::Result<()> {
    out.extend_from_slice(path);
    out.push(0);
    out.extend_from_slice(node.entry_count.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(node.subtrees.len().to_string().as_bytes());
    out.push(b'\n');
    if node.entry_count >= 0 {
        out.extend_from_slice(node.oid.as_bytes());
    }
    for child in &node.subtrees {
        encode_cache_tree_node(out, &child.cache_tree, &child.name)?;
    }
    Ok(())
}

fn estimated_cache_tree_extension_len(root: &IndexCacheTree) -> usize {
    8 + estimated_cache_tree_node_len(root)
}

fn estimated_cache_tree_node_len(node: &IndexCacheTree) -> usize {
    1 + decimal_i32_len(node.entry_count)
        + 1
        + decimal_usize_len(node.subtrees.len())
        + 1
        + usize::from(node.entry_count >= 0) * node.oid.algorithm().digest_len()
        + node
            .subtrees
            .iter()
            .map(|child| child.name.len() + estimated_cache_tree_node_len(&child.cache_tree))
            .sum::<usize>()
}

fn decimal_i32_len(value: i32) -> usize {
    if value < 0 {
        1 + decimal_u32_len(value.unsigned_abs())
    } else {
        decimal_u32_len(value as u32)
    }
}

fn decimal_u32_len(mut value: u32) -> usize {
    let mut len = 1;
    while value >= 10 {
        value /= 10;
        len += 1;
    }
    len
}

fn decimal_usize_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 10 {
        value /= 10;
        len += 1;
    }
    len
}

fn octal_u32_len(mut value: u32) -> usize {
    let mut len = 1;
    while value >= 8 {
        value /= 8;
        len += 1;
    }
    len
}

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

fn write_index_to_path(
    path: &Path,
    index: &GitIndex,
    version: Option<GitIndexVersion>,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(metadata) = split_index_metadata(path, index.hash_algorithm)? {
        return write_split_index_to_path(path, index, version, metadata);
    }
    let lock_path = index_lock_path(path);
    let write_result = write_index_lock(&lock_path, index, version);
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

fn write_index_to_path_without_split(
    path: &Path,
    index: &GitIndex,
    version: Option<GitIndexVersion>,
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = index_lock_path(path);
    let write_result = write_index_lock(&lock_path, index, version);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SplitIndexMode {
    SingleEntry,
    EmptyOverlay,
}

struct SplitIndexMetadata {
    shared_hash: Vec<u8>,
    shared_path: PathBuf,
    base_index: GitIndex,
    delete_positions: Vec<usize>,
    replace_positions: Vec<usize>,
}

fn detect_split_index_mode(path: &Path) -> io::Result<Option<SplitIndexMode>> {
    Ok(
        split_index_metadata(path, GitHashAlgorithm::Sha1)?.map(|metadata| {
            if metadata.base_index.entries.is_empty() {
                SplitIndexMode::EmptyOverlay
            } else {
                SplitIndexMode::SingleEntry
            }
        }),
    )
}

fn split_index_metadata(
    path: &Path,
    algorithm: GitHashAlgorithm,
) -> io::Result<Option<SplitIndexMetadata>> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(None);
    };
    let Some(link) = parse_split_index_link(path, &bytes, algorithm)? else {
        return Ok(None);
    };
    let Some(shared_path) = split_index_shared_path(path, &link.shared_hash) else {
        return Ok(None);
    };
    let base_index = read_index_with_algorithm(&shared_path, algorithm)?;
    Ok(Some(SplitIndexMetadata {
        shared_hash: link.shared_hash,
        shared_path,
        base_index,
        delete_positions: link.delete_positions,
        replace_positions: link.replace_positions,
    }))
}

fn write_split_index_to_path(
    path: &Path,
    index: &GitIndex,
    version: Option<GitIndexVersion>,
    metadata: SplitIndexMetadata,
) -> io::Result<()> {
    let mut local_entries = Vec::new();
    let mut delete_positions = metadata.delete_positions.clone();
    let mut replace_positions = Vec::new();
    let mut matched_current = vec![false; index.entries.len()];

    for (position, base_entry) in metadata.base_index.entries.iter().enumerate() {
        if metadata.delete_positions.binary_search(&position).is_ok() {
            continue;
        }
        let current =
            index.entries.iter().enumerate().find(|(_, entry)| {
                entry.path == base_entry.path && entry.stage == base_entry.stage
            });
        match current {
            None => {
                delete_positions.push(position);
            }
            Some((current_position, current)) if current == base_entry => {
                matched_current[current_position] = true;
            }
            Some((current_position, current)) => {
                matched_current[current_position] = true;
                let mut replacement = current.clone();
                replacement.path.clear();
                local_entries.push(replacement);
                replace_positions.push(position);
            }
        }
    }
    for (position, current) in index.entries.iter().enumerate() {
        if !matched_current[position] {
            local_entries.push(current.clone());
        }
    }
    delete_positions.sort_unstable();
    delete_positions.dedup();
    replace_positions.sort_unstable();
    replace_positions.dedup();
    local_entries.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.stage.cmp(&right.stage))
    });

    let lock_path = index_lock_path(path);
    let local_write_result = write_split_index_lock(
        &lock_path,
        index.hash_algorithm,
        version,
        &local_entries,
        &metadata.shared_hash,
        &delete_positions,
        &replace_positions,
    );
    if let Err(error) = local_write_result {
        let _ = fs::remove_file(&lock_path);
        return Err(error);
    }
    if let Err(error) = replace_with_lock(&lock_path, path) {
        let _ = fs::remove_file(&lock_path);
        return Err(error);
    }
    Ok(())
}

fn write_split_index_lock(
    lock_path: &Path,
    algorithm: GitHashAlgorithm,
    version: Option<GitIndexVersion>,
    entries: &[IndexEntry],
    shared_hash: &[u8],
    delete_positions: &[usize],
    replace_positions: &[usize],
) -> io::Result<()> {
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)?;
    let mut writer = IndexStreamingWriter::new(BufWriter::new(lock), algorithm);
    write_split_index_stream(
        &mut writer,
        algorithm,
        version,
        entries,
        shared_hash,
        delete_positions,
        replace_positions,
    )?;
    writer.finish()
}

fn write_split_index_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    algorithm: GitHashAlgorithm,
    forced_version: Option<GitIndexVersion>,
    entries: &[IndexEntry],
    shared_hash: &[u8],
    delete_positions: &[usize],
    replace_positions: &[usize],
) -> io::Result<()> {
    let version = forced_version
        .map(GitIndexVersion::as_u32)
        .unwrap_or_else(|| {
            if entries.iter().any(|entry| entry.path.len() > 0xfff) {
                INDEX_VERSION_V4
            } else {
                INDEX_VERSION_V2
            }
        });
    out.write_all(INDEX_SIGNATURE)?;
    write_u32_stream(out, version)?;
    write_u32_stream(
        out,
        u32::try_from(entries.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "split-index entry count is too large",
            )
        })?,
    )?;
    let mut previous_path = Vec::new();
    for entry in entries {
        encode_split_entry_stream(out, entry, version, &previous_path, algorithm)?;
        previous_path.clone_from(&entry.path);
    }
    out.write_all(SPLIT_INDEX_LINK_EXTENSION)?;
    let link_body = split_index_link_body(shared_hash, delete_positions, replace_positions);
    write_u32_stream(
        out,
        u32::try_from(link_body.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "split-index link body is too large",
            )
        })?,
    )?;
    out.write_all(&link_body)?;
    Ok(())
}

fn encode_split_single_entry_stream<W: Write>(
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
    let path_len = if entry.path.is_empty() {
        0
    } else {
        entry.path.len().min(0x0fff) as u16
    };
    write_u16_stream(out, (flags_for_entry(entry) & !0x0fff) | path_len)?;
    if entry.skip_worktree() || entry.intent_to_add() {
        write_u16_stream(out, extended_flags_for_entry(entry))?;
    }
    out.write_all(&[0])?;
    while !out.entry_len().is_multiple_of(8) {
        out.write_all(&[0])?;
    }
    Ok(())
}

fn encode_split_entry_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    entry: &IndexEntry,
    version: u32,
    previous_path: &[u8],
    _algorithm: GitHashAlgorithm,
) -> io::Result<()> {
    if version == INDEX_VERSION_V4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "split index does not support version 4",
        ));
    }
    if !entry.path.is_empty() {
        validate_index_path(&entry.path)?;
    }
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
    let path_len = if entry.path.is_empty() {
        0
    } else {
        entry.path.len().min(0x0fff) as u16
    };
    write_u16_stream(out, (flags_for_entry(entry) & !0x0fff) | path_len)?;
    if entry.skip_worktree() || entry.intent_to_add() {
        write_u16_stream(out, extended_flags_for_entry(entry))?;
    }
    out.write_all(&entry.path)?;
    out.write_all(&[0])?;
    while !out.entry_len().is_multiple_of(8) {
        out.write_all(&[0])?;
    }
    let _ = previous_path;
    Ok(())
}

fn split_index_link_body(
    shared_hash: &[u8],
    delete_positions: &[usize],
    replace_positions: &[usize],
) -> Vec<u8> {
    let delete_bitmap = encode_ewah_positions(delete_positions);
    let replace_bitmap = encode_ewah_positions(replace_positions);
    let mut body =
        Vec::with_capacity(shared_hash.len() + delete_bitmap.len() + replace_bitmap.len());
    body.extend_from_slice(shared_hash);
    body.extend_from_slice(&delete_bitmap);
    body.extend_from_slice(&replace_bitmap);
    body
}

fn encode_ewah_positions(positions: &[usize]) -> Vec<u8> {
    if positions.is_empty() {
        return SPLIT_INDEX_EMPTY_BITMAP.to_vec();
    }
    let bit_size = positions.iter().copied().max().unwrap_or(0) + 1;
    let word_count = bit_size.div_ceil(64);
    let mut words = vec![0_u64; word_count];
    for &position in positions {
        words[position / 64] |= 1_u64 << (position % 64);
    }
    let mut compressed = Vec::with_capacity(word_count * 2);
    for word in words {
        compressed.push(1_u64 << 33);
        compressed.push(word);
    }
    let mut encoded = Vec::with_capacity(12 + compressed.len() * 8);
    encoded.extend_from_slice(&(bit_size as u32).to_be_bytes());
    encoded.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    for word in compressed {
        encoded.extend_from_slice(&word.to_be_bytes());
    }
    encoded.extend_from_slice(&0_u32.to_be_bytes());
    encoded
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn write_index_lock(
    lock_path: &Path,
    index: &GitIndex,
    version: Option<GitIndexVersion>,
) -> io::Result<()> {
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)?;
    let mut writer = IndexStreamingWriter::new(BufWriter::new(lock), index.hash_algorithm);
    write_index_stream(&mut writer, index, version)?;
    writer.finish()
}

struct IndexStreamingWriter<W> {
    inner: W,
    digest: GitObjectHash,
    entry_start: usize,
    written: usize,
}

impl<W: Write> IndexStreamingWriter<W> {
    fn new(inner: W, algorithm: GitHashAlgorithm) -> Self {
        Self {
            inner,
            digest: GitObjectHash::new(algorithm),
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
        self.inner.write_all(digest.as_bytes())?;
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
    forced_version: Option<GitIndexVersion>,
) -> io::Result<()> {
    let version = forced_version
        .map(GitIndexVersion::as_u32)
        .unwrap_or_else(|| index_version_for_index(index));
    out.write_all(INDEX_SIGNATURE)?;
    write_u32_stream(out, version)?;
    write_u32_stream(out, checked_entry_count(index.entries.len())?)?;

    let mut previous_path = Vec::new();
    for entry in &index.entries {
        encode_entry_stream(out, entry, version, &previous_path)?;
        previous_path.clone_from(&entry.path);
    }
    if !index.resolve_undo.is_empty() {
        encode_resolve_undo_extension_stream(out, &index.resolve_undo)?;
    }
    if let Some(cache_tree) = index
        .cache_tree
        .as_ref()
        .filter(|tree| tree.entry_count > 0)
    {
        encode_cache_tree_extension_stream(out, cache_tree)?;
    }
    Ok(())
}

fn encode_cache_tree_extension_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    root: &IndexCacheTree,
) -> io::Result<()> {
    out.write_all(CACHE_TREE_EXTENSION)?;
    write_u32_stream(out, cache_tree_extension_body_len(root)?)?;
    encode_cache_tree_node_stream(out, root, &[])
}

fn encode_cache_tree_node_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    node: &IndexCacheTree,
    path: &[u8],
) -> io::Result<()> {
    out.write_all(path)?;
    out.write_all(&[0])?;
    write_decimal_i32_stream(out, node.entry_count)?;
    out.write_all(&[b' '])?;
    write_decimal_usize_stream(out, node.subtrees.len())?;
    out.write_all(&[b'\n'])?;
    if node.entry_count >= 0 {
        out.write_all(node.oid.as_bytes())?;
    }
    for child in &node.subtrees {
        encode_cache_tree_node_stream(out, &child.cache_tree, &child.name)?;
    }
    Ok(())
}

fn cache_tree_extension_body_len(root: &IndexCacheTree) -> io::Result<u32> {
    u32::try_from(cache_tree_node_len(root)).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "git cache-tree extension is too large",
        )
    })
}

fn cache_tree_node_len(node: &IndexCacheTree) -> usize {
    1 + decimal_i32_stream_len(node.entry_count)
        + 1
        + decimal_usize_stream_len(node.subtrees.len())
        + 1
        + usize::from(node.entry_count >= 0) * node.oid.algorithm().digest_len()
        + node
            .subtrees
            .iter()
            .map(|child| child.name.len() + cache_tree_node_len(&child.cache_tree))
            .sum::<usize>()
}

fn decimal_i32_stream_len(value: i32) -> usize {
    if value < 0 {
        1 + decimal_u32_stream_len(value.unsigned_abs())
    } else {
        decimal_u32_stream_len(value as u32)
    }
}

fn decimal_u32_stream_len(mut value: u32) -> usize {
    let mut len = 1;
    while value >= 10 {
        value /= 10;
        len += 1;
    }
    len
}

fn decimal_usize_stream_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 10 {
        value /= 10;
        len += 1;
    }
    len
}

fn write_decimal_i32_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    value: i32,
) -> io::Result<()> {
    out.write_all(value.to_string().as_bytes())
}

fn write_decimal_usize_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    value: usize,
) -> io::Result<()> {
    out.write_all(value.to_string().as_bytes())
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
        let ids_len = entry
            .stages
            .iter()
            .flatten()
            .map(|stage| stage.id.algorithm().digest_len())
            .sum::<usize>();
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
    version: u32,
    previous_path: &[u8],
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
    if version == INDEX_VERSION_V4 {
        encode_v4_entry_path_stream(out, &entry.path, previous_path)?;
    } else {
        out.write_all(&entry.path)?;
    }
    out.write_all(&[0])?;
    while version != INDEX_VERSION_V4 && !out.entry_len().is_multiple_of(8) {
        out.write_all(&[0])?;
    }
    Ok(())
}

fn encode_v4_entry_path_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    path: &[u8],
    previous_path: &[u8],
) -> io::Result<()> {
    let prefix_len = common_prefix_len(path, previous_path);
    let remove_len = previous_path.len() - prefix_len;
    write_index_v4_path_remove_len_stream(out, remove_len)?;
    out.write_all(&path[prefix_len..])
}

fn write_index_v4_path_remove_len_stream<W: Write>(
    out: &mut IndexStreamingWriter<W>,
    value: usize,
) -> io::Result<()> {
    let mut parts = vec![(value & 0x7f) as u8];
    let mut value = value >> 7;
    while value != 0 {
        value -= 1;
        parts.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    for byte in parts.into_iter().rev() {
        out.write_all(&[byte])?;
    }
    Ok(())
}

fn common_prefix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
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
    read_index_with_algorithm(path, GitHashAlgorithm::Sha1)
}

pub fn read_index_with_algorithm(
    path: impl AsRef<Path>,
    algorithm: GitHashAlgorithm,
) -> io::Result<GitIndex> {
    let total_started = index_phase_started();
    let path = path.as_ref();
    let open_started = index_phase_started();
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    index_phase_emit("index.read.open", open_started);
    if metadata.len() == 0 {
        index_phase_emit("index.read.total", total_started);
        return decode_index(path, &[], algorithm);
    }
    // The mmap is read-only and all parsing uses checked slice bounds. Git index
    // updates replace the file through index.lock, so readers see one immutable
    // snapshot for the lifetime of this parse.
    let mmap_started = index_phase_started();
    let bytes = unsafe { Mmap::map(&file)? };
    index_phase_emit("index.read.mmap", mmap_started);
    if let Some(link) = parse_split_index_link(path, &bytes, algorithm)? {
        if let Some(shared_path) = split_index_shared_path(path, &link.shared_hash) {
            let base = read_index_with_algorithm(shared_path, algorithm)?;
            let local = decode_index(path, &bytes, algorithm)?;
            let result = merge_split_index(base, local, &link);
            index_phase_emit("index.read.total", total_started);
            return result;
        }
    }
    let decode_started = index_phase_started();
    let result = decode_index(path, &bytes, algorithm);
    index_phase_emit("index.read.decode", decode_started);
    index_phase_emit("index.read.total", total_started);
    result
}

struct SplitIndexLink {
    shared_hash: Vec<u8>,
    delete_positions: Vec<usize>,
    replace_positions: Vec<usize>,
}

fn parse_split_index_link(
    _index_path: &Path,
    bytes: &[u8],
    algorithm: GitHashAlgorithm,
) -> io::Result<Option<SplitIndexLink>> {
    let (_, _, checksum_offset) = decode_index_header(bytes, algorithm)?;
    let entries_end = find_index_entries_end(bytes, checksum_offset, algorithm)?;
    let mut cursor = entries_end;
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
        let len = read_u32(bytes, cursor + 4)? as usize;
        let end = header_end.checked_add(len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension length overflow",
            )
        })?;
        if end > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension body is truncated",
            ));
        }
        if signature == SPLIT_INDEX_LINK_EXTENSION {
            let body = &bytes[header_end..end];
            let digest_len = algorithm.digest_len();
            if body.len() < digest_len {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "split-index link is truncated",
                ));
            }
            let rest = &body[digest_len..];
            let (delete_positions, used) = decode_ewah_positions(rest)?;
            let (replace_positions, used_replace) = decode_ewah_positions(&rest[used..])?;
            if used + used_replace != rest.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "split-index link has trailing data",
                ));
            }
            return Ok(Some(SplitIndexLink {
                shared_hash: body[..digest_len].to_vec(),
                delete_positions,
                replace_positions,
            }));
        }
        cursor = end;
    }
    Ok(None)
}

fn decode_ewah_positions(bytes: &[u8]) -> io::Result<(Vec<usize>, usize)> {
    if bytes.len() < 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "split-index bitmap is truncated",
        ));
    }
    let bit_size = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let buffer_size = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let compressed_len = buffer_size.checked_mul(8).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "split-index bitmap is too large",
        )
    })?;
    let total = 12usize.checked_add(compressed_len).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "split-index bitmap length overflow",
        )
    })?;
    if total > bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "split-index bitmap is truncated",
        ));
    }
    let mut positions = Vec::new();
    let mut output_word = 0usize;
    let mut cursor = 8usize;
    let mut word_index = 0usize;
    while word_index < buffer_size {
        let rlw = u64::from_be_bytes(bytes[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;
        word_index += 1;
        let run_value = rlw & 1 != 0;
        let run_length = ((rlw >> 1) & 0x7fff_ffff) as usize;
        let literal_count = (rlw >> 33) as usize;
        if run_value {
            for bit in 0..run_length.saturating_mul(64) {
                if output_word.saturating_mul(64) + bit < bit_size {
                    positions.push(output_word * 64 + bit);
                }
            }
        }
        output_word = output_word.saturating_add(run_length);
        for _ in 0..literal_count {
            if word_index >= buffer_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "split-index bitmap literals are truncated",
                ));
            }
            let literal = u64::from_be_bytes(bytes[cursor..cursor + 8].try_into().unwrap());
            cursor += 8;
            word_index += 1;
            for bit in 0..64 {
                let position = output_word * 64 + bit;
                if position < bit_size && literal & (1_u64 << bit) != 0 {
                    positions.push(position);
                }
            }
            output_word += 1;
        }
    }
    if cursor + 4 > bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "split-index bitmap trailer is truncated",
        ));
    }
    Ok((positions, cursor + 4))
}

fn split_index_shared_path(index_path: &Path, shared_hash: &[u8]) -> Option<PathBuf> {
    let name = format!("sharedindex.{}", hex_string(shared_hash));
    let mut candidates = Vec::new();
    if let Some(parent) = index_path.parent() {
        candidates.push(parent.join(&name));
        for ancestor in parent.ancestors() {
            candidates.push(ancestor.join(&name));
            candidates.push(ancestor.join(".git").join(&name));
        }
    }
    if let Ok(current) = std::env::current_dir() {
        candidates.push(current.join(&name));
        candidates.push(current.join(".git").join(&name));
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn merge_split_index(
    base: GitIndex,
    local: GitIndex,
    link: &SplitIndexLink,
) -> io::Result<GitIndex> {
    let mut local_entries = local.entries.into_iter();
    let replacements = link.replace_positions.len();
    let mut replacement_entries = Vec::with_capacity(replacements);
    for _ in 0..replacements {
        replacement_entries.push(local_entries.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "split-index replacement is missing",
            )
        })?);
    }
    let deleted = |position: usize| link.delete_positions.binary_search(&position).is_ok();
    let replaced = |position: usize| link.replace_positions.binary_search(&position).is_ok();
    let mut replacement_cursor = 0usize;
    let mut entries = Vec::new();
    for (position, base_entry) in base.entries.into_iter().enumerate() {
        if deleted(position) {
            continue;
        }
        if replaced(position) {
            let mut replacement = replacement_entries[replacement_cursor].clone();
            replacement.path = base_entry.path;
            replacement_cursor += 1;
            entries.push(replacement);
        } else {
            entries.push(base_entry);
        }
    }
    entries.extend(local_entries);
    GitIndex::from_entries_and_resolve_undo(entries, local.resolve_undo)
}

fn supported_split_index_shared_path(
    index_path: &Path,
    bytes: &[u8],
    algorithm: GitHashAlgorithm,
) -> io::Result<Option<PathBuf>> {
    let (version, count, checksum_offset) = decode_index_header(bytes, algorithm)?;
    if !matches!(version, INDEX_VERSION_V2 | INDEX_VERSION_V3) {
        return Ok(None);
    }
    let entries_end = find_index_entries_end(bytes, checksum_offset, algorithm)?;
    let supports_shape = match count {
        0 => split_index_empty_overlay_supported(bytes, checksum_offset, entries_end)?,
        1 => split_index_single_entry_supported(bytes, checksum_offset, entries_end, algorithm)?,
        _ => false,
    };
    if !supports_shape {
        return Ok(None);
    }
    let mut cursor = entries_end;
    let mut shared_hash = None;
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
        let len = read_u32(bytes, cursor + 4)? as usize;
        let end = header_end.checked_add(len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension length overflow",
            )
        })?;
        if end > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension body is truncated",
            ));
        }
        if signature == SPLIT_INDEX_LINK_EXTENSION {
            let body = &bytes[header_end..end];
            if !split_index_link_body_supported(count as u32, body, algorithm) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "index uses unsupported split-index shape",
                ));
            }
            shared_hash = Some(body[..algorithm.digest_len()].to_vec());
        }
        cursor = end;
    }
    let Some(shared_hash) = shared_hash else {
        return Ok(None);
    };
    let shared_name = shared_hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(Some(
        index_path
            .parent()
            .expect("index path should have parent")
            .join(format!("sharedindex.{shared_name}")),
    ))
}

fn split_index_link_body_supported(
    entry_count: u32,
    body: &[u8],
    algorithm: GitHashAlgorithm,
) -> bool {
    let checksum_len = algorithm.digest_len();
    if body.len() < checksum_len {
        return false;
    }
    match entry_count {
        0 => {
            body.len() == checksum_len + SPLIT_INDEX_EMPTY_BITMAP.len() * 2
                && &body[checksum_len..checksum_len + SPLIT_INDEX_EMPTY_BITMAP.len()]
                    == SPLIT_INDEX_EMPTY_BITMAP
                && &body[checksum_len + SPLIT_INDEX_EMPTY_BITMAP.len()..]
                    == SPLIT_INDEX_EMPTY_BITMAP
        }
        1 => {
            body.len() == checksum_len + SPLIT_INDEX_SINGLE_ENTRY_TAIL.len()
                && &body[checksum_len..] == SPLIT_INDEX_SINGLE_ENTRY_TAIL
        }
        _ => false,
    }
}

fn split_index_shared_hash_from_bytes(
    bytes: &[u8],
    checksum_offset: usize,
    entries_end: usize,
    entry_count: u32,
    algorithm: GitHashAlgorithm,
) -> io::Result<Option<Vec<u8>>> {
    let mut cursor = entries_end;
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
        let len = read_u32(bytes, cursor + 4)? as usize;
        let end = header_end.checked_add(len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension length overflow",
            )
        })?;
        if end > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index extension body is truncated",
            ));
        }
        if signature == SPLIT_INDEX_LINK_EXTENSION {
            let body = &bytes[header_end..end];
            if !split_index_link_body_supported(entry_count, body, algorithm) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "index uses unsupported split-index shape",
                ));
            }
            return Ok(Some(body[..algorithm.digest_len()].to_vec()));
        }
        cursor = end;
    }
    Ok(None)
}

fn split_index_single_entry_supported(
    bytes: &[u8],
    checksum_offset: usize,
    entries_end: usize,
    algorithm: GitHashAlgorithm,
) -> io::Result<bool> {
    let entry_start = 12usize;
    let fixed_end = entry_start + entry_fixed_len(algorithm);
    if fixed_end > checksum_offset {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index entry is truncated",
        ));
    }
    let flags = read_u16(
        bytes,
        entry_start + ENTRY_METADATA_LEN + algorithm.digest_len(),
    )?;
    let path_start = fixed_end + usize::from(flags & ENTRY_FLAG_EXTENDED != 0) * 2;
    if path_start >= checksum_offset {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index path is truncated",
        ));
    }
    Ok(bytes[path_start] == 0
        && flags & 0x0fff == 0
        && entries_end == aligned_entry_end(path_start + 1, entry_start))
}

fn split_index_empty_overlay_supported(
    _bytes: &[u8],
    checksum_offset: usize,
    entries_end: usize,
) -> io::Result<bool> {
    Ok(entries_end == 12 && checksum_offset >= 12)
}

fn aligned_entry_end(next: usize, start: usize) -> usize {
    let mut aligned = next;
    while !(aligned - start).is_multiple_of(8) {
        aligned += 1;
    }
    aligned
}

fn find_index_entries_end(
    bytes: &[u8],
    checksum_offset: usize,
    algorithm: GitHashAlgorithm,
) -> io::Result<usize> {
    if checksum_offset < 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index is truncated",
        ));
    }
    let count = usize::try_from(read_u32(bytes, 8)?).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "git index entry count overflow")
    })?;
    let mut cursor = 12usize;
    for _ in 0..count {
        let fixed_end = cursor
            .checked_add(entry_fixed_len(algorithm))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "git index entry overflow")
            })?;
        if fixed_end > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index entry is truncated",
            ));
        }
        let flags = read_u16(bytes, cursor + ENTRY_METADATA_LEN + algorithm.digest_len())?;
        let path_start = fixed_end + usize::from(flags & ENTRY_FLAG_EXTENDED != 0) * 2;
        if path_start > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index path is truncated",
            ));
        }
        let nul = bytes[path_start..checksum_offset]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "git index path is unterminated")
            })?;
        cursor = aligned_entry_end(path_start + nul + 1, cursor);
        if cursor > checksum_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index entry padding is truncated",
            ));
        }
    }
    Ok(cursor)
}

fn decode_index(_path: &Path, bytes: &[u8], algorithm: GitHashAlgorithm) -> io::Result<GitIndex> {
    let header_started = index_phase_started();
    let (version, count, checksum_offset) = decode_index_header(bytes, algorithm)?;
    index_phase_emit("index.decode.header", header_started);
    let mut cursor = 12;
    let mut entries = Vec::with_capacity(index_entry_initial_capacity(count));
    let mut previous_path = Vec::new();
    let mut previous_stage = 0_u8;
    let entries_started = index_phase_started();
    for _ in 0..count {
        let (entry, next) = decode_entry(
            bytes,
            cursor,
            checksum_offset,
            version,
            &previous_path,
            algorithm,
        )?;
        if !entries.is_empty()
            && !previous_path
                .as_slice()
                .cmp(entry.path.as_slice())
                .then(previous_stage.cmp(&entry.stage))
                .is_lt()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "git index entries are not sorted",
            ));
        }
        previous_path.clone_from(&entry.path);
        previous_stage = entry.stage;
        entries.push(entry);
        cursor = next;
    }
    index_phase_emit("index.decode.entries", entries_started);
    let extensions_started = index_phase_started();
    let extensions = decode_index_extensions(bytes, cursor, checksum_offset, algorithm)?;
    index_phase_emit("index.decode.extensions", extensions_started);
    let resolve_undo_validate_started = index_phase_started();
    validate_sorted_resolve_undo_entries(&extensions.resolve_undo)?;
    index_phase_emit(
        "index.decode.resolve_undo_validate",
        resolve_undo_validate_started,
    );
    Ok(GitIndex {
        hash_algorithm: algorithm,
        entries,
        resolve_undo: extensions.resolve_undo,
        cache_tree: extensions.cache_tree,
    })
}

fn index_entry_initial_capacity(count: usize) -> usize {
    count.min(INDEX_ENTRY_INITIAL_CAPACITY_LIMIT)
}

fn decode_index_header(
    bytes: &[u8],
    algorithm: GitHashAlgorithm,
) -> io::Result<(u32, usize, usize)> {
    let checksum_len = algorithm.digest_len();
    if bytes.len() < 12 + checksum_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index is too short",
        ));
    }
    let checksum_offset = bytes.len() - checksum_len;
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&bytes[..checksum_offset]);
    let expected = hasher.finalize();
    let checksum = &bytes[checksum_offset..];
    if expected.as_bytes() != checksum && checksum.iter().any(|byte| *byte != 0) {
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

struct IndexExtensions {
    resolve_undo: Vec<ResolveUndoEntry>,
    cache_tree: Option<IndexCacheTree>,
}

fn decode_index_extensions(
    bytes: &[u8],
    mut cursor: usize,
    checksum_offset: usize,
    algorithm: GitHashAlgorithm,
) -> io::Result<IndexExtensions> {
    let mut resolve_undo = Vec::new();
    let mut cache_tree = None;
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
        if signature.iter().any(u8::is_ascii_lowercase)
            && signature != SPARSE_DIRECTORY_EXTENSION
            && signature != SPLIT_INDEX_LINK_EXTENSION
        {
            let extension = index_extension_name(signature);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("index uses {extension} extension, which we do not understand"),
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
            resolve_undo = decode_resolve_undo_extension(&bytes[header_end..cursor], algorithm)?;
        } else if signature == CACHE_TREE_EXTENSION {
            cache_tree = Some(decode_cache_tree_extension(
                &bytes[header_end..cursor],
                algorithm,
            )?);
        }
    }
    Ok(IndexExtensions {
        resolve_undo,
        cache_tree,
    })
}

fn index_extension_name(signature: &[u8]) -> String {
    String::from_utf8_lossy(signature).into_owned()
}

fn decode_resolve_undo_extension(
    body: &[u8],
    algorithm: GitHashAlgorithm,
) -> io::Result<Vec<ResolveUndoEntry>> {
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
                let end = cursor.checked_add(algorithm.digest_len()).ok_or_else(|| {
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
                    id: ObjectId::new(algorithm, &body[cursor..end]),
                });
                cursor = end;
            }
        }
        entries.push(ResolveUndoEntry { path, stages });
    }
    Ok(entries)
}

fn decode_cache_tree_extension(
    body: &[u8],
    algorithm: GitHashAlgorithm,
) -> io::Result<IndexCacheTree> {
    let mut cursor = 0;
    let (root_name, root) = decode_named_cache_tree_node(body, &mut cursor, algorithm)?;
    if !root_name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git cache-tree root name must be empty",
        ));
    }
    if cursor != body.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git cache-tree extension has trailing bytes",
        ));
    }
    Ok(root)
}

fn decode_named_cache_tree_node(
    body: &[u8],
    cursor: &mut usize,
    algorithm: GitHashAlgorithm,
) -> io::Result<(Vec<u8>, IndexCacheTree)> {
    let path = read_nul_terminated(body, cursor, "cache-tree path")?.to_vec();
    let entry_count = read_decimal_i32_until(body, cursor, b' ', "cache-tree entry count")?;
    let subtree_nr = read_decimal_usize_until(body, cursor, b'\n', "cache-tree subtree count")?;
    let oid = if entry_count >= 0 {
        let end = cursor.checked_add(algorithm.digest_len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "cache-tree object id offset overflow",
            )
        })?;
        if end > body.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cache-tree object id is truncated",
            ));
        }
        let oid = ObjectId::new(algorithm, &body[*cursor..end]);
        *cursor = end;
        oid
    } else {
        empty_tree_oid(algorithm)
    };
    let mut subtrees = Vec::with_capacity(subtree_nr);
    for _ in 0..subtree_nr {
        let (name, child) = decode_named_cache_tree_node(body, cursor, algorithm)?;
        subtrees.push(IndexCacheTreeChild {
            name,
            cache_tree: child,
        });
    }
    Ok((
        path,
        IndexCacheTree {
            entry_count,
            oid,
            subtrees,
        },
    ))
}

fn build_cache_tree(algorithm: GitHashAlgorithm, entries: &[IndexEntry]) -> Option<IndexCacheTree> {
    if entries
        .iter()
        .any(|entry| entry.stage != 0 || entry.intent_to_add())
    {
        return Some(IndexCacheTree {
            entry_count: -1,
            oid: empty_tree_oid(algorithm),
            subtrees: Vec::new(),
        });
    }
    if entries.iter().any(|entry| {
        entry.path.iter().filter(|byte| **byte == b'/').count() > CACHE_TREE_MAX_COMPONENT_DEPTH
    }) {
        return None;
    }
    Some(build_cache_tree_node(algorithm, entries, &[]))
}

fn build_cache_tree_node(
    algorithm: GitHashAlgorithm,
    entries: &[IndexEntry],
    base: &[u8],
) -> IndexCacheTree {
    let mut leaves = Vec::new();
    let mut children: BTreeMap<Vec<u8>, ()> = BTreeMap::new();

    for entry in entries.iter().filter(|entry| entry.path.starts_with(base)) {
        let suffix = &entry.path[base.len()..];
        if suffix.is_empty() {
            continue;
        }
        if let Some(slash_idx) = suffix.iter().position(|byte| *byte == b'/') {
            children.entry(suffix[..slash_idx].to_vec()).or_insert(());
            continue;
        }
        leaves.push((
            suffix.to_vec(),
            entry.mode.tree_mode(),
            entry.id.clone(),
            entry.mode == IndexMode::Tree,
        ));
    }

    let mut subtree_entries = Vec::with_capacity(children.len());
    let mut tree_entries = Vec::new();

    for (name, _) in children {
        let mut child_base = Vec::with_capacity(base.len() + name.len() + 1);
        child_base.extend_from_slice(base);
        child_base.extend_from_slice(&name);
        child_base.push(b'/');
        let child = build_cache_tree_node(algorithm, entries, &child_base);
        tree_entries.push(
            TreeEntry::new(TreeMode::Tree, name.clone(), child.oid.clone())
                .expect("cache-tree child name should be valid"),
        );
        subtree_entries.push(IndexCacheTreeChild {
            name,
            cache_tree: child,
        });
    }

    for (name, mode, id, is_tree) in leaves {
        tree_entries
            .push(TreeEntry::new(mode, name, id).expect("cache-tree leaf name should be valid"));
        if is_tree {
            // Sparse-directory style entries are represented directly in the tree object
            // and do not become nested cache-tree children.
        }
    }

    tree_entries.sort_by(cache_tree_tree_entry_cmp);
    subtree_entries.sort_by(|left, right| cache_tree_subtree_name_cmp(&left.name, &right.name));
    let encoded = encode_tree(&tree_entries).expect("cache-tree entries should encode");
    let oid = crate::hash_object(algorithm, GitObjectKind::Tree, &encoded);
    let entry_count = entries
        .iter()
        .filter(|entry| entry.stage == 0 && entry.path.starts_with(base))
        .count() as i32;
    IndexCacheTree {
        entry_count,
        oid,
        subtrees: subtree_entries,
    }
}

fn invalidate_cache_tree_path(root: Option<&mut IndexCacheTree>, path: &[u8]) {
    let Some(root) = root else {
        return;
    };
    do_invalidate_cache_tree_path(root, path);
}

fn do_invalidate_cache_tree_path(node: &mut IndexCacheTree, path: &[u8]) {
    node.entry_count = -1;
    let slash_idx = path.iter().position(|byte| *byte == b'/');
    match slash_idx {
        None => {
            if let Ok(idx) = node
                .subtrees
                .binary_search_by(|probe| cache_tree_subtree_name_cmp(&probe.name, path))
            {
                node.subtrees.remove(idx);
            }
        }
        Some(idx) => {
            let name = &path[..idx];
            if let Ok(child_idx) = node
                .subtrees
                .binary_search_by(|probe| cache_tree_subtree_name_cmp(&probe.name, name))
            {
                do_invalidate_cache_tree_path(
                    &mut node.subtrees[child_idx].cache_tree,
                    &path[idx + 1..],
                );
            }
        }
    }
}

fn cache_tree_tree_entry_cmp(left: &TreeEntry, right: &TreeEntry) -> Ordering {
    compare_tree_entry_names(
        &left.name,
        left.mode == TreeMode::Tree,
        &right.name,
        right.mode == TreeMode::Tree,
    )
}

fn compare_tree_entry_names(
    left: &[u8],
    left_is_tree: bool,
    right: &[u8],
    right_is_tree: bool,
) -> Ordering {
    let mut idx = 0;
    loop {
        let left_byte = tree_name_byte(left, left_is_tree, idx);
        let right_byte = tree_name_byte(right, right_is_tree, idx);
        match (left_byte, right_byte) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left), Some(right)) if left != right => return left.cmp(&right),
            _ => idx += 1,
        }
    }
}

fn tree_name_byte(name: &[u8], is_tree: bool, idx: usize) -> Option<u8> {
    if idx < name.len() {
        Some(name[idx])
    } else if idx == name.len() && is_tree {
        Some(b'/')
    } else {
        None
    }
}

fn cache_tree_subtree_name_cmp(left: &[u8], right: &[u8]) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn empty_tree_oid(algorithm: GitHashAlgorithm) -> ObjectId {
    let encoded = encode_tree(&[]).expect("encoding an empty tree should succeed");
    crate::hash_object(algorithm, GitObjectKind::Tree, &encoded)
}

fn read_decimal_i32_until(
    bytes: &[u8],
    cursor: &mut usize,
    delimiter: u8,
    label: &str,
) -> io::Result<i32> {
    let raw = read_until_delimiter(bytes, cursor, delimiter, label)?;
    let text = std::str::from_utf8(raw).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} is not valid UTF-8"),
        )
    })?;
    text.parse::<i32>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} is not a valid integer"),
        )
    })
}

fn read_decimal_usize_until(
    bytes: &[u8],
    cursor: &mut usize,
    delimiter: u8,
    label: &str,
) -> io::Result<usize> {
    let raw = read_until_delimiter(bytes, cursor, delimiter, label)?;
    let text = std::str::from_utf8(raw).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} is not valid UTF-8"),
        )
    })?;
    text.parse::<usize>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{label} is not a valid integer"),
        )
    })
}

fn read_until_delimiter<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    delimiter: u8,
    label: &str,
) -> io::Result<&'a [u8]> {
    let start = *cursor;
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == delimiter)
        .map(|offset| start + offset)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} delimiter is missing"),
            )
        })?;
    *cursor = end + 1;
    Ok(&bytes[start..end])
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
    algorithm: GitHashAlgorithm,
) -> io::Result<(IndexEntry, usize)> {
    let fixed_end = start
        .checked_add(entry_fixed_len(algorithm))
        .ok_or_else(|| {
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

    let object_id_start = start + ENTRY_METADATA_LEN;
    let object_id_end = object_id_start + algorithm.digest_len();
    let flags = read_u16(bytes, object_id_end)?;
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
    if !path.is_empty() {
        validate_index_path(&path)?;
    }

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
            id: ObjectId::new(algorithm, &bytes[object_id_start..object_id_end]),
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
    let algorithm = index_entries_algorithm(entries);
    for entry in entries {
        validate_index_path(&entry.path)?;
        if entry.stage > 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git index stage must be 0..=3",
            ));
        }
        if algorithm.is_some_and(|algorithm| entry.id.algorithm() != algorithm) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "git index entries use mixed object formats",
            ));
        }
    }
    Ok(())
}

fn index_entries_algorithm(entries: &[IndexEntry]) -> Option<GitHashAlgorithm> {
    entries.first().map(|entry| entry.id.algorithm())
}

fn validate_resolve_undo_entries(entries: &[ResolveUndoEntry]) -> io::Result<()> {
    let mut algorithm = None;
    for entry in entries {
        validate_index_path(&entry.path)?;
        for stage in entry.stages.iter().flatten() {
            if algorithm.is_some_and(|algorithm| stage.id.algorithm() != algorithm) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "git resolve-undo entries use mixed object formats",
                ));
            }
            algorithm = Some(stage.id.algorithm());
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

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

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
    use sha1::{Digest as _, Sha1};
    use tempfile::TempDir;

    use super::*;
    use crate::stock_git_support;
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
    fn sha256_index_round_trips_with_stock_git() {
        let repo = stock_git_support::git_init_sha256();
        let store =
            LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha256);
        let blob = store
            .write_object(GitObjectKind::Blob, b"sha256 index\n")
            .expect("write sha256 blob");
        let entry =
            IndexEntry::new("sha256.txt", blob.clone(), IndexMode::File, 13).expect("index entry");
        let index = GitIndex::from_entries(vec![entry]).expect("sha256 index");
        let index_path = repo.path().join(".git/index");

        write_index(&index_path, &index).expect("write sha256 index");

        assert_eq!(
            git(&repo, ["ls-files", "--stage"]),
            format!("100644 {} 0\tsha256.txt", blob.to_hex())
        );
        assert_eq!(
            read_index_with_algorithm(&index_path, GitHashAlgorithm::Sha256)
                .expect("read sha256 index"),
            index
        );
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
        let checksum_offset = encoded.len() - GitHashAlgorithm::Sha1.digest_len();
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
        let entries = vec![
            IndexEntry::new("a.txt", first.clone(), IndexMode::File, 3).expect("entry"),
            extended,
        ];
        let resolve_undo = vec![ResolveUndoEntry {
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
        }];
        let index = GitIndex::from_trusted_sorted_entries_and_resolve_undo(entries, resolve_undo)
            .expect("index");
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
        let merge = stock_git_support::git_output(&repo, &["merge", "left"]);
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

    #[test]
    fn cache_tree_roundtrips_through_index_encoding() {
        let blob_a = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let blob_b = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        let blob_c = ObjectId::new(GitHashAlgorithm::Sha1, &[3; 20]);
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("root.txt", blob_a, IndexMode::File, 0).expect("entry"),
            IndexEntry::new("dir1/a.txt", blob_b, IndexMode::File, 0).expect("entry"),
            IndexEntry::new("dir2/b.txt", blob_c, IndexMode::File, 0).expect("entry"),
        ])
        .expect("index");

        let encoded = encode_index(&index).expect("encode index");
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("index");
        std::fs::write(&path, encoded).expect("write index");
        let decoded = read_index(&path).expect("read index");

        assert_eq!(decoded.cache_tree, index.cache_tree);
        let cache_tree = decoded.cache_tree.as_ref().expect("cache tree");
        assert_eq!(cache_tree.entry_count, 3);
        assert_eq!(
            cache_tree
                .subtrees
                .iter()
                .map(|child| (child.name.as_slice(), child.cache_tree.entry_count))
                .collect::<Vec<_>>(),
            vec![(b"dir1".as_slice(), 1), (b"dir2".as_slice(), 1)]
        );
    }

    #[test]
    fn cache_tree_invalidation_preserves_unaffected_sibling_subtrees() {
        let blob_a = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let blob_b = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        let blob_c = ObjectId::new(GitHashAlgorithm::Sha1, &[3; 20]);
        let mut index = GitIndex::from_entries(vec![
            IndexEntry::new("root.txt", blob_a, IndexMode::File, 0).expect("entry"),
            IndexEntry::new("dir1/a.txt", blob_b, IndexMode::File, 0).expect("entry"),
            IndexEntry::new("dir2/b.txt", blob_c, IndexMode::File, 0).expect("entry"),
        ])
        .expect("index");

        let replacement = ObjectId::new(GitHashAlgorithm::Sha1, &[4; 20]);
        index
            .upsert(IndexEntry::new("dir1/a.txt", replacement, IndexMode::File, 0).expect("entry"))
            .expect("upsert");

        let cache_tree = index.cache_tree.as_ref().expect("cache tree");
        assert_eq!(cache_tree.entry_count, -1);
        assert_eq!(cache_tree.subtrees.len(), 2);
        assert_eq!(cache_tree.subtrees[0].name, b"dir1");
        assert_eq!(cache_tree.subtrees[0].cache_tree.entry_count, -1);
        assert_eq!(cache_tree.subtrees[1].name, b"dir2");
        assert_eq!(cache_tree.subtrees[1].cache_tree.entry_count, 1);
    }

    fn git_init() -> TempDir {
        stock_git_support::git_init()
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        stock_git_support::git(repo, &args)
    }
}
