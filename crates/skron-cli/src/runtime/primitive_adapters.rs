use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use skron_git_core::{
    CommitBuilder, CommitObject, CommitObjectCache, GitHashAlgorithm, GitObjectKind, GitObjectSink,
    GitObjectStore as CoreGitObjectStore, LooseObjectStore, ObjectId, PackRefsOptions, RefStore,
    RefTarget, TreeObjectCache,
};
use skron_git_core::{TreeEntry, TreeMode, decode_commit, encode_tree};
use skron_primitives::git_runtime::{
    GitEndpoint, GitObjectEnvelope, GitObjectStore as PrimitiveGitObjectStore, GitPatchRenderer,
    GitPrimitiveRuntime, GitRefName, GitRefsStore, GitRepoPath, GitRewriteEngine, GitTransport,
    GitTransportService, GitWorktreeEngine, RefDiscovery, RefUpdate, RefUpdateAction,
    TransferRequest,
};
use skron_primitives::{Error as PrimitiveError, Result as PrimitiveResult};

use super::{
    CliError, FormatPatchBlobCache, FormatPatchContext, FormatPatchEntry, GitRepo,
    default_abbrev_len, local_clone_source, local_repository_path_from_location,
    normalize_git_path, read_config_file, signature_from_commit_bytes, transport_commands,
    write_format_patch_with_tree_diff_cached,
};

// The legacy CLI runtime module currently owns repository-level object stores as concrete
// `skron_git_core` types. This adapter bridges that implementation into shared primitives
// so custom clients and WASM can consume a stable object contract later.

pub(crate) fn parse_oid(raw_oid: &str) -> PrimitiveResult<ObjectId> {
    let algorithm = match raw_oid.len() {
        40 => GitHashAlgorithm::Sha1,
        64 => GitHashAlgorithm::Sha256,
        _ => {
            return Err(PrimitiveError::Validation {
                details: format!("unsupported object id length: {len}", len = raw_oid.len()),
            });
        }
    };
    ObjectId::from_hex(algorithm, raw_oid).map_err(|error| PrimitiveError::Validation {
        details: format!("invalid object id: {error}"),
    })
}

fn map_io_error(error: io::Error, action: &str, context: &str) -> PrimitiveError {
    PrimitiveError::Storage {
        details: format!("{action} {context}: {error}"),
    }
}

fn map_cli_result_error(error: CliError) -> PrimitiveError {
    PrimitiveError::Storage {
        details: format!("CLI error: {error:?}"),
    }
}

fn normalize_worktree_path(path: &str) -> PrimitiveResult<String> {
    if path.is_empty() {
        return Err(PrimitiveError::Validation {
            details: String::from("path must not be empty"),
        });
    }
    let normalized = normalize_git_path(path).map_err(|error| PrimitiveError::Validation {
        details: format!("invalid worktree path '{path}': {error}"),
    })?;
    if normalized.starts_with('/') {
        return Err(PrimitiveError::Validation {
            details: format!("invalid worktree path '{path}': absolute paths are not allowed"),
        });
    }
    if normalized.is_empty() {
        return Err(PrimitiveError::Validation {
            details: String::from("path must not be empty"),
        });
    }
    Ok(normalized)
}

fn is_zero_oid(id: &ObjectId) -> bool {
    id.as_bytes().iter().all(|byte| *byte == 0)
}

fn commit_from_bytes(algorithm: GitHashAlgorithm, content: &[u8]) -> PrimitiveResult<CommitObject> {
    decode_commit(algorithm, content).map_err(|error| PrimitiveError::Validation {
        details: format!("failed to decode commit: {error}"),
    })
}

struct CountingWriter<'a> {
    inner: &'a mut dyn Write,
    written: usize,
}

impl<'a> CountingWriter<'a> {
    fn new(inner: &'a mut dyn Write) -> Self {
        Self { inner, written: 0 }
    }

    fn bytes_written(&self) -> usize {
        self.written
    }
}

impl<'a> Write for CountingWriter<'a> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.written = self.written.saturating_add(written);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug)]
pub(crate) struct CliWorktreeAdapter {
    repo: GitRepo,
}

impl CliWorktreeAdapter {
    pub(crate) fn new(repo: &GitRepo) -> Self {
        Self { repo: repo.clone() }
    }

    fn absolute_path(&self, path: &str) -> PrimitiveResult<PathBuf> {
        let normalized = normalize_worktree_path(path)?;
        Ok(self.repo.root.join(normalized))
    }
}

impl GitWorktreeEngine for CliWorktreeAdapter {
    fn materialize_file(&self, path: &GitRepoPath, content: &[u8]) -> PrimitiveResult<()> {
        let absolute_path = self.absolute_path(path)?;
        if let Some(parent) = absolute_path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                return Err(map_io_error(error, "create parent directories for", path));
            }
        }
        std::fs::write(&absolute_path, content)
            .map_err(|error| map_io_error(error, "write file", path))?;
        Ok(())
    }

    fn remove_path(&self, path: &GitRepoPath) -> PrimitiveResult<()> {
        let absolute = self.absolute_path(path)?;
        match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                std::fs::remove_file(&absolute)
                    .map_err(|error| map_io_error(error, "remove file", path))?;
            }
            Ok(metadata) if metadata.is_dir() => {
                if let Err(error) = std::fs::remove_dir_all(&absolute) {
                    return Err(map_io_error(error, "remove directory", path));
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(map_io_error(error, "remove", path)),
        }
        Ok(())
    }

    fn touch_path(&self, path: &GitRepoPath) -> PrimitiveResult<()> {
        let absolute_path = self.absolute_path(path)?;
        if let Some(parent) = absolute_path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                return Err(map_io_error(error, "create parent directories for", path));
            }
        }
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&absolute_path)
            .map_err(|error| map_io_error(error, "touch", path))?;
        Ok(())
    }

    fn read_path(&self, path: &GitRepoPath) -> PrimitiveResult<Vec<u8>> {
        let absolute_path = self.absolute_path(path)?;
        std::fs::read(&absolute_path).map_err(|error| map_io_error(error, "read", path))
    }

    fn rename_path(&self, from: &GitRepoPath, to: &GitRepoPath) -> PrimitiveResult<()> {
        let source = self.absolute_path(from)?;
        let target = self.absolute_path(to)?;
        if let Some(parent) = target.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                return Err(map_io_error(error, "create parent directories for", to));
            }
        }
        std::fs::rename(&source, &target).map_err(|error| map_io_error(error, "rename", from))?;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct CliPatchRenderer {
    repo: GitRepo,
    store: OwnedCliObjectStoreAdapter,
}

