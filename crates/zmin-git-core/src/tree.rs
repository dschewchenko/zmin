use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::sync::Arc;

use crate::GitObjectKind;
use crate::index::{GitIndex, IndexEntry, IndexMode};
use crate::object::{GitHashAlgorithm, ObjectId};
use crate::object_store::{GitObjectSink, GitObjectStore};

const TREE_OBJECT_CACHE_ENTRY_LIMIT: usize = 8192;
const TREE_WRITE_ENTRY_INITIAL_CAPACITY_LIMIT: usize = 8192;
const TREE_INDEX_ENTRY_INITIAL_CAPACITY_LIMIT: usize = 8192;
const TREE_INDEX_STACK_INITIAL_CAPACITY_LIMIT: usize = 64;
const TREE_INDEX_PATH_INITIAL_CAPACITY: usize = 256;
const TREE_ENCODE_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TreeMode {
    File,
    Executable,
    Symlink,
    Tree,
    Gitlink,
}

impl TreeMode {
    pub const fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::File => b"100644",
            Self::Executable => b"100755",
            Self::Symlink => b"120000",
            Self::Tree => b"40000",
            Self::Gitlink => b"160000",
        }
    }

    pub fn parse(bytes: &[u8]) -> Option<Self> {
        match bytes {
            b"100644" => Some(Self::File),
            b"100755" => Some(Self::Executable),
            b"120000" => Some(Self::Symlink),
            b"40000" => Some(Self::Tree),
            b"160000" => Some(Self::Gitlink),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: TreeMode,
    pub name: Vec<u8>,
    pub id: ObjectId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeObjectRef {
    pub mode: TreeMode,
    pub id: ObjectId,
}

impl TreeEntry {
    pub fn new(mode: TreeMode, name: impl Into<Vec<u8>>, id: ObjectId) -> io::Result<Self> {
        let name = name.into();
        validate_tree_name(&name)?;
        Ok(Self { mode, name, id })
    }
}

pub fn encode_tree(entries: &[TreeEntry]) -> io::Result<Vec<u8>> {
    let mut encoded = Vec::with_capacity(tree_encode_initial_capacity(entries));
    for entry in entries {
        validate_tree_name(&entry.name)?;
        encoded.extend_from_slice(entry.mode.as_bytes());
        encoded.push(b' ');
        encoded.extend_from_slice(&entry.name);
        encoded.push(0);
        encoded.extend_from_slice(entry.id.as_bytes());
    }
    Ok(encoded)
}

pub fn decode_tree(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<Vec<TreeEntry>> {
    let mut cursor = 0;
    let mut entries = Vec::with_capacity(tree_entry_capacity_hint(algorithm, bytes));
    while cursor < bytes.len() {
        let mode_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|offset| cursor + offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree mode missing space"))?;
        let mode = TreeMode::parse(&bytes[cursor..mode_end])
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid tree mode"))?;
        cursor = mode_end + 1;

        let name_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| cursor + offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree name missing NUL"))?;
        let name = bytes[cursor..name_end].to_vec();
        validate_tree_name(&name)?;
        cursor = name_end + 1;

        let digest_len = algorithm.digest_len();
        if bytes.len().saturating_sub(cursor) < digest_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tree object id is truncated",
            ));
        }
        let id = ObjectId::new(algorithm, &bytes[cursor..cursor + digest_len]);
        cursor += digest_len;
        entries.push(TreeEntry { mode, name, id });
    }
    Ok(entries)
}

