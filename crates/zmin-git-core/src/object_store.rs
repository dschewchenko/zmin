use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use crate::{
    GitHashAlgorithm, GitObjectKind, LooseObject, ObjectId, TreeObjectRef, decode_tree_object_refs,
    hash_object,
};

const STREAMED_BLOB_INITIAL_CAPACITY_LIMIT: usize = 8192;
const IN_MEMORY_OBJECT_ID_INITIAL_CAPACITY_LIMIT: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectStorageHint {
    Loose,
    Packed,
    Unknown,
}

pub trait GitObjectStore {
    fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject>;

    fn contains_object(&self, id: &ObjectId) -> io::Result<bool> {
        match self.read_object(id) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn object_count(&self) -> io::Result<usize> {
        Ok(0)
    }

    fn object_id_capacity_hint(&self) -> io::Result<usize> {
        Ok(0)
    }

    fn for_each_object_id(
        &self,
        _for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
    ) -> io::Result<()> {
        Ok(())
    }

    fn append_object_ids(&self, _ids: &mut Vec<ObjectId>) -> io::Result<()> {
        Ok(())
    }

    fn read_tree_refs(&self, id: &ObjectId) -> io::Result<Vec<TreeObjectRef>> {
        let object = self.read_object(id)?;
        if object.kind != GitObjectKind::Tree {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "object is not a tree",
            ));
        }
        decode_tree_object_refs(id.algorithm(), &object.content)
    }

    fn try_write_reusable_pack(
        &self,
        algorithm: GitHashAlgorithm,
        ids: &[ObjectId],
        writer: &mut dyn Write,
    ) -> io::Result<Option<ObjectId>> {
        let _ = (algorithm, ids, writer);
        Ok(None)
    }

    fn blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        let _ = id;
        Ok(None)
    }

    fn read_blob_prefix(&self, id: &ObjectId, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        let _ = (id, max_bytes);
        Ok(None)
    }

    fn streamable_blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        let _ = id;
        Ok(None)
    }

    fn object_kind_hint(&self, id: &ObjectId) -> io::Result<Option<GitObjectKind>> {
        let _ = id;
        Ok(None)
    }

    fn object_header_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
        let _ = id;
        Ok(None)
    }

    fn object_storage_hint(&self, id: &ObjectId) -> io::Result<ObjectStorageHint> {
        let _ = id;
        Ok(ObjectStorageHint::Unknown)
    }

    fn try_write_reusable_pack_object(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
    ) -> io::Result<bool> {
        let _ = (id, writer);
        Ok(false)
    }

    fn try_write_reusable_pack_object_with_buffer(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
        buffer: &mut [u8],
    ) -> io::Result<bool> {
        let _ = buffer;
        self.try_write_reusable_pack_object(id, writer)
    }

    fn try_write_blob(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        let _ = (id, writer);
        Ok(false)
    }

    fn write_streamable_blob(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        let _ = (id, writer);
        Ok(false)
    }

    fn try_write_blob_to_path(
        &self,
        id: &ObjectId,
        min_bytes: usize,
        path: &Path,
    ) -> io::Result<bool> {
        let _ = (id, min_bytes, path);
        Ok(false)
    }

    fn try_write_reusable_pack_with_guard(
        &self,
        algorithm: GitHashAlgorithm,
        ids: &[ObjectId],
        writer: &mut dyn Write,
        _guard: &mut dyn FnMut() -> io::Result<bool>,
    ) -> io::Result<Option<ObjectId>> {
        if !_guard()? {
            return Ok(None);
        }
        self.try_write_reusable_pack(algorithm, ids, writer)
    }
}

pub trait GitObjectSink {
    fn write_object(&self, kind: GitObjectKind, content: &[u8]) -> io::Result<ObjectId>;

    fn write_streamed_blob_content<F>(&self, size: usize, write_content: F) -> io::Result<ObjectId>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        let mut content = Vec::with_capacity(streamed_blob_initial_capacity(size));
        write_content(&mut content)?;
        if content.len() != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "blob stream ended before declared size",
            ));
        }
        self.write_object(GitObjectKind::Blob, &content)
    }
}

fn streamed_blob_initial_capacity(size: usize) -> usize {
    size.min(STREAMED_BLOB_INITIAL_CAPACITY_LIMIT)
}

#[derive(Debug)]
pub struct InMemoryObjectStore {
    algorithm: GitHashAlgorithm,
    objects: std::sync::RwLock<BTreeMap<Vec<u8>, LooseObject>>,
}

impl InMemoryObjectStore {
    pub fn new(algorithm: GitHashAlgorithm) -> Self {
        Self {
            algorithm,
            objects: std::sync::RwLock::new(BTreeMap::new()),
        }
    }

    pub const fn algorithm(&self) -> GitHashAlgorithm {
        self.algorithm
    }

    pub fn object_ids(&self) -> Vec<ObjectId> {
        let mut ids = Vec::with_capacity(in_memory_object_id_initial_capacity(
            self.objects
                .read()
                .expect("in-memory object store lock poisoned")
                .len(),
        ));
        self.append_object_ids(&mut ids)
            .expect("in-memory object ids cannot fail");
        ids
    }
}

fn in_memory_object_id_initial_capacity(count: usize) -> usize {
    count.min(IN_MEMORY_OBJECT_ID_INITIAL_CAPACITY_LIMIT)
}

