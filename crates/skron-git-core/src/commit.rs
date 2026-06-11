use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use crate::object_store::GitObjectStore;
use crate::{GitHashAlgorithm, GitObjectKind, LooseObject, ObjectId};

const COMMIT_ENCODE_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitObject {
    pub tree: ObjectId,
    pub parents: Vec<ObjectId>,
    pub author: Vec<u8>,
    pub committer: Vec<u8>,
    pub message: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitLinks {
    pub tree: ObjectId,
    pub parents: Vec<ObjectId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub name: String,
    pub email: String,
    pub timestamp: i64,
    pub timezone: String,
}

impl Signature {
    pub fn new(
        name: impl Into<String>,
        email: impl Into<String>,
        timestamp: i64,
        timezone: impl Into<String>,
    ) -> io::Result<Self> {
        let signature = Self {
            name: name.into(),
            email: email.into(),
            timestamp,
            timezone: timezone.into(),
        };
        validate_signature_field(&signature.name, "signature name")?;
        validate_signature_field(&signature.email, "signature email")?;
        validate_timezone(&signature.timezone)?;
        Ok(signature)
    }

    pub(crate) fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.name.as_bytes());
        out.extend_from_slice(b" <");
        out.extend_from_slice(self.email.as_bytes());
        out.extend_from_slice(b"> ");
        write_decimal_i64(out, self.timestamp);
        out.push(b' ');
        out.extend_from_slice(self.timezone.as_bytes());
    }

    pub(crate) fn encoded_len(&self) -> usize {
        self.name
            .len()
            .saturating_add(self.email.len())
            .saturating_add(self.timezone.len())
            .saturating_add(5)
            .saturating_add(decimal_i64_len(self.timestamp))
    }
}

#[derive(Debug, Clone)]
pub struct CommitBuilder {
    tree: ObjectId,
    parents: Vec<ObjectId>,
    author: Signature,
    committer: Signature,
    message: Vec<u8>,
}

impl CommitBuilder {
    pub fn new(tree: ObjectId, author: Signature, committer: Signature) -> Self {
        Self {
            tree,
            parents: Vec::new(),
            author,
            committer,
            message: Vec::new(),
        }
    }

    pub fn parent(mut self, parent: ObjectId) -> Self {
        self.parents.push(parent);
        self
    }

    pub fn message(mut self, message: impl Into<Vec<u8>>) -> io::Result<Self> {
        let message = message.into();
        if message.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "commit message contains NUL",
            ));
        }
        self.message = message;
        Ok(self)
    }

    pub fn encode(&self) -> io::Result<Vec<u8>> {
        encode_commit(
            &self.tree,
            &self.parents,
            &self.author,
            &self.committer,
            &self.message,
        )
    }
}

pub fn encode_commit(
    tree: &ObjectId,
    parents: &[ObjectId],
    author: &Signature,
    committer: &Signature,
    message: &[u8],
) -> io::Result<Vec<u8>> {
    if message.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "commit message contains NUL",
        ));
    }
    let algorithm = tree.algorithm();
    if parents.iter().any(|parent| parent.algorithm() != algorithm) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "commit parent hash algorithm does not match tree",
        ));
    }

    let mut out = Vec::with_capacity(commit_encode_initial_capacity(
        tree, parents, author, committer, message,
    ));
    out.extend_from_slice(b"tree ");
    tree.write_hex_bytes(&mut out);
    out.push(b'\n');
    for parent in parents {
        out.extend_from_slice(b"parent ");
        parent.write_hex_bytes(&mut out);
        out.push(b'\n');
    }
    out.extend_from_slice(b"author ");
    author.write_to(&mut out);
    out.push(b'\n');
    out.extend_from_slice(b"committer ");
    committer.write_to(&mut out);
    out.extend_from_slice(b"\n\n");
    out.extend_from_slice(message);
    Ok(out)
}

fn commit_encode_initial_capacity(
    tree: &ObjectId,
    parents: &[ObjectId],
    author: &Signature,
    committer: &Signature,
    message: &[u8],
) -> usize {
    let parent_bytes = parents.iter().fold(0_usize, |total, parent| {
        total.saturating_add(8).saturating_add(parent.hex_len())
    });
    let bytes = 6_usize
        .saturating_add(tree.hex_len())
        .saturating_add(parent_bytes)
        .saturating_add(8)
        .saturating_add(author.encoded_len())
        .saturating_add(12)
        .saturating_add(committer.encoded_len())
        .saturating_add(message.len());
    bytes.min(COMMIT_ENCODE_INITIAL_CAPACITY_LIMIT)
}

