//! Bounded Git graph planning for local Git LFS fetch and push operations.
//!
//! This module deliberately separates repository/ref discovery from graph
//! traversal.  A repository adapter supplies already-resolved roots through
//! [`LfsReachabilityRepository`]; the planner then walks Git objects through
//! `zmin-git-core` without invoking an external `git` process.
//!
//! Current command semantics are documented by upstream Git LFS:
//! <https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-fetch.adoc>,
//! <https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-push.adoc>,
//! and <https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-pre-push.adoc>.
//! Commit, annotated-tag and tree roots are supported. A direct blob root is a
//! typed MVP error because it has no repository path to report or filter.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::{self, BufRead};

use zmin_git_core::object_store::PrefixOrFullObject;
use zmin_git_core::{
    GitHashAlgorithm, GitObjectKind, GitObjectStore, ObjectId, TreeMode, check_ref_format,
    decode_commit_links, decode_tag, decode_tree_entry_ref,
};

use super::{
    LFS_POINTER_MAX_BYTES, LfsFetchFilter, LfsOid, LfsPointer, LfsPointerError,
    LfsRemoteSelectionPolicy,
};

const DEFAULT_MAX_ROOTS: usize = 65_536;
const DEFAULT_MAX_GRAPH_OBJECTS: usize = 1_000_000;
const DEFAULT_MAX_POINTERS: usize = 250_000;
const DEFAULT_MAX_TREE_DEPTH: usize = 1_024;
const DEFAULT_MAX_HISTORY_DEPTH: usize = 1_000_000;
const DEFAULT_MAX_STRUCTURED_OBJECT_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_PATH_BYTES: usize = 4_096;
const DEFAULT_MAX_PRE_PUSH_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_PRE_PUSH_LINE_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_PRE_PUSH_UPDATES: usize = 65_536;
const DEFAULT_MAX_RETAINED_PATH_BYTES: usize = 128 * 1024 * 1024;
const MAX_REVISION_BYTES: usize = 4_096;

/// Remote-selection inputs needed while discovering fetch roots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LfsReachabilityRemotePolicy {
    autodetect: bool,
    search_all: bool,
}

impl LfsReachabilityRemotePolicy {
    pub(crate) const fn new(autodetect: bool, search_all: bool) -> Self {
        Self {
            autodetect,
            search_all,
        }
    }

    pub(crate) const fn autodetect(self) -> bool {
        self.autodetect
    }

    pub(crate) const fn search_all(self) -> bool {
        self.search_all
    }
}

impl From<LfsRemoteSelectionPolicy> for LfsReachabilityRemotePolicy {
    fn from(policy: LfsRemoteSelectionPolicy) -> Self {
        Self::new(policy.autodetect(), policy.search_all())
    }
}

/// Hard resource ceilings for a single reachability plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LfsReachabilityLimits {
    max_roots: usize,
    max_graph_objects: usize,
    max_pointers: usize,
    max_tree_depth: usize,
    max_history_depth: usize,
    max_structured_object_bytes: usize,
    max_path_bytes: usize,
    max_pre_push_bytes: usize,
    max_pre_push_line_bytes: usize,
    max_pre_push_updates: usize,
    max_retained_path_bytes: usize,
}

impl Default for LfsReachabilityLimits {
    fn default() -> Self {
        Self {
            max_roots: DEFAULT_MAX_ROOTS,
            max_graph_objects: DEFAULT_MAX_GRAPH_OBJECTS,
            max_pointers: DEFAULT_MAX_POINTERS,
            max_tree_depth: DEFAULT_MAX_TREE_DEPTH,
            max_history_depth: DEFAULT_MAX_HISTORY_DEPTH,
            max_structured_object_bytes: DEFAULT_MAX_STRUCTURED_OBJECT_BYTES,
            max_path_bytes: DEFAULT_MAX_PATH_BYTES,
            max_pre_push_bytes: DEFAULT_MAX_PRE_PUSH_BYTES,
            max_pre_push_line_bytes: DEFAULT_MAX_PRE_PUSH_LINE_BYTES,
            max_pre_push_updates: DEFAULT_MAX_PRE_PUSH_UPDATES,
            max_retained_path_bytes: DEFAULT_MAX_RETAINED_PATH_BYTES,
        }
    }
}

impl LfsReachabilityLimits {
    pub(crate) fn validate(self) -> Result<Self, LfsReachabilityError> {
        let values = [
            self.max_roots,
            self.max_graph_objects,
            self.max_pointers,
            self.max_tree_depth,
            self.max_history_depth,
            self.max_structured_object_bytes,
            self.max_path_bytes,
            self.max_pre_push_bytes,
            self.max_pre_push_line_bytes,
            self.max_pre_push_updates,
            self.max_retained_path_bytes,
        ];
        if values.contains(&0)
            || self.max_pre_push_line_bytes > self.max_pre_push_bytes
            || self.max_roots > self.max_graph_objects
            || self.max_pointers > self.max_graph_objects
            || self.max_structured_object_bytes == usize::MAX
        {
            return Err(LfsReachabilityError::InvalidLimits);
        }
        Ok(self)
    }

    #[cfg(test)]
    fn with_max_tree_depth(mut self, max_tree_depth: usize) -> Self {
        self.max_tree_depth = max_tree_depth;
        self
    }

    #[cfg(test)]
    fn with_max_history_depth(mut self, max_history_depth: usize) -> Self {
        self.max_history_depth = max_history_depth;
        self
    }

    #[cfg(test)]
    fn with_max_pre_push_line_bytes(mut self, max_bytes: usize) -> Self {
        self.max_pre_push_line_bytes = max_bytes;
        self.max_pre_push_bytes = self.max_pre_push_bytes.max(max_bytes);
        self
    }

    #[cfg(test)]
    fn with_max_retained_path_bytes(mut self, max_bytes: usize) -> Self {
        self.max_retained_path_bytes = max_bytes;
        self
    }
}

/// Explicit revision names used by `lfs fetch`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsExplicitFetchSelection {
    revisions: Vec<String>,
}

impl LfsExplicitFetchSelection {
    pub(crate) fn new(revisions: Vec<String>) -> Result<Self, LfsReachabilityError> {
        if revisions.is_empty() {
            return Err(LfsReachabilityError::InvalidRevision);
        }
        for revision in &revisions {
            validate_revision(revision)?;
        }
        Ok(Self { revisions })
    }

    pub(crate) fn revisions(&self) -> &[String] {
        &self.revisions
    }
}

/// Time cutoffs used by current Git LFS recent-fetch behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LfsRecentFetchSelection {
    reference_cutoff_seconds: i64,
    commit_cutoff_seconds: i64,
}

impl LfsRecentFetchSelection {
    pub(crate) const fn new(reference_cutoff_seconds: i64, commit_cutoff_seconds: i64) -> Self {
        Self {
            reference_cutoff_seconds,
            commit_cutoff_seconds,
        }
    }

    pub(crate) const fn reference_cutoff_seconds(self) -> i64 {
        self.reference_cutoff_seconds
    }

    pub(crate) const fn commit_cutoff_seconds(self) -> i64 {
        self.commit_cutoff_seconds
    }
}

/// Supported fetch root/history modes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LfsFetchSelection {
    Current,
    Explicit(LfsExplicitFetchSelection),
    Recent(LfsRecentFetchSelection),
    All,
}

/// A single source revision/refspec selected for push.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsPushRevision {
    revision: String,
}

impl LfsPushRevision {
    pub(crate) fn new(revision: String) -> Result<Self, LfsReachabilityError> {
        validate_revision(&revision)?;
        if revision.starts_with('+') || revision.contains(':') || revision.contains("..") {
            return Err(LfsReachabilityError::InvalidRefspec);
        }
        Ok(Self { revision })
    }

    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }
}

/// A current, non-wildcard push refspec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsPushRefspec {
    source: String,
    destination: String,
    force: bool,
}

impl LfsPushRefspec {
    pub(crate) fn new(
        source: String,
        destination: String,
        force: bool,
    ) -> Result<Self, LfsReachabilityError> {
        validate_revision(&source)?;
        validate_revision(&destination)?;
        if source.contains('*')
            || destination.contains('*')
            || source.contains(':')
            || destination.contains(':')
            || source.contains("..")
            || destination.contains("..")
        {
            return Err(LfsReachabilityError::InvalidRefspec);
        }
        Ok(Self {
            source,
            destination,
            force,
        })
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn destination(&self) -> &str {
        &self.destination
    }

    pub(crate) const fn force(&self) -> bool {
        self.force
    }
}

/// The include/exclude revisions of a two-dot push range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsPushRange {
    exclude: String,
    include: String,
}

impl LfsPushRange {
    pub(crate) fn new(exclude: String, include: String) -> Result<Self, LfsReachabilityError> {
        validate_revision(&exclude)?;
        validate_revision(&include)?;
        if exclude.contains("..")
            || include.contains("..")
            || exclude.contains(':')
            || include.contains(':')
        {
            return Err(LfsReachabilityError::InvalidRefspec);
        }
        Ok(Self { exclude, include })
    }

    pub(crate) fn exclude(&self) -> &str {
        &self.exclude
    }

    pub(crate) fn include(&self) -> &str {
        &self.include
    }
}

/// A parsed, currently supported push command argument.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LfsPushSpec {
    Revision(LfsPushRevision),
    Refspec(LfsPushRefspec),
    Range(LfsPushRange),
}

impl LfsPushSpec {
    pub(crate) fn parse(value: &str) -> Result<Self, LfsReachabilityError> {
        if value.contains("...") {
            return Err(LfsReachabilityError::InvalidRefspec);
        }
        if let Some((exclude, include)) = value.split_once("..") {
            if include.contains("..") {
                return Err(LfsReachabilityError::InvalidRefspec);
            }
            return Ok(Self::Range(LfsPushRange::new(
                exclude.to_owned(),
                include.to_owned(),
            )?));
        }

        let (force, value) = match value.strip_prefix('+') {
            Some(value) => (true, value),
            None => (false, value),
        };
        if let Some((source, destination)) = value.split_once(':') {
            if destination.contains(':') {
                return Err(LfsReachabilityError::InvalidRefspec);
            }
            return Ok(Self::Refspec(LfsPushRefspec::new(
                source.to_owned(),
                destination.to_owned(),
                force,
            )?));
        }
        if force {
            return Err(LfsReachabilityError::InvalidRefspec);
        }
        Ok(Self::Revision(LfsPushRevision::new(value.to_owned())?))
    }
}

/// A possibly absent Git object in a pre-push update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LfsPrePushObject {
    Zero,
    Object(ObjectId),
}

/// Classification of a Git pre-push update line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsPrePushUpdateKind {
    Create,
    Update,
    Delete,
}

/// A fully validated Git pre-push update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsPrePushUpdate {
    local_ref: Vec<u8>,
    local: LfsPrePushObject,
    remote_ref: String,
    remote: LfsPrePushObject,
    kind: LfsPrePushUpdateKind,
}

impl LfsPrePushUpdate {
    pub(crate) fn local_ref(&self) -> &[u8] {
        &self.local_ref
    }

    pub(crate) fn local(&self) -> &LfsPrePushObject {
        &self.local
    }

    pub(crate) fn remote_ref(&self) -> &str {
        &self.remote_ref
    }

    pub(crate) fn remote(&self) -> &LfsPrePushObject {
        &self.remote
    }

    pub(crate) const fn kind(&self) -> LfsPrePushUpdateKind {
        self.kind
    }
}

/// A named resolved root passed without forcing an unbounded collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsRevisionRoot {
    name: String,
    object: ObjectId,
}

/// A revision resolved without losing its canonical ref identity, when one
/// exists. Full object IDs and detached `HEAD` values intentionally have no
/// reference identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsResolvedRevision {
    object: ObjectId,
    reference: Option<String>,
}

impl LfsResolvedRevision {
    pub(crate) fn detached(object: ObjectId) -> Self {
        Self {
            object,
            reference: None,
        }
    }

    pub(crate) fn referenced(
        object: ObjectId,
        reference: String,
    ) -> Result<Self, LfsReachabilityError> {
        validate_remote_ref(&reference)?;
        Ok(Self {
            object,
            reference: Some(reference),
        })
    }

    pub(crate) fn object(&self) -> &ObjectId {
        &self.object
    }

    pub(crate) fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }
}

impl LfsRevisionRoot {
    pub(crate) fn new(name: String, object: ObjectId) -> Result<Self, LfsReachabilityError> {
        validate_revision(&name)?;
        Ok(Self { name, object })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn object(&self) -> &ObjectId {
        &self.object
    }
}

/// Bounded sink used by repository adapters while enumerating refs.
pub(crate) trait LfsReachabilityRootVisitor {
    fn visit(&mut self, root: LfsRevisionRoot) -> Result<(), LfsReachabilityError>;
}

/// Repository/ref adapter required by the independent planner.
///
/// Implementations must use Zmin's repository, revision and reflog APIs.  The
/// interface intentionally has no external-process fallback.
pub(crate) trait LfsReachabilityRepository: GitObjectStore {
    fn object_format(&self) -> GitHashAlgorithm;

    /// Reads at most `max_bytes` of object content.
    ///
    /// This method is required rather than relying on `GitObjectStore`'s
    /// compatibility default, which may materialize a full object before
    /// truncating. Production adapters must delegate to the bounded loose or
    /// packed-store implementation.
    fn read_lfs_object_bounded(
        &self,
        object: &ObjectId,
        max_bytes: usize,
    ) -> io::Result<PrefixOrFullObject>;

    fn resolve_revision(
        &self,
        revision: &str,
    ) -> Result<Option<LfsResolvedRevision>, LfsReachabilityError>;

