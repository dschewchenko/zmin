use std::collections::BTreeMap;
use std::io;

use crate::index::{GitIndex, IndexEntry, IndexMode};
use crate::object::ObjectId;
use crate::object_store::GitObjectStore;
use crate::tree::{TreeEntry, TreeMode, TreeObjectCache, read_tree_to_index};

const DIFF_ENTRY_INITIAL_CAPACITY_LIMIT: usize = 8192;
const DIFF_PATH_INITIAL_CAPACITY_LIMIT: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexDiffStatus {
    Added,
    Copied,
    Deleted,
    Modified,
    Renamed,
}

impl IndexDiffStatus {
    pub const fn name_status(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Copied => "C100",
            Self::Deleted => "D",
            Self::Modified => "M",
            Self::Renamed => "R100",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDiffEntry {
    pub status: IndexDiffStatus,
    pub path: Vec<u8>,
    pub old_path: Option<Vec<u8>>,
    pub similarity: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDiffFileEntry {
    pub path: Vec<u8>,
    pub id: ObjectId,
    pub mode: IndexMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDiffEntry {
    pub status: IndexDiffStatus,
    pub path: Vec<u8>,
    pub old_entry: Option<TreeDiffFileEntry>,
    pub new_entry: Option<TreeDiffFileEntry>,
}

pub fn diff_index_to_tree<S: GitObjectStore>(
    store: &S,
    tree_id: &ObjectId,
    index: &GitIndex,
) -> io::Result<Vec<IndexDiffEntry>> {
    let tree_index = read_tree_to_index(store, tree_id)?;
    diff_indexes(&tree_index, index)
}

pub fn diff_indexes(old: &GitIndex, new: &GitIndex) -> io::Result<Vec<IndexDiffEntry>> {
    validate_diff_index(old)?;
    validate_diff_index(new)?;
    let diff_capacity = diff_entry_initial_capacity(old.entries().len(), new.entries().len());
    let mut diff = Vec::with_capacity(diff_capacity);
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;

    while old_idx < old.entries().len() || new_idx < new.entries().len() {
        match (old.entries().get(old_idx), new.entries().get(new_idx)) {
            (Some(old_entry), Some(new_entry)) => match old_entry.path.cmp(&new_entry.path) {
                std::cmp::Ordering::Less => {
                    diff.push(IndexDiffEntry {
                        status: IndexDiffStatus::Deleted,
                        path: old_entry.path.clone(),
                        old_path: None,
                        similarity: None,
                    });
                    old_idx += 1;
                }
                std::cmp::Ordering::Greater => {
                    diff.push(IndexDiffEntry {
                        status: IndexDiffStatus::Added,
                        path: new_entry.path.clone(),
                        old_path: None,
                        similarity: None,
                    });
                    new_idx += 1;
                }
                std::cmp::Ordering::Equal => {
                    if entry_identity(old_entry) != entry_identity(new_entry) {
                        diff.push(IndexDiffEntry {
                            status: IndexDiffStatus::Modified,
                            path: new_entry.path.clone(),
                            old_path: None,
                            similarity: None,
                        });
                    }
                    old_idx += 1;
                    new_idx += 1;
                }
            },
            (Some(old_entry), None) => {
                diff.push(IndexDiffEntry {
                    status: IndexDiffStatus::Deleted,
                    path: old_entry.path.clone(),
                    old_path: None,
                    similarity: None,
                });
                old_idx += 1;
            }
            (None, Some(new_entry)) => {
                diff.push(IndexDiffEntry {
                    status: IndexDiffStatus::Added,
                    path: new_entry.path.clone(),
                    old_path: None,
                    similarity: None,
                });
                new_idx += 1;
            }
            (None, None) => break,
        }
    }

    Ok(diff)
}

fn validate_diff_index(index: &GitIndex) -> io::Result<()> {
    for entry in index.entries() {
        if entry.stage != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot diff an index with unresolved conflicts",
            ));
        }
    }
    Ok(())
}

pub fn diff_trees<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
) -> io::Result<Vec<TreeDiffEntry>> {
    let mut diff = Vec::new();
    for_each_tree_diff(tree_cache, old_tree, new_tree, |entry| {
        diff.push(entry);
        Ok(())
    })?;
    Ok(diff)
}

