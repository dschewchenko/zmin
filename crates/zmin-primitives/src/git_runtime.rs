//! Shared runtime contracts for Git-compatible primitives.
//!
//! These traits are intentionally focused on reusable behavior that must be identical
//! between CLI, WASM, and custom clients.
//!
//! Keep implementations explicit and avoid compatibility shims here: this module is the
//! contract for reusable Git primitive surfaces.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::sync::{Arc, RwLock};

use crate::error::Result;

/// Canonical Git object identifier representation for reusable adapters.
pub type GitOid = String;

/// Canonical reference name representation used by custom clients and CLI core.
pub type GitRefName = String;

/// Canonical transport endpoint string (for example: URL or remote name alias).
pub type GitEndpoint = String;

/// Canonical repository path string used by transport and local adapters.
pub type GitRepoPath = String;

/// Logical execution mode for a reusable Git runtime.
///
/// Runtime mode is a local policy layer over the same primitive contracts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitRuntimeMode {
    /// Default mode with remote service expectations aligned to public Git workflows.
    Public,
    /// Mode where private repository policy and access checks are enabled.
    Private,
    /// Encrypted mode with client-side zero-knowledge constraints.
    Secret,
}

impl Default for GitRuntimeMode {
    fn default() -> Self {
        Self::Public
    }
}