fn decimal_i64_len(value: i64) -> usize {
    if value < 0 {
        1 + decimal_u64_len(value.unsigned_abs())
    } else {
        decimal_u64_len(value as u64)
    }
}

fn decimal_u64_len(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 10 {
        value /= 10;
        len += 1;
    }
    len
}

pub fn decode_commit(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<CommitObject> {
    let message_start = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|idx| idx + 2)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "commit missing header end"))?;
    let headers = &bytes[..message_start - 2];
    let message = bytes[message_start..].to_vec();

    let mut tree = None;
    let mut parents = Vec::with_capacity(1);
    let mut author = None;
    let mut committer = None;

    for line in headers.split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"tree ") {
            if tree.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "commit has multiple tree headers",
                ));
            }
            tree = Some(parse_commit_id(algorithm, value, "tree")?);
        } else if let Some(value) = line.strip_prefix(b"parent ") {
            parents.push(parse_commit_id(algorithm, value, "parent")?);
        } else if let Some(value) = line.strip_prefix(b"author ") {
            author = Some(value.to_vec());
        } else if let Some(value) = line.strip_prefix(b"committer ") {
            committer = Some(value.to_vec());
        }
    }

    Ok(CommitObject {
        tree: tree.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "commit missing tree header")
        })?,
        parents,
        author: author.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "commit missing author header")
        })?,
        committer: committer.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "commit missing committer header",
            )
        })?,
        message,
    })
}

pub fn decode_commit_links(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<CommitLinks> {
    let message_start = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "commit missing header end"))?;
    let headers = &bytes[..message_start];

    let mut tree = None;
    let mut parents = Vec::with_capacity(1);
    let mut author = false;
    let mut committer = false;

    for line in headers.split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"tree ") {
            if tree.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "commit has multiple tree headers",
                ));
            }
            tree = Some(parse_commit_id(algorithm, value, "tree")?);
        } else if let Some(value) = line.strip_prefix(b"parent ") {
            parents.push(parse_commit_id(algorithm, value, "parent")?);
        } else if line.strip_prefix(b"author ").is_some() {
            author = true;
        } else if line.strip_prefix(b"committer ").is_some() {
            committer = true;
        }
    }

    if !author {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit missing author header",
        ));
    }
    if !committer {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit missing committer header",
        ));
    }

    Ok(CommitLinks {
        tree: tree.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "commit missing tree header")
        })?,
        parents,
    })
}

pub struct CommitObjectCache<'a, S: GitObjectStore + ?Sized> {
    store: &'a S,
    commits: RefCell<HashMap<ObjectId, Arc<CommitObject>>>,
    commit_links: RefCell<HashMap<ObjectId, Arc<CommitLinks>>>,
}

impl<'a, S: GitObjectStore + ?Sized> CommitObjectCache<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self {
            store,
            commits: RefCell::new(HashMap::new()),
            commit_links: RefCell::new(HashMap::new()),
        }
    }

    pub fn read_commit(&self, id: &ObjectId) -> io::Result<Arc<CommitObject>> {
        if let Some(commit) = self.commits.borrow().get(id) {
            return Ok(Arc::clone(commit));
        }

        let object = self.store.read_object(id)?;
        self.read_loaded_commit(object)
    }

    pub fn read_loaded_commit(&self, object: LooseObject) -> io::Result<Arc<CommitObject>> {
        let object_id = object.id;
        if let Some(commit) = self.commits.borrow().get(&object_id) {
            return Ok(Arc::clone(commit));
        }
        if let Some(links) = self.commit_links.borrow().get(&object_id) {
            let mut commit = CommitObject {
                tree: links.tree.clone(),
                parents: links.parents.clone(),
                author: Vec::new(),
                committer: Vec::new(),
                message: Vec::new(),
            };
            let decoded = decode_commit(object_id.algorithm(), &object.content)?;
            commit.author = decoded.author;
            commit.committer = decoded.committer;
            commit.message = decoded.message;
            let commit = Arc::new(commit);
            self.commits
                .borrow_mut()
                .insert(object_id, Arc::clone(&commit));
            return Ok(commit);
        }
        if object.kind != GitObjectKind::Commit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "object is not a commit",
            ));
        }
        let commit = Arc::new(decode_commit(object_id.algorithm(), &object.content)?);
        self.commits
            .borrow_mut()
            .insert(object_id.clone(), Arc::clone(&commit));
        self.commit_links.borrow_mut().insert(
            object_id,
            Arc::new(CommitLinks {
                tree: commit.tree.clone(),
                parents: commit.parents.clone(),
            }),
        );
        Ok(commit)
    }

    pub fn read_commit_links(&self, id: &ObjectId) -> io::Result<Arc<CommitLinks>> {
        if let Some(links) = self.commit_links.borrow().get(id) {
            return Ok(Arc::clone(links));
        }
        if let Some(commit) = self.commits.borrow().get(id) {
            let links = Arc::new(CommitLinks {
                tree: commit.tree.clone(),
                parents: commit.parents.clone(),
            });
            self.commit_links
                .borrow_mut()
                .insert(id.clone(), Arc::clone(&links));
            return Ok(links);
        }

        let object = self.store.read_object(id)?;
        if object.kind != GitObjectKind::Commit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "object is not a commit",
            ));
        }
        let links = Arc::new(decode_commit_links(object.id.algorithm(), &object.content)?);
        self.commit_links
            .borrow_mut()
            .insert(object.id, Arc::clone(&links));
        Ok(links)
    }
}