pub fn for_each_tree_diff<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
    mut visit: impl FnMut(TreeDiffEntry) -> io::Result<()>,
) -> io::Result<()> {
    match old_tree {
        Some(old_tree) if old_tree == new_tree => Ok(()),
        Some(old_tree) => {
            let old_entries = tree_cache.read_tree(old_tree)?;
            let new_entries = tree_cache.read_tree(new_tree)?;
            diff_tree_entries(
                tree_cache,
                old_entries.as_ref(),
                new_entries.as_ref(),
                &mut visit,
            )
        }
        None => {
            let new_entries = tree_cache.read_tree(new_tree)?;
            emit_tree_entries(
                tree_cache,
                new_entries.as_ref(),
                IndexDiffStatus::Added,
                &mut Vec::new(),
                &mut visit,
            )
        }
    }
}

fn diff_tree_entries<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    old_entries: &[TreeEntry],
    new_entries: &[TreeEntry],
    visit: &mut impl FnMut(TreeDiffEntry) -> io::Result<()>,
) -> io::Result<()> {
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;
    let mut path = Vec::new();
    diff_tree_entries_at(
        tree_cache,
        old_entries,
        new_entries,
        &mut old_idx,
        &mut new_idx,
        &mut path,
        visit,
    )
}

fn diff_tree_entries_at<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    old_entries: &[TreeEntry],
    new_entries: &[TreeEntry],
    old_idx: &mut usize,
    new_idx: &mut usize,
    path: &mut Vec<u8>,
    visit: &mut impl FnMut(TreeDiffEntry) -> io::Result<()>,
) -> io::Result<()> {
    while *old_idx < old_entries.len() || *new_idx < new_entries.len() {
        match (old_entries.get(*old_idx), new_entries.get(*new_idx)) {
            (Some(old_entry), Some(new_entry)) => {
                match compare_tree_entries(old_entry, new_entry) {
                    std::cmp::Ordering::Less => {
                        emit_tree_entry(
                            tree_cache,
                            old_entry,
                            IndexDiffStatus::Deleted,
                            path,
                            visit,
                        )?;
                        *old_idx += 1;
                    }
                    std::cmp::Ordering::Greater => {
                        emit_tree_entry(
                            tree_cache,
                            new_entry,
                            IndexDiffStatus::Added,
                            path,
                            visit,
                        )?;
                        *new_idx += 1;
                    }
                    std::cmp::Ordering::Equal => {
                        diff_matching_tree_entry(tree_cache, old_entry, new_entry, path, visit)?;
                        *old_idx += 1;
                        *new_idx += 1;
                    }
                }
            }
            (Some(old_entry), None) => {
                emit_tree_entry(tree_cache, old_entry, IndexDiffStatus::Deleted, path, visit)?;
                *old_idx += 1;
            }
            (None, Some(new_entry)) => {
                emit_tree_entry(tree_cache, new_entry, IndexDiffStatus::Added, path, visit)?;
                *new_idx += 1;
            }
            (None, None) => break,
        }
    }
    Ok(())
}