impl GitRuntimeMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
            Self::Secret => "secret",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "public" => Some(Self::Public),
            "private" => Some(Self::Private),
            "secret" => Some(Self::Secret),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitObjectEnvelope {
    pub id: GitOid,
    pub size: usize,
    pub object_type: String,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct GitRefDelta {
    pub from: GitOid,
    pub to: GitOid,
}

#[derive(Clone, Debug)]
pub enum RefUpdateAction {
    Update,
    Create,
    Delete,
    NoChange,
}

#[derive(Clone, Debug)]
pub struct RefUpdate {
    pub name: GitRefName,
    pub old_oid: Option<GitOid>,
    pub new_oid: Option<GitOid>,
    pub action: RefUpdateAction,
    pub reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RefDiscovery {
    pub refs: BTreeMap<GitRefName, GitOid>,
    pub symref: Option<(GitRefName, GitRefName)>,
}

#[derive(Clone, Debug)]
pub struct TransportLease {
    pub ref_name: GitRefName,
    pub old_oid: GitOid,
    pub ident: String,
}

#[derive(Clone, Debug)]
pub enum GitTransportService {
    UploadPack,
    ReceivePack,
}

#[derive(Clone, Debug)]
pub struct TransferRequest {
    pub remote: GitEndpoint,
    pub service: GitTransportService,
    pub refspecs: Vec<String>,
    pub atomic: bool,
    pub thin_pack: bool,
    pub depth: Option<u32>,
    pub filter: Option<String>,
    pub lease: Option<TransportLease>,
}

/// Transport contract used by reusable CLI and custom clients.
pub trait GitTransport: Send + Sync {
    fn discover_refs(&self, remote: &GitEndpoint) -> Result<RefDiscovery>;

    fn has_remote_object(&self, remote: &GitEndpoint, oid: &GitOid) -> Result<bool>;

    fn upload_pack_request(
        &self,
        request: &TransferRequest,
        stdin: &mut dyn Read,
    ) -> Result<Vec<u8>>;

    fn receive_pack_request(
        &self,
        request: &TransferRequest,
        stdin: &mut dyn Read,
    ) -> Result<Vec<u8>>;

    fn read_object_stream(
        &self,
        remote: &GitEndpoint,
        oid: &GitOid,
        writer: &mut dyn Write,
    ) -> Result<usize>;
}

/// Object store contract for reusable fetch/pack/object materialization workflows.
pub trait GitObjectStore: Send + Sync {
    fn read_envelope(&self, oid: &GitOid, size_hint: Option<usize>) -> Result<GitObjectEnvelope>;

    fn read_object_content(&self, oid: &GitOid) -> Result<Vec<u8>>;

    fn write_object_content(&self, envelope: &GitObjectEnvelope, content: &[u8]) -> Result<GitOid>;

    fn object_exists(&self, oid: &GitOid) -> Result<bool> {
        match self.read_envelope(oid, None) {
            Ok(_) => Ok(true),
            Err(crate::Error::Validation { .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn stream_object_to_writer(
        &self,
        oid: &GitOid,
        output: &mut dyn Write,
        max_bytes: Option<usize>,
    ) -> io::Result<usize> {
        let mut content = self
            .read_object_content(oid)
            .map_err(|error| io::Error::other(format!("read object {oid}: {error}")))?;
        if let Some(limit) = max_bytes {
            if content.len() > limit {
                content.truncate(limit);
            }
        }
        output
            .write_all(&content)
            .map_err(|error| io::Error::new(io::ErrorKind::WriteZero, error))?;
        Ok(content.len())
    }
}

/// Refs and packed refs contract for update-ref-like flows.
pub trait GitRefsStore: Send + Sync {
    fn read_ref(&self, name: &GitRefName) -> Result<Option<GitOid>>;

    fn write_ref(&self, name: &GitRefName, value: &GitOid) -> Result<()>;

    fn delete_ref(&self, name: &GitRefName) -> Result<()>;

    fn read_symbolic_ref(&self, _name: &GitRefName) -> Result<Option<GitRefName>> {
        Err(crate::Error::UnsupportedRuntime {
            runtime: "read_symbolic_ref is not supported by this runtime".to_owned(),
        })
    }

    fn write_symbolic_ref(&self, _name: &GitRefName, _target: &GitRefName) -> Result<()> {
        Err(crate::Error::UnsupportedRuntime {
            runtime: "write_symbolic_ref is not supported by this runtime".to_owned(),
        })
    }

    fn visit_refs(
        &self,
        pattern: Option<&str>,
        visitor: &mut dyn FnMut(&GitRefName, &GitOid) -> Result<()>,
    ) -> Result<()>;

    fn list_refs(&self, pattern: Option<&str>) -> Result<Vec<(GitRefName, GitOid)>>;

    fn pack_refs(&self, _all: bool, _prune: bool) -> Result<()> {
        Ok(())
    }

    fn begin_transaction(&self) -> Result<()> {
        Ok(())
    }

    fn apply_ref_updates(&self, updates: &[RefUpdate]) -> Result<()>;

    fn commit_transaction(&self) -> Result<()>;

    fn rollback_transaction(&self) -> Result<()>;
}

/// Checkout/worktree contract for reusable clients.
pub trait GitWorktreeEngine: Send + Sync {
    fn materialize_file(&self, path: &GitRepoPath, content: &[u8]) -> Result<()>;

    fn remove_path(&self, path: &GitRepoPath) -> Result<()>;

    fn touch_path(&self, path: &GitRepoPath) -> Result<()>;

    fn read_path(&self, path: &GitRepoPath) -> Result<Vec<u8>>;

    fn rename_path(&self, from: &GitRepoPath, to: &GitRepoPath) -> Result<()>;
}

/// Patch/mail contract used by `git format-patch`, `git apply`, and custom review tools.
pub trait GitPatchRenderer: Send + Sync {
    fn render_diff_patch(
        &self,
        old_object: &GitOid,
        new_object: &GitOid,
        writer: &mut dyn Write,
    ) -> Result<usize>;

    fn render_format_patch(
        &self,
        commits: &[GitOid],
        writer: &mut dyn Write,
        with_binary: bool,
    ) -> Result<usize>;
}

/// Merge/rewrite contract for reusable history rewrite helpers.
pub trait GitRewriteEngine: Send + Sync {
    fn rewrite_commit_tree(&self, commit_oid: &GitOid, new_tree: &GitOid) -> Result<GitOid>;

    fn create_replacement_commit(&self, source: &GitOid, message: &str) -> Result<GitOid>;
}

/// Aggregate trait used by higher-level CLI shells and custom clients.
pub trait GitPrimitiveRuntime: Send + Sync {
    fn transport(&self) -> &dyn GitTransport;

    fn objects(&self) -> &dyn GitObjectStore;

    fn refs(&self) -> &dyn GitRefsStore;

    fn worktree(&self) -> &dyn GitWorktreeEngine;

    fn patch_renderer(&self) -> &dyn GitPatchRenderer;

    fn rewrite(&self) -> &dyn GitRewriteEngine;

    /// Returns the active execution mode for this runtime instance.
    ///
    /// Custom clients and non-CLI integrations should route policy checks by this value.
    fn runtime_mode(&self) -> GitRuntimeMode {
        GitRuntimeMode::Public
    }
}

/// In-memory object store implementation for custom clients, tests, and WASM adapters.
#[derive(Debug, Default, Clone)]
pub struct InMemoryGitObjectStore {
    objects: Arc<RwLock<BTreeMap<GitOid, GitObjectEnvelope>>>,
    contents: Arc<RwLock<BTreeMap<GitOid, Vec<u8>>>>,
}

impl InMemoryGitObjectStore {
    pub fn new() -> Self {
        Self {
            objects: Arc::new(RwLock::new(BTreeMap::new())),
            contents: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    fn is_valid_object_id(oid: &GitOid) -> bool {
        (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn normalize_object_id(raw_id: &GitOid) -> crate::Result<GitOid> {
        if Self::is_valid_object_id(raw_id) {
            return Ok(raw_id.to_ascii_lowercase());
        }

        Err(crate::Error::Validation {
            details: format!("invalid object id: {raw_id}"),
        })
    }
}

impl GitObjectStore for InMemoryGitObjectStore {
    fn read_envelope(&self, oid: &GitOid, _size_hint: Option<usize>) -> Result<GitObjectEnvelope> {
        let objects = self.objects.read().map_err(|error| crate::Error::Storage {
            details: format!("failed to read object envelope {oid}: {error}"),
        })?;
        objects
            .get(oid)
            .cloned()
            .ok_or_else(|| crate::Error::Validation {
                details: format!("object not found: {oid}"),
            })
    }

    fn read_object_content(&self, oid: &GitOid) -> Result<Vec<u8>> {
        let contents = self
            .contents
            .read()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to read object content {oid}: {error}"),
            })?;
        contents
            .get(oid)
            .cloned()
            .ok_or_else(|| crate::Error::Validation {
                details: format!("object not found: {oid}"),
            })
    }

    fn write_object_content(&self, envelope: &GitObjectEnvelope, content: &[u8]) -> Result<GitOid> {
        let id = Self::normalize_object_id(&envelope.id)?;
        let size = content.len();
        let payload = GitObjectEnvelope {
            id: id.clone(),
            size,
            object_type: envelope.object_type.clone(),
            metadata: envelope.metadata.clone(),
        };

        self.objects
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to write object envelope {id}: {error}"),
            })?
            .insert(id.clone(), payload);
        self.contents
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to write object content {id}: {error}"),
            })?
            .insert(id.clone(), content.to_vec());

        Ok(id)
    }
}

#[derive(Debug, Default)]
pub struct InMemoryGitRefsStore {
    refs: Arc<RwLock<BTreeMap<GitRefName, GitOid>>>,
    transactions: Arc<RwLock<Vec<BTreeMap<GitRefName, GitOid>>>>,
}

impl InMemoryGitRefsStore {
    pub fn new() -> Self {
        Self {
            refs: Arc::new(RwLock::new(BTreeMap::new())),
            transactions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn validate_ref_update(current: Option<&GitOid>, update: &RefUpdate) -> Result<()> {
        match update.action {
            RefUpdateAction::Create if current.is_some() => Err(crate::Error::Validation {
                details: format!("create ref {}: already exists", update.name),
            }),
            RefUpdateAction::Delete if current.is_none() => Err(crate::Error::Validation {
                details: format!("delete ref {}: does not exist", update.name),
            }),
            RefUpdateAction::Update if update.new_oid.is_none() => Err(crate::Error::Validation {
                details: format!("update ref {}: missing new value", update.name),
            }),
            RefUpdateAction::NoChange if update.old_oid != current.cloned() => {
                Err(crate::Error::Validation {
                    details: format!("noop ref {}: expected current value", update.name),
                })
            }
            _ => Ok(()),
        }
    }
}

impl GitRefsStore for InMemoryGitRefsStore {
    fn read_ref(&self, name: &GitRefName) -> Result<Option<GitOid>> {
        let refs = self.refs.read().map_err(|error| crate::Error::Storage {
            details: format!("failed to read ref {name}: {error}"),
        })?;
        Ok(refs.get(name).cloned())
    }

    fn write_ref(&self, name: &GitRefName, value: &GitOid) -> Result<()> {
        self.refs
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to write ref {name}: {error}"),
            })?
            .insert(name.clone(), value.clone());
        Ok(())
    }

    fn delete_ref(&self, name: &GitRefName) -> Result<()> {
        self.refs
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to delete ref {name}: {error}"),
            })?
            .remove(name);
        Ok(())
    }

    fn list_refs(&self, pattern: Option<&str>) -> Result<Vec<(GitRefName, GitOid)>> {
        let prefix = pattern.unwrap_or("");
        let refs = self.refs.read().map_err(|error| crate::Error::Storage {
            details: format!("failed to list refs {prefix}: {error}"),
        })?;
        let mut out = Vec::new();
        for (name, value) in refs.iter() {
            if name.starts_with(prefix) {
                out.push((name.clone(), value.clone()));
            }
        }
        Ok(out)
    }

    fn visit_refs(
        &self,
        pattern: Option<&str>,
        visitor: &mut dyn FnMut(&GitRefName, &GitOid) -> Result<()>,
    ) -> Result<()> {
        let prefix = pattern.unwrap_or("");
        let refs = self.refs.read().map_err(|error| crate::Error::Storage {
            details: format!("failed to visit refs {prefix}: {error}"),
        })?;
        for (name, value) in refs.iter() {
            if name.starts_with(prefix) {
                visitor(name, value)?;
            }
        }
        Ok(())
    }

    fn begin_transaction(&self) -> Result<()> {
        let snapshot = self
            .refs
            .read()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to begin ref transaction: {error}"),
            })?
            .clone();
        self.transactions
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to begin ref transaction: {error}"),
            })?
            .push(snapshot);
        Ok(())
    }

    fn apply_ref_updates(&self, updates: &[RefUpdate]) -> Result<()> {
        let mut refs = self.refs.write().map_err(|error| crate::Error::Storage {
            details: format!("failed to apply ref updates: {error}"),
        })?;

        for update in updates {
            let current = refs.get(&update.name);
            Self::validate_ref_update(current, update)?;

            match update.action {
                RefUpdateAction::NoChange => {}
                RefUpdateAction::Create | RefUpdateAction::Update => {
                    if let Some(value) = update.new_oid.as_ref() {
                        refs.insert(update.name.clone(), value.clone());
                    }
                }
                RefUpdateAction::Delete => {
                    refs.remove(&update.name);
                }
            }
        }
        Ok(())
    }

    fn commit_transaction(&self) -> Result<()> {
        self.transactions
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to commit ref transaction: {error}"),
            })?
            .pop();
        Ok(())
    }

    fn rollback_transaction(&self) -> Result<()> {
        let snapshot = self
            .transactions
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("failed to rollback ref transaction: {error}"),
            })?
            .pop()
            .ok_or_else(|| crate::Error::Validation {
                details: String::from("no active ref transaction to rollback"),
            })?;

        let mut refs = self.refs.write().map_err(|error| crate::Error::Storage {
            details: format!("failed to rollback ref transaction: {error}"),
        })?;
        *refs = snapshot;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryPrimitiveRuntime {
    mode: GitRuntimeMode,
    objects: InMemoryGitObjectStore,
    refs: InMemoryGitRefsStore,
    worktree: InMemoryWorktree,
    patch_renderer: InMemoryPatchRenderer,
    rewrite: InMemoryRewriteEngine,
}