impl CliPatchRenderer {
    pub(crate) fn new(repo: GitRepo, store: OwnedCliObjectStoreAdapter) -> Self {
        Self { repo, store }
    }

    fn read_object_type(&self, oid: &ObjectId) -> PrimitiveResult<String> {
        let envelope = self
            .store
            .read_envelope(&oid.to_hex(), Some(0))
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read object envelope {}: {error:?}", oid.to_hex()),
            })?;
        Ok(envelope.object_type)
    }

    fn as_tree_oid(&self, object: &str) -> PrimitiveResult<Option<ObjectId>> {
        let object_id = parse_oid(object)?;
        if is_zero_oid(&object_id) {
            return Ok(None);
        }
        let kind = self.read_object_type(&object_id)?;
        match kind.as_str() {
            "tree" => Ok(Some(object_id)),
            "commit" => {
                let content = self
                    .store
                    .read_object_content(&object_id.to_hex())
                    .map_err(|error| PrimitiveError::Storage {
                        details: format!("read object {object}: {error}"),
                    })?;
                let commit = commit_from_bytes(object_id.algorithm(), &content)?;
                Ok(Some(commit.tree))
            }
            _ => Err(PrimitiveError::Validation {
                details: format!("unsupported object type '{kind}' for patch render"),
            }),
        }
    }

    fn as_commit_object(&self, object: &str) -> PrimitiveResult<CommitObject> {
        let object_id = parse_oid(object)?;
        let kind = self.read_object_type(&object_id)?;
        if kind != "commit" {
            return Err(PrimitiveError::Validation {
                details: format!("expected commit object, got {kind}"),
            });
        }
        let content = self
            .store
            .read_object_content(&object_id.to_hex())
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read object {object}: {error}"),
            })?;
        commit_from_bytes(object_id.algorithm(), &content)
    }
}

impl GitPatchRenderer for CliPatchRenderer {
    fn render_diff_patch(
        &self,
        old_object: &String,
        new_object: &String,
        writer: &mut dyn Write,
    ) -> PrimitiveResult<usize> {
        let old_tree = self.as_tree_oid(old_object)?;
        let new_tree = self
            .as_tree_oid(new_object)?
            .ok_or_else(|| PrimitiveError::Validation {
                details: String::from("new object id must not be empty tree"),
            })?;
        let store = self.store.as_object_store();
        let tree_cache = TreeObjectCache::new(store);
        let commit = FormatPatchContext {
            repo: &self.repo,
            store,
            abbrev_len: default_abbrev_len(store).map_err(map_cli_result_error)?,
            total: 1,
        };
        let synthetic_signature = b"Skron Primitive <primitive@example.test> 1 +0000".to_vec();
        let format_entry = FormatPatchEntry {
            id: &new_tree,
            commit: &CommitObject {
                tree: new_tree.clone(),
                parents: Vec::new(),
                author: synthetic_signature.clone(),
                committer: synthetic_signature,
                message: Vec::new(),
            },
            number: 1,
        };
        let mut blob_cache = FormatPatchBlobCache::new(store);
        let mut counted = CountingWriter::new(writer);
        write_format_patch_with_tree_diff_cached(
            &mut counted,
            &commit,
            format_entry,
            &tree_cache,
            old_tree.as_ref(),
            &new_tree,
            &mut blob_cache,
        )
        .map_err(|error| PrimitiveError::Storage {
            details: format!("failed to render patch: {error:?}"),
        })?;
        counted
            .flush()
            .map_err(|error| map_io_error(error, "flush", "patch output"))?;
        Ok(counted.bytes_written())
    }

    fn render_format_patch(
        &self,
        commits: &[String],
        writer: &mut dyn Write,
        _with_binary: bool,
    ) -> PrimitiveResult<usize> {
        if commits.is_empty() {
            return Ok(0);
        }

        let store = self.store.as_object_store();
        let tree_cache = TreeObjectCache::new(store);
        let context = FormatPatchContext {
            repo: &self.repo,
            store,
            abbrev_len: default_abbrev_len(store).map_err(map_cli_result_error)?,
            total: commits.len(),
        };
        let mut blob_cache = FormatPatchBlobCache::new(store);
        let mut total_written = 0usize;
        let mut counted = CountingWriter::new(writer);

        for (number, commit_id) in commits.iter().enumerate() {
            let commit = self.as_commit_object(commit_id)?;
            let current_commit_id = parse_oid(commit_id)?;
            let old_tree = format_patch_old_tree_for_primitive(self, &commit)?;
            let before = counted.bytes_written();
            write_format_patch_with_tree_diff_cached(
                &mut counted,
                &context,
                FormatPatchEntry {
                    id: &current_commit_id,
                    commit: &commit,
                    number: number + 1,
                },
                &tree_cache,
                old_tree.as_ref(),
                &commit.tree,
                &mut blob_cache,
            )
            .map_err(|error| PrimitiveError::Storage {
                details: format!("failed to render patch entry: {error:?}"),
            })?;
            total_written =
                total_written.saturating_add(counted.bytes_written().saturating_sub(before));
        }

        counted
            .flush()
            .map_err(|error| map_io_error(error, "flush", "format-patch output"))?;
        Ok(total_written)
    }
}

fn format_patch_old_tree_for_primitive(
    renderer: &CliPatchRenderer,
    commit: &CommitObject,
) -> PrimitiveResult<Option<ObjectId>> {
    let Some(parent) = commit.parents.first() else {
        return Ok(None);
    };
    let parent_hex = parent.to_hex();
    renderer.as_tree_oid(&parent_hex)
}

#[derive(Debug)]
pub(crate) struct CliRewriteEngine {
    store: OwnedCliObjectStoreAdapter,
}

impl CliRewriteEngine {
    pub(crate) fn new(store: OwnedCliObjectStoreAdapter) -> Self {
        Self { store }
    }