    /// Resolves the current object advertised for a push destination.
    ///
    /// The adapter is bound to the selected remote, so it can map a server ref
    /// to the corresponding advertised or remote-tracking object without the
    /// planner guessing a remote name.
    fn resolve_push_destination(
        &self,
        destination: &str,
    ) -> Result<Option<ObjectId>, LfsReachabilityError>;

    /// Maps a canonical local ref to the selected remote's server-side ref
    /// for Batch authorization. Implementations reverse configured fetch
    /// refspecs and preserve an already-server-side full ref.
    fn resolve_fetch_batch_ref(
        &self,
        reference: &str,
    ) -> Result<Option<String>, LfsReachabilityError>;

    fn current_fetch_revision(
        &self,
        policy: LfsReachabilityRemotePolicy,
    ) -> Result<Option<LfsResolvedRevision>, LfsReachabilityError>;

    fn visit_recent_fetch_roots(
        &self,
        reference_cutoff_seconds: i64,
        policy: LfsReachabilityRemotePolicy,
        visitor: &mut dyn LfsReachabilityRootVisitor,
    ) -> Result<(), LfsReachabilityError>;

    fn visit_all_fetch_roots(
        &self,
        policy: LfsReachabilityRemotePolicy,
        visitor: &mut dyn LfsReachabilityRootVisitor,
    ) -> Result<(), LfsReachabilityError>;
}

/// A canonical current LFS pointer and the deterministic Git path that found it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsReachablePointer {
    pointer: LfsPointer,
    repository_path: Vec<u8>,
    git_blob: ObjectId,
}

impl LfsReachablePointer {
    pub(crate) fn pointer(&self) -> &LfsPointer {
        &self.pointer
    }

    pub(crate) fn oid(&self) -> &LfsOid {
        self.pointer.oid()
    }

    pub(crate) fn size(&self) -> u64 {
        self.pointer.size()
    }

    pub(crate) fn repository_path(&self) -> &[u8] {
        &self.repository_path
    }

    pub(crate) fn git_blob(&self) -> &ObjectId {
        &self.git_blob
    }
}

/// Counters that make traversal and peak queue bounds observable in tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LfsReachabilityStats {
    roots: usize,
    pre_push_updates: usize,
    graph_objects: usize,
    commits: usize,
    tags: usize,
    tree_contexts: usize,
    unique_blobs: usize,
    blob_prefix_reads: usize,
    blob_prefix_bytes: usize,
    pointers: usize,
    deduplicated_pointers: usize,
    excluded_pointers: usize,
    max_pending_graph: usize,
    max_pending_trees: usize,
    max_tree_depth: usize,
    max_path_bytes: usize,
    retained_path_bytes: usize,
    max_retained_path_bytes: usize,
}

impl LfsReachabilityStats {
    pub(crate) const fn roots(self) -> usize {
        self.roots
    }

    pub(crate) const fn pre_push_updates(self) -> usize {
        self.pre_push_updates
    }

    pub(crate) const fn graph_objects(self) -> usize {
        self.graph_objects
    }

    pub(crate) const fn unique_blobs(self) -> usize {
        self.unique_blobs
    }

    pub(crate) const fn blob_prefix_reads(self) -> usize {
        self.blob_prefix_reads
    }

    pub(crate) const fn blob_prefix_bytes(self) -> usize {
        self.blob_prefix_bytes
    }

    pub(crate) const fn pointers(self) -> usize {
        self.pointers
    }

    pub(crate) const fn deduplicated_pointers(self) -> usize {
        self.deduplicated_pointers
    }

    pub(crate) const fn excluded_pointers(self) -> usize {
        self.excluded_pointers
    }

    pub(crate) const fn max_pending_graph(self) -> usize {
        self.max_pending_graph
    }

    pub(crate) const fn max_pending_trees(self) -> usize {
        self.max_pending_trees
    }

    pub(crate) const fn max_tree_depth(self) -> usize {
        self.max_tree_depth
    }

    pub(crate) const fn max_path_bytes(self) -> usize {
        self.max_path_bytes
    }

    pub(crate) const fn retained_path_bytes(self) -> usize {
        self.retained_path_bytes
    }

    pub(crate) const fn max_retained_path_bytes(self) -> usize {
        self.max_retained_path_bytes
    }
}

/// Deterministically ordered, LFS-OID-deduplicated traversal result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsReachabilityPlan {
    pointers: Vec<LfsReachablePointer>,
    transfer_groups: Vec<LfsReachabilityTransferGroup>,
    stats: LfsReachabilityStats,
}

impl LfsReachabilityPlan {
    pub(crate) fn pointers(&self) -> &[LfsReachablePointer] {
        &self.pointers
    }

    /// Batch-ref-specific transfer groups. Fetch, push and pre-push plans use
    /// indexes into the globally deduplicated pointer slice so an OID is
    /// transferred at most once.
    pub(crate) fn transfer_groups(&self) -> &[LfsReachabilityTransferGroup] {
        &self.transfer_groups
    }

    pub(crate) const fn stats(&self) -> LfsReachabilityStats {
        self.stats
    }
}

/// A deterministic Batch transfer group for one optional server ref.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LfsReachabilityTransferGroup {
    remote_ref: Option<String>,
    pointer_indexes: Vec<usize>,
}

impl LfsReachabilityTransferGroup {
    pub(crate) fn remote_ref(&self) -> Option<&str> {
        self.remote_ref.as_deref()
    }

    pub(crate) fn pointer_indexes(&self) -> &[usize] {
        &self.pointer_indexes
    }
}

#[derive(Clone, Debug)]
struct PendingTransferGroup {
    remote_ref: Option<String>,
    include: Vec<ObjectId>,
}

#[derive(Clone, Debug)]
struct PlannedTransferGroup {
    remote_ref: Option<String>,
    pointer_oids: Vec<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryTraversal {
    TipsOnly,
    All,
    Since(i64),
}

#[derive(Clone, Debug)]
struct GraphTask {
    object: ObjectId,
    depth: usize,
    is_root: bool,
    expected_kind: Option<GitObjectKind>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TreeContextKey {
    object: ObjectId,
    path: Vec<u8>,
}

#[derive(Clone, Debug)]
struct TreeTask {
    object: ObjectId,
    path: Vec<u8>,
    depth: usize,
}

struct ExcludedGraph {
    commits: HashSet<ObjectId>,
    trees: VecDeque<TreeTask>,
}

struct RootCollector<'a> {
    roots: &'a mut Vec<LfsRevisionRoot>,
    max_roots: usize,
    retained_name_bytes: &'a mut usize,
    max_retained_name_bytes: usize,
}

impl LfsReachabilityRootVisitor for RootCollector<'_> {
    fn visit(&mut self, root: LfsRevisionRoot) -> Result<(), LfsReachabilityError> {
        if self.roots.len() >= self.max_roots {
            return Err(LfsReachabilityError::TooManyRoots);
        }
        *self.retained_name_bytes = self
            .retained_name_bytes
            .checked_add(root.name.len())
            .ok_or(LfsReachabilityError::RetainedPathBytesExceeded)?;
        if *self.retained_name_bytes > self.max_retained_name_bytes {
            return Err(LfsReachabilityError::RetainedPathBytesExceeded);
        }
        self.roots.push(root);
        Ok(())
    }
}

struct TraversalState {
    stats: LfsReachabilityStats,
    counted_objects: HashSet<ObjectId>,
    seen_tree_contexts: HashSet<TreeContextKey>,
    blob_cache: HashMap<ObjectId, Option<[u8; 32]>>,
    pointers: HashMap<[u8; 32], LfsReachablePointer>,
    excluded_pointers: HashMap<[u8; 32], LfsPointer>,
    retained_path_bytes: usize,
}

impl TraversalState {
    fn new(root_count: usize, pre_push_updates: usize) -> Self {
        Self {
            stats: LfsReachabilityStats {
                roots: root_count,
                pre_push_updates,
                ..LfsReachabilityStats::default()
            },
            counted_objects: HashSet::new(),
            seen_tree_contexts: HashSet::new(),
            blob_cache: HashMap::new(),
            pointers: HashMap::new(),
            excluded_pointers: HashMap::new(),
            retained_path_bytes: 0,
        }
    }
}

/// Bounded reachability engine over a typed repository adapter.
pub(crate) struct LfsReachabilityPlanner<'a, R: LfsReachabilityRepository + ?Sized> {
    repository: &'a R,
    fetch_filter: &'a LfsFetchFilter,
    remote_policy: LfsReachabilityRemotePolicy,
    limits: LfsReachabilityLimits,
}

impl<'a, R: LfsReachabilityRepository + ?Sized> LfsReachabilityPlanner<'a, R> {
    pub(crate) fn new(
        repository: &'a R,
        fetch_filter: &'a LfsFetchFilter,
        remote_policy: LfsReachabilityRemotePolicy,
        limits: LfsReachabilityLimits,
    ) -> Result<Self, LfsReachabilityError> {
        Ok(Self {
            repository,
            fetch_filter,
            remote_policy,
            limits: limits.validate()?,
        })
    }

    pub(crate) fn plan_fetch(
        &self,
        selection: &LfsFetchSelection,
    ) -> Result<LfsReachabilityPlan, LfsReachabilityError> {
        match selection {
            LfsFetchSelection::Current => {
                let Some(resolved) = self.repository.current_fetch_revision(self.remote_policy)?
                else {
                    return self.plan_transfer_groups(
                        Vec::new(),
                        Vec::new(),
                        HistoryTraversal::TipsOnly,
                        true,
                        0,
                    );
                };
                self.validate_object_format(resolved.object())?;
                let remote_ref = self.fetch_batch_ref(&resolved)?;
                self.plan_transfer_groups(
                    vec![PendingTransferGroup {
                        remote_ref,
                        include: vec![resolved.object],
                    }],
                    Vec::new(),
                    HistoryTraversal::TipsOnly,
                    true,
                    0,
                )
            }
            LfsFetchSelection::Explicit(selection) => {
                let mut groups = BTreeMap::<Option<String>, Vec<ObjectId>>::new();
                for revision in selection.revisions() {
                    let resolved = self.resolve_required_revision(revision)?;
                    let remote_ref = self.fetch_batch_ref(&resolved)?;
                    groups.entry(remote_ref).or_default().push(resolved.object);
                }
                self.plan_transfer_groups(
                    pending_transfer_groups(groups),
                    Vec::new(),
                    HistoryTraversal::TipsOnly,
                    true,
                    0,
                )
            }
            LfsFetchSelection::Recent(_) | LfsFetchSelection::All => {
                let (roots, traversal) = self.resolve_fetch_roots(selection)?;
                let apply_filter = !matches!(selection, LfsFetchSelection::All);
                self.plan(roots, Vec::new(), traversal, apply_filter, 0)
            }
        }
    }

    fn fetch_batch_ref(
        &self,
        resolved: &LfsResolvedRevision,
    ) -> Result<Option<String>, LfsReachabilityError> {
        resolved
            .reference()
            .map(|reference| self.repository.resolve_fetch_batch_ref(reference))
            .transpose()
            .map(Option::flatten)
    }

    pub(crate) fn plan_push_specs(
        &self,
        specs: &[LfsPushSpec],
    ) -> Result<LfsReachabilityPlan, LfsReachabilityError> {
        if specs.is_empty() {
            return Err(LfsReachabilityError::InvalidRefspec);
        }
        let mut groups = BTreeMap::<Option<String>, Vec<ObjectId>>::new();
        let mut exclude = Vec::new();
        for spec in specs {
            match spec {
                LfsPushSpec::Revision(revision) => {
                    let resolved = self.resolve_required_revision(revision.revision())?;
                    let remote_ref = resolved.reference.clone();
                    if let Some(destination) = remote_ref.as_deref()
                        && let Some(object) =
                            self.repository.resolve_push_destination(destination)?
                    {
                        self.validate_object_format(&object)?;
                        exclude.push(object);
                    }
                    groups.entry(remote_ref).or_default().push(resolved.object);
                }
                LfsPushSpec::Refspec(refspec) => {
                    let resolved = self.resolve_required_revision(refspec.source())?;
                    let destination = canonical_push_destination(&resolved, refspec.destination())?;
                    if let Some(object) = self.repository.resolve_push_destination(&destination)? {
                        self.validate_object_format(&object)?;
                        exclude.push(object);
                    }
                    groups
                        .entry(Some(destination))
                        .or_default()
                        .push(resolved.object);
                }
                LfsPushSpec::Range(range) => {
                    exclude.push(self.resolve_required(range.exclude())?);
                    let resolved = self.resolve_required_revision(range.include())?;
                    groups
                        .entry(resolved.reference.clone())
                        .or_default()
                        .push(resolved.object);
                }
            }
        }
        self.validate_and_deduplicate_ids(&mut exclude)?;
        let groups = pending_transfer_groups(groups);
        self.plan_transfer_groups(groups, exclude, HistoryTraversal::All, false, 0)
    }

    pub(crate) fn plan_pre_push<B: BufRead>(
        &self,
        input: &mut B,
    ) -> Result<LfsReachabilityPlan, LfsReachabilityError> {
        let updates = parse_pre_push_updates(input, self.repository.object_format(), self.limits)?;
        let update_count = updates.len();
        let mut groups = BTreeMap::<Option<String>, Vec<ObjectId>>::new();
        let mut exclude = Vec::new();
        for update in updates {
            match update.kind {
                LfsPrePushUpdateKind::Delete => {}
                LfsPrePushUpdateKind::Create => {
                    if let LfsPrePushObject::Object(object) = update.local {
                        groups
                            .entry(Some(update.remote_ref))
                            .or_default()
                            .push(object);
                    }
                }
                LfsPrePushUpdateKind::Update => {
                    if let LfsPrePushObject::Object(object) = update.local {
                        groups
                            .entry(Some(update.remote_ref))
                            .or_default()
                            .push(object);
                    }
                    if let LfsPrePushObject::Object(object) = update.remote {
                        exclude.push(object);
                    }
                }
            }
        }
        self.validate_and_deduplicate_ids(&mut exclude)?;
        let groups = pending_transfer_groups(groups);
        self.plan_transfer_groups(groups, exclude, HistoryTraversal::All, false, update_count)
    }