impl InMemoryPrimitiveRuntime {
    pub fn new() -> Self {
        Self::new_with_mode(GitRuntimeMode::Public)
    }

    pub fn new_with_mode(mode: GitRuntimeMode) -> Self {
        let objects = InMemoryGitObjectStore::new();
        Self {
            mode,
            objects: objects.clone(),
            refs: InMemoryGitRefsStore::new(),
            worktree: InMemoryWorktree::default(),
            patch_renderer: InMemoryPatchRenderer::new(objects.clone()),
            rewrite: InMemoryRewriteEngine::new(objects),
        }
    }

    pub fn mode(&self) -> GitRuntimeMode {
        self.mode
    }

    pub fn objects_store(&self) -> &InMemoryGitObjectStore {
        &self.objects
    }

    pub fn refs_store(&self) -> &InMemoryGitRefsStore {
        &self.refs
    }
}

/// Shared constructors for non-CLI runtime clients.
///
/// The factory is intentionally small and explicit: caller passes all policy/transport
/// choices and gets back a primitive runtime that implements `GitPrimitiveRuntime`.
pub struct GitPrimitiveRuntimeFactory;

impl GitPrimitiveRuntimeFactory {
    pub fn in_memory(mode: GitRuntimeMode) -> InMemoryPrimitiveRuntime {
        InMemoryPrimitiveRuntime::new_with_mode(mode)
    }

