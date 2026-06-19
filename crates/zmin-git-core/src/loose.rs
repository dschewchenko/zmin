use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use crate::object::{GitHashAlgorithm, GitObjectHash, GitObjectKind, ObjectId, hash_object};
use crate::object_store::{GitObjectSink, GitObjectStore, ObjectStorageHint};
use crate::pack::PackedObjectStore;
use crate::tree::{TreeObjectRef, decode_tree_object_refs};

static TEMP_OBJECT_COUNTER: AtomicU64 = AtomicU64::new(0);
const STREAM_LOOSE_BLOB_WRITE_MIN_BYTES: usize = 1024 * 1024;
const OBJECT_ID_INITIAL_CAPACITY_LIMIT: usize = 8192;
const LOOSE_OBJECT_ID_GROWTH_CAPACITY_LIMIT: usize = 8192;
const LOOSE_OBJECT_CONTENT_INITIAL_CAPACITY_LIMIT: usize = 8192;
const ALTERNATES_FILE_BUFFER_CAPACITY: usize = 64 * 1024;
const ALTERNATE_LINE_INITIAL_CAPACITY: usize = 256;
const OBJECT_FANOUT_DIRS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LooseObject {
    pub id: ObjectId,
    pub kind: GitObjectKind,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LooseObjectStore {
    objects_dir: PathBuf,
    algorithm: GitHashAlgorithm,
    max_object_bytes: usize,
    packed_store: PackedObjectStore,
    alternates_cache: Arc<Mutex<Option<Arc<Vec<PathBuf>>>>>,
    objects_dir_ready: Arc<AtomicBool>,
    fanout_dirs_ready: Arc<[AtomicBool; OBJECT_FANOUT_DIRS]>,
}

#[derive(Debug, Clone, Copy)]
pub struct PackedFirstObjectStore<'a> {
    inner: &'a LooseObjectStore,
}

struct CopyingReader<R, W> {
    reader: R,
    writer: W,
}

struct LooseBlobContentWriter<W> {
    inner: W,
    hasher: GitObjectHash,
    remaining: usize,
}

impl LooseObjectStore {
    pub fn new(objects_dir: impl Into<PathBuf>, algorithm: GitHashAlgorithm) -> Self {
        let objects_dir = objects_dir.into();
        Self {
            packed_store: PackedObjectStore::new(&objects_dir, algorithm),
            objects_dir,
            algorithm,
            max_object_bytes: 512 * 1024 * 1024,
            alternates_cache: Arc::new(Mutex::new(None)),
            objects_dir_ready: Arc::new(AtomicBool::new(false)),
            fanout_dirs_ready: Arc::new(std::array::from_fn(|_| AtomicBool::new(false))),
        }
    }

    pub fn with_max_object_bytes(mut self, max_object_bytes: usize) -> Self {
        self.max_object_bytes = max_object_bytes;
        self.packed_store = self.packed_store.with_max_object_bytes(max_object_bytes);
        self
    }

    pub fn objects_dir(&self) -> &Path {
        &self.objects_dir
    }

    fn ensure_objects_dir(&self) -> io::Result<()> {
        if self.objects_dir_ready.load(Ordering::Acquire) {
            return Ok(());
        }
        fs::create_dir_all(&self.objects_dir)?;
        self.objects_dir_ready.store(true, Ordering::Release);
        Ok(())
    }

    fn ensure_object_parent_dir(&self, id: &ObjectId, parent: &Path) -> io::Result<()> {
        let fanout = object_fanout_index(id)?;
        if self.fanout_dirs_ready[fanout].load(Ordering::Acquire) {
            return Ok(());
        }
        fs::create_dir_all(parent)?;
        self.fanout_dirs_ready[fanout].store(true, Ordering::Release);
        Ok(())
    }