    fn read_commit_object(&self, commit: &str) -> PrimitiveResult<CommitObject> {
        let commit_id = parse_oid(commit)?;
        let commit_type = self
            .store
            .read_envelope(&commit_id.to_hex(), None)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read envelope {commit}: {error}"),
            })?;
        if commit_type.object_type != "commit" {
            return Err(PrimitiveError::Validation {
                details: String::from("expected commit object"),
            });
        }
        let content = self
            .store
            .read_object_content(&commit_id.to_hex())
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read object {commit}: {error}"),
            })?;
        commit_from_bytes(commit_id.algorithm(), &content)
    }

    fn rewrite_commit(
        &self,
        source: &str,
        new_tree: &str,
        message: Vec<u8>,
    ) -> PrimitiveResult<String> {
        let commit = self.read_commit_object(source)?;
        let tree = parse_oid(new_tree)?;
        let tree_type = self
            .store
            .read_envelope(&tree.to_hex(), None)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read envelope {}: {error}", tree.to_hex()),
            })?;
        if tree_type.object_type != "tree" {
            return Err(PrimitiveError::Validation {
                details: format!("expected tree object, got {}", tree_type.object_type),
            });
        }

        let mut builder = CommitBuilder::new(
            tree,
            signature_from_commit_bytes(&commit.author).map_err(map_cli_result_error)?,
            signature_from_commit_bytes(&commit.committer).map_err(map_cli_result_error)?,
        );
        for parent in &commit.parents {
            builder = builder.parent(parent.clone());
        }

        let rewritten =
            builder
                .message(message)?
                .encode()
                .map_err(|error| PrimitiveError::Storage {
                    details: format!("failed to encode commit: {error}"),
                })?;
        let id = self
            .store
            .write_object_content(
                &GitObjectEnvelope {
                    id: "0".repeat(commit.tree.to_hex().len()),
                    size: rewritten.len(),
                    object_type: String::from("commit"),
                    metadata: Default::default(),
                },
                &rewritten,
            )
            .map_err(|error| PrimitiveError::Storage {
                details: format!("failed to write rewritten commit: {error}"),
            })?;
        Ok(id)
    }
}

impl GitRewriteEngine for CliRewriteEngine {
    fn rewrite_commit_tree(
        &self,
        commit_oid: &String,
        new_tree: &String,
    ) -> PrimitiveResult<String> {
        self.rewrite_commit(
            commit_oid,
            new_tree,
            self.read_commit_object(commit_oid)?.message,
        )
    }

    fn create_replacement_commit(&self, source: &String, message: &str) -> PrimitiveResult<String> {
        let source_commit = self.read_commit_object(source)?;
        self.rewrite_commit(
            source,
            &source_commit.tree.to_hex(),
            message.as_bytes().to_vec(),
        )
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CliObjectStoreAdapter<'a, Store, Sink>
where
    Store: CoreGitObjectStore + Send + Sync,
    Sink: GitObjectSink + Send + Sync,
{
    object_store: &'a Store,
    object_sink: &'a Sink,
}

impl<'a, Store, Sink> CliObjectStoreAdapter<'a, Store, Sink>
where
    Store: CoreGitObjectStore + Send + Sync,
    Sink: GitObjectSink + Send + Sync,
{
    pub(crate) fn new(store: &'a Store, sink: &'a Sink) -> Self {
        Self {
            object_store: store,
            object_sink: sink,
        }
    }

    fn object_type(raw_type: &str) -> PrimitiveResult<GitObjectKind> {
        GitObjectKind::parse(raw_type.as_bytes()).ok_or_else(|| PrimitiveError::Validation {
            details: format!("unsupported git object type: {raw_type}"),
        })
    }
}

impl<'a, Store, Sink> PrimitiveGitObjectStore for CliObjectStoreAdapter<'a, Store, Sink>
where
    Store: CoreGitObjectStore + Send + Sync,
    Sink: GitObjectSink + Send + Sync,
{
    fn read_envelope(
        &self,
        oid: &String,
        _size_hint: Option<usize>,
    ) -> PrimitiveResult<GitObjectEnvelope> {
        let id = parse_oid(oid)?;
        let object =
            self.object_store
                .read_object(&id)
                .map_err(|error| PrimitiveError::Storage {
                    details: format!("read object {oid}: {error}"),
                })?;

        let algorithm = match object.id.algorithm() {
            GitHashAlgorithm::Sha1 => "sha1",
            GitHashAlgorithm::Sha256 => "sha256",
        };
        let mut metadata = BTreeMap::new();
        metadata.insert("algorithm".to_owned(), algorithm.to_owned());

        Ok(GitObjectEnvelope {
            id: object.id.to_hex(),
            size: object.content.len(),
            object_type: object.kind.as_str().to_owned(),
            metadata,
        })
    }

    fn read_object_content(&self, oid: &String) -> PrimitiveResult<Vec<u8>> {
        let id = parse_oid(oid)?;
        self.object_store
            .read_object(&id)
            .map(|object| object.content)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read object {oid}: {error}"),
            })
    }

    fn write_object_content(
        &self,
        envelope: &GitObjectEnvelope,
        content: &[u8],
    ) -> PrimitiveResult<String> {
        let object_kind = Self::object_type(&envelope.object_type)?;
        let object_id = self
            .object_sink
            .write_object(object_kind, content)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("write object failed: {error}"),
            })?;
        Ok(object_id.to_hex())
    }
}

#[derive(Debug)]
pub(crate) struct CliTransportAdapter;

impl CliTransportAdapter {
    fn local_source(remote: &GitEndpoint) -> PrimitiveResult<GitRepo> {
        let source_path = local_repository_path_from_location(remote)
            .map_err(|error| PrimitiveError::Validation {
                details: format!("{remote}: {error:?}"),
            })?
            .ok_or_else(|| PrimitiveError::Validation {
                details: format!("transport discovery is not supported for remote: {remote}"),
            })?;
        let source = local_clone_source(&source_path).map_err(|error| PrimitiveError::Storage {
            details: format!("remote {remote} is not a local git repository: {error:?}"),
        })?;
        Ok(GitRepo {
            root: source_path,
            git_dir: source.git_dir.clone(),
            objects_dir: source.git_dir.join("objects"),
            index_path: source.git_dir.join("index"),
        })
    }

    fn map_remote_not_found(error: io::Error, remote: &GitEndpoint) -> PrimitiveError {
        if error.kind() == io::ErrorKind::NotFound {
            PrimitiveError::Validation {
                details: format!("remote object was not found in {remote}"),
            }
        } else {
            PrimitiveError::Storage {
                details: format!("read remote object from {remote}: {error}"),
            }
        }
    }

