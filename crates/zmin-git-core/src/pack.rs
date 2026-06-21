use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::loose::{LooseObject, record_prefix_candidate, validate_hex_prefix};
use crate::object::{GitHashAlgorithm, GitObjectHash, GitObjectKind, ObjectId, hash_object};
use crate::object_store::{GitObjectSink, GitObjectStore, ObjectStorageHint};

const IDX_MAGIC: &[u8; 4] = b"\xfftOc";
const RIDX_MAGIC: &[u8; 4] = b"RIDX";
const PACK_MAGIC: &[u8; 4] = b"PACK";
const PACK_HEADER_LEN: usize = 12;
const IDX_VERSION: u32 = 2;
const RIDX_VERSION: u32 = 1;
const PACK_VERSION_2: u32 = 2;
const PACK_VERSION_3: u32 = 3;
const MAX_DELTA_DEPTH: usize = 4095;
const PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT: usize = 8192;
const PACK_RETENTION_RESERVE_CAPACITY_LIMIT: usize = 8192;
const PACK_DELTA_LOOKUP_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_EXTERNAL_BASES_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_REF_DELTA_LINEAR_LOOKUP_LIMIT: usize = 8;
const THIN_PACK_REPAIR_EXTRA_CAPACITY_LIMIT: usize = 8192;
const PACK_INDEX_CACHE_ENTRY_LIMIT: usize = 256;
const PACK_INDEX_CACHE_BYTE_LIMIT: u64 = 64 * 1024 * 1024;
const PACK_INDEX_MERGE_HEAP_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_INDEX_ENTRY_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_INDEX_OBJECT_ID_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_INDEX_LIST_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_INDEX_PATHS_INITIAL_CAPACITY_HINT: usize = 4;
const PACK_OBJECT_READ_CACHE_ENTRY_LIMIT: usize = 4096;
const PACK_OBJECT_READ_CACHE_BYTE_LIMIT: usize = 8 * 1024 * 1024;
const PACK_BLOB_OUTPUT_BUFFER_CAPACITY: usize = 64 * 1024;
const PACK_REUSABLE_ENTRY_COPY_BUFFER_CAPACITY: usize = 64 * 1024;
const PACK_ZLIB_STREAM_BUFFER_CAPACITY: usize = 64 * 1024;
const ZLIB_CONTENT_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;
const DELTA_RESULT_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;
const REPLACEMENT_DELTA_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;
const PACK_DELTA_WINDOW_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_OUTPUT_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;
const PACK_OUTPUT_OBJECT_BYTES_HINT: usize = 16;
#[cfg(not(test))]
const PACK_DELTA_WINDOW_CONTENT_BUDGET: usize = 64 * 1024 * 1024;
#[cfg(test)]
const PACK_DELTA_WINDOW_CONTENT_BUDGET: usize = 1024;

static PACK_INDEX_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedPackIndex>>> = OnceLock::new();
static PACK_BLOB_OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct PackedObjectStore {
    objects_dir: PathBuf,
    algorithm: GitHashAlgorithm,
    max_object_bytes: usize,
    idx_paths_cache: Arc<Mutex<Option<CachedPackIndexPaths>>>,
    last_index_lookup: Arc<Mutex<Option<CachedLastPackIndexLookup>>>,
    last_pack_file: Arc<Mutex<Option<CachedLastPackFile>>>,
    object_read_cache: Arc<Mutex<PackObjectReadCache>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackIndexEntry {
    pub offset: u64,
    pub object_id: ObjectId,
    pub crc32: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedPack {
    pub pack_id: ObjectId,
    pub index: Vec<u8>,
    pub reverse_index: Vec<u8>,
    pub objects: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedPackIndexOnly {
    pub pack_id: ObjectId,
    pub index: Vec<u8>,
    pub objects: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackIndexVersion {
    V1,
    V2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinPackRepair {
    pub pack: Vec<u8>,
    pub indexed: IndexedPack,
    pub fixed_objects: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThinPackFileRepair {
    pub indexed: IndexedPack,
    pub fixed_objects: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackVerifyEntry {
    pub object_id: ObjectId,
    pub kind: GitObjectKind,
    pub object_size: u64,
    pub packed_size: usize,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPack {
    pub objects: usize,
    pub entries: Vec<PackVerifyEntry>,
    pub index: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPackIndexMatch {
    pub objects: usize,
    pub entries: Vec<PackVerifyEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackEncodeOptions {
    pub window: usize,
    pub depth: usize,
}

impl PackEncodeOptions {
    pub const UNDELTIFIED: Self = Self {
        window: 0,
        depth: 0,
    };

    pub fn delta(window: usize, depth: usize) -> Self {
        Self { window, depth }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackObjectData {
    pub id: ObjectId,
    pub kind: GitObjectKind,
    pub content: Vec<u8>,
}

#[derive(Clone)]
struct CachedPackIndex {
    modified: Option<SystemTime>,
    len: u64,
    index: Arc<PackIndex>,
}

#[derive(Clone)]
struct CachedLastPackIndexLookup {
    idx_path: PathBuf,
    pack_path: Arc<PathBuf>,
    modified: Option<SystemTime>,
    len: u64,
    index: Arc<PackIndex>,
}

#[derive(Debug)]
struct CachedLastPackFile {
    pack_path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
    file: fs::File,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PackObjectReadCacheKey {
    pack_path: PathBuf,
    offset: u64,
}

#[derive(Debug, Clone)]
struct CachedPackObjectRead {
    kind: GitObjectKind,
    content: Vec<u8>,
}

#[derive(Debug, Default)]
struct PackObjectReadCache {
    entries: HashMap<PackObjectReadCacheKey, CachedPackObjectRead>,
    order: VecDeque<PackObjectReadCacheKey>,
    bytes: usize,
}

impl PackObjectReadCache {
    fn insert(&mut self, key: PackObjectReadCacheKey, kind: GitObjectKind, content: &[u8]) {
        if content.len() > PACK_OBJECT_READ_CACHE_BYTE_LIMIT {
            return;
        }
        if let Some(existing) = self.entries.get_mut(&key) {
            self.bytes = self.bytes.saturating_sub(existing.content.len());
            self.bytes = self.bytes.saturating_add(content.len());
            existing.kind = kind;
            existing.content = content.to_vec();
            self.evict_over_budget();
            return;
        }
        self.bytes = self.bytes.saturating_add(content.len());
        self.entries.insert(
            key.clone(),
            CachedPackObjectRead {
                kind,
                content: content.to_vec(),
            },
        );
        self.order.push_back(key);
        self.evict_over_budget();
    }

    fn evict_over_budget(&mut self) {
        while self.bytes > PACK_OBJECT_READ_CACHE_BYTE_LIMIT
            || self.entries.len() > PACK_OBJECT_READ_CACHE_ENTRY_LIMIT
        {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            let Some(entry) = self.entries.remove(&key) else {
                continue;
            };
            self.bytes = self.bytes.saturating_sub(entry.content.len());
        }
    }
}

struct PackIndexLookup {
    pack_path: Arc<PathBuf>,
    index: Arc<PackIndex>,
    offset: u64,
}

impl std::fmt::Debug for CachedLastPackIndexLookup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CachedLastPackIndexLookup")
            .field("idx_path", &self.idx_path)
            .field("pack_path", &self.pack_path)
            .field("modified", &self.modified)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct CachedPackIndexPaths {
    modified: Option<SystemTime>,
    len: u64,
    paths: Arc<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnpackPackStats {
    pub objects: usize,
}

pub fn decode_pack_index(
    algorithm: GitHashAlgorithm,
    bytes: Vec<u8>,
) -> io::Result<Vec<PackIndexEntry>> {
    PackIndex::read_bytes(bytes, algorithm).and_then(|index| index.entries())
}

pub fn for_each_pack_index_entry(
    algorithm: GitHashAlgorithm,
    bytes: Vec<u8>,
    for_each: &mut dyn FnMut(PackIndexEntry) -> io::Result<()>,
) -> io::Result<()> {
    PackIndex::read_bytes(bytes, algorithm)?.for_each_entry(for_each)
}

pub fn decode_pack_index_from_path(
    algorithm: GitHashAlgorithm,
    path: &Path,
) -> io::Result<Vec<PackIndexEntry>> {
    PackIndex::read(&pack_index_path(path), algorithm).and_then(|index| index.entries())
}

pub fn for_each_pack_index_entry_from_path(
    algorithm: GitHashAlgorithm,
    path: &Path,
    for_each: &mut dyn FnMut(PackIndexEntry) -> io::Result<()>,
) -> io::Result<()> {
    PackIndex::read(&pack_index_path(path), algorithm)?.for_each_entry(for_each)
}

pub fn decode_pack_index_object_ids(
    algorithm: GitHashAlgorithm,
    bytes: Vec<u8>,
) -> io::Result<Vec<ObjectId>> {
    PackIndex::read_bytes(bytes, algorithm).map(|index| index.object_ids())
}

pub fn decode_pack_index_object_ids_from_path(
    algorithm: GitHashAlgorithm,
    path: &Path,
) -> io::Result<Vec<ObjectId>> {
    PackIndex::read(&pack_index_path(path), algorithm).map(|index| index.object_ids())
}

pub fn for_each_pack_index_object_id_from_path(
    algorithm: GitHashAlgorithm,
    path: &Path,
    for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
) -> io::Result<()> {
    PackIndex::read(&pack_index_path(path), algorithm)?.for_each_object_id(for_each)
}

pub fn pack_index_object_ids_all_from_path(
    algorithm: GitHashAlgorithm,
    path: &Path,
    predicate: &mut dyn FnMut(&ObjectId) -> io::Result<bool>,
) -> io::Result<bool> {
    PackIndex::read(&pack_index_path(path), algorithm)?.object_ids_all(predicate)
}

pub fn pack_index_object_ids_are_subset_from_paths(
    algorithm: GitHashAlgorithm,
    needle_path: &Path,
    haystack_path: &Path,
) -> io::Result<bool> {
    let needle = PackIndex::read(&pack_index_path(needle_path), algorithm)?;
    let haystack = PackIndex::read(&pack_index_path(haystack_path), algorithm)?;
    Ok(needle.object_ids_are_subset_of(&haystack))
}

pub fn pack_index_object_count(path: &Path) -> io::Result<usize> {
    let mut file = fs::File::open(pack_index_path(path))?;
    let mut header = [0_u8; 8 + 256 * 4];
    file.read_exact(&mut header)?;
    pack_index_object_count_from_header(&header)
}

fn pack_index_path(path: &Path) -> PathBuf {
    if path.extension().and_then(|value| value.to_str()) == Some("pack") {
        path.with_extension("idx")
    } else {
        path.to_path_buf()
    }
}

fn pack_data_path(path: &Path) -> PathBuf {
    match path.extension().and_then(|value| value.to_str()) {
        Some("pack") => path.to_path_buf(),
        Some("idx") => path.with_extension("pack"),
        _ if path.is_file() => path.to_path_buf(),
        _ => path.with_extension("pack"),
    }
}

fn map_pack_file(path: &Path) -> io::Result<memmap2::Mmap> {
    let file = fs::File::open(pack_data_path(path))?;
    // Mapping keeps large pack files out of the Rust heap while preserving the
    // existing slice parser and checksum validation paths.
    unsafe { memmap2::Mmap::map(&file) }
}

#[cfg(unix)]
fn release_mapped_pack_pages(pack: &memmap2::Mmap) {
    // The temporary index-pack mmap is not used after parsing. Drop its
    // resident pages before allocating the encoded .idx buffer.
    let _ = unsafe { pack.unchecked_advise(memmap2::UncheckedAdvice::DontNeed) };
}

#[cfg(not(unix))]
fn release_mapped_pack_pages(_pack: &memmap2::Mmap) {}

fn pack_index_object_count_from_header(header: &[u8]) -> io::Result<usize> {
    if header.len() < 8 + 256 * 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index is too short",
        ));
    }
    let version = pack_index_version(header)?;
    if version == PackIndexVersion::V2 {
        let raw_version = read_u32(header, 4)?;
        if raw_version != IDX_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported pack index version {raw_version}"),
            ));
        }
    }
    let fanout_start = pack_index_fanout_start(version);
    let mut previous = 0_u32;
    for idx in 0..256 {
        let value = read_u32(header, fanout_start + idx * 4)?;
        if value < previous {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pack index fanout is not sorted",
            ));
        }
        previous = value;
    }
    usize::try_from(previous).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index object count overflows usize",
        )
    })
}

fn pack_index_version(bytes: &[u8]) -> io::Result<PackIndexVersion> {
    if bytes.len() < 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index is too short",
        ));
    }
    if &bytes[..4] == IDX_MAGIC {
        Ok(PackIndexVersion::V2)
    } else {
        Ok(PackIndexVersion::V1)
    }
}

const fn pack_index_fanout_start(version: PackIndexVersion) -> usize {
    match version {
        PackIndexVersion::V1 => 0,
        PackIndexVersion::V2 => 8,
    }
}

pub fn validate_pack_index_bytes(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<()> {
    validate_pack_index_layout(algorithm, bytes)?;
    Ok(())
}

pub fn validate_pack_index_file(algorithm: GitHashAlgorithm, path: &Path) -> io::Result<()> {
    let file = fs::File::open(pack_index_path(path))?;
    // Map large index files for validation so callers do not need a heap copy
    // just to check structure and checksum.
    let bytes = unsafe { memmap2::Mmap::map(&file)? };
    validate_pack_index_bytes(algorithm, &bytes)
}

pub fn validate_pack_reverse_index(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    objects: usize,
) -> io::Result<()> {
    let digest_len = algorithm.digest_len();
    let expected_len = 12 + objects * 4 + digest_len * 2;
    if bytes.len() != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack reverse index has the wrong length",
        ));
    }
    if &bytes[..4] != RIDX_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported pack reverse index signature",
        ));
    }
    let version = read_u32(bytes, 4)?;
    if version != RIDX_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported pack reverse index version",
        ));
    }
    let hash_version = read_u32(bytes, 8)?;
    if hash_version != reverse_index_hash_version(algorithm) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack reverse index hash version mismatch",
        ));
    }
    validate_pack_index_checksum(algorithm, bytes)
}

pub fn validate_pack_reverse_index_file(
    algorithm: GitHashAlgorithm,
    path: &Path,
    objects: usize,
) -> io::Result<()> {
    let file = fs::File::open(path)?;
    let bytes = unsafe { memmap2::Mmap::map(&file)? };
    validate_pack_reverse_index(algorithm, &bytes, objects)
}

pub fn index_pack_bytes(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<IndexedPack> {
    index_pack_bytes_with_version(algorithm, bytes, PackIndexVersion::V2)
}

pub fn index_pack_bytes_with_version(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    version: PackIndexVersion,
) -> io::Result<IndexedPack> {
    match parse_pack_entries_index_only_if_base(algorithm, bytes, false)? {
        PackIndexOnlyResult::Fast {
            pack_id, entries, ..
        } => index_pack_from_entries(algorithm, pack_id, entries, version),
        PackIndexOnlyResult::RequiresFullParse {
            retention,
            validated,
        } => {
            let parsed = parse_pack_entries_validated(
                algorithm,
                bytes,
                None,
                false,
                false,
                Some(retention),
                validated,
            )?;
            index_pack_from_entries(algorithm, parsed.pack_id, parsed.entries, version)
        }
    }
}

pub fn index_pack_bytes_with_store_and_version<S: GitObjectStore>(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    store: &S,
    version: PackIndexVersion,
) -> io::Result<IndexedPack> {
    match parse_pack_entries_index_only_if_base(algorithm, bytes, false)? {
        PackIndexOnlyResult::Fast {
            pack_id, entries, ..
        } => index_pack_from_entries(algorithm, pack_id, entries, version),
        PackIndexOnlyResult::RequiresFullParse {
            retention,
            validated,
        } => {
            let parsed = parse_pack_entries_validated(
                algorithm,
                bytes,
                Some(store),
                false,
                false,
                Some(retention),
                validated,
            )?;
            index_pack_from_entries(algorithm, parsed.pack_id, parsed.entries, version)
        }
    }
}

pub fn index_pack_bytes_with_store<S: GitObjectStore>(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    store: &S,
) -> io::Result<IndexedPack> {
    index_pack_bytes_with_store_and_version(algorithm, bytes, store, PackIndexVersion::V2)
}

pub fn index_pack_bytes_index_only(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<IndexedPackIndexOnly> {
    index_pack_bytes_index_only_with_version(algorithm, bytes, PackIndexVersion::V2)
}

pub fn index_pack_bytes_index_only_with_version(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    version: PackIndexVersion,
) -> io::Result<IndexedPackIndexOnly> {
    let parsed = match parse_pack_entries_index_only_if_base(algorithm, bytes, false)? {
        PackIndexOnlyResult::Fast {
            pack_id, entries, ..
        } => {
            let objects = entries.len();
            let index =
                encode_pack_index_from_owned_entries(algorithm, &pack_id, entries, version)?;
            return Ok(IndexedPackIndexOnly {
                pack_id,
                index,
                objects,
            });
        }
        PackIndexOnlyResult::RequiresFullParse {
            retention,
            validated,
        } => parse_pack_entries_validated(
            algorithm,
            bytes,
            None,
            false,
            false,
            Some(retention),
            validated,
        )?,
    };
    let pack_id = parsed.pack_id;
    let objects = parsed.entries.len();
    Ok(IndexedPackIndexOnly {
        index: encode_pack_index_from_owned_entries(algorithm, &pack_id, parsed.entries, version)?,
        pack_id,
        objects,
    })
}

pub fn index_pack_file_with_version(
    algorithm: GitHashAlgorithm,
    path: &Path,
    version: PackIndexVersion,
) -> io::Result<IndexedPack> {
    let pack = map_pack_file(path)?;
    let indexed = index_pack_bytes_with_version(algorithm, &pack, version)?;
    release_mapped_pack_pages(&pack);
    Ok(indexed)
}

pub fn index_pack_file(algorithm: GitHashAlgorithm, path: &Path) -> io::Result<IndexedPack> {
    index_pack_file_with_version(algorithm, path, PackIndexVersion::V2)
}

pub fn index_pack_file_with_store_and_version<S: GitObjectStore>(
    algorithm: GitHashAlgorithm,
    path: &Path,
    store: &S,
    version: PackIndexVersion,
) -> io::Result<IndexedPack> {
    let pack = map_pack_file(path)?;
    let indexed = index_pack_bytes_with_store_and_version(algorithm, &pack, store, version)?;
    release_mapped_pack_pages(&pack);
    Ok(indexed)
}

pub fn index_pack_file_with_store<S: GitObjectStore>(
    algorithm: GitHashAlgorithm,
    path: &Path,
    store: &S,
) -> io::Result<IndexedPack> {
    index_pack_file_with_store_and_version(algorithm, path, store, PackIndexVersion::V2)
}

pub fn index_pack_file_index_only(
    algorithm: GitHashAlgorithm,
    path_or_bytes: &Path,
) -> io::Result<IndexedPackIndexOnly> {
    index_pack_file_index_only_with_version(algorithm, path_or_bytes, PackIndexVersion::V2)
}

pub fn index_pack_file_index_only_with_version(
    algorithm: GitHashAlgorithm,
    path_or_bytes: &Path,
    version: PackIndexVersion,
) -> io::Result<IndexedPackIndexOnly> {
    let pack = map_pack_file(path_or_bytes)?;
    let parsed = match parse_pack_entries_index_only_if_base(algorithm, &pack, false)? {
        PackIndexOnlyResult::Fast {
            pack_id, entries, ..
        } => {
            let objects = entries.len();
            release_mapped_pack_pages(&pack);
            let index =
                encode_pack_index_from_owned_entries(algorithm, &pack_id, entries, version)?;
            return Ok(IndexedPackIndexOnly {
                pack_id,
                index,
                objects,
            });
        }
        PackIndexOnlyResult::RequiresFullParse {
            retention,
            validated,
        } => parse_pack_entries_validated(
            algorithm,
            &pack,
            None,
            false,
            false,
            Some(retention),
            validated,
        )?,
    };
    release_mapped_pack_pages(&pack);
    let pack_id = parsed.pack_id;
    let objects = parsed.entries.len();
    Ok(IndexedPackIndexOnly {
        index: encode_pack_index_from_owned_entries(algorithm, &pack_id, parsed.entries, version)?,
        pack_id,
        objects,
    })
}

pub fn for_each_pack_object<F>(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    mut for_each: F,
) -> io::Result<()>
where
    F: FnMut(&ObjectId, GitObjectKind, &[u8]) -> io::Result<()>,
{
    let parsed = parse_pack_entries(algorithm, bytes, None, false, true, None)?;
    for object in &parsed.objects {
        for_each(&object.id, object.kind, &object.content)?;
    }
    Ok(())
}

pub fn for_each_pack_object_file<F>(
    algorithm: GitHashAlgorithm,
    path: &Path,
    for_each: F,
) -> io::Result<()>
where
    F: FnMut(&ObjectId, GitObjectKind, &[u8]) -> io::Result<()>,
{
    let pack = map_pack_file(path)?;
    for_each_pack_object(algorithm, &pack, for_each)
}

pub fn decode_pack_objects(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<Vec<PackObjectData>> {
    parse_pack_entries(algorithm, bytes, None, false, true, None).map(|parsed| parsed.objects)
}

pub fn decode_pack_objects_with_store<S: GitObjectStore>(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    external_store: &S,
) -> io::Result<Vec<PackObjectData>> {
    parse_pack_entries(algorithm, bytes, Some(external_store), false, true, None)
        .map(|parsed| parsed.objects)
}

pub fn repair_thin_pack_bytes<S: GitObjectStore>(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    external_store: &S,
) -> io::Result<ThinPackRepair> {
    let parsed = parse_pack_entries(algorithm, bytes, Some(external_store), false, false, None)?;
    if parsed.external_bases.is_empty() {
        let sorted_positions = sorted_pack_index_positions(&parsed.entries)?;
        let index = encode_pack_index_with_positions_version(
            algorithm,
            &parsed.pack_id,
            &parsed.entries,
            &sorted_positions,
            PackIndexVersion::V2,
        )?;
        let reverse_index = encode_pack_reverse_index_from_positions(
            algorithm,
            &parsed.pack_id,
            &parsed.entries,
            &sorted_positions,
        )?;
        return Ok(ThinPackRepair {
            pack: bytes.to_vec(),
            indexed: IndexedPack {
                pack_id: parsed.pack_id,
                index,
                reverse_index,
                objects: parsed.entries.len(),
            },
            fixed_objects: 0,
        });
    }

    let fixed_objects = parsed.external_bases.len();
    let repaired_pack = prepend_external_bases_to_pack(algorithm, bytes, &parsed.external_bases)?;
    let indexed = index_pack_bytes(algorithm, &repaired_pack)?;
    Ok(ThinPackRepair {
        pack: repaired_pack,
        indexed,
        fixed_objects,
    })
}

pub fn encode_pack_from_store<S: GitObjectStore>(
    store: &S,
    algorithm: GitHashAlgorithm,
    ids: &[ObjectId],
) -> io::Result<Vec<u8>> {
    encode_undeltified_pack_from_store(store, algorithm, ids)
}

pub fn encode_pack_from_store_with_options<S: GitObjectStore>(
    store: &S,
    algorithm: GitHashAlgorithm,
    ids: &[ObjectId],
    options: PackEncodeOptions,
) -> io::Result<Vec<u8>> {
    if !pack_encode_options_allows_delta(options) {
        return encode_undeltified_pack_from_store(store, algorithm, ids);
    }
    let mut pack = Vec::with_capacity(pack_output_initial_capacity(algorithm, ids.len()));
    pack.extend_from_slice(PACK_MAGIC);
    pack.extend_from_slice(&PACK_VERSION_2.to_be_bytes());
    let count = pack_object_count_u32(ids.len())?;
    pack.extend_from_slice(&count.to_be_bytes());
    let mut encoded = VecDeque::<EncodedPackObject>::with_capacity(pack_delta_window_capacity(
        ids.len(),
        options,
    ));
    let mut encoded_content_bytes = 0_usize;
    for id in ids {
        if id.algorithm() != algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match pack algorithm",
            ));
        }
        let object = store.read_object(id)?;
        let offset = pack.len() as u64;
        if let Some(delta) = best_pack_delta(&object, &encoded, options)? {
            write_ofs_delta_object(&mut pack, offset, delta.base_offset, &delta.delta)?;
            push_encoded_pack_object(
                &mut encoded,
                &mut encoded_content_bytes,
                EncodedPackObject {
                    id: object.id,
                    kind: object.kind,
                    content: object.content,
                    offset,
                    depth: delta.base_depth + 1,
                },
                options,
            );
        } else {
            append_packed_base_object(&mut pack, object.kind, &object.content)?;
            push_encoded_pack_object(
                &mut encoded,
                &mut encoded_content_bytes,
                EncodedPackObject {
                    id: object.id,
                    kind: object.kind,
                    content: object.content,
                    offset,
                    depth: 0,
                },
                options,
            );
        }
    }
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&pack);
    pack.extend_from_slice(hasher.finalize().as_bytes());
    Ok(pack)
}

fn encode_undeltified_pack_from_store<S: GitObjectStore>(
    store: &S,
    algorithm: GitHashAlgorithm,
    ids: &[ObjectId],
) -> io::Result<Vec<u8>> {
    let mut pack = Vec::with_capacity(pack_output_initial_capacity(algorithm, ids.len()));
    pack.extend_from_slice(PACK_MAGIC);
    pack.extend_from_slice(&PACK_VERSION_2.to_be_bytes());
    let count = pack_object_count_u32(ids.len())?;
    pack.extend_from_slice(&count.to_be_bytes());
    for id in ids {
        if id.algorithm() != algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match pack algorithm",
            ));
        }
        let object = store.read_object(id)?;
        append_packed_base_object(&mut pack, object.kind, &object.content)?;
    }
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&pack);
    pack.extend_from_slice(hasher.finalize().as_bytes());
    Ok(pack)
}

struct EncodedPackObject {
    id: ObjectId,
    kind: GitObjectKind,
    content: Vec<u8>,
    offset: u64,
    depth: usize,
}

fn push_encoded_pack_object(
    encoded: &mut VecDeque<EncodedPackObject>,
    content_bytes: &mut usize,
    object: EncodedPackObject,
    options: PackEncodeOptions,
) {
    if options.window == 0 || options.depth == 0 {
        return;
    }
    let object_bytes = object.content.len();
    if object_bytes > PACK_DELTA_WINDOW_CONTENT_BUDGET {
        return;
    }
    while encoded.len() == options.window
        || content_bytes.saturating_add(object_bytes) > PACK_DELTA_WINDOW_CONTENT_BUDGET
    {
        if let Some(evicted) = encoded.pop_front() {
            *content_bytes = content_bytes.saturating_sub(evicted.content.len());
        } else {
            break;
        }
    }
    *content_bytes = content_bytes.saturating_add(object_bytes);
    encoded.push_back(object);
}

fn pack_delta_window_capacity(ids_len: usize, options: PackEncodeOptions) -> usize {
    if !pack_encode_options_allows_delta(options) {
        0
    } else {
        ids_len
            .min(options.window)
            .min(PACK_DELTA_WINDOW_INITIAL_CAPACITY_LIMIT)
    }
}

fn pack_encode_options_allows_delta(options: PackEncodeOptions) -> bool {
    options.window != 0 && options.depth != 0
}

fn pack_output_initial_capacity(algorithm: GitHashAlgorithm, ids_len: usize) -> usize {
    PACK_HEADER_LEN
        .saturating_add(algorithm.digest_len())
        .saturating_add(ids_len.saturating_mul(PACK_OUTPUT_OBJECT_BYTES_HINT))
        .min(PACK_OUTPUT_INITIAL_CAPACITY_LIMIT)
}

fn copy_pack_entry_exact_with_buffer<R: Read, W: Write + ?Sized>(
    reader: &mut R,
    writer: &mut W,
    len: u64,
    buffer: &mut [u8],
) -> io::Result<()> {
    let mut remaining = len;
    while remaining > 0 {
        let read_len = remaining.min(buffer.len() as u64) as usize;
        reader
            .read_exact(&mut buffer[..read_len])
            .map_err(|error| {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "packed object entry ended before expected length",
                    )
                } else {
                    error
                }
            })?;
        writer.write_all(&buffer[..read_len])?;
        remaining -= read_len as u64;
    }
    Ok(())
}

struct CandidateDelta {
    base_offset: u64,
    base_depth: usize,
    delta: Vec<u8>,
    packed_len: usize,
}

struct ReplacementDeltaPlan {
    prefix: usize,
    suffix: usize,
    insert_end: usize,
    delta_len: usize,
}

fn best_pack_delta(
    object: &LooseObject,
    encoded: &VecDeque<EncodedPackObject>,
    options: PackEncodeOptions,
) -> io::Result<Option<CandidateDelta>> {
    if options.window == 0 || options.depth == 0 || object.content.is_empty() {
        return Ok(None);
    }
    let mut best = None::<CandidateDelta>;
    let object_packed_len = packed_base_object_len(object.kind, &object.content)?;
    let max_delta_len = object
        .content
        .len()
        .checked_div(2)
        .and_then(|half| half.checked_sub(object.id.algorithm().digest_len()))
        .unwrap_or(0);
    if max_delta_len == 0 {
        return Ok(None);
    }
    for base in encoded.iter().rev().take(options.window) {
        if base.kind != object.kind || base.depth >= options.depth || base.id == object.id {
            continue;
        }
        let sizediff = if base.content.len() < object.content.len() {
            object.content.len() - base.content.len()
        } else {
            0
        };
        if sizediff >= max_delta_len || object.content.len() < base.content.len() / 32 {
            continue;
        }
        let Some(delta_plan) =
            replacement_delta_plan_if_better(&base.content, &object.content, max_delta_len)
        else {
            continue;
        };
        let delta = build_replacement_delta(&base.content, &object.content, delta_plan);
        let packed_len = packed_delta_object_len(base.offset, &delta)?;
        let best_packed_len = best
            .as_ref()
            .map_or(object_packed_len, |candidate| candidate.packed_len);
        if packed_len >= best_packed_len {
            continue;
        }
        best = Some(CandidateDelta {
            base_offset: base.offset,
            base_depth: base.depth,
            delta,
            packed_len,
        });
    }
    Ok(best)
}

struct CountWriter {
    count: usize,
}

impl CountWriter {
    fn new() -> Self {
        Self { count: 0 }
    }

    fn count(&self) -> usize {
        self.count
    }
}

impl Write for CountWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.count = self.count.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn packed_base_object_len(kind: GitObjectKind, content: &[u8]) -> io::Result<usize> {
    let mut header = [0_u8; 10];
    let header_len = pack_object_header_bytes(&mut header, kind, content.len() as u64);
    let mut writer = CountWriter::new();
    writer.write_all(&header[..header_len])?;
    let mut encoder = ZlibEncoder::new(&mut writer, Compression::default());
    encoder.write_all(content)?;
    let _ = encoder.finish()?;
    Ok(writer.count())
}

fn packed_delta_object_len(base_offset: u64, delta: &[u8]) -> io::Result<usize> {
    let object_offset = base_offset
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pack offset overflow"))?;
    let mut header = [0_u8; 20];
    let header_len =
        ofs_delta_header_bytes(&mut header, object_offset, base_offset, delta.len() as u64)?;
    let mut writer = CountWriter::new();
    writer.write_all(&header[..header_len])?;
    let mut encoder = ZlibEncoder::new(&mut writer, Compression::default());
    encoder.write_all(delta)?;
    let _ = encoder.finish()?;
    Ok(writer.count())
}
pub fn unpack_pack_to_loose<S: GitObjectStore + GitObjectSink>(
    store: &S,
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<UnpackPackStats> {
    let parsed = parse_pack_entries(algorithm, bytes, Some(store), false, true, None)?;
    for entry in &parsed.objects {
        store.write_object(entry.kind, &entry.content)?;
    }
    Ok(UnpackPackStats {
        objects: parsed.entries.len(),
    })
}

pub fn unpack_pack_file_to_loose<S: GitObjectStore + GitObjectSink>(
    store: &S,
    algorithm: GitHashAlgorithm,
    path: &Path,
) -> io::Result<UnpackPackStats> {
    let pack = map_pack_file(path)?;
    unpack_pack_to_loose(store, algorithm, &pack)
}

pub fn verify_pack_file(
    algorithm: GitHashAlgorithm,
    path: &Path,
    include_entries: bool,
) -> io::Result<VerifiedPack> {
    verify_pack_file_with_version(algorithm, path, PackIndexVersion::V2, include_entries)
}

pub fn verify_pack_file_with_version(
    algorithm: GitHashAlgorithm,
    path: &Path,
    version: PackIndexVersion,
    include_entries: bool,
) -> io::Result<VerifiedPack> {
    let pack = map_pack_file(path)?;
    verify_pack_bytes_with_version(algorithm, &pack, version, include_entries)
}

pub fn verify_pack_file_matches_index(
    algorithm: GitHashAlgorithm,
    pack_path: &Path,
    idx_path: &Path,
    include_entries: bool,
) -> io::Result<VerifiedPackIndexMatch> {
    verify_pack_file_matches_index_with_version(
        algorithm,
        pack_path,
        idx_path,
        PackIndexVersion::V2,
        include_entries,
    )
}

pub fn verify_pack_file_matches_index_with_version(
    algorithm: GitHashAlgorithm,
    pack_path: &Path,
    idx_path: &Path,
    index_version: PackIndexVersion,
    include_entries: bool,
) -> io::Result<VerifiedPackIndexMatch> {
    let index = PackIndex::read(idx_path, algorithm)?;
    let pack = map_pack_file(pack_path)?;
    match parse_pack_entries_index_only_if_base(algorithm, &pack, include_entries)? {
        PackIndexOnlyResult::Fast {
            pack_id,
            entries,
            object_metadata,
            pack_data_len,
        } => verified_pack_index_match_from_entries(
            &index,
            index_version,
            &pack_id,
            entries,
            object_metadata,
            pack_data_len,
            include_entries,
        ),
        PackIndexOnlyResult::RequiresFullParse {
            retention,
            validated,
        } => {
            let parsed = parse_pack_entries_validated(
                algorithm,
                &pack,
                None,
                include_entries,
                false,
                Some(retention),
                validated,
            )?;
            verified_pack_index_match_from_entries(
                &index,
                index_version,
                &parsed.pack_id,
                parsed.entries,
                parsed.object_metadata,
                parsed.pack_data_len,
                include_entries,
            )
        }
    }
}

pub fn verify_pack_bytes_with_version(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    index_version: PackIndexVersion,
    include_entries: bool,
) -> io::Result<VerifiedPack> {
    match parse_pack_entries_index_only_if_base(algorithm, bytes, include_entries)? {
        PackIndexOnlyResult::Fast {
            pack_id,
            entries,
            object_metadata,
            pack_data_len,
        } => {
            let objects = entries.len();
            let verify_entries = if include_entries {
                pack_verify_entries_from_metadata(&entries, &object_metadata, pack_data_len)
            } else {
                Vec::new()
            };
            let index =
                encode_pack_index_from_owned_entries(algorithm, &pack_id, entries, index_version)?;
            return Ok(VerifiedPack {
                objects,
                entries: verify_entries,
                index,
            });
        }
        PackIndexOnlyResult::RequiresFullParse {
            retention,
            validated,
        } => {
            let parsed = parse_pack_entries_validated(
                algorithm,
                bytes,
                None,
                include_entries,
                false,
                Some(retention),
                validated,
            )?;
            return verified_pack_from_parsed(algorithm, parsed, index_version, include_entries);
        }
    }
}

fn verified_pack_index_match_from_entries(
    index: &PackIndex,
    expected_version: PackIndexVersion,
    pack_id: &ObjectId,
    entries: Vec<PackIndexEntry>,
    object_metadata: Vec<PackObjectMetadata>,
    pack_data_len: usize,
    include_entries: bool,
) -> io::Result<VerifiedPackIndexMatch> {
    if !pack_index_matches_entries(index, expected_version, pack_id, &entries)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index does not match pack",
        ));
    }
    let objects = entries.len();
    let verify_entries = if include_entries {
        pack_verify_entries_from_metadata(&entries, &object_metadata, pack_data_len)
    } else {
        Vec::new()
    };
    Ok(VerifiedPackIndexMatch {
        objects,
        entries: verify_entries,
    })
}