    pub fn write_object(&self, kind: GitObjectKind, content: &[u8]) -> io::Result<ObjectId> {
        let id = hash_object(self.algorithm, kind, content);
        let path = self.object_path(&id)?;
        if path.exists() {
            return Ok(id);
        }
        if kind == GitObjectKind::Blob && content.len() >= STREAM_LOOSE_BLOB_WRITE_MIN_BYTES {
            self.write_streamed_blob(&id, content.len(), |writer| writer.write_all(content))?;
            return Ok(id);
        }

        let compressed = encode_loose_object(kind, content)?;

        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "loose object path has no parent",
            )
        })?;
        self.ensure_object_parent_dir(&id, parent)?;

        let tmp_path = temp_object_path(parent, &id);
        write_temp_object(&tmp_path, &compressed)?;
        install_temp_object_file(&tmp_path, &path)?;

        Ok(id)
    }

    pub fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        self.read_object_inner(id, &mut HashSet::new())
    }

    pub fn contains_object(&self, id: &ObjectId) -> io::Result<bool> {
        self.contains_object_inner(id, &mut HashSet::new())
    }

    pub fn object_kind_hint(&self, id: &ObjectId) -> io::Result<Option<GitObjectKind>> {
        self.object_kind_hint_inner(id, &mut HashSet::new())
    }

    pub fn loose_blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        self.stream_loose_blob(id, None)
    }

    pub fn loose_blob_prefix(
        &self,
        id: &ObjectId,
        max_bytes: usize,
    ) -> io::Result<Option<Vec<u8>>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let path = self.object_path(id)?;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut decoder = ZlibDecoder::new(file);
        let (kind, size) = read_loose_object_header(&mut decoder)?;
        if kind != GitObjectKind::Blob {
            return Ok(None);
        }
        if size > self.max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object exceeds configured size limit",
            ));
        }
        let mut prefix = vec![0_u8; size.min(max_bytes)];
        decoder.read_exact(&mut prefix)?;
        Ok(Some(prefix))
    }

    pub fn loose_object_header_hint(
        &self,
        id: &ObjectId,
    ) -> io::Result<Option<(GitObjectKind, usize)>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let path = self.object_path(id)?;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut decoder = ZlibDecoder::new(file);
        let (kind, size) = read_loose_object_header(&mut decoder)?;
        if size > self.max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object exceeds configured size limit",
            ));
        }
        Ok(Some((kind, size)))
    }

    pub fn delta_base_hint(&self, id: &ObjectId) -> io::Result<Option<ObjectId>> {
        self.delta_base_hint_inner(id, &mut HashSet::new())
    }

    pub fn object_disk_size_hint(&self, id: &ObjectId) -> io::Result<Option<u64>> {
        self.object_disk_size_hint_inner(id, &mut HashSet::new())
    }

    pub fn read_tree_refs(&self, id: &ObjectId) -> io::Result<Vec<TreeObjectRef>> {
        self.read_tree_refs_inner(id, &mut HashSet::new())
    }

    pub fn copy_loose_object_to(
        &self,
        destination: &LooseObjectStore,
        id: &ObjectId,
    ) -> io::Result<bool> {
        self.copy_loose_object_to_inner(destination, id, true)
    }

    pub fn copy_loose_object_to_known_missing(
        &self,
        destination: &LooseObjectStore,
        id: &ObjectId,
    ) -> io::Result<bool> {
        self.copy_loose_object_to_inner(destination, id, false)
    }

    fn copy_loose_object_to_inner(
        &self,
        destination: &LooseObjectStore,
        id: &ObjectId,
        check_destination: bool,
    ) -> io::Result<bool> {
        if self.algorithm != destination.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source and destination object stores use different algorithms",
            ));
        }
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let destination_path = destination.object_path(id)?;
        if check_destination && destination_path.exists() {
            return Ok(true);
        }
        let source_path = self.object_path(id)?;
        let source_file = match fs::File::open(source_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };

        let parent = destination_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "loose object path has no parent",
            )
        })?;
        destination.ensure_object_parent_dir(id, parent)?;
        let tmp_path = temp_object_path(parent, id);
        let tmp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        let result = (|| {
            let copying_reader = CopyingReader {
                reader: source_file,
                writer: tmp_file,
            };
            verify_loose_object_copy(copying_reader, destination.max_object_bytes, id)?;
            install_temp_object_file(&tmp_path, &destination_path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        result.map(|()| true)
    }

    pub fn write_streamed_blob<F>(
        &self,
        id: &ObjectId,
        size: usize,
        write_content: F,
    ) -> io::Result<()>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        if size > self.max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object exceeds configured size limit",
            ));
        }
        let path = self.object_path(id)?;
        if path.exists() {
            return Ok(());
        }
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "loose object path has no parent",
            )
        })?;
        self.ensure_object_parent_dir(id, parent)?;
        let tmp_path = temp_object_path(parent, id);
        let tmp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        let result = (|| {
            let mut encoder = ZlibEncoder::new(tmp_file, loose_object_compression());
            write_loose_object_header(&mut encoder, GitObjectKind::Blob, size)?;
            let mut content = LooseBlobContentWriter {
                inner: encoder,
                hasher: GitObjectHash::new(self.algorithm),
                remaining: size,
            };
            content
                .hasher
                .update_object_header(GitObjectKind::Blob, size);
            write_content(&mut content)?;
            let encoder = content.finish(id)?;
            encoder.finish()?;
            install_temp_object_file(&tmp_path, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        result
    }

    pub fn write_streamed_blob_content<F>(
        &self,
        size: usize,
        write_content: F,
    ) -> io::Result<ObjectId>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        if size > self.max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object exceeds configured size limit",
            ));
        }
        self.ensure_objects_dir()?;
        let tmp_path = temp_unknown_object_path(&self.objects_dir);
        let tmp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        let result = (|| {
            let mut encoder = ZlibEncoder::new(tmp_file, loose_object_compression());
            write_loose_object_header(&mut encoder, GitObjectKind::Blob, size)?;
            let mut content = LooseBlobContentWriter {
                inner: encoder,
                hasher: GitObjectHash::new(self.algorithm),
                remaining: size,
            };
            content
                .hasher
                .update_object_header(GitObjectKind::Blob, size);
            write_content(&mut content)?;
            let (encoder, id) = content.finish_with_id()?;
            encoder.finish()?;
            let path = self.object_path(&id)?;
            let parent = path.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "loose object path has no parent",
                )
            })?;
            self.ensure_object_parent_dir(&id, parent)?;
            install_temp_object_file(&tmp_path, &path)?;
            Ok(id)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        result
    }

    fn stream_loose_blob(
        &self,
        id: &ObjectId,
        writer: Option<&mut dyn Write>,
    ) -> io::Result<Option<usize>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let path = self.object_path(id)?;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut decoder = ZlibDecoder::new(file);
        let (kind, size) = read_loose_object_header(&mut decoder)?;
        if kind != GitObjectKind::Blob {
            return Ok(None);
        }
        if size > self.max_object_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object exceeds configured size limit",
            ));
        }
        let Some(writer) = writer else {
            return Ok(Some(size));
        };

        let mut hasher = GitObjectHash::new(self.algorithm);
        hasher.update_object_header(kind, size);
        let mut remaining = size;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining > 0 {
            let read_len = remaining.min(buffer.len());
            let read = decoder.read(&mut buffer[..read_len])?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "loose git object ended before declared size",
                ));
            }
            hasher.update(&buffer[..read]);
            writer.write_all(&buffer[..read])?;
            remaining -= read;
        }
        let mut trailing = [0_u8; 1];
        if decoder.read(&mut trailing)? != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object has trailing content",
            ));
        }
        if hasher.finalize() != *id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object hash mismatch",
            ));
        }
        Ok(Some(size))
    }

    pub fn packed_first(&self) -> PackedFirstObjectStore<'_> {
        PackedFirstObjectStore { inner: self }
    }

    fn read_object_packed_first(&self, id: &ObjectId) -> io::Result<LooseObject> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        match self.packed_store.read_object(id) {
            Ok(object) => Ok(object),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.read_object(id),
            Err(error) => Err(error),
        }
    }

    fn read_object_inner(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<LooseObject> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "git object not found in alternates",
            ));
        }
        let path = self.object_path(id)?;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                match self.packed_store.read_object(id) {
                    Ok(object) => return Ok(object),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        return self.read_object_from_alternates(id, visited);
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(err) => return Err(err),
        };
        let (kind, content) =
            read_verified_loose_object_content(file, self.algorithm, id, self.max_object_bytes)?;

        Ok(LooseObject {
            id: id.clone(),
            kind,
            content,
        })
    }

    fn contains_object_inner(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<bool> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(false);
        }
        if self.object_path(id)?.is_file() {
            return Ok(true);
        }
        if self.packed_store.contains_object(id)? {
            return Ok(true);
        }
        self.contains_object_in_alternates(id, visited)
    }

    fn object_kind_hint_inner(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Option<GitObjectKind>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(None);
        }
        match fs::File::open(self.object_path(id)?) {
            Ok(file) => {
                let mut decoder = ZlibDecoder::new(file);
                let (kind, size) = read_loose_object_header(&mut decoder)?;
                if size > self.max_object_bytes {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "loose git object exceeds configured size limit",
                    ));
                }
                Ok(Some(kind))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match self.packed_store.object_kind_hint(id)? {
                    Some(kind) => Ok(Some(kind)),
                    None => self.object_kind_hint_from_alternates(id, visited),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn read_tree_refs_inner(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Vec<TreeObjectRef>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "git object not found in alternates",
            ));
        }
        match fs::File::open(self.object_path(id)?) {
            Ok(file) => {
                let (kind, content) = read_verified_loose_object_content(
                    file,
                    self.algorithm,
                    id,
                    self.max_object_bytes,
                )?;
                if kind != GitObjectKind::Tree {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "object is not a tree",
                    ));
                }
                decode_tree_object_refs(id.algorithm(), &content)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match self.packed_store.read_tree_refs(id) {
                    Ok(entries) => Ok(entries),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        self.read_tree_refs_from_alternates(id, visited)
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn delta_base_hint_inner(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Option<ObjectId>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(None);
        }
        if self.object_path(id)?.is_file() {
            return Ok(None);
        }
        match self.packed_store.delta_base_hint(id)? {
            Some(base) => Ok(Some(base)),
            None if self.packed_store.contains_object(id)? => Ok(None),
            None => self.delta_base_hint_from_alternates(id, visited),
        }
    }

    fn object_disk_size_hint_inner(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Option<u64>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(None);
        }
        match fs::metadata(self.object_path(id)?) {
            Ok(metadata) if metadata.is_file() => return Ok(Some(metadata.len())),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match self.packed_store.object_disk_size_hint(id)? {
            Some(size) => Ok(Some(size)),
            None => self.object_disk_size_hint_from_alternates(id, visited),
        }
    }

    pub fn resolve_prefix(&self, hex_prefix: &str) -> io::Result<ObjectId> {
        validate_hex_prefix(hex_prefix, self.algorithm)?;
        if hex_prefix.len() == self.algorithm.digest_len() * 2 {
            return ObjectId::from_hex(self.algorithm, hex_prefix);
        }

        let mut resolved = None;
        self.collect_prefix_inner(hex_prefix, &mut resolved, &mut HashSet::new())?;
        resolved.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "git object not found"))
    }

    pub fn object_ids(&self) -> io::Result<Vec<ObjectId>> {
        let mut ids =
            Vec::with_capacity(object_id_initial_capacity(self.object_id_capacity_hint()?));
        self.collect_object_ids_inner(&mut ids, &mut HashSet::new())?;
        ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        ids.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
        Ok(ids)
    }

    pub fn disk_object_ids(&self) -> io::Result<Vec<ObjectId>> {
        let mut ids =
            Vec::with_capacity(object_id_initial_capacity(self.object_id_capacity_hint()?));
        self.collect_object_ids_inner(&mut ids, &mut HashSet::new())?;
        ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        Ok(ids)
    }

    pub fn disk_object_entries(&self) -> io::Result<Vec<(ObjectId, u64)>> {
        let mut entries =
            Vec::with_capacity(object_id_initial_capacity(self.object_id_capacity_hint()?));
        self.collect_disk_object_entries_inner(&mut entries, &mut HashSet::new())?;
        entries.sort_by(|left, right| {
            left.0
                .as_bytes()
                .cmp(right.0.as_bytes())
                .then_with(|| left.1.cmp(&right.1))
        });
        Ok(entries)
    }

    pub fn object_id_capacity_hint(&self) -> io::Result<usize> {
        self.object_id_capacity_hint_inner(&mut HashSet::new())
    }

    pub fn loose_object_ids(&self) -> io::Result<Vec<ObjectId>> {
        let mut ids = Vec::new();
        self.collect_loose_object_ids(&mut ids)?;
        ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        ids.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
        Ok(ids)
    }

    pub fn prune_packed(&self, dry_run: bool) -> io::Result<Vec<ObjectId>> {
        let mut pruned = Vec::with_capacity(prune_packed_initial_capacity(
            self.packed_store.object_id_capacity_hint()?,
        ));
        let mut for_each = |id: &ObjectId| {
            let path = self.object_path(id)?;
            if !path.is_file() {
                return Ok(());
            }
            pruned.push(id.clone());
            if !dry_run {
                fs::remove_file(path)?;
            }
            Ok(())
        };
        self.packed_store.for_each_object_id(&mut for_each)?;
        Ok(pruned)
    }

    pub fn loose_object_path(&self, id: &ObjectId) -> io::Result<PathBuf> {
        self.object_path(id)
    }

    fn collect_loose_prefix(
        &self,
        hex_prefix: &str,
        resolved: &mut Option<ObjectId>,
    ) -> io::Result<()> {
        let dir = self.objects_dir.join(&hex_prefix[..2]);
        let suffix_prefix = &hex_prefix[2..];
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.len() + 2 != self.algorithm.digest_len() * 2
                || !name.starts_with(suffix_prefix)
                || !name.as_bytes().iter().all(u8::is_ascii_hexdigit)
            {
                continue;
            }
            record_prefix_candidate(
                resolved,
                loose_object_id_from_parts(self.algorithm, &hex_prefix[..2], name)?,
            )?;
        }
        Ok(())
    }

    fn collect_prefix_inner(
        &self,
        hex_prefix: &str,
        resolved: &mut Option<ObjectId>,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<()> {
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(());
        }
        self.collect_loose_prefix(hex_prefix, resolved)?;
        self.packed_store.collect_prefix(hex_prefix, resolved)?;
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .collect_prefix_inner(hex_prefix, resolved, visited)?;
        }
        Ok(())
    }

    fn collect_loose_object_ids(&self, ids: &mut Vec<ObjectId>) -> io::Result<()> {
        self.for_each_loose_object_id(&mut |id| {
            reserve_loose_object_id_growth(ids);
            ids.push(id.clone());
            Ok(())
        })
    }

    pub fn for_each_loose_object_id(
        &self,
        for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
    ) -> io::Result<()> {
        let entries = match fs::read_dir(&self.objects_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        let suffix_len = self.algorithm.digest_len() * 2 - 2;
        for entry in entries {
            let entry = entry?;
            let dir_name = entry.file_name();
            let Some(dir_name) = dir_name.to_str() else {
                continue;
            };
            if !is_loose_object_dir_name(dir_name) {
                continue;
            }
            let file_entries = match fs::read_dir(entry.path()) {
                Ok(entries) => entries,
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(err),
            };
            for file_entry in file_entries {
                let file_entry = file_entry?;
                if !file_entry.file_type()?.is_file() {
                    continue;
                }
                let file_name = file_entry.file_name();
                let Some(file_name) = file_name.to_str() else {
                    continue;
                };
                if file_name.len() != suffix_len
                    || !file_name.as_bytes().iter().all(u8::is_ascii_hexdigit)
                {
                    continue;
                }
                for_each(&loose_object_id_from_parts(
                    self.algorithm,
                    dir_name,
                    file_name,
                )?)?;
            }
        }
        Ok(())
    }

    fn collect_object_ids_inner(
        &self,
        ids: &mut Vec<ObjectId>,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<()> {
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(());
        }
        self.collect_loose_object_ids(ids)?;
        self.packed_store.for_each_object_id(&mut |id| {
            ids.push(id.clone());
            Ok(())
        })?;
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .collect_object_ids_inner(ids, visited)?;
        }
        Ok(())
    }

    fn collect_disk_object_entries_inner(
        &self,
        entries: &mut Vec<(ObjectId, u64)>,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<()> {
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(());
        }
        self.for_each_loose_object_id(&mut |id| {
            let size = fs::metadata(self.object_path(id)?)?.len();
            entries.push((id.clone(), size));
            Ok(())
        })?;
        self.packed_store.for_each_object_id(&mut |id| {
            let size = self.packed_store.object_disk_size_hint(id)?.unwrap_or(0);
            entries.push((id.clone(), size));
            Ok(())
        })?;
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .collect_disk_object_entries_inner(entries, visited)?;
        }
        Ok(())
    }

    fn object_id_capacity_hint_inner(&self, visited: &mut HashSet<PathBuf>) -> io::Result<usize> {
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(0);
        }

        let mut count = self.packed_store.object_id_capacity_hint()?;
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            count = count
                .checked_add(
                    Self::new(alternate, self.algorithm)
                        .with_max_object_bytes(self.max_object_bytes)
                        .object_id_capacity_hint_inner(visited)?,
                )
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "object id capacity hint overflows usize",
                    )
                })?;
        }
        Ok(count)
    }

    fn for_each_object_id_inner(
        &self,
        for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
        visited: &mut HashSet<PathBuf>,
        emitted: &mut HashSet<ObjectId>,
    ) -> io::Result<()> {
        let key = canonical_or_original(&self.objects_dir);
        if !visited.insert(key) {
            return Ok(());
        }
        let alternates = self.alternate_object_dirs()?;
        if emitted.is_empty() && alternates.is_empty() {
            return self.for_each_local_object_id(for_each);
        }
        self.for_each_loose_object_id(&mut |id| emit_unique_object_id(id, emitted, for_each))?;
        self.packed_store
            .for_each_object_id(&mut |id| emit_unique_object_id(id, emitted, for_each))?;
        for alternate in alternates.iter().cloned() {
            Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .for_each_object_id_inner(for_each, visited, emitted)?;
        }
        Ok(())
    }

    fn for_each_local_object_id(
        &self,
        for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
    ) -> io::Result<()> {
        if !self.packed_store.has_object_ids()? {
            return self.for_each_loose_object_id(for_each);
        }

        let mut loose_ids = Vec::new();
        self.for_each_loose_object_id(&mut |id| {
            for_each(id)?;
            loose_ids.push(id.clone());
            Ok(())
        })?;
        if loose_ids.is_empty() {
            return self.packed_store.for_each_object_id(for_each);
        }

        let mut emitted = loose_ids.into_iter().collect::<HashSet<_>>();
        self.packed_store
            .for_each_object_id(&mut |id| emit_unique_object_id(id, &mut emitted, for_each))
    }

    fn read_object_from_alternates(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<LooseObject> {
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            match Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .read_object_inner(id, visited)
            {
                Ok(object) => return Ok(object),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "git object not found",
        ))
    }

    fn read_tree_refs_from_alternates(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Vec<TreeObjectRef>> {
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            match Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .read_tree_refs_inner(id, visited)
            {
                Ok(entries) => return Ok(entries),
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "git object not found",
        ))
    }

    fn contains_object_in_alternates(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<bool> {
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            if Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .contains_object_inner(id, visited)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn object_kind_hint_from_alternates(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Option<GitObjectKind>> {
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            match Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .object_kind_hint_inner(id, visited)
            {
                Ok(Some(kind)) => return Ok(Some(kind)),
                Ok(None) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn delta_base_hint_from_alternates(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Option<ObjectId>> {
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            match Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .delta_base_hint_inner(id, visited)
            {
                Ok(Some(base)) => return Ok(Some(base)),
                Ok(None) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn object_disk_size_hint_from_alternates(
        &self,
        id: &ObjectId,
        visited: &mut HashSet<PathBuf>,
    ) -> io::Result<Option<u64>> {
        for alternate in self.alternate_object_dirs()?.iter().cloned() {
            match Self::new(alternate, self.algorithm)
                .with_max_object_bytes(self.max_object_bytes)
                .object_disk_size_hint_inner(id, visited)
            {
                Ok(Some(size)) => return Ok(Some(size)),
                Ok(None) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn alternate_object_dirs(&self) -> io::Result<Arc<Vec<PathBuf>>> {
        if let Some(cached) = self
            .alternates_cache
            .lock()
            .map_err(|_| io::Error::other("alternates cache mutex poisoned"))?
            .as_ref()
            .cloned()
        {
            return Ok(cached);
        }
        let path = self.objects_dir.join("info/alternates");
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let alternates = Arc::new(Vec::new());
                *self
                    .alternates_cache
                    .lock()
                    .map_err(|_| io::Error::other("alternates cache mutex poisoned"))? =
                    Some(Arc::clone(&alternates));
                return Ok(alternates);
            }
            Err(error) => return Err(error),
        };
        let mut dirs = Vec::new();
        let mut reader = alternates_file_reader(file);
        let mut line = alternate_line_buffer();
        while reader.read_line(&mut line)? != 0 {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                let path = PathBuf::from(trimmed);
                dirs.push(if path.is_absolute() {
                    path
                } else {
                    self.objects_dir.join(path)
                });
            }
            line.clear();
        }
        let alternates = Arc::new(dirs);
        *self
            .alternates_cache
            .lock()
            .map_err(|_| io::Error::other("alternates cache mutex poisoned"))? =
            Some(Arc::clone(&alternates));
        Ok(alternates)
    }

    fn object_path(&self, id: &ObjectId) -> io::Result<PathBuf> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        let mut hex = [0_u8; 64];
        let hex = object_id_hex_bytes(id, &mut hex)?;
        Ok(self.objects_dir.join(&hex[..2]).join(&hex[2..]))
    }
}

fn alternates_file_reader(file: fs::File) -> io::BufReader<fs::File> {
    io::BufReader::with_capacity(ALTERNATES_FILE_BUFFER_CAPACITY, file)
}

fn alternate_line_buffer() -> String {
    String::with_capacity(ALTERNATE_LINE_INITIAL_CAPACITY)
}

impl<R: Read, W: Write> Read for CopyingReader<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.reader.read(buf)?;
        if read != 0 {
            self.writer.write_all(&buf[..read])?;
        }
        Ok(read)
    }
}

impl<W: Write> LooseBlobContentWriter<W> {
    fn finish_with_id(self) -> io::Result<(W, ObjectId)> {
        let Self {
            inner,
            hasher,
            remaining,
        } = self;
        if remaining != 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "blob stream ended before declared size",
            ));
        }
        Ok((inner, hasher.finalize()))
    }

    fn finish(self, id: &ObjectId) -> io::Result<W> {
        let (inner, actual_id) = self.finish_with_id()?;
        if actual_id != *id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "streamed blob hash mismatch",
            ));
        }
        Ok(inner)
    }
}