fn diff_matching_tree_entry<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    old_entry: &TreeEntry,
    new_entry: &TreeEntry,
    path: &mut Vec<u8>,
    visit: &mut impl FnMut(TreeDiffEntry) -> io::Result<()>,
) -> io::Result<()> {
    if old_entry.mode == TreeMode::Tree && new_entry.mode == TreeMode::Tree {
        if old_entry.id == new_entry.id {
            return Ok(());
        }
        let path_len = push_tree_path(path, &old_entry.name);
        let old_entries = tree_cache.read_tree(&old_entry.id)?;
        let new_entries = tree_cache.read_tree(&new_entry.id)?;
        let mut old_idx = 0usize;
        let mut new_idx = 0usize;
        diff_tree_entries_at(
            tree_cache,
            old_entries.as_ref(),
            new_entries.as_ref(),
            &mut old_idx,
            &mut new_idx,
            path,
            visit,
        )?;
        path.truncate(path_len);
        return Ok(());
    }
    if old_entry.mode == TreeMode::Tree {
        emit_tree_entry(tree_cache, old_entry, IndexDiffStatus::Deleted, path, visit)?;
        emit_tree_entry(tree_cache, new_entry, IndexDiffStatus::Added, path, visit)?;
        return Ok(());
    }
    if new_entry.mode == TreeMode::Tree {
        emit_tree_entry(tree_cache, old_entry, IndexDiffStatus::Deleted, path, visit)?;
        emit_tree_entry(tree_cache, new_entry, IndexDiffStatus::Added, path, visit)?;
        return Ok(());
    }
    if old_entry.mode != new_entry.mode || old_entry.id != new_entry.id {
        let mut file_path = path.clone();
        file_path.extend_from_slice(&new_entry.name);
        visit(TreeDiffEntry {
            status: IndexDiffStatus::Modified,
            path: file_path.clone(),
            old_entry: Some(tree_diff_file_entry(file_path.clone(), old_entry)?),
            new_entry: Some(tree_diff_file_entry(file_path, new_entry)?),
        })?;
    }
    Ok(())
}

fn emit_tree_entry<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    entry: &TreeEntry,
    status: IndexDiffStatus,
    path: &mut Vec<u8>,
    visit: &mut impl FnMut(TreeDiffEntry) -> io::Result<()>,
) -> io::Result<()> {
    if entry.mode == TreeMode::Tree {
        let path_len = push_tree_path(path, &entry.name);
        let entries = tree_cache.read_tree(&entry.id)?;
        emit_tree_entries(tree_cache, entries.as_ref(), status, path, visit)?;
        path.truncate(path_len);
        return Ok(());
    }
    let mut file_path = path.clone();
    file_path.extend_from_slice(&entry.name);
    let file_entry = tree_diff_file_entry(file_path.clone(), entry)?;
    let (old_entry, new_entry) = match status {
        IndexDiffStatus::Added => (None, Some(file_entry)),
        IndexDiffStatus::Deleted => (Some(file_entry), None),
        IndexDiffStatus::Modified | IndexDiffStatus::Renamed | IndexDiffStatus::Copied => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tree entry emission only supports added or deleted statuses",
            ));
        }
    };
    visit(TreeDiffEntry {
        status,
        path: file_path,
        old_entry,
        new_entry,
    })
}

fn emit_tree_entries<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    entries: &[TreeEntry],
    status: IndexDiffStatus,
    path: &mut Vec<u8>,
    visit: &mut impl FnMut(TreeDiffEntry) -> io::Result<()>,
) -> io::Result<()> {
    for entry in entries {
        emit_tree_entry(tree_cache, entry, status, path, visit)?;
    }
    Ok(())
}

fn tree_diff_file_entry(path: Vec<u8>, entry: &TreeEntry) -> io::Result<TreeDiffFileEntry> {
    Ok(TreeDiffFileEntry {
        path,
        id: entry.id.clone(),
        mode: index_mode_from_tree_mode(entry.mode)?,
    })
}

fn index_mode_from_tree_mode(mode: TreeMode) -> io::Result<IndexMode> {
    match mode {
        TreeMode::File => Ok(IndexMode::File),
        TreeMode::Executable => Ok(IndexMode::Executable),
        TreeMode::Symlink => Ok(IndexMode::Symlink),
        TreeMode::Gitlink => Ok(IndexMode::Gitlink),
        TreeMode::Tree => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "tree entry mode was expected to be expanded recursively",
        )),
    }
}

fn push_tree_path(path: &mut Vec<u8>, name: &[u8]) -> usize {
    let path_len = path.len();
    path.extend_from_slice(name);
    path.push(b'/');
    path_len
}