fn parse_commit_id(algorithm: GitHashAlgorithm, value: &[u8], label: &str) -> io::Result<ObjectId> {
    let value = std::str::from_utf8(value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("commit {label} id is not utf-8"),
        )
    })?;
    ObjectId::from_hex(algorithm, value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("commit {label} id is invalid"),
        )
    })
}

fn validate_signature_field(value: &str, label: &str) -> io::Result<()> {
    if value.is_empty() || value.contains('\n') || value.contains('<') || value.contains('>') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{label} contains invalid characters"),
        ));
    }
    Ok(())
}

fn validate_timezone(value: &str) -> io::Result<()> {
    let bytes = value.as_bytes();
    if bytes.len() != 5 || (bytes[0] != b'+' && bytes[0] != b'-') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "timezone must use +/-HHMM",
        ));
    }
    if !bytes[1..].iter().all(u8::is_ascii_digit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "timezone must use numeric +/-HHMM",
        ));
    }
    Ok(())
}

fn write_decimal_i64(out: &mut Vec<u8>, value: i64) {
    if value < 0 {
        out.push(b'-');
        write_decimal_u64(out, value.unsigned_abs());
    } else {
        write_decimal_u64(out, value as u64);
    }
}

fn write_decimal_u64(out: &mut Vec<u8>, mut value: u64) {
    let mut buf = [0_u8; 20];
    let mut cursor = buf.len();
    if value == 0 {
        cursor -= 1;
        buf[cursor] = b'0';
    } else {
        while value > 0 {
            cursor -= 1;
            buf[cursor] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    out.extend_from_slice(&buf[cursor..]);
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::{
        GitHashAlgorithm, GitObjectKind, GitObjectSink, GitObjectStore, InMemoryObjectStore,
        LooseObject, LooseObjectStore, TreeEntry, TreeMode, encode_tree,
    };

    #[test]
    fn commit_encode_initial_capacity_is_bounded() {
        let tree = crate::hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Tree, b"tree\n");
        let parent = crate::hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Commit, b"parent\n");
        let author = Signature::new("Skron Test", "skron@example.com", 1_700_000_000, "+0000")
            .expect("author");
        let committer = Signature::new("Skron Test", "skron@example.com", -1_700_000_000, "+0000")
            .expect("committer");
        let message = b"initial commit\n";
        let expected = encode_commit(
            &tree,
            std::slice::from_ref(&parent),
            &author,
            &committer,
            message,
        )
        .expect("encode commit")
        .len();

        assert_eq!(
            commit_encode_initial_capacity(&tree, &[parent], &author, &committer, message),
            expected
        );
        assert_eq!(decimal_i64_len(0), 1);
        assert_eq!(decimal_i64_len(-1), 2);

        let large_message = vec![b'a'; COMMIT_ENCODE_INITIAL_CAPACITY_LIMIT];
        assert_eq!(
            commit_encode_initial_capacity(&tree, &[], &author, &committer, &large_message),
            COMMIT_ENCODE_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn commit_object_is_readable_by_stock_git() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"hello commit\n")
            .expect("write blob");
        let tree_bytes = encode_tree(&[
            TreeEntry::new(TreeMode::File, "README.md", blob.clone()).expect("tree entry")
        ])
        .expect("encode tree");
        let tree = store
            .write_object(GitObjectKind::Tree, &tree_bytes)
            .expect("write tree");
        let signature = Signature::new("Skron Test", "skron@example.com", 1_700_000_000, "+0000")
            .expect("signature");
        let commit_bytes = CommitBuilder::new(tree.clone(), signature.clone(), signature)
            .message(b"initial commit\n".to_vec())
            .expect("message")
            .encode()
            .expect("encode commit");
        let commit = store
            .write_object(GitObjectKind::Commit, &commit_bytes)
            .expect("write commit");

        let rendered = git(&repo, ["cat-file", "-p", &commit.to_hex()]);

        assert!(rendered.contains(&format!("tree {}", tree.to_hex())));
        assert!(rendered.contains("author Skron Test <skron@example.com> 1700000000 +0000"));
        assert!(rendered.contains("committer Skron Test <skron@example.com> 1700000000 +0000"));
        assert!(rendered.ends_with("\n\ninitial commit"));
    }

    #[test]
    fn decodes_stock_commit_tree_parents_and_message() {
        let repo = git_init();
        git(&repo, ["config", "user.name", "Skron Test"]);
        git(&repo, ["config", "user.email", "skron@example.com"]);
        std::fs::write(repo.path().join("README.md"), b"one\n").expect("write file");
        git(&repo, ["add", "README.md"]);
        git(
            &repo,
            [
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "one",
            ],
        );
        let first = git(&repo, ["rev-parse", "HEAD"]);
        let tree = git(&repo, ["rev-parse", "HEAD^{tree}"]);
        std::fs::write(repo.path().join("README.md"), b"two\n").expect("write file");
        git(&repo, ["add", "README.md"]);
        git(
            &repo,
            [
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "two",
            ],
        );
        let second = git(&repo, ["rev-parse", "HEAD"]);
        let second_tree = git(&repo, ["rev-parse", "HEAD^{tree}"]);
        let raw = git_raw(&repo, ["cat-file", "-p", &second]);

        let decoded = decode_commit(GitHashAlgorithm::Sha1, &raw).expect("decode commit");

        assert_eq!(
            decoded.tree,
            ObjectId::from_hex(GitHashAlgorithm::Sha1, &second_tree).expect("tree id")
        );
        assert_eq!(
            decoded.parents,
            vec![ObjectId::from_hex(GitHashAlgorithm::Sha1, &first).expect("parent id")]
        );
        assert!(
            decoded
                .author
                .starts_with(b"Skron Test <skron@example.com>")
        );
        assert!(
            decoded
                .committer
                .starts_with(b"Skron Test <skron@example.com>")
        );
        assert_eq!(decoded.message, b"two\n");
        assert_ne!(tree, second_tree);
    }

    #[test]
    fn commit_object_cache_reuses_decoded_commits() {
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
        let tree = store
            .inner
            .write_object(GitObjectKind::Tree, &encode_tree(&[]).expect("encode tree"))
            .expect("write tree");
        let signature = Signature::new("Skron Test", "skron@example.com", 1_700_000_000, "+0000")
            .expect("signature");
        let commit_bytes = CommitBuilder::new(tree, signature.clone(), signature)
            .message(b"cached commit\n".to_vec())
            .expect("message")
            .encode()
            .expect("encode commit");
        let commit = store
            .inner
            .write_object(GitObjectKind::Commit, &commit_bytes)
            .expect("write commit");
        let cache = CommitObjectCache::new(&store);

        assert_eq!(
            cache
                .read_commit(&commit)
                .expect("first read")
                .parents
                .len(),
            0
        );
        assert_eq!(
            cache.read_commit(&commit).expect("second read").message,
            b"cached commit\n"
        );

        assert_eq!(store.reads.get(), 1);
    }

    #[test]
    fn commit_object_cache_accepts_loaded_commit_without_second_store_read() {
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
        let tree = store
            .inner
            .write_object(GitObjectKind::Tree, &encode_tree(&[]).expect("encode tree"))
            .expect("write tree");
        let signature = Signature::new("Skron Test", "skron@example.com", 1_700_000_000, "+0000")
            .expect("signature");
        let commit_bytes = CommitBuilder::new(tree, signature.clone(), signature)
            .message(b"loaded commit\n".to_vec())
            .expect("message")
            .encode()
            .expect("encode commit");
        let commit_id = store
            .inner
            .write_object(GitObjectKind::Commit, &commit_bytes)
            .expect("write commit");
        let loaded = store.inner.read_object(&commit_id).expect("load commit");
        let cache = CommitObjectCache::new(&store);

        assert_eq!(
            cache
                .read_loaded_commit(loaded)
                .expect("loaded read")
                .message,
            b"loaded commit\n"
        );
        assert_eq!(
            cache
                .read_commit(&commit_id)
                .expect("cached read")
                .parents
                .len(),
            0
        );

        assert_eq!(store.reads.get(), 0);
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
}