impl<W: Write> Write for LooseBlobContentWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "blob stream exceeded declared size",
            ));
        }
        self.hasher.update(buf);
        self.inner.write_all(buf)?;
        self.remaining -= buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl GitObjectStore for LooseObjectStore {
    fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        Self::read_object(self, id)
    }

    fn object_id_capacity_hint(&self) -> io::Result<usize> {
        Self::object_id_capacity_hint(self)
    }

    fn object_kind_hint(&self, id: &ObjectId) -> io::Result<Option<GitObjectKind>> {
        Self::object_kind_hint(self, id)
    }

    fn object_header_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
        match self.loose_object_header_hint(id)? {
            Some(header) => Ok(Some(header)),
            None => self.packed_store.object_header_hint(id),
        }
    }

    fn object_storage_hint(&self, id: &ObjectId) -> io::Result<ObjectStorageHint> {
        if self.object_path(id)?.is_file() {
            return Ok(ObjectStorageHint::Loose);
        }
        if self.packed_store.contains_object(id)? {
            return Ok(ObjectStorageHint::Packed);
        }
        Ok(ObjectStorageHint::Unknown)
    }

    fn read_tree_refs(&self, id: &ObjectId) -> io::Result<Vec<TreeObjectRef>> {
        Self::read_tree_refs(self, id)
    }

    fn for_each_object_id(
        &self,
        for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
    ) -> io::Result<()> {
        self.for_each_object_id_inner(for_each, &mut HashSet::new(), &mut HashSet::new())
    }

    fn try_write_reusable_pack(
        &self,
        algorithm: GitHashAlgorithm,
        ids: &[ObjectId],
        writer: &mut dyn Write,
    ) -> io::Result<Option<ObjectId>> {
        let mut guard = || -> io::Result<bool> {
            for id in ids {
                if self.object_path(id)?.is_file() {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        self.packed_store
            .try_write_reusable_pack_with_guard(algorithm, ids, writer, &mut guard)
    }

    fn blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        match self.stream_loose_blob(id, None)? {
            Some(size) => Ok(Some(size)),
            None => self.packed_store.blob_size_hint(id),
        }
    }

    fn read_blob_prefix(&self, id: &ObjectId, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        match self.loose_blob_prefix(id, max_bytes)? {
            Some(prefix) => Ok(Some(prefix)),
            None => self.packed_store.read_blob_prefix(id, max_bytes),
        }
    }

    fn streamable_blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        match self.stream_loose_blob(id, None)? {
            Some(size) => Ok(Some(size)),
            None => self.packed_store.blob_size_hint(id),
        }
    }

    fn try_write_reusable_pack_object(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
    ) -> io::Result<bool> {
        if self.object_path(id)?.is_file() {
            return Ok(false);
        }
        self.packed_store.try_write_reusable_pack_object(id, writer)
    }

    fn try_write_reusable_pack_object_with_buffer(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
        buffer: &mut [u8],
    ) -> io::Result<bool> {
        if self.object_path(id)?.is_file() {
            return Ok(false);
        }
        self.packed_store
            .try_write_reusable_pack_object_with_buffer(id, writer, buffer)
    }

    fn write_streamable_blob(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        if self.stream_loose_blob(id, Some(&mut *writer))?.is_some() {
            return Ok(true);
        }
        self.packed_store.try_write_blob(id, writer)
    }
}