    fn resolve_fetch_roots(
        &self,
        selection: &LfsFetchSelection,
    ) -> Result<(Vec<ObjectId>, HistoryTraversal), LfsReachabilityError> {
        let mut roots = Vec::new();
        let mut retained_root_name_bytes = 0;
        match selection {
            LfsFetchSelection::Current => {
                if let Some(resolved) =
                    self.repository.current_fetch_revision(self.remote_policy)?
                {
                    let name = resolved.reference().unwrap_or("HEAD").to_owned();
                    let root = LfsRevisionRoot::new(name, resolved.object)?;
                    account_retained_bytes(
                        &mut retained_root_name_bytes,
                        root.name.len(),
                        self.limits.max_retained_path_bytes,
                    )?;
                    roots.push(root);
                }
                self.finish_discovered_roots(roots, HistoryTraversal::TipsOnly)
            }
            LfsFetchSelection::Explicit(selection) => {
                for revision in selection.revisions() {
                    let object = self.resolve_required(revision)?;
                    account_retained_bytes(
                        &mut retained_root_name_bytes,
                        revision.len(),
                        self.limits.max_retained_path_bytes,
                    )?;
                    roots.push(LfsRevisionRoot::new(revision.clone(), object)?);
                    if roots.len() > self.limits.max_roots {
                        return Err(LfsReachabilityError::TooManyRoots);
                    }
                }
                self.finish_discovered_roots(roots, HistoryTraversal::TipsOnly)
            }
            LfsFetchSelection::Recent(selection) => {
                if let Some(resolved) =
                    self.repository.current_fetch_revision(self.remote_policy)?
                {
                    let name = resolved.reference().unwrap_or("HEAD").to_owned();
                    let root = LfsRevisionRoot::new(name, resolved.object)?;
                    account_retained_bytes(
                        &mut retained_root_name_bytes,
                        root.name.len(),
                        self.limits.max_retained_path_bytes,
                    )?;
                    roots.push(root);
                }
                let mut collector = RootCollector {
                    roots: &mut roots,
                    max_roots: self.limits.max_roots,
                    retained_name_bytes: &mut retained_root_name_bytes,
                    max_retained_name_bytes: self.limits.max_retained_path_bytes,
                };
                self.repository.visit_recent_fetch_roots(
                    selection.reference_cutoff_seconds(),
                    self.remote_policy,
                    &mut collector,
                )?;
                self.finish_discovered_roots(
                    roots,
                    HistoryTraversal::Since(selection.commit_cutoff_seconds()),
                )
            }
            LfsFetchSelection::All => {
                let mut collector = RootCollector {
                    roots: &mut roots,
                    max_roots: self.limits.max_roots,
                    retained_name_bytes: &mut retained_root_name_bytes,
                    max_retained_name_bytes: self.limits.max_retained_path_bytes,
                };
                self.repository
                    .visit_all_fetch_roots(self.remote_policy, &mut collector)?;
                self.finish_discovered_roots(roots, HistoryTraversal::All)
            }
        }
    }

    fn finish_discovered_roots(
        &self,
        mut roots: Vec<LfsRevisionRoot>,
        traversal: HistoryTraversal,
    ) -> Result<(Vec<ObjectId>, HistoryTraversal), LfsReachabilityError> {
        if roots.len() > self.limits.max_roots {
            return Err(LfsReachabilityError::TooManyRoots);
        }
        for root in &roots {
            self.validate_object_format(root.object())?;
        }
        roots.sort_unstable_by(compare_revision_roots);
        roots.dedup_by(|left, right| left.object == right.object);
        Ok((
            roots.into_iter().map(|root| root.object).collect(),
            traversal,
        ))
    }

    fn resolve_required_revision(
        &self,
        revision: &str,
    ) -> Result<LfsResolvedRevision, LfsReachabilityError> {
        validate_revision(revision)?;
        let resolved = self
            .repository
            .resolve_revision(revision)?
            .ok_or(LfsReachabilityError::RevisionNotFound)?;
        self.validate_object_format(resolved.object())?;
        Ok(resolved)
    }

    fn resolve_required(&self, revision: &str) -> Result<ObjectId, LfsReachabilityError> {
        self.resolve_required_revision(revision)
            .map(|resolved| resolved.object)
    }

    fn plan_transfer_groups(
        &self,
        groups: Vec<PendingTransferGroup>,
        exclude: Vec<ObjectId>,
        traversal: HistoryTraversal,
        apply_filter: bool,
        pre_push_updates: usize,
    ) -> Result<LfsReachabilityPlan, LfsReachabilityError> {
        let root_count = groups
            .iter()
            .try_fold(exclude.len(), |count, group| {
                count.checked_add(group.include.len())
            })
            .ok_or(LfsReachabilityError::TooManyRoots)?;
        if root_count > self.limits.max_roots {
            return Err(LfsReachabilityError::TooManyRoots);
        }

        let mut pointers = HashMap::<[u8; 32], LfsReachablePointer>::new();
        let mut claimed = HashSet::<[u8; 32]>::new();
        let mut planned_groups = Vec::new();
        let mut stats = LfsReachabilityStats {
            roots: root_count,
            pre_push_updates,
            ..LfsReachabilityStats::default()
        };
        let mut retained_path_bytes = 0usize;

        for group in groups {
            let group_plan =
                self.plan(group.include, exclude.clone(), traversal, apply_filter, 0)?;
            merge_transfer_stats(&mut stats, group_plan.stats());
            if stats.graph_objects > self.limits.max_graph_objects {
                return Err(LfsReachabilityError::TooManyGraphObjects);
            }

            let mut pointer_oids = Vec::new();
            for pointer in group_plan.pointers {
                let oid = pointer.oid().bytes();
                if let Some(existing) = pointers.get_mut(&oid) {
                    if existing.pointer != pointer.pointer {
                        return Err(LfsReachabilityError::ConflictingPointer);
                    }
                    stats.deduplicated_pointers = stats.deduplicated_pointers.saturating_add(1);
                    if compare_path_blob(&pointer.repository_path, &pointer.git_blob, existing)
                        .is_lt()
                    {
                        retained_path_bytes = retained_path_bytes
                            .checked_sub(existing.repository_path.len())
                            .expect("retained grouped LFS path accounting remains balanced");
                        retained_path_bytes = retained_path_bytes
                            .checked_add(pointer.repository_path.len())
                            .ok_or(LfsReachabilityError::RetainedPathBytesExceeded)?;
                        *existing = pointer;
                    }
                } else {
                    if pointers.len() >= self.limits.max_pointers {
                        return Err(LfsReachabilityError::TooManyPointers);
                    }
                    retained_path_bytes = retained_path_bytes
                        .checked_add(pointer.repository_path.len())
                        .ok_or(LfsReachabilityError::RetainedPathBytesExceeded)?;
                    if retained_path_bytes > self.limits.max_retained_path_bytes {
                        return Err(LfsReachabilityError::RetainedPathBytesExceeded);
                    }
                    pointers.insert(oid, pointer);
                }
                if claimed.insert(oid) {
                    pointer_oids.push(oid);
                }
            }
            pointer_oids.sort_unstable();
            planned_groups.push(PlannedTransferGroup {
                remote_ref: group.remote_ref,
                pointer_oids,
            });
        }

        let mut pointers = pointers.into_values().collect::<Vec<_>>();
        pointers.sort_unstable_by(compare_reachable_pointers);
        let pointer_indexes = pointers
            .iter()
            .enumerate()
            .map(|(index, pointer)| (pointer.oid().bytes(), index))
            .collect::<HashMap<_, _>>();
        let transfer_groups = planned_groups
            .into_iter()
            .map(|group| LfsReachabilityTransferGroup {
                remote_ref: group.remote_ref,
                pointer_indexes: group
                    .pointer_oids
                    .into_iter()
                    .map(|oid| {
                        *pointer_indexes
                            .get(&oid)
                            .expect("grouped LFS pointer remains in final plan")
                    })
                    .collect(),
            })
            .collect();
        stats.pointers = pointers.len();
        stats.retained_path_bytes = retained_path_bytes;
        stats.max_retained_path_bytes = stats.max_retained_path_bytes.max(retained_path_bytes);
        Ok(LfsReachabilityPlan {
            pointers,
            transfer_groups,
            stats,
        })
    }

    fn validate_and_deduplicate_ids(
        &self,
        objects: &mut Vec<ObjectId>,
    ) -> Result<(), LfsReachabilityError> {
        if objects.len() > self.limits.max_roots {
            return Err(LfsReachabilityError::TooManyRoots);
        }
        for object in objects.iter() {
            self.validate_object_format(object)?;
        }
        objects.sort_unstable_by(compare_object_ids);
        objects.dedup();
        Ok(())
    }

    fn validate_object_format(&self, object: &ObjectId) -> Result<(), LfsReachabilityError> {
        if object.algorithm() != self.repository.object_format() {
            return Err(LfsReachabilityError::ObjectFormatMismatch);
        }
        Ok(())
    }

    fn plan(
        &self,
        mut include: Vec<ObjectId>,
        mut exclude: Vec<ObjectId>,
        traversal: HistoryTraversal,
        apply_filter: bool,
        pre_push_updates: usize,
    ) -> Result<LfsReachabilityPlan, LfsReachabilityError> {
        self.validate_and_deduplicate_ids(&mut include)?;
        self.validate_and_deduplicate_ids(&mut exclude)?;
        let mut state = TraversalState::new(
            include.len().saturating_add(exclude.len()),
            pre_push_updates,
        );
        let mut excluded = self.collect_excluded_graph(&exclude, &mut state)?;
        self.scan_trees(&mut excluded.trees, false, &mut state)?;
        let excluded_results = std::mem::take(&mut state.pointers);
        for (oid, reachable) in excluded_results {
            release_retained_bytes(
                &mut state.retained_path_bytes,
                reachable.repository_path.len(),
            );
            state.excluded_pointers.insert(oid, reachable.pointer);
        }
        state.stats.retained_path_bytes = state.retained_path_bytes;
        state.stats.deduplicated_pointers = 0;
        let mut tree_tasks =
            self.collect_included_trees(&include, &excluded.commits, traversal, &mut state)?;
        self.scan_trees(&mut tree_tasks, apply_filter, &mut state)?;

        let mut pointers: Vec<_> = state.pointers.into_values().collect();
        pointers.sort_unstable_by(compare_reachable_pointers);
        state.stats.pointers = pointers.len();
        state.stats.retained_path_bytes = state.retained_path_bytes;
        Ok(LfsReachabilityPlan {
            pointers,
            transfer_groups: Vec::new(),
            stats: state.stats,
        })
    }

    fn collect_excluded_graph(
        &self,
        roots: &[ObjectId],
        state: &mut TraversalState,
    ) -> Result<ExcludedGraph, LfsReachabilityError> {
        let mut excluded = HashSet::new();
        let mut trees = VecDeque::new();
        let mut seen = HashMap::new();
        let mut pending = VecDeque::new();
        for root in roots {
            pending.push_back(GraphTask {
                object: root.clone(),
                depth: 0,
                is_root: true,
                expected_kind: None,
            });
        }
        while let Some(task) = pending.pop_front() {
            state.stats.max_pending_graph = state.stats.max_pending_graph.max(pending.len() + 1);
            if let Some(kind) = seen.get(&task.object) {
                validate_expected_kind(task.expected_kind, *kind)?;
                continue;
            }
            self.check_history_depth(task.depth)?;
            let object = self.read_structured_object(&task.object, state)?;
            validate_expected_kind(task.expected_kind, object.kind)?;
            seen.insert(task.object.clone(), object.kind);
            match object.kind {
                GitObjectKind::Commit => {
                    excluded.insert(task.object);
                    let links =
                        decode_commit_links(self.repository.object_format(), &object.content)
                            .map_err(LfsReachabilityError::invalid_object)?;
                    if trees.len() >= self.limits.max_graph_objects {
                        return Err(LfsReachabilityError::TooManyGraphObjects);
                    }
                    trees.push_back(TreeTask {
                        object: links.tree,
                        path: Vec::new(),
                        depth: 0,
                    });
                    for parent in links.parents {
                        if pending.len() >= self.limits.max_graph_objects {
                            return Err(LfsReachabilityError::TooManyGraphObjects);
                        }
                        pending.push_back(GraphTask {
                            object: parent,
                            depth: task.depth.saturating_add(1),
                            is_root: false,
                            expected_kind: Some(GitObjectKind::Commit),
                        });
                    }
                }
                GitObjectKind::Tag => {
                    let tag = decode_tag(self.repository.object_format(), &object.content)
                        .map_err(LfsReachabilityError::invalid_object)?;
                    if pending.len() >= self.limits.max_graph_objects {
                        return Err(LfsReachabilityError::TooManyGraphObjects);
                    }
                    pending.push_front(GraphTask {
                        object: tag.target,
                        depth: task.depth.saturating_add(1),
                        is_root: task.is_root,
                        expected_kind: Some(tag.target_kind),
                    });
                }
                GitObjectKind::Tree if task.is_root => {
                    if trees.len() >= self.limits.max_graph_objects {
                        return Err(LfsReachabilityError::TooManyGraphObjects);
                    }
                    trees.push_back(TreeTask {
                        object: task.object,
                        path: Vec::new(),
                        depth: 0,
                    });
                }
                GitObjectKind::Tree | GitObjectKind::Blob => {
                    return Err(LfsReachabilityError::UnsupportedRootKind(object.kind));
                }
            }
        }
        Ok(ExcludedGraph {
            commits: excluded,
            trees,
        })
    }