fn pack_index_matches_entries(
    index: &PackIndex,
    expected_version: PackIndexVersion,
    pack_id: &ObjectId,
    entries: &[PackIndexEntry],
) -> io::Result<bool> {
    if index.version != expected_version
        || index.count != entries.len()
        || index.pack_checksum().as_bytes() != pack_id.as_bytes()
    {
        return Ok(false);
    }
    let sorted_positions = sorted_pack_index_positions(entries)?;
    for (sorted_idx, position) in sorted_positions.iter().enumerate() {
        let entry = &entries[position as usize];
        if index.object_bytes_at(sorted_idx) != entry.object_id.as_bytes()
            || index.offset_at(sorted_idx)? != entry.offset
        {
            return Ok(false);
        }
        if index.version == PackIndexVersion::V2 && index.crc_at(sorted_idx)? != entry.crc32 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn verified_pack_from_parsed(
    algorithm: GitHashAlgorithm,
    parsed: ParsedPack,
    index_version: PackIndexVersion,
    include_entries: bool,
) -> io::Result<VerifiedPack> {
    let object_count = parsed.entries.len();
    let entries = if include_entries {
        pack_verify_entries_from_metadata(
            &parsed.entries,
            &parsed.object_metadata,
            parsed.pack_data_len,
        )
    } else {
        Vec::new()
    };
    let pack_id = parsed.pack_id;
    let index =
        encode_pack_index_from_owned_entries(algorithm, &pack_id, parsed.entries, index_version)?;
    Ok(VerifiedPack {
        objects: object_count,
        entries,
        index,
    })
}

fn pack_verify_entries_from_metadata(
    entries: &[PackIndexEntry],
    object_metadata: &[PackObjectMetadata],
    pack_data_len: usize,
) -> Vec<PackVerifyEntry> {
    let mut verify_entries = Vec::with_capacity(entries.len());
    for i in 0..entries.len() {
        let pack_entry = &entries[i];
        let metadata = object_metadata[i];
        verify_entries.push(PackVerifyEntry {
            object_id: pack_entry.object_id.clone(),
            kind: metadata.kind(),
            object_size: u64::from(metadata.size()),
            packed_size: pack_entry_packed_size(entries, i, pack_data_len),
            offset: pack_entry.offset,
        });
    }
    verify_entries
}

fn pack_entry_packed_size(entries: &[PackIndexEntry], index: usize, pack_data_len: usize) -> usize {
    let start = entries[index].offset as usize;
    let end = entries
        .get(index + 1)
        .map_or(pack_data_len, |entry| entry.offset as usize);
    end.saturating_sub(start)
}

pub fn repair_thin_pack_file_to_path<S: GitObjectStore>(
    algorithm: GitHashAlgorithm,
    pack_path: &Path,
    external_store: &S,
    output_path: &Path,
    version: PackIndexVersion,
) -> io::Result<ThinPackFileRepair> {
    let pack = map_pack_file(pack_path)?;
    let parsed = parse_pack_entries(algorithm, &pack, Some(external_store), false, false, None)?;
    if parsed.external_bases.is_empty() {
        fs::copy(pack_path, output_path)?;
        let sorted_positions = sorted_pack_index_positions(&parsed.entries)?;
        let index = encode_pack_index_with_positions_version(
            algorithm,
            &parsed.pack_id,
            &parsed.entries,
            &sorted_positions,
            version,
        )?;
        let reverse_index = encode_pack_reverse_index_from_positions(
            algorithm,
            &parsed.pack_id,
            &parsed.entries,
            &sorted_positions,
        )?;
        return Ok(ThinPackFileRepair {
            indexed: IndexedPack {
                pack_id: parsed.pack_id,
                index,
                reverse_index,
                objects: parsed.entries.len(),
            },
            fixed_objects: 0,
        });
    }

    let fixed_objects = parsed.external_bases.len();
    let repaired_pack_id =
        write_repaired_thin_pack_to_path(algorithm, &pack, &parsed.external_bases, output_path)?;
    let repaired_pack = map_pack_file(output_path)?;
    let repaired_parsed = parse_pack_entries(algorithm, &repaired_pack, None, false, false, None)?;
    if repaired_parsed.pack_id != repaired_pack_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "repaired thin pack hash mismatch",
        ));
    }
    let sorted_positions = sorted_pack_index_positions(&repaired_parsed.entries)?;
    let index = encode_pack_index_with_positions_version(
        algorithm,
        &repaired_parsed.pack_id,
        &repaired_parsed.entries,
        &sorted_positions,
        version,
    )?;
    let reverse_index = encode_pack_reverse_index_from_positions(
        algorithm,
        &repaired_parsed.pack_id,
        &repaired_parsed.entries,
        &sorted_positions,
    )?;
    Ok(ThinPackFileRepair {
        indexed: IndexedPack {
            pack_id: repaired_parsed.pack_id,
            index,
            reverse_index,
            objects: repaired_parsed.entries.len(),
        },
        fixed_objects,
    })
}

pub fn write_pack_from_store_with_options<S: GitObjectStore + GitObjectSink>(
    store: &S,
    algorithm: GitHashAlgorithm,
    ids: &[ObjectId],
    options: PackEncodeOptions,
    out: &mut dyn Write,
) -> io::Result<()> {
    if !pack_encode_options_allows_delta(options) {
        return write_undeltified_pack_from_store(store, algorithm, ids, out);
    }
    if store
        .try_write_reusable_pack(algorithm, ids, out)?
        .is_some()
    {
        return Ok(());
    }
    write_delta_pack_from_store_with_options(store, algorithm, ids, options, out)
}

fn write_delta_pack_from_store_with_options<S: GitObjectStore + GitObjectSink>(
    store: &S,
    algorithm: GitHashAlgorithm,
    ids: &[ObjectId],
    options: PackEncodeOptions,
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut writer = PackHashWriter::new(out, algorithm);
    writer.write_all(PACK_MAGIC)?;
    writer.write_all(&PACK_VERSION_2.to_be_bytes())?;
    let count = pack_object_count_u32(ids.len())?;
    writer.write_all(&count.to_be_bytes())?;
    let mut encoded = VecDeque::<EncodedPackObject>::with_capacity(pack_delta_window_capacity(
        ids.len(),
        options,
    ));
    let mut encoded_content_bytes = 0_usize;
    let mut reusable_entry_buffer = [0_u8; PACK_REUSABLE_ENTRY_COPY_BUFFER_CAPACITY];
    for id in ids {
        if id.algorithm() != algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match pack algorithm",
            ));
        }
        let object = store.read_object(id)?;
        let offset = writer.position();
        if let Some(delta) = best_pack_delta(&object, &encoded, options)? {
            write_ofs_delta_object_to_writer(&mut writer, offset, delta.base_offset, &delta.delta)?;
            push_encoded_pack_object(
                &mut encoded,
                &mut encoded_content_bytes,
                EncodedPackObject {
                    id: object.id,
                    kind: object.kind,
                    content: object.content,
                    offset,
                    depth: delta.base_depth + 1,
                },
                options,
            );
        } else {
            if !store.try_write_reusable_pack_object_with_buffer(
                id,
                &mut writer,
                &mut reusable_entry_buffer,
            )? {
                write_packed_base_object_to_writer(&mut writer, object.kind, &object.content)?;
            }
            push_encoded_pack_object(
                &mut encoded,
                &mut encoded_content_bytes,
                EncodedPackObject {
                    id: object.id,
                    kind: object.kind,
                    content: object.content,
                    offset,
                    depth: 0,
                },
                options,
            );
        }
    }
    let pack_id = writer.finalize();
    out.write_all(pack_id.as_bytes())?;
    Ok(())
}

fn pack_object_count_u32(count: usize) -> io::Result<u32> {
    u32::try_from(count).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "pack object count exceeds pack v2 limit",
        )
    })
}

pub fn write_undeltified_pack_from_store<S: GitObjectStore + GitObjectSink>(
    store: &S,
    algorithm: GitHashAlgorithm,
    ids: &[ObjectId],
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut writer = PackHashWriter::new(out, algorithm);
    writer.write_all(PACK_MAGIC)?;
    writer.write_all(&PACK_VERSION_2.to_be_bytes())?;
    let count = u32::try_from(ids.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "pack object count exceeds pack v2 limit",
        )
    })?;
    writer.write_all(&count.to_be_bytes())?;
    let mut reusable_entry_buffer = [0_u8; PACK_REUSABLE_ENTRY_COPY_BUFFER_CAPACITY];
    for id in ids {
        if id.algorithm() != algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match pack algorithm",
            ));
        }
        if store.try_write_reusable_pack_object_with_buffer(
            id,
            &mut writer,
            &mut reusable_entry_buffer,
        )? {
            continue;
        }
        let object = store.read_object(id)?;
        write_packed_base_object_to_writer(&mut writer, object.kind, &object.content)?;
    }
    let pack_id = writer.finalize();
    out.write_all(pack_id.as_bytes())?;
    Ok(())
}

struct PackHashWriter<'a> {
    inner: &'a mut dyn Write,
    hasher: GitObjectHash,
    position: u64,
}

impl<'a> PackHashWriter<'a> {
    fn new(inner: &'a mut dyn Write, algorithm: GitHashAlgorithm) -> Self {
        Self {
            inner,
            hasher: GitObjectHash::new(algorithm),
            position: 0,
        }
    }

    const fn position(&self) -> u64 {
        self.position
    }

    fn finalize(self) -> ObjectId {
        self.hasher.finalize()
    }
}

impl Write for PackHashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.hasher.update(&buf[..written]);
        self.position = self
            .position
            .checked_add(written as u64)
            .ok_or_else(|| io::Error::other("pack stream offset overflow"))?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn index_pack_from_entries(
    algorithm: GitHashAlgorithm,
    pack_id: ObjectId,
    entries: Vec<PackIndexEntry>,
    version: PackIndexVersion,
) -> io::Result<IndexedPack> {
    let sorted_positions = sorted_pack_index_positions(&entries)?;
    let index = encode_pack_index_with_positions_version(
        algorithm,
        &pack_id,
        &entries,
        &sorted_positions,
        version,
    )?;
    let reverse_index =
        encode_pack_reverse_index_from_positions(algorithm, &pack_id, &entries, &sorted_positions)?;
    Ok(IndexedPack {
        pack_id,
        index,
        reverse_index,
        objects: entries.len(),
    })
}

enum PackIndexPositions {
    Identity(usize),
    Sorted(Vec<u32>),
}

impl PackIndexPositions {
    fn len(&self) -> usize {
        match self {
            Self::Identity(len) => *len,
            Self::Sorted(positions) => positions.len(),
        }
    }

    fn position_at(&self, index: usize) -> u32 {
        match self {
            Self::Identity(_) => index as u32,
            Self::Sorted(positions) => positions[index],
        }
    }

    fn iter(&self) -> PackIndexPositionIter<'_> {
        PackIndexPositionIter {
            positions: self,
            index: 0,
        }
    }
}

struct PackIndexPositionIter<'a> {
    positions: &'a PackIndexPositions,
    index: usize,
}

impl Iterator for PackIndexPositionIter<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.positions.len() {
            return None;
        }
        let position = self.positions.position_at(self.index);
        self.index += 1;
        Some(position)
    }
}

fn sorted_pack_index_positions(entries: &[PackIndexEntry]) -> io::Result<PackIndexPositions> {
    if entries.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index has too many objects",
        ));
    }
    if pack_index_entries_are_sorted(entries) {
        return Ok(PackIndexPositions::Identity(entries.len()));
    }
    let mut positions = (0..entries.len() as u32).collect::<Vec<_>>();
    positions.sort_unstable_by(|left, right| {
        entries[*left as usize]
            .object_id
            .as_bytes()
            .cmp(entries[*right as usize].object_id.as_bytes())
    });
    Ok(PackIndexPositions::Sorted(positions))
}

fn pack_index_entries_are_sorted(entries: &[PackIndexEntry]) -> bool {
    entries
        .windows(2)
        .all(|pair| pair[0].object_id.as_bytes() <= pair[1].object_id.as_bytes())
}

#[cfg(test)]
fn encode_pack_index_with_version(
    algorithm: GitHashAlgorithm,
    pack_id: &ObjectId,
    entries: &[PackIndexEntry],
    version: PackIndexVersion,
) -> io::Result<Vec<u8>> {
    let sorted_positions = sorted_pack_index_positions(entries)?;
    encode_pack_index_with_positions_version(
        algorithm,
        pack_id,
        entries,
        &sorted_positions,
        version,
    )
}

fn encode_pack_index_from_owned_entries(
    algorithm: GitHashAlgorithm,
    pack_id: &ObjectId,
    mut entries: Vec<PackIndexEntry>,
    version: PackIndexVersion,
) -> io::Result<Vec<u8>> {
    if !pack_index_entries_are_sorted(&entries) {
        entries.sort_unstable_by(|left, right| {
            left.object_id.as_bytes().cmp(right.object_id.as_bytes())
        });
    }
    let sorted_positions = PackIndexPositions::Identity(entries.len());
    encode_pack_index_with_positions_version(
        algorithm,
        pack_id,
        &entries,
        &sorted_positions,
        version,
    )
}

fn encode_pack_index_with_positions_version(
    algorithm: GitHashAlgorithm,
    pack_id: &ObjectId,
    entries: &[PackIndexEntry],
    sorted_positions: &PackIndexPositions,
    version: PackIndexVersion,
) -> io::Result<Vec<u8>> {
    if entries.len() != sorted_positions.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index entry count mismatch",
        ));
    }
    match version {
        PackIndexVersion::V1 => encode_pack_index_v1(algorithm, pack_id, entries, sorted_positions),
        PackIndexVersion::V2 => encode_pack_index_v2(algorithm, pack_id, entries, sorted_positions),
    }
}

fn encode_pack_index_v1(
    algorithm: GitHashAlgorithm,
    pack_id: &ObjectId,
    entries: &[PackIndexEntry],
    sorted_positions: &PackIndexPositions,
) -> io::Result<Vec<u8>> {
    if entries.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index has too many objects",
        ));
    }

    let mut out = Vec::with_capacity(pack_index_v1_capacity(algorithm, entries.len())?);

    let mut count = 0_u32;
    let mut next = 0_usize;
    for bucket in 0_u8..=255 {
        while next < sorted_positions.len()
            && entries[sorted_positions.position_at(next) as usize]
                .object_id
                .as_bytes()[0]
                <= bucket
        {
            count = count.checked_add(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "pack index has too many objects",
                )
            })?;
            next += 1;
        }
        out.extend_from_slice(&count.to_be_bytes());
    }
    for position in sorted_positions.iter() {
        let entry = &entries[position as usize];
        if entry.offset > u32::MAX as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pack index v1 cannot represent offsets above 4GiB",
            ));
        }
        out.extend_from_slice(&(entry.offset as u32).to_be_bytes());
        out.extend_from_slice(entry.object_id.as_bytes());
    }

    if pack_id.algorithm() != algorithm {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pack checksum algorithm mismatch",
        ));
    }
    out.extend_from_slice(pack_id.as_bytes());
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&out);
    out.extend_from_slice(hasher.finalize().as_bytes());
    Ok(out)
}

fn encode_pack_index_v2(
    algorithm: GitHashAlgorithm,
    pack_id: &ObjectId,
    entries: &[PackIndexEntry],
    sorted_positions: &PackIndexPositions,
) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(pack_index_v2_capacity(algorithm, entries.len())?);
    out.extend_from_slice(IDX_MAGIC);
    out.extend_from_slice(&IDX_VERSION.to_be_bytes());

    let mut count = 0_u32;
    let mut next = 0_usize;
    for bucket in 0_u8..=255 {
        while next < sorted_positions.len()
            && entries[sorted_positions.position_at(next) as usize]
                .object_id
                .as_bytes()[0]
                <= bucket
        {
            count = count.checked_add(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "pack index has too many objects",
                )
            })?;
            next += 1;
        }
        out.extend_from_slice(&count.to_be_bytes());
    }
    for position in sorted_positions.iter() {
        let entry = &entries[position as usize];
        out.extend_from_slice(entry.object_id.as_bytes());
    }
    for position in sorted_positions.iter() {
        let entry = &entries[position as usize];
        out.extend_from_slice(&entry.crc32.to_be_bytes());
    }

    let mut large_offsets = 0_u32;
    for position in sorted_positions.iter() {
        let entry = &entries[position as usize];
        if entry.offset <= 0x7fff_ffff {
            out.extend_from_slice(&(entry.offset as u32).to_be_bytes());
        } else {
            let large_idx = large_offsets;
            large_offsets = large_offsets.checked_add(1).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "too many large pack offsets")
            })?;
            out.extend_from_slice(&(0x8000_0000 | large_idx).to_be_bytes());
        }
    }
    if large_offsets > 0 {
        out.reserve(pack_large_offset_table_capacity(large_offsets)?);
        for position in sorted_positions.iter() {
            let entry = &entries[position as usize];
            if entry.offset > 0x7fff_ffff {
                out.extend_from_slice(&entry.offset.to_be_bytes());
            }
        }
    }

    if pack_id.algorithm() != algorithm {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pack checksum algorithm mismatch",
        ));
    }
    out.extend_from_slice(pack_id.as_bytes());
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&out);
    out.extend_from_slice(hasher.finalize().as_bytes());
    Ok(out)
}

fn pack_index_v1_capacity(algorithm: GitHashAlgorithm, entries: usize) -> io::Result<usize> {
    let entry_bytes = 4_usize
        .checked_add(algorithm.digest_len())
        .ok_or_else(pack_index_capacity_overflow)?;
    256_usize
        .checked_mul(4)
        .and_then(|fanout| {
            entries
                .checked_mul(entry_bytes)
                .and_then(|body| fanout.checked_add(body))
        })
        .and_then(|len| len.checked_add(algorithm.digest_len() * 2))
        .ok_or_else(pack_index_capacity_overflow)
}

fn pack_index_v2_capacity(algorithm: GitHashAlgorithm, entries: usize) -> io::Result<usize> {
    let digest_len = algorithm.digest_len();
    let per_entry = digest_len
        .checked_add(4)
        .and_then(|len| len.checked_add(4))
        .ok_or_else(pack_index_capacity_overflow)?;
    8_usize
        .checked_add(256 * 4)
        .and_then(|header| {
            entries
                .checked_mul(per_entry)
                .and_then(|body| header.checked_add(body))
        })
        .and_then(|len| len.checked_add(digest_len * 2))
        .ok_or_else(pack_index_capacity_overflow)
}

fn pack_large_offset_table_capacity(large_offsets: u32) -> io::Result<usize> {
    (large_offsets as usize)
        .checked_mul(8)
        .ok_or_else(pack_index_capacity_overflow)
}

fn pack_reverse_index_capacity(algorithm: GitHashAlgorithm, entries: usize) -> io::Result<usize> {
    entries
        .checked_mul(4)
        .and_then(|body| 12_usize.checked_add(body))
        .and_then(|len| len.checked_add(algorithm.digest_len() * 2))
        .ok_or_else(pack_index_capacity_overflow)
}

fn pack_index_min_len(
    version: PackIndexVersion,
    entries: usize,
    digest_len: usize,
    records_start: usize,
) -> io::Result<usize> {
    let checksum_bytes = digest_len
        .checked_mul(2)
        .ok_or_else(pack_index_capacity_overflow)?;
    let per_entry = match version {
        PackIndexVersion::V1 => 4_usize
            .checked_add(digest_len)
            .ok_or_else(pack_index_capacity_overflow)?,
        PackIndexVersion::V2 => digest_len
            .checked_add(4)
            .and_then(|len| len.checked_add(4))
            .ok_or_else(pack_index_capacity_overflow)?,
    };
    entries
        .checked_mul(per_entry)
        .and_then(|body| records_start.checked_add(body))
        .and_then(|len| len.checked_add(checksum_bytes))
        .ok_or_else(pack_index_capacity_overflow)
}

fn pack_index_layout(
    version: PackIndexVersion,
    entries: usize,
    digest_len: usize,
) -> io::Result<PackIndexLayout> {
    let names_start = pack_index_fanout_start(version)
        .checked_add(256 * 4)
        .ok_or_else(pack_index_capacity_overflow)?;
    if version == PackIndexVersion::V1 {
        return Ok(PackIndexLayout {
            names_start,
            v2_crc_start: names_start,
            v2_offsets_start: names_start,
            v2_large_offsets_start: names_start,
        });
    }
    let names_bytes = entries
        .checked_mul(digest_len)
        .ok_or_else(pack_index_capacity_overflow)?;
    let table_bytes = entries
        .checked_mul(4)
        .ok_or_else(pack_index_capacity_overflow)?;
    let v2_crc_start = names_start
        .checked_add(names_bytes)
        .ok_or_else(pack_index_capacity_overflow)?;
    let v2_offsets_start = v2_crc_start
        .checked_add(table_bytes)
        .ok_or_else(pack_index_capacity_overflow)?;
    let v2_large_offsets_start = v2_offsets_start
        .checked_add(table_bytes)
        .ok_or_else(pack_index_capacity_overflow)?;
    Ok(PackIndexLayout {
        names_start,
        v2_crc_start,
        v2_offsets_start,
        v2_large_offsets_start,
    })
}

fn pack_index_capacity_overflow() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "pack index is too large")
}

fn encode_pack_reverse_index_from_positions(
    algorithm: GitHashAlgorithm,
    pack_id: &ObjectId,
    entries: &[PackIndexEntry],
    sorted_positions: &PackIndexPositions,
) -> io::Result<Vec<u8>> {
    if pack_id.algorithm() != algorithm {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pack checksum algorithm mismatch",
        ));
    }
    if entries.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack reverse index has too many objects",
        ));
    }
    if entries.len() != sorted_positions.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack reverse index entry count mismatch",
        ));
    }
    let mut out = Vec::with_capacity(pack_reverse_index_capacity(algorithm, entries.len())?);
    out.extend_from_slice(RIDX_MAGIC);
    out.extend_from_slice(&RIDX_VERSION.to_be_bytes());
    out.extend_from_slice(&reverse_index_hash_version(algorithm).to_be_bytes());
    match sorted_positions {
        PackIndexPositions::Identity(len) => {
            for index_position in 0..*len {
                let index_position = u32::try_from(index_position).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "pack reverse index has too many objects",
                    )
                })?;
                out.extend_from_slice(&index_position.to_be_bytes());
            }
        }
        PackIndexPositions::Sorted(_) => {
            let mut index_positions_by_pack_position = vec![0_u32; entries.len()];
            for (index_position, pack_position) in sorted_positions.iter().enumerate() {
                let pack_position = pack_position as usize;
                if pack_position >= entries.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "pack reverse index position out of range",
                    ));
                }
                let index_position = u32::try_from(index_position).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "pack reverse index has too many objects",
                    )
                })?;
                index_positions_by_pack_position[pack_position] = index_position;
            }
            for index_position in index_positions_by_pack_position {
                out.extend_from_slice(&index_position.to_be_bytes());
            }
        }
    }
    out.extend_from_slice(pack_id.as_bytes());
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&out);
    out.extend_from_slice(hasher.finalize().as_bytes());
    Ok(out)
}

fn reverse_index_hash_version(algorithm: GitHashAlgorithm) -> u32 {
    match algorithm {
        GitHashAlgorithm::Sha1 => 1,
        GitHashAlgorithm::Sha256 => 2,
    }
}

struct ParsedPack {
    pack_id: ObjectId,
    pack_data_len: usize,
    entries: Vec<PackIndexEntry>,
    objects: Vec<PackObjectData>,
    object_metadata: Vec<PackObjectMetadata>,
    external_bases: Vec<LooseObject>,
}

#[derive(Clone)]
struct ValidatedPackHeader {
    pack_id: ObjectId,
    object_count: usize,
    trailer_start: usize,
}

enum PackIndexOnlyResult {
    Fast {
        pack_id: ObjectId,
        pack_data_len: usize,
        entries: Vec<PackIndexEntry>,
        object_metadata: Vec<PackObjectMetadata>,
    },
    RequiresFullParse {
        retention: PackRetentionPlan,
        validated: ValidatedPackHeader,
    },
}

enum PackObjectRef {
    Internal(u32),
    External(u32),
}

struct PackResolvedBase<'a> {
    kind: GitObjectKind,
    content: &'a [u8],
    internal_index: Option<usize>,
}

struct ParsedPackObjectData(u32);

#[derive(Clone, Copy)]
struct PackObjectMetadata(u32);

const PACK_OBJECT_CONTENT_SLOT_MASK: u32 = 0x3fff_ffff;
const PACK_OBJECT_CONTENT_NOT_RETAINED: u32 = PACK_OBJECT_CONTENT_SLOT_MASK;

impl ParsedPackObjectData {
    fn new(kind: GitObjectKind, content_slot: Option<usize>) -> io::Result<Self> {
        let content_slot = match content_slot {
            Some(slot) => {
                let slot = u32::try_from(slot).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "pack retained object content count exceeds u30",
                    )
                })?;
                if slot >= PACK_OBJECT_CONTENT_NOT_RETAINED {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "pack retained object content count exceeds u30",
                    ));
                }
                slot
            }
            None => PACK_OBJECT_CONTENT_NOT_RETAINED,
        };
        Ok(Self((pack_object_kind_bits(kind) << 30) | content_slot))
    }

    fn kind(&self) -> GitObjectKind {
        pack_object_kind_from_bits(self.0 >> 30)
    }

    fn content_slot(&self) -> Option<usize> {
        let slot = self.0 & PACK_OBJECT_CONTENT_SLOT_MASK;
        if slot == PACK_OBJECT_CONTENT_NOT_RETAINED {
            return None;
        }
        usize::try_from(slot).ok()
    }

    fn content<'a>(&self, retained_contents: &'a [Vec<u8>]) -> &'a [u8] {
        let Some(slot) = self.content_slot() else {
            return &[];
        };
        retained_contents
            .get(slot)
            .map_or(&[], |content| content.as_slice())
    }

    fn has_content(&self) -> bool {
        self.content_slot().is_some()
    }

    fn release_content(&mut self, retained_contents: &mut [Vec<u8>]) {
        let Some(slot) = self.content_slot() else {
            return;
        };
        if let Some(content) = retained_contents.get_mut(slot) {
            *content = Vec::new();
        }
        self.0 = (self.0 & !PACK_OBJECT_CONTENT_SLOT_MASK) | PACK_OBJECT_CONTENT_NOT_RETAINED;
    }
}

impl PackObjectMetadata {
    const SIZE_MASK: u32 = 0x3fff_ffff;

    fn new(kind: GitObjectKind, size: usize) -> io::Result<Self> {
        let size = pack_object_size_metadata(size)?;
        let kind = match kind {
            GitObjectKind::Blob => 0_u32,
            GitObjectKind::Tree => 1,
            GitObjectKind::Commit => 2,
            GitObjectKind::Tag => 3,
        };
        Ok(Self((kind << 30) | size))
    }

    fn kind(self) -> GitObjectKind {
        match self.0 >> 30 {
            0 => GitObjectKind::Blob,
            1 => GitObjectKind::Tree,
            2 => GitObjectKind::Commit,
            _ => GitObjectKind::Tag,
        }
    }

    fn size(self) -> u32 {
        self.0 & Self::SIZE_MASK
    }
}

fn pack_object_kind_bits(kind: GitObjectKind) -> u32 {
    match kind {
        GitObjectKind::Blob => 0,
        GitObjectKind::Tree => 1,
        GitObjectKind::Commit => 2,
        GitObjectKind::Tag => 3,
    }
}

fn pack_object_kind_from_bits(bits: u32) -> GitObjectKind {
    match bits {
        0 => GitObjectKind::Blob,
        1 => GitObjectKind::Tree,
        2 => GitObjectKind::Commit,
        _ => GitObjectKind::Tag,
    }
}

struct PackRetentionPlan {
    offsets: HashMap<u64, usize>,
    ids: HashMap<ObjectId, usize>,
}

struct ParsedPackObject {
    id: ObjectId,
    kind: GitObjectKind,
    content: Vec<u8>,
    size: usize,
}

impl PackRetentionPlan {
    fn empty() -> Self {
        Self {
            offsets: HashMap::new(),
            ids: HashMap::new(),
        }
    }

    fn insert_offset(&mut self, offset: u64, reserve_hint: usize) {
        if self.offsets.is_empty() && reserve_hint > 1 {
            self.offsets
                .reserve(pack_retention_reserve_capacity(reserve_hint));
        }
        *self.offsets.entry(offset).or_insert(0) += 1;
    }

    fn insert_id(&mut self, id: ObjectId, reserve_hint: usize) {
        if self.ids.is_empty() && reserve_hint > 1 {
            self.ids
                .reserve(pack_retention_reserve_capacity(reserve_hint));
        }
        *self.ids.entry(id).or_insert(0) += 1;
    }

    fn consume_offset(&mut self, offset: u64) {
        decrement_pack_retention_count(&mut self.offsets, &offset);
    }

    fn consume_id(&mut self, id: &ObjectId) {
        decrement_pack_retention_count(&mut self.ids, id);
    }
}

fn decrement_pack_retention_count<K>(counts: &mut HashMap<K, usize>, key: &K)
where
    K: Eq + std::hash::Hash,
{
    let Some(count) = counts.get_mut(key) else {
        return;
    };
    *count = count.saturating_sub(1);
    if *count == 0 {
        counts.remove(key);
    }
}

fn pack_retention_reserve_capacity(reserve_hint: usize) -> usize {
    reserve_hint.min(PACK_RETENTION_RESERVE_CAPACITY_LIMIT)
}

fn pack_external_bases_initial_capacity(reserve_hint: usize) -> usize {
    reserve_hint.min(PACK_EXTERNAL_BASES_INITIAL_CAPACITY_LIMIT)
}

fn reserve_pack_external_bases(external_bases: &mut Vec<LooseObject>, reserve_hint: usize) {
    if external_bases.capacity() == 0 && reserve_hint > 1 {
        external_bases.reserve(pack_external_bases_initial_capacity(reserve_hint));
    }
}

fn parse_pack_entries(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    external_store: Option<&dyn GitObjectStore>,
    collect_packed_sizes: bool,
    retain_all_objects: bool,
    retention: Option<PackRetentionPlan>,
) -> io::Result<ParsedPack> {
    let validated = validate_pack_header_and_checksum(algorithm, bytes)?;
    parse_pack_entries_validated(
        algorithm,
        bytes,
        external_store,
        collect_packed_sizes,
        retain_all_objects,
        retention,
        validated,
    )
}