impl GitObjectStore for PackedFirstObjectStore<'_> {
    fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        self.inner.read_object_packed_first(id)
    }

    fn object_id_capacity_hint(&self) -> io::Result<usize> {
        self.inner.object_id_capacity_hint()
    }

    fn read_tree_refs(&self, id: &ObjectId) -> io::Result<Vec<TreeObjectRef>> {
        if id.algorithm() != self.inner.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        match self.inner.packed_store.read_tree_refs(id) {
            Ok(entries) => Ok(entries),
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.inner.read_tree_refs(id),
            Err(error) => Err(error),
        }
    }

    fn try_write_reusable_pack(
        &self,
        algorithm: GitHashAlgorithm,
        ids: &[ObjectId],
        writer: &mut dyn Write,
    ) -> io::Result<Option<ObjectId>> {
        self.inner
            .packed_store
            .try_write_reusable_pack(algorithm, ids, writer)
    }

    fn blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        self.inner.packed_store.blob_size_hint(id)
    }

    fn read_blob_prefix(&self, id: &ObjectId, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        match self.inner.packed_store.read_blob_prefix(id, max_bytes)? {
            Some(prefix) => Ok(Some(prefix)),
            None => self.inner.loose_blob_prefix(id, max_bytes),
        }
    }

    fn streamable_blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        match self.inner.packed_store.blob_size_hint(id)? {
            Some(size) => Ok(Some(size)),
            None => self.inner.streamable_blob_size_hint(id),
        }
    }

    fn object_kind_hint(&self, id: &ObjectId) -> io::Result<Option<GitObjectKind>> {
        match self.inner.packed_store.object_kind_hint(id)? {
            Some(kind) => Ok(Some(kind)),
            None => self.inner.object_kind_hint(id),
        }
    }

    fn object_header_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
        match self.inner.packed_store.object_header_hint(id)? {
            Some(header) => Ok(Some(header)),
            None => self.inner.object_header_hint(id),
        }
    }

    fn object_storage_hint(&self, id: &ObjectId) -> io::Result<ObjectStorageHint> {
        if self.inner.packed_store.contains_object(id)? {
            return Ok(ObjectStorageHint::Packed);
        }
        if self.inner.object_path(id)?.is_file() {
            return Ok(ObjectStorageHint::Loose);
        }
        Ok(ObjectStorageHint::Unknown)
    }

    fn try_write_reusable_pack_object(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
    ) -> io::Result<bool> {
        match self
            .inner
            .packed_store
            .try_write_reusable_pack_object(id, writer)
        {
            Ok(written) => Ok(written),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn try_write_reusable_pack_object_with_buffer(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
        buffer: &mut [u8],
    ) -> io::Result<bool> {
        match self
            .inner
            .packed_store
            .try_write_reusable_pack_object_with_buffer(id, writer, buffer)
        {
            Ok(written) => Ok(written),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn try_write_blob(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        match self.inner.packed_store.try_write_blob(id, writer) {
            Ok(written) => Ok(written),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn write_streamable_blob(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        match self.inner.packed_store.try_write_blob(id, writer) {
            Ok(true) => Ok(true),
            Ok(false) => match self.inner.read_object_packed_first(id) {
                Ok(object) if object.kind == GitObjectKind::Blob => {
                    writer.write_all(&object.content)?;
                    Ok(true)
                }
                Ok(_) => Ok(false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    self.inner.write_streamable_blob(id, writer)
                }
                Err(error) => Err(error),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.inner.write_streamable_blob(id, writer)
            }
            Err(error) => Err(error),
        }
    }

    fn try_write_blob_to_path(
        &self,
        id: &ObjectId,
        min_bytes: usize,
        path: &Path,
    ) -> io::Result<bool> {
        match self
            .inner
            .packed_store
            .try_write_blob_to_path(id, min_bytes, path)
        {
            Ok(written) => Ok(written),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

impl GitObjectSink for PackedFirstObjectStore<'_> {
    fn write_object(&self, kind: GitObjectKind, content: &[u8]) -> io::Result<ObjectId> {
        self.inner.write_object(kind, content)
    }

    fn write_streamed_blob_content<F>(&self, size: usize, write_content: F) -> io::Result<ObjectId>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        self.inner.write_streamed_blob_content(size, write_content)
    }
}

impl GitObjectSink for LooseObjectStore {
    fn write_object(&self, kind: GitObjectKind, content: &[u8]) -> io::Result<ObjectId> {
        Self::write_object(self, kind, content)
    }

    fn write_streamed_blob_content<F>(&self, size: usize, write_content: F) -> io::Result<ObjectId>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        Self::write_streamed_blob_content(self, size, write_content)
    }
}

fn object_id_initial_capacity(object_hint: usize) -> usize {
    object_hint.min(OBJECT_ID_INITIAL_CAPACITY_LIMIT)
}

fn prune_packed_initial_capacity(object_hint: usize) -> usize {
    object_id_initial_capacity(object_hint)
}

fn reserve_loose_object_id_growth(ids: &mut Vec<ObjectId>) {
    if ids.capacity() == ids.len() {
        ids.reserve(loose_object_id_growth_capacity(ids.len()));
    }
}

fn loose_object_id_growth_capacity(current_len: usize) -> usize {
    current_len
        .max(1024)
        .saturating_mul(2)
        .min(LOOSE_OBJECT_ID_GROWTH_CAPACITY_LIMIT)
}

fn emit_unique_object_id(
    id: &ObjectId,
    emitted: &mut HashSet<ObjectId>,
    for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
) -> io::Result<()> {
    if emitted.insert(id.clone()) {
        for_each(id)?;
    }
    Ok(())
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn read_loose_object_header(reader: &mut impl Read) -> io::Result<(GitObjectKind, usize)> {
    const MAX_HEADER_LEN: usize = 128;
    let mut header = Vec::with_capacity(32);
    let mut byte = [0_u8; 1];
    loop {
        if header.len() >= MAX_HEADER_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object header is too large",
            ));
        }
        reader.read_exact(&mut byte).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                io::Error::new(io::ErrorKind::InvalidData, "git object header missing NUL")
            } else {
                error
            }
        })?;
        if byte[0] == 0 {
            break;
        }
        header.push(byte[0]);
    }
    let space = header
        .iter()
        .position(|byte| *byte == b' ')
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object header is malformed",
            )
        })?;
    if space > 32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "object type header too long",
        ));
    }
    let kind = GitObjectKind::parse(&header[..space])
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid object type"))?;
    let size = parse_loose_object_size(&header[space + 1..])?;
    Ok((kind, size))
}