    pub fn in_memory_public() -> InMemoryPrimitiveRuntime {
        InMemoryPrimitiveRuntime::new()
    }

    pub fn custom_client<T>(mode: GitRuntimeMode, transport: T) -> CustomClientPrimitiveRuntime
    where
        T: GitTransport + 'static,
    {
        CustomClientPrimitiveRuntime::new_with_mode(mode, transport)
    }

    pub fn custom_client_with_transport(
        mode: GitRuntimeMode,
        transport: Arc<dyn GitTransport>,
    ) -> CustomClientPrimitiveRuntime {
        CustomClientPrimitiveRuntime::new_with_mode_and_transport(mode, transport)
    }
}

/// Primitive runtime that reuses in-memory object/ref/worktree stores and accepts
/// a pluggable transport implementation.
///
/// This is intended for custom clients and non-CLI integrations that need a
/// stable reusable contract while owning transport behaviour externally.
pub struct CustomClientPrimitiveRuntime {
    mode: GitRuntimeMode,
    transport: Arc<dyn GitTransport>,
    objects: InMemoryGitObjectStore,
    refs: InMemoryGitRefsStore,
    worktree: InMemoryWorktree,
    patch_renderer: InMemoryPatchRenderer,
    rewrite: InMemoryRewriteEngine,
}

impl CustomClientPrimitiveRuntime {
    pub fn new<T>(transport: T) -> Self
    where
        T: GitTransport + 'static,
    {
        Self::new_with_mode(GitRuntimeMode::Public, transport)
    }

    pub fn new_with_mode<T>(mode: GitRuntimeMode, transport: T) -> Self
    where
        T: GitTransport + 'static,
    {
        Self::new_with_mode_and_transport(mode, Arc::new(transport))
    }

    pub fn new_with_mode_and_transport(
        mode: GitRuntimeMode,
        transport: Arc<dyn GitTransport>,
    ) -> Self {
        let objects = InMemoryGitObjectStore::new();
        Self {
            mode,
            transport,
            objects: objects.clone(),
            refs: InMemoryGitRefsStore::new(),
            worktree: InMemoryWorktree::default(),
            patch_renderer: InMemoryPatchRenderer::new(objects.clone()),
            rewrite: InMemoryRewriteEngine::new(objects),
        }
    }

    pub fn mode(&self) -> GitRuntimeMode {
        self.mode
    }
}

struct InMemoryGitTransport;

impl GitTransport for InMemoryGitTransport {
    fn discover_refs(&self, remote: &GitEndpoint) -> Result<RefDiscovery> {
        Err(crate::Error::Validation {
            details: format!("no transport source for remote {remote}"),
        })
    }

    fn has_remote_object(&self, remote: &GitEndpoint, _oid: &GitOid) -> Result<bool> {
        Err(crate::Error::Validation {
            details: format!("no transport source for remote {remote}"),
        })
    }

    fn upload_pack_request(
        &self,
        request: &TransferRequest,
        _stdin: &mut dyn Read,
    ) -> Result<Vec<u8>> {
        Err(crate::Error::Validation {
            details: format!(
                "upload-pack not supported for in-memory transport: {:?}",
                request.service
            ),
        })
    }

    fn receive_pack_request(
        &self,
        request: &TransferRequest,
        _stdin: &mut dyn Read,
    ) -> Result<Vec<u8>> {
        Err(crate::Error::Validation {
            details: format!(
                "receive-pack not supported for in-memory transport: {:?}",
                request.service
            ),
        })
    }

    fn read_object_stream(
        &self,
        remote: &GitEndpoint,
        _oid: &GitOid,
        _writer: &mut dyn Write,
    ) -> Result<usize> {
        Err(crate::Error::Validation {
            details: format!("no transport stream for remote {remote}"),
        })
    }
}

fn synthetic_in_memory_oid(parts: &[&str]) -> GitOid {
    let mut hasher = DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
    }

    let digest = hasher.finish();
    let seed = format!("{:016x}", digest);
    let mut oid = String::new();
    while oid.len() < 40 {
        oid.push_str(&seed);
    }
    oid.truncate(40);
    oid
}

#[derive(Debug, Clone, Default)]
struct InMemoryPatchRenderer {
    objects: InMemoryGitObjectStore,
}

impl InMemoryPatchRenderer {
    fn new(objects: InMemoryGitObjectStore) -> Self {
        Self { objects }
    }
}