fn parse_pack_entries_validated(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    external_store: Option<&dyn GitObjectStore>,
    collect_packed_sizes: bool,
    retain_all_objects: bool,
    retention: Option<PackRetentionPlan>,
    validated: ValidatedPackHeader,
) -> io::Result<ParsedPack> {
    let ValidatedPackHeader {
        pack_id,
        object_count,
        trailer_start,
    } = validated;
    let digest_len = algorithm.digest_len();
    debug_assert!(bytes.len() >= 12 + digest_len);
    debug_assert_eq!(trailer_start + digest_len, bytes.len());
    let mut retention = if retain_all_objects {
        PackRetentionPlan::empty()
    } else if let Some(retention) = retention {
        retention
    } else {
        collect_pack_delta_base_references(algorithm, bytes, object_count, trailer_start)?
    };

    let mut cursor = std::io::Cursor::new(&bytes[..trailer_start]);
    cursor.set_position(12);
    let use_linear_ref_delta_lookup =
        pack_ref_delta_uses_linear_lookup(retain_all_objects, retention.ids.len());
    let external_base_capacity_hint = pack_external_bases_initial_capacity(retention.ids.len());
    let mut by_id: Option<HashMap<ObjectId, (GitObjectKind, PackObjectRef)>> = None;
    let mut external_bases: Vec<LooseObject> = Vec::new();
    let initial_capacity = pack_parse_initial_capacity(object_count);
    let mut entries = Vec::with_capacity(initial_capacity);
    let mut objects: Vec<ParsedPackObjectData> = Vec::with_capacity(initial_capacity);
    let mut retained_contents: Vec<Vec<u8>> = Vec::new();
    let mut object_metadata = if collect_packed_sizes {
        Vec::with_capacity(initial_capacity)
    } else {
        Vec::new()
    };
    for _ in 0..object_count {
        let offset = cursor.position();
        let crc_start = offset as usize;
        let parsed_object = match read_pack_object_header(&mut cursor)? {
            PackObjectHeader::Base { kind, size } => {
                if pack_can_stream_hash_base_object_content(retain_all_objects, &retention, offset)
                {
                    let size = usize::try_from(size).map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "packed object size overflows usize",
                        )
                    })?;
                    let content_start = cursor.position();
                    let mut hasher = GitObjectHash::new(algorithm);
                    hasher.update_object_header(kind, size);
                    let object_size = read_zlib_content_hashing(
                        &mut cursor,
                        &mut hasher,
                        512 * 1024 * 1024,
                        size as u64,
                    )?;
                    validate_packed_inflated_size(size as u64, object_size)?;
                    let id = hasher.finalize();
                    let content = if pack_should_retain_content(
                        retain_all_objects,
                        &retention,
                        offset,
                        &id,
                    ) {
                        cursor.set_position(content_start);
                        let content = read_zlib_content_from_cursor(
                            &mut cursor,
                            512 * 1024 * 1024,
                            size as u64,
                        )?;
                        validate_packed_inflated_size(size as u64, content.len())?;
                        content
                    } else {
                        Vec::new()
                    };
                    ParsedPackObject {
                        id,
                        kind,
                        content,
                        size: object_size,
                    }
                } else {
                    let content =
                        read_zlib_content_from_cursor(&mut cursor, 512 * 1024 * 1024, size)?;
                    validate_packed_inflated_size(size, content.len())?;
                    let id = hash_object(algorithm, kind, &content);
                    let size = content.len();
                    ParsedPackObject {
                        id,
                        kind,
                        content,
                        size,
                    }
                }
            }
            PackObjectHeader::OfsDelta { size } => {
                let base_offset = read_delta_base_offset(&mut cursor, offset)?;
                let base_index =
                    pack_object_offset_index(&entries, base_offset).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "ofs-delta base object not found",
                        )
                    })?;
                let base_index = base_index as usize;
                let base_object = &objects[base_index];
                let kind = base_object.kind();
                if !base_object.has_content() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "ofs-delta base object content was not retained",
                    ));
                }
                let base_content = base_object.content(&retained_contents);
                let parsed_object = if pack_can_stream_hash_delta_object_content(
                    retain_all_objects,
                    &retention,
                    offset,
                ) {
                    let delta_start = cursor.position();
                    let (id, result_size) = apply_zlib_delta_hashing_from_cursor(
                        algorithm,
                        kind,
                        base_content,
                        &mut cursor,
                        size,
                        512 * 1024 * 1024,
                    )?;
                    let content = if pack_should_retain_content(
                        retain_all_objects,
                        &retention,
                        offset,
                        &id,
                    ) {
                        cursor.set_position(delta_start);
                        apply_zlib_delta_from_cursor(
                            base_content,
                            &mut cursor,
                            size,
                            512 * 1024 * 1024,
                        )?
                    } else {
                        Vec::new()
                    };
                    ParsedPackObject {
                        id,
                        kind,
                        content,
                        size: result_size,
                    }
                } else {
                    let content = apply_zlib_delta_from_cursor(
                        base_content,
                        &mut cursor,
                        size,
                        512 * 1024 * 1024,
                    )?;
                    let id = hash_object(algorithm, kind, &content);
                    let size = content.len();
                    ParsedPackObject {
                        id,
                        kind,
                        content,
                        size,
                    }
                };
                retention.consume_offset(base_offset);
                pack_release_internal_base_if_unused(
                    &mut objects,
                    &entries,
                    retain_all_objects,
                    &retention,
                    base_index,
                    base_offset,
                    &mut retained_contents,
                );
                parsed_object
            }
            PackObjectHeader::RefDelta { size } => {
                let mut base = [0_u8; 32];
                cursor.read_exact(&mut base[..digest_len])?;
                let base_id = ObjectId::new(algorithm, &base[..digest_len]);
                let (parsed_object, internal_base_index) = {
                    let base = if use_linear_ref_delta_lookup {
                        pack_ref_delta_base_linear(
                            &base_id,
                            &entries,
                            &objects,
                            &retained_contents,
                            retain_all_objects,
                            &mut external_bases,
                            external_base_capacity_hint,
                            external_store,
                        )?
                    } else {
                        let by_id = by_id.get_or_insert_with(|| {
                            pack_object_id_map(&entries, &objects, &retention, retain_all_objects)
                        });
                        pack_ref_delta_base_mapped(
                            base_id.clone(),
                            by_id,
                            &objects,
                            &retained_contents,
                            retain_all_objects,
                            &mut external_bases,
                            external_base_capacity_hint,
                            external_store,
                        )?
                    };
                    let kind = base.kind;
                    let base_content = base.content;
                    let parsed_object = if pack_can_stream_hash_delta_object_content(
                        retain_all_objects,
                        &retention,
                        offset,
                    ) {
                        let delta_start = cursor.position();
                        let (id, result_size) = apply_zlib_delta_hashing_from_cursor(
                            algorithm,
                            kind,
                            base_content,
                            &mut cursor,
                            size,
                            512 * 1024 * 1024,
                        )?;
                        let content = if pack_should_retain_content(
                            retain_all_objects,
                            &retention,
                            offset,
                            &id,
                        ) {
                            cursor.set_position(delta_start);
                            apply_zlib_delta_from_cursor(
                                base_content,
                                &mut cursor,
                                size,
                                512 * 1024 * 1024,
                            )?
                        } else {
                            Vec::new()
                        };
                        ParsedPackObject {
                            id,
                            kind,
                            content,
                            size: result_size,
                        }
                    } else {
                        let content = apply_zlib_delta_from_cursor(
                            base_content,
                            &mut cursor,
                            size,
                            512 * 1024 * 1024,
                        )?;
                        let id = hash_object(algorithm, kind, &content);
                        let size = content.len();
                        ParsedPackObject {
                            id,
                            kind,
                            content,
                            size,
                        }
                    };
                    (parsed_object, base.internal_index)
                };
                retention.consume_id(&base_id);
                if !use_linear_ref_delta_lookup
                    && !retention.ids.contains_key(&base_id)
                    && let Some(by_id) = by_id.as_mut()
                {
                    by_id.remove(&base_id);
                }
                if let Some(base_index) = internal_base_index {
                    let base_offset = entries[base_index].offset;
                    pack_release_internal_base_if_unused(
                        &mut objects,
                        &entries,
                        retain_all_objects,
                        &retention,
                        base_index,
                        base_offset,
                        &mut retained_contents,
                    );
                }
                parsed_object
            }
        };
        let crc_end = cursor.position() as usize;
        let crc32 = crc32fast::hash(&bytes[crc_start..crc_end]);
        entries.push(PackIndexEntry {
            offset,
            object_id: parsed_object.id.clone(),
            crc32,
        });
        let object_size = parsed_object.size;
        let retain_content =
            pack_should_retain_content(retain_all_objects, &retention, offset, &parsed_object.id);
        let object_index = objects.len();
        if !use_linear_ref_delta_lookup
            && (retain_all_objects || retention.ids.contains_key(&parsed_object.id))
            && let Some(by_id) = by_id.as_mut()
        {
            by_id.insert(
                parsed_object.id.clone(),
                (
                    parsed_object.kind,
                    PackObjectRef::Internal(object_index as u32),
                ),
            );
        }
        if collect_packed_sizes {
            object_metadata.push(PackObjectMetadata::new(parsed_object.kind, object_size)?);
        }
        let content_slot = if retain_content || object_size == 0 {
            let slot = retained_contents.len();
            retained_contents.push(parsed_object.content);
            Some(slot)
        } else {
            None
        };
        objects.push(ParsedPackObjectData::new(parsed_object.kind, content_slot)?);
    }
    if cursor.position() != trailer_start as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack stream has trailing data before checksum",
        ));
    }
    let returned_objects = if pack_should_return_parsed_objects(retain_all_objects) {
        pack_return_parsed_objects(entries.as_slice(), objects, retained_contents)
    } else {
        Vec::new()
    };
    Ok(ParsedPack {
        pack_id,
        pack_data_len: trailer_start,
        entries,
        objects: returned_objects,
        object_metadata,
        external_bases,
    })
}

fn collect_pack_delta_base_references(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    object_count: usize,
    trailer_start: usize,
) -> io::Result<PackRetentionPlan> {
    let digest_len = algorithm.digest_len();
    let mut cursor = std::io::Cursor::new(&bytes[..trailer_start]);
    cursor.set_position(12);
    let mut retention = PackRetentionPlan::empty();
    for object_index in 0..object_count {
        let offset = cursor.position();
        let reserve_hint = object_count - object_index;
        let size = match read_pack_object_header(&mut cursor)? {
            PackObjectHeader::Base { size, .. } => size,
            PackObjectHeader::OfsDelta { size } => {
                retention.insert_offset(read_delta_base_offset(&mut cursor, offset)?, reserve_hint);
                size
            }
            PackObjectHeader::RefDelta { size } => {
                let mut base = [0_u8; 32];
                cursor.read_exact(&mut base[..digest_len])?;
                retention.insert_id(ObjectId::new(algorithm, &base[..digest_len]), reserve_hint);
                size
            }
        };
        let _ = skip_zlib_content_from_cursor(&mut cursor, 512 * 1024 * 1024, size)?;
    }
    if cursor.position() != trailer_start as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack stream has trailing data before checksum",
        ));
    }
    Ok(retention)
}

fn pack_object_offset_index(entries: &[PackIndexEntry], offset: u64) -> Option<u32> {
    entries
        .binary_search_by_key(&offset, |entry| entry.offset)
        .ok()
        .map(|index| index as u32)
}

fn pack_object_id_map(
    entries: &[PackIndexEntry],
    objects: &[ParsedPackObjectData],
    retention: &PackRetentionPlan,
    retain_all_objects: bool,
) -> HashMap<ObjectId, (GitObjectKind, PackObjectRef)> {
    let capacity_hint = if retain_all_objects {
        entries.len()
    } else {
        retention.ids.len()
    };
    let mut by_id = HashMap::with_capacity(pack_delta_lookup_initial_capacity(capacity_hint));
    for (index, object) in objects.iter().enumerate() {
        let object_id = &entries[index].object_id;
        if retain_all_objects || retention.ids.contains_key(object_id) {
            by_id.insert(
                object_id.clone(),
                (object.kind(), PackObjectRef::Internal(index as u32)),
            );
        }
    }
    by_id
}

fn pack_ref_delta_uses_linear_lookup(retain_all_objects: bool, base_ids: usize) -> bool {
    !retain_all_objects && base_ids <= PACK_REF_DELTA_LINEAR_LOOKUP_LIMIT
}

fn pack_should_return_parsed_objects(retain_all_objects: bool) -> bool {
    retain_all_objects
}

fn pack_return_parsed_objects(
    entries: &[PackIndexEntry],
    objects: Vec<ParsedPackObjectData>,
    mut retained_contents: Vec<Vec<u8>>,
) -> Vec<PackObjectData> {
    entries
        .iter()
        .zip(objects)
        .map(|(entry, object)| PackObjectData {
            id: entry.object_id.clone(),
            kind: object.kind(),
            content: pack_take_retained_content(&mut retained_contents, object.content_slot()),
        })
        .collect()
}

fn pack_take_retained_content(
    retained_contents: &mut [Vec<u8>],
    content_slot: Option<usize>,
) -> Vec<u8> {
    let Some(slot) = content_slot else {
        return Vec::new();
    };
    retained_contents
        .get_mut(slot)
        .map(std::mem::take)
        .unwrap_or_default()
}

fn pack_should_retain_content(
    retain_all_objects: bool,
    retention: &PackRetentionPlan,
    offset: u64,
    id: &ObjectId,
) -> bool {
    retain_all_objects
        || pack_should_retain_offset(retention, offset)
        || (!retention.ids.is_empty() && retention.ids.contains_key(id))
}

fn pack_should_retain_offset(retention: &PackRetentionPlan, offset: u64) -> bool {
    !retention.offsets.is_empty() && retention.offsets.contains_key(&offset)
}

fn pack_can_stream_hash_base_object_content(
    retain_all_objects: bool,
    retention: &PackRetentionPlan,
    offset: u64,
) -> bool {
    !retain_all_objects && !pack_should_retain_offset(retention, offset)
}

fn pack_can_stream_hash_delta_object_content(
    retain_all_objects: bool,
    retention: &PackRetentionPlan,
    offset: u64,
) -> bool {
    !retain_all_objects && !pack_should_retain_offset(retention, offset)
}

fn pack_release_internal_base_if_unused(
    objects: &mut [ParsedPackObjectData],
    entries: &[PackIndexEntry],
    retain_all_objects: bool,
    retention: &PackRetentionPlan,
    index: usize,
    offset: u64,
    retained_contents: &mut [Vec<u8>],
) {
    if retain_all_objects
        || objects.get(index).is_none_or(|_| {
            pack_should_retain_content(false, retention, offset, &entries[index].object_id)
        })
    {
        return;
    }
    objects[index].release_content(retained_contents);
}

fn pack_ref_delta_base_linear<'a>(
    base_id: &ObjectId,
    entries: &[PackIndexEntry],
    objects: &'a [ParsedPackObjectData],
    retained_contents: &'a [Vec<u8>],
    retain_all_objects: bool,
    external_bases: &'a mut Vec<LooseObject>,
    external_base_capacity_hint: usize,
    external_store: Option<&dyn GitObjectStore>,
) -> io::Result<PackResolvedBase<'a>> {
    if let Some(index) = pack_object_id_index(entries, base_id) {
        let index = index as usize;
        if !retain_all_objects && !objects[index].has_content() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ref-delta base object content was not retained",
            ));
        }
        return Ok(PackResolvedBase {
            kind: objects[index].kind(),
            content: objects[index].content(retained_contents),
            internal_index: Some(index),
        });
    }
    if let Some(index) = pack_external_base_index(external_bases, base_id) {
        let base_object = &external_bases[index as usize];
        return Ok(PackResolvedBase {
            kind: base_object.kind,
            content: base_object.content.as_slice(),
            internal_index: None,
        });
    }
    let store = external_store.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "ref-delta base object not found",
        )
    })?;
    let object = store.read_object(base_id)?;
    reserve_pack_external_bases(external_bases, external_base_capacity_hint);
    let external_index = external_bases.len();
    external_bases.push(object);
    let base_object = &external_bases[external_index];
    Ok(PackResolvedBase {
        kind: base_object.kind,
        content: base_object.content.as_slice(),
        internal_index: None,
    })
}

fn pack_ref_delta_base_mapped<'a>(
    base_id: ObjectId,
    by_id: &mut HashMap<ObjectId, (GitObjectKind, PackObjectRef)>,
    objects: &'a [ParsedPackObjectData],
    retained_contents: &'a [Vec<u8>],
    retain_all_objects: bool,
    external_bases: &'a mut Vec<LooseObject>,
    external_base_capacity_hint: usize,
    external_store: Option<&dyn GitObjectStore>,
) -> io::Result<PackResolvedBase<'a>> {
    match by_id.get(&base_id) {
        Some((kind, PackObjectRef::Internal(index))) => {
            let index = *index as usize;
            if !retain_all_objects && !objects[index].has_content() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ref-delta base object content was not retained",
                ));
            }
            Ok(PackResolvedBase {
                kind: *kind,
                content: objects[index].content(retained_contents),
                internal_index: Some(index),
            })
        }
        Some((kind, PackObjectRef::External(index))) => Ok(PackResolvedBase {
            kind: *kind,
            content: external_bases[*index as usize].content.as_slice(),
            internal_index: None,
        }),
        None => {
            let store = external_store.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ref-delta base object not found",
                )
            })?;
            let object = store.read_object(&base_id)?;
            reserve_pack_external_bases(external_bases, external_base_capacity_hint);
            let external_index = external_bases.len() as u32;
            by_id.insert(
                base_id,
                (object.kind, PackObjectRef::External(external_index)),
            );
            external_bases.push(object);
            let base_object = &external_bases[external_index as usize];
            Ok(PackResolvedBase {
                kind: base_object.kind,
                content: base_object.content.as_slice(),
                internal_index: None,
            })
        }
    }
}

fn pack_object_id_index(entries: &[PackIndexEntry], id: &ObjectId) -> Option<u32> {
    entries
        .iter()
        .position(|entry| entry.object_id.as_bytes() == id.as_bytes())
        .map(|index| index as u32)
}

fn pack_external_base_index(objects: &[LooseObject], id: &ObjectId) -> Option<u32> {
    objects
        .iter()
        .position(|object| object.id.as_bytes() == id.as_bytes())
        .map(|index| index as u32)
}

fn pack_delta_lookup_initial_capacity(existing_entries: usize) -> usize {
    existing_entries.min(PACK_DELTA_LOOKUP_INITIAL_CAPACITY_LIMIT)
}

fn pack_parse_initial_capacity(object_count: usize) -> usize {
    object_count
}

fn pack_object_size_metadata(size: usize) -> io::Result<u32> {
    u32::try_from(size).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object size metadata overflows u32",
        )
    })
}

fn parse_pack_entries_index_only_if_base(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    collect_metadata: bool,
) -> io::Result<PackIndexOnlyResult> {
    let validated = validate_pack_header_and_checksum(algorithm, bytes)?;
    let digest_len = algorithm.digest_len();
    let trailer_start = validated.trailer_start;
    let pack_id = validated.pack_id.clone();
    let object_count = validated.object_count;

    let mut cursor = std::io::Cursor::new(&bytes[..trailer_start]);
    cursor.set_position(12);
    let mut retention = PackRetentionPlan::empty();
    let mut entries = Vec::with_capacity(pack_parse_initial_capacity(object_count));
    let initial_capacity = pack_parse_initial_capacity(object_count);
    let mut object_metadata = if collect_metadata {
        Vec::with_capacity(initial_capacity)
    } else {
        Vec::new()
    };
    for object_index in 0..object_count {
        let offset = cursor.position();
        let crc_start = offset as usize;
        let reserve_hint = object_count - object_index;
        match read_pack_object_header(&mut cursor)? {
            PackObjectHeader::Base { kind, size } => {
                let size = usize::try_from(size).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "packed object size overflows usize",
                    )
                })?;
                let mut hasher = GitObjectHash::new(algorithm);
                hasher.update_object_header(kind, size);
                let content_start = cursor.position() as usize;
                let mut crc32 = crc32fast::Hasher::new();
                crc32.update(&bytes[crc_start..content_start]);
                let packed_size = read_zlib_content_hashing_with_crc(
                    &mut cursor,
                    &mut hasher,
                    512 * 1024 * 1024,
                    size as u64,
                    Some(&mut crc32),
                )?;
                validate_packed_inflated_size(size as u64, packed_size)?;
                if collect_metadata {
                    object_metadata.push(PackObjectMetadata::new(kind, packed_size)?);
                }
                entries.push(PackIndexEntry {
                    offset,
                    object_id: hasher.finalize(),
                    crc32: crc32.finalize(),
                });
            }
            PackObjectHeader::OfsDelta { size } => {
                retention.insert_offset(read_delta_base_offset(&mut cursor, offset)?, reserve_hint);
                collect_remaining_delta_base_references(
                    algorithm,
                    &mut cursor,
                    size,
                    object_count - object_index - 1,
                    &mut retention,
                )?;
                if cursor.position() != trailer_start as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "pack stream has trailing data before checksum",
                    ));
                }
                return Ok(PackIndexOnlyResult::RequiresFullParse {
                    retention,
                    validated,
                });
            }
            PackObjectHeader::RefDelta { size } => {
                let mut base = [0_u8; 32];
                cursor.read_exact(&mut base[..digest_len])?;
                retention.insert_id(ObjectId::new(algorithm, &base[..digest_len]), reserve_hint);
                collect_remaining_delta_base_references(
                    algorithm,
                    &mut cursor,
                    size,
                    object_count - object_index - 1,
                    &mut retention,
                )?;
                if cursor.position() != trailer_start as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "pack stream has trailing data before checksum",
                    ));
                }
                return Ok(PackIndexOnlyResult::RequiresFullParse {
                    retention,
                    validated,
                });
            }
        }
    }
    if cursor.position() != trailer_start as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack stream has trailing data before checksum",
        ));
    }
    Ok(PackIndexOnlyResult::Fast {
        pack_id,
        pack_data_len: trailer_start,
        entries,
        object_metadata,
    })
}

fn collect_remaining_delta_base_references(
    algorithm: GitHashAlgorithm,
    cursor: &mut std::io::Cursor<&[u8]>,
    current_object_size: u64,
    remaining_objects: usize,
    retention: &mut PackRetentionPlan,
) -> io::Result<()> {
    let digest_len = algorithm.digest_len();
    let _ = skip_zlib_content_from_cursor(cursor, 512 * 1024 * 1024, current_object_size)?;
    for object_index in 0..remaining_objects {
        let offset = cursor.position();
        let reserve_hint = remaining_objects - object_index;
        let size = match read_pack_object_header(cursor)? {
            PackObjectHeader::Base { size, .. } => size,
            PackObjectHeader::OfsDelta { size } => {
                retention.insert_offset(read_delta_base_offset(cursor, offset)?, reserve_hint);
                size
            }
            PackObjectHeader::RefDelta { size } => {
                let mut base = [0_u8; 32];
                cursor.read_exact(&mut base[..digest_len])?;
                retention.insert_id(ObjectId::new(algorithm, &base[..digest_len]), reserve_hint);
                size
            }
        };
        let _ = skip_zlib_content_from_cursor(cursor, 512 * 1024 * 1024, size)?;
    }
    Ok(())
}

fn read_zlib_content_hashing(
    cursor: &mut std::io::Cursor<&[u8]>,
    hasher: &mut GitObjectHash,
    max_object_bytes: usize,
    expected_size: u64,
) -> io::Result<usize> {
    read_zlib_content_hashing_with_crc(cursor, hasher, max_object_bytes, expected_size, None)
}

fn read_zlib_content_hashing_with_crc(
    cursor: &mut std::io::Cursor<&[u8]>,
    hasher: &mut GitObjectHash,
    max_object_bytes: usize,
    expected_size: u64,
    mut crc32: Option<&mut crc32fast::Hasher>,
) -> io::Result<usize> {
    zlib_content_read_limit(max_object_bytes, expected_size)?;
    let start = cursor.position() as usize;
    let remaining = &cursor.get_ref()[start..];
    let mut decoder = ZlibDecoder::new(remaining);
    let mut buffer = [0_u8; PACK_ZLIB_STREAM_BUFFER_CAPACITY];
    let mut object_size = 0_usize;

    loop {
        let read_limit = zlib_expected_remaining_read_limit(object_size, expected_size)?;
        let read_len = read_limit.min(buffer.len());
        let compressed_before = decoder.total_in() as usize;
        let read = decoder.read(&mut buffer[..read_len])?;
        let compressed_after = decoder.total_in() as usize;
        if compressed_after > compressed_before {
            let compressed = remaining
                .get(compressed_before..compressed_after)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "packed object compressed byte range is invalid",
                    )
                })?;
            if let Some(crc32) = &mut crc32 {
                crc32.update(compressed);
            }
        }
        if read == 0 {
            break;
        }
        object_size = object_size.checked_add(read).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "packed object size overflow")
        })?;
        if object_size as u64 > expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object inflated size mismatch",
            ));
        }
        if object_size > max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed git object exceeds configured size limit",
            ));
        }
        hasher.update(&buffer[..read]);
    }

    cursor.set_position(start as u64 + decoder.total_in());
    Ok(object_size)
}

fn zlib_expected_remaining_read_limit(inflated: usize, expected_size: u64) -> io::Result<usize> {
    let inflated = u64::try_from(inflated).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object size overflows u64",
        )
    })?;
    let remaining = expected_size
        .checked_add(1)
        .and_then(|limit| limit.checked_sub(inflated))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object size overflows u64",
            )
        })?;
    usize::try_from(remaining).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object size overflows usize",
        )
    })
}

fn prepend_external_bases_to_pack(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    external_bases: &[LooseObject],
) -> io::Result<Vec<u8>> {
    let digest_len = algorithm.digest_len();
    if bytes.len() < 12 + digest_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack stream is too short",
        ));
    }
    let trailer_start = bytes.len() - digest_len;
    let original_count = parse_pack_header_bytes(&bytes[..12])?;
    let repaired_count = original_count
        .checked_add(u32::try_from(external_bases.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "thin pack has too many base objects",
            )
        })?)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "pack has too many objects"))?;

    let mut repaired = Vec::with_capacity(thin_pack_repair_initial_capacity(
        bytes.len(),
        external_bases.len(),
    )?);
    repaired.extend_from_slice(&bytes[..8]);
    repaired.extend_from_slice(&repaired_count.to_be_bytes());
    for object in external_bases {
        if object.id.algorithm() != algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "thin pack base algorithm mismatch",
            ));
        }
        if hash_object(algorithm, object.kind, &object.content) != object.id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "thin pack base object hash mismatch",
            ));
        }
        append_packed_base_object(&mut repaired, object.kind, &object.content)?;
    }
    repaired.extend_from_slice(&bytes[12..trailer_start]);
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&repaired);
    repaired.extend_from_slice(hasher.finalize().as_bytes());
    Ok(repaired)
}

fn thin_pack_repair_initial_capacity(
    pack_bytes: usize,
    external_bases: usize,
) -> io::Result<usize> {
    let extra = external_bases
        .saturating_mul(64)
        .min(THIN_PACK_REPAIR_EXTRA_CAPACITY_LIMIT);
    pack_bytes
        .checked_add(extra)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "pack is too large"))
}

fn write_repaired_thin_pack_to_path(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    external_bases: &[LooseObject],
    output_path: &Path,
) -> io::Result<ObjectId> {
    let digest_len = algorithm.digest_len();
    if bytes.len() < 12 + digest_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack stream is too short",
        ));
    }
    let trailer_start = bytes.len() - digest_len;
    let original_count = parse_pack_header_bytes(&bytes[..12])?;
    let repaired_count = original_count
        .checked_add(u32::try_from(external_bases.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "thin pack has too many base objects",
            )
        })?)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "pack has too many objects"))?;

    let mut file = fs::File::create(output_path)?;
    let mut writer = PackHashWriter::new(&mut file, algorithm);
    writer.write_all(&bytes[..8])?;
    writer.write_all(&repaired_count.to_be_bytes())?;
    for object in external_bases {
        if object.id.algorithm() != algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "thin pack base algorithm mismatch",
            ));
        }
        if hash_object(algorithm, object.kind, &object.content) != object.id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "thin pack base object hash mismatch",
            ));
        }
        write_packed_base_object_to_writer(&mut writer, object.kind, &object.content)?;
    }
    writer.write_all(&bytes[12..trailer_start])?;
    let pack_id = writer.finalize();
    file.write_all(pack_id.as_bytes())?;
    file.flush()?;
    Ok(pack_id)
}

fn append_packed_base_object(
    out: &mut Vec<u8>,
    kind: GitObjectKind,
    content: &[u8],
) -> io::Result<()> {
    write_pack_object_header(out, kind, content.len() as u64);
    let mut encoder = ZlibEncoder::new(out, Compression::default());
    encoder.write_all(content)?;
    let _ = encoder.finish()?;
    Ok(())
}

fn write_packed_base_object_to_writer<W: Write>(
    out: &mut W,
    kind: GitObjectKind,
    content: &[u8],
) -> io::Result<()> {
    let mut header = [0_u8; 10];
    let header_len = pack_object_header_bytes(&mut header, kind, content.len() as u64);
    out.write_all(&header[..header_len])?;
    let mut encoder = ZlibEncoder::new(out, Compression::default());
    encoder.write_all(content)?;
    let _ = encoder.finish()?;
    Ok(())
}

fn write_ofs_delta_object(
    out: &mut Vec<u8>,
    object_offset: u64,
    base_offset: u64,
    delta: &[u8],
) -> io::Result<()> {
    write_ofs_delta_header(out, object_offset, base_offset, delta.len() as u64)?;
    let mut encoder = ZlibEncoder::new(out, Compression::default());
    encoder.write_all(delta)?;
    let _ = encoder.finish()?;
    Ok(())
}

fn write_ofs_delta_object_to_writer<W: Write>(
    out: &mut W,
    object_offset: u64,
    base_offset: u64,
    delta: &[u8],
) -> io::Result<()> {
    let mut header = [0_u8; 20];
    let header_len =
        ofs_delta_header_bytes(&mut header, object_offset, base_offset, delta.len() as u64)?;
    out.write_all(&header[..header_len])?;
    let mut encoder = ZlibEncoder::new(out, Compression::default());
    encoder.write_all(delta)?;
    let _ = encoder.finish()?;
    Ok(())
}

fn write_ofs_delta_header(
    out: &mut Vec<u8>,
    object_offset: u64,
    base_offset: u64,
    size: u64,
) -> io::Result<()> {
    let mut header = [0_u8; 20];
    let header_len = ofs_delta_header_bytes(&mut header, object_offset, base_offset, size)?;
    out.extend_from_slice(&header[..header_len]);
    Ok(())
}

fn ofs_delta_header_bytes(
    out: &mut [u8; 20],
    object_offset: u64,
    base_offset: u64,
    size: u64,
) -> io::Result<usize> {
    let distance = object_offset.checked_sub(base_offset).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "ofs-delta base offset must precede object offset",
        )
    })?;
    let mut len = pack_delta_object_header_bytes(out, size);
    len += delta_base_offset_bytes(&mut out[len..], distance);
    Ok(len)
}

fn pack_delta_object_header_bytes(out: &mut [u8], mut size: u64) -> usize {
    let mut len = 0;
    let mut byte = (6_u8 << 4) | ((size as u8) & 0x0f);
    size >>= 4;
    if size != 0 {
        byte |= 0x80;
    }
    out[len] = byte;
    len += 1;
    while size != 0 {
        let mut byte = (size as u8) & 0x7f;
        size >>= 7;
        if size != 0 {
            byte |= 0x80;
        }
        out[len] = byte;
        len += 1;
    }
    len
}

fn delta_base_offset_bytes(out: &mut [u8], mut distance: u64) -> usize {
    let mut bytes = [0_u8; 10];
    let mut len = 1;
    bytes[0] = (distance & 0x7f) as u8;
    distance >>= 7;
    while distance != 0 {
        distance -= 1;
        bytes[len] = ((distance & 0x7f) as u8) | 0x80;
        len += 1;
        distance >>= 7;
    }
    for (idx, byte) in bytes[..len].iter().rev().enumerate() {
        out[idx] = *byte;
    }
    len
}

impl PackedObjectStore {
    pub fn new(objects_dir: impl Into<PathBuf>, algorithm: GitHashAlgorithm) -> Self {
        Self {
            objects_dir: objects_dir.into(),
            algorithm,
            max_object_bytes: 512 * 1024 * 1024,
            idx_paths_cache: Arc::new(Mutex::new(None)),
            last_index_lookup: Arc::new(Mutex::new(None)),
            last_pack_file: Arc::new(Mutex::new(None)),
            object_read_cache: Arc::new(Mutex::new(PackObjectReadCache::default())),
        }
    }

    pub fn with_max_object_bytes(mut self, max_object_bytes: usize) -> Self {
        self.max_object_bytes = max_object_bytes;
        self
    }