fn verify_loose_object_copy<R: Read>(
    reader: R,
    max_object_bytes: usize,
    id: &ObjectId,
) -> io::Result<()> {
    verify_loose_object_kind(reader, max_object_bytes, id).map(|_| ())
}

fn verify_loose_object_kind<R: Read>(
    reader: R,
    max_object_bytes: usize,
    id: &ObjectId,
) -> io::Result<GitObjectKind> {
    let mut decoder = ZlibDecoder::new(reader);
    let (kind, size) = read_loose_object_header(&mut decoder)?;
    if size > max_object_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loose git object exceeds configured size limit",
        ));
    }
    let mut hasher = GitObjectHash::new(id.algorithm());
    hasher.update_object_header(kind, size);
    let mut remaining = size;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let read_len = remaining.min(buffer.len());
        let read = decoder.read(&mut buffer[..read_len])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "loose git object ended before declared size",
            ));
        }
        hasher.update(&buffer[..read]);
        remaining -= read;
    }
    let mut trailing = [0_u8; 1];
    if decoder.read(&mut trailing)? != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loose git object has trailing content",
        ));
    }
    if hasher.finalize() != *id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loose git object hash mismatch",
        ));
    }
    Ok(kind)
}

fn temp_object_path(parent: &Path, id: &ObjectId) -> PathBuf {
    parent.join(format!(
        "tmp_obj_{}_{}_{}",
        std::process::id(),
        TEMP_OBJECT_COUNTER.fetch_add(1, Ordering::Relaxed),
        &id.short_hex(14)[2..]
    ))
}