fn reserve_in_memory_object_ids_spare(ids: &mut Vec<ObjectId>, count: usize) {
    let desired_spare = in_memory_object_id_initial_capacity(count);
    let spare = ids.capacity().saturating_sub(ids.len());
    if spare < desired_spare {
        ids.reserve(desired_spare);
    }
}

impl GitObjectStore for InMemoryObjectStore {
    fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        self.objects
            .read()
            .expect("in-memory object store lock poisoned")
            .get(id.as_bytes())
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "git object not found"))
    }

    fn object_count(&self) -> io::Result<usize> {
        Ok(self
            .objects
            .read()
            .expect("in-memory object store lock poisoned")
            .len())
    }

    fn object_id_capacity_hint(&self) -> io::Result<usize> {
        self.object_count()
    }

    fn for_each_object_id(
        &self,
        for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
    ) -> io::Result<()> {
        let objects = self
            .objects
            .read()
            .expect("in-memory object store lock poisoned");
        for object in objects.values() {
            for_each(&object.id)?;
        }
        Ok(())
    }

    fn append_object_ids(&self, ids: &mut Vec<ObjectId>) -> io::Result<()> {
        reserve_in_memory_object_ids_spare(ids, self.object_id_capacity_hint()?);
        self.for_each_object_id(&mut |id| {
            ids.push(id.clone());
            Ok(())
        })
    }

    fn object_kind_hint(&self, id: &ObjectId) -> io::Result<Option<GitObjectKind>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        Ok(self
            .objects
            .read()
            .expect("in-memory object store lock poisoned")
            .get(id.as_bytes())
            .map(|object| object.kind))
    }

    fn object_header_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object id algorithm does not match store",
            ));
        }
        Ok(self
            .objects
            .read()
            .expect("in-memory object store lock poisoned")
            .get(id.as_bytes())
            .map(|object| (object.kind, object.content.len())))
    }
}

impl GitObjectSink for InMemoryObjectStore {
    fn write_object(&self, kind: GitObjectKind, content: &[u8]) -> io::Result<ObjectId> {
        let id = hash_object(self.algorithm, kind, content);
        self.objects
            .write()
            .expect("in-memory object store lock poisoned")
            .entry(id.as_bytes().to_vec())
            .or_insert_with(|| LooseObject {
                id: id.clone(),
                kind,
                content: content.to_vec(),
            });
        Ok(id)
    }

    fn write_streamed_blob_content<F>(&self, size: usize, write_content: F) -> io::Result<ObjectId>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        let mut content = Vec::with_capacity(streamed_blob_initial_capacity(size));
        write_content(&mut content)?;
        if content.len() != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "blob stream ended before declared size",
            ));
        }
        let id = hash_object(self.algorithm, GitObjectKind::Blob, &content);
        self.objects
            .write()
            .expect("in-memory object store lock poisoned")
            .entry(id.as_bytes().to_vec())
            .or_insert_with(|| LooseObject {
                id: id.clone(),
                kind: GitObjectKind::Blob,
                content,
            });
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_blob_initial_capacity_is_bounded() {
        assert_eq!(
            streamed_blob_initial_capacity(usize::MAX),
            STREAMED_BLOB_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(streamed_blob_initial_capacity(2), 2);
        assert_eq!(streamed_blob_initial_capacity(0), 0);
    }

    #[test]
    fn in_memory_object_id_initial_capacity_is_bounded() {
        assert_eq!(in_memory_object_id_initial_capacity(0), 0);
        assert_eq!(in_memory_object_id_initial_capacity(2), 2);
        assert_eq!(
            in_memory_object_id_initial_capacity(usize::MAX),
            IN_MEMORY_OBJECT_ID_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn in_memory_object_id_reserve_does_not_grow_when_spare_capacity_is_enough() {
        let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let mut ids = Vec::with_capacity(4);
        ids.push(id);
        let capacity = ids.capacity();

        reserve_in_memory_object_ids_spare(&mut ids, 2);

        assert_eq!(ids.capacity(), capacity);
    }

    #[test]
    fn in_memory_object_id_reserve_grows_when_spare_capacity_is_insufficient() {
        let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"first");
        let mut ids = Vec::with_capacity(1);
        ids.push(id);

        reserve_in_memory_object_ids_spare(&mut ids, 5);

        assert!(ids.capacity().saturating_sub(ids.len()) >= 5);
    }

    #[test]
    fn in_memory_store_round_trips_objects_without_filesystem() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"browser object\n")
            .expect("write object");

        let object = store.read_object(&id).expect("read object");

        assert_eq!(object.id, id);
        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, b"browser object\n");
        assert_eq!(store.object_ids(), vec![id]);
    }

    #[test]
    fn in_memory_store_writes_streamed_blob_content() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);

        let id = store
            .write_streamed_blob_content(14, |writer| writer.write_all(b"streamed blob\n"))
            .expect("write streamed blob");
        let object = store.read_object(&id).expect("read streamed blob");

        assert_eq!(object.kind, GitObjectKind::Blob);
        assert_eq!(object.content, b"streamed blob\n");
    }

    #[test]
    fn in_memory_store_reports_object_header_hint_without_reading_content() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"header hint\n")
            .expect("write object");

        assert_eq!(
            store.object_header_hint(&id).expect("header hint"),
            Some((GitObjectKind::Blob, "header hint\n".len()))
        );
    }
}