pub fn decode_tree_object_refs(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<Vec<TreeObjectRef>> {
    let mut entries = Vec::with_capacity(tree_entry_capacity_hint(algorithm, bytes));
    for_each_tree_object_ref(algorithm, bytes, |mode, id| {
        entries.push(TreeObjectRef { mode, id });
        Ok(())
    })?;
    Ok(entries)
}

pub fn for_each_tree_object_ref(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
    mut visit: impl FnMut(TreeMode, ObjectId) -> io::Result<()>,
) -> io::Result<()> {
    let mut cursor = 0;
    while cursor < bytes.len() {
        let mode_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|offset| cursor + offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree mode missing space"))?;
        let mode = TreeMode::parse(&bytes[cursor..mode_end])
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid tree mode"))?;
        cursor = mode_end + 1;

        let name_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| cursor + offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree name missing NUL"))?;
        validate_tree_name(&bytes[cursor..name_end])?;
        cursor = name_end + 1;

        let digest_len = algorithm.digest_len();
        if bytes.len().saturating_sub(cursor) < digest_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tree object id is truncated",
            ));
        }
        let id = ObjectId::new(algorithm, &bytes[cursor..cursor + digest_len]);
        cursor += digest_len;
        visit(mode, id)?;
    }
    Ok(())
}

fn tree_entry_capacity_hint(algorithm: GitHashAlgorithm, bytes: &[u8]) -> usize {
    bytes.len() / (algorithm.digest_len() + 8)
}

pub fn write_tree_from_index<S: GitObjectSink>(
    store: &S,
    index: &GitIndex,
) -> io::Result<ObjectId> {
    let mut root = TreeNode::default();
    for entry in index.entries() {
        if entry.stage != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write-tree cannot write an index with unresolved conflicts",
            ));
        }
        root.insert(entry)?;
    }
    write_tree_node(store, &root)
}

pub fn read_tree_to_index<S: GitObjectStore>(
    store: &S,
    tree_id: &ObjectId,
) -> io::Result<GitIndex> {
    TreeObjectCache::new(store).read_tree_to_index(tree_id)
}

pub fn read_tree_to_index_uncached<S: GitObjectStore>(
    store: &S,
    tree_id: &ObjectId,
) -> io::Result<GitIndex> {
    let root_entries = read_tree(store, tree_id)?;
    let mut entries = Vec::with_capacity(tree_index_entry_initial_capacity(root_entries.len()));
    let mut path = Vec::with_capacity(TREE_INDEX_PATH_INITIAL_CAPACITY);
    collect_tree_index_entries_uncached(store, root_entries, &mut path, &mut entries)?;
    Ok(GitIndex::from_trusted_sorted_entries_unchecked(entries))
}

pub fn read_tree<S: GitObjectStore + ?Sized>(
    store: &S,
    tree_id: &ObjectId,
) -> io::Result<Vec<TreeEntry>> {
    let object = store.read_object(tree_id)?;
    if object.kind != GitObjectKind::Tree {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a tree",
        ));
    }
    decode_tree(tree_id.algorithm(), &object.content)
}

pub struct TreeObjectCache<'a, S: GitObjectStore + ?Sized> {
    store: &'a S,
    trees: RefCell<HashMap<ObjectId, Arc<[TreeEntry]>>>,
    entry_limit: usize,
}

impl<'a, S: GitObjectStore + ?Sized> TreeObjectCache<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self {
            store,
            trees: RefCell::new(HashMap::new()),
            entry_limit: TREE_OBJECT_CACHE_ENTRY_LIMIT,
        }
    }

    #[cfg(test)]
    fn with_entry_limit(store: &'a S, entry_limit: usize) -> Self {
        Self {
            store,
            trees: RefCell::new(HashMap::new()),
            entry_limit: entry_limit.max(1),
        }
    }

    pub fn read_tree(&self, tree_id: &ObjectId) -> io::Result<Arc<[TreeEntry]>> {
        if let Some(entries) = self.trees.borrow().get(tree_id) {
            return Ok(Arc::clone(entries));
        }

        let object = self.store.read_object(tree_id)?;
        if object.kind != GitObjectKind::Tree {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "object is not a tree",
            ));
        }
        let entries: Arc<[TreeEntry]> =
            Arc::from(decode_tree(tree_id.algorithm(), &object.content)?.into_boxed_slice());
        let mut trees = self.trees.borrow_mut();
        if trees.len() >= self.entry_limit {
            trees.clear();
        }
        trees.insert(tree_id.clone(), Arc::clone(&entries));
        Ok(entries)
    }

    pub fn read_tree_to_index(&self, tree_id: &ObjectId) -> io::Result<GitIndex> {
        let root_entries = self.read_tree(tree_id)?;
        let mut entries = Vec::with_capacity(tree_index_entry_initial_capacity(root_entries.len()));
        let mut path = Vec::with_capacity(TREE_INDEX_PATH_INITIAL_CAPACITY);
        collect_tree_index_entries(self, root_entries, &mut path, &mut entries)?;
        // Tree traversal only materializes stage-0 entries in path order from
        // Git tree objects, so the result is already trusted and sorted.
        Ok(GitIndex::from_trusted_sorted_entries_unchecked(entries))
    }
}