fn temp_unknown_object_path(parent: &Path) -> PathBuf {
    parent.join(format!(
        "tmp_obj_{}_{}_stream",
        std::process::id(),
        TEMP_OBJECT_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn object_id_hex_bytes<'a>(id: &ObjectId, buffer: &'a mut [u8; 64]) -> io::Result<&'a str> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let len = id.hex_len();
    for (idx, byte) in id.as_bytes().iter().enumerate() {
        buffer[idx * 2] = HEX[(byte >> 4) as usize];
        buffer[idx * 2 + 1] = HEX[(byte & 0x0f) as usize];
    }
    std::str::from_utf8(&buffer[..len])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid object id hex"))
}

fn object_fanout_index(id: &ObjectId) -> io::Result<usize> {
    id.as_bytes()
        .first()
        .copied()
        .map(usize::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty git object id"))
}

fn install_temp_object_file(tmp_path: &Path, path: &Path) -> io::Result<()> {
    match fs::hard_link(tmp_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(tmp_path);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(tmp_path);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(tmp_path);
            Err(error)
        }
    }
}

fn write_temp_object(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)
}

pub fn encode_loose_object(kind: GitObjectKind, content: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), loose_object_compression());
    write_loose_object_header(&mut encoder, kind, content.len())?;
    encoder.write_all(content)?;
    encoder.finish()
}

fn write_loose_object_header<W: Write>(
    writer: &mut W,
    kind: GitObjectKind,
    size: usize,
) -> io::Result<()> {
    let mut header = [0_u8; 64];
    let kind_bytes = kind.as_bytes();
    let mut len = kind_bytes.len();
    header[..len].copy_from_slice(kind_bytes);
    header[len] = b' ';
    len += 1;
    len += write_decimal_usize(&mut header[len..], size);
    header[len] = 0;
    len += 1;
    writer.write_all(&header[..len])
}

fn write_decimal_usize(buffer: &mut [u8], value: usize) -> usize {
    let mut digits = [0_u8; 20];
    let mut value = value;
    let mut len = 0;
    loop {
        digits[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for idx in 0..len {
        buffer[idx] = digits[len - idx - 1];
    }
    len
}

fn loose_object_compression() -> Compression {
    Compression::fast()
}

#[cfg(test)]
fn read_loose_object_content(
    reader: impl Read,
    max_object_bytes: usize,
) -> io::Result<(GitObjectKind, Vec<u8>)> {
    let mut decoder = ZlibDecoder::new(reader);
    let (kind, size) = read_loose_object_header(&mut decoder)?;
    if size > max_object_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loose git object exceeds configured size limit",
        ));
    }
    let content = read_exact_loose_object_content(&mut decoder, size)?;
    Ok((kind, content))
}

fn read_verified_loose_object_content(
    reader: impl Read,
    algorithm: GitHashAlgorithm,
    expected_id: &ObjectId,
    max_object_bytes: usize,
) -> io::Result<(GitObjectKind, Vec<u8>)> {
    let mut decoder = ZlibDecoder::new(reader);
    let (kind, size) = read_loose_object_header(&mut decoder)?;
    if size > max_object_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loose git object exceeds configured size limit",
        ));
    }
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update_object_header(kind, size);
    let content = read_exact_loose_object_content_with_chunks(&mut decoder, size, |chunk| {
        hasher.update(chunk);
        Ok(())
    })?;
    if hasher.finalize() != *expected_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loose git object hash mismatch",
        ));
    }
    Ok((kind, content))
}

#[cfg(test)]
fn read_exact_loose_object_content<R: Read>(
    decoder: &mut ZlibDecoder<R>,
    size: usize,
) -> io::Result<Vec<u8>> {
    read_exact_loose_object_content_with_chunks(decoder, size, |_| Ok(()))
}

fn read_exact_loose_object_content_with_chunks<R: Read>(
    decoder: &mut ZlibDecoder<R>,
    size: usize,
    mut on_chunk: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<Vec<u8>> {
    let mut content = Vec::with_capacity(loose_object_content_initial_capacity(size));
    let mut buffer = [0_u8; 64 * 1024];
    let mut remaining = size;
    while remaining > 0 {
        let want = remaining.min(buffer.len());
        let len = decoder.read(&mut buffer[..want])?;
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose git object ended before declared size",
            ));
        }
        on_chunk(&buffer[..len])?;
        content.extend_from_slice(&buffer[..len]);
        remaining -= len;
    }

    {
        let mut extra = [0_u8; 1];
        if decoder.read(&mut extra)? == 0 {
            return Ok(content);
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "loose git object size does not match header",
        ))
    }
}

fn loose_object_content_initial_capacity(size: usize) -> usize {
    size.min(LOOSE_OBJECT_CONTENT_INITIAL_CAPACITY_LIMIT)
}

fn parse_loose_object_size(bytes: &[u8]) -> io::Result<usize> {
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty git object size",
        ));
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git object size has leading zero",
        ));
    }
    let mut value = 0_usize;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid git object size digit",
            ));
        }
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add((byte - b'0') as usize))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "git object size overflows usize",
                )
            })?;
    }
    Ok(value)
}

fn is_loose_object_dir_name(value: &str) -> bool {
    value.len() == 2 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

pub(crate) fn validate_hex_prefix(hex_prefix: &str, algorithm: GitHashAlgorithm) -> io::Result<()> {
    let max = algorithm.digest_len() * 2;
    if hex_prefix.len() < 4 || hex_prefix.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git object id prefix has the wrong length",
        ));
    }
    if !hex_prefix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git object id prefix contains non-hex characters",
        ));
    }
    Ok(())
}

pub(crate) fn record_prefix_candidate(
    resolved: &mut Option<ObjectId>,
    candidate: ObjectId,
) -> io::Result<()> {
    match resolved {
        Some(existing) if existing != &candidate => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "short git object id is ambiguous",
        )),
        Some(_) => Ok(()),
        None => {
            *resolved = Some(candidate);
            Ok(())
        }
    }
}