fn compare_tree_entries(left: &TreeEntry, right: &TreeEntry) -> std::cmp::Ordering {
    compare_tree_entry_names(
        &left.name,
        left.mode == TreeMode::Tree,
        &right.name,
        right.mode == TreeMode::Tree,
    )
}

fn compare_tree_entry_names(
    left: &[u8],
    left_tree: bool,
    right: &[u8],
    right_tree: bool,
) -> std::cmp::Ordering {
    let mut idx = 0;
    loop {
        let left_byte = tree_entry_name_byte(left, left_tree, idx);
        let right_byte = tree_entry_name_byte(right, right_tree, idx);
        match (left_byte, right_byte) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(left), Some(right)) if left != right => return left.cmp(&right),
            _ => idx += 1,
        }
    }
}

fn tree_entry_name_byte(name: &[u8], is_tree: bool, idx: usize) -> Option<u8> {
    if idx < name.len() {
        Some(name[idx])
    } else if idx == name.len() && is_tree {
        Some(b'/')
    } else {
        None
    }
}

pub fn diff_indexes_with_exact_renames(
    old: &GitIndex,
    new: &GitIndex,
) -> io::Result<Vec<IndexDiffEntry>> {
    diff_indexes_with_exact_renames_and_copies(old, new, false)
}

pub fn diff_indexes_with_exact_renames_and_copies(
    old: &GitIndex,
    new: &GitIndex,
    find_copies_harder: bool,
) -> io::Result<Vec<IndexDiffEntry>> {
    let old_map = index_map(old)?;
    let new_map = index_map(new)?;
    let mut diff = diff_indexes(old, new)?;
    detect_exact_renames(&old_map, &new_map, &mut diff);
    detect_exact_copies(&old_map, &new_map, &mut diff, find_copies_harder);
    Ok(diff)
}

fn detect_exact_renames(
    old_map: &BTreeMap<Vec<u8>, (IndexMode, ObjectId)>,
    new_map: &BTreeMap<Vec<u8>, (IndexMode, ObjectId)>,
    diff: &mut Vec<IndexDiffEntry>,
) {
    let mut deleted = Vec::with_capacity(diff_path_initial_capacity(diff_status_count(
        diff,
        IndexDiffStatus::Deleted,
    )));
    let mut added = Vec::with_capacity(diff_path_initial_capacity(diff_status_count(
        diff,
        IndexDiffStatus::Added,
    )));
    for entry in diff.iter() {
        if entry.status == IndexDiffStatus::Deleted {
            deleted.push(entry);
        } else if entry.status == IndexDiffStatus::Added {
            added.push(entry);
        }
    }
    let rename_capacity = diff_path_initial_capacity(deleted.len().min(added.len()));
    let mut renamed_old_paths = Vec::with_capacity(rename_capacity);
    let mut renamed_new_paths = Vec::with_capacity(rename_capacity);

    for deleted_entry in deleted {
        let Some(deleted_identity) = old_map.get(&deleted_entry.path) else {
            continue;
        };
        let Some(added_entry) = added
            .iter()
            .find(|entry| {
                !renamed_new_paths.contains(&entry.path)
                    && new_map.get(&entry.path) == Some(deleted_identity)
            })
            .copied()
        else {
            continue;
        };
        renamed_old_paths.push(deleted_entry.path.clone());
        renamed_new_paths.push(added_entry.path.clone());
    }

    diff.retain(|entry| {
        !((entry.status == IndexDiffStatus::Deleted && renamed_old_paths.contains(&entry.path))
            || (entry.status == IndexDiffStatus::Added && renamed_new_paths.contains(&entry.path)))
    });
    for (old_path, new_path) in renamed_old_paths.into_iter().zip(renamed_new_paths) {
        diff.push(IndexDiffEntry {
            status: IndexDiffStatus::Renamed,
            path: new_path,
            old_path: Some(old_path),
            similarity: Some(100),
        });
    }
    diff.sort_by(|left, right| left.path.cmp(&right.path));
}