    fn collect_included_trees(
        &self,
        roots: &[ObjectId],
        excluded_commits: &HashSet<ObjectId>,
        traversal: HistoryTraversal,
        state: &mut TraversalState,
    ) -> Result<VecDeque<TreeTask>, LfsReachabilityError> {
        let mut pending = VecDeque::new();
        let mut seen = HashMap::new();
        let mut trees = VecDeque::new();
        for root in roots {
            pending.push_back(GraphTask {
                object: root.clone(),
                depth: 0,
                is_root: true,
                expected_kind: None,
            });
        }
        while let Some(task) = pending.pop_front() {
            state.stats.max_pending_graph = state.stats.max_pending_graph.max(pending.len() + 1);
            if excluded_commits.contains(&task.object) {
                validate_expected_kind(task.expected_kind, GitObjectKind::Commit)?;
                continue;
            }
            if let Some(kind) = seen.get(&task.object) {
                validate_expected_kind(task.expected_kind, *kind)?;
                continue;
            }
            self.check_history_depth(task.depth)?;
            let object = self.read_structured_object(&task.object, state)?;
            validate_expected_kind(task.expected_kind, object.kind)?;
            seen.insert(task.object.clone(), object.kind);
            match object.kind {
                GitObjectKind::Commit => {
                    state.stats.commits = state.stats.commits.saturating_add(1);
                    let links =
                        decode_commit_links(self.repository.object_format(), &object.content)
                            .map_err(LfsReachabilityError::invalid_object)?;
                    let scan_commit = match traversal {
                        HistoryTraversal::TipsOnly | HistoryTraversal::All => true,
                        HistoryTraversal::Since(cutoff) => {
                            task.is_root || parse_committer_timestamp(&object.content)? >= cutoff
                        }
                    };
                    if !scan_commit {
                        continue;
                    }
                    if trees.len() >= self.limits.max_graph_objects {
                        return Err(LfsReachabilityError::TooManyGraphObjects);
                    }
                    trees.push_back(TreeTask {
                        object: links.tree,
                        path: Vec::new(),
                        depth: 0,
                    });
                    if !matches!(traversal, HistoryTraversal::TipsOnly) {
                        for parent in links.parents {
                            if pending.len() >= self.limits.max_graph_objects {
                                return Err(LfsReachabilityError::TooManyGraphObjects);
                            }
                            pending.push_back(GraphTask {
                                object: parent,
                                depth: task.depth.saturating_add(1),
                                is_root: false,
                                expected_kind: Some(GitObjectKind::Commit),
                            });
                        }
                    }
                }
                GitObjectKind::Tag => {
                    state.stats.tags = state.stats.tags.saturating_add(1);
                    let tag = decode_tag(self.repository.object_format(), &object.content)
                        .map_err(LfsReachabilityError::invalid_object)?;
                    if pending.len() >= self.limits.max_graph_objects {
                        return Err(LfsReachabilityError::TooManyGraphObjects);
                    }
                    pending.push_front(GraphTask {
                        object: tag.target,
                        depth: task.depth.saturating_add(1),
                        is_root: task.is_root,
                        expected_kind: Some(tag.target_kind),
                    });
                }
                GitObjectKind::Tree if task.is_root => {
                    if trees.len() >= self.limits.max_graph_objects {
                        return Err(LfsReachabilityError::TooManyGraphObjects);
                    }
                    trees.push_back(TreeTask {
                        object: task.object,
                        path: Vec::new(),
                        depth: 0,
                    });
                }
                GitObjectKind::Tree | GitObjectKind::Blob => {
                    return Err(LfsReachabilityError::UnsupportedRootKind(object.kind));
                }
            }
        }
        state.stats.max_pending_trees = state.stats.max_pending_trees.max(trees.len());
        Ok(trees)
    }

    fn scan_trees(
        &self,
        pending: &mut VecDeque<TreeTask>,
        apply_filter: bool,
        state: &mut TraversalState,
    ) -> Result<(), LfsReachabilityError> {
        while let Some(task) = pending.pop_front() {
            release_retained_bytes(&mut state.retained_path_bytes, task.path.len());
            state.stats.max_pending_trees = state.stats.max_pending_trees.max(pending.len() + 1);
            self.check_tree_depth(task.depth)?;
            let key = TreeContextKey {
                object: task.object.clone(),
                path: task.path.clone(),
            };
            if !state.seen_tree_contexts.contains(&key)
                && state.seen_tree_contexts.len() >= self.limits.max_graph_objects
            {
                return Err(LfsReachabilityError::TooManyGraphObjects);
            }
            if !state.seen_tree_contexts.contains(&key) {
                account_retained_bytes(
                    &mut state.retained_path_bytes,
                    task.path.len(),
                    self.limits.max_retained_path_bytes,
                )?;
                state.stats.max_retained_path_bytes = state
                    .stats
                    .max_retained_path_bytes
                    .max(state.retained_path_bytes);
            }
            if !state.seen_tree_contexts.insert(key) {
                continue;
            }
            state.stats.tree_contexts = state.stats.tree_contexts.saturating_add(1);
            state.stats.max_tree_depth = state.stats.max_tree_depth.max(task.depth);
            let object = self.read_structured_object(&task.object, state)?;
            if object.kind != GitObjectKind::Tree {
                return Err(LfsReachabilityError::InvalidTreeEntryKind);
            }
            let mut cursor = 0;
            while let Some(entry) = decode_tree_entry_ref(
                self.repository.object_format(),
                &object.content,
                &mut cursor,
            )
            .map_err(LfsReachabilityError::invalid_object)?
            {
                let path =
                    join_repository_path(&task.path, entry.name, self.limits.max_path_bytes)?;
                state.stats.max_path_bytes = state.stats.max_path_bytes.max(path.len());
                match entry.mode {
                    TreeMode::Tree => {
                        if pending.len() >= self.limits.max_graph_objects {
                            return Err(LfsReachabilityError::TooManyGraphObjects);
                        }
                        account_retained_bytes(
                            &mut state.retained_path_bytes,
                            path.len(),
                            self.limits.max_retained_path_bytes,
                        )?;
                        state.stats.max_retained_path_bytes = state
                            .stats
                            .max_retained_path_bytes
                            .max(state.retained_path_bytes);
                        pending.push_back(TreeTask {
                            object: entry.id,
                            path,
                            depth: task.depth.saturating_add(1),
                        });
                    }
                    TreeMode::File | TreeMode::Executable => {
                        if !apply_filter || self.fetch_filter.allows_bytes(&path) {
                            self.inspect_blob(entry.id, path, state)?;
                        }
                    }
                    TreeMode::Symlink | TreeMode::Gitlink => {}
                }
            }
        }
        Ok(())
    }

    fn inspect_blob(
        &self,
        object: ObjectId,
        repository_path: Vec<u8>,
        state: &mut TraversalState,
    ) -> Result<(), LfsReachabilityError> {
        if let Some(cached_oid) = state.blob_cache.get(&object).copied() {
            if let Some(cached_oid) = cached_oid {
                let pointer = state
                    .pointers
                    .get(&cached_oid)
                    .map(|reachable| reachable.pointer.clone())
                    .or_else(|| state.excluded_pointers.get(&cached_oid).cloned())
                    .expect("cached Git LFS blob retains one canonical pointer owner");
                self.insert_pointer(pointer, repository_path, object, state)?;
            }
            return Ok(());
        }

        self.count_object(&object, state)?;
        state.stats.unique_blobs = state.stats.unique_blobs.saturating_add(1);
        if let Some((kind, size)) = self
            .repository
            .object_header_hint(&object)
            .map_err(LfsReachabilityError::io)?
        {
            if kind != GitObjectKind::Blob {
                return Err(LfsReachabilityError::InvalidTreeEntryKind);
            }
            if size >= LFS_POINTER_MAX_BYTES {
                state.blob_cache.insert(object, None);
                return Ok(());
            }
        }

        let prefix = self
            .repository
            .read_lfs_object_bounded(&object, LFS_POINTER_MAX_BYTES)
            .map_err(LfsReachabilityError::io)?;
        if prefix.object.kind != GitObjectKind::Blob {
            return Err(LfsReachabilityError::InvalidTreeEntryKind);
        }
        state.stats.blob_prefix_reads = state.stats.blob_prefix_reads.saturating_add(1);
        state.stats.blob_prefix_bytes = state
            .stats
            .blob_prefix_bytes
            .saturating_add(prefix.object.content.len());
        if !prefix.is_complete || prefix.object.content.len() >= LFS_POINTER_MAX_BYTES {
            state.blob_cache.insert(object, None);
            return Ok(());
        }
        let pointer = match LfsPointer::parse_current(&prefix.object.content) {
            Ok(pointer) => Some(canonicalize_pointer(pointer)?),
            Err(_) => None,
        };
        state.blob_cache.insert(
            object.clone(),
            pointer.as_ref().map(|pointer| pointer.oid().bytes()),
        );
        if let Some(pointer) = pointer {
            self.insert_pointer(pointer, repository_path, object, state)?;
        }
        Ok(())
    }

    fn insert_pointer(
        &self,
        pointer: LfsPointer,
        repository_path: Vec<u8>,
        git_blob: ObjectId,
        state: &mut TraversalState,
    ) -> Result<(), LfsReachabilityError> {
        let key = pointer.oid().bytes();
        if let Some(excluded) = state.excluded_pointers.get(&key) {
            if excluded != &pointer {
                return Err(LfsReachabilityError::ConflictingPointer);
            }
            state.stats.excluded_pointers = state.stats.excluded_pointers.saturating_add(1);
            return Ok(());
        }
        if let Some(existing) = state.pointers.get(&key) {
            if existing.pointer != pointer {
                return Err(LfsReachabilityError::ConflictingPointer);
            }
            state.stats.deduplicated_pointers = state.stats.deduplicated_pointers.saturating_add(1);
            if compare_path_blob(&repository_path, &git_blob, existing).is_lt() {
                let old_path_len = existing.repository_path.len();
                release_retained_bytes(&mut state.retained_path_bytes, old_path_len);
                account_retained_bytes(
                    &mut state.retained_path_bytes,
                    repository_path.len(),
                    self.limits.max_retained_path_bytes,
                )?;
                state.stats.max_retained_path_bytes = state
                    .stats
                    .max_retained_path_bytes
                    .max(state.retained_path_bytes);
                let existing = state
                    .pointers
                    .get_mut(&key)
                    .expect("existing LFS pointer remains present");
                existing.repository_path = repository_path;
                existing.git_blob = git_blob;
            }
            state.stats.retained_path_bytes = state.retained_path_bytes;
            return Ok(());
        }
        if state
            .pointers
            .len()
            .saturating_add(state.excluded_pointers.len())
            >= self.limits.max_pointers
        {
            return Err(LfsReachabilityError::TooManyPointers);
        }
        account_retained_bytes(
            &mut state.retained_path_bytes,
            repository_path.len(),
            self.limits.max_retained_path_bytes,
        )?;
        state.stats.max_retained_path_bytes = state
            .stats
            .max_retained_path_bytes
            .max(state.retained_path_bytes);
        state.stats.retained_path_bytes = state.retained_path_bytes;
        state.pointers.insert(
            key,
            LfsReachablePointer {
                pointer,
                repository_path,
                git_blob,
            },
        );
        Ok(())
    }

    fn read_structured_object(
        &self,
        object: &ObjectId,
        state: &mut TraversalState,
    ) -> Result<zmin_git_core::LooseObject, LfsReachabilityError> {
        self.count_object(object, state)?;
        let max_bytes = self
            .limits
            .max_structured_object_bytes
            .checked_add(1)
            .ok_or(LfsReachabilityError::InvalidLimits)?;
        let value = self
            .repository
            .read_lfs_object_bounded(object, max_bytes)
            .map_err(LfsReachabilityError::io)?;
        if !value.is_complete
            || value.object.content.len() > self.limits.max_structured_object_bytes
        {
            return Err(LfsReachabilityError::ObjectTooLarge(value.object.kind));
        }
        Ok(value.object)
    }

    fn count_object(
        &self,
        object: &ObjectId,
        state: &mut TraversalState,
    ) -> Result<(), LfsReachabilityError> {
        self.validate_object_format(object)?;
        if state.counted_objects.insert(object.clone()) {
            if state.counted_objects.len() > self.limits.max_graph_objects {
                return Err(LfsReachabilityError::TooManyGraphObjects);
            }
            state.stats.graph_objects = state.counted_objects.len();
        }
        Ok(())
    }

    fn check_history_depth(&self, depth: usize) -> Result<(), LfsReachabilityError> {
        if depth > self.limits.max_history_depth {
            return Err(LfsReachabilityError::HistoryTooDeep);
        }
        Ok(())
    }

    fn check_tree_depth(&self, depth: usize) -> Result<(), LfsReachabilityError> {
        if depth > self.limits.max_tree_depth {
            return Err(LfsReachabilityError::TreeTooDeep);
        }
        Ok(())
    }
}