pub fn find_tree_entry<S: GitObjectStore + ?Sized>(
    store: &S,
    tree_id: &ObjectId,
    path: &[u8],
) -> io::Result<Option<TreeEntry>> {
    if path.is_empty() {
        return Ok(None);
    }

    let mut current_tree = tree_id.clone();
    let mut components = path.split(|byte| *byte == b'/').peekable();
    while let Some(component) = components.next() {
        if component.is_empty() {
            return Ok(None);
        }
        let entry = read_tree(store, &current_tree)?
            .into_iter()
            .find(|entry| entry.name == component);
        let Some(entry) = entry else {
            return Ok(None);
        };
        if components.peek().is_none() {
            return Ok(Some(entry));
        }
        if entry.mode != TreeMode::Tree {
            return Ok(None);
        }
        current_tree = entry.id;
    }
    Ok(None)
}

fn collect_tree_index_entries<S: GitObjectStore + ?Sized>(
    tree_cache: &TreeObjectCache<'_, S>,
    root_entries: Arc<[TreeEntry]>,
    path: &mut Vec<u8>,
    entries: &mut Vec<IndexEntry>,
) -> io::Result<()> {
    struct TreeIndexFrame {
        entries: Arc<[TreeEntry]>,
        next: usize,
        path_len: usize,
    }

    let initial_path_len = path.len();
    let mut stack = Vec::with_capacity(tree_index_stack_initial_capacity(root_entries.len()));
    stack.push(TreeIndexFrame {
        entries: root_entries,
        next: 0,
        path_len: initial_path_len,
    });

    while let Some(frame) = stack.last_mut() {
        if frame.next == frame.entries.len() {
            let path_len = frame.path_len;
            stack.pop();
            path.truncate(path_len);
            continue;
        }

        let entry = &frame.entries[frame.next];
        let mode = entry.mode;
        let id = entry.id.clone();
        frame.next += 1;

        path.truncate(frame.path_len);
        path.extend_from_slice(&entry.name);
        match mode {
            TreeMode::Tree => {
                path.push(b'/');
                stack.push(TreeIndexFrame {
                    entries: tree_cache.read_tree(&id)?,
                    next: 0,
                    path_len: path.len(),
                });
            }
            TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                let mode = index_mode_from_tree_mode(mode)?;
                entries.push(IndexEntry {
                    path: path.clone(),
                    id,
                    mode,
                    stage: 0,
                    size: 0,
                    ctime_seconds: 0,
                    ctime_nanoseconds: 0,
                    mtime_seconds: 0,
                    mtime_nanoseconds: 0,
                    dev: 0,
                    ino: 0,
                    uid: 0,
                    gid: 0,
                    flags: 0,
                });
            }
        }
    }
    path.truncate(initial_path_len);
    Ok(())
}