    pub fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }

        if let Some(lookup) = self.lookup_pack_index_for(id)? {
            return self.read_pack_object(
                lookup.pack_path.as_ref(),
                &lookup.index,
                lookup.offset,
                id,
            );
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "packed git object not found",
        ))
    }

    pub fn read_blob_prefix(&self, id: &ObjectId, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }

        let Some(lookup) = self.lookup_pack_index_for(id)? else {
            return Ok(None);
        };
        self.read_pack_blob_prefix(lookup.pack_path.as_ref(), lookup.offset, max_bytes)
    }

    pub fn resolve_prefix(&self, hex_prefix: &str) -> io::Result<ObjectId> {
        validate_hex_prefix(hex_prefix, self.algorithm)?;
        if hex_prefix.len() == self.algorithm.digest_len() * 2 {
            return ObjectId::from_hex(self.algorithm, hex_prefix);
        }

        let mut resolved = None;
        self.collect_prefix(hex_prefix, &mut resolved)?;
        resolved
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "packed git object not found"))
    }

    pub(crate) fn collect_prefix(
        &self,
        hex_prefix: &str,
        resolved: &mut Option<ObjectId>,
    ) -> io::Result<()> {
        validate_hex_prefix(hex_prefix, self.algorithm)?;
        let idx_paths = self.idx_paths()?;
        for idx_path in idx_paths.iter() {
            let index = read_cached_pack_index(idx_path, self.algorithm)?;
            index.collect_prefix(hex_prefix, resolved)?;
        }
        Ok(())
    }

    pub fn object_ids(&self) -> io::Result<Vec<ObjectId>> {
        let mut ids = Vec::new();
        self.append_object_ids(&mut ids)?;
        Ok(ids)
    }

    pub fn object_id_capacity_hint(&self) -> io::Result<usize> {
        let idx_paths = self.idx_paths()?;
        count_packed_object_ids(self.algorithm, &idx_paths)
    }

    pub(crate) fn has_object_ids(&self) -> io::Result<bool> {
        for idx_path in self.idx_paths()?.iter() {
            if pack_index_object_count(idx_path)? > 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn pack_names(&self) -> io::Result<Vec<String>> {
        let idx_paths = self.idx_paths()?;
        let mut names = Vec::with_capacity(pack_index_list_initial_capacity(idx_paths.len()));
        for idx_path in idx_paths.iter() {
            let pack_path = idx_path.with_extension("pack");
            if pack_path.is_file()
                && let Some(name) = pack_path.file_name().and_then(|name| name.to_str())
            {
                names.push(name.to_owned());
            }
        }
        Ok(names)
    }

    fn lookup_pack_index_for(&self, id: &ObjectId) -> io::Result<Option<PackIndexLookup>> {
        let mut checked_last_idx_path = None;
        {
            let mut last_lookup = self
                .last_index_lookup
                .lock()
                .map_err(|_| io::Error::other("pack index lookup cache mutex poisoned"))?;
            if let Some(lookup) = last_lookup.as_ref() {
                match fs::metadata(&lookup.idx_path) {
                    Ok(metadata) => {
                        let modified = metadata.modified().ok();
                        let len = metadata.len();
                        if lookup.modified == modified && lookup.len == len {
                            checked_last_idx_path = Some(lookup.idx_path.clone());
                            if let Some(offset) = lookup.index.offset_for(id)? {
                                return Ok(Some(PackIndexLookup {
                                    pack_path: lookup.pack_path.clone(),
                                    index: lookup.index.clone(),
                                    offset,
                                }));
                            }
                        } else {
                            *last_lookup = None;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        *last_lookup = None;
                    }
                    Err(error) => return Err(error),
                }
            }
        }

        let idx_paths = self.idx_paths()?;
        for idx_path in idx_paths.iter() {
            if checked_last_idx_path.as_ref() == Some(idx_path) {
                continue;
            }
            let index = match read_cached_pack_index(idx_path, self.algorithm) {
                Ok(index) => index,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if let Some(offset) = index.offset_for(id)? {
                let metadata = fs::metadata(idx_path)?;
                let pack_path = Arc::new(idx_path.with_extension("pack"));
                let mut last = self
                    .last_index_lookup
                    .lock()
                    .map_err(|_| io::Error::other("pack index lookup cache mutex poisoned"))?;
                *last = Some(CachedLastPackIndexLookup {
                    idx_path: idx_path.clone(),
                    pack_path: pack_path.clone(),
                    modified: metadata.modified().ok(),
                    len: metadata.len(),
                    index: index.clone(),
                });
                return Ok(Some(PackIndexLookup {
                    pack_path,
                    index,
                    offset,
                }));
            }
        }
        Ok(None)
    }

    fn idx_paths(&self) -> io::Result<Arc<Vec<PathBuf>>> {
        let pack_dir = self.objects_dir.join("pack");
        let metadata = match fs::metadata(&pack_dir) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(Arc::new(Vec::new()));
            }
            Err(err) => return Err(err),
        };
        let modified = metadata.modified().ok();
        let len = metadata.len();
        {
            let cache = self
                .idx_paths_cache
                .lock()
                .map_err(|_| io::Error::other("pack index path cache mutex poisoned"))?;
            if let Some(entry) = cache.as_ref()
                && entry.modified == modified
                && entry.len == len
            {
                return Ok(entry.paths.clone());
            }
        }

        let mut paths = Vec::with_capacity(PACK_INDEX_PATHS_INITIAL_CAPACITY_HINT);
        for entry in fs::read_dir(pack_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("idx") {
                paths.push(path);
            }
        }
        paths.sort();
        let paths = Arc::new(paths);
        let mut cache = self
            .idx_paths_cache
            .lock()
            .map_err(|_| io::Error::other("pack index path cache mutex poisoned"))?;
        *cache = Some(CachedPackIndexPaths {
            modified,
            len,
            paths: paths.clone(),
        });
        Ok(paths)
    }

    fn read_pack_object(
        &self,
        pack_path: &Path,
        index: &PackIndex,
        offset: u64,
        id: &ObjectId,
    ) -> io::Result<LooseObject> {
        if let Some((kind, content)) = self.cached_pack_object(pack_path, offset)? {
            let actual = hash_object(self.algorithm, kind, &content);
            if &actual != id {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed git object hash mismatch",
                ));
            }
            return Ok(LooseObject {
                id: id.clone(),
                kind,
                content,
            });
        }

        let (kind, content) = self.with_validated_pack_file(pack_path, |file, _| {
            self.read_pack_object_at(pack_path, file, index, offset, 0)
        })?;
        self.cache_pack_object(pack_path, offset, kind, &content)?;
        let actual = hash_object(self.algorithm, kind, &content);
        if &actual != id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed git object hash mismatch",
            ));
        }
        Ok(LooseObject {
            id: id.clone(),
            kind,
            content,
        })
    }

    fn read_pack_blob_prefix(
        &self,
        pack_path: &Path,
        offset: u64,
        max_bytes: usize,
    ) -> io::Result<Option<Vec<u8>>> {
        self.with_validated_pack_file(pack_path, |file, _| {
            file.seek(SeekFrom::Start(offset))?;
            match read_pack_object_header(file)? {
                PackObjectHeader::Base {
                    kind: GitObjectKind::Blob,
                    size,
                } => {
                    read_zlib_content_prefix(file, self.max_object_bytes, size, max_bytes).map(Some)
                }
                PackObjectHeader::Base { .. }
                | PackObjectHeader::OfsDelta { .. }
                | PackObjectHeader::RefDelta { .. } => Ok(None),
            }
        })
    }

    fn cached_pack_object(
        &self,
        pack_path: &Path,
        offset: u64,
    ) -> io::Result<Option<(GitObjectKind, Vec<u8>)>> {
        let key = PackObjectReadCacheKey {
            pack_path: pack_path.to_path_buf(),
            offset,
        };
        let cache = self
            .object_read_cache
            .lock()
            .map_err(|_| io::Error::other("pack object read cache mutex poisoned"))?;
        Ok(cache
            .entries
            .get(&key)
            .map(|entry| (entry.kind, entry.content.clone())))
    }

    fn cache_pack_object(
        &self,
        pack_path: &Path,
        offset: u64,
        kind: GitObjectKind,
        content: &[u8],
    ) -> io::Result<()> {
        let mut cache = self
            .object_read_cache
            .lock()
            .map_err(|_| io::Error::other("pack object read cache mutex poisoned"))?;
        cache.insert(
            PackObjectReadCacheKey {
                pack_path: pack_path.to_path_buf(),
                offset,
            },
            kind,
            content,
        );
        Ok(())
    }

    fn read_pack_object_at(
        &self,
        pack_path: &Path,
        file: &mut fs::File,
        index: &PackIndex,
        offset: u64,
        depth: usize,
    ) -> io::Result<(GitObjectKind, Vec<u8>)> {
        if depth > MAX_DELTA_DEPTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed delta chain is too deep",
            ));
        }
        if let Some(cached) = self.cached_pack_object(pack_path, offset)? {
            return Ok(cached);
        }
        file.seek(SeekFrom::Start(offset))?;
        let decoded: io::Result<(GitObjectKind, Vec<u8>)> = match read_pack_object_header(file)? {
            PackObjectHeader::Base { kind, size } => {
                let content = read_zlib_content(file, self.max_object_bytes, size)?;
                validate_packed_inflated_size(size, content.len())?;
                Ok((kind, content))
            }
            PackObjectHeader::OfsDelta { size } => {
                let base_offset = read_delta_base_offset(file, offset)?;
                let delta_start = file.stream_position()?;
                let (kind, base) =
                    self.read_pack_object_at(pack_path, file, index, base_offset, depth + 1)?;
                file.seek(SeekFrom::Start(delta_start))?;
                let content =
                    apply_zlib_delta_from_reader(&mut *file, &base, size, self.max_object_bytes)?;
                Ok((kind, content))
            }
            PackObjectHeader::RefDelta { size } => {
                let digest_len = self.algorithm.digest_len();
                let mut base = [0_u8; 32];
                file.read_exact(&mut base[..digest_len])?;
                let base_id = ObjectId::new(self.algorithm, &base[..digest_len]);
                let base_offset = index.offset_for(&base_id)?.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "ref-delta base object not found",
                    )
                })?;
                let delta_start = file.stream_position()?;
                let (kind, base) =
                    self.read_pack_object_at(pack_path, file, index, base_offset, depth + 1)?;
                file.seek(SeekFrom::Start(delta_start))?;
                let content =
                    apply_zlib_delta_from_reader(&mut *file, &base, size, self.max_object_bytes)?;
                Ok((kind, content))
            }
        };
        let decoded = decoded?;
        self.cache_pack_object(pack_path, offset, decoded.0, &decoded.1)?;
        Ok(decoded)
    }

    fn packed_object_hint(
        &self,
        pack_path: &Path,
        index: &PackIndex,
        offset: u64,
    ) -> io::Result<Option<(GitObjectKind, usize)>> {
        self.with_validated_pack_file(pack_path, |file, _| {
            self.read_pack_object_hint_at(file, index, offset, 0)
        })
    }

    pub fn delta_base_hint(&self, id: &ObjectId) -> io::Result<Option<ObjectId>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }

        let Some(lookup) = self.lookup_pack_index_for(id)? else {
            return Ok(None);
        };
        self.with_validated_pack_file(lookup.pack_path.as_ref(), |file, _| {
            file.seek(SeekFrom::Start(lookup.offset))?;
            match read_pack_object_header(file)? {
                PackObjectHeader::Base { .. } => Ok(None),
                PackObjectHeader::OfsDelta { .. } => {
                    let base_offset = read_delta_base_offset(file, lookup.offset)?;
                    index_object_id_for_offset(&lookup.index, base_offset)
                }
                PackObjectHeader::RefDelta { .. } => {
                    let digest_len = self.algorithm.digest_len();
                    let mut base = [0_u8; 32];
                    file.read_exact(&mut base[..digest_len])?;
                    Ok(Some(ObjectId::new(self.algorithm, &base[..digest_len])))
                }
            }
        })
    }

    pub fn object_disk_size_hint(&self, id: &ObjectId) -> io::Result<Option<u64>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }
        let Some(lookup) = self.lookup_pack_index_for(id)? else {
            return Ok(None);
        };
        let pack_len = fs::metadata(lookup.pack_path.as_ref())?.len();
        let pack_payload_end = pack_len
            .checked_sub(self.algorithm.digest_len() as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "pack file is too short"))?;
        let next_offset =
            index_next_offset_after(&lookup.index, lookup.offset)?.unwrap_or(pack_payload_end);
        if next_offset < lookup.offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pack index object offsets are not ordered",
            ));
        }
        Ok(Some(next_offset - lookup.offset))
    }

    fn with_validated_pack_file<T>(
        &self,
        pack_path: &Path,
        read: impl FnOnce(&mut fs::File, u64) -> io::Result<T>,
    ) -> io::Result<T> {
        let metadata = fs::metadata(pack_path)?;
        let modified = metadata.modified().ok();
        let len = metadata.len();
        let mut cache = self
            .last_pack_file
            .lock()
            .map_err(|_| io::Error::other("pack file cache mutex poisoned"))?;
        if let Some(cached) = cache.as_mut().filter(|cached| {
            cached.pack_path == pack_path && cached.modified == modified && cached.len == len
        }) {
            return read(&mut cached.file, cached.len);
        }

        let mut file = fs::File::open(pack_path)?;
        validate_pack_header(&mut file)?;
        *cache = Some(CachedLastPackFile {
            pack_path: pack_path.to_path_buf(),
            modified,
            len,
            file,
        });
        let cached = cache
            .as_mut()
            .expect("validated pack file was just inserted");
        read(&mut cached.file, cached.len)
    }

    fn read_pack_object_hint_at(
        &self,
        file: &mut fs::File,
        index: &PackIndex,
        offset: u64,
        depth: usize,
    ) -> io::Result<Option<(GitObjectKind, usize)>> {
        if depth > MAX_DELTA_DEPTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed delta chain is too deep",
            ));
        }
        file.seek(SeekFrom::Start(offset))?;
        match read_pack_object_header(&mut *file)? {
            PackObjectHeader::Base { kind, size } => {
                let size = usize::try_from(size).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "packed object size overflows usize",
                    )
                })?;
                Ok(Some((kind, size)))
            }
            PackObjectHeader::OfsDelta { size } => {
                let base_offset = read_delta_base_offset(file, offset)?;
                let (delta_base_size, result_size) =
                    read_zlib_delta_base_and_result_size(&mut *file, self.max_object_bytes, size)?;
                self.read_delta_object_hint(
                    file,
                    index,
                    base_offset,
                    depth,
                    delta_base_size,
                    result_size,
                )
            }
            PackObjectHeader::RefDelta { size } => {
                let digest_len = self.algorithm.digest_len();
                let mut base = [0_u8; 32];
                file.read_exact(&mut base[..digest_len])?;
                let base_id = ObjectId::new(self.algorithm, &base[..digest_len]);
                let base_offset = index.offset_for(&base_id)?.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "ref-delta base object not found",
                    )
                })?;
                let (delta_base_size, result_size) =
                    read_zlib_delta_base_and_result_size(&mut *file, self.max_object_bytes, size)?;
                self.read_delta_object_hint(
                    file,
                    index,
                    base_offset,
                    depth,
                    delta_base_size,
                    result_size,
                )
            }
        }
    }

    fn read_delta_object_hint(
        &self,
        file: &mut fs::File,
        index: &PackIndex,
        base_offset: u64,
        depth: usize,
        delta_base_size: u64,
        result_size: u64,
    ) -> io::Result<Option<(GitObjectKind, usize)>> {
        let Some((kind, base_size)) =
            self.read_pack_object_hint_at(file, index, base_offset, depth + 1)?
        else {
            return Ok(None);
        };
        if delta_base_size != base_size as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta base size mismatch",
            ));
        }
        if result_size > self.max_object_bytes as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta result exceeds configured size limit",
            ));
        }
        let result_size = usize::try_from(result_size).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "delta result size overflows usize",
            )
        })?;
        Ok(Some((kind, result_size)))
    }

    fn base_object_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }

        if let Some(lookup) = self.lookup_pack_index_for(id)? {
            return self.packed_object_hint(
                lookup.pack_path.as_ref(),
                &lookup.index,
                lookup.offset,
            );
        }

        Ok(None)
    }

    fn try_write_reusable_pack_object_inner(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
    ) -> io::Result<bool> {
        let mut buffer = [0_u8; PACK_REUSABLE_ENTRY_COPY_BUFFER_CAPACITY];
        self.try_write_reusable_pack_object_inner_with_buffer(id, writer, &mut buffer)
    }

    fn try_write_reusable_pack_object_inner_with_buffer(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
        buffer: &mut [u8],
    ) -> io::Result<bool> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }

        let Some(lookup) = self.lookup_pack_index_for(id)? else {
            return Ok(false);
        };
        self.with_validated_pack_file(lookup.pack_path.as_ref(), |file, pack_len| {
            let pack_data_end = pack_data_end_from_len(pack_len, self.algorithm)?;
            file.seek(SeekFrom::Start(lookup.offset))?;
            let PackObjectHeader::Base { kind, size } = read_pack_object_header(&mut *file)? else {
                return Ok(false);
            };
            if size > self.max_object_bytes as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed git object exceeds configured size limit",
                ));
            }
            let content_start = file.stream_position()?;
            if content_start >= pack_data_end {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object entry is truncated",
                ));
            }
            let max_compressed_len = pack_data_end.checked_sub(content_start).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object entry range is invalid",
                )
            })?;
            let compressed_len = verify_or_write_packed_base_object_content(
                &mut *file,
                max_compressed_len,
                self.algorithm,
                kind,
                size,
                id,
                None,
            )?;
            let entry_end = content_start.checked_add(compressed_len).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object entry range is invalid",
                )
            })?;
            if entry_end > pack_data_end {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object entry extends beyond pack data",
                ));
            }

            file.seek(SeekFrom::Start(lookup.offset))?;
            let entry_len = entry_end.checked_sub(lookup.offset).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object entry range is invalid",
                )
            })?;
            copy_pack_entry_exact_with_buffer(file, writer, entry_len, buffer)?;
            Ok(true)
        })
    }

    fn try_write_reusable_pack_from_full_pack_parts(
        &self,
        idx_paths: &[PathBuf],
        ids: &[ObjectId],
        writer: &mut dyn Write,
    ) -> io::Result<Option<ObjectId>> {
        let requested = ids.iter().cloned().collect::<HashSet<_>>();
        if requested.len() != ids.len() {
            return Ok(None);
        }
        let mut covered =
            HashSet::with_capacity(ids.len().min(PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT));
        let mut selected = Vec::<memmap2::Mmap>::new();

        for idx_path in idx_paths {
            let index = read_cached_pack_index(idx_path, self.algorithm)?;
            if index.count == 0 || index.count > ids.len().saturating_sub(covered.len()) {
                continue;
            }

            let mut pack_ids =
                Vec::with_capacity(index.count.min(PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT));
            let usable = index.object_ids_all(&mut |id| {
                if !requested.contains(id) || covered.contains(id) {
                    return Ok(false);
                }
                pack_ids.push(id.clone());
                Ok(true)
            })?;
            if !usable || pack_ids.len() != index.count {
                continue;
            }

            let pack_path = idx_path.with_extension("pack");
            let file = fs::File::open(&pack_path)?;
            let bytes = unsafe { memmap2::Mmap::map(&file)? };
            let validated = validate_pack_header_and_checksum(self.algorithm, &bytes)?;
            if validated.object_count != index.count || validated.pack_id != index.pack_checksum() {
                continue;
            }

            for id in pack_ids {
                covered.insert(id);
            }
            selected.push(bytes);
            if covered.len() == ids.len() {
                break;
            }
        }

        if selected.is_empty() || covered.len() != ids.len() {
            return Ok(None);
        }

        let mut pack_writer = PackHashWriter::new(writer, self.algorithm);
        pack_writer.write_all(PACK_MAGIC)?;
        pack_writer.write_all(&PACK_VERSION_2.to_be_bytes())?;
        pack_writer.write_all(&pack_object_count_u32(ids.len())?.to_be_bytes())?;
        let digest_len = self.algorithm.digest_len();
        for bytes in &selected {
            let data_end = bytes.len().checked_sub(digest_len).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "pack stream is too short")
            })?;
            pack_writer.write_all(&bytes[PACK_HEADER_LEN..data_end])?;
        }
        let pack_id = pack_writer.finalize();
        writer.write_all(pack_id.as_bytes())?;
        Ok(Some(pack_id))
    }

    fn try_write_blob_inner(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }

        let Some(lookup) = self.lookup_pack_index_for(id)? else {
            return Ok(false);
        };
        self.with_validated_pack_file(lookup.pack_path.as_ref(), |file, pack_len| {
            let pack_data_end = pack_data_end_from_len(pack_len, self.algorithm)?;
            file.seek(SeekFrom::Start(lookup.offset))?;
            let PackObjectHeader::Base { kind, size } = read_pack_object_header(&mut *file)? else {
                return Ok(false);
            };
            if kind != GitObjectKind::Blob {
                return Ok(false);
            }
            if size > self.max_object_bytes as u64 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed git object exceeds configured size limit",
                ));
            }
            let content_start = file.stream_position()?;
            if content_start >= pack_data_end {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object entry is truncated",
                ));
            }
            let max_compressed_len = pack_data_end.checked_sub(content_start).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object entry range is invalid",
                )
            })?;
            verify_or_write_packed_base_object_content(
                &mut *file,
                max_compressed_len,
                self.algorithm,
                kind,
                size,
                id,
                Some(writer),
            )?;
            Ok(true)
        })
    }

    fn try_write_blob_to_path_inner(
        &self,
        id: &ObjectId,
        min_bytes: usize,
        path: &Path,
    ) -> io::Result<bool> {
        let Some(size) = self.blob_size_hint(id)? else {
            return Ok(false);
        };
        if size < min_bytes || path.exists() {
            return Ok(false);
        }
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "blob output path has no parent",
            )
        })?;
        fs::create_dir_all(parent)?;
        let temp_path = temp_pack_blob_output_path(path, id);
        let temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        let result = (|| {
            let mut writer =
                io::BufWriter::with_capacity(PACK_BLOB_OUTPUT_BUFFER_CAPACITY, temp_file);
            if !self.try_write_blob_inner(id, &mut writer)? {
                return Ok(false);
            }
            writer.flush()?;
            fs::rename(&temp_path, path)?;
            Ok(true)
        })();
        if !matches!(result, Ok(true)) {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }
}

fn pack_object_id_growth_capacity(object_hint: usize) -> usize {
    object_hint.min(PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT)
}

fn reserve_pack_object_ids_spare(ids: &mut Vec<ObjectId>, object_hint: usize) {
    let desired_spare = pack_object_id_growth_capacity(object_hint);
    let spare = ids.capacity().saturating_sub(ids.len());
    if spare < desired_spare {
        ids.reserve(desired_spare);
    }
}

fn pack_index_ids_match_sorted_ids(index: &PackIndex, sorted_ids: &[ObjectId]) -> io::Result<bool> {
    let mut idx = 0_usize;
    let matches = index.object_ids_all(&mut |id| {
        let matches = sorted_ids
            .get(idx)
            .is_some_and(|expected| expected.as_bytes() == id.as_bytes());
        idx += 1;
        Ok(matches)
    })?;
    Ok(matches && idx == sorted_ids.len())
}

fn count_packed_object_ids(
    algorithm: GitHashAlgorithm,
    idx_paths: &[PathBuf],
) -> io::Result<usize> {
    if idx_paths.len() <= 1 {
        return idx_paths
            .first()
            .map_or(Ok(0), |idx_path| pack_index_object_count(idx_path));
    }

    let mut indexes = Vec::with_capacity(pack_index_list_initial_capacity(idx_paths.len()));
    for idx_path in idx_paths.iter() {
        indexes.push(read_cached_pack_index(idx_path, algorithm)?);
    }
    count_unique_sorted_pack_index_ids(&indexes)
}

impl GitObjectStore for PackedObjectStore {
    fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        Self::read_object(self, id)
    }

    fn object_id_capacity_hint(&self) -> io::Result<usize> {
        PackedObjectStore::object_id_capacity_hint(self)
    }

    fn contains_object(&self, id: &ObjectId) -> io::Result<bool> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match packed store",
            ));
        }

        Ok(self.lookup_pack_index_for(id)?.is_some())
    }

    fn object_storage_hint(&self, id: &ObjectId) -> io::Result<ObjectStorageHint> {
        if self.contains_object(id)? {
            return Ok(ObjectStorageHint::Packed);
        }
        Ok(ObjectStorageHint::Unknown)
    }

    fn object_count(&self) -> io::Result<usize> {
        let idx_paths = self.idx_paths()?;
        count_packed_object_ids(self.algorithm, &idx_paths)
    }

    fn for_each_object_id(
        &self,
        for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
    ) -> io::Result<()> {
        let idx_paths = self.idx_paths()?;
        if idx_paths.len() > 1 {
            let mut indexes = Vec::with_capacity(pack_index_list_initial_capacity(idx_paths.len()));
            for idx_path in idx_paths.iter() {
                indexes.push(read_cached_pack_index(idx_path, self.algorithm)?);
            }
            for_each_unique_sorted_pack_index_object_id(&indexes, self.algorithm, for_each)?;
            return Ok(());
        }
        for idx_path in idx_paths.iter() {
            let index = read_cached_pack_index(idx_path, self.algorithm)?;
            index.for_each_object_id(for_each)?;
        }
        Ok(())
    }

    fn try_write_reusable_pack(
        &self,
        algorithm: GitHashAlgorithm,
        ids: &[ObjectId],
        writer: &mut dyn Write,
    ) -> io::Result<Option<ObjectId>> {
        if algorithm != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pack algorithm does not match packed store",
            ));
        }
        if ids.is_empty() {
            return Ok(None);
        }

        let mut sorted_ids = ids.to_vec();
        sorted_ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

        let idx_paths = self.idx_paths()?;
        for idx_path in idx_paths.iter() {
            let index = read_cached_pack_index(idx_path, self.algorithm)?;
            if index.count != ids.len() || !pack_index_ids_match_sorted_ids(&index, &sorted_ids)? {
                continue;
            }

            let pack_path = idx_path.with_extension("pack");
            let file = fs::File::open(&pack_path)?;
            let bytes = unsafe { memmap2::Mmap::map(&file)? };
            let validated = validate_pack_header_and_checksum(self.algorithm, &bytes)?;
            if validated.object_count != ids.len() || validated.pack_id != index.pack_checksum() {
                continue;
            }
            writer.write_all(&bytes)?;
            return Ok(Some(validated.pack_id));
        }
        if let Some(pack_id) =
            self.try_write_reusable_pack_from_full_pack_parts(&idx_paths, ids, writer)?
        {
            return Ok(Some(pack_id));
        }
        Ok(None)
    }

    fn append_object_ids(&self, ids: &mut Vec<ObjectId>) -> io::Result<()> {
        let idx_paths = self.idx_paths()?;
        if ids.is_empty()
            && let [idx_path] = idx_paths.as_slice()
        {
            let index = read_cached_pack_index(idx_path, self.algorithm)?;
            reserve_pack_object_ids_spare(ids, index.count);
            index.for_each_object_id(&mut |id| {
                ids.push(id.clone());
                Ok(())
            })?;
            return Ok(());
        }
        if ids.is_empty() && idx_paths.len() > 1 {
            let mut indexes = Vec::with_capacity(pack_index_list_initial_capacity(idx_paths.len()));
            let mut object_hint = 0_usize;
            for idx_path in idx_paths.iter() {
                let index = read_cached_pack_index(idx_path, self.algorithm)?;
                object_hint = object_hint.checked_add(index.count).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "packed object count overflows usize",
                    )
                })?;
                indexes.push(index);
            }
            reserve_pack_object_ids_spare(ids, object_hint);
            append_unique_sorted_pack_index_ids(&indexes, ids)?;
            return Ok(());
        }
        for idx_path in idx_paths.iter() {
            let index = read_cached_pack_index(idx_path, self.algorithm)?;
            reserve_pack_object_ids_spare(ids, index.count);
            index.for_each_object_id(&mut |id| {
                ids.push(id.clone());
                Ok(())
            })?;
        }
        if !ids.is_empty() {
            ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            ids.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
        }
        Ok(())
    }

    fn object_kind_hint(&self, id: &ObjectId) -> io::Result<Option<GitObjectKind>> {
        if let Some((kind, _)) = self.base_object_hint(id)? {
            return Ok(Some(kind));
        }
        match self.read_object(id) {
            Ok(object) => Ok(Some(object.kind)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn object_header_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
        self.base_object_hint(id)
    }

    fn blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        match self.base_object_hint(id)? {
            Some((GitObjectKind::Blob, size)) => return Ok(Some(size)),
            Some(_) => return Ok(None),
            None => {}
        }
        match self.read_object(id) {
            Ok(object) if object.kind == GitObjectKind::Blob => Ok(Some(object.content.len())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn read_blob_prefix(&self, id: &ObjectId, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        PackedObjectStore::read_blob_prefix(self, id, max_bytes)
    }

    fn try_write_reusable_pack_object(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
    ) -> io::Result<bool> {
        self.try_write_reusable_pack_object_inner(id, writer)
    }

    fn try_write_reusable_pack_object_with_buffer(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
        buffer: &mut [u8],
    ) -> io::Result<bool> {
        self.try_write_reusable_pack_object_inner_with_buffer(id, writer, buffer)
    }

    fn try_write_blob(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        self.try_write_blob_inner(id, writer)
    }

    fn try_write_blob_to_path(
        &self,
        id: &ObjectId,
        min_bytes: usize,
        path: &Path,
    ) -> io::Result<bool> {
        self.try_write_blob_to_path_inner(id, min_bytes, path)
    }
}

fn count_unique_sorted_pack_index_ids(indexes: &[Arc<PackIndex>]) -> io::Result<usize> {
    let mut count = 0_usize;
    for_each_unique_sorted_pack_index_id(indexes, |_| {
        count = count.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object count overflows usize",
            )
        })?;
        Ok(())
    })?;
    Ok(count)
}

fn append_unique_sorted_pack_index_ids(
    indexes: &[Arc<PackIndex>],
    ids: &mut Vec<ObjectId>,
) -> io::Result<()> {
    let Some(first_index) = indexes.first() else {
        return Ok(());
    };
    for_each_unique_sorted_pack_index_id(indexes, |id| {
        ids.push(ObjectId::new(first_index.algorithm, id));
        Ok(())
    })
}

fn for_each_unique_sorted_pack_index_object_id(
    indexes: &[Arc<PackIndex>],
    algorithm: GitHashAlgorithm,
    for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
) -> io::Result<()> {
    if indexes.is_empty() {
        return Ok(());
    }
    for_each_unique_sorted_pack_index_id(indexes, |id| {
        let id = ObjectId::new(algorithm, id);
        for_each(&id)
    })
}

fn for_each_unique_sorted_pack_index_id<'a, F>(
    indexes: &'a [Arc<PackIndex>],
    mut visit: F,
) -> io::Result<()>
where
    F: FnMut(&'a [u8]) -> io::Result<()>,
{
    let mut heap = BinaryHeap::with_capacity(pack_index_merge_heap_initial_capacity(indexes.len()));
    for index in 0..indexes.len() {
        push_pack_index_cursor(indexes, index, 0, &mut heap);
    }
    loop {
        let Some(entry) = heap.pop() else {
            return Ok(());
        };
        let min_id = entry.id;
        visit(min_id)?;
        advance_pack_index_cursor(indexes, entry, min_id, &mut heap);
        while heap.peek().is_some_and(|entry| entry.id == min_id) {
            let entry = heap.pop().expect("heap peeked entry");
            advance_pack_index_cursor(indexes, entry, min_id, &mut heap);
        }
    }
}

fn advance_pack_index_cursor<'a>(
    indexes: &'a [Arc<PackIndex>],
    cursor: PackIndexCursor<'a>,
    current_id: &[u8],
    heap: &mut BinaryHeap<PackIndexCursor<'a>>,
) {
    let mut position = cursor.position;
    let index = indexes[cursor.index].as_ref();
    while position < index.count
        && index.object_bytes_at_with_layout(
            position,
            cursor.layout.digest_len,
            cursor.layout.names_start,
        ) == current_id
    {
        position += 1;
    }
    push_pack_index_cursor_with_layout(index, cursor.index, position, cursor.layout, heap);
}

fn push_pack_index_cursor<'a>(
    indexes: &'a [Arc<PackIndex>],
    index: usize,
    position: usize,
    heap: &mut BinaryHeap<PackIndexCursor<'a>>,
) {
    let pack_index = indexes[index].as_ref();
    let layout = pack_index.scan_layout();
    push_pack_index_cursor_with_layout(pack_index, index, position, layout, heap);
}

fn push_pack_index_cursor_with_layout<'a>(
    pack_index: &'a PackIndex,
    index: usize,
    position: usize,
    layout: PackIndexScanLayout,
    heap: &mut BinaryHeap<PackIndexCursor<'a>>,
) {
    if position < pack_index.count {
        heap.push(PackIndexCursor {
            id: pack_index.object_bytes_at_with_layout(
                position,
                layout.digest_len,
                layout.names_start,
            ),
            index,
            position,
            layout,
        });
    }
}

fn pack_index_merge_heap_initial_capacity(indexes_len: usize) -> usize {
    indexes_len.min(PACK_INDEX_MERGE_HEAP_INITIAL_CAPACITY_LIMIT)
}

fn pack_index_entry_initial_capacity(entries_len: usize) -> usize {
    entries_len.min(PACK_INDEX_ENTRY_INITIAL_CAPACITY_LIMIT)
}

fn pack_index_object_id_initial_capacity(entries_len: usize) -> usize {
    entries_len.min(PACK_INDEX_OBJECT_ID_INITIAL_CAPACITY_LIMIT)
}

fn pack_index_list_initial_capacity(indexes_len: usize) -> usize {
    indexes_len.min(PACK_INDEX_LIST_INITIAL_CAPACITY_LIMIT)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct PackIndexCursor<'a> {
    id: &'a [u8],
    index: usize,
    position: usize,
    layout: PackIndexScanLayout,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct PackIndexScanLayout {
    digest_len: usize,
    names_start: usize,
}

impl Ord for PackIndexCursor<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .id
            .cmp(self.id)
            .then_with(|| other.index.cmp(&self.index))
            .then_with(|| other.position.cmp(&self.position))
    }
}

impl PartialOrd for PackIndexCursor<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn verify_or_write_packed_base_object_content(
    file: &mut fs::File,
    max_compressed_len: u64,
    algorithm: GitHashAlgorithm,
    kind: GitObjectKind,
    size: u64,
    expected: &ObjectId,
    mut writer: Option<&mut dyn Write>,
) -> io::Result<u64> {
    let content_len = usize::try_from(size).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object size overflows usize",
        )
    })?;
    let mut decoder = ZlibDecoder::new(Read::by_ref(file).take(max_compressed_len));
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update_object_header(kind, content_len);
    let mut inflated = 0_u64;
    let mut buffer = [0_u8; PACK_ZLIB_STREAM_BUFFER_CAPACITY];
    loop {
        let read_limit = zlib_expected_remaining_read_limit(
            usize::try_from(inflated).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "packed object size overflows usize",
                )
            })?,
            size,
        )?;
        let read_len = read_limit.min(buffer.len());
        let read = decoder.read(&mut buffer[..read_len])?;
        if read == 0 {
            break;
        }
        inflated = inflated.checked_add(read as u64).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object size overflows u64",
            )
        })?;
        if inflated > size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object inflated beyond declared size",
            ));
        }
        hasher.update(&buffer[..read]);
        if let Some(writer) = writer.as_deref_mut() {
            writer.write_all(&buffer[..read])?;
        }
    }
    if inflated != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object inflated size mismatch",
        ));
    }
    if &hasher.finalize() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed git object hash mismatch",
        ));
    }
    Ok(decoder.total_in())
}