impl GitPatchRenderer for InMemoryPatchRenderer {
    fn render_diff_patch(
        &self,
        old_object: &GitOid,
        new_object: &GitOid,
        writer: &mut dyn Write,
    ) -> Result<usize> {
        let old_object = InMemoryGitObjectStore::normalize_object_id(old_object)?;
        let new_object = InMemoryGitObjectStore::normalize_object_id(new_object)?;
        let old_envelope = self
            .objects
            .read_envelope(&old_object, None)
            .map_err(|error| crate::Error::Storage {
                details: format!("read old object {old_object}: {error}"),
            })?;
        let new_envelope = self
            .objects
            .read_envelope(&new_object, None)
            .map_err(|error| crate::Error::Storage {
                details: format!("read new object {new_object}: {error}"),
            })?;
        let payload = format!(
            "diff --git a/{old_object} b/{new_object}\n\
index {old_prefix}..{new_prefix} 100644\n\
--- a/{old_object}\n\
+++ b/{new_object}\n\
@@\n\
-old object type={old_type} size={old_size}\n\
+new object type={new_type} size={new_size}\n",
            old_object = old_object,
            new_object = new_object,
            old_prefix = &old_object[..7],
            new_prefix = &new_object[..7],
            old_type = old_envelope.object_type,
            new_type = new_envelope.object_type,
            old_size = old_envelope.size,
            new_size = new_envelope.size
        );
        writer
            .write_all(payload.as_bytes())
            .map_err(|error| crate::Error::Storage {
                details: format!("write in-memory patch payload: {error}"),
            })?;
        Ok(payload.len())
    }

    fn render_format_patch(
        &self,
        commits: &[GitOid],
        writer: &mut dyn Write,
        with_binary: bool,
    ) -> Result<usize> {
        let mut output = Vec::new();
        for (index, commit) in commits.iter().enumerate() {
            let commit = InMemoryGitObjectStore::normalize_object_id(commit)?;
            let envelope = self.objects.read_envelope(&commit, None).map_err(|error| {
                crate::Error::Storage {
                    details: format!("read commit {commit}: {error}"),
                }
            })?;
            if envelope.object_type != "commit" {
                return Err(crate::Error::Validation {
                    details: format!(
                        "expected commit object, got {commit}={}",
                        envelope.object_type
                    ),
                });
            }
            output.extend_from_slice(
                format!(
                    "From {commit} Tue, 01 Jan 2000 00:00:00 +0000\n\
Subject: [PATCH {index}] {commit}\n\
Content-Type: text/plain; charset=UTF-8\n\
with_binary={with_binary}\n\
\n\
synthetic format-patch payload\n\n",
                    index = index + 1,
                    commit = commit,
                    with_binary = with_binary
                )
                .as_bytes(),
            );
        }
        writer
            .write_all(&output)
            .map_err(|error| crate::Error::Storage {
                details: format!("write in-memory format-patch payload: {error}"),
            })?;
        Ok(output.len())
    }
}

#[derive(Debug, Clone, Default)]
struct InMemoryRewriteEngine {
    objects: InMemoryGitObjectStore,
}

impl InMemoryRewriteEngine {
    fn new(objects: InMemoryGitObjectStore) -> Self {
        Self { objects }
    }
}

impl GitRewriteEngine for InMemoryRewriteEngine {
    fn rewrite_commit_tree(&self, commit_oid: &GitOid, new_tree: &GitOid) -> Result<GitOid> {
        let commit_oid = InMemoryGitObjectStore::normalize_object_id(commit_oid)?;
        let new_tree = InMemoryGitObjectStore::normalize_object_id(new_tree)?;
        let source = self
            .objects
            .read_envelope(&commit_oid, None)
            .map_err(|error| crate::Error::Storage {
                details: format!("read source commit {commit_oid}: {error}"),
            })?;
        if source.object_type != "commit" {
            return Err(crate::Error::Validation {
                details: format!(
                    "expected commit object, got {commit_oid}={}",
                    source.object_type
                ),
            });
        }
        let tree = self
            .objects
            .read_envelope(&new_tree, None)
            .map_err(|error| crate::Error::Storage {
                details: format!("read tree object {new_tree}: {error}"),
            })?;
        if tree.object_type != "tree" {
            return Err(crate::Error::Validation {
                details: format!("expected tree object, got {new_tree}={}", tree.object_type),
            });
        }
        let rewritten_id = synthetic_in_memory_oid(&[&commit_oid, &new_tree, "rewrite"]);
        let payload = format!("rewritten {commit_oid} -> {new_tree}\n").into_bytes();
        self.objects
            .write_object_content(
                &GitObjectEnvelope {
                    id: rewritten_id.clone(),
                    size: payload.len(),
                    object_type: String::from("commit"),
                    metadata: Default::default(),
                },
                &payload,
            )
            .map_err(|error| crate::Error::Storage {
                details: format!("write rewritten commit {rewritten_id}: {error}"),
            })?;
        Ok(rewritten_id)
    }

    fn create_replacement_commit(&self, source: &GitOid, message: &str) -> Result<GitOid> {
        let source_oid = InMemoryGitObjectStore::normalize_object_id(source)?;
        let source_object = self
            .objects
            .read_envelope(&source_oid, None)
            .map_err(|error| crate::Error::Storage {
                details: format!("read source commit {source_oid}: {error}"),
            })?;
        if source_object.object_type != "commit" {
            return Err(crate::Error::Validation {
                details: format!(
                    "expected commit object, got {source_oid}={}",
                    source_object.object_type
                ),
            });
        }
        let replacement_id = synthetic_in_memory_oid(&[&source_oid, message, "replace"]);
        let payload = format!("replacement commit {source_oid}\nmessage={message}\n").into_bytes();
        self.objects
            .write_object_content(
                &GitObjectEnvelope {
                    id: replacement_id.clone(),
                    size: payload.len(),
                    object_type: String::from("commit"),
                    metadata: Default::default(),
                },
                &payload,
            )
            .map_err(|error| crate::Error::Storage {
                details: format!("write replacement commit {replacement_id}: {error}"),
            })?;
        Ok(replacement_id)
    }
}