    fn map_cli_error(error: CliError) -> PrimitiveError {
        match error {
            CliError::Exit(code) => PrimitiveError::ExitStatus { code },
            CliError::Stderr { code, text } => PrimitiveError::Fatal {
                code,
                message: format!("stderr: {text}"),
            },
            CliError::Io(error) => PrimitiveError::Io(error),
            CliError::Message(message) => PrimitiveError::Validation { details: message },
            CliError::Fatal { code, message } => PrimitiveError::Validation {
                details: format!("fatal (code {code}): {message}"),
            },
        }
    }
}

impl GitTransport for CliTransportAdapter {
    fn discover_refs(&self, remote: &GitEndpoint) -> PrimitiveResult<RefDiscovery> {
        let source = Self::local_source(remote)?;
        let refs = RefStore::new(&source.git_dir, GitHashAlgorithm::Sha1);
        let store = LooseObjectStore::new(&source.objects_dir, GitHashAlgorithm::Sha1);

        let mut discovered = BTreeMap::new();
        let symref = match refs.read_head() {
            Ok(RefTarget::Symbolic(target)) => Some((String::from("HEAD"), target)),
            Ok(RefTarget::Direct(_)) | Err(_) => None,
        };

        if let Ok(id) = refs.resolve("HEAD") {
            discovered.insert(String::from("HEAD"), id.to_hex());
        }

        refs.for_each_resolved_ref("refs/heads/", |name, id| {
            discovered.insert(name.to_owned(), id.to_hex());
            Ok::<(), PrimitiveError>(())
        })
        .map_err(|error| PrimitiveError::Storage {
            details: format!("read local ref for {remote}: {error}"),
        })?;

        refs.for_each_resolved_ref("refs/tags/", |name, id| {
            let tag_object_id = id.to_hex();
            if !discovered.contains_key(&format!("{name}^{{}}")) {
                discovered.insert(format!("{name}^{{}}"), tag_object_id.clone());
            }
            discovered.insert(name.to_owned(), tag_object_id);
            if let Some(peeled) =
                peel_local_tag(&store, id).map_err(|error| PrimitiveError::Storage {
                    details: format!("peek tag for {name}: {error}"),
                })?
            {
                discovered.insert(format!("{name}^{{}}"), peeled.to_hex());
            }
            Ok::<(), PrimitiveError>(())
        })
        .map_err(|error| PrimitiveError::Storage {
            details: format!("read local tag refs for {remote}: {error}"),
        })?;

        Ok(RefDiscovery {
            refs: discovered,
            symref,
        })
    }

    fn has_remote_object(&self, remote: &GitEndpoint, oid: &String) -> PrimitiveResult<bool> {
        let source = Self::local_source(remote)?;
        let store = LooseObjectStore::new(&source.objects_dir, GitHashAlgorithm::Sha1);
        let object_id = parse_oid(oid)?;
        match store.read_object(&object_id) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Self::map_remote_not_found(error, remote)),
        }
    }

    fn upload_pack_request(
        &self,
        request: &TransferRequest,
        stdin: &mut dyn Read,
    ) -> PrimitiveResult<Vec<u8>> {
        match request.service {
            GitTransportService::UploadPack => {}
            _ => {
                return Err(PrimitiveError::Validation {
                    details: format!(
                        "invalid transfer service for upload-pack request: {:?}",
                        request.service
                    ),
                });
            }
        }
        let repo = Self::local_source(&request.remote)?;
        transport_commands::process_upload_pack_request_from_reader(&repo, stdin, false)
            .map_err(Self::map_cli_error)
    }

    fn receive_pack_request(
        &self,
        request: &TransferRequest,
        stdin: &mut dyn Read,
    ) -> PrimitiveResult<Vec<u8>> {
        match request.service {
            GitTransportService::ReceivePack => {}
            _ => {
                return Err(PrimitiveError::Validation {
                    details: format!(
                        "invalid transfer service for receive-pack request: {:?}",
                        request.service
                    ),
                });
            }
        }
        let repo = Self::local_source(&request.remote)?;
        transport_commands::process_receive_pack_request_from_reader(&repo, stdin)
            .map_err(Self::map_cli_error)
    }

    fn read_object_stream(
        &self,
        remote: &GitEndpoint,
        oid: &String,
        writer: &mut dyn Write,
    ) -> PrimitiveResult<usize> {
        let source = Self::local_source(remote)?;
        let store = LooseObjectStore::new(&source.objects_dir, GitHashAlgorithm::Sha1);
        let object_id = parse_oid(oid)?;
        let object = store
            .read_object(&object_id)
            .map_err(|error| Self::map_remote_not_found(error, remote))?;
        writer
            .write_all(&object.content)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("write remote object {oid} stream for {remote}: {error}"),
            })?;
        Ok(object.content.len())
    }
}