fn collect_tree_index_entries_uncached<S: GitObjectStore>(
    store: &S,
    root_entries: Vec<TreeEntry>,
    path: &mut Vec<u8>,
    entries: &mut Vec<IndexEntry>,
) -> io::Result<()> {
    struct TreeIndexFrame {
        entries: Vec<TreeEntry>,
        next: usize,
        path_len: usize,
    }

    let initial_path_len = path.len();
    let mut stack = Vec::with_capacity(tree_index_stack_initial_capacity(root_entries.len()));
    stack.push(TreeIndexFrame {
        entries: root_entries,
        next: 0,
        path_len: initial_path_len,
    });

    while let Some(frame) = stack.last_mut() {
        if frame.next == frame.entries.len() {
            let path_len = frame.path_len;
            stack.pop();
            path.truncate(path_len);
            continue;
        }

        let entry = &frame.entries[frame.next];
        let mode = entry.mode;
        let id = entry.id.clone();
        frame.next += 1;

        path.truncate(frame.path_len);
        path.extend_from_slice(&entry.name);
        match mode {
            TreeMode::Tree => {
                path.push(b'/');
                stack.push(TreeIndexFrame {
                    entries: read_tree(store, &id)?,
                    next: 0,
                    path_len: path.len(),
                });
            }
            TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                let mode = index_mode_from_tree_mode(mode)?;
                entries.push(IndexEntry {
                    path: path.clone(),
                    id,
                    mode,
                    stage: 0,
                    size: 0,
                    ctime_seconds: 0,
                    ctime_nanoseconds: 0,
                    mtime_seconds: 0,
                    mtime_nanoseconds: 0,
                    dev: 0,
                    ino: 0,
                    uid: 0,
                    gid: 0,
                    flags: 0,
                });
            }
        }
    }
    path.truncate(initial_path_len);
    Ok(())
}

fn tree_index_entry_initial_capacity(root_entry_count: usize) -> usize {
    root_entry_count.min(TREE_INDEX_ENTRY_INITIAL_CAPACITY_LIMIT)
}

fn tree_index_stack_initial_capacity(root_entry_count: usize) -> usize {
    root_entry_count
        .min(TREE_INDEX_STACK_INITIAL_CAPACITY_LIMIT)
        .max(1)
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

#[derive(Default)]
struct TreeNode {
    children: BTreeMap<Vec<u8>, TreeNode>,
    leaves: BTreeMap<Vec<u8>, (IndexMode, ObjectId)>,
}

impl Drop for TreeNode {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children)
            .into_values()
            .collect::<Vec<_>>();
        while let Some(mut node) = pending.pop() {
            pending.extend(std::mem::take(&mut node.children).into_values());
        }
    }
}

impl TreeNode {
    fn insert(&mut self, entry: &IndexEntry) -> io::Result<()> {
        let mut parts = entry.path.split(|byte| *byte == b'/').peekable();
        let mut node = self;
        while let Some(part) = parts.next() {
            if part.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "git index path contains an empty component",
                ));
            }
            if parts.peek().is_none() {
                if node.children.contains_key(part) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "git index path collides with a tree",
                    ));
                }
                node.leaves
                    .insert(part.to_vec(), (entry.mode, entry.id.clone()));
                return Ok(());
            }
            if node.leaves.contains_key(part) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "git index path collides with a file",
                ));
            }
            node = node.children.entry(part.to_vec()).or_default();
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "git index path is empty",
        ))
    }
}

fn write_tree_node<S: GitObjectSink>(store: &S, node: &TreeNode) -> io::Result<ObjectId> {
    struct WriteTreeFrame<'a> {
        name_in_parent: Option<&'a [u8]>,
        node: &'a TreeNode,
        children: std::collections::btree_map::Iter<'a, Vec<u8>, TreeNode>,
        entries: Vec<TreeEntry>,
    }

    let mut stack = vec![WriteTreeFrame {
        name_in_parent: None,
        node,
        children: node.children.iter(),
        entries: Vec::with_capacity(tree_write_entry_initial_capacity(
            node.children.len(),
            node.leaves.len(),
        )),
    }];

    while let Some(frame) = stack.last_mut() {
        if let Some((name, child)) = frame.children.next() {
            stack.push(WriteTreeFrame {
                name_in_parent: Some(name),
                node: child,
                children: child.children.iter(),
                entries: Vec::with_capacity(tree_write_entry_initial_capacity(
                    child.children.len(),
                    child.leaves.len(),
                )),
            });
            continue;
        }

        for (name, (mode, id)) in &frame.node.leaves {
            frame
                .entries
                .push(TreeEntry::new(mode.tree_mode(), name.clone(), id.clone())?);
        }
        frame.entries.sort_by(tree_entry_cmp);
        let encoded = encode_tree(&frame.entries)?;
        let id = store.write_object(GitObjectKind::Tree, &encoded)?;
        let name_in_parent = frame.name_in_parent;
        stack.pop();

        let Some(parent) = stack.last_mut() else {
            return Ok(id);
        };
        parent.entries.push(TreeEntry::new(
            TreeMode::Tree,
            name_in_parent.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "tree frame missing parent entry name",
                )
            })?,
            id,
        )?);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "git index path is empty",
    ))
}