fn temp_pack_blob_output_path(path: &Path, id: &ObjectId) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("blob");
    path.with_file_name(format!(
        ".tmp-zmin-blob-{}-{}-{}-{name}",
        std::process::id(),
        PACK_BLOB_OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed),
        id.short_hex(12)
    ))
}

fn read_cached_pack_index(path: &Path, algorithm: GitHashAlgorithm) -> io::Result<Arc<PackIndex>> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified().ok();
    let len = metadata.len();
    let cache = PACK_INDEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let cache = cache
            .lock()
            .map_err(|_| io::Error::other("pack index cache mutex poisoned"))?;
        if let Some(entry) = cache.get(path)
            && entry.modified == modified
            && entry.len == len
        {
            return Ok(entry.index.clone());
        }
    }

    let index = Arc::new(PackIndex::read(path, algorithm)?);
    let mut cache = cache
        .lock()
        .map_err(|_| io::Error::other("pack index cache mutex poisoned"))?;
    if pack_index_cache_should_store(len) {
        trim_pack_index_cache_for_insert(&mut cache, path, len);
        cache.insert(
            path.to_path_buf(),
            CachedPackIndex {
                modified,
                len,
                index: index.clone(),
            },
        );
    } else {
        cache.remove(path);
    }
    Ok(index)
}

fn pack_data_end_from_len(pack_len: u64, algorithm: GitHashAlgorithm) -> io::Result<u64> {
    pack_len
        .checked_sub(algorithm.digest_len() as u64)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "pack file is shorter than its checksum",
            )
        })
}

fn trim_pack_index_cache_for_insert(
    cache: &mut HashMap<PathBuf, CachedPackIndex>,
    inserting_path: &Path,
    inserting_len: u64,
) {
    cache.remove(inserting_path);
    while cache.len() >= PACK_INDEX_CACHE_ENTRY_LIMIT
        || pack_index_cache_len_for_insert(cache, inserting_len) > PACK_INDEX_CACHE_BYTE_LIMIT
    {
        let Some(stale_path) = cache
            .keys()
            .find(|cached_path| cached_path.as_path() != inserting_path)
            .cloned()
        else {
            break;
        };
        cache.remove(&stale_path);
    }
}

fn pack_index_cache_should_store(len: u64) -> bool {
    len <= PACK_INDEX_CACHE_BYTE_LIMIT
}

fn pack_index_cache_len_for_insert(
    cache: &HashMap<PathBuf, CachedPackIndex>,
    inserting_len: u64,
) -> u64 {
    cache.values().fold(inserting_len, |total, entry| {
        total.saturating_add(entry.len)
    })
}

enum PackIndexBytes {
    Owned(Vec<u8>),
    Mapped(memmap2::Mmap),
}

impl std::ops::Deref for PackIndexBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Mapped(bytes) => bytes,
        }
    }
}

struct PackIndex {
    bytes: PackIndexBytes,
    fanout: [u32; 256],
    count: usize,
    algorithm: GitHashAlgorithm,
    version: PackIndexVersion,
    layout: PackIndexLayout,
}

#[derive(Debug, Clone, Copy)]
struct PackIndexLayout {
    names_start: usize,
    v2_crc_start: usize,
    v2_offsets_start: usize,
    v2_large_offsets_start: usize,
}

impl PackIndex {
    fn read(path: &Path, algorithm: GitHashAlgorithm) -> io::Result<Self> {
        let file = fs::File::open(path)?;
        let len = file.metadata()?.len();
        let bytes = if len == 0 {
            PackIndexBytes::Owned(Vec::new())
        } else {
            PackIndexBytes::Mapped(unsafe { memmap2::Mmap::map(&file)? })
        };
        Self::read_index_bytes(bytes, algorithm)
    }

    fn read_bytes(bytes: Vec<u8>, algorithm: GitHashAlgorithm) -> io::Result<Self> {
        Self::read_index_bytes(PackIndexBytes::Owned(bytes), algorithm)
    }

    fn read_index_bytes(bytes: PackIndexBytes, algorithm: GitHashAlgorithm) -> io::Result<Self> {
        let (fanout, count, version, layout) = validate_pack_index_layout(algorithm, &bytes)?;
        Ok(Self {
            bytes,
            fanout,
            count,
            algorithm,
            version,
            layout,
        })
    }
}

fn validate_pack_index_layout(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<([u32; 256], usize, PackIndexVersion, PackIndexLayout)> {
    if bytes.len() < 8 + 256 * 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index is too short",
        ));
    }
    let version = pack_index_version(bytes)?;
    if version == PackIndexVersion::V2 {
        let raw_version = read_u32(bytes, 4)?;
        if raw_version != IDX_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported pack index version {raw_version}"),
            ));
        }
    }

    let mut fanout = [0_u32; 256];
    let fanout_start = pack_index_fanout_start(version);
    for (idx, value) in fanout.iter_mut().enumerate() {
        *value = read_u32(bytes, fanout_start + idx * 4)?;
    }
    for pair in fanout.windows(2) {
        if pair[0] > pair[1] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pack index fanout is not sorted",
            ));
        }
    }
    let count = fanout[255] as usize;
    let digest_len = algorithm.digest_len();
    let layout = pack_index_layout(version, count, digest_len)?;
    let min_len = pack_index_min_len(version, count, digest_len, layout.names_start)?;
    if bytes.len() < min_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index is truncated",
        ));
    }
    validate_pack_index_checksum(algorithm, bytes)?;

    Ok((fanout, count, version, layout))
}

impl PackIndex {
    fn offset_for(&self, id: &ObjectId) -> io::Result<Option<u64>> {
        let first = id.as_bytes()[0] as usize;
        let start = if first == 0 {
            0
        } else {
            self.fanout[first - 1] as usize
        };
        let end = self.fanout[first] as usize;
        let needle = id.as_bytes();
        let idx = match self.version {
            PackIndexVersion::V1 => {
                let layout = self.scan_layout();
                let mut lo = start;
                let mut hi = end;
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    match self
                        .object_bytes_at_with_layout(mid, layout.digest_len, layout.names_start)
                        .cmp(needle)
                    {
                        std::cmp::Ordering::Less => lo = mid + 1,
                        std::cmp::Ordering::Greater => hi = mid,
                        std::cmp::Ordering::Equal => {
                            return Ok(Some(self.offset_at(mid)?));
                        }
                    }
                }
                return Ok(None);
            }
            PackIndexVersion::V2 => {
                let digest_len = self.algorithm.digest_len();
                let names_start = self.names_start();
                let names = &self.bytes[names_start..names_start + self.count * digest_len];
                match binary_search_object_name(names, digest_len, start, end, needle) {
                    Some(idx) => idx,
                    None => return Ok(None),
                }
            }
        };

        Ok(Some(self.offset_at(idx)?))
    }

    fn collect_prefix(&self, hex_prefix: &str, resolved: &mut Option<ObjectId>) -> io::Result<()> {
        let (lower, upper) = hex_prefix_search_bounds(hex_prefix, self.algorithm)?;
        let layout = self.scan_layout();
        let start = self.object_id_lower_bound_with_layout(&lower, layout);
        let end = upper.as_deref().map_or(self.count, |upper| {
            self.object_id_lower_bound_with_layout(upper, layout)
        });
        for idx in start..end {
            let bytes =
                self.object_bytes_at_with_layout(idx, layout.digest_len, layout.names_start);
            debug_assert!(object_bytes_match_hex_prefix(bytes, hex_prefix)?);
            record_prefix_candidate(resolved, ObjectId::new(self.algorithm, bytes))?;
        }
        Ok(())
    }

    fn object_ids(&self) -> Vec<ObjectId> {
        let mut ids = Vec::with_capacity(pack_index_object_id_initial_capacity(self.count));
        let digest_len = self.algorithm.digest_len();
        let names_start = self.names_start();
        match self.version {
            PackIndexVersion::V1 => {
                for idx in 0..self.count {
                    ids.push(ObjectId::new(
                        self.algorithm,
                        self.object_bytes_at_with_layout(idx, digest_len, names_start),
                    ));
                }
            }
            PackIndexVersion::V2 => {
                for idx in 0..self.count {
                    ids.push(ObjectId::new(
                        self.algorithm,
                        self.object_bytes_at_with_layout(idx, digest_len, names_start),
                    ));
                }
            }
        }
        ids
    }

    fn object_ids_are_subset_of(&self, other: &Self) -> bool {
        let mut needle_idx = 0;
        let mut haystack_idx = 0;
        let needle_digest_len = self.algorithm.digest_len();
        let needle_names_start = self.names_start();
        let haystack_digest_len = other.algorithm.digest_len();
        let haystack_names_start = other.names_start();
        while needle_idx < self.count {
            if haystack_idx == other.count {
                return false;
            }
            match self
                .object_bytes_at_with_layout(needle_idx, needle_digest_len, needle_names_start)
                .cmp(other.object_bytes_at_with_layout(
                    haystack_idx,
                    haystack_digest_len,
                    haystack_names_start,
                )) {
                std::cmp::Ordering::Less => return false,
                std::cmp::Ordering::Equal => {
                    needle_idx += 1;
                    haystack_idx += 1;
                }
                std::cmp::Ordering::Greater => haystack_idx += 1,
            }
        }
        true
    }

    fn for_each_object_id(
        &self,
        for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
    ) -> io::Result<()> {
        self.for_each_object_id_bytes(&mut |bytes| {
            for_each(&ObjectId::new(self.algorithm, bytes))?;
            Ok(true)
        })
    }

    fn object_ids_all(
        &self,
        predicate: &mut dyn FnMut(&ObjectId) -> io::Result<bool>,
    ) -> io::Result<bool> {
        let mut all = true;
        self.for_each_object_id_bytes(&mut |bytes| {
            if !predicate(&ObjectId::new(self.algorithm, bytes))? {
                all = false;
                return Ok(false);
            }
            Ok(true)
        })?;
        Ok(all)
    }

    fn for_each_object_id_bytes(
        &self,
        for_each: &mut dyn FnMut(&[u8]) -> io::Result<bool>,
    ) -> io::Result<()> {
        let digest_len = self.algorithm.digest_len();
        let names_start = self.names_start();
        match self.version {
            PackIndexVersion::V1 => {
                for idx in 0..self.count {
                    if !for_each(self.object_bytes_at_with_layout(idx, digest_len, names_start))? {
                        return Ok(());
                    }
                }
            }
            PackIndexVersion::V2 => {
                for idx in 0..self.count {
                    if !for_each(self.object_bytes_at_with_layout(idx, digest_len, names_start))? {
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }

    fn entries(&self) -> io::Result<Vec<PackIndexEntry>> {
        let mut entries = Vec::with_capacity(pack_index_entry_initial_capacity(self.count));
        self.for_each_entry(&mut |entry| {
            entries.push(entry);
            Ok(())
        })?;
        Ok(entries)
    }

    fn for_each_entry(
        &self,
        for_each: &mut dyn FnMut(PackIndexEntry) -> io::Result<()>,
    ) -> io::Result<()> {
        match self.version {
            PackIndexVersion::V1 => {
                let digest_len = self.algorithm.digest_len();
                let names_start = self.names_start();
                for idx in 0..self.count {
                    let record_start = names_start + idx * (4 + digest_len);
                    let offset = read_u32(&self.bytes, record_start)?;
                    if offset & 0x8000_0000 != 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "pack index v1 uses only 32-bit offsets",
                        ));
                    }
                    for_each(PackIndexEntry {
                        offset: offset as u64,
                        object_id: ObjectId::new(
                            self.algorithm,
                            &self.bytes[record_start + 4..record_start + 4 + digest_len],
                        ),
                        crc32: 0,
                    })?;
                }
            }
            PackIndexVersion::V2 => {
                let digest_len = self.algorithm.digest_len();
                let names_start = self.names_start();
                let crc_start = self.v2_crc_start();
                let offsets_start = self.v2_offsets_start();
                let large_offsets_start = self.v2_large_offsets_start();
                for idx in 0..self.count {
                    for_each(PackIndexEntry {
                        offset: self.v2_offset_at(idx, offsets_start, large_offsets_start)?,
                        object_id: ObjectId::new(
                            self.algorithm,
                            &self.bytes[names_start + idx * digest_len
                                ..names_start + (idx + 1) * digest_len],
                        ),
                        crc32: read_u32(&self.bytes, crc_start + idx * 4)?,
                    })?;
                }
            }
        }
        Ok(())
    }

    fn object_bytes_at(&self, idx: usize) -> &[u8] {
        let digest_len = self.algorithm.digest_len();
        let names_start = self.names_start();
        self.object_bytes_at_with_layout(idx, digest_len, names_start)
    }

    fn scan_layout(&self) -> PackIndexScanLayout {
        PackIndexScanLayout {
            digest_len: self.algorithm.digest_len(),
            names_start: self.names_start(),
        }
    }

    fn object_bytes_at_with_layout(
        &self,
        idx: usize,
        digest_len: usize,
        names_start: usize,
    ) -> &[u8] {
        let start = match self.version {
            PackIndexVersion::V1 => names_start + idx * (4 + digest_len) + 4,
            PackIndexVersion::V2 => names_start + idx * digest_len,
        };
        &self.bytes[start..start + digest_len]
    }

    fn object_id_lower_bound_with_layout(
        &self,
        needle: &[u8],
        layout: PackIndexScanLayout,
    ) -> usize {
        let mut left = 0;
        let mut right = self.count;
        while left < right {
            let mid = left + (right - left) / 2;
            if self.object_bytes_at_with_layout(mid, layout.digest_len, layout.names_start) < needle
            {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        left
    }

    fn offset_at(&self, idx: usize) -> io::Result<u64> {
        let digest_len = self.algorithm.digest_len();
        let names_start = self.names_start();
        if self.version == PackIndexVersion::V1 {
            let start = names_start + idx * (4 + digest_len);
            let offset = read_u32(&self.bytes, start)?;
            if offset & 0x8000_0000 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "pack index v1 uses only 32-bit offsets",
                ));
            }
            return Ok(offset as u64);
        }
        self.v2_offset_at(idx, self.v2_offsets_start(), self.v2_large_offsets_start())
    }

    fn crc_at(&self, idx: usize) -> io::Result<u32> {
        if self.version == PackIndexVersion::V1 {
            return Ok(0);
        }
        read_u32(&self.bytes, self.v2_crc_start() + idx * 4)
    }

    fn pack_checksum(&self) -> ObjectId {
        let digest_len = self.algorithm.digest_len();
        let start = self.bytes.len() - digest_len * 2;
        ObjectId::new(self.algorithm, &self.bytes[start..start + digest_len])
    }

    fn v2_offset_at(
        &self,
        idx: usize,
        offsets_start: usize,
        large_offsets_start: usize,
    ) -> io::Result<u64> {
        let offset = read_u32(&self.bytes, offsets_start + idx * 4)?;
        if offset & 0x8000_0000 == 0 {
            return Ok(offset as u64);
        }
        let large_idx = (offset & 0x7fff_ffff) as usize;
        let large_start = large_idx
            .checked_mul(8)
            .and_then(|offset| large_offsets_start.checked_add(offset))
            .ok_or_else(pack_index_capacity_overflow)?;
        read_u64(&self.bytes, large_start)
    }

    fn names_start(&self) -> usize {
        self.layout.names_start
    }

    fn v2_crc_start(&self) -> usize {
        self.layout.v2_crc_start
    }

    fn v2_offsets_start(&self) -> usize {
        self.layout.v2_offsets_start
    }

    fn v2_large_offsets_start(&self) -> usize {
        self.layout.v2_large_offsets_start
    }
}

fn index_object_id_for_offset(index: &PackIndex, offset: u64) -> io::Result<Option<ObjectId>> {
    let mut found = None;
    index.for_each_entry(&mut |entry| {
        if entry.offset == offset {
            found = Some(entry.object_id);
        }
        Ok(())
    })?;
    Ok(found)
}

fn index_next_offset_after(index: &PackIndex, offset: u64) -> io::Result<Option<u64>> {
    let mut next = None;
    index.for_each_entry(&mut |entry| {
        if entry.offset > offset && next.is_none_or(|current| entry.offset < current) {
            next = Some(entry.offset);
        }
        Ok(())
    })?;
    Ok(next)
}

fn object_bytes_match_hex_prefix(bytes: &[u8], hex_prefix: &str) -> io::Result<bool> {
    for (idx, expected) in hex_prefix.bytes().enumerate() {
        let byte = bytes[idx / 2];
        let nibble = if idx % 2 == 0 { byte >> 4 } else { byte & 0x0f };
        if nibble != hex_nibble(expected)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn hex_prefix_search_bounds(
    hex_prefix: &str,
    algorithm: GitHashAlgorithm,
) -> io::Result<(Vec<u8>, Option<Vec<u8>>)> {
    let mut lower = vec![0_u8; algorithm.digest_len()];
    let mut upper_increment = None;
    for (idx, byte) in hex_prefix.bytes().enumerate() {
        let nibble = hex_nibble(byte)?;
        write_bound_nibble(&mut lower, idx, nibble);
        if nibble < 0x0f {
            upper_increment = Some((idx, nibble + 1));
        }
    }
    let upper = if let Some((increment_idx, increment_nibble)) = upper_increment {
        let mut upper = vec![0_u8; algorithm.digest_len()];
        for (idx, byte) in hex_prefix.bytes().take(increment_idx).enumerate() {
            write_bound_nibble(&mut upper, idx, hex_nibble(byte)?);
        }
        write_bound_nibble(&mut upper, increment_idx, increment_nibble);
        Some(upper)
    } else {
        None
    };
    Ok((lower, upper))
}

fn write_bound_nibble(bytes: &mut [u8], idx: usize, nibble: u8) {
    if idx % 2 == 0 {
        bytes[idx / 2] |= nibble << 4;
    } else {
        bytes[idx / 2] |= nibble;
    }
}

fn hex_nibble(byte: u8) -> io::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git object id prefix contains non-hex characters",
        )),
    }
}

fn binary_search_object_name(
    names: &[u8],
    digest_len: usize,
    start: usize,
    end: usize,
    needle: &[u8],
) -> Option<usize> {
    let mut left = start;
    let mut right = end;
    while left < right {
        let mid = left + (right - left) / 2;
        match names[mid * digest_len..(mid + 1) * digest_len].cmp(needle) {
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Equal => return Some(mid),
            std::cmp::Ordering::Greater => right = mid,
        }
    }
    None
}

fn validate_pack_header(file: &mut fs::File) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut header = [0_u8; 12];
    file.read_exact(&mut header)?;
    if &header[..4] != PACK_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack file signature mismatch",
        ));
    }
    let version = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    if version != PACK_VERSION_2 && version != PACK_VERSION_3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported pack file version {version}"),
        ));
    }
    Ok(())
}

fn parse_pack_header_bytes(header: &[u8]) -> io::Result<u32> {
    if header.len() != 12 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack header has the wrong length",
        ));
    }
    if &header[..4] != PACK_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack file signature mismatch",
        ));
    }
    let version = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    if version != PACK_VERSION_2 && version != PACK_VERSION_3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported pack file version {version}"),
        ));
    }
    Ok(u32::from_be_bytes([
        header[8], header[9], header[10], header[11],
    ]))
}

fn validate_pack_header_and_checksum(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<ValidatedPackHeader> {
    let digest_len = algorithm.digest_len();
    if bytes.len() < 12 + digest_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack stream is too short",
        ));
    }
    let trailer_start = bytes.len() - digest_len;
    validate_pack_checksum(algorithm, &bytes[..trailer_start], &bytes[trailer_start..])?;
    Ok(ValidatedPackHeader {
        pack_id: ObjectId::new(algorithm, &bytes[trailer_start..]),
        object_count: parse_pack_header_bytes(&bytes[..12])? as usize,
        trailer_start,
    })
}

fn validate_pack_checksum(
    algorithm: GitHashAlgorithm,
    payload: &[u8],
    trailer: &[u8],
) -> io::Result<()> {
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(payload);
    let actual = hasher.finalize();
    if actual.as_bytes() != trailer {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack checksum mismatch",
        ));
    }
    Ok(())
}

fn validate_pack_index_checksum(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<()> {
    let digest_len = algorithm.digest_len();
    if bytes.len() < digest_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index checksum is truncated",
        ));
    }
    let checksum_start = bytes.len() - digest_len;
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update(&bytes[..checksum_start]);
    if hasher.finalize().as_bytes() != &bytes[checksum_start..] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pack index checksum mismatch",
        ));
    }
    Ok(())
}

enum PackObjectHeader {
    Base { kind: GitObjectKind, size: u64 },
    OfsDelta { size: u64 },
    RefDelta { size: u64 },
}

fn read_pack_object_header(reader: &mut impl Read) -> io::Result<PackObjectHeader> {
    let mut byte = read_byte(reader)?;
    let type_code = (byte >> 4) & 0b111;
    let mut size = (byte & 0x0f) as u64;
    let mut shift = 4;
    while byte & 0x80 != 0 {
        byte = read_byte(reader)?;
        size |= ((byte & 0x7f) as u64) << shift;
        shift += 7;
        if shift > 63 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object size overflows u64",
            ));
        }
    }
    match type_code {
        1 => Ok(PackObjectHeader::Base {
            kind: GitObjectKind::Commit,
            size,
        }),
        2 => Ok(PackObjectHeader::Base {
            kind: GitObjectKind::Tree,
            size,
        }),
        3 => Ok(PackObjectHeader::Base {
            kind: GitObjectKind::Blob,
            size,
        }),
        4 => Ok(PackObjectHeader::Base {
            kind: GitObjectKind::Tag,
            size,
        }),
        6 => Ok(PackObjectHeader::OfsDelta { size }),
        7 => Ok(PackObjectHeader::RefDelta { size }),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid packed object type",
        )),
    }
}

fn write_pack_object_header(out: &mut Vec<u8>, kind: GitObjectKind, size: u64) {
    let mut header = [0_u8; 10];
    let len = pack_object_header_bytes(&mut header, kind, size);
    out.extend_from_slice(&header[..len]);
}

fn pack_object_header_bytes(out: &mut [u8; 10], kind: GitObjectKind, mut size: u64) -> usize {
    let type_code = match kind {
        GitObjectKind::Commit => 1,
        GitObjectKind::Tree => 2,
        GitObjectKind::Blob => 3,
        GitObjectKind::Tag => 4,
    };
    let mut len = 0;
    let mut byte = ((type_code as u8) << 4) | ((size as u8) & 0x0f);
    size >>= 4;
    if size != 0 {
        byte |= 0x80;
    }
    out[len] = byte;
    len += 1;
    while size != 0 {
        let mut byte = (size as u8) & 0x7f;
        size >>= 7;
        if size != 0 {
            byte |= 0x80;
        }
        out[len] = byte;
        len += 1;
    }
    len
}

fn read_delta_base_offset(reader: &mut impl Read, object_offset: u64) -> io::Result<u64> {
    let mut byte = read_byte(reader)?;
    let mut distance = (byte & 0x7f) as u64;
    while byte & 0x80 != 0 {
        byte = read_byte(reader)?;
        distance = ((distance + 1) << 7) | (byte & 0x7f) as u64;
    }
    object_offset.checked_sub(distance).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "ofs-delta base offset points before pack start",
        )
    })
}

fn read_zlib_content(
    reader: impl Read,
    max_object_bytes: usize,
    expected_size: u64,
) -> io::Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(reader);
    read_zlib_decoder_content(&mut decoder, max_object_bytes, expected_size)
}

fn read_zlib_content_prefix(
    reader: impl Read,
    max_object_bytes: usize,
    expected_size: u64,
    max_bytes: usize,
) -> io::Result<Vec<u8>> {
    zlib_content_capacity(max_object_bytes, expected_size)?;
    let prefix_len = usize::try_from(expected_size)
        .unwrap_or(usize::MAX)
        .min(max_bytes);
    let mut decoder = ZlibDecoder::new(reader);
    let mut prefix = vec![0_u8; prefix_len];
    decoder.read_exact(&mut prefix)?;
    Ok(prefix)
}

fn read_zlib_content_from_cursor(
    cursor: &mut std::io::Cursor<&[u8]>,
    max_object_bytes: usize,
    expected_size: u64,
) -> io::Result<Vec<u8>> {
    let read_limit = zlib_content_read_limit(max_object_bytes, expected_size)?;
    let start = cursor.position() as usize;
    let remaining = &cursor.get_ref()[start..];
    let mut decoder = ZlibDecoder::new(remaining);
    let content = read_zlib_decoder_content_with_limit(
        &mut decoder,
        max_object_bytes,
        expected_size,
        read_limit,
    )?;
    cursor.set_position(start as u64 + decoder.total_in());
    Ok(content)
}

fn read_zlib_decoder_content<R: Read>(
    decoder: &mut ZlibDecoder<R>,
    max_object_bytes: usize,
    expected_size: u64,
) -> io::Result<Vec<u8>> {
    let read_limit = zlib_content_read_limit(max_object_bytes, expected_size)?;
    read_zlib_decoder_content_with_limit(decoder, max_object_bytes, expected_size, read_limit)
}

fn read_zlib_decoder_content_with_limit<R: Read>(
    decoder: &mut ZlibDecoder<R>,
    max_object_bytes: usize,
    expected_size: u64,
    mut remaining: u64,
) -> io::Result<Vec<u8>> {
    let mut content = Vec::with_capacity(zlib_content_capacity(max_object_bytes, expected_size)?);
    let mut buffer = [0_u8; PACK_ZLIB_STREAM_BUFFER_CAPACITY];
    while remaining > 0 {
        let read_len = remaining.min(buffer.len() as u64) as usize;
        let read = decoder.read(&mut buffer[..read_len])?;
        if read == 0 {
            break;
        }
        content.extend_from_slice(&buffer[..read]);
        remaining -= read as u64;
        if content.len() > max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed git object exceeds configured size limit",
            ));
        }
    }
    Ok(content)
}

fn read_zlib_delta_base_and_result_size(
    reader: impl Read,
    max_object_bytes: usize,
    expected_size: u64,
) -> io::Result<(u64, u64)> {
    zlib_content_capacity(max_object_bytes, expected_size)?;
    let mut decoder = ZlibDecoder::new(reader);
    let mut inflated = 0_u64;
    let base_size = read_delta_size_from_reader(&mut decoder, &mut inflated, expected_size)?;
    let result_size = read_delta_size_from_reader(&mut decoder, &mut inflated, expected_size)?;
    Ok((base_size, result_size))
}

fn read_delta_size_from_reader<R: Read>(
    reader: &mut R,
    inflated: &mut u64,
    expected_size: u64,
) -> io::Result<u64> {
    let mut size = 0_u64;
    let mut shift = 0;
    loop {
        let mut byte = [0_u8; 1];
        if reader.read(&mut byte)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta size is truncated",
            ));
        }
        *inflated = inflated.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "delta size overflows u64")
        })?;
        if *inflated > expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object inflated size mismatch",
            ));
        }
        size |= ((byte[0] & 0x7f) as u64) << shift;
        if byte[0] & 0x80 == 0 {
            return Ok(size);
        }
        shift += 7;
        if shift > 63 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta size overflows u64",
            ));
        }
    }
}

fn zlib_content_capacity(max_object_bytes: usize, expected_size: u64) -> io::Result<usize> {
    if expected_size > max_object_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed git object exceeds configured size limit",
        ));
    }
    let expected_size = usize::try_from(expected_size).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "packed git object size overflows usize",
        )
    })?;
    Ok(expected_size.min(ZLIB_CONTENT_INITIAL_CAPACITY_LIMIT))
}

fn zlib_content_read_limit(max_object_bytes: usize, expected_size: u64) -> io::Result<u64> {
    zlib_content_capacity(max_object_bytes, expected_size)?;
    expected_size.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "packed git object size overflows u64",
        )
    })
}

fn skip_zlib_content_from_cursor(
    cursor: &mut std::io::Cursor<&[u8]>,
    max_object_bytes: usize,
    expected_size: u64,
) -> io::Result<usize> {
    zlib_content_read_limit(max_object_bytes, expected_size)?;
    let start = cursor.position() as usize;
    let remaining = &cursor.get_ref()[start..];
    let mut decoder = ZlibDecoder::new(remaining);
    let mut buffer = [0_u8; PACK_ZLIB_STREAM_BUFFER_CAPACITY];
    let mut inflated = 0_usize;
    loop {
        let read_limit = zlib_expected_remaining_read_limit(inflated, expected_size)?;
        let read_len = read_limit.min(buffer.len());
        let read = decoder.read(&mut buffer[..read_len])?;
        if read == 0 {
            break;
        }
        inflated = inflated.checked_add(read).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "packed object size overflow")
        })?;
        if inflated as u64 > expected_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed object inflated size mismatch",
            ));
        }
        if inflated > max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packed git object exceeds configured size limit",
            ));
        }
    }
    cursor.set_position(start as u64 + decoder.total_in());
    validate_packed_inflated_size(expected_size, inflated)?;
    Ok(inflated)
}

fn validate_packed_inflated_size(expected: u64, actual: usize) -> io::Result<()> {
    if expected != actual as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object inflated size mismatch",
        ));
    }
    Ok(())
}

fn checked_delta_result_len(current: usize, added: usize, expected: u64) -> io::Result<usize> {
    let next = current.checked_add(added).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result size overflows usize",
        )
    })?;
    if next as u64 > expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result size mismatch",
        ));
    }
    Ok(next)
}

#[cfg(test)]
fn apply_delta(base: &[u8], delta: &[u8], max_object_bytes: usize) -> io::Result<Vec<u8>> {
    let mut cursor = 0;
    let base_size = read_delta_size(delta, &mut cursor)?;
    if base_size != base.len() as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta base size mismatch",
        ));
    }
    let result_size = read_delta_size(delta, &mut cursor)?;
    if result_size > max_object_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result exceeds configured size limit",
        ));
    }

    let mut out = Vec::with_capacity(delta_result_initial_capacity(
        max_object_bytes,
        result_size,
    )?);
    while cursor < delta.len() {
        let opcode = delta[cursor];
        cursor += 1;
        if opcode & 0x80 != 0 {
            let (copy_offset, copy_size) = read_delta_copy(delta, &mut cursor, opcode)?;
            let copy_end = copy_offset.checked_add(copy_size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "delta copy range overflows")
            })?;
            let copy = base.get(copy_offset..copy_end).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "delta copy range is outside base",
                )
            })?;
            let next_len = checked_delta_result_len(out.len(), copy.len(), result_size)?;
            out.extend_from_slice(copy);
            debug_assert_eq!(out.len(), next_len);
        } else if opcode != 0 {
            let insert_size = opcode as usize;
            let insert_end = cursor.checked_add(insert_size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "delta insert range overflows")
            })?;
            let insert = delta.get(cursor..insert_end).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "delta insert range is truncated",
                )
            })?;
            let next_len = checked_delta_result_len(out.len(), insert.len(), result_size)?;
            out.extend_from_slice(insert);
            debug_assert_eq!(out.len(), next_len);
            cursor = insert_end;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta instruction opcode 0 is reserved",
            ));
        }
        if out.len() > max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta result exceeds configured size limit",
            ));
        }
    }

    if out.len() as u64 != result_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result size mismatch",
        ));
    }
    Ok(out)
}

fn apply_zlib_delta_from_reader<R: Read>(
    reader: R,
    base: &[u8],
    expected_delta_size: u64,
    max_object_bytes: usize,
) -> io::Result<Vec<u8>> {
    zlib_content_capacity(max_object_bytes, expected_delta_size)?;
    let mut decoder = ZlibDecoder::new(reader);
    apply_delta_from_counted_reader(&mut decoder, base, expected_delta_size, max_object_bytes)
}

fn apply_delta_from_counted_reader<R: Read>(
    reader: &mut R,
    base: &[u8],
    expected_delta_size: u64,
    max_object_bytes: usize,
) -> io::Result<Vec<u8>> {
    let mut inflated = 0_u64;

    let base_size = read_delta_size_from_reader(reader, &mut inflated, expected_delta_size)?;
    if base_size != base.len() as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta base size mismatch",
        ));
    }
    let result_size = read_delta_size_from_reader(reader, &mut inflated, expected_delta_size)?;
    if result_size > max_object_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result exceeds configured size limit",
        ));
    }

    let mut out = Vec::with_capacity(delta_result_initial_capacity(
        max_object_bytes,
        result_size,
    )?);
    while let Some(opcode) = read_counted_delta_byte(reader, &mut inflated, expected_delta_size)? {
        if opcode & 0x80 != 0 {
            let (copy_offset, copy_size) =
                read_delta_copy_from_reader(reader, &mut inflated, expected_delta_size, opcode)?;
            let copy_end = copy_offset.checked_add(copy_size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "delta copy range overflows")
            })?;
            let copy = base.get(copy_offset..copy_end).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "delta copy range is outside base",
                )
            })?;
            let next_len = checked_delta_result_len(out.len(), copy.len(), result_size)?;
            out.extend_from_slice(copy);
            debug_assert_eq!(out.len(), next_len);
        } else if opcode != 0 {
            let insert_size = opcode as usize;
            let mut insert = [0_u8; 127];
            read_counted_delta_bytes(
                reader,
                &mut inflated,
                expected_delta_size,
                &mut insert[..insert_size],
                "delta insert range is truncated",
            )?;
            let next_len = checked_delta_result_len(out.len(), insert_size, result_size)?;
            out.extend_from_slice(&insert[..insert_size]);
            debug_assert_eq!(out.len(), next_len);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta instruction opcode 0 is reserved",
            ));
        }
        if out.len() > max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta result exceeds configured size limit",
            ));
        }
    }

    validate_packed_inflated_size(
        expected_delta_size,
        usize::try_from(inflated).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "packed git object size overflows usize",
            )
        })?,
    )?;
    if out.len() as u64 != result_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result size mismatch",
        ));
    }
    Ok(out)
}