fn peel_local_tag(store: &LooseObjectStore, id: &ObjectId) -> PrimitiveResult<Option<ObjectId>> {
    let mut current = id.clone();
    let mut object = store
        .read_object(&current)
        .map_err(|error| PrimitiveError::Storage {
            details: format!("read local tag object {}: {error}", current.to_hex()),
        })?;
    if object.kind != GitObjectKind::Tag {
        return Ok(None);
    }

    for _ in 0..8 {
        let tag = crate::runtime::decode_tag(GitHashAlgorithm::Sha1, &object.content).map_err(
            |error| PrimitiveError::Validation {
                details: format!("decode local tag object {}: {error}", current.to_hex()),
            },
        )?;
        current = tag.target;
        object = store
            .read_object(&current)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read peeled local tag target {}: {error}", current.to_hex()),
            })?;
        if object.kind != GitObjectKind::Tag {
            return Ok(Some(current));
        }
    }
    Err(PrimitiveError::Validation {
        details: String::from("tag nesting is too deep"),
    })
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;
    use tempfile::TempDir;

    use skron_primitives::git_runtime::GitTransportService;

    fn write_pkt_line(out: &mut Vec<u8>, payload: &str) {
        out.extend_from_slice(format!("{:04x}", payload.len() + 4).as_bytes());
        out.extend_from_slice(payload.as_bytes());
    }

    #[test]
    fn transport_adapter_discovers_local_refs() {
        let temp = TempDir::new().expect("temp");
        let repo_root = temp.path().join("remote");
        let repo = GitRepo {
            root: repo_root.clone(),
            git_dir: repo_root.join(".git"),
            objects_dir: repo_root.join(".git/objects"),
            index_path: repo_root.join(".git/index"),
        };
        fs::create_dir_all(&repo.git_dir).expect("create git dir");
        fs::create_dir_all(&repo.objects_dir).expect("create objects dir");
        let payload = b"transport test object";
        let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
        let payload_id = store
            .write_object(GitObjectKind::Blob, payload)
            .expect("write object")
            .to_hex();
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let head = ObjectId::new(GitHashAlgorithm::Sha1, &[1_u8; 20]);
        refs.write_ref("refs/heads/main", &head)
            .expect("write main");
        refs.write_symbolic_ref("HEAD", "refs/heads/main")
            .expect("head symbolic");

        let adapter = CliTransportAdapter;
        let remote = repo.root.to_string_lossy().to_string();
        let discovery = adapter.discover_refs(&remote).expect("discover");
        assert_eq!(
            discovery.symref,
            Some(("HEAD".to_owned(), "refs/heads/main".to_owned()))
        );
        assert!(discovery.refs.contains_key("refs/heads/main"));
        assert_eq!(discovery.refs["HEAD"], head.to_hex());

        assert!(
            adapter
                .has_remote_object(&remote, &payload_id)
                .expect("object exists")
        );
        assert!(
            !adapter
                .has_remote_object(&remote, &"0".repeat(40))
                .expect("missing object")
        );

        let mut out = Vec::new();
        let written = adapter
            .read_object_stream(&remote, &payload_id, &mut out)
            .expect("read object stream");
        assert_eq!(written, payload.len());
        assert_eq!(out, payload);
    }

    #[test]
    fn upload_pack_request_from_local_remote_returns_pack_response() {
        let temp = TempDir::new().expect("temp");
        let repo_root = temp.path().join("remote");
        fs::create_dir_all(repo_root.join(".git/objects")).expect("create repo");
        let repo = GitRepo {
            root: repo_root.clone(),
            git_dir: repo_root.join(".git"),
            objects_dir: repo_root.join(".git/objects"),
            index_path: repo_root.join(".git/index"),
        };
        let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
        let tree_id = store
            .write_object(
                GitObjectKind::Tree,
                &skron_git_core::encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let signature =
            skron_git_core::Signature::new("A", "a@example.test", 1, "+0000").expect("signature");
        let object_id =
            skron_git_core::CommitBuilder::new(tree_id.clone(), signature.clone(), signature)
                .message("transport test commit\n")
                .expect("commit message")
                .encode()
                .expect("encode commit")
                .as_slice()
                .to_vec();
        let object_id = store
            .write_object(GitObjectKind::Commit, &object_id)
            .expect("write object")
            .to_hex();

        let mut request = Vec::new();
        write_pkt_line(&mut request, &format!("want {object_id}\n"));
        write_pkt_line(&mut request, "done\n");
        request.extend_from_slice(b"0000");

        let mut input = Cursor::new(request);
        let output = CliTransportAdapter
            .upload_pack_request(
                &TransferRequest {
                    remote: repo_root.to_string_lossy().to_string(),
                    service: GitTransportService::UploadPack,
                    refspecs: vec![],
                    atomic: false,
                    thin_pack: false,
                    depth: None,
                    filter: None,
                    lease: None,
                },
                &mut input,
            )
            .expect("upload-pack request");

        assert!(!output.is_empty());
        assert!(output.len() > 20);
    }

    #[test]
    fn receive_pack_request_from_local_remote_applies_refs_and_reports_status() {
        let temp = TempDir::new().expect("temp");
        let repo_root = temp.path().join("remote");
        fs::create_dir_all(repo_root.join(".git/objects")).expect("create repo");
        let git_dir = repo_root.join(".git");
        let refs = OwnedCliRefsStoreAdapter::from_path(&git_dir, GitHashAlgorithm::Sha1);
        let objects =
            OwnedCliObjectStoreAdapter::from_path(&git_dir.join("objects"), GitHashAlgorithm::Sha1);

        let old_oid = objects
            .write_object_content(
                &GitObjectEnvelope {
                    id: "0".repeat(40),
                    size: 0,
                    object_type: "blob".to_string(),
                    metadata: Default::default(),
                },
                b"old",
            )
            .expect("write object");
        refs.write_ref(&"refs/heads/main".to_owned(), &old_oid)
            .expect("write ref");

        let mut request = Vec::new();
        write_pkt_line(
            &mut request,
            &format!(
                "{old} {zero} refs/heads/main\0report-status ofs-delta\n",
                old = old_oid,
                zero = "0".repeat(40)
            ),
        );
        request.extend_from_slice(b"0000");

        let mut input = Cursor::new(request);
        let output = CliTransportAdapter
            .receive_pack_request(
                &TransferRequest {
                    remote: repo_root.to_string_lossy().to_string(),
                    service: GitTransportService::ReceivePack,
                    refspecs: vec![],
                    atomic: false,
                    thin_pack: false,
                    depth: None,
                    filter: None,
                    lease: None,
                },
                &mut input,
            )
            .expect("receive-pack request");

        assert!(!output.is_empty());
        assert!(String::from_utf8_lossy(&output).contains("ok refs/heads/main"));
        assert!(String::from_utf8_lossy(&output).contains("unpack ok"));
    }

    #[test]
    fn upload_pack_request_non_local_remote_is_validation_error() {
        let request = TransferRequest {
            remote: "https://example.test/repo.git".to_string(),
            service: GitTransportService::UploadPack,
            refspecs: vec![],
            atomic: false,
            thin_pack: false,
            depth: None,
            filter: None,
            lease: None,
        };
        let mut input = Cursor::new(Vec::new());
        let error = CliTransportAdapter
            .upload_pack_request(&request, &mut input)
            .expect_err("non-local remote");
        assert!(matches!(error, PrimitiveError::Validation { .. }));
    }

    #[test]
    fn receive_pack_request_rejects_mismatched_service() {
        let temp = TempDir::new().expect("temp");
        let repo_root = temp.path().join("remote");
        fs::create_dir_all(repo_root.join(".git/objects")).expect("create repo");

        let mut input = Cursor::new(Vec::new());
        let error = CliTransportAdapter
            .receive_pack_request(
                &TransferRequest {
                    remote: repo_root.to_string_lossy().to_string(),
                    service: GitTransportService::UploadPack,
                    refspecs: vec![],
                    atomic: false,
                    thin_pack: false,
                    depth: None,
                    filter: None,
                    lease: None,
                },
                &mut input,
            )
            .expect_err("wrong service");
        assert!(matches!(error, PrimitiveError::Validation { .. }));
    }
}