fn detect_exact_copies(
    old_map: &BTreeMap<Vec<u8>, (IndexMode, ObjectId)>,
    new_map: &BTreeMap<Vec<u8>, (IndexMode, ObjectId)>,
    diff: &mut [IndexDiffEntry],
    find_copies_harder: bool,
) {
    let mut added_paths = Vec::with_capacity(diff_path_initial_capacity(diff_status_count(
        diff,
        IndexDiffStatus::Added,
    )));
    let mut changed_old_paths = Vec::with_capacity(diff_path_initial_capacity(
        diff_changed_old_path_count(diff),
    ));
    for entry in diff.iter() {
        if entry.status == IndexDiffStatus::Added {
            added_paths.push(entry.path.clone());
        } else if matches!(
            entry.status,
            IndexDiffStatus::Deleted | IndexDiffStatus::Modified
        ) {
            changed_old_paths.push(entry.path.clone());
        }
    }

    for added_path in added_paths {
        let Some(added_identity) = new_map.get(&added_path) else {
            continue;
        };
        let source = old_map.iter().find(|(path, identity)| {
            *identity == added_identity && (find_copies_harder || changed_old_paths.contains(path))
        });
        let Some((source_path, _)) = source else {
            continue;
        };
        if let Some(entry) = diff
            .iter_mut()
            .find(|entry| entry.status == IndexDiffStatus::Added && entry.path == added_path)
        {
            entry.status = IndexDiffStatus::Copied;
            entry.old_path = Some(source_path.clone());
            entry.similarity = Some(100);
        }
    }
}

fn index_map(index: &GitIndex) -> io::Result<BTreeMap<Vec<u8>, (IndexMode, ObjectId)>> {
    let mut entries = BTreeMap::new();
    for entry in index.entries() {
        if entry.stage != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot diff an index with unresolved conflicts",
            ));
        }
        entries.insert(entry.path.to_vec(), entry_identity(entry));
    }
    Ok(entries)
}

fn diff_entry_initial_capacity(old_len: usize, new_len: usize) -> usize {
    old_len
        .saturating_add(new_len)
        .min(DIFF_ENTRY_INITIAL_CAPACITY_LIMIT)
}

fn diff_path_initial_capacity(path_count: usize) -> usize {
    path_count.min(DIFF_PATH_INITIAL_CAPACITY_LIMIT)
}

fn diff_status_count(diff: &[IndexDiffEntry], status: IndexDiffStatus) -> usize {
    diff.iter().filter(|entry| entry.status == status).count()
}

fn diff_changed_old_path_count(diff: &[IndexDiffEntry]) -> usize {
    diff.iter()
        .filter(|entry| {
            matches!(
                entry.status,
                IndexDiffStatus::Deleted | IndexDiffStatus::Modified
            )
        })
        .count()
}