fn apply_zlib_delta_from_cursor(
    base: &[u8],
    cursor: &mut std::io::Cursor<&[u8]>,
    expected_delta_size: u64,
    max_object_bytes: usize,
) -> io::Result<Vec<u8>> {
    zlib_content_capacity(max_object_bytes, expected_delta_size)?;
    let start = cursor.position() as usize;
    let remaining = &cursor.get_ref()[start..];
    let mut decoder = ZlibDecoder::new(remaining);
    let content =
        apply_delta_from_counted_reader(&mut decoder, base, expected_delta_size, max_object_bytes)?;
    cursor.set_position(start as u64 + decoder.total_in());
    Ok(content)
}

fn apply_zlib_delta_hashing_from_cursor(
    algorithm: GitHashAlgorithm,
    kind: GitObjectKind,
    base: &[u8],
    cursor: &mut std::io::Cursor<&[u8]>,
    expected_delta_size: u64,
    max_object_bytes: usize,
) -> io::Result<(ObjectId, usize)> {
    zlib_content_capacity(max_object_bytes, expected_delta_size)?;
    let start = cursor.position() as usize;
    let remaining = &cursor.get_ref()[start..];
    let mut decoder = ZlibDecoder::new(remaining);
    let mut inflated = 0_u64;

    let base_size = read_delta_size_from_reader(&mut decoder, &mut inflated, expected_delta_size)?;
    if base_size != base.len() as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta base size mismatch",
        ));
    }
    let result_size =
        read_delta_size_from_reader(&mut decoder, &mut inflated, expected_delta_size)?;
    if result_size > max_object_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result exceeds configured size limit",
        ));
    }
    let result_size = usize::try_from(result_size).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result size overflows usize",
        )
    })?;
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update_object_header(kind, result_size);
    let mut len = 0_usize;

    while let Some(opcode) =
        read_counted_delta_byte(&mut decoder, &mut inflated, expected_delta_size)?
    {
        if opcode & 0x80 != 0 {
            let (copy_offset, copy_size) = read_delta_copy_from_reader(
                &mut decoder,
                &mut inflated,
                expected_delta_size,
                opcode,
            )?;
            let copy_end = copy_offset.checked_add(copy_size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "delta copy range overflows")
            })?;
            let copy = base.get(copy_offset..copy_end).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "delta copy range is outside base",
                )
            })?;
            len = checked_delta_result_len(len, copy.len(), result_size as u64)?;
            hasher.update(copy);
        } else if opcode != 0 {
            let insert_size = opcode as usize;
            let mut insert = [0_u8; 127];
            read_counted_delta_bytes(
                &mut decoder,
                &mut inflated,
                expected_delta_size,
                &mut insert[..insert_size],
                "delta insert range is truncated",
            )?;
            len = checked_delta_result_len(len, insert_size, result_size as u64)?;
            hasher.update(&insert[..insert_size]);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta instruction opcode 0 is reserved",
            ));
        }
        if len > max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta result exceeds configured size limit",
            ));
        }
    }

    validate_packed_inflated_size(
        expected_delta_size,
        usize::try_from(inflated).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "packed git object size overflows usize",
            )
        })?,
    )?;
    if len != result_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result size mismatch",
        ));
    }
    cursor.set_position(start as u64 + decoder.total_in());
    Ok((hasher.finalize(), len))
}

#[cfg(test)]
fn read_delta_size(delta: &[u8], cursor: &mut usize) -> io::Result<u64> {
    let mut size = 0_u64;
    let mut shift = 0;
    loop {
        let byte = *delta
            .get(*cursor)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "delta size is truncated"))?;
        *cursor += 1;
        size |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(size);
        }
        shift += 7;
        if shift > 63 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "delta size overflows u64",
            ));
        }
    }
}

fn delta_result_initial_capacity(max_object_bytes: usize, result_size: u64) -> io::Result<usize> {
    if result_size > max_object_bytes as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result exceeds configured size limit",
        ));
    }
    let result_size = usize::try_from(result_size).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "delta result size overflows usize",
        )
    })?;
    Ok(result_size.min(DELTA_RESULT_INITIAL_CAPACITY_LIMIT))
}

#[cfg(test)]
fn read_delta_copy(delta: &[u8], cursor: &mut usize, opcode: u8) -> io::Result<(usize, usize)> {
    let mut offset = 0_usize;
    let mut size = 0_usize;
    for idx in 0..4 {
        if opcode & (1 << idx) != 0 {
            offset |= read_delta_byte(delta, cursor)? << (idx * 8);
        }
    }
    for idx in 0..3 {
        if opcode & (1 << (4 + idx)) != 0 {
            size |= read_delta_byte(delta, cursor)? << (idx * 8);
        }
    }
    if size == 0 {
        size = 0x10000;
    }
    Ok((offset, size))
}

fn read_delta_copy_from_reader<R: Read>(
    reader: &mut R,
    inflated: &mut u64,
    expected_size: u64,
    opcode: u8,
) -> io::Result<(usize, usize)> {
    let mut offset = 0_usize;
    let mut size = 0_usize;
    let mut instruction = [0_u8; 7];
    let mut len = 0_usize;
    for idx in 0..4 {
        if opcode & (1 << idx) != 0 {
            len += 1;
        }
    }
    for idx in 0..3 {
        if opcode & (1 << (4 + idx)) != 0 {
            len += 1;
        }
    }
    read_counted_delta_bytes(
        reader,
        inflated,
        expected_size,
        &mut instruction[..len],
        "delta instruction is truncated",
    )?;
    let mut bytes = instruction[..len].iter().copied();
    for idx in 0..4 {
        if opcode & (1 << idx) != 0 {
            let byte = bytes.next().expect("delta copy offset byte was read");
            offset |= usize::from(byte) << (idx * 8);
        }
    }
    for idx in 0..3 {
        if opcode & (1 << (4 + idx)) != 0 {
            let byte = bytes.next().expect("delta copy size byte was read");
            size |= usize::from(byte) << (idx * 8);
        }
    }
    if size == 0 {
        size = 0x10000;
    }
    Ok((offset, size))
}

fn read_counted_delta_bytes<R: Read>(
    reader: &mut R,
    inflated: &mut u64,
    expected_size: u64,
    bytes: &mut [u8],
    truncated_message: &'static str,
) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let next = inflated
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "delta size overflows u64"))?;
    if next > expected_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object inflated size mismatch",
        ));
    }
    reader
        .read_exact(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, truncated_message))?;
    *inflated = next;
    Ok(())
}

fn read_counted_delta_byte<R: Read>(
    reader: &mut R,
    inflated: &mut u64,
    expected_size: u64,
) -> io::Result<Option<u8>> {
    let mut byte = [0_u8; 1];
    if reader.read(&mut byte)? == 0 {
        return Ok(None);
    }
    *inflated = inflated
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "delta size overflows u64"))?;
    if *inflated > expected_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "packed object inflated size mismatch",
        ));
    }
    Ok(Some(byte[0]))
}

#[cfg(test)]
fn read_delta_byte(delta: &[u8], cursor: &mut usize) -> io::Result<usize> {
    let byte = *delta.get(*cursor).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "delta instruction is truncated")
    })?;
    *cursor += 1;
    Ok(byte as usize)
}

#[cfg(test)]
fn build_replacement_delta_if_smaller(base: &[u8], replacement: &[u8]) -> Option<Vec<u8>> {
    let plan = replacement_delta_plan_if_better(base, replacement, replacement.len())?;
    Some(build_replacement_delta(base, replacement, plan))
}

fn replacement_delta_plan_if_better(
    base: &[u8],
    replacement: &[u8],
    max_delta_len: usize,
) -> Option<ReplacementDeltaPlan> {
    let prefix = common_prefix_len(base, replacement);
    let suffix = common_suffix_len(&base[prefix..], &replacement[prefix..]);
    let insert_end = replacement.len() - suffix;
    let delta_len = replacement_delta_encoded_len(base.len(), replacement.len(), prefix, suffix);
    if delta_len >= max_delta_len {
        return None;
    }
    Some(ReplacementDeltaPlan {
        prefix,
        suffix,
        insert_end,
        delta_len,
    })
}

fn build_replacement_delta(base: &[u8], replacement: &[u8], plan: ReplacementDeltaPlan) -> Vec<u8> {
    let mut delta = Vec::with_capacity(replacement_delta_initial_capacity(plan.delta_len));
    write_delta_size(&mut delta, base.len() as u64);
    write_delta_size(&mut delta, replacement.len() as u64);
    if plan.prefix > 0 {
        write_delta_copy(&mut delta, 0, plan.prefix);
    }
    write_delta_insert(&mut delta, &replacement[plan.prefix..plan.insert_end]);
    if plan.suffix > 0 {
        write_delta_copy(&mut delta, base.len() - plan.suffix, plan.suffix);
    }
    delta
}

fn replacement_delta_initial_capacity(delta_len: usize) -> usize {
    delta_len.min(REPLACEMENT_DELTA_INITIAL_CAPACITY_LIMIT)
}

fn replacement_delta_encoded_len(
    base_len: usize,
    replacement_len: usize,
    prefix: usize,
    suffix: usize,
) -> usize {
    let insert_len = replacement_len - suffix - prefix;
    delta_size_len(base_len as u64)
        + delta_size_len(replacement_len as u64)
        + delta_copy_len(0, prefix)
        + delta_insert_len(insert_len)
        + delta_copy_len(base_len - suffix, suffix)
}

fn delta_size_len(mut size: u64) -> usize {
    let mut len = 1;
    while size >= 0x80 {
        size >>= 7;
        len += 1;
    }
    len
}

fn delta_copy_len(mut offset: usize, mut size: usize) -> usize {
    let mut len = 0;
    while size > 0 {
        let chunk_size = size.min(0x10000);
        len += 1;
        let chunk_offset = offset;
        for idx in 0..4 {
            len += usize::from(((chunk_offset >> (idx * 8)) & 0xff) != 0);
        }
        if chunk_size != 0x10000 {
            for idx in 0..3 {
                len += usize::from(((chunk_size >> (idx * 8)) & 0xff) != 0);
            }
        }
        offset += chunk_size;
        size -= chunk_size;
    }
    len
}

fn delta_insert_len(insert_len: usize) -> usize {
    if insert_len == 0 {
        0
    } else {
        insert_len + insert_len.div_ceil(127)
    }
}

fn common_prefix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

fn common_suffix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .rev()
        .zip(right.iter().rev())
        .take_while(|(left, right)| left == right)
        .count()
}

fn write_delta_size(out: &mut Vec<u8>, mut size: u64) {
    loop {
        let mut byte = (size & 0x7f) as u8;
        size >>= 7;
        if size != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if size == 0 {
            break;
        }
    }
}

fn write_delta_copy(out: &mut Vec<u8>, mut offset: usize, mut size: usize) {
    while size > 0 {
        let chunk_size = size.min(0x10000);
        let mut opcode = 0x80_u8;
        let mut params = [0_u8; 7];
        let mut params_len = 0;
        let chunk_offset = offset;
        for idx in 0..4 {
            let byte = ((chunk_offset >> (idx * 8)) & 0xff) as u8;
            if byte != 0 {
                opcode |= 1 << idx;
                params[params_len] = byte;
                params_len += 1;
            }
        }
        if chunk_size != 0x10000 {
            for idx in 0..3 {
                let byte = ((chunk_size >> (idx * 8)) & 0xff) as u8;
                if byte != 0 {
                    opcode |= 1 << (4 + idx);
                    params[params_len] = byte;
                    params_len += 1;
                }
            }
        }
        out.push(opcode);
        out.extend_from_slice(&params[..params_len]);
        offset += chunk_size;
        size -= chunk_size;
    }
}

fn write_delta_insert(out: &mut Vec<u8>, mut content: &[u8]) {
    while !content.is_empty() {
        let chunk_size = content.len().min(127);
        out.push(chunk_size as u8);
        out.extend_from_slice(&content[..chunk_size]);
        content = &content[chunk_size..];
    }
}

fn read_byte(reader: &mut impl Read) -> io::Result<u8> {
    let mut byte = [0_u8; 1];
    reader.read_exact(&mut byte)?;
    Ok(byte[0])
}