/// Runtime adapter that lifts [`RefStore`] into the shared [`GitRefsStore`] contract.
#[derive(Clone, Copy)]
pub(crate) struct CliRefsStoreAdapter<'a> {
    refs: &'a RefStore,
}

impl<'a> CliRefsStoreAdapter<'a> {
    pub(crate) fn new(refs: &'a RefStore) -> Self {
        Self { refs }
    }

    fn read_oid_from_target(&self, target: RefTarget, name: &str) -> PrimitiveResult<String> {
        match target {
            RefTarget::Direct(id) => Ok(id.to_hex()),
            RefTarget::Symbolic(_) => self
                .refs
                .resolve(name)
                .map(|resolved| resolved.to_hex())
                .map_err(|error| PrimitiveError::Storage {
                    details: format!("failed to resolve symbolic ref {name}: {error}"),
                }),
        }
    }

    fn validate_reference_target(
        &self,
        ref_name: &GitRefName,
        old_oid: &Option<String>,
        new_oid: &Option<String>,
        action: &str,
    ) -> PrimitiveResult<()> {
        let old = match old_oid {
            Some(value) => Some(parse_oid(value)?),
            None => None,
        };

        let current = self.read_ref(ref_name)?;
        if let Some(old_value) = old
            && current.as_ref() != Some(&old_value.to_hex())
        {
            return Err(PrimitiveError::Validation {
                details: format!(
                    "{action} ref {ref_name}: expected old oid {:?}, current {:?}",
                    old_value.to_hex(),
                    current
                ),
            });
        }

        if matches!(action, "create") && current.is_some() {
            return Err(PrimitiveError::Validation {
                details: format!("create ref {ref_name}: ref already exists"),
            });
        }

        if matches!(action, "delete") && current.is_none() {
            return Err(PrimitiveError::Validation {
                details: format!("delete ref {ref_name}: ref does not exist"),
            });
        }

        if matches!(action, "update") && new_oid.is_none() {
            return Err(PrimitiveError::Validation {
                details: format!("update ref {ref_name}: missing target oid"),
            });
        }

        Ok(())
    }
}

impl<'a> GitRefsStore for CliRefsStoreAdapter<'a> {
    fn read_ref(&self, name: &GitRefName) -> PrimitiveResult<Option<String>> {
        let target = if name == "HEAD" {
            self.refs.read_head()
        } else {
            self.refs.read_ref(name)
        };
        match target {
            Ok(ref_target) => self.read_oid_from_target(ref_target, name).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(PrimitiveError::Storage {
                details: format!("read ref {name}: {error}"),
            }),
        }
    }

    fn write_ref(&self, name: &GitRefName, value: &String) -> PrimitiveResult<()> {
        let object_id = parse_oid(value)?;
        self.refs
            .write_ref(name, &object_id)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("write ref {name}: {error}"),
            })
    }

    fn delete_ref(&self, name: &GitRefName) -> PrimitiveResult<()> {
        self.refs
            .delete_ref(name)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("delete ref {name}: {error}"),
            })
    }

    fn read_symbolic_ref(&self, name: &GitRefName) -> PrimitiveResult<Option<String>> {
        let target = if name == "HEAD" {
            self.refs.read_head()
        } else {
            self.refs.read_ref(name)
        };
        match target {
            Ok(RefTarget::Symbolic(target)) => Ok(Some(target)),
            Ok(RefTarget::Direct(_)) => Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(PrimitiveError::Storage {
                details: format!("read symbolic ref {name}: {error}"),
            }),
        }
    }

    fn write_symbolic_ref(&self, name: &GitRefName, target: &String) -> PrimitiveResult<()> {
        self.refs
            .write_symbolic_ref(name, target)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("write symbolic ref {name}: {error}"),
            })
    }

    fn list_refs(&self, pattern: Option<&str>) -> PrimitiveResult<Vec<(GitRefName, String)>> {
        let prefix = pattern.unwrap_or("");
        let names = self
            .refs
            .list_refs(prefix)
            .map_err(|error| PrimitiveError::Storage {
                details: format!("list refs {prefix}: {error}"),
            })?;

        let mut refs = Vec::new();
        for name in names {
            if let Some(oid) = self.read_ref(&name)? {
                refs.push((name, oid));
            }
        }
        Ok(refs)
    }

    fn visit_refs(
        &self,
        pattern: Option<&str>,
        visitor: &mut dyn FnMut(&GitRefName, &String) -> PrimitiveResult<()>,
    ) -> PrimitiveResult<()> {
        let prefix = pattern.unwrap_or("");
        self.refs
            .for_each_ref_name(prefix, |name| {
                let ref_name = name.to_owned();
                if let Some(oid) = self.read_ref(&ref_name)? {
                    visitor(&ref_name, &oid)?;
                }
                Ok::<(), PrimitiveError>(())
            })
            .map_err(|error| PrimitiveError::Storage {
                details: format!("visit refs {prefix}: {error}"),
            })?;
        Ok(())
    }

    fn apply_ref_updates(&self, updates: &[RefUpdate]) -> PrimitiveResult<()> {
        for update in updates {
            let new = update.new_oid.clone();
            let old = update.old_oid.clone();

            match update.action {
                RefUpdateAction::Delete => {
                    self.validate_reference_target(&update.name, &old, &new, "delete")?;
                    self.delete_ref(&update.name)?;
                }
                RefUpdateAction::Create => {
                    self.validate_reference_target(&update.name, &old, &new, "create")?;
                    let object_id = new
                        .as_ref()
                        .ok_or_else(|| PrimitiveError::Validation {
                            details: format!("create ref {}: missing target oid", update.name),
                        })
                        .and_then(|value| parse_oid(value))?;
                    self.refs
                        .write_ref(&update.name, &object_id)
                        .map_err(|error| PrimitiveError::Storage {
                            details: format!("create ref {}: {error}", update.name),
                        })?;
                }
                RefUpdateAction::Update => {
                    self.validate_reference_target(&update.name, &old, &new, "update")?;
                    let object_id = new
                        .as_ref()
                        .ok_or_else(|| PrimitiveError::Validation {
                            details: format!("update ref {}: missing target oid", update.name),
                        })
                        .and_then(|value| parse_oid(value))?;
                    self.refs
                        .write_ref(&update.name, &object_id)
                        .map_err(|error| PrimitiveError::Storage {
                            details: format!("update ref {}: {error}", update.name),
                        })?;
                }
                RefUpdateAction::NoChange => {
                    self.validate_reference_target(&update.name, &old, &new, "noop")?;
                }
            }
        }

        Ok(())
    }

    fn begin_transaction(&self) -> PrimitiveResult<()> {
        Ok(())
    }

    fn commit_transaction(&self) -> PrimitiveResult<()> {
        Ok(())
    }

    fn rollback_transaction(&self) -> PrimitiveResult<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OwnedCliObjectStoreAdapter {
    object_store: LooseObjectStore,
    object_sink: LooseObjectStore,
}