#[derive(Debug, Default)]
struct InMemoryWorktree {
    paths: Arc<RwLock<BTreeMap<GitRepoPath, Vec<u8>>>>,
}

impl GitWorktreeEngine for InMemoryWorktree {
    fn materialize_file(&self, path: &GitRepoPath, _content: &[u8]) -> Result<()> {
        self.paths
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("materialize in-memory worktree path {path}: {error}"),
            })?
            .insert(path.clone(), _content.to_vec());
        Ok(())
    }

    fn remove_path(&self, path: &GitRepoPath) -> Result<()> {
        let mut files = self.paths.write().map_err(|error| crate::Error::Storage {
            details: format!("remove in-memory worktree path {path}: {error}"),
        })?;

        if files.remove(path).is_none() {
            let prefix = format!("{path}/");
            let child_paths: Vec<_> = files
                .keys()
                .filter(|entry| entry.starts_with(&prefix))
                .cloned()
                .collect();
            for key in child_paths {
                files.remove(&key);
            }
        }
        Ok(())
    }

    fn touch_path(&self, path: &GitRepoPath) -> Result<()> {
        self.paths
            .write()
            .map_err(|error| crate::Error::Storage {
                details: format!("touch in-memory worktree path {path}: {error}"),
            })?
            .entry(path.clone())
            .or_default();
        Ok(())
    }

    fn read_path(&self, path: &GitRepoPath) -> Result<Vec<u8>> {
        self.paths
            .read()
            .map_err(|error| crate::Error::Storage {
                details: format!("read in-memory worktree path {path}: {error}"),
            })?
            .get(path)
            .cloned()
            .ok_or_else(|| crate::Error::Validation {
                details: format!("path not found in in-memory worktree: {path}"),
            })
    }

    fn rename_path(&self, from: &GitRepoPath, to: &GitRepoPath) -> Result<()> {
        let mut files = self.paths.write().map_err(|error| crate::Error::Storage {
            details: format!("rename in-memory worktree path {from}: {error}"),
        })?;

        if let Some(content) = files.remove(from) {
            files.insert(to.clone(), content);
            return Ok(());
        }

        let from_prefix = format!("{from}/");
        let to_prefix = format!("{to}/");
        let candidates: Vec<_> = files
            .keys()
            .filter(|entry| entry.starts_with(&from_prefix))
            .cloned()
            .collect();
        if candidates.is_empty() {
            return Err(crate::Error::Validation {
                details: format!("path not found in in-memory worktree: {from}"),
            });
        }

        for source in candidates {
            if let Some(content) = files.remove(&source) {
                let remainder = &source[from_prefix.len()..];
                let target = format!("{to_prefix}{remainder}");
                files.insert(target, content);
            }
        }
        Ok(())
    }
}

impl GitPrimitiveRuntime for InMemoryPrimitiveRuntime {
    fn transport(&self) -> &dyn GitTransport {
        static TRANSPORT: InMemoryGitTransport = InMemoryGitTransport;
        &TRANSPORT
    }

    fn objects(&self) -> &dyn GitObjectStore {
        &self.objects
    }

    fn refs(&self) -> &dyn GitRefsStore {
        &self.refs
    }

    fn worktree(&self) -> &dyn GitWorktreeEngine {
        &self.worktree
    }

    fn patch_renderer(&self) -> &dyn GitPatchRenderer {
        &self.patch_renderer
    }

    fn rewrite(&self) -> &dyn GitRewriteEngine {
        &self.rewrite
    }

    fn runtime_mode(&self) -> GitRuntimeMode {
        self.mode
    }
}

impl GitPrimitiveRuntime for CustomClientPrimitiveRuntime {
    fn transport(&self) -> &dyn GitTransport {
        self.transport.as_ref()
    }

    fn objects(&self) -> &dyn GitObjectStore {
        &self.objects
    }

    fn refs(&self) -> &dyn GitRefsStore {
        &self.refs
    }

    fn worktree(&self) -> &dyn GitWorktreeEngine {
        &self.worktree
    }

    fn patch_renderer(&self) -> &dyn GitPatchRenderer {
        &self.patch_renderer
    }

    fn rewrite(&self) -> &dyn GitRewriteEngine {
        &self.rewrite
    }