fn read_u32(bytes: &[u8], offset: usize) -> io::Result<u32> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "u32 is truncated"))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> io::Result<u64> {
    let slice = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "u64 is truncated"))?;
    Ok(u64::from_be_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::{InMemoryObjectStore, LooseObjectStore};

    #[test]
    fn reads_non_delta_commit_from_stock_pack() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("README.md"), b"packed\n").expect("write readme");
        git_env(&repo, ["add", "README.md"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let head = git(&repo, ["rev-parse", "HEAD"]);
        git_env(&repo, ["repack", "-ad"]);

        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &head).expect("head id");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let object = store.read_object(&id).expect("read packed commit");

        assert_eq!(object.kind, GitObjectKind::Commit);
        assert!(object.content.starts_with(b"tree "));
        assert!(object.content.ends_with(b"packed\n"));
    }

    #[test]
    fn reads_delta_blob_from_stock_pack() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        let base = format!("{}\nbase\n", "same line\n".repeat(2_000));
        std::fs::write(repo.path().join("delta.txt"), base).expect("write base");
        git_env(&repo, ["add", "delta.txt"]);
        git_env(&repo, ["commit", "-m", "base"]);
        let changed = format!("{}\nchanged\n", "same line\n".repeat(2_000));
        std::fs::write(repo.path().join("delta.txt"), changed).expect("write changed");
        git_env(&repo, ["add", "delta.txt"]);
        git_env(&repo, ["commit", "-m", "changed"]);
        git_env(&repo, ["repack", "-ad", "--depth=50", "--window=50"]);

        let idx_path = first_idx_path(&repo);
        let delta_blob = first_delta_blob(&repo, &idx_path);
        let expected = git_raw(&repo, ["cat-file", "-p", &delta_blob]);
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &delta_blob).expect("delta blob id");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let object = store.read_object(&id).expect("read delta blob");

        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, expected);
    }

    #[test]
    fn reads_base_packed_blob_prefix_without_full_content() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("blob.bin"), b"prefix-content-tail").expect("write blob");
        git_env(&repo, ["add", "blob.bin"]);
        git_env(&repo, ["commit", "-m", "blob"]);
        let blob = git(&repo, ["rev-parse", "HEAD:blob.bin"]);
        git_env(&repo, ["repack", "-ad", "--depth=0"]);

        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &blob).expect("blob id");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        assert_eq!(
            store
                .read_blob_prefix(&id, 6)
                .expect("read packed blob prefix")
                .as_deref(),
            Some(b"prefix".as_slice())
        );
    }

    #[test]
    fn packed_blob_size_hint_reads_delta_result_size() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        let base = format!("{}\nbase\n", "same line\n".repeat(2_000));
        std::fs::write(repo.path().join("delta.txt"), base).expect("write base");
        git_env(&repo, ["add", "delta.txt"]);
        git_env(&repo, ["commit", "-m", "base"]);
        let changed = format!("{}\nchanged\n", "same line\n".repeat(2_000));
        std::fs::write(repo.path().join("delta.txt"), &changed).expect("write changed");
        git_env(&repo, ["add", "delta.txt"]);
        git_env(&repo, ["commit", "-m", "changed"]);
        git_env(&repo, ["repack", "-ad", "--depth=50", "--window=50"]);

        let idx_path = first_idx_path(&repo);
        let delta_blob = first_delta_blob(&repo, &idx_path);
        let expected_size = git(&repo, ["cat-file", "-s", &delta_blob])
            .parse::<usize>()
            .expect("git cat-file size");
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &delta_blob).expect("delta blob id");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        assert_eq!(
            store.blob_size_hint(&id).expect("blob size hint"),
            Some(expected_size)
        );
    }

    #[test]
    fn delta_size_hint_reads_zlib_delta_header() {
        let base = b"base content";
        let replacement = b"base content with change";
        let delta = replacement_delta(base, replacement);
        let mut encoded = Vec::new();
        append_zlib(&mut encoded, &delta);

        let (base_size, result_size) = read_zlib_delta_base_and_result_size(
            std::io::Cursor::new(encoded),
            512 * 1024 * 1024,
            delta.len() as u64,
        )
        .expect("delta size hint");

        assert_eq!(base_size, base.len() as u64);
        assert_eq!(result_size, replacement.len() as u64);
    }

    #[test]
    fn decodes_pack_index_like_stock_show_index() {
        let repo = git_init();
        git_env(&repo, ["commit", "--allow-empty", "-m", "initial"]);
        git_env(&repo, ["repack", "-adq"]);
        let idx_path = first_idx_path(&repo);
        let idx = std::fs::read(&idx_path).expect("read idx");
        let entries = decode_pack_index(GitHashAlgorithm::Sha1, idx).expect("decode idx");
        let actual = entries
            .iter()
            .map(|entry| {
                format!(
                    "{} {} ({:08x})",
                    entry.offset,
                    entry.object_id.to_hex(),
                    entry.crc32
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let expected = git_with_stdin(
            &repo,
            ["show-index"],
            &std::fs::read(idx_path).expect("read idx"),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn pack_index_object_count_reads_fanout_without_decoding_ids() {
        let repo = git_init();
        git_env(&repo, ["commit", "--allow-empty", "-m", "initial"]);
        std::fs::write(repo.path().join("a.txt"), b"one").expect("write a");
        git_env(&repo, ["add", "a.txt"]);
        git_env(&repo, ["commit", "-m", "one"]);
        git_env(&repo, ["repack", "-adq"]);
        let idx_path = first_idx_path(&repo);
        let ids =
            decode_pack_index_object_ids_from_path(GitHashAlgorithm::Sha1, &idx_path).unwrap();

        assert_eq!(pack_index_object_count(&idx_path).unwrap(), ids.len());
        assert_eq!(
            pack_index_object_count(&idx_path.with_extension("pack")).unwrap(),
            ids.len()
        );
    }

    #[test]
    fn index_pack_file_reads_existing_temp_pack_path_with_extra_suffix() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let pack =
            encode_pack_from_store(&source, GitHashAlgorithm::Sha1, &[first]).expect("encode pack");
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("index-pack-stdin.pack.tmp-test");
        fs::write(&path, &pack).expect("write temp pack");

        let indexed =
            index_pack_file_with_version(GitHashAlgorithm::Sha1, &path, PackIndexVersion::V1)
                .expect("index temp pack path");

        assert_eq!(indexed.objects, 1);
    }

    #[test]
    fn write_undeltified_pack_stream_matches_encoded_pack() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let ids = [first, second];
        let expected =
            encode_pack_from_store(&source, GitHashAlgorithm::Sha1, &ids).expect("encode pack");
        let mut actual = Vec::new();

        write_undeltified_pack_from_store(&source, GitHashAlgorithm::Sha1, &ids, &mut actual)
            .expect("write pack");

        assert_eq!(actual, expected);
        index_pack_bytes(GitHashAlgorithm::Sha1, &actual).expect("index written pack");
    }

    #[test]
    fn packed_store_reuses_base_pack_entry_for_undeltified_pack() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"packed content\n").expect("write file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let blob = git(&repo, ["rev-parse", "HEAD:packed.txt"]);
        git_env(&repo, ["repack", "-ad", "--depth=0"]);
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &blob).expect("blob id");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let mut pack = Vec::new();
        let pack_id = {
            let mut writer = PackHashWriter::new(&mut pack, GitHashAlgorithm::Sha1);
            writer.write_all(PACK_MAGIC).expect("write pack magic");
            writer
                .write_all(&PACK_VERSION_2.to_be_bytes())
                .expect("write pack version");
            writer.write_all(&1_u32.to_be_bytes()).expect("write count");
            assert!(
                store
                    .try_write_reusable_pack_object_inner(&id, &mut writer)
                    .expect("reuse packed base object")
            );
            writer.finalize()
        };
        pack.extend_from_slice(pack_id.as_bytes());

        let objects =
            decode_pack_objects(GitHashAlgorithm::Sha1, &pack).expect("decode reused pack");
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].id, id);
        assert_eq!(objects[0].content, b"packed content\n");
        index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index reused pack");
    }

    #[test]
    fn packed_store_reuses_entire_pack_when_ids_match_single_pack() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("first.txt"), b"first\n").expect("write first");
        git_env(&repo, ["add", "first.txt"]);
        git_env(&repo, ["commit", "-m", "first"]);
        std::fs::write(repo.path().join("second.txt"), b"second\n").expect("write second");
        git_env(&repo, ["add", "second.txt"]);
        git_env(&repo, ["commit", "-m", "second"]);
        git_env(&repo, ["repack", "-ad"]);
        let pack_dir = repo.path().join(".git/objects/pack");
        let pack_path = fs::read_dir(&pack_dir)
            .expect("read pack dir")
            .map(|entry| entry.expect("pack entry").path())
            .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("pack"))
            .expect("pack path");
        let expected = fs::read(&pack_path).expect("read source pack");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let mut ids = Vec::new();
        store
            .for_each_object_id(&mut |id| {
                ids.push(id.clone());
                Ok(())
            })
            .expect("collect ids");
        let mut actual = Vec::new();

        let reused = <PackedObjectStore as GitObjectStore>::try_write_reusable_pack(
            &store,
            GitHashAlgorithm::Sha1,
            &ids,
            &mut actual,
        )
        .expect("reuse whole pack");

        assert!(reused.is_some());
        assert_eq!(actual, expected);
        index_pack_bytes(GitHashAlgorithm::Sha1, &actual).expect("index reused whole pack");
    }

    #[test]
    fn packed_store_reuses_matching_pack_from_multi_pack_store() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("first.txt"), b"first\n").expect("write first");
        git_env(&repo, ["add", "first.txt"]);
        git_env(&repo, ["commit", "-m", "first"]);
        git_env(&repo, ["repack", "-ad"]);
        std::fs::write(repo.path().join("second.txt"), b"second\n").expect("write second");
        git_env(&repo, ["add", "second.txt"]);
        git_env(&repo, ["commit", "-m", "second"]);
        let pack_base = repo.path().join(".git/objects/pack/pack-latest");
        let pack_base_arg = pack_base.to_str().expect("pack base path");
        git_with_stdin(&repo, ["pack-objects", "--revs", pack_base_arg], b"HEAD\n");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let mut ids = Vec::new();
        store
            .for_each_object_id(&mut |id| {
                ids.push(id.clone());
                Ok(())
            })
            .expect("collect ids");
        let expected_pack = fs::read_dir(repo.path().join(".git/objects/pack"))
            .expect("read pack dir")
            .map(|entry| entry.expect("pack entry").path())
            .find(|path| {
                path.extension().and_then(|ext| ext.to_str()) == Some("idx")
                    && pack_index_object_count(path).expect("pack index count") == ids.len()
            })
            .expect("matching pack index")
            .with_extension("pack");
        let expected = fs::read(expected_pack).expect("read matching pack");
        let mut actual = Vec::new();

        let reused = <PackedObjectStore as GitObjectStore>::try_write_reusable_pack(
            &store,
            GitHashAlgorithm::Sha1,
            &ids,
            &mut actual,
        )
        .expect("reuse matching pack");

        assert!(reused.is_some());
        assert_eq!(actual, expected);
        index_pack_bytes(GitHashAlgorithm::Sha1, &actual).expect("index reused multi-pack");
    }

    #[test]
    fn packed_store_reuses_full_pack_parts_when_no_single_pack_matches() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        for idx in 1..=3 {
            std::fs::write(
                repo.path().join(format!("file-{idx}.txt")),
                format!("content {idx}\n"),
            )
            .expect("write file");
            git_env(&repo, ["add", "."]);
            git_env(&repo, ["commit", "-m", &format!("commit {idx}")]);
        }
        let revs = git(
            &repo,
            ["rev-list", "--objects", "--no-object-names", "--all"],
        );
        let object_ids = revs.lines().collect::<Vec<_>>();
        assert!(object_ids.len() > 1);
        let chunk_size = object_ids.len().div_ceil(2);
        for (idx, chunk) in object_ids.chunks(chunk_size).enumerate() {
            let pack_base = repo
                .path()
                .join(format!(".git/objects/pack/pack-part-{}", idx + 1));
            let pack_base_arg = pack_base.to_str().expect("pack base path");
            let pack_input = format!("{}\n", chunk.join("\n"));
            git_with_stdin(
                &repo,
                ["pack-objects", pack_base_arg],
                pack_input.as_bytes(),
            );
        }
        git_env(&repo, ["prune-packed", "-q"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let mut ids = Vec::new();
        store
            .for_each_object_id(&mut |id| {
                ids.push(id.clone());
                Ok(())
            })
            .expect("collect ids");
        let expected_ids = ids.iter().cloned().collect::<HashSet<_>>();
        let has_single_matching_pack = fs::read_dir(repo.path().join(".git/objects/pack"))
            .expect("read pack dir")
            .map(|entry| entry.expect("pack entry").path())
            .any(|path| {
                path.extension().and_then(|ext| ext.to_str()) == Some("idx")
                    && PackIndex::read(&path, GitHashAlgorithm::Sha1)
                        .expect("read pack index")
                        .object_ids()
                        .into_iter()
                        .collect::<HashSet<_>>()
                        == expected_ids
            });
        let mut actual = Vec::new();

        let reused = <PackedObjectStore as GitObjectStore>::try_write_reusable_pack(
            &store,
            GitHashAlgorithm::Sha1,
            &ids,
            &mut actual,
        )
        .expect("reuse full pack parts");
        let indexed = index_pack_bytes(GitHashAlgorithm::Sha1, &actual).expect("index reused pack");

        assert!(!has_single_matching_pack);
        assert!(reused.is_some());
        assert_eq!(indexed.objects, ids.len());
    }

    #[test]
    fn packed_store_streams_base_blob_without_materializing_object() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"streamed packed content\n")
            .expect("write file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let blob = git(&repo, ["rev-parse", "HEAD:packed.txt"]);
        git_env(&repo, ["repack", "-ad", "--depth=0"]);
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &blob).expect("blob id");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let mut streamed = Vec::new();

        assert!(
            store
                .try_write_blob_inner(&id, &mut streamed)
                .expect("stream packed blob")
        );

        assert_eq!(streamed, b"streamed packed content\n");
        assert_eq!(
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &streamed),
            id
        );
    }

    #[test]
    fn packed_store_streams_base_blob_to_path_for_large_checkout() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(
            repo.path().join("packed.txt"),
            b"path streamed packed content\n",
        )
        .expect("write file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let blob = git(&repo, ["rev-parse", "HEAD:packed.txt"]);
        git_env(&repo, ["repack", "-ad", "--depth=0"]);
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &blob).expect("blob id");
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let out_dir = TempDir::new().expect("output dir");
        let out_path = out_dir.path().join("packed.txt");

        assert!(
            store
                .try_write_blob_to_path_inner(&id, 1, &out_path)
                .expect("stream packed blob to path")
        );

        assert_eq!(
            std::fs::read(&out_path).expect("read streamed path"),
            b"path streamed packed content\n"
        );
        assert!(
            !store
                .try_write_blob_to_path_inner(&id, usize::MAX, &out_dir.path().join("small.txt"))
                .expect("below min size")
        );
    }

    #[test]
    fn packed_base_stream_rejects_beyond_declared_size_before_write() {
        let mut compressed = Vec::new();
        append_zlib(&mut compressed, b"actual");
        let mut file = tempfile::tempfile().expect("temp compressed object");
        file.write_all(&compressed)
            .expect("write compressed object");
        file.seek(SeekFrom::Start(0))
            .expect("rewind compressed object");
        let expected = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"ac");
        let mut out = Vec::new();

        let error = verify_or_write_packed_base_object_content(
            &mut file,
            compressed.len() as u64,
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            2,
            &expected,
            Some(&mut out),
        )
        .expect_err("declared size mismatch");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "packed object inflated beyond declared size"
        );
        assert!(out.is_empty());
    }

    #[test]
    fn pack_object_count_rejects_values_over_pack_v2_limit() {
        assert_eq!(pack_object_count_u32(u32::MAX as usize).unwrap(), u32::MAX);
        let error = pack_object_count_u32(u32::MAX as usize + 1).expect_err("count overflow");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "pack object count exceeds pack v2 limit");
    }

    #[test]
    fn write_delta_pack_stream_matches_encoded_pack() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let mut first_content = vec![b'a'; 4096];
        first_content.extend_from_slice(b"\nbase\n");
        let mut second_content = first_content.clone();
        second_content[2048] = b'b';
        second_content.extend_from_slice(b"tail\n");
        let first = source
            .write_object(GitObjectKind::Blob, &first_content)
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, &second_content)
            .expect("write second blob");
        let ids = [first, second];
        let options = PackEncodeOptions::delta(10, 50);
        let expected =
            encode_pack_from_store_with_options(&source, GitHashAlgorithm::Sha1, &ids, options)
                .expect("encode pack");
        let mut actual = Vec::new();

        write_pack_from_store_with_options(
            &source,
            GitHashAlgorithm::Sha1,
            &ids,
            options,
            &mut actual,
        )
        .expect("write pack");

        assert_eq!(actual, expected);
        let indexed =
            index_pack_bytes(GitHashAlgorithm::Sha1, &actual).expect("index written pack");
        assert_eq!(indexed.objects, ids.len());
    }

    #[test]
    fn zero_window_pack_options_write_undeltified_pack() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let ids = [first, second];
        let options = PackEncodeOptions::delta(0, 50);
        let expected =
            encode_pack_from_store(&source, GitHashAlgorithm::Sha1, &ids).expect("encode pack");
        let mut actual = Vec::new();

        write_pack_from_store_with_options(
            &source,
            GitHashAlgorithm::Sha1,
            &ids,
            options,
            &mut actual,
        )
        .expect("write zero-window pack");

        assert_eq!(actual, expected);
        assert_eq!(pack_delta_window_capacity(ids.len(), options), 0);
    }

    #[test]
    fn delta_candidate_cache_is_limited_to_pack_window() {
        let mut encoded = VecDeque::new();
        let mut content_bytes = 0_usize;
        let options = PackEncodeOptions::delta(2, 50);
        for idx in 0..3 {
            let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &[idx]);
            push_encoded_pack_object(
                &mut encoded,
                &mut content_bytes,
                EncodedPackObject {
                    id,
                    kind: GitObjectKind::Blob,
                    content: vec![idx],
                    offset: idx as u64,
                    depth: 0,
                },
                options,
            );
        }

        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0].content, vec![1]);
        assert_eq!(encoded[1].content, vec![2]);
        assert_eq!(content_bytes, 2);
    }

    #[test]
    fn delta_candidate_cache_initial_capacity_is_bounded() {
        assert_eq!(
            pack_delta_window_capacity(usize::MAX, PackEncodeOptions::delta(usize::MAX, 50)),
            PACK_DELTA_WINDOW_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(
            pack_delta_window_capacity(2, PackEncodeOptions::delta(usize::MAX, 50)),
            2
        );
        assert_eq!(
            pack_delta_window_capacity(2, PackEncodeOptions::delta(0, 50)),
            0
        );
    }

    #[test]
    fn pack_output_initial_capacity_is_bounded() {
        assert_eq!(
            pack_output_initial_capacity(GitHashAlgorithm::Sha1, 0),
            PACK_HEADER_LEN + GitHashAlgorithm::Sha1.digest_len()
        );
        assert_eq!(
            pack_output_initial_capacity(GitHashAlgorithm::Sha256, 2),
            PACK_HEADER_LEN
                + GitHashAlgorithm::Sha256.digest_len()
                + 2 * PACK_OUTPUT_OBJECT_BYTES_HINT
        );
        assert_eq!(
            pack_output_initial_capacity(GitHashAlgorithm::Sha1, usize::MAX),
            PACK_OUTPUT_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn delta_candidate_cache_skips_objects_over_content_budget() {
        let mut encoded = VecDeque::new();
        let mut content_bytes = 0_usize;
        let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"large");

        push_encoded_pack_object(
            &mut encoded,
            &mut content_bytes,
            EncodedPackObject {
                id,
                kind: GitObjectKind::Blob,
                content: vec![0; PACK_DELTA_WINDOW_CONTENT_BUDGET + 1],
                offset: 0,
                depth: 0,
            },
            PackEncodeOptions::delta(10, 50),
        );

        assert!(encoded.is_empty());
        assert_eq!(content_bytes, 0);
    }

    #[test]
    fn delta_candidate_cache_evicts_to_content_budget() {
        let mut encoded = VecDeque::new();
        let mut content_bytes = 0_usize;
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let first_len = PACK_DELTA_WINDOW_CONTENT_BUDGET - 2;

        push_encoded_pack_object(
            &mut encoded,
            &mut content_bytes,
            EncodedPackObject {
                id: first,
                kind: GitObjectKind::Blob,
                content: vec![1; first_len],
                offset: 0,
                depth: 0,
            },
            PackEncodeOptions::delta(10, 50),
        );
        push_encoded_pack_object(
            &mut encoded,
            &mut content_bytes,
            EncodedPackObject {
                id: second,
                kind: GitObjectKind::Blob,
                content: vec![2; 4],
                offset: 1,
                depth: 0,
            },
            PackEncodeOptions::delta(10, 50),
        );

        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].content, vec![2; 4]);
        assert_eq!(content_bytes, 4);
    }

    #[test]
    fn replacement_delta_skips_candidates_that_cannot_shrink() {
        assert!(build_replacement_delta_if_smaller(b"aaaa", b"bbbb").is_none());
    }

    #[test]
    fn replacement_delta_plan_skips_candidates_that_cannot_beat_current_best() {
        let base = b"same prefix: old value, same suffix";
        let replacement = b"same prefix: new value, same suffix";
        let plan =
            replacement_delta_plan_if_better(base, replacement, replacement.len()).expect("plan");

        assert!(
            replacement_delta_plan_if_better(base, replacement, plan.delta_len).is_none(),
            "equal-sized candidates should not replace the current best delta"
        );
        assert!(
            replacement_delta_plan_if_better(base, replacement, plan.delta_len + 1).is_some(),
            "strictly smaller candidates should still be built"
        );
    }

    #[test]
    fn replacement_delta_preallocates_exact_encoded_len() {
        let base = b"same prefix: old value, same suffix";
        let replacement = b"same prefix: new value, same suffix";
        let delta = build_replacement_delta_if_smaller(base, replacement).expect("smaller delta");

        assert_eq!(delta.len(), delta.capacity());
        assert_eq!(
            apply_delta(base, &delta, replacement.len()).expect("apply delta"),
            replacement
        );
    }

    #[test]
    fn zlib_delta_hashing_streams_script_without_materializing_delta() {
        let base = b"base content for zlib streaming hash";
        let replacement = b"base content for zlib streaming hash with a suffix";
        let delta = replacement_delta(base, replacement);
        let mut encoded = Vec::new();
        append_zlib(&mut encoded, &delta);
        let mut cursor = std::io::Cursor::new(encoded.as_slice());

        let (id, len) = apply_zlib_delta_hashing_from_cursor(
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            base,
            &mut cursor,
            delta.len() as u64,
            512 * 1024 * 1024,
        )
        .expect("hash zlib delta");

        assert_eq!(cursor.position(), encoded.len() as u64);
        assert_eq!(len, replacement.len());
        assert_eq!(
            id,
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, replacement)
        );
    }

    #[test]
    fn zlib_delta_apply_streams_script_without_materializing_delta() {
        let base = b"base content for zlib streaming apply";
        let replacement = b"base content for zlib streaming apply with a suffix";
        let delta = replacement_delta(base, replacement);
        let mut encoded = Vec::new();
        append_zlib(&mut encoded, &delta);

        let applied = apply_zlib_delta_from_reader(
            encoded.as_slice(),
            base,
            delta.len() as u64,
            512 * 1024 * 1024,
        )
        .expect("apply zlib delta");

        assert_eq!(applied, replacement);
    }

    #[test]
    fn zlib_delta_apply_from_cursor_advances_packed_cursor() {
        let base = b"base content for cursor apply";
        let replacement = b"base content for cursor apply with a suffix";
        let delta = replacement_delta(base, replacement);
        let mut encoded = Vec::new();
        append_zlib(&mut encoded, &delta);
        encoded.extend_from_slice(b"next");
        let mut cursor = std::io::Cursor::new(encoded.as_slice());

        let applied =
            apply_zlib_delta_from_cursor(base, &mut cursor, delta.len() as u64, 512 * 1024 * 1024)
                .expect("apply zlib delta from cursor");

        assert_eq!(applied, replacement);
        assert_eq!(&cursor.get_ref()[cursor.position() as usize..], b"next");
    }

    #[test]
    fn delta_copy_rejects_result_beyond_declared_size_before_copy() {
        let base = b"abcdef";
        let mut delta = Vec::new();
        write_delta_varint(&mut delta, base.len() as u64);
        write_delta_varint(&mut delta, 2);
        write_delta_copy(&mut delta, 0, base.len());

        let error = apply_delta(base, &delta, 512 * 1024 * 1024)
            .expect_err("delta result should exceed declared size");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "delta result size mismatch");
    }

    #[test]
    fn zlib_delta_hashing_rejects_result_beyond_declared_size_before_hash() {
        let base = b"abcdef";
        let mut delta = Vec::new();
        write_delta_varint(&mut delta, base.len() as u64);
        write_delta_varint(&mut delta, 2);
        write_delta_copy(&mut delta, 0, base.len());
        let mut encoded = Vec::new();
        append_zlib(&mut encoded, &delta);
        let mut cursor = std::io::Cursor::new(encoded.as_slice());

        let error = apply_zlib_delta_hashing_from_cursor(
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            base,
            &mut cursor,
            delta.len() as u64,
            512 * 1024 * 1024,
        )
        .expect_err("delta result should exceed declared size");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "delta result size mismatch");
    }

    #[test]
    fn replacement_delta_initial_capacity_is_bounded() {
        assert_eq!(
            replacement_delta_initial_capacity(REPLACEMENT_DELTA_INITIAL_CAPACITY_LIMIT + 1),
            REPLACEMENT_DELTA_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(replacement_delta_initial_capacity(2), 2);
        assert_eq!(replacement_delta_initial_capacity(0), 0);
    }

    #[test]
    fn decodes_internal_pack_index_v1_layout() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let pack = encode_pack_from_store(
            &source,
            GitHashAlgorithm::Sha1,
            &[first.clone(), second.clone()],
        )
        .expect("encode pack");
        let indexed =
            index_pack_bytes_with_version(GitHashAlgorithm::Sha1, &pack, PackIndexVersion::V1)
                .expect("index v1");
        let entries = decode_pack_index(GitHashAlgorithm::Sha1, indexed.index.clone())
            .expect("decode v1 index");
        let ids = entries
            .iter()
            .map(|entry| entry.object_id.clone())
            .collect::<Vec<_>>();

        assert_eq!(entries.len(), 2);
        assert!(ids.contains(&first));
        assert!(ids.contains(&second));
        assert_eq!(
            pack_index_object_count_from_header(&indexed.index[..8 + 256 * 4]).unwrap(),
            2
        );
    }

    #[test]
    fn encodes_pack_index_v2_large_offsets() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let third = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"third");
        let pack_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"pack");
        let entries = vec![
            PackIndexEntry {
                object_id: first.clone(),
                offset: 12,
                crc32: 0x1234_5678,
            },
            PackIndexEntry {
                object_id: second.clone(),
                offset: 0x8000_0000,
                crc32: 0x8765_4321,
            },
            PackIndexEntry {
                object_id: third.clone(),
                offset: 0x8000_0010,
                crc32: 0x1111_2222,
            },
        ];

        let index = encode_pack_index_with_version(
            GitHashAlgorithm::Sha1,
            &pack_id,
            &entries,
            PackIndexVersion::V2,
        )
        .expect("encode index");
        let decoded =
            decode_pack_index(GitHashAlgorithm::Sha1, index).expect("decode large-offset index");

        let large = decoded
            .iter()
            .find(|entry| entry.object_id == second)
            .expect("large-offset entry");
        assert_eq!(large.offset, 0x8000_0000);
        let next_large = decoded
            .iter()
            .find(|entry| entry.object_id == third)
            .expect("second large-offset entry");
        assert_eq!(next_large.offset, 0x8000_0010);
        assert!(decoded.iter().any(|entry| entry.object_id == first));
    }

    #[test]
    fn streams_pack_index_v2_entries_with_large_offsets() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let pack_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"pack");
        let entries = vec![
            PackIndexEntry {
                object_id: first.clone(),
                offset: 12,
                crc32: 0x1234_5678,
            },
            PackIndexEntry {
                object_id: second.clone(),
                offset: 0x8000_0000,
                crc32: 0x8765_4321,
            },
        ];
        let index = encode_pack_index_with_version(
            GitHashAlgorithm::Sha1,
            &pack_id,
            &entries,
            PackIndexVersion::V2,
        )
        .expect("encode index");
        let mut streamed = Vec::new();

        for_each_pack_index_entry(GitHashAlgorithm::Sha1, index, &mut |entry| {
            streamed.push(entry);
            Ok(())
        })
        .expect("stream index entries");

        assert_eq!(streamed.len(), 2);
        assert!(streamed.iter().any(|entry| entry.object_id == first));
        let large = streamed
            .iter()
            .find(|entry| entry.object_id == second)
            .expect("large offset");
        assert_eq!(large.offset, 0x8000_0000);
        assert_eq!(large.crc32, 0x8765_4321);
    }

    #[test]
    fn sorted_pack_index_positions_skips_sort_for_ordered_entries() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let mut entries = vec![
            PackIndexEntry {
                object_id: first,
                offset: 12,
                crc32: 0,
            },
            PackIndexEntry {
                object_id: second,
                offset: 24,
                crc32: 0,
            },
        ];
        entries.sort_by(|left, right| left.object_id.as_bytes().cmp(right.object_id.as_bytes()));

        assert!(pack_index_entries_are_sorted(&entries));
        let positions = sorted_pack_index_positions(&entries).expect("sorted positions");
        assert!(matches!(positions, PackIndexPositions::Identity(2)));
        assert_eq!(positions.iter().collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn sorted_pack_index_positions_orders_unsorted_entries() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let mut entries = vec![
            PackIndexEntry {
                object_id: first,
                offset: 12,
                crc32: 0,
            },
            PackIndexEntry {
                object_id: second,
                offset: 24,
                crc32: 0,
            },
        ];
        entries.sort_by(|left, right| right.object_id.as_bytes().cmp(left.object_id.as_bytes()));

        assert!(!pack_index_entries_are_sorted(&entries));
        let positions = sorted_pack_index_positions(&entries).expect("sorted positions");
        assert!(matches!(positions, PackIndexPositions::Sorted(_)));
        assert_eq!(positions.iter().collect::<Vec<_>>(), vec![1, 0]);
    }

    #[test]
    fn owned_pack_index_encoder_matches_position_encoder_for_unsorted_entries() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let pack_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"pack");
        let mut entries = vec![
            PackIndexEntry {
                object_id: first,
                offset: 12,
                crc32: 1,
            },
            PackIndexEntry {
                object_id: second,
                offset: 24,
                crc32: 2,
            },
        ];
        entries.sort_by(|left, right| right.object_id.as_bytes().cmp(left.object_id.as_bytes()));

        let expected = encode_pack_index_with_version(
            GitHashAlgorithm::Sha1,
            &pack_id,
            &entries,
            PackIndexVersion::V2,
        )
        .expect("position encoded index");
        let actual = encode_pack_index_from_owned_entries(
            GitHashAlgorithm::Sha1,
            &pack_id,
            entries,
            PackIndexVersion::V2,
        )
        .expect("owned encoded index");

        assert_eq!(actual, expected);
    }

    #[test]
    fn unique_pack_index_id_merge_dedupes_with_heap_order() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let third = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"third");
        let pack_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"pack");
        let indexes = [
            test_pack_index(&pack_id, &[first.clone(), third.clone()]),
            test_pack_index(&pack_id, &[first.clone(), second.clone()]),
            test_pack_index(&pack_id, &[second.clone(), third.clone()]),
        ];
        let mut expected = vec![first, second, third];
        expected.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        expected.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
        let mut actual = Vec::new();

        append_unique_sorted_pack_index_ids(&indexes, &mut actual).expect("append ids");

        assert_eq!(
            count_unique_sorted_pack_index_ids(&indexes).expect("count ids"),
            expected.len()
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn pack_index_object_ids_subset_uses_sorted_name_tables() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let third = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"third");
        let pack_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"pack");
        let subset = test_pack_index(&pack_id, &[first.clone(), third.clone()]);
        let superset = test_pack_index(&pack_id, &[first, second.clone(), third]);
        let missing = test_pack_index(&pack_id, &[second]);

        assert!(subset.object_ids_are_subset_of(&superset));
        assert!(!superset.object_ids_are_subset_of(&subset));
        assert!(!missing.object_ids_are_subset_of(&subset));
    }

    #[test]
    fn pack_index_object_ids_all_stops_after_first_false_predicate() {
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let pack_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"pack");
        let index = test_pack_index(&pack_id, &[first, second]);
        let mut visits = 0;

        let all = index
            .object_ids_all(&mut |_| {
                visits += 1;
                Ok(false)
            })
            .expect("object ids all");

        assert!(!all);
        assert_eq!(visits, 1);
    }

    #[test]
    fn pack_index_merge_heap_initial_capacity_is_bounded() {
        assert_eq!(pack_index_merge_heap_initial_capacity(0), 0);
        assert_eq!(pack_index_merge_heap_initial_capacity(2), 2);
        assert_eq!(
            pack_index_merge_heap_initial_capacity(usize::MAX),
            PACK_INDEX_MERGE_HEAP_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn pack_index_entry_initial_capacity_is_bounded() {
        assert_eq!(pack_index_entry_initial_capacity(0), 0);
        assert_eq!(pack_index_entry_initial_capacity(2), 2);
        assert_eq!(
            pack_index_entry_initial_capacity(usize::MAX),
            PACK_INDEX_ENTRY_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn pack_index_object_id_initial_capacity_is_bounded() {
        assert_eq!(pack_index_object_id_initial_capacity(0), 0);
        assert_eq!(pack_index_object_id_initial_capacity(2), 2);
        assert_eq!(
            pack_index_object_id_initial_capacity(usize::MAX),
            PACK_INDEX_OBJECT_ID_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn pack_object_id_reserve_does_not_grow_when_spare_capacity_is_enough() {
        let mut ids = Vec::with_capacity(4);
        ids.push(hash_object(
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            b"first",
        ));
        let capacity = ids.capacity();

        reserve_pack_object_ids_spare(&mut ids, 2);

        assert_eq!(ids.capacity(), capacity);
    }

    #[test]
    fn pack_object_id_reserve_grows_when_spare_capacity_is_insufficient() {
        let mut ids = Vec::with_capacity(1);
        ids.push(hash_object(
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            b"first",
        ));

        reserve_pack_object_ids_spare(&mut ids, 5);

        assert!(ids.capacity().saturating_sub(ids.len()) >= 5);
    }

    #[test]
    fn pack_index_list_initial_capacity_is_bounded() {
        assert_eq!(pack_index_list_initial_capacity(0), 0);
        assert_eq!(pack_index_list_initial_capacity(2), 2);
        assert_eq!(
            pack_index_list_initial_capacity(usize::MAX),
            PACK_INDEX_LIST_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn pack_index_min_len_matches_v1_and_v2_layouts() {
        assert_eq!(
            pack_index_min_len(PackIndexVersion::V1, 3, 20, 256 * 4).expect("v1 min len"),
            256 * 4 + 3 * (4 + 20) + 2 * 20
        );
        assert_eq!(
            pack_index_min_len(PackIndexVersion::V2, 3, 20, 8 + 256 * 4).expect("v2 min len"),
            8 + 256 * 4 + 3 * (20 + 4 + 4) + 2 * 20
        );
    }

    #[test]
    fn pack_index_min_len_detects_overflow() {
        assert_eq!(
            pack_index_min_len(PackIndexVersion::V2, usize::MAX, 20, 8 + 256 * 4)
                .expect_err("overflow")
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            pack_index_min_len(PackIndexVersion::V1, 1, 20, usize::MAX)
                .expect_err("overflow")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn pack_index_layout_precomputes_v2_table_offsets() {
        let layout = pack_index_layout(PackIndexVersion::V2, 3, 20).expect("layout");

        assert_eq!(layout.names_start, 8 + 256 * 4);
        assert_eq!(layout.v2_crc_start, 8 + 256 * 4 + 3 * 20);
        assert_eq!(layout.v2_offsets_start, 8 + 256 * 4 + 3 * 20 + 3 * 4);
        assert_eq!(
            layout.v2_large_offsets_start,
            8 + 256 * 4 + 3 * 20 + 3 * 4 + 3 * 4
        );
    }

    #[test]
    fn pack_index_layout_detects_overflow() {
        assert_eq!(
            pack_index_layout(PackIndexVersion::V2, usize::MAX, 20)
                .expect_err("overflow")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    fn test_pack_index(pack_id: &ObjectId, ids: &[ObjectId]) -> Arc<PackIndex> {
        let entries = ids
            .iter()
            .enumerate()
            .map(|(index, object_id)| PackIndexEntry {
                object_id: object_id.clone(),
                offset: 12 + index as u64,
                crc32: index as u32,
            })
            .collect::<Vec<_>>();
        let index = encode_pack_index_with_version(
            GitHashAlgorithm::Sha1,
            pack_id,
            &entries,
            PackIndexVersion::V2,
        )
        .expect("encode index");
        Arc::new(PackIndex::read_bytes(index, GitHashAlgorithm::Sha1).expect("read index"))
    }

    #[test]
    fn pack_reverse_index_capacity_detects_overflow() {
        assert_eq!(
            pack_reverse_index_capacity(GitHashAlgorithm::Sha1, 2).expect("capacity"),
            12 + 2 * 4 + GitHashAlgorithm::Sha1.digest_len() * 2
        );
        assert!(pack_reverse_index_capacity(GitHashAlgorithm::Sha1, usize::MAX).is_err());
    }

    #[test]
    fn encodes_reverse_pack_index_like_stock_git() {
        let source = git_init();
        git_env(&source, ["commit", "--allow-empty", "-m", "initial"]);
        std::fs::write(source.path().join("a.txt"), b"one").expect("write a");
        git_env(&source, ["add", "a.txt"]);
        git_env(&source, ["commit", "-m", "one"]);
        let pack = git_raw_with_stdin(&source, ["pack-objects", "--stdout", "--revs"], b"HEAD\n");
        let indexed = index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index pack");

        let target = git_init();
        let output = git_with_stdin(&target, ["index-pack", "--stdin", "--rev-index"], &pack);
        assert_eq!(output, format!("pack\t{}", indexed.pack_id.to_hex()));
        let stock_rev = std::fs::read(
            target
                .path()
                .join(".git/objects/pack")
                .join(format!("pack-{}.rev", indexed.pack_id.to_hex())),
        )
        .expect("read stock rev index");

        assert_eq!(indexed.reverse_index, stock_rev);
    }

    #[test]
    fn reverse_pack_index_identity_positions_match_sorted_positions() {
        let pack_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"pack");
        let first = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let second = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"second");
        let entries = vec![
            PackIndexEntry {
                object_id: first,
                offset: 12,
                crc32: 1,
            },
            PackIndexEntry {
                object_id: second,
                offset: 24,
                crc32: 2,
            },
        ];

        let identity = encode_pack_reverse_index_from_positions(
            GitHashAlgorithm::Sha1,
            &pack_id,
            &entries,
            &PackIndexPositions::Identity(entries.len()),
        )
        .expect("encode identity reverse index");
        let sorted = encode_pack_reverse_index_from_positions(
            GitHashAlgorithm::Sha1,
            &pack_id,
            &entries,
            &PackIndexPositions::Sorted(vec![0, 1]),
        )
        .expect("encode sorted reverse index");

        assert_eq!(identity, sorted);
    }

    #[test]
    fn pack_roundtrip_works_with_in_memory_object_store() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let pack = encode_pack_from_store(
            &source,
            GitHashAlgorithm::Sha1,
            &[first.clone(), second.clone()],
        )
        .expect("encode pack");
        let indexed = index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index pack");
        let target = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);

        let stats =
            unpack_pack_to_loose(&target, GitHashAlgorithm::Sha1, &pack).expect("unpack pack");

        assert_eq!(indexed.objects, 2);
        assert_eq!(stats.objects, 2);
        assert_eq!(
            target.read_object(&first).expect("read first").content,
            b"first\n"
        );
        assert_eq!(
            target.read_object(&second).expect("read second").content,
            b"second\n"
        );
    }

    #[test]
    fn pack_file_object_iteration_and_unpack_use_file_mapping() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let pack = encode_pack_from_store(
            &source,
            GitHashAlgorithm::Sha1,
            &[first.clone(), second.clone()],
        )
        .expect("encode pack");
        let tmp = TempDir::new().expect("temp dir");
        let pack_path = tmp.path().join("objects.pack");
        fs::write(&pack_path, &pack).expect("write pack file");
        let mut visited = Vec::new();

        for_each_pack_object_file(GitHashAlgorithm::Sha1, &pack_path, |id, kind, content| {
            visited.push((id.clone(), kind, content.to_vec()));
            Ok(())
        })
        .expect("iterate pack file");
        let target = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let stats = unpack_pack_file_to_loose(&target, GitHashAlgorithm::Sha1, &pack_path)
            .expect("unpack pack file");

        visited.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
        assert_eq!(visited.len(), 2);
        assert_eq!(stats.objects, 2);
        assert_eq!(
            target.read_object(&first).expect("read first").content,
            b"first\n"
        );
        assert_eq!(
            target.read_object(&second).expect("read second").content,
            b"second\n"
        );
    }

    #[test]
    fn index_pack_bytes_index_only_matches_full_index_for_base_only_pack() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let pack = encode_pack_from_store(
            &source,
            GitHashAlgorithm::Sha1,
            &[first.clone(), second.clone()],
        )
        .expect("encode pack");
        let indexed = index_pack_bytes_index_only(GitHashAlgorithm::Sha1, &pack)
            .expect("index pack bytes index only");
        let indexed_full =
            index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index pack bytes");

        assert_eq!(indexed.pack_id, indexed_full.pack_id);
        assert_eq!(indexed.index, indexed_full.index);
        validate_pack_reverse_index(
            GitHashAlgorithm::Sha1,
            &indexed_full.reverse_index,
            indexed_full.objects,
        )
        .expect("validate full reverse index");
    }

    #[test]
    fn pack_parser_collects_object_sizes_only_for_verbose_verify() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let pack = encode_pack_from_store(
            &source,
            GitHashAlgorithm::Sha1,
            &[first.clone(), second.clone()],
        )
        .expect("encode pack");

        let index_only =
            parse_pack_entries(GitHashAlgorithm::Sha1, &pack, None, false, false, None)
                .expect("parse index-only pack");
        let verbose = parse_pack_entries(GitHashAlgorithm::Sha1, &pack, None, true, false, None)
            .expect("parse verbose pack");

        assert_eq!(index_only.entries.len(), 2);
        assert!(index_only.object_metadata.is_empty());
        assert!(index_only.objects.is_empty());
        assert_eq!(
            verbose
                .object_metadata
                .iter()
                .map(|metadata| metadata.size())
                .collect::<Vec<_>>(),
            vec![b"first\n".len() as u32, b"second\n".len() as u32]
        );
        assert_eq!(
            verbose
                .object_metadata
                .iter()
                .map(|metadata| metadata.kind())
                .collect::<Vec<_>>(),
            vec![GitObjectKind::Blob, GitObjectKind::Blob]
        );
        assert!(verbose.objects.is_empty());
    }

    #[test]
    fn verify_pack_entries_use_compact_object_kind_metadata() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let pack = encode_pack_from_store(
            &source,
            GitHashAlgorithm::Sha1,
            &[first.clone(), second.clone()],
        )
        .expect("encode pack");

        let verified = verify_pack_bytes_with_version(
            GitHashAlgorithm::Sha1,
            &pack,
            PackIndexVersion::V2,
            true,
        )
        .expect("verify pack entries");

        assert_eq!(verified.objects, 2);
        assert_eq!(verified.entries.len(), 2);
        validate_pack_index_bytes(GitHashAlgorithm::Sha1, &verified.index)
            .expect("verified pack index");
        assert_eq!(
            verified.entries[0].packed_size,
            verified.entries[1].offset as usize - verified.entries[0].offset as usize
        );
        assert!(verified.entries.iter().all(|entry| {
            entry.kind == GitObjectKind::Blob
                && (entry.object_size == b"first\n".len() as u64
                    || entry.object_size == b"second\n".len() as u64)
        }));
    }

    #[test]
    fn verify_pack_file_matches_index_without_regenerating_index_bytes() {
        let source = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = source
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second = source
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let pack = encode_pack_from_store(
            &source,
            GitHashAlgorithm::Sha1,
            &[first.clone(), second.clone()],
        )
        .expect("encode pack");
        let indexed =
            index_pack_bytes_index_only(GitHashAlgorithm::Sha1, &pack).expect("index pack bytes");
        let temp = TempDir::new().expect("temp dir");
        let pack_path = temp.path().join("sample.pack");
        let idx_path = temp.path().join("sample.idx");
        fs::write(&pack_path, &pack).expect("write pack");
        fs::write(&idx_path, &indexed.index).expect("write index");

        let verified =
            verify_pack_file_matches_index(GitHashAlgorithm::Sha1, &pack_path, &idx_path, true)
                .expect("verify pack against index");

        assert_eq!(verified.objects, 2);
        assert_eq!(verified.entries.len(), 2);
        assert!(verified.entries.iter().all(|entry| {
            entry.kind == GitObjectKind::Blob
                && (entry.object_size == b"first\n".len() as u64
                    || entry.object_size == b"second\n".len() as u64)
        }));
    }

    #[test]
    fn verify_pack_entries_reserve_exact_output_capacity() {
        let entries = vec![
            PackIndexEntry {
                object_id: ObjectId::new(GitHashAlgorithm::Sha1, &[1_u8; 20]),
                offset: 12,
                crc32: 1,
            },
            PackIndexEntry {
                object_id: ObjectId::new(GitHashAlgorithm::Sha1, &[2_u8; 20]),
                offset: 24,
                crc32: 2,
            },
        ];
        let metadata = vec![
            PackObjectMetadata::new(GitObjectKind::Blob, 1).expect("blob metadata"),
            PackObjectMetadata::new(GitObjectKind::Tree, 2).expect("tree metadata"),
        ];

        let verify_entries = pack_verify_entries_from_metadata(&entries, &metadata, 40);

        assert_eq!(verify_entries.len(), entries.len());
        assert_eq!(verify_entries.capacity(), entries.len());
        assert_eq!(verify_entries[0].packed_size, 12);
        assert_eq!(verify_entries[1].packed_size, 16);
    }

    #[test]
    fn index_pack_bytes_index_only_handles_delta_pack() {
        let mut pack = Vec::new();
        pack.extend_from_slice(PACK_MAGIC);
        pack.extend_from_slice(&PACK_VERSION_2.to_be_bytes());
        pack.extend_from_slice(&(2_u32).to_be_bytes());

        let base = b"base content for pack delta test";
        write_pack_object_header(&mut pack, GitObjectKind::Blob, base.len() as u64);
        append_zlib(&mut pack, base);

        let delta_object_offset = pack.len() as u64;
        let delta = replacement_delta(base, b"base content for pack delta test with a change");
        write_ofs_delta_header(&mut pack, delta_object_offset, 12_u64, delta.len() as u64);
        append_zlib(&mut pack, &delta);
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
        hasher.update(&pack);
        pack.extend_from_slice(hasher.finalize().as_bytes());

        match parse_pack_entries_index_only_if_base(GitHashAlgorithm::Sha1, &pack, false)
            .expect("index-only parse")
        {
            PackIndexOnlyResult::RequiresFullParse { retention, .. } => {
                assert!(retention.offsets.contains_key(&12_u64));
                assert!(retention.ids.is_empty());
            }
            PackIndexOnlyResult::Fast { .. } => panic!("delta pack requires full parse"),
        }
        let indexed = index_pack_bytes_index_only(GitHashAlgorithm::Sha1, &pack)
            .expect("index pack bytes index only");
        let indexed_full =
            index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index pack bytes");

        assert_eq!(indexed.pack_id, indexed_full.pack_id);
        assert_eq!(indexed.index, indexed_full.index);
    }

    #[test]
    fn rejects_pack_reverse_index_checksum_mismatch() {
        let repo = git_init();
        git_env(&repo, ["commit", "--allow-empty", "-m", "initial"]);
        git_env(&repo, ["repack", "-adq"]);
        let pack = std::fs::read(first_idx_path(&repo).with_extension("pack")).expect("read pack");
        let indexed = index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index pack");
        let mut reverse_index = indexed.reverse_index;
        let last = reverse_index.last_mut().expect("non-empty reverse index");
        *last ^= 1;

        let error =
            validate_pack_reverse_index(GitHashAlgorithm::Sha1, &reverse_index, indexed.objects)
                .expect_err("corrupt reverse index");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "pack index checksum mismatch");
    }

    #[test]
    fn repairs_thin_ref_delta_pack_with_external_base() {
        let source = git_init();
        let base = format!("{}\nbase\n", "shared line\n".repeat(4_000));
        std::fs::write(source.path().join("delta.txt"), base).expect("write base");
        git_env(&source, ["add", "delta.txt"]);
        git_env(&source, ["commit", "-m", "base"]);
        let base_commit = git(&source, ["rev-parse", "HEAD"]);
        let changed = format!("{}\nchanged\n", "shared line\n".repeat(4_000));
        std::fs::write(source.path().join("delta.txt"), changed).expect("write changed");
        git_env(&source, ["add", "delta.txt"]);
        git_env(&source, ["commit", "-m", "changed"]);
        let pack_input = format!("HEAD\n^{base_commit}\n");
        let thin_pack = git_raw_with_stdin(
            &source,
            [
                "pack-objects",
                "--stdout",
                "--thin",
                "--window=50",
                "--depth=50",
                "--revs",
            ],
            pack_input.as_bytes(),
        );

        let target = git_init();
        let source_path = source.path().to_str().expect("source path");
        let fetch_ref = format!("{base_commit}:refs/heads/base");
        git_env(&target, ["fetch", source_path, &fetch_ref]);
        let store =
            LooseObjectStore::new(target.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let repaired =
            repair_thin_pack_bytes(GitHashAlgorithm::Sha1, &thin_pack, &store).expect("repair");
        assert!(
            repaired.fixed_objects > 0,
            "fixture must exercise an actual thin pack"
        );
        let reparsed = index_pack_bytes(GitHashAlgorithm::Sha1, &repaired.pack).expect("reparse");
        assert_eq!(reparsed.pack_id, repaired.indexed.pack_id);
        assert_eq!(reparsed.objects, repaired.indexed.objects);
    }

    #[test]
    fn thin_pack_repair_initial_capacity_is_bounded() {
        assert_eq!(
            thin_pack_repair_initial_capacity(100, 2).expect("capacity"),
            100 + 2 * 64
        );
        assert_eq!(
            thin_pack_repair_initial_capacity(100, usize::MAX).expect("capacity"),
            100 + THIN_PACK_REPAIR_EXTRA_CAPACITY_LIMIT
        );
        assert!(thin_pack_repair_initial_capacity(usize::MAX, 1).is_err());
    }

    #[test]
    fn reads_delta_chain_deeper_than_default_git_depth() {
        let repo = git_init();
        let mut contents = Vec::new();
        let mut pack = Vec::new();
        pack.extend_from_slice(PACK_MAGIC);
        pack.extend_from_slice(&PACK_VERSION_2.to_be_bytes());
        pack.extend_from_slice(&81_u32.to_be_bytes());

        let base = b"blob-000".to_vec();
        write_pack_object_header(&mut pack, GitObjectKind::Blob, base.len() as u64);
        append_zlib(&mut pack, &base);
        contents.push(base);

        let mut base_offset = 12_u64;
        for idx in 1..=80 {
            let offset = pack.len() as u64;
            let content = format!("blob-{idx:03}").into_bytes();
            let delta = replacement_delta(contents.last().expect("previous content"), &content);
            write_ofs_delta_header(&mut pack, offset, base_offset, delta.len() as u64);
            append_zlib(&mut pack, &delta);
            contents.push(content);
            base_offset = offset;
        }
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
        hasher.update(&pack);
        pack.extend_from_slice(hasher.finalize().as_bytes());

        let indexed = index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index pack");
        let pack_dir = repo.path().join(".git/objects/pack");
        std::fs::create_dir_all(&pack_dir).expect("create pack dir");
        let pack_path = pack_dir.join(format!("pack-{}.pack", indexed.pack_id.to_hex()));
        let idx_path = pack_path.with_extension("idx");
        std::fs::write(&pack_path, &pack).expect("write pack");
        std::fs::write(&idx_path, &indexed.index).expect("write idx");

        let final_id = hash_object(
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            contents.last().expect("final content"),
        );
        git_raw(&repo, ["verify-pack", idx_path.to_str().expect("idx path")]);
        assert_eq!(
            git_raw(&repo, ["cat-file", "-p", &final_id.to_hex()]),
            *contents.last().expect("final content")
        );
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let object = store.read_object(&final_id).expect("read deep delta chain");

        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, *contents.last().expect("final content"));
    }

    #[test]
    fn packed_store_caches_decoded_objects_for_repeated_reads() {
        let repo = git_init();
        std::fs::write(repo.path().join("packed.txt"), b"packed\n").expect("write packed file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let id = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            &git(&repo, ["rev-parse", "HEAD:packed.txt"]),
        )
        .expect("blob id");
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        let first = store.read_object(&id).expect("first read");
        let second = store.read_object(&id).expect("second read");

        assert_eq!(first.content, b"packed\n");
        assert_eq!(second.content, b"packed\n");
        let cache = store.object_read_cache.lock().expect("object read cache");
        assert!(!cache.entries.is_empty());
        assert!(cache.bytes <= PACK_OBJECT_READ_CACHE_BYTE_LIMIT);
    }

    #[test]
    fn rejects_pack_index_checksum_mismatch() {
        let repo = git_init();
        git_env(&repo, ["commit", "--allow-empty", "-m", "initial"]);
        git_env(&repo, ["repack", "-adq"]);
        let idx_path = first_idx_path(&repo);
        let mut idx = std::fs::read(idx_path).expect("read idx");
        let last = idx.last_mut().expect("non-empty idx");
        *last ^= 1;

        let error = decode_pack_index(GitHashAlgorithm::Sha1, idx).expect_err("corrupt idx");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "pack index checksum mismatch");
    }

    #[test]
    fn lists_pack_names_for_server_info() {
        let repo = git_init();
        git_env(&repo, ["commit", "--allow-empty", "-m", "initial"]);
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let actual = store
            .pack_names()
            .expect("pack names")
            .into_iter()
            .map(|name| format!("P {name}"))
            .chain(std::iter::once(String::new()))
            .collect::<Vec<_>>()
            .join("\n");
        git_env(&repo, ["update-server-info"]);
        let expected = std::fs::read_to_string(repo.path().join(".git/objects/info/packs"))
            .expect("read packs info");

        assert_eq!(format!("{actual}\n"), expected);
    }

    #[test]
    fn packed_store_append_object_ids_streams_into_existing_vector() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"packed\n").expect("write packed file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let expected = store.object_ids().expect("object ids");
        let mut actual = Vec::new();

        store
            .append_object_ids(&mut actual)
            .expect("append packed ids");

        assert_eq!(actual, expected);
    }

    #[test]
    fn packed_store_append_object_ids_single_pack_keeps_index_order() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("first.txt"), b"first\n").expect("write first file");
        std::fs::write(repo.path().join("second.txt"), b"second\n").expect("write second file");
        git_env(&repo, ["add", "first.txt", "second.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let mut ids = Vec::new();

        store
            .append_object_ids(&mut ids)
            .expect("append packed ids");

        assert!(
            ids.windows(2)
                .all(|pair| pair[0].as_bytes() <= pair[1].as_bytes())
        );
    }

    #[test]
    fn packed_store_caches_last_hit_pack_index_for_repeated_reads() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"packed\n").expect("write packed file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let id = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            &git(&repo, ["rev-parse", "HEAD:packed.txt"]),
        )
        .expect("blob id");
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        assert!(
            store
                .last_index_lookup
                .lock()
                .expect("last lookup cache")
                .is_none()
        );
        assert_eq!(
            store.read_object(&id).expect("read packed object").content,
            b"packed\n"
        );
        assert!(
            store
                .last_index_lookup
                .lock()
                .expect("last lookup cache")
                .is_some()
        );
    }

    #[test]
    fn packed_store_caches_last_validated_pack_file_for_repeated_reads() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"packed\n").expect("write packed file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let id = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            &git(&repo, ["rev-parse", "HEAD:packed.txt"]),
        )
        .expect("blob id");
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        assert!(
            store
                .last_pack_file
                .lock()
                .expect("last pack file cache")
                .is_none()
        );
        assert_eq!(
            store.read_object(&id).expect("read packed object").content,
            b"packed\n"
        );
        let cached_pack_path = store
            .last_pack_file
            .lock()
            .expect("last pack file cache")
            .as_ref()
            .map(|cached| cached.pack_path.clone())
            .expect("cached pack path");

        assert_eq!(
            store
                .read_object(&id)
                .expect("read packed object again")
                .content,
            b"packed\n"
        );
        assert_eq!(
            store
                .last_pack_file
                .lock()
                .expect("last pack file cache")
                .as_ref()
                .map(|cached| cached.pack_path.as_path()),
            Some(cached_pack_path.as_path())
        );
    }

    #[test]
    fn packed_store_ignores_stale_last_hit_pack_index() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"packed\n").expect("write packed file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let id = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            &git(&repo, ["rev-parse", "HEAD:packed.txt"]),
        )
        .expect("blob id");
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        store.read_object(&id).expect("prime last index lookup");
        let stale_idx = store
            .last_index_lookup
            .lock()
            .expect("last lookup cache")
            .as_ref()
            .map(|lookup| lookup.idx_path.clone())
            .expect("cached idx path");
        std::fs::remove_file(stale_idx).expect("remove cached idx");

        assert!(
            !store
                .contains_object(&id)
                .expect("contains skips stale idx")
        );
        assert!(
            store
                .last_index_lookup
                .lock()
                .expect("last lookup cache")
                .is_none()
        );
    }

    #[test]
    fn packed_store_object_count_counts_unique_pack_index_objects() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"packed\n").expect("write packed file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let expected = store.object_ids().expect("object ids").len();

        assert_eq!(store.object_count().expect("object count"), expected);

        let pack_dir = repo.path().join(".git/objects/pack");
        for entry in std::fs::read_dir(&pack_dir).expect("read pack dir") {
            let path = entry.expect("pack entry").path();
            if path.is_file() {
                let duplicate = path.with_file_name(format!(
                    "duplicate-{}",
                    path.file_name().expect("pack file name").to_string_lossy()
                ));
                std::fs::copy(&path, duplicate).expect("duplicate pack artifact");
            }
        }

        assert_eq!(
            store.object_count().expect("duplicate object count"),
            expected
        );
        assert_eq!(
            store
                .object_id_capacity_hint()
                .expect("duplicate object id capacity hint"),
            expected
        );
        assert_eq!(
            store.object_ids().expect("duplicate object ids").len(),
            expected
        );
        let mut streamed = Vec::new();
        store
            .for_each_object_id(&mut |id| {
                streamed.push(id.clone());
                Ok(())
            })
            .expect("stream duplicate object ids");
        assert_eq!(streamed.len(), expected);
        let mut appended = vec![streamed[0].clone()];
        store
            .append_object_ids(&mut appended)
            .expect("append into existing ids");
        assert_eq!(appended.len(), expected);
    }

    #[test]
    fn packed_store_resolves_odd_length_prefix_from_index_range() {
        let repo = git_init();
        git_env(&repo, ["config", "user.name", "Zmin Test"]);
        git_env(&repo, ["config", "user.email", "zmin@example.invalid"]);
        std::fs::write(repo.path().join("packed.txt"), b"packed\n").expect("write packed file");
        git_env(&repo, ["add", "packed.txt"]);
        git_env(&repo, ["commit", "-m", "packed"]);
        let blob = git(&repo, ["rev-parse", "HEAD:packed.txt"]);
        git_env(&repo, ["repack", "-adq"]);
        let store =
            PackedObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let expected = ObjectId::from_hex(GitHashAlgorithm::Sha1, &blob).expect("blob id");

        let resolved = store
            .resolve_prefix(&blob[..blob.len() - 1])
            .expect("resolve odd packed prefix");

        assert_eq!(resolved, expected);
    }

    #[test]
    fn hex_prefix_search_bounds_cover_even_odd_and_ff_prefixes() {
        let (lower, upper) =
            hex_prefix_search_bounds("0abc", GitHashAlgorithm::Sha1).expect("even bounds");
        assert_eq!(&lower[..3], &[0x0a, 0xbc, 0x00]);
        assert_eq!(
            upper.as_ref().map(|bytes| &bytes[..3]),
            Some(&[0x0a, 0xbd, 0x00][..])
        );

        let (lower, upper) =
            hex_prefix_search_bounds("0ab", GitHashAlgorithm::Sha1).expect("odd bounds");
        assert_eq!(&lower[..2], &[0x0a, 0xb0]);
        assert_eq!(
            upper.as_ref().map(|bytes| &bytes[..2]),
            Some(&[0x0a, 0xc0][..])
        );

        let (lower, upper) =
            hex_prefix_search_bounds("ffff", GitHashAlgorithm::Sha1).expect("ff bounds");
        assert_eq!(&lower[..3], &[0xff, 0xff, 0x00]);
        assert!(upper.is_none());
    }

    #[test]
    fn packed_store_append_object_ids_growth_capacity_is_bounded() {
        assert_eq!(pack_object_id_growth_capacity(usize::MAX), 8192);
        assert_eq!(pack_object_id_growth_capacity(2), 2);
        assert_eq!(pack_object_id_growth_capacity(0), 0);
    }

    #[test]
    fn pack_retention_reserve_capacity_is_bounded() {
        assert_eq!(pack_retention_reserve_capacity(usize::MAX), 8192);
        assert_eq!(pack_retention_reserve_capacity(2), 2);
        assert_eq!(pack_retention_reserve_capacity(0), 0);
    }

    #[test]
    fn pack_external_bases_initial_capacity_is_bounded() {
        assert_eq!(pack_external_bases_initial_capacity(usize::MAX), 8192);
        assert_eq!(pack_external_bases_initial_capacity(2), 2);
        assert_eq!(pack_external_bases_initial_capacity(0), 0);
    }

    #[test]
    fn pack_external_bases_reserve_only_seeds_empty_vec() {
        let mut external_bases = Vec::new();
        reserve_pack_external_bases(&mut external_bases, 4);
        assert!(external_bases.capacity() >= 4);

        let capacity = external_bases.capacity();
        reserve_pack_external_bases(&mut external_bases, 8);
        assert_eq!(external_bases.capacity(), capacity);

        let mut small_hint = Vec::new();
        reserve_pack_external_bases(&mut small_hint, 1);
        assert_eq!(small_hint.capacity(), 0);
    }

    #[test]
    fn pack_retention_counts_release_after_last_offset_use() {
        let id = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let mut retention = PackRetentionPlan::empty();
        retention.insert_offset(12, 2);
        retention.insert_offset(12, 1);
        let entries = vec![PackIndexEntry {
            object_id: id,
            offset: 12,
            crc32: 0,
        }];
        let mut retained_contents = vec![b"retained base".to_vec()];
        let mut objects = vec![ParsedPackObjectData::new(GitObjectKind::Blob, Some(0)).unwrap()];

        retention.consume_offset(12);
        pack_release_internal_base_if_unused(
            &mut objects,
            &entries,
            false,
            &retention,
            0,
            12,
            &mut retained_contents,
        );
        assert_eq!(objects[0].content(&retained_contents), b"retained base");

        retention.consume_offset(12);
        pack_release_internal_base_if_unused(
            &mut objects,
            &entries,
            false,
            &retention,
            0,
            12,
            &mut retained_contents,
        );
        assert!(!objects[0].has_content());
    }

    #[test]
    fn pack_retention_counts_release_after_last_id_use() {
        let id = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let mut retention = PackRetentionPlan::empty();
        retention.insert_id(id.clone(), 2);
        retention.insert_id(id.clone(), 1);
        let entries = vec![PackIndexEntry {
            object_id: id.clone(),
            offset: 12,
            crc32: 0,
        }];
        let mut retained_contents = vec![b"retained ref base".to_vec()];
        let mut objects = vec![ParsedPackObjectData::new(GitObjectKind::Blob, Some(0)).unwrap()];

        retention.consume_id(&id);
        pack_release_internal_base_if_unused(
            &mut objects,
            &entries,
            false,
            &retention,
            0,
            12,
            &mut retained_contents,
        );
        assert_eq!(objects[0].content(&retained_contents), b"retained ref base");

        retention.consume_id(&id);
        pack_release_internal_base_if_unused(
            &mut objects,
            &entries,
            false,
            &retention,
            0,
            12,
            &mut retained_contents,
        );
        assert!(!objects[0].has_content());
    }

    #[test]
    fn parsed_pack_object_data_packs_kind_and_content_slot() {
        let object = ParsedPackObjectData::new(GitObjectKind::Tree, Some(7)).unwrap();

        assert_eq!(std::mem::size_of::<ParsedPackObjectData>(), 4);
        assert_eq!(object.kind(), GitObjectKind::Tree);
        assert_eq!(object.content_slot(), Some(7));
    }

    #[test]
    fn pack_delta_lookup_initial_capacity_is_bounded() {
        assert_eq!(pack_delta_lookup_initial_capacity(usize::MAX), 8192);
        assert_eq!(pack_delta_lookup_initial_capacity(2), 2);
        assert_eq!(pack_delta_lookup_initial_capacity(0), 0);
    }

    #[test]
    fn pack_object_id_map_only_keeps_future_ref_delta_bases() {
        let kept = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let skipped = ObjectId::new(GitHashAlgorithm::Sha1, &[0x22; 20]);
        let mut retention = PackRetentionPlan::empty();
        retention.insert_id(kept.clone(), 2);
        let entries = vec![
            PackIndexEntry {
                object_id: skipped.clone(),
                offset: 12,
                crc32: 0,
            },
            PackIndexEntry {
                object_id: kept.clone(),
                offset: 48,
                crc32: 0,
            },
        ];
        let objects = vec![
            ParsedPackObjectData::new(GitObjectKind::Blob, Some(0)).unwrap(),
            ParsedPackObjectData::new(GitObjectKind::Blob, Some(1)).unwrap(),
        ];

        let by_id = pack_object_id_map(&entries, &objects, &retention, false);

        assert!(!by_id.contains_key(&skipped));
        assert!(matches!(
            by_id.get(&kept),
            Some((GitObjectKind::Blob, PackObjectRef::Internal(1)))
        ));
    }

    #[test]
    fn pack_object_id_map_keeps_all_objects_when_retain_all_is_enabled() {
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[0x22; 20]);
        let entries = vec![
            PackIndexEntry {
                object_id: first.clone(),
                offset: 12,
                crc32: 0,
            },
            PackIndexEntry {
                object_id: second.clone(),
                offset: 48,
                crc32: 0,
            },
        ];
        let objects = vec![
            ParsedPackObjectData::new(GitObjectKind::Blob, Some(0)).unwrap(),
            ParsedPackObjectData::new(GitObjectKind::Commit, Some(1)).unwrap(),
        ];

        let by_id = pack_object_id_map(&entries, &objects, &PackRetentionPlan::empty(), true);

        assert!(matches!(
            by_id.get(&first),
            Some((GitObjectKind::Blob, PackObjectRef::Internal(0)))
        ));
        assert!(matches!(
            by_id.get(&second),
            Some((GitObjectKind::Commit, PackObjectRef::Internal(1)))
        ));
    }

    #[test]
    fn pack_object_offset_index_uses_sorted_offsets_without_map() {
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[0x22; 20]);
        let third = ObjectId::new(GitHashAlgorithm::Sha1, &[0x33; 20]);
        let entries = vec![
            PackIndexEntry {
                object_id: first,
                offset: 12,
                crc32: 0,
            },
            PackIndexEntry {
                object_id: second,
                offset: 48,
                crc32: 0,
            },
            PackIndexEntry {
                object_id: third,
                offset: 96,
                crc32: 0,
            },
        ];

        assert_eq!(pack_object_offset_index(&entries, 12), Some(0));
        assert_eq!(pack_object_offset_index(&entries, 96), Some(2));
        assert_eq!(pack_object_offset_index(&entries, 64), None);
    }

    #[test]
    fn pack_ref_delta_uses_linear_lookup_for_small_base_sets() {
        assert!(pack_ref_delta_uses_linear_lookup(false, 0));
        assert!(pack_ref_delta_uses_linear_lookup(
            false,
            PACK_REF_DELTA_LINEAR_LOOKUP_LIMIT
        ));
        assert!(!pack_ref_delta_uses_linear_lookup(
            false,
            PACK_REF_DELTA_LINEAR_LOOKUP_LIMIT + 1
        ));
        assert!(!pack_ref_delta_uses_linear_lookup(true, 0));
    }

    #[test]
    fn pack_should_retain_content_skips_empty_retention_sets() {
        let id = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let mut retention = PackRetentionPlan::empty();

        assert!(pack_should_retain_content(true, &retention, 12, &id));
        assert!(!pack_should_retain_content(false, &retention, 12, &id));

        retention.insert_offset(12, 1);
        assert!(pack_should_retain_content(false, &retention, 12, &id));
        assert!(!pack_should_retain_content(false, &retention, 24, &id));

        retention.insert_id(id.clone(), 1);
        assert!(pack_should_retain_content(false, &retention, 24, &id));
    }

    #[test]
    fn pack_stream_hash_content_checks_offset_retention_only() {
        let id = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let mut retention = PackRetentionPlan::empty();

        assert!(pack_can_stream_hash_base_object_content(
            false, &retention, 12
        ));
        assert!(pack_can_stream_hash_delta_object_content(
            false, &retention, 12
        ));
        assert!(!pack_can_stream_hash_base_object_content(
            true, &retention, 12
        ));
        assert!(!pack_can_stream_hash_delta_object_content(
            true, &retention, 12
        ));

        retention.insert_offset(12, 1);
        assert!(!pack_can_stream_hash_base_object_content(
            false, &retention, 12
        ));
        assert!(!pack_can_stream_hash_delta_object_content(
            false, &retention, 12
        ));
        assert!(pack_can_stream_hash_base_object_content(
            false, &retention, 24
        ));
        assert!(pack_can_stream_hash_delta_object_content(
            false, &retention, 24
        ));

        retention.insert_id(id, 1);
        assert!(pack_can_stream_hash_base_object_content(
            false, &retention, 24
        ));
        assert!(pack_can_stream_hash_delta_object_content(
            false, &retention, 24
        ));
    }

    #[test]
    fn parse_pack_entries_drops_internal_objects_for_index_only_delta_parse() {
        let mut pack = Vec::new();
        pack.extend_from_slice(PACK_MAGIC);
        pack.extend_from_slice(&PACK_VERSION_2.to_be_bytes());
        pack.extend_from_slice(&(3_u32).to_be_bytes());

        let unused = b"unused base object content";
        let retained_base = b"retained base object content";
        let replacement = b"retained base object content with delta";
        let unused_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, unused);
        let retained_base_id =
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, retained_base);
        let replacement_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, replacement);

        write_pack_object_header(&mut pack, GitObjectKind::Blob, unused.len() as u64);
        append_zlib(&mut pack, unused);
        let retained_base_offset = pack.len() as u64;
        write_pack_object_header(&mut pack, GitObjectKind::Blob, retained_base.len() as u64);
        append_zlib(&mut pack, retained_base);
        let delta_offset = pack.len() as u64;
        let delta = replacement_delta(retained_base, replacement);
        write_ofs_delta_header(
            &mut pack,
            delta_offset,
            retained_base_offset,
            delta.len() as u64,
        );
        append_zlib(&mut pack, &delta);
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
        hasher.update(&pack);
        pack.extend_from_slice(hasher.finalize().as_bytes());

        let parsed = parse_pack_entries(GitHashAlgorithm::Sha1, &pack, None, false, false, None)
            .expect("parse pack entries");

        assert!(parsed.objects.is_empty());
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == unused_id)
        );
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == retained_base_id)
        );
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == replacement_id)
        );
    }

    #[test]
    fn base_objects_not_referenced_by_ref_delta_can_stream_hash_with_id_retention() {
        let mut pack = Vec::new();
        pack.extend_from_slice(PACK_MAGIC);
        pack.extend_from_slice(&PACK_VERSION_2.to_be_bytes());
        pack.extend_from_slice(&(3_u32).to_be_bytes());

        let unused = b"unused base object beside a ref-delta";
        let retained_base = b"retained base object for ref-delta";
        let replacement = b"retained base object for ref-delta with a suffix";
        let unused_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, unused);
        let retained_base_id =
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, retained_base);
        let replacement_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, replacement);

        write_pack_object_header(&mut pack, GitObjectKind::Blob, unused.len() as u64);
        append_zlib(&mut pack, unused);
        write_pack_object_header(&mut pack, GitObjectKind::Blob, retained_base.len() as u64);
        append_zlib(&mut pack, retained_base);
        let delta = replacement_delta(retained_base, replacement);
        write_ref_delta_header(&mut pack, retained_base_id.as_bytes(), delta.len() as u64);
        append_zlib(&mut pack, &delta);
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
        hasher.update(&pack);
        pack.extend_from_slice(hasher.finalize().as_bytes());

        let parsed = parse_pack_entries(GitHashAlgorithm::Sha1, &pack, None, false, false, None)
            .expect("parse ref-delta pack");

        assert!(parsed.objects.is_empty());
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == unused_id)
        );
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == retained_base_id)
        );
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == replacement_id)
        );
    }

    #[test]
    fn delta_objects_referenced_by_ref_delta_are_retained_after_stream_hashing() {
        let mut pack = Vec::new();
        pack.extend_from_slice(PACK_MAGIC);
        pack.extend_from_slice(&PACK_VERSION_2.to_be_bytes());
        pack.extend_from_slice(&(3_u32).to_be_bytes());

        let base = b"base object for chained ref deltas";
        let middle = b"base object for chained ref deltas with middle";
        let replacement = b"base object for chained ref deltas with middle and tail";
        let base_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, base);
        let middle_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, middle);
        let replacement_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, replacement);

        write_pack_object_header(&mut pack, GitObjectKind::Blob, base.len() as u64);
        append_zlib(&mut pack, base);
        let middle_delta = replacement_delta(base, middle);
        write_ref_delta_header(&mut pack, base_id.as_bytes(), middle_delta.len() as u64);
        append_zlib(&mut pack, &middle_delta);
        let replacement_delta = replacement_delta(middle, replacement);
        write_ref_delta_header(
            &mut pack,
            middle_id.as_bytes(),
            replacement_delta.len() as u64,
        );
        append_zlib(&mut pack, &replacement_delta);
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
        hasher.update(&pack);
        pack.extend_from_slice(hasher.finalize().as_bytes());

        let parsed = parse_pack_entries(GitHashAlgorithm::Sha1, &pack, None, false, false, None)
            .expect("parse chained ref-delta pack");

        assert!(parsed.objects.is_empty());
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == base_id)
        );
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == middle_id)
        );
        assert!(
            parsed
                .entries
                .iter()
                .any(|entry| entry.object_id == replacement_id)
        );
    }

    #[test]
    fn pack_should_return_parsed_objects_only_for_callers_that_need_them() {
        assert!(pack_should_return_parsed_objects(true));
        assert!(!pack_should_return_parsed_objects(false));
    }

    #[test]
    fn ref_delta_mapped_base_allows_empty_content_when_retain_all() {
        let base_id = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let mut by_id = HashMap::new();
        by_id.insert(
            base_id.clone(),
            (GitObjectKind::Blob, PackObjectRef::Internal(0)),
        );
        let objects = vec![ParsedPackObjectData::new(GitObjectKind::Blob, None).unwrap()];
        let retained_contents = Vec::new();
        let mut external_bases = Vec::new();

        let base = pack_ref_delta_base_mapped(
            base_id,
            &mut by_id,
            &objects,
            &retained_contents,
            true,
            &mut external_bases,
            0,
            None,
        )
        .expect("retain-all empty base");

        assert_eq!(base.kind, GitObjectKind::Blob);
        assert!(base.content.is_empty());
        assert_eq!(base.internal_index, Some(0));
    }

    #[test]
    fn pack_object_id_index_scans_existing_objects_without_map() {
        let first = ObjectId::new(GitHashAlgorithm::Sha1, &[0x11; 20]);
        let second = ObjectId::new(GitHashAlgorithm::Sha1, &[0x22; 20]);
        let missing = ObjectId::new(GitHashAlgorithm::Sha1, &[0x33; 20]);
        let entries = vec![
            PackIndexEntry {
                object_id: first,
                offset: 12,
                crc32: 0,
            },
            PackIndexEntry {
                object_id: second.clone(),
                offset: 48,
                crc32: 0,
            },
        ];

        assert_eq!(pack_object_id_index(&entries, &second), Some(1));
        assert_eq!(pack_object_id_index(&entries, &missing), None);
    }

    #[test]
    fn pack_parse_initial_capacity_matches_object_count() {
        assert_eq!(pack_parse_initial_capacity(usize::MAX), usize::MAX);
        assert_eq!(pack_parse_initial_capacity(2), 2);
        assert_eq!(pack_parse_initial_capacity(0), 0);
    }

    #[test]
    fn zlib_content_initial_capacity_is_bounded() {
        assert_eq!(
            zlib_content_capacity(
                512 * 1024 * 1024,
                ZLIB_CONTENT_INITIAL_CAPACITY_LIMIT as u64 + 1
            )
            .expect("capacity"),
            ZLIB_CONTENT_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(
            zlib_content_capacity(512 * 1024 * 1024, 2).expect("capacity"),
            2
        );
        assert_eq!(
            zlib_content_capacity(512 * 1024 * 1024, 0).expect("capacity"),
            0
        );
        assert!(zlib_content_capacity(10, 11).is_err());
    }

    #[test]
    fn zlib_content_read_limit_stops_after_declared_size() {
        assert_eq!(
            zlib_content_read_limit(512 * 1024 * 1024, 2).expect("limit"),
            3
        );
        assert!(zlib_content_read_limit(10, 11).is_err());
    }

    #[test]
    fn zlib_content_reader_stops_after_declared_size_mismatch() {
        let mut compressed = Vec::new();
        append_zlib(&mut compressed, b"actual");
        let mut cursor = std::io::Cursor::new(compressed.as_slice());

        let content = read_zlib_content_from_cursor(&mut cursor, 512 * 1024 * 1024, 2)
            .expect("read bounded zlib content");

        assert_eq!(content, b"act");
    }

    #[test]
    fn zlib_hash_reader_rejects_beyond_declared_size() {
        let mut compressed = Vec::new();
        append_zlib(&mut compressed, b"actual");
        let mut cursor = std::io::Cursor::new(compressed.as_slice());
        let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
        hasher.update_object_header(GitObjectKind::Blob, 2);

        let error = read_zlib_content_hashing(&mut cursor, &mut hasher, 512 * 1024 * 1024, 2)
            .expect_err("declared size mismatch");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "packed object inflated size mismatch");
    }

    #[test]
    fn zlib_skip_reader_stops_after_declared_size_mismatch() {
        let mut compressed = Vec::new();
        append_zlib(&mut compressed, b"actual");
        let mut cursor = std::io::Cursor::new(compressed.as_slice());

        let error = skip_zlib_content_from_cursor(&mut cursor, 512 * 1024 * 1024, 2)
            .expect_err("declared size mismatch");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "packed object inflated size mismatch");
    }

    #[test]
    fn zlib_skip_reader_validates_declared_size() {
        let mut compressed = Vec::new();
        append_zlib(&mut compressed, b"actual");
        let mut cursor = std::io::Cursor::new(compressed.as_slice());

        let skipped = skip_zlib_content_from_cursor(&mut cursor, 512 * 1024 * 1024, 6)
            .expect("skip valid zlib content");

        assert_eq!(skipped, 6);
        assert!(cursor.position() > 0);
    }

    #[test]
    fn zlib_skip_reader_handles_multi_buffer_content() {
        let content = vec![0x41; PACK_ZLIB_STREAM_BUFFER_CAPACITY + 17];
        let mut compressed = Vec::new();
        append_zlib(&mut compressed, &content);
        let mut cursor = std::io::Cursor::new(compressed.as_slice());

        let skipped =
            skip_zlib_content_from_cursor(&mut cursor, 512 * 1024 * 1024, content.len() as u64)
                .expect("skip multi-buffer zlib content");

        assert_eq!(skipped, content.len());
        assert!(cursor.position() > 0);
    }

    #[test]
    fn delta_result_initial_capacity_is_bounded() {
        assert_eq!(
            delta_result_initial_capacity(
                512 * 1024 * 1024,
                DELTA_RESULT_INITIAL_CAPACITY_LIMIT as u64 + 1
            )
            .expect("capacity"),
            DELTA_RESULT_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(
            delta_result_initial_capacity(512 * 1024 * 1024, 2).expect("capacity"),
            2
        );
        assert_eq!(
            delta_result_initial_capacity(512 * 1024 * 1024, 0).expect("capacity"),
            0
        );
        assert!(delta_result_initial_capacity(10, 11).is_err());
    }

    #[test]
    fn pack_index_cache_trim_bounds_entry_count_before_insert() {
        let mut cache = HashMap::new();
        for index in 0..PACK_INDEX_CACHE_ENTRY_LIMIT {
            cache.insert(
                PathBuf::from(format!("pack-{index}.idx")),
                empty_cached_pack_index(),
            );
        }

        trim_pack_index_cache_for_insert(&mut cache, Path::new("new-pack.idx"), 0);
        cache.insert(PathBuf::from("new-pack.idx"), empty_cached_pack_index());

        assert_eq!(cache.len(), PACK_INDEX_CACHE_ENTRY_LIMIT);
        assert!(cache.contains_key(Path::new("new-pack.idx")));
    }

    #[test]
    fn pack_index_cache_trim_bounds_total_cached_bytes() {
        let mut cache = HashMap::new();
        cache.insert(
            PathBuf::from("pack-a.idx"),
            cached_pack_index_with_len(PACK_INDEX_CACHE_BYTE_LIMIT),
        );

        trim_pack_index_cache_for_insert(&mut cache, Path::new("pack-b.idx"), 1);
        cache.insert(PathBuf::from("pack-b.idx"), cached_pack_index_with_len(1));

        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(Path::new("pack-b.idx")));
        assert!(pack_index_cache_should_store(PACK_INDEX_CACHE_BYTE_LIMIT));
        assert!(!pack_index_cache_should_store(
            PACK_INDEX_CACHE_BYTE_LIMIT + 1
        ));
    }

    fn empty_cached_pack_index() -> CachedPackIndex {
        cached_pack_index_with_len(0)
    }

    fn cached_pack_index_with_len(len: u64) -> CachedPackIndex {
        CachedPackIndex {
            modified: None,
            len,
            index: Arc::new(PackIndex {
                bytes: PackIndexBytes::Owned(Vec::new()),
                fanout: [0; 256],
                count: 0,
                algorithm: GitHashAlgorithm::Sha1,
                version: PackIndexVersion::V2,
                layout: pack_index_layout(
                    PackIndexVersion::V2,
                    0,
                    GitHashAlgorithm::Sha1.digest_len(),
                )
                .expect("empty index layout"),
            }),
        }
    }

    fn git_init() -> TempDir {
        let repo = TempDir::new().expect("temp repo");
        git_env(&repo, ["init", "--quiet"]);
        repo
    }

    fn git<const N: usize>(repo: &TempDir, args: [&str; N]) -> String {
        String::from_utf8(git_raw(repo, args))
            .expect("git stdout utf8")
            .trim_end_matches('\n')
            .to_owned()
    }

    fn git_raw<const N: usize>(repo: &TempDir, args: [&str; N]) -> Vec<u8> {
        let output = git_output(repo, args);
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn git_with_stdin<const N: usize>(repo: &TempDir, args: [&str; N], stdin: &[u8]) -> String {
        String::from_utf8(git_raw_with_stdin(repo, args, stdin))
            .expect("git stdout utf8")
            .trim_end_matches('\n')
            .to_owned()
    }

    fn git_raw_with_stdin<const N: usize>(
        repo: &TempDir,
        args: [&str; N],
        stdin: &[u8],
    ) -> Vec<u8> {
        use std::io::Write as _;
        use std::process::Stdio;

        let mut child = Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn git");
        child
            .stdin
            .as_mut()
            .expect("git stdin")
            .write_all(stdin)
            .expect("write git stdin");
        let output = child.wait_with_output().expect("wait git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn git_env<const N: usize>(repo: &TempDir, args: [&str; N]) {
        let output = git_output(repo, args);
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output<const N: usize>(repo: &TempDir, args: [&str; N]) -> std::process::Output {
        Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(repo.path())
            .env("GIT_AUTHOR_NAME", "Zmin Test")
            .env("GIT_AUTHOR_EMAIL", "zmin@example.invalid")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Zmin Test")
            .env("GIT_COMMITTER_EMAIL", "zmin@example.invalid")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .output()
            .expect("run git")
    }

    fn append_zlib(out: &mut Vec<u8>, content: &[u8]) {
        use std::io::Write as _;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content).expect("write zlib content");
        out.extend_from_slice(&encoder.finish().expect("finish zlib"));
    }

    fn write_ref_delta_header(out: &mut Vec<u8>, base_id: &[u8], mut size: u64) {
        let mut byte = (7_u8 << 4) | ((size as u8) & 0x0f);
        size >>= 4;
        if size != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        while size != 0 {
            let mut byte = (size as u8) & 0x7f;
            size >>= 7;
            if size != 0 {
                byte |= 0x80;
            }
            out.push(byte);
        }
        out.extend_from_slice(base_id);
    }

    fn replacement_delta(base: &[u8], replacement: &[u8]) -> Vec<u8> {
        let mut delta = Vec::new();
        write_delta_varint(&mut delta, base.len() as u64);
        write_delta_varint(&mut delta, replacement.len() as u64);
        for chunk in replacement.chunks(127) {
            delta.push(chunk.len() as u8);
            delta.extend_from_slice(chunk);
        }
        delta
    }

    fn write_delta_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn write_ofs_delta_header(out: &mut Vec<u8>, object_offset: u64, base_offset: u64, size: u64) {
        let type_code = 6_u8;
        let mut remaining = size >> 4;
        let mut byte = (type_code << 4) | ((size as u8) & 0x0f);
        if remaining != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        while remaining != 0 {
            let mut byte = (remaining as u8) & 0x7f;
            remaining >>= 7;
            if remaining != 0 {
                byte |= 0x80;
            }
            out.push(byte);
        }
        write_delta_base_offset(out, object_offset - base_offset);
    }

    fn write_delta_base_offset(out: &mut Vec<u8>, mut distance: u64) {
        let mut bytes = vec![(distance & 0x7f) as u8];
        distance >>= 7;
        while distance != 0 {
            distance -= 1;
            bytes.push(((distance & 0x7f) as u8) | 0x80);
            distance >>= 7;
        }
        out.extend(bytes.into_iter().rev());
    }

    fn first_idx_path(repo: &TempDir) -> PathBuf {
        let mut paths = std::fs::read_dir(repo.path().join(".git/objects/pack"))
            .expect("read pack dir")
            .map(|entry| entry.expect("pack dir entry").path())
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("idx"))
            .collect::<Vec<_>>();
        paths.sort();
        paths.into_iter().next().expect("idx path")
    }

    fn first_delta_blob(repo: &TempDir, idx_path: &Path) -> String {
        let output = Command::new("git")
            .args(["verify-pack", "-v"])
            .arg(idx_path)
            .current_dir(repo.path())
            .output()
            .expect("run git verify-pack");
        assert!(
            output.status.success(),
            "git verify-pack failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("verify-pack stdout utf8");
        stdout
            .lines()
            .find_map(|line| {
                let parts = line.split_whitespace().collect::<Vec<_>>();
                if parts.len() >= 6 && parts.get(1) == Some(&"blob") {
                    Some(parts[0].to_owned())
                } else {
                    None
                }
            })
            .expect("delta blob in verify-pack output")
    }
}
