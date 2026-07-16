use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use zmin_git_core::{
    GitHashAlgorithm, GitObjectKind, GitObjectStore, LooseObject, LooseObjectStore, ObjectId,
    decode_tree, encode_tree, hash_object,
};

use super::{
    GitRepo, ResolvedObjectish, local_config_path, read_config_file,
    repo_hash_algorithm_from_config, resolve_objectish, resolve_objectish_with_mode,
};

const LOOSE_OBJECT_INDEX_HEADER: &str = "# loose-object-idx";

pub(crate) struct ObjectFormatTranslator {
    objects_dir: PathBuf,
    storage_algorithm: GitHashAlgorithm,
    compat_algorithm: Option<GitHashAlgorithm>,
    store: LooseObjectStore,
    storage_to_compat: HashMap<ObjectId, ObjectId>,
    compat_to_storage: HashMap<ObjectId, ObjectId>,
    pending: Vec<(ObjectId, ObjectId)>,
    translating: HashSet<ObjectId>,
}

pub(crate) fn resolve_compatible_objectish(
    repo: &GitRepo,
    translator: &ObjectFormatTranslator,
    value: &str,
) -> io::Result<ObjectId> {
    if let Some(id) = translator.resolve_full_hex(value)? {
        return Ok(id);
    }
    resolve_objectish(repo, value)
}

pub(crate) fn resolve_compatible_objectish_with_mode(
    repo: &GitRepo,
    translator: &ObjectFormatTranslator,
    value: &str,
) -> io::Result<ResolvedObjectish> {
    if let Some(id) = translator.resolve_full_hex(value)? {
        return Ok(ResolvedObjectish { id, mode: None });
    }
    resolve_objectish_with_mode(repo, value)
}

impl ObjectFormatTranslator {
    pub(crate) fn new(repo: &GitRepo) -> io::Result<Self> {
        let storage_algorithm = repo_hash_algorithm_from_config(repo)?;
        let compat_algorithm = repo_compat_hash_algorithm_from_config(repo)?;
        let objects_dir = repo.objects_dir.clone();
        let (storage_to_compat, compat_to_storage) = load_loose_object_index(
            &objects_dir.join("loose-object-idx"),
            storage_algorithm,
            compat_algorithm,
        )?;
        Ok(Self {
            store: LooseObjectStore::new(&objects_dir, storage_algorithm),
            objects_dir,
            storage_algorithm,
            compat_algorithm,
            storage_to_compat,
            compat_to_storage,
            pending: Vec::new(),
            translating: HashSet::new(),
        })
    }

    pub(crate) const fn storage_algorithm(&self) -> GitHashAlgorithm {
        self.storage_algorithm
    }

    pub(crate) const fn compat_algorithm(&self) -> Option<GitHashAlgorithm> {
        self.compat_algorithm
    }