fn loose_object_id_from_parts(
    algorithm: GitHashAlgorithm,
    dir_name: &str,
    file_name: &str,
) -> io::Result<ObjectId> {
    let hex_len = algorithm.digest_len() * 2;
    let mut hex_id = [0_u8; 64];
    hex_id[..2].copy_from_slice(dir_name.as_bytes());
    hex_id[2..hex_len].copy_from_slice(file_name.as_bytes());
    ObjectId::from_hex_bytes(algorithm, &hex_id[..hex_len])
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn writes_loose_blob_readable_by_stock_git() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"hello loose object\n")
            .expect("write loose object");

        let kind = git(&repo, ["cat-file", "-t", &id.to_hex()]);
        let content = git_raw(&repo, ["cat-file", "-p", &id.to_hex()]);

        assert_eq!(kind, "blob");
        assert_eq!(content, b"hello loose object\n");
    }

    #[test]
    fn reads_loose_blob_written_by_stock_git() {
        let repo = git_init();
        let id = git_hash_object_write(&repo, b"from git\n");
        let object_id = ObjectId::new(GitHashAlgorithm::Sha1, &hex::decode(id).expect("hex id"));
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        let object = store.read_object(&object_id).expect("read loose object");

        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, b"from git\n");
    }

    #[test]
    fn loose_object_kind_hint_reads_header_without_materializing_content() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"hinted loose object\n")
            .expect("write object");

        assert_eq!(
            store.object_kind_hint(&id).expect("kind hint"),
            Some(GitObjectKind::Blob)
        );
    }

    #[test]
    fn loose_object_reader_enforces_declared_size() {
        let encoded = encode_loose_object(GitObjectKind::Blob, b"abc").expect("encode object");
        let (kind, content) =
            read_loose_object_content(encoded.as_slice(), 1024).expect("read object");
        assert_eq!(kind, GitObjectKind::Blob);
        assert_eq!(content, b"abc");

        let mut malformed = ZlibEncoder::new(Vec::new(), Compression::default());
        malformed
            .write_all(b"blob 3\0abcd")
            .expect("write malformed object");
        let malformed = malformed.finish().expect("finish malformed object");

        let error =
            read_loose_object_content(malformed.as_slice(), 1024).expect_err("trailing content");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn temp_object_write_refuses_existing_path() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("tmp_obj");
        std::fs::write(&path, b"existing").expect("write existing temp");

        let error = write_temp_object(&path, b"replacement").expect_err("write should fail");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).expect("read temp"), b"existing");
    }

    #[test]
    fn copies_loose_object_without_reencoding() {
        let source_repo = git_init();
        let destination_repo = git_init();
        let id = git_hash_object_write(&source_repo, b"copy me without reencoding\n");
        let object_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &id).expect("object id");
        let source = LooseObjectStore::new(
            source_repo.path().join(".git/objects"),
            GitHashAlgorithm::Sha1,
        );
        let destination = LooseObjectStore::new(
            destination_repo.path().join(".git/objects"),
            GitHashAlgorithm::Sha1,
        );
        let source_path = source.object_path(&object_id).expect("source path");
        let source_bytes = std::fs::read(&source_path).expect("read source loose object");

        assert!(
            source
                .copy_loose_object_to(&destination, &object_id)
                .expect("copy loose object")
        );

        let destination_path = destination
            .object_path(&object_id)
            .expect("destination path");
        assert_eq!(
            std::fs::read(&destination_path).expect("read destination loose object"),
            source_bytes
        );
        assert_eq!(
            destination
                .read_object(&object_id)
                .expect("read copied object")
                .content,
            b"copy me without reencoding\n"
        );
    }

    #[test]
    fn copy_loose_object_skips_source_open_when_destination_exists() {
        let source_repo = git_init();
        let destination_repo = git_init();
        let id = git_hash_object_write(&destination_repo, b"already copied\n");
        let object_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &id).expect("object id");
        let source = LooseObjectStore::new(
            source_repo.path().join(".git/objects"),
            GitHashAlgorithm::Sha1,
        );
        let destination = LooseObjectStore::new(
            destination_repo.path().join(".git/objects"),
            GitHashAlgorithm::Sha1,
        );

        assert!(
            source
                .copy_loose_object_to(&destination, &object_id)
                .expect("destination object should make copy a no-op")
        );
    }

    #[test]
    fn writes_streamed_blob_as_loose_object() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let content = b"streamed destination blob\n";
        let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, content);

        store
            .write_streamed_blob(&id, content.len(), |writer| writer.write_all(content))
            .expect("write streamed blob");

        let object = store.read_object(&id).expect("read streamed blob");
        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, content);
    }

    #[test]
    fn reads_loose_blob_prefix_without_full_content() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let content = b"prefix-content-tail";
        let id = git_hash_object_write(&repo, content);
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &id).expect("object id");

        assert_eq!(
            store
                .read_blob_prefix(&id, 6)
                .expect("read blob prefix")
                .as_deref(),
            Some(b"prefix".as_slice())
        );
    }

    #[test]
    fn writes_streamed_blob_content_with_computed_id() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let content = b"streamed source blob\n";

        let id = store
            .write_streamed_blob_content(content.len(), |writer| writer.write_all(content))
            .expect("write streamed blob content");

        assert_eq!(
            id,
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, content)
        );
        let object = store.read_object(&id).expect("read streamed blob");
        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, content);
    }

    #[test]
    fn streamed_blob_content_handles_existing_object_without_leaking_temp() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let content = b"duplicate streamed source blob\n";
        let existing = store
            .write_object(GitObjectKind::Blob, content)
            .expect("write existing blob");

        let streamed = store
            .write_streamed_blob_content(content.len(), |writer| writer.write_all(content))
            .expect("write duplicate streamed blob content");

        assert_eq!(streamed, existing);
        assert_eq!(
            store.loose_object_ids().expect("list loose objects"),
            vec![existing]
        );
        assert!(
            fs::read_dir(store.objects_dir())
                .expect("read objects dir")
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().starts_with("tmp_obj_")),
            "duplicate streamed write should remove its unknown temp file"
        );
    }

    #[test]
    fn write_object_streams_large_blob_without_changing_id() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let content = vec![b'a'; STREAM_LOOSE_BLOB_WRITE_MIN_BYTES + 1];
        let expected = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content);

        let id = store
            .write_object(GitObjectKind::Blob, &content)
            .expect("write large blob");

        assert_eq!(id, expected);
        let object = store.read_object(&id).expect("read large blob");
        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, content);
    }

    #[test]
    fn streamed_blob_content_rejects_short_input() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        let error = store
            .write_streamed_blob_content(4, |writer| writer.write_all(b"abc"))
            .expect_err("short blob should fail");

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(
            store.loose_object_ids().expect("list loose objects"),
            Vec::new()
        );
    }

    #[test]
    fn object_ids_initial_capacity_is_bounded() {
        assert_eq!(object_id_initial_capacity(usize::MAX), 8192);
        assert_eq!(object_id_initial_capacity(2), 2);
        assert_eq!(object_id_initial_capacity(0), 0);
    }

    #[test]
    fn prune_packed_initial_capacity_is_bounded() {
        assert_eq!(prune_packed_initial_capacity(usize::MAX), 8192);
        assert_eq!(prune_packed_initial_capacity(2), 2);
        assert_eq!(prune_packed_initial_capacity(0), 0);
    }

    #[test]
    fn loose_object_id_growth_capacity_is_bounded() {
        assert_eq!(
            loose_object_id_growth_capacity(usize::MAX),
            LOOSE_OBJECT_ID_GROWTH_CAPACITY_LIMIT
        );
        assert_eq!(loose_object_id_growth_capacity(0), 2048);
        assert_eq!(loose_object_id_growth_capacity(10), 2048);
    }

    #[test]
    fn collect_loose_object_ids_reserves_growth_without_capacity_hint() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"loose")
            .expect("write loose object");
        let mut ids = Vec::new();

        store
            .collect_loose_object_ids(&mut ids)
            .expect("collect loose ids");

        assert_eq!(ids, vec![id]);
        assert!(ids.capacity() >= loose_object_id_growth_capacity(0));
    }

    #[test]
    fn loose_object_content_initial_capacity_is_bounded() {
        assert_eq!(
            loose_object_content_initial_capacity(usize::MAX),
            LOOSE_OBJECT_CONTENT_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(loose_object_content_initial_capacity(2), 2);
        assert_eq!(loose_object_content_initial_capacity(0), 0);
    }

    #[test]
    fn prunes_loose_objects_that_exist_in_pack() {
        let repo = git_init();
        git_env(&repo, ["commit", "--allow-empty", "-m", "initial"]);
        let id = git(&repo, ["rev-parse", "HEAD"]);
        let loose_path = repo
            .path()
            .join(".git/objects")
            .join(&id[..2])
            .join(&id[2..]);
        let copy_path = repo.path().join("duplicate-head-copy");
        std::fs::copy(&loose_path, &copy_path).expect("copy loose commit");
        git(&repo, ["repack", "-adq"]);
        std::fs::create_dir_all(loose_path.parent().expect("loose parent"))
            .expect("create loose dir");
        std::fs::copy(&copy_path, &loose_path).expect("restore loose commit");

        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let object_id = ObjectId::new(GitHashAlgorithm::Sha1, &hex::decode(id).expect("hex id"));
        let dry_pruned = store.prune_packed(true).expect("dry prune");
        assert_eq!(dry_pruned, vec![object_id.clone()]);
        assert!(loose_path.is_file());

        assert_eq!(
            store.prune_packed(false).expect("prune"),
            vec![object_id.clone()]
        );
        assert!(!loose_path.exists());
        assert_eq!(
            store.read_object(&object_id).expect("read packed").kind,
            GitObjectKind::Commit
        );
    }

    #[test]
    fn reads_objects_from_alternate_object_database() {
        let alternate = git_init();
        let local = git_init();
        let blob = git_hash_object_write(&alternate, b"from alternate\n");
        std::fs::write(
            local.path().join(".git/objects/info/alternates"),
            format!("{}\n", alternate.path().join(".git/objects").display()),
        )
        .expect("write alternates");

        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &blob).expect("blob id");
        let store =
            LooseObjectStore::new(local.path().join(".git/objects"), GitHashAlgorithm::Sha1);

        let object = store.read_object(&id).expect("read alternate object");

        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, b"from alternate\n");
        assert_eq!(
            store
                .resolve_prefix(&blob[..8])
                .expect("resolve alternate prefix"),
            id
        );
        assert!(store.object_ids().expect("object ids").contains(&id));
    }

    #[test]
    fn for_each_object_id_deduplicates_loose_packed_and_alternate_ids() {
        let alternate = git_init();
        let local = git_init();
        for repo in [&alternate, &local] {
            std::fs::write(repo.path().join("README.md"), b"same content\n")
                .expect("write tracked file");
            git(repo, ["add", "README.md"]);
            git_env(repo, ["commit", "-m", "same commit"]);
        }
        let id = git(&local, ["rev-parse", "HEAD"]);
        assert_eq!(git(&alternate, ["rev-parse", "HEAD"]), id);

        let loose_path = local
            .path()
            .join(".git/objects")
            .join(&id[..2])
            .join(&id[2..]);
        let loose_copy_path = local.path().join("duplicate-head-copy");
        std::fs::copy(&loose_path, &loose_copy_path).expect("copy loose commit");
        git(&local, ["repack", "-adq"]);
        std::fs::create_dir_all(loose_path.parent().expect("loose parent"))
            .expect("create loose dir");
        std::fs::copy(&loose_copy_path, &loose_path).expect("restore loose commit");
        std::fs::write(
            local.path().join(".git/objects/info/alternates"),
            format!("{}\n", alternate.path().join(".git/objects").display()),
        )
        .expect("write alternates");

        let store =
            LooseObjectStore::new(local.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let target = ObjectId::from_hex(GitHashAlgorithm::Sha1, &id).expect("object id");
        let mut ids = Vec::new();
        store
            .for_each_object_id(&mut |id| {
                ids.push(id.clone());
                Ok(())
            })
            .expect("iterate object ids");
        let unique = ids.iter().cloned().collect::<HashSet<_>>();

        assert_eq!(ids.len(), unique.len());
        assert_eq!(ids.iter().filter(|id| *id == &target).count(), 1);
    }

    #[test]
    fn alternate_object_dirs_reader_streams_lines_and_resolves_relative_paths() {
        let repo = git_init();
        let objects_dir = repo.path().join(".git/objects");
        let relative = PathBuf::from("../alternate/objects");
        std::fs::write(
            objects_dir.join("info/alternates"),
            format!(
                "\n# ignored\n{}\n{}\r\n",
                relative.display(),
                repo.path().join("absolute/objects").display()
            ),
        )
        .expect("write alternates");
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);

        let alternates = store.alternate_object_dirs().expect("read alternates");

        assert_eq!(
            alternates.as_ref(),
            &vec![
                objects_dir.join(relative),
                repo.path().join("absolute/objects")
            ]
        );
    }

    #[test]
    fn alternate_object_dirs_reader_uses_explicit_buffer_capacity() {
        let file = tempfile::tempfile().expect("temp alternates file");
        let reader = alternates_file_reader(file);
        let line = alternate_line_buffer();

        assert_eq!(reader.capacity(), ALTERNATES_FILE_BUFFER_CAPACITY);
        assert_eq!(line.capacity(), ALTERNATE_LINE_INITIAL_CAPACITY);
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
        String::from_utf8(git_raw(repo, args))
            .expect("git stdout utf8")
            .trim_end_matches('\n')
            .to_owned()
    }

    fn git_raw<const N: usize>(repo: &TempDir, args: [&str; N]) -> Vec<u8> {
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
        output.stdout
    }

    fn git_env<const N: usize>(repo: &TempDir, args: [&str; N]) {
        let output = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(repo.path())
            .env("GIT_AUTHOR_NAME", "Zmin")
            .env("GIT_AUTHOR_EMAIL", "zmin@example.com")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_NAME", "Zmin")
            .env("GIT_COMMITTER_EMAIL", "zmin@example.com")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_hash_object_write(repo: &TempDir, content: &[u8]) -> String {
        use std::io::Write as _;
        use std::process::Stdio;

        let mut child = Command::new("git")
            .args(["hash-object", "-w", "--stdin"])
            .current_dir(repo.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn git hash-object");
        child
            .stdin
            .as_mut()
            .expect("git stdin")
            .write_all(content)
            .expect("write git stdin");
        let output = child.wait_with_output().expect("wait git hash-object");
        assert!(
            output.status.success(),
            "git hash-object failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git stdout utf8")
            .trim()
            .to_owned()
    }
}
