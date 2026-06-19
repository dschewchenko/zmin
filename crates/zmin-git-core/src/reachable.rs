use std::collections::HashSet;
use std::io;

use crate::{
    GitObjectKind, GitObjectStore, ObjectId, TreeMode, decode_commit_links, decode_tag,
    for_each_tree_object_ref,
};

const REACHABLE_SEEN_INITIAL_CAPACITY_LIMIT: usize = 8192;
const REACHABLE_PENDING_RESERVE_LIMIT: usize = 8192;
const REACHABLE_PENDING_INITIAL_CAPACITY_LIMIT: usize = 8192;
const REACHABLE_TREE_ENTRY_MIN_BYTES: usize = 24;

pub fn collect_reachable_objects_from_roots<S: GitObjectStore>(
    store: &S,
    roots: &[ObjectId],
) -> io::Result<HashSet<ObjectId>> {
    let mut seen = HashSet::with_capacity(reachable_seen_initial_capacity(
        store.object_id_capacity_hint()?,
        roots.len(),
    ));
    let mut pending = Vec::with_capacity(reachable_pending_initial_capacity(roots.len()));
    for root in roots.iter().rev() {
        if !schedule_reachable_object(&mut seen, &mut pending, root) {
            continue;
        }
        while let Some(id) = pending.pop() {
            mark_reachable_object(store, id, &mut seen, &mut pending)?;
        }
    }
    Ok(seen)
}