    pub(crate) fn resolve_full_hex(&self, value: &str) -> io::Result<Option<ObjectId>> {
        let algorithm = match value.len() {
            40 => GitHashAlgorithm::Sha1,
            64 => GitHashAlgorithm::Sha256,
            _ => return Ok(None),
        };
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Ok(None);
        }
        let id = ObjectId::from_hex(algorithm, value)?;
        if algorithm == self.storage_algorithm {
            return self
                .store
                .contains_object(&id)
                .map(|exists| exists.then_some(id));
        }
        if Some(algorithm) == self.compat_algorithm && self.compat_to_storage.contains_key(&id) {
            return Ok(Some(id));
        }
        Ok(None)
    }

    pub(crate) fn translate_id(
        &mut self,
        id: &ObjectId,
        target_algorithm: GitHashAlgorithm,
    ) -> io::Result<ObjectId> {
        let translated = self.translate_id_inner(id, target_algorithm)?;
        self.persist_pending()?;
        Ok(translated)
    }

    pub(crate) fn storage_id(&self, id: &ObjectId) -> io::Result<ObjectId> {
        if id.algorithm() == self.storage_algorithm {
            return Ok(id.clone());
        }
        if Some(id.algorithm()) != self.compat_algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm is not configured for this repository",
            ));
        }
        self.compat_to_storage.get(id).cloned().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "compatible object id is not mapped",
            )
        })
    }

    pub(crate) fn read_object(&mut self, id: &ObjectId) -> io::Result<LooseObject> {
        let storage_id = self.storage_id(id)?;
        let object = self.store.packed_first().read_object(&storage_id)?;
        if id.algorithm() == self.storage_algorithm {
            return Ok(object);
        }
        let content = self.translate_content(object.kind, &object.content, id.algorithm())?;
        let translated_id = hash_object(id.algorithm(), object.kind, &content);
        if translated_id != *id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "compatible object mapping does not match translated object content",
            ));
        }
        self.persist_pending()?;
        Ok(LooseObject {
            id: translated_id,
            kind: object.kind,
            content,
        })
    }

    fn translate_id_inner(
        &mut self,
        id: &ObjectId,
        target_algorithm: GitHashAlgorithm,
    ) -> io::Result<ObjectId> {
        if id.algorithm() == target_algorithm {
            return Ok(id.clone());
        }
        if target_algorithm == self.storage_algorithm {
            return self.storage_id(id);
        }
        if Some(target_algorithm) != self.compat_algorithm
            || id.algorithm() != self.storage_algorithm
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "requested object format is not configured for this repository",
            ));
        }
        if let Some(mapped) = self.storage_to_compat.get(id) {
            return Ok(mapped.clone());
        }
        if !self.translating.insert(id.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "object-format translation cycle detected",
            ));
        }
        let result = (|| {
            let object = self.store.packed_first().read_object(id)?;
            let content = self.translate_content(object.kind, &object.content, target_algorithm)?;
            let translated = hash_object(target_algorithm, object.kind, &content);
            self.storage_to_compat
                .insert(id.clone(), translated.clone());
            self.compat_to_storage
                .insert(translated.clone(), id.clone());
            self.pending.push((id.clone(), translated.clone()));
            Ok(translated)
        })();
        self.translating.remove(id);
        result
    }

    fn translate_content(
        &mut self,
        kind: GitObjectKind,
        content: &[u8],
        target_algorithm: GitHashAlgorithm,
    ) -> io::Result<Vec<u8>> {
        match kind {
            GitObjectKind::Blob => Ok(content.to_vec()),
            GitObjectKind::Tree => {
                let mut entries = decode_tree(self.storage_algorithm, content)?;
                for entry in &mut entries {
                    entry.id = self.translate_id_inner(&entry.id, target_algorithm)?;
                }
                encode_tree(&entries)
            }
            GitObjectKind::Commit => self.translate_header_object_ids(
                content,
                target_algorithm,
                &[b"tree ".as_slice(), b"parent ".as_slice()],
            ),
            GitObjectKind::Tag => self.translate_header_object_ids(
                content,
                target_algorithm,
                &[b"object ".as_slice()],
            ),
        }
    }

    fn translate_header_object_ids(
        &mut self,
        content: &[u8],
        target_algorithm: GitHashAlgorithm,
        prefixes: &[&[u8]],
    ) -> io::Result<Vec<u8>> {
        let header_end = content
            .windows(2)
            .position(|window| window == b"\n\n")
            .map(|position| position + 2)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "object header is incomplete")
            })?;
        let mut translated = Vec::with_capacity(content.len() + 32);
        for line in content[..header_end].split_inclusive(|byte| *byte == b'\n') {
            let (body, newline) = line
                .strip_suffix(b"\n")
                .map_or((line, false), |body| (body, true));
            let mut replaced = false;
            for prefix in prefixes {
                let Some(raw_id) = body.strip_prefix(*prefix) else {
                    continue;
                };
                if raw_id.len() != self.storage_algorithm.digest_len() * 2 {
                    continue;
                }
                let raw_id = std::str::from_utf8(raw_id).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "object header id is not UTF-8")
                })?;
                let id = ObjectId::from_hex(self.storage_algorithm, raw_id)?;
                let id = self.translate_id_inner(&id, target_algorithm)?;
                translated.extend_from_slice(prefix);
                id.write_hex_bytes(&mut translated);
                replaced = true;
                break;
            }
            if !replaced {
                translated.extend_from_slice(body);
            }
            if newline {
                translated.push(b'\n');
            }
        }
        translated.extend_from_slice(&content[header_end..]);
        Ok(translated)
    }

    fn persist_pending(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.objects_dir)?;
        let path = self.objects_dir.join("loose-object-idx");
        let lock_path = self.objects_dir.join("loose-object-idx.lock");
        let mut lock = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)?;
        let result = (|| {
            let (mut current, _) =
                load_loose_object_index(&path, self.storage_algorithm, self.compat_algorithm)?;
            for (storage, compat) in self.pending.drain(..) {
                current.insert(storage, compat);
            }
            let mut rows = current.into_iter().collect::<Vec<_>>();
            rows.sort_by(|left, right| left.0.to_hex().cmp(&right.0.to_hex()));
            writeln!(lock, "{LOOSE_OBJECT_INDEX_HEADER}")?;
            for (storage, compat) in rows {
                writeln!(lock, "{} {}", storage.to_hex(), compat.to_hex())?;
            }
            lock.flush()?;
            lock.sync_all()?;
            fs::rename(&lock_path, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&lock_path);
        }
        result
    }
}

pub(crate) fn repo_compat_hash_algorithm_from_config(
    repo: &GitRepo,
) -> io::Result<Option<GitHashAlgorithm>> {
    let value = read_config_file(&local_config_path(repo)?)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "extensions"
                && entry.subsection.is_empty()
                && entry.key == "compatobjectformat"
        })
        .map(|entry| entry.value);
    let algorithm = match value.as_deref() {
        None => return Ok(None),
        Some("sha1") => GitHashAlgorithm::Sha1,
        Some("sha256") => GitHashAlgorithm::Sha256,
        Some(value) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid compatible object format '{value}'"),
            ));
        }
    };
    if algorithm == repo_hash_algorithm_from_config(repo)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "compatible object format matches storage object format",
        ));
    }
    Ok(Some(algorithm))
}

fn load_loose_object_index(
    path: &Path,
    storage_algorithm: GitHashAlgorithm,
    compat_algorithm: Option<GitHashAlgorithm>,
) -> io::Result<(HashMap<ObjectId, ObjectId>, HashMap<ObjectId, ObjectId>)> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((HashMap::new(), HashMap::new()));
        }
        Err(error) => return Err(error),
    };
    let compat_algorithm = compat_algorithm.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "loose-object-idx exists without a compatible object format",
        )
    })?;
    let mut lines = text.lines();
    if lines.next() != Some(LOOSE_OBJECT_INDEX_HEADER) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid loose-object-idx header",
        ));
    }
    let mut storage_to_compat = HashMap::new();
    let mut compat_to_storage = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_ascii_whitespace();
        let storage = fields.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing storage object id")
        })?;
        let compat = fields.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing compatible object id")
        })?;
        if fields.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "loose-object-idx row has extra fields",
            ));
        }
        let storage = ObjectId::from_hex(storage_algorithm, storage)?;
        let compat = ObjectId::from_hex(compat_algorithm, compat)?;
        storage_to_compat.insert(storage.clone(), compat.clone());
        compat_to_storage.insert(compat, storage);
    }
    Ok((storage_to_compat, compat_to_storage))
}