impl OwnedCliObjectStoreAdapter {
    pub(crate) fn new(store: LooseObjectStore, sink: LooseObjectStore) -> Self {
        Self {
            object_store: store,
            object_sink: sink,
        }
    }

    pub(crate) fn from_path(objects_dir: impl AsRef<Path>, algorithm: GitHashAlgorithm) -> Self {
        Self::new(
            LooseObjectStore::new(objects_dir.as_ref(), algorithm),
            LooseObjectStore::new(objects_dir.as_ref(), algorithm),
        )
    }

    pub(crate) fn as_object_store(&self) -> &LooseObjectStore {
        &self.object_store
    }
}

impl std::ops::Deref for OwnedCliObjectStoreAdapter {
    type Target = LooseObjectStore;

    fn deref(&self) -> &Self::Target {
        &self.object_store
    }
}

impl CoreGitObjectStore for OwnedCliObjectStoreAdapter {
    fn read_object(&self, id: &ObjectId) -> std::io::Result<skron_git_core::LooseObject> {
        self.object_store.read_object(id)
    }
}

impl GitObjectSink for OwnedCliObjectStoreAdapter {
    fn write_object(&self, kind: GitObjectKind, content: &[u8]) -> std::io::Result<ObjectId> {
        self.object_sink.write_object(kind, content)
    }
}

impl PrimitiveGitObjectStore for OwnedCliObjectStoreAdapter {
    fn read_envelope(
        &self,
        oid: &String,
        _size_hint: Option<usize>,
    ) -> PrimitiveResult<GitObjectEnvelope> {
        CliObjectStoreAdapter::new(&self.object_store, &self.object_sink)
            .read_envelope(oid, _size_hint)
    }

    fn read_object_content(&self, oid: &String) -> PrimitiveResult<Vec<u8>> {
        CliObjectStoreAdapter::new(&self.object_store, &self.object_sink).read_object_content(oid)
    }

    fn write_object_content(
        &self,
        envelope: &GitObjectEnvelope,
        content: &[u8],
    ) -> PrimitiveResult<String> {
        CliObjectStoreAdapter::new(&self.object_store, &self.object_sink)
            .write_object_content(envelope, content)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OwnedCliRefsStoreAdapter {
    refs: RefStore,
}

impl OwnedCliRefsStoreAdapter {
    pub(crate) fn new(refs: RefStore) -> Self {
        Self { refs }
    }

    pub(crate) fn from_path(git_dir: impl AsRef<Path>, algorithm: GitHashAlgorithm) -> Self {
        Self::new(RefStore::new(git_dir.as_ref(), algorithm))
    }

    pub(crate) fn as_ref_store(&self) -> &RefStore {
        &self.refs
    }

    pub(crate) fn objects_dir(&self) -> PathBuf {
        self.refs.git_dir().join("objects")
    }

    pub(crate) fn ref_names(&self, prefix: &str) -> super::Result<Vec<String>> {
        let mut ref_names = Vec::new();
        self.refs
            .for_each_ref_name(prefix, |ref_name| {
                ref_names.push(ref_name.to_owned());
                Ok::<(), io::Error>(())
            })
            .map_err(|error| CliError::Io(error))?;
        Ok(ref_names)
    }

    pub(crate) fn read_ref_oid(&self, name: &str) -> super::Result<Option<ObjectId>> {
        match self.refs.read_ref(name) {
            Ok(RefTarget::Direct(id)) => Ok(Some(id)),
            Ok(RefTarget::Symbolic(target)) => Ok(self.refs.resolve(&target).ok()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CliError::Io(error)),
        }
    }

    pub(crate) fn resolve_ref(&self, name: &str) -> super::Result<Option<ObjectId>> {
        match self.refs.resolve(name) {
            Ok(id) => Ok(Some(id)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CliError::Io(error)),
        }
    }

    pub(crate) fn for_each_server_info_ref<F>(&self, visitor: F) -> super::Result<()>
    where
        F: FnMut(&ObjectId, &str) -> super::Result<()>,
    {
        self.refs.for_each_server_info_ref(visitor)
    }

    pub(crate) fn validate_push_delete(&self, destination: &str) -> super::Result<()> {
        if !self.ref_exists(destination)? {
            return Err(CliError::Stderr {
                code: 1,
                text: format!(
                    "error: unable to delete '{}': remote ref does not exist\n\
                     error: failed to push some refs\n",
                    destination
                        .strip_prefix("refs/heads/")
                        .unwrap_or(destination)
                ),
            });
        }
        if let Ok(RefTarget::Symbolic(target)) = self.refs.read_head()
            && target == destination
            && !self.receive_allows_deleting_current_branch()?
        {
            return Err(CliError::Stderr {
                code: 1,
                text: format!(
                    "remote: error: refusing to delete the current branch: {destination}\n\
                     error: failed to push some refs\n"
                ),
            });
        }
        Ok(())
    }
}

impl std::ops::Deref for OwnedCliRefsStoreAdapter {
    type Target = RefStore;

    fn deref(&self) -> &Self::Target {
        &self.refs
    }
}

impl OwnedCliRefsStoreAdapter {
    pub(crate) fn head_symbolic_ref(&self) -> Option<String> {
        match self.refs.read_head() {
            Ok(RefTarget::Symbolic(target)) => Some(target),
            _ => None,
        }
    }

    pub(crate) fn server_info_refs(&self) -> PrimitiveResult<Vec<(GitRefName, String)>> {
        let mut out = Vec::new();
        self.refs
            .for_each_server_info_ref(|id, name| {
                out.push((name.to_owned(), id.to_hex()));
                Ok::<_, io::Error>(())
            })
            .map_err(|error| PrimitiveError::Storage {
                details: format!("read server-info refs: {error}"),
            })?;
        Ok(out)
    }

    pub(crate) fn ref_exists(&self, name: &str) -> super::Result<bool> {
        match self.refs.read_ref(name) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(CliError::Io(error)),
        }
    }

    fn receive_allows_deleting_current_branch(&self) -> super::Result<bool> {
        let entries = read_config_file(&self.refs.git_dir().join("config"))?;
        let value = entries
            .into_iter()
            .rev()
            .find(|entry| {
                entry.section == "receive"
                    && entry.subsection.is_empty()
                    && entry.key == "denyDeleteCurrent"
            })
            .map(|entry| entry.value.to_ascii_lowercase());

        Ok(matches!(value.as_deref(), Some("warn" | "ignore")))
    }
}

impl GitRefsStore for OwnedCliRefsStoreAdapter {
    fn read_ref(&self, name: &GitRefName) -> PrimitiveResult<Option<String>> {
        CliRefsStoreAdapter::new(&self.refs).read_ref(name)
    }