fn tree_write_entry_initial_capacity(children_len: usize, leaves_len: usize) -> usize {
    children_len
        .saturating_add(leaves_len)
        .min(TREE_WRITE_ENTRY_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn tree_encode_initial_capacity(entries: &[TreeEntry]) -> usize {
    let bytes = entries.iter().fold(0_usize, |total, entry| {
        total
            .saturating_add(entry.mode.as_bytes().len())
            .saturating_add(2)
            .saturating_add(entry.name.len())
            .saturating_add(entry.id.algorithm().digest_len())
    });
    bytes.min(TREE_ENCODE_INITIAL_CAPACITY_LIMIT)
}

fn tree_entry_cmp(left: &TreeEntry, right: &TreeEntry) -> std::cmp::Ordering {
    let left_tree = left.mode == TreeMode::Tree;
    let right_tree = right.mode == TreeMode::Tree;
    compare_tree_names(&left.name, left_tree, &right.name, right_tree)
}

fn compare_tree_names(
    left: &[u8],
    left_tree: bool,
    right: &[u8],
    right_tree: bool,
) -> std::cmp::Ordering {
    let mut idx = 0;
    loop {
        let left_byte = tree_name_byte(left, left_tree, idx);
        let right_byte = tree_name_byte(right, right_tree, idx);
        match (left_byte, right_byte) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
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

fn validate_tree_name(name: &[u8]) -> io::Result<()> {
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tree entry name is empty",
        ));
    }
    if name.contains(&0) || name.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tree entry name contains invalid byte",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        GitIndex, GitObjectKind, GitObjectSink, GitObjectStore, InMemoryObjectStore, IndexEntry,
        IndexMode, LooseObject, LooseObjectStore,
    };

    #[test]
    fn tree_write_entry_initial_capacity_is_bounded() {
        assert_eq!(
            tree_write_entry_initial_capacity(usize::MAX, 1),
            TREE_WRITE_ENTRY_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(tree_write_entry_initial_capacity(2, 3), 5);
        assert_eq!(tree_write_entry_initial_capacity(0, 0), 1);
    }

    #[test]
    fn tree_encode_initial_capacity_is_bounded() {
        let blob = crate::hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"content\n");
        let entries = vec![
            TreeEntry::new(TreeMode::File, "a.txt", blob.clone()).expect("tree entry"),
            TreeEntry::new(TreeMode::Executable, "script.sh", blob.clone()).expect("tree entry"),
        ];
        let expected = entries
            .iter()
            .map(|entry| {
                entry.mode.as_bytes().len()
                    + 2
                    + entry.name.len()
                    + entry.id.algorithm().digest_len()
            })
            .sum::<usize>();

        assert_eq!(tree_encode_initial_capacity(&entries), expected);

        let large_name = vec![b'a'; TREE_ENCODE_INITIAL_CAPACITY_LIMIT];
        let large = vec![TreeEntry::new(TreeMode::File, large_name, blob).expect("tree entry")];
        assert_eq!(
            tree_encode_initial_capacity(&large),
            TREE_ENCODE_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn tree_roundtrip_preserves_entry() {
        let blob = crate::hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, b"content\n");
        let entries = vec![
            TreeEntry::new(TreeMode::File, b"README.md".to_vec(), blob.clone())
                .expect("tree entry"),
        ];

        let encoded = encode_tree(&entries).expect("encode tree");
        let decoded = decode_tree(GitHashAlgorithm::Sha1, &encoded).expect("decode tree");

        assert_eq!(decoded, entries);
    }

    #[test]
    fn encoded_tree_is_readable_by_stock_git() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"hello tree\n")
            .expect("write blob");
        let tree_bytes = encode_tree(&[
            TreeEntry::new(TreeMode::File, "README.md", blob.clone()).expect("tree entry")
        ])
        .expect("encode tree");
        let tree = store
            .write_object(GitObjectKind::Tree, &tree_bytes)
            .expect("write tree");

        let rendered = git(&repo, ["cat-file", "-p", &tree.to_hex()]);

        assert_eq!(
            rendered,
            format!("100644 blob {}\tREADME.md", blob.to_hex())
        );
    }

    #[test]
    fn write_tree_from_index_matches_stock_git() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let readme = store
            .write_object(GitObjectKind::Blob, b"hello tree index\n")
            .expect("write blob");
        let guide = store
            .write_object(GitObjectKind::Blob, b"nested\n")
            .expect("write blob");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("README.md", readme, IndexMode::File, 17).expect("entry"),
            IndexEntry::new("docs/guide.md", guide, IndexMode::File, 7).expect("entry"),
        ])
        .expect("index");
        index
            .write_to_path(repo.path().join(".git/index"))
            .expect("write index");

        let ours = write_tree_from_index(&store, &index)
            .expect("write tree")
            .to_hex();
        let git_tree = git(&repo, ["write-tree"]);

        assert_eq!(ours, git_tree);
    }

    #[test]
    fn read_tree_to_index_matches_stock_git() {
        let repo = git_init();
        std::fs::create_dir_all(repo.path().join("docs")).expect("mkdir docs");
        std::fs::write(repo.path().join("README.md"), b"readme\n").expect("write readme");
        std::fs::write(repo.path().join("docs/guide.md"), b"guide\n").expect("write guide");
        git(&repo, ["add", "README.md", "docs/guide.md"]);
        let expected = git(&repo, ["ls-files", "--stage"]);
        let tree = git(&repo, ["write-tree"]);
        std::fs::remove_file(repo.path().join(".git/index")).expect("remove index");

        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let tree_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &tree).expect("tree id");
        let index = read_tree_to_index(&store, &tree_id).expect("read tree to index");
        let uncached =
            read_tree_to_index_uncached(&store, &tree_id).expect("read uncached tree to index");
        assert_eq!(uncached, index);
        index
            .write_to_path(repo.path().join(".git/index"))
            .expect("write index");

        assert_eq!(git(&repo, ["ls-files", "--stage"]), expected);
    }

    #[test]
    fn tree_primitives_work_with_in_memory_object_store() {
        let store = crate::InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob_id = store
            .write_object(GitObjectKind::Blob, b"opfs-ready\n")
            .expect("write blob");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new(
                b"docs/page.md".to_vec(),
                blob_id.clone(),
                IndexMode::File,
                0,
            )
            .expect("index entry"),
        ])
        .expect("index");

        let tree_id = write_tree_from_index(&store, &index).expect("write tree");
        let entry = find_tree_entry(&store, &tree_id, b"docs/page.md")
            .expect("find tree entry")
            .expect("tree entry");
        let restored = read_tree_to_index(&store, &tree_id).expect("read tree to index");

        assert_eq!(entry.id, blob_id);
        assert_eq!(restored.entries()[0].path, b"docs/page.md");
    }

    #[test]
    fn tree_object_cache_reuses_decoded_tree_objects() {
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
        let blob_id = store
            .inner
            .write_object(GitObjectKind::Blob, b"cached\n")
            .expect("write blob");
        let child_tree_id = store
            .inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "cached.txt", blob_id).expect("tree entry")
                ])
                .expect("encode tree"),
            )
            .expect("write tree");
        let root_tree_id = store
            .inner
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "a", child_tree_id.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "b", child_tree_id.clone()).expect("tree entry"),
                ])
                .expect("encode tree"),
            )
            .expect("write tree");
        let cache = TreeObjectCache::new(&store);

        assert_eq!(
            cache.read_tree(&child_tree_id).expect("first read").len(),
            1
        );
        assert_eq!(
            cache.read_tree(&child_tree_id).expect("second read").len(),
            1
        );
        let index = cache
            .read_tree_to_index(&root_tree_id)
            .expect("read cached tree to index");

        assert_eq!(index.entries().len(), 2);
        assert_eq!(store.reads.get(), 2);
    }

    #[test]
    fn tree_index_capacity_hints_are_bounded() {
        assert_eq!(tree_index_entry_initial_capacity(3), 3);
        assert_eq!(
            tree_index_entry_initial_capacity(usize::MAX),
            TREE_INDEX_ENTRY_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(tree_index_stack_initial_capacity(0), 1);
        assert_eq!(tree_index_stack_initial_capacity(3), 3);
        assert_eq!(
            tree_index_stack_initial_capacity(usize::MAX),
            TREE_INDEX_STACK_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn tree_object_cache_bounds_retained_entries() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first_blob = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second_blob = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let first_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "first.txt", first_blob).expect("tree entry")
                ])
                .expect("encode first tree"),
            )
            .expect("write first tree");
        let second_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "second.txt", second_blob).expect("tree entry")
                ])
                .expect("encode second tree"),
            )
            .expect("write second tree");
        let cache = TreeObjectCache::with_entry_limit(&store, 1);

        cache.read_tree(&first_tree).expect("read first tree");
        cache.read_tree(&second_tree).expect("read second tree");

        let trees = cache.trees.borrow();
        assert_eq!(trees.len(), 1);
        assert!(trees.contains_key(&second_tree));
    }

    #[test]
    fn read_tree_to_index_handles_deep_trees_without_recursion() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob_id = store
            .write_object(GitObjectKind::Blob, b"deep\n")
            .expect("write blob");
        let mut tree_id = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "leaf.txt", blob_id).expect("tree entry")
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        for depth in (0..2048).rev() {
            tree_id = store
                .write_object(
                    GitObjectKind::Tree,
                    &encode_tree(&[TreeEntry::new(
                        TreeMode::Tree,
                        format!("d{depth:04}"),
                        tree_id,
                    )
                    .expect("tree entry")])
                    .expect("encode tree"),
                )
                .expect("write tree");
        }

        let index = read_tree_to_index(&store, &tree_id).expect("read deep tree");
        let uncached =
            read_tree_to_index_uncached(&store, &tree_id).expect("read uncached deep tree");
        let mut expected_path = (0..2048)
            .map(|depth| format!("d{depth:04}"))
            .collect::<Vec<_>>()
            .join("/")
            .into_bytes();
        expected_path.extend_from_slice(b"/leaf.txt");

        assert_eq!(index.entries().len(), 1);
        assert_eq!(index.entries()[0].path, expected_path);
        assert_eq!(uncached, index);
    }

    #[test]
    fn write_tree_from_index_handles_deep_paths_without_recursion() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob_id = store
            .write_object(GitObjectKind::Blob, b"deep-write\n")
            .expect("write blob");
        let mut path = (0..2048)
            .map(|depth| format!("d{depth:04}"))
            .collect::<Vec<_>>()
            .join("/")
            .into_bytes();
        path.extend_from_slice(b"/leaf.txt");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new(path.clone(), blob_id, IndexMode::File, 0).expect("index entry"),
        ])
        .expect("index");

        let tree_id = write_tree_from_index(&store, &index).expect("write deep tree");
        let restored = read_tree_to_index(&store, &tree_id).expect("read deep tree");

        assert_eq!(restored.entries().len(), 1);
        assert_eq!(restored.entries()[0].path, path);
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