fn entry_identity(entry: &IndexEntry) -> (IndexMode, ObjectId) {
    (entry.mode, entry.id.clone())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        GitHashAlgorithm, GitIndex, GitObjectKind, GitObjectSink, GitObjectStore,
        InMemoryObjectStore, IndexEntry, IndexMode, LooseObject, LooseObjectStore, TreeEntry,
        TreeObjectCache, encode_tree, read_index, write_tree_from_index,
    };

    #[test]
    fn diff_index_to_tree_matches_stock_git_name_status() {
        let repo = git_init();
        std::fs::write(repo.path().join("a.txt"), b"old\n").expect("write a");
        std::fs::write(repo.path().join("b.txt"), b"delete\n").expect("write b");
        git(&repo, ["add", "a.txt", "b.txt"]);
        let tree = git(&repo, ["write-tree"]);

        std::fs::write(repo.path().join("a.txt"), b"new\n").expect("rewrite a");
        std::fs::write(repo.path().join("c.txt"), b"add\n").expect("write c");
        git(&repo, ["add", "a.txt", "c.txt"]);
        git(&repo, ["rm", "--cached", "--quiet", "b.txt"]);

        let expected = git(&repo, ["diff-index", "--cached", "--name-status", &tree]);
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let tree_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &tree).expect("tree id");
        let index = read_index(repo.path().join(".git/index")).expect("read index");
        let actual = diff_index_to_tree(&store, &tree_id, &index)
            .expect("diff")
            .into_iter()
            .map(|entry| {
                format!(
                    "{}\t{}",
                    entry.status.name_status(),
                    String::from_utf8_lossy(&entry.path)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(actual, expected);
    }

    #[test]
    fn diff_trees_emits_only_changed_leaf_entries() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let old_blob = store
            .write_object(GitObjectKind::Blob, b"old\n")
            .expect("write old blob");
        let new_blob = store
            .write_object(GitObjectKind::Blob, b"new\n")
            .expect("write new blob");
        let shared_blob = store
            .write_object(GitObjectKind::Blob, b"shared\n")
            .expect("write shared blob");
        let old_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "delete.txt", old_blob.clone())
                        .expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "modify.txt", old_blob).expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "shared.txt", shared_blob.clone())
                        .expect("tree entry"),
                ])
                .expect("encode old tree"),
            )
            .expect("write old tree");
        let new_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "add.txt", new_blob.clone())
                        .expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "modify.txt", new_blob).expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "shared.txt", shared_blob).expect("tree entry"),
                ])
                .expect("encode new tree"),
            )
            .expect("write new tree");
        let tree_cache = TreeObjectCache::new(&store);

        let diff = diff_trees(&tree_cache, Some(&old_tree), &new_tree).expect("diff trees");
        let actual = diff
            .iter()
            .map(|entry| (entry.status, entry.path.as_slice()))
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                (IndexDiffStatus::Added, b"add.txt".as_slice()),
                (IndexDiffStatus::Deleted, b"delete.txt".as_slice()),
                (IndexDiffStatus::Modified, b"modify.txt".as_slice()),
            ]
        );
    }

    #[test]
    fn diff_trees_skips_equal_subtrees_by_object_id() {
        struct CountingStore {
            inner: InMemoryObjectStore,
            reads: Cell<usize>,
        }

        impl GitObjectStore for CountingStore {
            fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
                self.reads.set(self.reads.get() + 1);
                self.inner.read_object(id)
            }
        }

        let store = CountingStore {
            inner: InMemoryObjectStore::new(GitHashAlgorithm::Sha1),
            reads: Cell::new(0),
        };
        let shared_blob = store
            .inner
            .write_object(GitObjectKind::Blob, b"shared\n")
            .expect("write shared blob");
        let changed_blob = store
            .inner
            .write_object(GitObjectKind::Blob, b"changed\n")
            .expect("write changed blob");
        let shared_tree = store
            .inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "leaf.txt", shared_blob).expect("tree entry")
                ])
                .expect("encode shared tree"),
            )
            .expect("write shared tree");
        let old_root = store
            .inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "shared", shared_tree.clone())
                        .expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "same-name.txt", changed_blob.clone())
                        .expect("tree entry"),
                ])
                .expect("encode old root"),
            )
            .expect("write old root");
        let new_root = store
            .inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "shared", shared_tree).expect("tree entry"),
                    TreeEntry::new(TreeMode::Executable, "same-name.txt", changed_blob)
                        .expect("tree entry"),
                ])
                .expect("encode new root"),
            )
            .expect("write new root");
        let tree_cache = TreeObjectCache::new(&store);

        let diff = diff_trees(&tree_cache, Some(&old_root), &new_root).expect("diff trees");

        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].path, b"same-name.txt");
        assert_eq!(store.reads.get(), 2);
    }

    #[test]
    fn diff_index_to_tree_works_with_in_memory_object_store() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let old_blob = store
            .write_object(GitObjectKind::Blob, b"old\n")
            .expect("write old blob");
        let new_blob = store
            .write_object(GitObjectKind::Blob, b"new\n")
            .expect("write new blob");
        let keep_blob = store
            .write_object(GitObjectKind::Blob, b"keep\n")
            .expect("write keep blob");
        let add_blob = store
            .write_object(GitObjectKind::Blob, b"add\n")
            .expect("write add blob");
        let old_index = GitIndex::from_entries(vec![
            IndexEntry::new("a.txt", old_blob, IndexMode::File, 4).expect("old a"),
            IndexEntry::new("b.txt", keep_blob.clone(), IndexMode::File, 5).expect("keep b"),
            IndexEntry::new("delete.txt", keep_blob.clone(), IndexMode::File, 5).expect("delete"),
        ])
        .expect("old index");
        let tree = write_tree_from_index(&store, &old_index).expect("write tree");
        let new_index = GitIndex::from_entries(vec![
            IndexEntry::new("a.txt", new_blob, IndexMode::File, 4).expect("new a"),
            IndexEntry::new("b.txt", keep_blob, IndexMode::File, 5).expect("keep b"),
            IndexEntry::new("c.txt", add_blob, IndexMode::File, 4).expect("add c"),
        ])
        .expect("new index");

        let diff = diff_index_to_tree(&store, &tree, &new_index).expect("diff");
        let actual = diff
            .into_iter()
            .map(|entry| {
                (
                    entry.status.name_status().to_owned(),
                    String::from_utf8(entry.path).expect("utf8 path"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                ("M".to_owned(), "a.txt".to_owned()),
                ("A".to_owned(), "c.txt".to_owned()),
                ("D".to_owned(), "delete.txt".to_owned()),
            ]
        );
    }

    #[test]
    fn exact_rename_detection_works_with_in_memory_indexes() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"same\n")
            .expect("write blob");
        let old_index = GitIndex::from_entries(vec![
            IndexEntry::new("old.txt", blob.clone(), IndexMode::File, 5).expect("old"),
        ])
        .expect("old index");
        let new_index = GitIndex::from_entries(vec![
            IndexEntry::new("new.txt", blob, IndexMode::File, 5).expect("new"),
        ])
        .expect("new index");

        let diff = diff_indexes_with_exact_renames(&old_index, &new_index).expect("diff");

        assert_eq!(
            diff,
            vec![IndexDiffEntry {
                status: IndexDiffStatus::Renamed,
                path: b"new.txt".to_vec(),
                old_path: Some(b"old.txt".to_vec()),
                similarity: Some(100),
            }]
        );
    }

    #[test]
    fn exact_copy_detection_works_with_in_memory_indexes() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"same\n")
            .expect("write blob");
        let old_index = GitIndex::from_entries(vec![
            IndexEntry::new("old.txt", blob.clone(), IndexMode::File, 5).expect("old"),
        ])
        .expect("old index");
        let new_index = GitIndex::from_entries(vec![
            IndexEntry::new("old.txt", blob.clone(), IndexMode::File, 5).expect("old"),
            IndexEntry::new("copy.txt", blob, IndexMode::File, 6).expect("copy"),
        ])
        .expect("new index");

        let diff =
            diff_indexes_with_exact_renames_and_copies(&old_index, &new_index, true).expect("diff");

        assert_eq!(
            diff,
            vec![IndexDiffEntry {
                status: IndexDiffStatus::Copied,
                path: b"copy.txt".to_vec(),
                old_path: Some(b"old.txt".to_vec()),
                similarity: Some(100),
            }]
        );
    }

    #[test]
    fn diff_capacity_hints_are_bounded() {
        assert_eq!(diff_entry_initial_capacity(2, 3), 5);
        assert_eq!(
            diff_entry_initial_capacity(usize::MAX, usize::MAX),
            DIFF_ENTRY_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(diff_path_initial_capacity(3), 3);
        assert_eq!(
            diff_path_initial_capacity(usize::MAX),
            DIFF_PATH_INITIAL_CAPACITY_LIMIT
        );
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