    fn write_ref(&self, name: &GitRefName, value: &String) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).write_ref(name, value)
    }

    fn delete_ref(&self, name: &GitRefName) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).delete_ref(name)
    }

    fn read_symbolic_ref(&self, name: &GitRefName) -> PrimitiveResult<Option<String>> {
        CliRefsStoreAdapter::new(&self.refs).read_symbolic_ref(name)
    }

    fn write_symbolic_ref(&self, name: &GitRefName, target: &String) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).write_symbolic_ref(name, target)
    }

    fn list_refs(&self, pattern: Option<&str>) -> PrimitiveResult<Vec<(GitRefName, String)>> {
        CliRefsStoreAdapter::new(&self.refs).list_refs(pattern)
    }

    fn visit_refs(
        &self,
        pattern: Option<&str>,
        visitor: &mut dyn FnMut(&GitRefName, &String) -> PrimitiveResult<()>,
    ) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).visit_refs(pattern, visitor)
    }

    fn pack_refs(&self, all: bool, prune: bool) -> PrimitiveResult<()> {
        self.refs
            .pack_refs(PackRefsOptions { all, prune })
            .map_err(|error| PrimitiveError::Storage {
                details: format!("pack refs: {error}"),
            })?;
        Ok(())
    }

    fn apply_ref_updates(&self, updates: &[RefUpdate]) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).apply_ref_updates(updates)
    }

    fn begin_transaction(&self) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).begin_transaction()
    }

    fn commit_transaction(&self) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).commit_transaction()
    }

    fn rollback_transaction(&self) -> PrimitiveResult<()> {
        CliRefsStoreAdapter::new(&self.refs).rollback_transaction()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn cli_refs_store_adapter_reads_and_writes_refs() {
        let dir = TempDir::new().expect("temp dir");
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(git_dir.join("objects")).expect("objects dir");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);

        let adapter = CliRefsStoreAdapter::new(&refs);

        let master = ObjectId::new(GitHashAlgorithm::Sha1, &[1_u8; 20]).to_hex();
        let main = ObjectId::new(GitHashAlgorithm::Sha1, &[2_u8; 20]).to_hex();

        adapter
            .write_ref(&"refs/heads/main".to_owned(), &main)
            .expect("write main ref");
        refs.write_symbolic_ref("HEAD", "refs/heads/main")
            .expect("write symbolic head");

        assert_eq!(
            adapter
                .read_ref(&"refs/heads/main".to_owned())
                .expect("read main"),
            Some(main.clone())
        );
        assert_eq!(
            adapter
                .read_ref(&"HEAD".to_owned())
                .expect("read head via symbolic"),
            Some(main.clone())
        );

        let updates = vec![RefUpdate {
            name: "refs/heads/main".to_owned(),
            old_oid: Some(main.clone()),
            new_oid: Some(master.clone()),
            reason: None,
            action: RefUpdateAction::Update,
        }];
        adapter
            .apply_ref_updates(&updates)
            .expect("apply update ref");
        assert_eq!(
            adapter
                .read_ref(&"refs/heads/main".to_owned())
                .expect("read main after update"),
            Some(master.clone())
        );

        assert_eq!(
            adapter.list_refs(Some("refs/heads")).expect("list refs"),
            vec![("refs/heads/main".to_owned(), master.clone())]
        );
        let mut visited = Vec::new();
        adapter
            .visit_refs(Some("refs/heads"), &mut |name, oid| {
                visited.push((name.clone(), oid.clone()));
                Ok(())
            })
            .expect("visit refs");
        assert_eq!(
            visited,
            vec![("refs/heads/main".to_owned(), master.clone())]
        );

        let del = vec![RefUpdate {
            name: "refs/heads/main".to_owned(),
            old_oid: Some(master.clone()),
            new_oid: None,
            reason: None,
            action: RefUpdateAction::Delete,
        }];
        adapter.apply_ref_updates(&del).expect("delete main ref");
        assert!(adapter.read_ref(&"refs/heads/main".to_owned()).is_ok());
        assert!(
            adapter
                .read_ref(&"refs/heads/main".to_owned())
                .expect("read missing")
                .is_none()
        );
    }

    #[test]
    fn cli_refs_store_adapter_rejects_stale_symbolic_target() {
        let dir = TempDir::new().expect("temp dir");
        let git_dir = dir.path().join(".git");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        let adapter = CliRefsStoreAdapter::new(&refs);

        refs.write_symbolic_ref("HEAD", "refs/heads/main")
            .expect("write symbolic head");

        let error = adapter
            .read_ref(&"HEAD".to_owned())
            .expect_err("symbolic to missing target should fail");
        assert!(matches!(error, PrimitiveError::Storage { .. }));
    }
}