/// Parses bounded pre-push standard input without ever growing an unbounded line.
pub(crate) fn parse_pre_push_updates<B: BufRead>(
    input: &mut B,
    algorithm: GitHashAlgorithm,
    limits: LfsReachabilityLimits,
) -> Result<Vec<LfsPrePushUpdate>, LfsReachabilityError> {
    let limits = limits.validate()?;
    let mut bytes_read = 0;
    let mut updates = Vec::new();
    while let Some(line) = read_bounded_line(input, &mut bytes_read, limits)? {
        if updates.len() >= limits.max_pre_push_updates {
            return Err(LfsReachabilityError::TooManyPrePushUpdates);
        }
        updates.push(parse_pre_push_line(&line, algorithm)?);
    }
    Ok(updates)
}

fn read_bounded_line<B: BufRead>(
    input: &mut B,
    bytes_read: &mut usize,
    limits: LfsReachabilityLimits,
) -> Result<Option<Vec<u8>>, LfsReachabilityError> {
    let mut line = Vec::new();
    loop {
        let available = input.fill_buf().map_err(LfsReachabilityError::io)?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        *bytes_read = bytes_read
            .checked_add(take)
            .ok_or(LfsReachabilityError::PrePushInputTooLarge)?;
        if *bytes_read > limits.max_pre_push_bytes {
            return Err(LfsReachabilityError::PrePushInputTooLarge);
        }
        let content_take = if newline.is_some() { take - 1 } else { take };
        if line.len().saturating_add(content_take) > limits.max_pre_push_line_bytes {
            return Err(LfsReachabilityError::PrePushLineTooLong);
        }
        line.extend_from_slice(&available[..content_take]);
        input.consume(take);
        if newline.is_some() {
            return Ok(Some(line));
        }
    }
}

fn parse_pre_push_line(
    line: &[u8],
    algorithm: GitHashAlgorithm,
) -> Result<LfsPrePushUpdate, LfsReachabilityError> {
    if line.is_empty() || line.contains(&b'\r') || line.contains(&b'\t') {
        return Err(LfsReachabilityError::InvalidPrePushLine);
    }
    let fields: Vec<_> = line.split(|byte| *byte == b' ').collect();
    if fields.len() != 4 || fields.iter().any(|field| field.is_empty()) {
        return Err(LfsReachabilityError::InvalidPrePushLine);
    }
    let local_ref = parse_pre_push_local_selector(fields[0])?;
    let local = parse_pre_push_object(fields[1], algorithm)?;
    let remote_ref = parse_pre_push_remote_ref(fields[2])?;
    let remote = parse_pre_push_object(fields[3], algorithm)?;
    let kind = match (&local, &remote) {
        (LfsPrePushObject::Zero, LfsPrePushObject::Zero) => {
            return Err(LfsReachabilityError::InvalidPrePushLine);
        }
        (LfsPrePushObject::Zero, LfsPrePushObject::Object(_)) => LfsPrePushUpdateKind::Delete,
        (LfsPrePushObject::Object(_), LfsPrePushObject::Zero) => LfsPrePushUpdateKind::Create,
        (LfsPrePushObject::Object(_), LfsPrePushObject::Object(_)) => LfsPrePushUpdateKind::Update,
    };
    if (local_ref.as_slice() == b"(delete)") != matches!(kind, LfsPrePushUpdateKind::Delete) {
        return Err(LfsReachabilityError::InvalidPrePushLine);
    }
    Ok(LfsPrePushUpdate {
        local_ref,
        local,
        remote_ref,
        remote,
        kind,
    })
}

fn parse_pre_push_local_selector(bytes: &[u8]) -> Result<Vec<u8>, LfsReachabilityError> {
    if bytes.is_empty()
        || bytes.len() > MAX_REVISION_BYTES
        || bytes
            .iter()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(LfsReachabilityError::InvalidPrePushLine);
    }
    Ok(bytes.to_vec())
}

fn parse_pre_push_remote_ref(bytes: &[u8]) -> Result<String, LfsReachabilityError> {
    let value = std::str::from_utf8(bytes).map_err(|_| LfsReachabilityError::InvalidPrePushLine)?;
    if value.len() > MAX_REVISION_BYTES
        || !value.starts_with("refs/")
        || !check_ref_format(value, false)
    {
        return Err(LfsReachabilityError::InvalidPrePushLine);
    }
    Ok(value.to_owned())
}

fn parse_pre_push_object(
    bytes: &[u8],
    algorithm: GitHashAlgorithm,
) -> Result<LfsPrePushObject, LfsReachabilityError> {
    let expected = algorithm.digest_len() * 2;
    if bytes.len() != expected {
        return Err(LfsReachabilityError::InvalidPrePushLine);
    }
    if bytes.iter().all(|byte| *byte == b'0') {
        return Ok(LfsPrePushObject::Zero);
    }
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(LfsReachabilityError::InvalidPrePushLine);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| LfsReachabilityError::InvalidPrePushLine)?;
    let object = ObjectId::from_hex(algorithm, text)
        .map_err(|_| LfsReachabilityError::InvalidPrePushLine)?;
    Ok(LfsPrePushObject::Object(object))
}

fn validate_revision(value: &str) -> Result<(), LfsReachabilityError> {
    if value.is_empty()
        || value.len() > MAX_REVISION_BYTES
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace() || byte == 0)
    {
        return Err(LfsReachabilityError::InvalidRevision);
    }
    Ok(())
}

fn validate_remote_ref(value: &str) -> Result<(), LfsReachabilityError> {
    if value.len() > MAX_REVISION_BYTES
        || !value.starts_with("refs/")
        || !check_ref_format(value, false)
    {
        return Err(LfsReachabilityError::InvalidRefspec);
    }
    Ok(())
}

fn canonical_push_destination(
    source: &LfsResolvedRevision,
    destination: &str,
) -> Result<String, LfsReachabilityError> {
    let destination = if destination.starts_with("refs/") {
        destination.to_owned()
    } else if let Some(source_ref) = source.reference() {
        if source_ref.starts_with("refs/heads/") {
            format!("refs/heads/{destination}")
        } else if source_ref.starts_with("refs/tags/") {
            format!("refs/tags/{destination}")
        } else {
            return Err(LfsReachabilityError::InvalidRefspec);
        }
    } else {
        return Err(LfsReachabilityError::InvalidRefspec);
    };
    validate_remote_ref(&destination)?;
    Ok(destination)
}

fn pending_transfer_groups(
    groups: BTreeMap<Option<String>, Vec<ObjectId>>,
) -> Vec<PendingTransferGroup> {
    groups
        .into_iter()
        .map(|(remote_ref, mut include)| {
            include.sort_unstable_by(compare_object_ids);
            include.dedup();
            PendingTransferGroup {
                remote_ref,
                include,
            }
        })
        .collect()
}

fn merge_transfer_stats(target: &mut LfsReachabilityStats, source: LfsReachabilityStats) {
    target.graph_objects = target.graph_objects.saturating_add(source.graph_objects);
    target.commits = target.commits.saturating_add(source.commits);
    target.tags = target.tags.saturating_add(source.tags);
    target.tree_contexts = target.tree_contexts.saturating_add(source.tree_contexts);
    target.unique_blobs = target.unique_blobs.saturating_add(source.unique_blobs);
    target.blob_prefix_reads = target
        .blob_prefix_reads
        .saturating_add(source.blob_prefix_reads);
    target.blob_prefix_bytes = target
        .blob_prefix_bytes
        .saturating_add(source.blob_prefix_bytes);
    target.deduplicated_pointers = target
        .deduplicated_pointers
        .saturating_add(source.deduplicated_pointers);
    target.excluded_pointers = target
        .excluded_pointers
        .saturating_add(source.excluded_pointers);
    target.max_pending_graph = target.max_pending_graph.max(source.max_pending_graph);
    target.max_pending_trees = target.max_pending_trees.max(source.max_pending_trees);
    target.max_tree_depth = target.max_tree_depth.max(source.max_tree_depth);
    target.max_path_bytes = target.max_path_bytes.max(source.max_path_bytes);
    target.max_retained_path_bytes = target
        .max_retained_path_bytes
        .max(source.max_retained_path_bytes);
}

fn join_repository_path(
    prefix: &[u8],
    name: &[u8],
    max_bytes: usize,
) -> Result<Vec<u8>, LfsReachabilityError> {
    if name.is_empty() || name.contains(&b'/') || name.contains(&0) {
        return Err(LfsReachabilityError::InvalidRepositoryPath);
    }
    let separator = usize::from(!prefix.is_empty());
    let length = prefix
        .len()
        .checked_add(separator)
        .and_then(|length| length.checked_add(name.len()))
        .ok_or(LfsReachabilityError::PathTooLong)?;
    if length > max_bytes {
        return Err(LfsReachabilityError::PathTooLong);
    }
    let mut path = Vec::with_capacity(length);
    path.extend_from_slice(prefix);
    if separator != 0 {
        path.push(b'/');
    }
    path.extend_from_slice(name);
    Ok(path)
}

fn account_retained_bytes(
    retained: &mut usize,
    added: usize,
    maximum: usize,
) -> Result<(), LfsReachabilityError> {
    *retained = retained
        .checked_add(added)
        .ok_or(LfsReachabilityError::RetainedPathBytesExceeded)?;
    if *retained > maximum {
        return Err(LfsReachabilityError::RetainedPathBytesExceeded);
    }
    Ok(())
}

fn release_retained_bytes(retained: &mut usize, released: usize) {
    *retained = retained
        .checked_sub(released)
        .expect("retained LFS path byte accounting remains balanced");
}

fn canonicalize_pointer(pointer: LfsPointer) -> Result<LfsPointer, LfsReachabilityError> {
    let bytes = pointer
        .serialize_canonical()
        .map_err(LfsReachabilityError::NonCanonicalPointer)?;
    LfsPointer::parse_strict(&bytes).map_err(LfsReachabilityError::NonCanonicalPointer)
}

fn parse_committer_timestamp(bytes: &[u8]) -> Result<i64, LfsReachabilityError> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .ok_or(LfsReachabilityError::InvalidCommitTimestamp)?;
    for line in bytes[..header_end].split(|byte| *byte == b'\n') {
        let Some(value) = line.strip_prefix(b"committer ") else {
            continue;
        };
        let marker = value
            .windows(2)
            .rposition(|window| window == b"> ")
            .ok_or(LfsReachabilityError::InvalidCommitTimestamp)?;
        let timestamp = value[marker + 2..]
            .split(|byte| *byte == b' ')
            .next()
            .ok_or(LfsReachabilityError::InvalidCommitTimestamp)?;
        let timestamp = std::str::from_utf8(timestamp)
            .map_err(|_| LfsReachabilityError::InvalidCommitTimestamp)?;
        return timestamp
            .parse::<i64>()
            .map_err(|_| LfsReachabilityError::InvalidCommitTimestamp);
    }
    Err(LfsReachabilityError::InvalidCommitTimestamp)
}

fn validate_expected_kind(
    expected: Option<GitObjectKind>,
    actual: GitObjectKind,
) -> Result<(), LfsReachabilityError> {
    if expected.is_some_and(|expected| expected != actual) {
        return Err(LfsReachabilityError::InvalidObjectKind);
    }
    Ok(())
}

fn compare_object_ids(left: &ObjectId, right: &ObjectId) -> std::cmp::Ordering {
    left.as_bytes().cmp(right.as_bytes())
}

fn compare_revision_roots(left: &LfsRevisionRoot, right: &LfsRevisionRoot) -> std::cmp::Ordering {
    compare_object_ids(&left.object, &right.object).then_with(|| left.name.cmp(&right.name))
}

fn compare_reachable_pointers(
    left: &LfsReachablePointer,
    right: &LfsReachablePointer,
) -> std::cmp::Ordering {
    left.pointer
        .oid()
        .bytes()
        .cmp(&right.pointer.oid().bytes())
        .then_with(|| left.repository_path.cmp(&right.repository_path))
        .then_with(|| compare_object_ids(&left.git_blob, &right.git_blob))
}

fn compare_path_blob(
    path: &[u8],
    blob: &ObjectId,
    existing: &LfsReachablePointer,
) -> std::cmp::Ordering {
    path.cmp(existing.repository_path.as_slice())
        .then_with(|| compare_object_ids(blob, &existing.git_blob))
}

/// Sanitized planner failures; raw revision names, paths and object contents are
/// intentionally never retained or formatted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LfsReachabilityError {
    InvalidLimits,
    InvalidRevision,
    UnsupportedRevision,
    AmbiguousRevision,
    RevisionNotFound,
    InvalidRefspec,
    InvalidRemoteFetchRefspec,
    AmbiguousFetchBatchRef,
    AmbiguousPushDestination,
    UnsupportedFetchSelection,
    InvalidPrePushLine,
    PrePushInputTooLarge,
    PrePushLineTooLong,
    TooManyPrePushUpdates,
    TooManyRoots,
    TooManyGraphObjects,
    TooManyPointers,
    HistoryTooDeep,
    TreeTooDeep,
    PathTooLong,
    RetainedPathBytesExceeded,
    InvalidRepositoryPath,
    ObjectFormatMismatch,
    UnsupportedRootKind(GitObjectKind),
    ObjectTooLarge(GitObjectKind),
    InvalidObject(io::ErrorKind),
    InvalidObjectKind,
    InvalidTreeEntryKind,
    InvalidCommitTimestamp,
    NonCanonicalPointer(LfsPointerError),
    ConflictingPointer,
    Io(io::ErrorKind),
}

impl LfsReachabilityError {
    fn io(error: io::Error) -> Self {
        Self::Io(error.kind())
    }

    fn invalid_object(error: io::Error) -> Self {
        Self::InvalidObject(error.kind())
    }
}