fn reachable_seen_initial_capacity(store_hint: usize, roots_len: usize) -> usize {
    store_hint
        .max(roots_len)
        .min(REACHABLE_SEEN_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn mark_reachable_object<S: GitObjectStore>(
    store: &S,
    id: ObjectId,
    seen: &mut HashSet<ObjectId>,
    pending: &mut Vec<ObjectId>,
) -> io::Result<()> {
    if matches!(store.object_kind_hint(&id)?, Some(GitObjectKind::Blob)) {
        return Ok(());
    }
    let object = store.read_object(&id)?;
    match object.kind {
        GitObjectKind::Blob => {}
        GitObjectKind::Tree => {
            reserve_reachable_tree_pending(pending, id.algorithm(), object.content.len());
            for_each_tree_object_ref(id.algorithm(), &object.content, |mode, entry_id| {
                if mode != TreeMode::Gitlink {
                    schedule_reachable_object(seen, pending, &entry_id);
                }
                Ok(())
            })?;
        }
        GitObjectKind::Commit => {
            let commit = decode_commit_links(id.algorithm(), &object.content)?;
            reserve_reachable_pending(pending, commit.parents.len());
            schedule_reachable_object(seen, pending, &commit.tree);
            for parent in &commit.parents {
                schedule_reachable_object(seen, pending, parent);
            }
        }
        GitObjectKind::Tag => {
            let tag = decode_tag(id.algorithm(), &object.content)?;
            schedule_reachable_object(seen, pending, &tag.target);
        }
    }
    Ok(())
}

fn schedule_reachable_object(
    seen: &mut HashSet<ObjectId>,
    pending: &mut Vec<ObjectId>,
    id: &ObjectId,
) -> bool {
    if seen.insert(id.clone()) {
        pending.push(id.clone());
        true
    } else {
        false
    }
}

fn reachable_pending_reserve(parents_len: usize) -> usize {
    parents_len
        .saturating_add(1)
        .min(REACHABLE_PENDING_RESERVE_LIMIT)
}

fn reachable_pending_initial_capacity(roots_len: usize) -> usize {
    roots_len.min(REACHABLE_PENDING_INITIAL_CAPACITY_LIMIT)
}

fn reserve_reachable_pending(pending: &mut Vec<ObjectId>, parents_len: usize) {
    let desired_spare = reachable_pending_reserve(parents_len);
    let spare = pending.capacity().saturating_sub(pending.len());
    if spare < desired_spare {
        pending.reserve(desired_spare);
    }
}

fn reachable_tree_pending_reserve(algorithm: crate::GitHashAlgorithm, content_len: usize) -> usize {
    let min_entry_len = algorithm
        .digest_len()
        .saturating_add(REACHABLE_TREE_ENTRY_MIN_BYTES.saturating_sub(20));
    content_len
        .checked_div(min_entry_len.max(1))
        .unwrap_or(0)
        .min(REACHABLE_PENDING_RESERVE_LIMIT)
}

fn reserve_reachable_tree_pending(
    pending: &mut Vec<ObjectId>,
    algorithm: crate::GitHashAlgorithm,
    content_len: usize,
) {
    let desired_spare = reachable_tree_pending_reserve(algorithm, content_len);
    let spare = pending.capacity().saturating_sub(pending.len());
    if spare < desired_spare {
        pending.reserve(desired_spare);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        CommitBuilder, GitHashAlgorithm, GitObjectSink, InMemoryObjectStore, LooseObject,
        LooseObjectStore, Signature, TagBuilder, TreeEntry, encode_tree,
    };

    #[test]
    fn collects_reachable_blob_tree_commit_and_tag_objects() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"hello\n")
            .expect("write blob");
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "a.txt", blob.clone()).expect("tree entry")
                ])
                .expect("encode tree"),
            )
            .expect("write tree");
        let signature =
            Signature::new("Bench", "bench@example.test", 1_700_000_000, "+0000").expect("sig");
        let commit = store
            .write_object(
                GitObjectKind::Commit,
                &CommitBuilder::new(tree.clone(), signature.clone(), signature.clone())
                    .message("initial\n")
                    .expect("message")
                    .encode()
                    .expect("commit"),
            )
            .expect("write commit");
        let tag = store
            .write_object(
                GitObjectKind::Tag,
                &TagBuilder::new(commit.clone(), GitObjectKind::Commit, "v1", signature)
                    .expect("tag builder")
                    .message("tag\n")
                    .expect("tag message")
                    .encode()
                    .expect("tag"),
            )
            .expect("write tag");

        let reachable = collect_reachable_objects_from_roots(&store, std::slice::from_ref(&tag))
            .expect("reachable");

        assert!(reachable.contains(&tag));
        assert!(reachable.contains(&commit));
        assert!(reachable.contains(&tree));
        assert!(reachable.contains(&blob));
        assert_eq!(reachable.len(), 4);
    }

    #[test]
    fn collects_reachable_objects_from_in_memory_store() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"browser reachable\n")
            .expect("write blob");
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "page.md", blob.clone()).expect("tree entry")
                ])
                .expect("encode tree"),
            )
            .expect("write tree");

        let reachable = collect_reachable_objects_from_roots(&store, std::slice::from_ref(&tree))
            .expect("reachable");

        assert!(reachable.contains(&tree));
        assert!(reachable.contains(&blob));
    }

    #[test]
    fn reachable_seen_capacity_hint_is_bounded_for_large_stores() {
        assert_eq!(reachable_seen_initial_capacity(usize::MAX, 1), 8192);
        assert_eq!(reachable_seen_initial_capacity(2, 4), 4);
        assert_eq!(reachable_seen_initial_capacity(0, 0), 1);
    }

    #[test]
    fn reachable_pending_reserve_is_bounded_for_wide_commits() {
        assert_eq!(reachable_pending_reserve(0), 1);
        assert_eq!(reachable_pending_reserve(2), 3);
        assert_eq!(reachable_pending_reserve(usize::MAX), 8192);
    }

    #[test]
    fn reachable_tree_pending_reserve_is_bounded_by_tree_payload() {
        assert_eq!(reachable_tree_pending_reserve(GitHashAlgorithm::Sha1, 0), 0);
        assert_eq!(
            reachable_tree_pending_reserve(GitHashAlgorithm::Sha1, 24),
            1
        );
        assert_eq!(
            reachable_tree_pending_reserve(GitHashAlgorithm::Sha1, usize::MAX),
            REACHABLE_PENDING_RESERVE_LIMIT
        );
    }

    #[test]
    fn reachable_tree_pending_reserve_does_not_grow_when_spare_capacity_is_enough() {
        let mut pending = Vec::with_capacity(4);
        pending.push(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );
        let capacity = pending.capacity();

        reserve_reachable_tree_pending(&mut pending, GitHashAlgorithm::Sha1, 48);

        assert_eq!(pending.capacity(), capacity);
    }

    #[test]
    fn reachable_pending_initial_capacity_is_bounded_for_many_roots() {
        assert_eq!(reachable_pending_initial_capacity(0), 0);
        assert_eq!(reachable_pending_initial_capacity(2), 2);
        assert_eq!(
            reachable_pending_initial_capacity(usize::MAX),
            REACHABLE_PENDING_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn reachable_pending_reserve_does_not_grow_when_spare_capacity_is_enough() {
        let mut pending = Vec::with_capacity(4);
        pending.push(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );
        let capacity = pending.capacity();

        reserve_reachable_pending(&mut pending, 2);

        assert_eq!(pending.capacity(), capacity);
    }

    #[test]
    fn reachable_scheduler_deduplicates_before_pending_push() {
        let id = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "1111111111111111111111111111111111111111",
        )
        .expect("object id");
        let mut seen = HashSet::new();
        let mut pending = Vec::new();

        assert!(schedule_reachable_object(&mut seen, &mut pending, &id));
        assert!(!schedule_reachable_object(&mut seen, &mut pending, &id));

        assert_eq!(pending, vec![id]);
        assert_eq!(seen.len(), 1);
    }

    #[test]
    fn decodes_tree_from_loaded_object_without_second_read() {
        let inner = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = inner
            .write_object(GitObjectKind::Blob, b"reachable\n")
            .expect("write blob");
        let tree = inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "file.txt", blob.clone()).expect("tree entry")
                ])
                .expect("encode tree"),
            )
            .expect("write tree");
        let store = CountingStore {
            inner,
            reads: RefCell::new(HashMap::new()),
            read_order: RefCell::new(Vec::new()),
            kind_hints: RefCell::new(HashMap::new()),
        };

        let reachable = collect_reachable_objects_from_roots(&store, std::slice::from_ref(&tree))
            .expect("reachable");

        assert!(reachable.contains(&tree));
        assert!(reachable.contains(&blob));
        assert_eq!(store.read_count(&tree), 1);
    }

    #[test]
    fn skips_blob_materialization_when_kind_hint_is_available() {
        let inner = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = inner
            .write_object(GitObjectKind::Blob, b"large blob placeholder\n")
            .expect("write blob");
        let tree = inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "large.bin", blob.clone()).expect("tree entry")
                ])
                .expect("encode tree"),
            )
            .expect("write tree");
        let store = CountingStore {
            inner,
            reads: RefCell::new(HashMap::new()),
            read_order: RefCell::new(Vec::new()),
            kind_hints: RefCell::new(HashMap::new()),
        };

        let reachable = collect_reachable_objects_from_roots(&store, std::slice::from_ref(&tree))
            .expect("reachable");

        assert!(reachable.contains(&tree));
        assert!(reachable.contains(&blob));
        assert_eq!(store.read_count(&blob), 0);
        assert_eq!(store.kind_hint_count(&blob), 1);
    }

    #[test]
    fn duplicate_tree_entries_schedule_same_object_once() {
        let inner = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = inner
            .write_object(GitObjectKind::Blob, b"shared blob\n")
            .expect("write blob");
        let tree = inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "a.txt", blob.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "b.txt", blob.clone()).expect("tree entry"),
                ])
                .expect("encode tree"),
            )
            .expect("write tree");
        let store = CountingStore {
            inner,
            reads: RefCell::new(HashMap::new()),
            read_order: RefCell::new(Vec::new()),
            kind_hints: RefCell::new(HashMap::new()),
        };

        let reachable = collect_reachable_objects_from_roots(&store, std::slice::from_ref(&tree))
            .expect("reachable");

        assert!(reachable.contains(&tree));
        assert!(reachable.contains(&blob));
        assert_eq!(reachable.len(), 2);
        assert_eq!(store.kind_hint_count(&blob), 1);
        assert_eq!(store.read_count(&blob), 0);
    }

    #[test]
    fn streams_roots_in_original_lifo_order() {
        let inner = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first_blob = inner
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let first_tree = inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "first.txt", first_blob).expect("tree entry")
                ])
                .expect("encode first tree"),
            )
            .expect("write first tree");
        let second_blob = inner
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let second_tree = inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "second.txt", second_blob).expect("tree entry")
                ])
                .expect("encode second tree"),
            )
            .expect("write second tree");
        let store = CountingStore {
            inner,
            reads: RefCell::new(HashMap::new()),
            read_order: RefCell::new(Vec::new()),
            kind_hints: RefCell::new(HashMap::new()),
        };

        collect_reachable_objects_from_roots(&store, &[first_tree.clone(), second_tree.clone()])
            .expect("reachable");

        assert_eq!(store.read_order(), vec![second_tree, first_tree]);
    }

    struct CountingStore {
        inner: InMemoryObjectStore,
        reads: RefCell<HashMap<ObjectId, usize>>,
        read_order: RefCell<Vec<ObjectId>>,
        kind_hints: RefCell<HashMap<ObjectId, usize>>,
    }

    impl CountingStore {
        fn read_count(&self, id: &ObjectId) -> usize {
            self.reads.borrow().get(id).copied().unwrap_or(0)
        }

        fn kind_hint_count(&self, id: &ObjectId) -> usize {
            self.kind_hints.borrow().get(id).copied().unwrap_or(0)
        }

        fn read_order(&self) -> Vec<ObjectId> {
            self.read_order.borrow().clone()
        }
    }

    impl GitObjectStore for CountingStore {
        fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
            *self.reads.borrow_mut().entry(id.clone()).or_insert(0) += 1;
            self.read_order.borrow_mut().push(id.clone());
            self.inner.read_object(id)
        }

        fn object_kind_hint(&self, id: &ObjectId) -> io::Result<Option<GitObjectKind>> {
            *self.kind_hints.borrow_mut().entry(id.clone()).or_insert(0) += 1;
            self.inner.object_kind_hint(id)
        }
    }
}