    fn runtime_mode(&self) -> GitRuntimeMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GitEndpoint, GitObjectStore, GitOid, GitPrimitiveRuntime, GitPrimitiveRuntimeFactory,
        GitRefsStore, GitRuntimeMode, GitTransport, InMemoryGitObjectStore, InMemoryGitRefsStore,
        InMemoryGitTransport, InMemoryPrimitiveRuntime, RefDiscovery, RefUpdate, RefUpdateAction,
        TransferRequest,
    };
    use crate::Error;
    use std::collections::BTreeMap;
    use std::io::Write;

    #[test]
    fn git_runtime_mode_default_is_public() {
        assert_eq!(GitRuntimeMode::default(), GitRuntimeMode::Public);
    }

    #[test]
    fn git_runtime_mode_as_str_and_from_str_are_inverse_for_public_private_secret() {
        for expected in [
            (GitRuntimeMode::Public, "public"),
            (GitRuntimeMode::Private, "private"),
            (GitRuntimeMode::Secret, "secret"),
        ] {
            assert_eq!(expected.0.as_str(), expected.1);
            assert_eq!(GitRuntimeMode::from_str(expected.1), Some(expected.0));
        }
    }

    #[test]
    fn git_runtime_mode_from_str_unknown_is_none() {
        assert_eq!(GitRuntimeMode::from_str(""), None);
        assert_eq!(GitRuntimeMode::from_str("Public"), None);
        assert_eq!(GitRuntimeMode::from_str("secretly"), None);
    }

    #[test]
    fn in_memory_object_store_writes_and_reads_objects() {
        let objects = InMemoryGitObjectStore::new();
        let id = objects
            .write_object_content(
                &crate::git_runtime::GitObjectEnvelope {
                    id: "0".repeat(40),
                    size: 3,
                    object_type: String::from("blob"),
                    metadata: BTreeMap::new(),
                },
                b"abc",
            )
            .expect("write object");

        let envelope = objects.read_envelope(&id, None).expect("read envelope");
        let content = objects.read_object_content(&id).expect("read content");
        assert!(envelope.size > 0);
        assert_eq!(content, b"abc");
    }

    #[test]
    fn in_memory_refs_store_applies_updates_with_transactions() {
        let refs = InMemoryGitRefsStore::new();
        refs.write_ref(&String::from("refs/heads/main"), &"{\"1\"}".into())
            .expect("write main");
        refs.begin_transaction().expect("begin tx");
        refs.apply_ref_updates(&[RefUpdate {
            name: String::from("refs/heads/main"),
            old_oid: Some(String::from("{\"1\"}")),
            new_oid: Some(String::from("{\"2\"}")),
            action: RefUpdateAction::Update,
            reason: None,
        }])
        .expect("apply update");
        refs.rollback_transaction().expect("rollback");
        assert_eq!(
            refs.read_ref(&String::from("refs/heads/main"))
                .expect("read after rollback"),
            Some(String::from("{\"1\"}"))
        );
    }

    #[test]
    fn in_memory_primitive_runtime_exposes_core_facets() {
        let runtime = InMemoryPrimitiveRuntime::new();
        assert_eq!(runtime.runtime_mode(), GitRuntimeMode::Public);
        runtime
            .objects()
            .write_object_content(
                &crate::git_runtime::GitObjectEnvelope {
                    id: String::from("0".repeat(40)),
                    size: 0,
                    object_type: String::from("blob"),
                    metadata: BTreeMap::new(),
                },
                b"data",
            )
            .expect("write object");
        runtime
            .refs()
            .write_ref(&String::from("refs/heads/main"), &"0".repeat(40))
            .expect("write ref");

        assert!(
            runtime
                .transport()
                .discover_refs(&String::from("origin"))
                .is_err()
        );
        assert_eq!(
            runtime
                .refs()
                .read_ref(&String::from("refs/heads/main"))
                .expect("read ref"),
            Some("0".repeat(40))
        );
    }

    #[test]
    fn in_memory_primitive_runtime_mode_is_configurable_for_custom_clients() {
        let public = InMemoryPrimitiveRuntime::new_with_mode(GitRuntimeMode::Public);
        let private = InMemoryPrimitiveRuntime::new_with_mode(GitRuntimeMode::Private);
        let secret = InMemoryPrimitiveRuntime::new_with_mode(GitRuntimeMode::Secret);

        assert_eq!(public.runtime_mode(), GitRuntimeMode::Public);
        assert_eq!(private.runtime_mode(), GitRuntimeMode::Private);
        assert_eq!(secret.runtime_mode(), GitRuntimeMode::Secret);

        // Ensure the same shared API stays available regardless of mode.
        let object_id = public
            .objects()
            .write_object_content(
                &crate::git_runtime::GitObjectEnvelope {
                    id: "1".repeat(40),
                    size: 7,
                    object_type: String::from("blob"),
                    metadata: BTreeMap::new(),
                },
                b"client",
            )
            .expect("write for public mode");

        let read_back = private
            .objects()
            .read_object_content(&object_id)
            .expect_err("private mode must not share object storage");

        assert!(
            matches!(read_back, crate::Error::Validation { .. }),
            "{read_back:?}"
        );

        let output = secret
            .patch_renderer()
            .render_format_patch(&[object_id.clone()], &mut Vec::new(), false)
            .expect_err("format-patch in empty storage should fail");
        let _ = output;
    }

    #[test]
    fn git_primitive_runtime_factory_constructs_expected_variants() {
        let public = GitPrimitiveRuntimeFactory::in_memory_public();
        let private = GitPrimitiveRuntimeFactory::in_memory(GitRuntimeMode::Private);
        let secret = GitPrimitiveRuntimeFactory::in_memory(GitRuntimeMode::Secret);

        assert_eq!(public.runtime_mode(), GitRuntimeMode::Public);
        assert_eq!(private.runtime_mode(), GitRuntimeMode::Private);
        assert_eq!(secret.runtime_mode(), GitRuntimeMode::Secret);

        let transport = InMemoryGitTransport;
        let runtime = GitPrimitiveRuntimeFactory::custom_client(GitRuntimeMode::Public, transport);
        assert_eq!(runtime.runtime_mode(), GitRuntimeMode::Public);
    }

    #[test]
    fn in_memory_worktree_supports_basic_file_ops() {
        let runtime = InMemoryPrimitiveRuntime::new();
        let worktree = runtime.worktree();
        let path = String::from("dir/file.txt");
        worktree
            .materialize_file(&path, b"hello")
            .expect("materialize file");
        assert_eq!(
            worktree.read_path(&path).expect("read file"),
            b"hello".to_vec()
        );

        let renamed = String::from("dir/file-renamed.txt");
        worktree.rename_path(&path, &renamed).expect("rename file");
        assert_eq!(
            worktree.read_path(&renamed).expect("read renamed"),
            b"hello".to_vec()
        );
        assert!(worktree.read_path(&path).is_err());
        worktree
            .touch_path(&String::from("dir/touched"))
            .expect("touch file");
        assert_eq!(
            worktree
                .read_path(&String::from("dir/touched"))
                .expect("read touched"),
            Vec::<u8>::new()
        );
        worktree.remove_path(&renamed).expect("remove renamed file");
        assert!(worktree.read_path(&renamed).is_err());
    }

    #[test]
    fn in_memory_patch_renderer_and_rewrite_are_deterministic() {
        let runtime = InMemoryPrimitiveRuntime::new();
        let objects = runtime.objects();
        let patch = runtime.patch_renderer();
        let rewrite = runtime.rewrite();

        let old_blob = objects
            .write_object_content(
                &crate::git_runtime::GitObjectEnvelope {
                    id: "1".repeat(40),
                    size: 4,
                    object_type: String::from("blob"),
                    metadata: BTreeMap::new(),
                },
                b"from",
            )
            .expect("write old blob");
        let new_blob = objects
            .write_object_content(
                &crate::git_runtime::GitObjectEnvelope {
                    id: "2".repeat(40),
                    size: 3,
                    object_type: String::from("blob"),
                    metadata: BTreeMap::new(),
                },
                b"to",
            )
            .expect("write new blob");

        let mut diff = Vec::new();
        let diff_size = patch
            .render_diff_patch(&old_blob, &new_blob, &mut diff)
            .expect("render diff");
        assert!(diff_size > 0);
        assert!(String::from_utf8_lossy(&diff).contains(&old_blob));

        let tree = objects
            .write_object_content(
                &crate::git_runtime::GitObjectEnvelope {
                    id: "3".repeat(40),
                    size: 0,
                    object_type: String::from("tree"),
                    metadata: BTreeMap::new(),
                },
                b"",
            )
            .expect("write tree");
        let commit = objects
            .write_object_content(
                &crate::git_runtime::GitObjectEnvelope {
                    id: "4".repeat(40),
                    size: 0,
                    object_type: String::from("commit"),
                    metadata: BTreeMap::new(),
                },
                b"commit",
            )
            .expect("write commit");

        let rewritten = rewrite
            .rewrite_commit_tree(&commit, &tree)
            .expect("rewrite commit");
        assert!(
            objects
                .read_object_content(&rewritten)
                .expect("read rewritten")
                .starts_with(b"rewritten")
        );

        let replacement = rewrite
            .create_replacement_commit(&commit, "updated message")
            .expect("replace commit");
        assert_ne!(replacement, rewritten);
    }

    struct ProbeTransport;

    impl GitTransport for ProbeTransport {
        fn discover_refs(&self, remote: &GitEndpoint) -> super::Result<RefDiscovery> {
            if remote != "origin" {
                return Err(Error::Validation {
                    details: format!("unsupported remote {remote}"),
                });
            }
            Ok(RefDiscovery {
                refs: BTreeMap::from([(
                    String::from("refs/heads/main"),
                    String::from("0".repeat(40)),
                )]),
                symref: Some((String::from("HEAD"), String::from("refs/heads/main"))),
            })
        }

        fn has_remote_object(&self, remote: &GitEndpoint, _oid: &GitOid) -> super::Result<bool> {
            if remote != "origin" {
                return Err(Error::Validation {
                    details: format!("unsupported remote {remote}"),
                });
            }
            Ok(true)
        }

        fn upload_pack_request(
            &self,
            _request: &TransferRequest,
            stdin: &mut dyn std::io::Read,
        ) -> super::Result<Vec<u8>> {
            let mut buffer = Vec::new();
            stdin
                .read_to_end(&mut buffer)
                .map_err(|error| Error::Storage {
                    details: format!("read upload stdin: {error}"),
                })?;
            Ok(format!("ok:{}B", buffer.len()).into_bytes())
        }

        fn receive_pack_request(
            &self,
            request: &TransferRequest,
            stdin: &mut dyn std::io::Read,
        ) -> super::Result<Vec<u8>> {
            self.upload_pack_request(request, stdin)
        }

        fn read_object_stream(
            &self,
            remote: &GitEndpoint,
            oid: &GitOid,
            writer: &mut dyn Write,
        ) -> super::Result<usize> {
            if remote != "origin" || oid.is_empty() {
                return Err(Error::Validation {
                    details: String::from("unexpected read request"),
                });
            }
            writer
                .write_all(format!("fetched:{oid}").as_bytes())
                .map_err(|error| Error::Storage {
                    details: format!("write stream {oid}: {error}"),
                })?;
            Ok(oid.len())
        }
    }

    #[test]
    fn custom_client_primitive_runtime_exposes_mode_and_facets() {
        let runtime = super::CustomClientPrimitiveRuntime::new_with_mode(
            GitRuntimeMode::Secret,
            ProbeTransport,
        );
        assert_eq!(runtime.runtime_mode(), GitRuntimeMode::Secret);

        let blob = runtime
            .objects()
            .write_object_content(
                &super::GitObjectEnvelope {
                    id: "1".repeat(40),
                    size: 2,
                    object_type: String::from("blob"),
                    metadata: BTreeMap::new(),
                },
                b"ok",
            )
            .expect("write object");
        assert_eq!(
            runtime.objects().read_object_content(&blob).expect("read"),
            b"ok".to_vec()
        );
        let discovered = runtime
            .transport()
            .discover_refs(&String::from("origin"))
            .expect("discover");
        assert_eq!(
            discovered.refs.get("refs/heads/main"),
            Some(&"0".repeat(40))
        );
    }
}