impl fmt::Display for LfsReachabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidLimits => "invalid LFS reachability limits",
            Self::InvalidRevision => "invalid LFS revision",
            Self::UnsupportedRevision => "unsupported LFS revision expression",
            Self::AmbiguousRevision => "ambiguous LFS revision",
            Self::RevisionNotFound => "LFS revision was not found",
            Self::InvalidRefspec => "invalid LFS push refspec",
            Self::InvalidRemoteFetchRefspec => "invalid remote fetch refspec",
            Self::AmbiguousFetchBatchRef => "ambiguous remote fetch Batch ref",
            Self::AmbiguousPushDestination => "ambiguous remote push destination",
            Self::UnsupportedFetchSelection => "unsupported LFS fetch selection",
            Self::InvalidPrePushLine => "invalid LFS pre-push update",
            Self::PrePushInputTooLarge => "LFS pre-push input is too large",
            Self::PrePushLineTooLong => "LFS pre-push line is too long",
            Self::TooManyPrePushUpdates => "too many LFS pre-push updates",
            Self::TooManyRoots => "too many LFS reachability roots",
            Self::TooManyGraphObjects => "LFS reachability object limit exceeded",
            Self::TooManyPointers => "LFS pointer limit exceeded",
            Self::HistoryTooDeep => "LFS history depth limit exceeded",
            Self::TreeTooDeep => "LFS tree depth limit exceeded",
            Self::PathTooLong => "LFS repository path is too long",
            Self::RetainedPathBytesExceeded => "LFS retained path byte limit exceeded",
            Self::InvalidRepositoryPath => "invalid LFS repository path",
            Self::ObjectFormatMismatch => "Git object format does not match repository",
            Self::UnsupportedRootKind(_) => "unsupported LFS reachability root object kind",
            Self::ObjectTooLarge(_) => "Git graph object exceeds the LFS traversal limit",
            Self::InvalidObject(_) => "invalid Git graph object",
            Self::InvalidObjectKind => "Git object kind does not match its graph edge",
            Self::InvalidTreeEntryKind => "Git tree entry has an unexpected object kind",
            Self::InvalidCommitTimestamp => "Git commit has an invalid committer timestamp",
            Self::NonCanonicalPointer(_) => "LFS pointer cannot be represented canonically",
            Self::ConflictingPointer => "conflicting metadata for an LFS object id",
            Self::Io(_) => "LFS reachability I/O failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LfsReachabilityError {}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::io::Cursor;

    use zmin_git_core::{
        CommitBuilder, GitObjectSink, InMemoryObjectStore, LooseObject, Signature, TagBuilder,
        TreeEntry, encode_tree,
    };

    use super::*;

    const FIRST_LFS_OID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const SECOND_LFS_OID: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    struct TestRepository {
        store: InMemoryObjectStore,
        refs: BTreeMap<String, ObjectId>,
        push_destinations: BTreeMap<String, ObjectId>,
        current: Option<String>,
        recent: Vec<String>,
        bounded_reads: Cell<usize>,
        largest_bounded_read: Cell<usize>,
    }

    impl TestRepository {
        fn new(algorithm: GitHashAlgorithm) -> Self {
            Self {
                store: InMemoryObjectStore::new(algorithm),
                refs: BTreeMap::new(),
                push_destinations: BTreeMap::new(),
                current: None,
                recent: Vec::new(),
                bounded_reads: Cell::new(0),
                largest_bounded_read: Cell::new(0),
            }
        }

        fn add_ref(&mut self, name: &str, object: ObjectId) {
            self.refs.insert(name.to_owned(), object);
        }

        fn add_push_destination(&mut self, name: &str, object: ObjectId) {
            self.push_destinations.insert(name.to_owned(), object);
        }

        fn set_current(&mut self, name: &str) {
            self.current = Some(name.to_owned());
        }

        fn set_recent(&mut self, names: &[&str]) {
            self.recent = names.iter().map(|name| (*name).to_owned()).collect();
        }

        fn write_blob(&self, bytes: &[u8]) -> ObjectId {
            self.store
                .write_object(GitObjectKind::Blob, bytes)
                .expect("write test blob")
        }

        fn write_tree(&self, entries: Vec<TreeEntry>) -> ObjectId {
            let bytes = encode_tree(&entries).expect("encode test tree");
            self.store
                .write_object(GitObjectKind::Tree, &bytes)
                .expect("write test tree")
        }

        fn write_commit(
            &self,
            tree: ObjectId,
            parent: Option<ObjectId>,
            timestamp: i64,
        ) -> ObjectId {
            let signature =
                Signature::new("A", "a@example.test", timestamp, "+0000").expect("test signature");
            let mut builder = CommitBuilder::new(tree, signature.clone(), signature);
            if let Some(parent) = parent {
                builder = builder.parent(parent);
            }
            let bytes = builder
                .message(b"fixture\n".to_vec())
                .expect("test message")
                .encode()
                .expect("encode test commit");
            self.store
                .write_object(GitObjectKind::Commit, &bytes)
                .expect("write test commit")
        }

        fn write_commit_with_parents(
            &self,
            tree: ObjectId,
            parents: &[ObjectId],
            timestamp: i64,
        ) -> ObjectId {
            let signature =
                Signature::new("A", "a@example.test", timestamp, "+0000").expect("test signature");
            let mut builder = CommitBuilder::new(tree, signature.clone(), signature);
            for parent in parents {
                builder = builder.parent(parent.clone());
            }
            let bytes = builder
                .message(b"merge fixture\n".to_vec())
                .expect("test message")
                .encode()
                .expect("encode test merge");
            self.store
                .write_object(GitObjectKind::Commit, &bytes)
                .expect("write test merge")
        }

        fn write_tag(&self, target: ObjectId, target_kind: GitObjectKind, name: &str) -> ObjectId {
            let signature =
                Signature::new("A", "a@example.test", 300, "+0000").expect("test signature");
            let bytes = TagBuilder::new(target, target_kind, name, signature)
                .expect("test tag")
                .message(b"tag fixture\n".to_vec())
                .expect("test tag message")
                .encode()
                .expect("encode test tag");
            self.store
                .write_object(GitObjectKind::Tag, &bytes)
                .expect("write test tag")
        }
    }

    impl GitObjectStore for TestRepository {
        fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
            self.store.read_object(id)
        }

        fn object_header_hint(&self, id: &ObjectId) -> io::Result<Option<(GitObjectKind, usize)>> {
            self.store.object_header_hint(id)
        }
    }

    impl LfsReachabilityRepository for TestRepository {
        fn object_format(&self) -> GitHashAlgorithm {
            self.store.algorithm()
        }

        fn read_lfs_object_bounded(
            &self,
            object: &ObjectId,
            max_bytes: usize,
        ) -> io::Result<PrefixOrFullObject> {
            self.bounded_reads
                .set(self.bounded_reads.get().saturating_add(1));
            self.largest_bounded_read
                .set(self.largest_bounded_read.get().max(max_bytes));
            self.store.read_object_prefix_or_full(object, max_bytes)
        }

        fn resolve_revision(
            &self,
            revision: &str,
        ) -> Result<Option<LfsResolvedRevision>, LfsReachabilityError> {
            self.refs
                .get(revision)
                .cloned()
                .map(|object| {
                    if revision.starts_with("refs/") {
                        LfsResolvedRevision::referenced(object, revision.to_owned())
                    } else {
                        Ok(LfsResolvedRevision::detached(object))
                    }
                })
                .transpose()
        }

        fn resolve_push_destination(
            &self,
            destination: &str,
        ) -> Result<Option<ObjectId>, LfsReachabilityError> {
            Ok(self
                .push_destinations
                .get(destination)
                .or_else(|| self.refs.get(destination))
                .cloned())
        }

        fn resolve_fetch_batch_ref(
            &self,
            reference: &str,
        ) -> Result<Option<String>, LfsReachabilityError> {
            if !reference.starts_with("refs/") || !check_ref_format(reference, false) {
                return Err(LfsReachabilityError::InvalidRemoteFetchRefspec);
            }
            Ok(Some(reference.to_owned()))
        }

        fn current_fetch_revision(
            &self,
            _policy: LfsReachabilityRemotePolicy,
        ) -> Result<Option<LfsResolvedRevision>, LfsReachabilityError> {
            self.current
                .as_ref()
                .map(|name| {
                    let object = self
                        .refs
                        .get(name)
                        .cloned()
                        .ok_or(LfsReachabilityError::RevisionNotFound)?;
                    if name.starts_with("refs/") {
                        LfsResolvedRevision::referenced(object, name.clone())
                    } else {
                        Ok(LfsResolvedRevision::detached(object))
                    }
                })
                .transpose()
        }

        fn visit_recent_fetch_roots(
            &self,
            _reference_cutoff_seconds: i64,
            _policy: LfsReachabilityRemotePolicy,
            visitor: &mut dyn LfsReachabilityRootVisitor,
        ) -> Result<(), LfsReachabilityError> {
            for name in &self.recent {
                let object = self
                    .refs
                    .get(name)
                    .cloned()
                    .ok_or(LfsReachabilityError::RevisionNotFound)?;
                visitor.visit(LfsRevisionRoot::new(name.clone(), object)?)?;
            }
            Ok(())
        }

        fn visit_all_fetch_roots(
            &self,
            _policy: LfsReachabilityRemotePolicy,
            visitor: &mut dyn LfsReachabilityRootVisitor,
        ) -> Result<(), LfsReachabilityError> {
            for (name, object) in &self.refs {
                visitor.visit(LfsRevisionRoot::new(name.clone(), object.clone())?)?;
            }
            Ok(())
        }
    }

    fn pointer_bytes(oid: &str, size: u64) -> Vec<u8> {
        LfsPointer::new(
            LfsOid::from_hex(oid).expect("test LFS oid"),
            size,
            Vec::new(),
        )
        .expect("test pointer")
        .serialize_canonical()
        .expect("serialize test pointer")
    }

    fn one_file_tree(repository: &TestRepository, path: &str, content: &[u8]) -> ObjectId {
        let blob = repository.write_blob(content);
        repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, path.as_bytes().to_vec(), blob)
                .expect("test tree entry"),
        ])
    }

    fn all_filter() -> LfsFetchFilter {
        LfsFetchFilter::from_values(None, None).expect("test filter")
    }

    fn planner<'a>(
        repository: &'a TestRepository,
        filter: &'a LfsFetchFilter,
    ) -> LfsReachabilityPlanner<'a, TestRepository> {
        LfsReachabilityPlanner::new(
            repository,
            filter,
            LfsReachabilityRemotePolicy::new(true, false),
            LfsReachabilityLimits::default(),
        )
        .expect("test planner")
    }

    fn oid_list(plan: &LfsReachabilityPlan) -> Vec<&str> {
        plan.pointers()
            .iter()
            .map(|pointer| pointer.oid().hex())
            .collect()
    }

    #[test]
    fn current_fetch_scans_tip_while_all_scans_history() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let old_tree = one_file_tree(&repository, "old.bin", &pointer_bytes(FIRST_LFS_OID, 10));
        let old = repository.write_commit(old_tree, None, 100);
        let new_tree = one_file_tree(&repository, "new.bin", &pointer_bytes(SECOND_LFS_OID, 20));
        let new = repository.write_commit(new_tree, Some(old), 200);
        repository.add_ref("refs/heads/main", new);
        repository.set_current("refs/heads/main");
        let filter = all_filter();

        let current = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Current)
            .expect("current fetch plan");
        assert_eq!(oid_list(&current), vec![SECOND_LFS_OID]);
        assert_eq!(current.transfer_groups().len(), 1);
        assert_eq!(
            current.transfer_groups()[0].remote_ref(),
            Some("refs/heads/main")
        );
        assert_eq!(current.transfer_groups()[0].pointer_indexes(), &[0]);

        let all = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::All)
            .expect("all fetch plan");
        assert_eq!(oid_list(&all), vec![FIRST_LFS_OID, SECOND_LFS_OID]);
        assert_eq!(all.stats().pointers(), 2);
    }

    #[test]
    fn fetch_all_ignores_configured_path_filters_like_current_git_lfs() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let tree = one_file_tree(
            &repository,
            "private.bin",
            &pointer_bytes(FIRST_LFS_OID, 10),
        );
        let commit = repository.write_commit(tree, None, 100);
        repository.add_ref("refs/heads/main", commit);
        let filter = LfsFetchFilter::from_values(None, Some("private*")).unwrap();

        let plan = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::All)
            .expect("all fetch plan");
        assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID]);
    }

    #[test]
    fn recent_forces_old_roots_but_stops_at_old_parents() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let parent_tree =
            one_file_tree(&repository, "parent.bin", &pointer_bytes(FIRST_LFS_OID, 10));
        let parent = repository.write_commit(parent_tree, None, 50);
        let root_tree = one_file_tree(&repository, "root.bin", &pointer_bytes(SECOND_LFS_OID, 20));
        let root = repository.write_commit(root_tree, Some(parent), 75);
        repository.add_ref("refs/heads/main", root);
        repository.set_current("refs/heads/main");
        repository.set_recent(&["refs/heads/main"]);
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Recent(LfsRecentFetchSelection::new(
                60, 100,
            )))
            .expect("recent fetch plan");
        assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
    }

    #[test]
    fn explicit_nested_annotated_tags_work_in_sha1_and_sha256_repositories() {
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let mut repository = TestRepository::new(algorithm);
            let tree = one_file_tree(&repository, "tagged.bin", &pointer_bytes(FIRST_LFS_OID, 10));
            let commit = repository.write_commit(tree, None, 100);
            let inner = repository.write_tag(commit, GitObjectKind::Commit, "inner");
            let outer = repository.write_tag(inner, GitObjectKind::Tag, "outer");
            repository.add_ref("refs/tags/outer", outer);
            let filter = all_filter();
            let selection = LfsFetchSelection::Explicit(
                LfsExplicitFetchSelection::new(vec!["refs/tags/outer".to_owned()]).unwrap(),
            );

            let plan = planner(&repository, &filter)
                .plan_fetch(&selection)
                .expect("tagged explicit plan");
            assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID]);
            assert_eq!(plan.transfer_groups().len(), 1);
            assert_eq!(
                plan.transfer_groups()[0].remote_ref(),
                Some("refs/tags/outer")
            );
        }
    }

    #[test]
    fn detached_current_and_explicit_object_ids_have_no_batch_ref() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let tree = one_file_tree(
            &repository,
            "detached.bin",
            &pointer_bytes(FIRST_LFS_OID, 10),
        );
        let commit = repository.write_commit(tree, None, 100);
        let commit_hex = commit.to_hex();
        repository.add_ref("detached", commit.clone());
        repository.add_ref(&commit_hex, commit);
        repository.set_current("detached");
        let filter = all_filter();

        let current = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Current)
            .expect("detached current plan");
        assert_eq!(current.transfer_groups().len(), 1);
        assert_eq!(current.transfer_groups()[0].remote_ref(), None);
        assert_eq!(current.transfer_groups()[0].pointer_indexes(), &[0]);

        let selection = LfsFetchSelection::Explicit(
            LfsExplicitFetchSelection::new(vec![commit_hex]).expect("explicit object selection"),
        );
        let explicit = planner(&repository, &filter)
            .plan_fetch(&selection)
            .expect("explicit object plan");
        assert_eq!(explicit.transfer_groups().len(), 1);
        assert_eq!(explicit.transfer_groups()[0].remote_ref(), None);
        assert_eq!(explicit.transfer_groups()[0].pointer_indexes(), &[0]);
    }

    #[test]
    fn explicit_fetch_groups_are_sorted_and_globally_deduplicate_shared_oids() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let shared_blob = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let unique_blob = repository.write_blob(&pointer_bytes(SECOND_LFS_OID, 20));
        let main_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"shared.bin".to_vec(), shared_blob.clone()).unwrap(),
        ]);
        let topic_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"shared.bin".to_vec(), shared_blob).unwrap(),
            TreeEntry::new(TreeMode::File, b"unique.bin".to_vec(), unique_blob).unwrap(),
        ]);
        let main = repository.write_commit(main_tree, None, 100);
        let topic = repository.write_commit(topic_tree, None, 200);
        repository.add_ref("refs/heads/main", main);
        repository.add_ref("refs/heads/topic", topic);
        let filter = all_filter();
        let selection = LfsFetchSelection::Explicit(
            LfsExplicitFetchSelection::new(vec![
                "refs/heads/topic".to_owned(),
                "refs/heads/main".to_owned(),
            ])
            .expect("explicit multi-ref selection"),
        );

        let plan = planner(&repository, &filter)
            .plan_fetch(&selection)
            .expect("grouped explicit fetch plan");
        assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID, SECOND_LFS_OID]);
        assert_eq!(plan.transfer_groups().len(), 2);
        assert_eq!(
            plan.transfer_groups()[0].remote_ref(),
            Some("refs/heads/main")
        );
        assert_eq!(plan.transfer_groups()[0].pointer_indexes(), &[0]);
        assert_eq!(
            plan.transfer_groups()[1].remote_ref(),
            Some("refs/heads/topic")
        );
        assert_eq!(plan.transfer_groups()[1].pointer_indexes(), &[1]);
    }

    #[test]
    fn annotated_tag_edges_obey_the_history_depth_limit() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let tree = one_file_tree(&repository, "tagged.bin", &pointer_bytes(FIRST_LFS_OID, 10));
        let commit = repository.write_commit(tree, None, 100);
        let inner = repository.write_tag(commit, GitObjectKind::Commit, "inner");
        let outer = repository.write_tag(inner, GitObjectKind::Tag, "outer");
        repository.add_ref("refs/tags/outer", outer);
        let filter = all_filter();
        let planner = LfsReachabilityPlanner::new(
            &repository,
            &filter,
            LfsReachabilityRemotePolicy::new(false, false),
            LfsReachabilityLimits::default().with_max_history_depth(1),
        )
        .expect("history-depth-limited planner");
        let selection = LfsFetchSelection::Explicit(
            LfsExplicitFetchSelection::new(vec!["refs/tags/outer".to_owned()]).unwrap(),
        );

        assert_eq!(
            planner.plan_fetch(&selection),
            Err(LfsReachabilityError::HistoryTooDeep)
        );
    }

    #[test]
    fn filter_and_oid_dedup_choose_the_smallest_path_deterministically() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let pointer = pointer_bytes(FIRST_LFS_OID, 10);
        let shared_blob = repository.write_blob(&pointer);
        let ignored_blob = repository.write_blob(&pointer_bytes(SECOND_LFS_OID, 20));
        let tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"keep-z.bin".to_vec(), shared_blob.clone()).unwrap(),
            TreeEntry::new(TreeMode::File, b"ignored.bin".to_vec(), ignored_blob).unwrap(),
            TreeEntry::new(TreeMode::File, b"keep-a.bin".to_vec(), shared_blob).unwrap(),
        ]);
        let commit = repository.write_commit(tree, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = LfsFetchFilter::from_values(Some("keep-*.bin"), None).unwrap();

        let plan = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Current)
            .expect("filtered fetch plan");
        assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID]);
        assert_eq!(plan.pointers()[0].repository_path(), b"keep-a.bin");
        assert_eq!(plan.stats().deduplicated_pointers(), 1);
        assert_eq!(plan.stats().unique_blobs(), 1);
    }

    #[test]
    fn push_range_excludes_remote_history() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let base_tree = one_file_tree(&repository, "base.bin", &pointer_bytes(FIRST_LFS_OID, 10));
        let base = repository.write_commit(base_tree, None, 100);
        let tip_tree = one_file_tree(&repository, "tip.bin", &pointer_bytes(SECOND_LFS_OID, 20));
        let tip = repository.write_commit(tip_tree, Some(base.clone()), 200);
        repository.add_ref("base", base);
        repository.add_ref("tip", tip);
        let filter = all_filter();

        let spec = LfsPushSpec::parse("base..tip").expect("range spec");
        let plan = planner(&repository, &filter)
            .plan_push_specs(&[spec])
            .expect("push range plan");
        assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
    }

    #[test]
    fn push_range_does_not_reemit_pointer_retained_from_remote_tree() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let base_blob = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let base_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"base.bin".to_vec(), base_blob.clone()).unwrap(),
        ]);
        let base = repository.write_commit(base_tree, None, 100);
        let tip_blob = repository.write_blob(&pointer_bytes(SECOND_LFS_OID, 20));
        let tip_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"base.bin".to_vec(), base_blob).unwrap(),
            TreeEntry::new(TreeMode::File, b"tip.bin".to_vec(), tip_blob).unwrap(),
        ]);
        let tip = repository.write_commit(tip_tree, Some(base.clone()), 200);
        repository.add_ref("base", base);
        repository.add_ref("tip", tip);
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_push_specs(&[LfsPushSpec::parse("base..tip").unwrap()])
            .expect("push range plan");
        assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
        assert_eq!(plan.stats().excluded_pointers(), 1);
    }

    #[test]
    fn remote_oid_exclusion_is_independent_of_fetch_path_filter() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let shared = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let private_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"object.bin".to_vec(), shared.clone()).unwrap(),
        ]);
        let base_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::Tree, b"private".to_vec(), private_tree).unwrap(),
        ]);
        let base = repository.write_commit(base_tree, None, 100);
        let assets_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"object.bin".to_vec(), shared).unwrap(),
        ]);
        let tip_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::Tree, b"assets".to_vec(), assets_tree).unwrap(),
        ]);
        let tip = repository.write_commit(tip_tree, Some(base.clone()), 200);
        repository.add_ref("base", base);
        repository.add_ref("tip", tip);
        let filter = LfsFetchFilter::from_values(Some("assets/**"), None).unwrap();

        let plan = planner(&repository, &filter)
            .plan_push_specs(&[LfsPushSpec::parse("base..tip").unwrap()])
            .expect("path-independent remote exclusion");
        assert!(plan.pointers().is_empty());
        assert_eq!(plan.stats().excluded_pointers(), 1);
    }

    #[test]
    fn push_planning_does_not_apply_fetch_only_path_filters() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let private_tree = one_file_tree(
            &repository,
            "private.bin",
            &pointer_bytes(FIRST_LFS_OID, 10),
        );
        let tip = repository.write_commit(private_tree, None, 100);
        repository.add_ref("tip", tip);
        let filter = LfsFetchFilter::from_values(Some("assets/**"), None).unwrap();

        let plan = planner(&repository, &filter)
            .plan_push_specs(&[LfsPushSpec::parse("tip").unwrap()])
            .expect("complete push plan");
        assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID]);
    }

    #[test]
    fn push_range_walks_all_merge_parents() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let empty_tree = repository.write_tree(Vec::new());
        let base = repository.write_commit(empty_tree.clone(), None, 100);
        let feature_tree = one_file_tree(
            &repository,
            "feature.bin",
            &pointer_bytes(SECOND_LFS_OID, 20),
        );
        let feature = repository.write_commit(feature_tree, Some(base.clone()), 200);
        let merge = repository.write_commit_with_parents(empty_tree, &[base.clone(), feature], 300);
        repository.add_ref("base", base);
        repository.add_ref("merge", merge);
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_push_specs(&[LfsPushSpec::parse("base..merge").unwrap()])
            .expect("merge push plan");
        assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
    }

    #[test]
    fn refspec_uses_remote_bound_destination_resolution() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let base_blob = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let base_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"base.bin".to_vec(), base_blob.clone()).unwrap(),
        ]);
        let base = repository.write_commit(base_tree, None, 100);
        let tip_blob = repository.write_blob(&pointer_bytes(SECOND_LFS_OID, 20));
        let tip_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"base.bin".to_vec(), base_blob).unwrap(),
            TreeEntry::new(TreeMode::File, b"tip.bin".to_vec(), tip_blob).unwrap(),
        ]);
        let tip = repository.write_commit(tip_tree, Some(base.clone()), 200);
        repository.add_ref("refs/heads/local", tip);
        repository.add_push_destination("refs/heads/remote", base);
        let filter = all_filter();

        let spec = LfsPushSpec::parse("+refs/heads/local:remote").unwrap();
        let plan = planner(&repository, &filter)
            .plan_push_specs(&[spec])
            .expect("refspec plan");
        assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
        assert_eq!(plan.transfer_groups().len(), 1);
        assert_eq!(
            plan.transfer_groups()[0].remote_ref(),
            Some("refs/heads/remote")
        );
        assert_eq!(plan.transfer_groups()[0].pointer_indexes(), &[0]);
    }

    #[test]
    fn revision_push_uses_its_canonical_ref_and_remote_history() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let base_tree = one_file_tree(&repository, "base.bin", &pointer_bytes(FIRST_LFS_OID, 10));
        let base = repository.write_commit(base_tree, None, 100);
        let tip_tree = one_file_tree(&repository, "tip.bin", &pointer_bytes(SECOND_LFS_OID, 20));
        let tip = repository.write_commit(tip_tree, Some(base.clone()), 200);
        repository.add_ref("refs/heads/main", tip);
        repository.add_push_destination("refs/heads/main", base);
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_push_specs(&[LfsPushSpec::parse("refs/heads/main").unwrap()])
            .expect("revision push plan");
        assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
        assert_eq!(plan.transfer_groups().len(), 1);
        assert_eq!(
            plan.transfer_groups()[0].remote_ref(),
            Some("refs/heads/main")
        );
    }

    #[test]
    fn pre_push_update_is_object_format_aware_and_excludes_remote() {
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let repository = TestRepository::new(algorithm);
            let base_tree =
                one_file_tree(&repository, "base.bin", &pointer_bytes(FIRST_LFS_OID, 10));
            let base = repository.write_commit(base_tree, None, 100);
            let tip_tree =
                one_file_tree(&repository, "tip.bin", &pointer_bytes(SECOND_LFS_OID, 20));
            let tip = repository.write_commit(tip_tree, Some(base.clone()), 200);
            let line = format!(
                "refs/heads/main {} refs/heads/main {}\n",
                tip.to_hex(),
                base.to_hex()
            );
            let filter = all_filter();
            let plan = planner(&repository, &filter)
                .plan_pre_push(&mut Cursor::new(line.into_bytes()))
                .expect("pre-push plan");
            assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
            assert_eq!(plan.stats().pre_push_updates(), 1);
        }
    }

    #[test]
    fn pre_push_parser_accepts_authoritative_local_selectors_and_rejects_uppercase() {
        let algorithm = GitHashAlgorithm::Sha1;
        let object = "1".repeat(40);
        let zero = "0".repeat(40);
        let input = format!(
            "refs/heads/a {object} refs/heads/a {zero}\nrefs/heads/b {object} refs/heads/b {object}\n(delete) {zero} refs/heads/c {object}\n"
        );
        let updates = parse_pre_push_updates(
            &mut Cursor::new(input.into_bytes()),
            algorithm,
            LfsReachabilityLimits::default(),
        )
        .expect("pre-push updates");
        assert_eq!(updates[0].kind(), LfsPrePushUpdateKind::Create);
        assert_eq!(updates[1].kind(), LfsPrePushUpdateKind::Update);
        assert_eq!(updates[2].kind(), LfsPrePushUpdateKind::Delete);

        let uppercase = format!("refs/heads/a {} refs/heads/a {zero}\n", "A".repeat(40));
        assert_eq!(
            parse_pre_push_updates(
                &mut Cursor::new(uppercase.into_bytes()),
                algorithm,
                LfsReachabilityLimits::default(),
            ),
            Err(LfsReachabilityError::InvalidPrePushLine)
        );

        let unqualified = format!("main {object} refs/heads/a {zero}\n");
        let updates = parse_pre_push_updates(
            &mut Cursor::new(unqualified.into_bytes()),
            algorithm,
            LfsReachabilityLimits::default(),
        )
        .expect("authoritative local object permits an unqualified selector");
        assert_eq!(updates[0].local_ref(), b"main");

        let mut non_utf8 = b"topic-\xff ".to_vec();
        non_utf8.extend_from_slice(object.as_bytes());
        non_utf8.extend_from_slice(b" refs/heads/a ");
        non_utf8.extend_from_slice(zero.as_bytes());
        non_utf8.push(b'\n');
        let updates = parse_pre_push_updates(
            &mut Cursor::new(non_utf8),
            algorithm,
            LfsReachabilityLimits::default(),
        )
        .expect("authoritative local object permits a raw byte selector");
        assert_eq!(updates[0].local_ref(), b"topic-\xff");

        for invalid in ["bad selector", "bad\tselector", "bad\rselector"] {
            let input = format!("{invalid} {object} refs/heads/a {zero}\n");
            assert_eq!(
                parse_pre_push_updates(
                    &mut Cursor::new(input.into_bytes()),
                    algorithm,
                    LfsReachabilityLimits::default(),
                ),
                Err(LfsReachabilityError::InvalidPrePushLine)
            );
        }
    }

    #[test]
    fn pre_push_retains_sorted_remote_groups_while_deduplicating_transfers() {
        let repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let first_blob = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let second_blob = repository.write_blob(&pointer_bytes(SECOND_LFS_OID, 20));
        let first_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"shared.bin".to_vec(), first_blob.clone()).unwrap(),
        ]);
        let second_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"shared.bin".to_vec(), first_blob).unwrap(),
            TreeEntry::new(TreeMode::File, b"unique.bin".to_vec(), second_blob).unwrap(),
        ]);
        let empty_tree = repository.write_tree(Vec::new());
        let first = repository.write_commit(first_tree, None, 100);
        let second = repository.write_commit(second_tree, None, 200);
        let empty = repository.write_commit(empty_tree, None, 300);
        let zero = "0".repeat(40);
        let input = format!(
            "topic {} refs/heads/b {zero}\nHEAD {} refs/heads/a {zero}\ndetached {} refs/heads/c {zero}\n",
            second.to_hex(),
            first.to_hex(),
            empty.to_hex()
        );
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_pre_push(&mut Cursor::new(input.into_bytes()))
            .expect("grouped pre-push plan");
        assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID, SECOND_LFS_OID]);
        assert_eq!(plan.stats().pre_push_updates(), 3);
        assert_eq!(plan.transfer_groups().len(), 3);
        assert_eq!(plan.transfer_groups()[0].remote_ref(), Some("refs/heads/a"));
        assert_eq!(plan.transfer_groups()[0].pointer_indexes(), &[0]);
        assert_eq!(plan.transfer_groups()[1].remote_ref(), Some("refs/heads/b"));
        assert_eq!(plan.transfer_groups()[1].pointer_indexes(), &[1]);
        assert_eq!(plan.transfer_groups()[2].remote_ref(), Some("refs/heads/c"));
        assert!(plan.transfer_groups()[2].pointer_indexes().is_empty());
    }

    #[test]
    fn empty_filter_preserves_valid_raw_repository_path_bytes() {
        for path in [
            b"back\\slash.bin".as_slice(),
            b"tab\tname.bin".as_slice(),
            b"line\nname.bin".as_slice(),
            b"non-utf8-\xff.bin".as_slice(),
        ] {
            let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
            let blob = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
            let tree = repository.write_tree(vec![
                TreeEntry::new(TreeMode::File, path.to_vec(), blob).expect("raw path tree entry"),
            ]);
            let commit = repository.write_commit(tree, None, 100);
            repository.add_ref("refs/heads/main", commit);
            repository.set_current("refs/heads/main");
            let filter = all_filter();

            let plan = planner(&repository, &filter)
                .plan_fetch(&LfsFetchSelection::Current)
                .expect("raw Git path plan");
            assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID]);
            assert_eq!(plan.pointers()[0].repository_path(), path);
        }
    }

    #[test]
    fn pre_push_line_limit_fails_before_accumulating_the_rest() {
        let limits = LfsReachabilityLimits::default().with_max_pre_push_line_bytes(16);
        let input = vec![b'x'; 1_000_000];
        let mut reader = Cursor::new(input);
        assert_eq!(
            parse_pre_push_updates(&mut reader, GitHashAlgorithm::Sha1, limits),
            Err(LfsReachabilityError::PrePushLineTooLong)
        );
        assert!(reader.position() <= 1_000_000);
    }

    #[test]
    fn noncanonical_current_pointer_is_canonicalized_and_large_blob_is_not_read() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let current = format!(
            "zeta tail\next-2-b sha256:{SECOND_LFS_OID}\nsize +10\nalpha \next-1-a sha256:{FIRST_LFS_OID}\noid sha256:{FIRST_LFS_OID}\nversion https://git-lfs.github.com/spec/v1"
        );
        let current_blob = repository.write_blob(current.as_bytes());
        let large_blob = repository.write_blob(&vec![7; LFS_POINTER_MAX_BYTES * 4]);
        let tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"current.bin".to_vec(), current_blob).unwrap(),
            TreeEntry::new(TreeMode::File, b"large.bin".to_vec(), large_blob).unwrap(),
        ]);
        let commit = repository.write_commit(tree, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Current)
            .expect("current pointer plan");
        assert_eq!(oid_list(&plan), vec![FIRST_LFS_OID]);
        let canonical = plan.pointers()[0]
            .pointer()
            .serialize_canonical()
            .expect("canonical output");
        assert!(canonical.ends_with(b"\n"));
        assert!(LfsPointer::parse_strict(&canonical).is_ok());
        assert_eq!(plan.pointers()[0].pointer().extensions()[0].priority(), 1);
        assert_eq!(plan.pointers()[0].pointer().extras()[0].key(), "alpha");
        assert_eq!(plan.stats().blob_prefix_reads(), 1);
        assert!(plan.stats().blob_prefix_bytes() < LFS_POINTER_MAX_BYTES);
        assert!(plan.stats().max_retained_path_bytes() > 0);
        assert!(
            plan.stats().max_retained_path_bytes() >= plan.stats().retained_path_bytes(),
            "peak retained path bytes must cover the final retained amount"
        );
    }

    #[test]
    fn conflicting_metadata_for_same_lfs_oid_fails_closed() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let first = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let second = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 11));
        let tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, b"a.bin".to_vec(), first).unwrap(),
            TreeEntry::new(TreeMode::File, b"b.bin".to_vec(), second).unwrap(),
        ]);
        let commit = repository.write_commit(tree, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = all_filter();

        assert_eq!(
            planner(&repository, &filter).plan_fetch(&LfsFetchSelection::Current),
            Err(LfsReachabilityError::ConflictingPointer)
        );
    }

    #[test]
    fn extension_pointer_is_preserved_for_the_transfer_policy_layer() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let extension = super::super::LfsPointerExtension::new(
            "ext-0-transform".to_owned(),
            LfsOid::from_hex(SECOND_LFS_OID).unwrap(),
        )
        .unwrap();
        let pointer = LfsPointer::new(
            LfsOid::from_hex(FIRST_LFS_OID).unwrap(),
            10,
            vec![extension],
        )
        .unwrap()
        .serialize_canonical()
        .unwrap();
        let tree = one_file_tree(&repository, "extended.bin", &pointer);
        let commit = repository.write_commit(tree, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Current)
            .expect("extension pointer plan");
        assert_eq!(plan.pointers().len(), 1);
        assert_eq!(plan.pointers()[0].pointer().extensions().len(), 1);
        assert_eq!(
            plan.pointers()[0].pointer().extensions()[0].key(),
            "ext-0-transform"
        );
    }

    #[test]
    fn contradictory_tag_edge_kind_fails_independently_of_root_order() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let tree = one_file_tree(&repository, "file.bin", &pointer_bytes(FIRST_LFS_OID, 10));
        let commit = repository.write_commit(tree, None, 100);
        let bad_tag = repository.write_tag(commit.clone(), GitObjectKind::Tag, "bad-kind");
        repository.add_ref("refs/heads/main", commit);
        repository.add_ref("refs/tags/bad", bad_tag);
        let filter = all_filter();

        assert_eq!(
            planner(&repository, &filter).plan_fetch(&LfsFetchSelection::All),
            Err(LfsReachabilityError::InvalidObjectKind)
        );
    }

    #[test]
    fn non_utf8_repository_path_is_filtered_and_preserved_as_bytes() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let blob = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, vec![b'f', 0xff], blob).unwrap(),
        ]);
        let commit = repository.write_commit(tree, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = all_filter();

        let plan = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Current)
            .expect("non-UTF-8 path plan");
        assert_eq!(plan.pointers().len(), 1);
        assert_eq!(plan.pointers()[0].repository_path(), &[b'f', 0xff]);
    }

    #[test]
    fn non_utf8_repository_paths_obey_byte_include_exclude_policy() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let private_blob = repository.write_blob(&pointer_bytes(FIRST_LFS_OID, 10));
        let private_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, vec![b'f', 0xff], private_blob).unwrap(),
        ]);
        let assets_blob = repository.write_blob(&pointer_bytes(SECOND_LFS_OID, 20));
        let assets_tree = repository.write_tree(vec![
            TreeEntry::new(TreeMode::File, vec![b'f', 0xfe], assets_blob).unwrap(),
        ]);
        let root = repository.write_tree(vec![
            TreeEntry::new(TreeMode::Tree, b"private".to_vec(), private_tree).unwrap(),
            TreeEntry::new(TreeMode::Tree, b"assets".to_vec(), assets_tree).unwrap(),
        ]);
        let commit = repository.write_commit(root, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = LfsFetchFilter::from_values(None, Some("private/**")).unwrap();

        let plan = planner(&repository, &filter)
            .plan_fetch(&LfsFetchSelection::Current)
            .expect("byte-filter plan");
        assert_eq!(oid_list(&plan), vec![SECOND_LFS_OID]);
        assert_eq!(plan.pointers()[0].repository_path(), b"assets/f\xfe");
    }

    #[test]
    fn tree_depth_limit_is_enforced() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let leaf = one_file_tree(&repository, "file.bin", &pointer_bytes(FIRST_LFS_OID, 10));
        let middle = repository.write_tree(vec![
            TreeEntry::new(TreeMode::Tree, b"nested".to_vec(), leaf).unwrap(),
        ]);
        let root = repository.write_tree(vec![
            TreeEntry::new(TreeMode::Tree, b"middle".to_vec(), middle).unwrap(),
        ]);
        let commit = repository.write_commit(root, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = all_filter();
        let planner = LfsReachabilityPlanner::new(
            &repository,
            &filter,
            LfsReachabilityRemotePolicy::new(false, false),
            LfsReachabilityLimits::default().with_max_tree_depth(1),
        )
        .expect("depth-limited planner");
        assert_eq!(
            planner.plan_fetch(&LfsFetchSelection::Current),
            Err(LfsReachabilityError::TreeTooDeep)
        );
    }

    #[test]
    fn cumulative_retained_path_budget_is_enforced() {
        let mut repository = TestRepository::new(GitHashAlgorithm::Sha1);
        let tree = one_file_tree(
            &repository,
            "123456789.bin",
            &pointer_bytes(FIRST_LFS_OID, 10),
        );
        let commit = repository.write_commit(tree, None, 100);
        repository.add_ref("refs/heads/main", commit);
        repository.set_current("refs/heads/main");
        let filter = all_filter();
        let planner = LfsReachabilityPlanner::new(
            &repository,
            &filter,
            LfsReachabilityRemotePolicy::new(false, false),
            LfsReachabilityLimits::default().with_max_retained_path_bytes(8),
        )
        .expect("path-budget planner");

        assert_eq!(
            planner.plan_fetch(&LfsFetchSelection::Current),
            Err(LfsReachabilityError::RetainedPathBytesExceeded)
        );
    }

    #[test]
    fn push_spec_parser_is_strict_and_preserves_force_destination() {
        let refspec = LfsPushSpec::parse("+refs/heads/a:refs/heads/b").expect("refspec");
        let LfsPushSpec::Refspec(refspec) = refspec else {
            panic!("expected refspec");
        };
        assert!(refspec.force());
        assert_eq!(refspec.source(), "refs/heads/a");
        assert_eq!(refspec.destination(), "refs/heads/b");
        assert_eq!(
            LfsPushSpec::parse("a...b"),
            Err(LfsReachabilityError::InvalidRefspec)
        );
        assert_eq!(
            LfsPushSpec::parse("refs/heads/*:refs/heads/*"),
            Err(LfsReachabilityError::InvalidRefspec)
        );
        assert_eq!(
            LfsPushRevision::new("a..b".to_owned()),
            Err(LfsReachabilityError::InvalidRefspec)
        );
        assert_eq!(
            LfsPushRange::new("a..b".to_owned(), "c".to_owned()),
            Err(LfsReachabilityError::InvalidRefspec)
        );
    }
}
