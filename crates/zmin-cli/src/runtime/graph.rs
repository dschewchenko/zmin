use std::cmp::Ordering;
use std::collections::hash_map::{DefaultHasher, Entry};
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use zmin_git_core::{
    CommitLinksCache, CommitObject, CommitObjectCache, GitHashAlgorithm, GitObjectKind,
    GitObjectStore, LooseObject, LooseObjectStore, ObjectId, PackedObjectOrdinalLookup, TreeMode,
    TreeObjectCache, TreeObjectRef, decode_commit, decode_commit_links,
    decode_pack_index_object_ids_from_path, decode_tag, decode_tree, decode_tree_entry_ref,
};

use super::{
    CliError, CommitGraphIndex, GitRepo, HistoryArgCursor, HistoryArgToken, HistoryOptionName,
    HistoryPathQuery, RefStore, Result, current_branch_ref, phase_trace,
    read_common_config_entries, read_common_git_dir, read_config_entries,
    repo_hash_algorithm_from_config, resolve_objectish, resolve_treeish, short_ref_name,
    signature_timestamp, wildcard_match,
};

const REV_LIST_INITIAL_CAPACITY_LIMIT: usize = 8192;
const TREE_OBJECT_REF_CACHE_ENTRY_LIMIT: usize = 8192;

trait RevListSeenObjectIds {
    fn insert(&mut self, id: ObjectId) -> bool;
}

impl RevListSeenObjectIds for HashSet<ObjectId> {
    fn insert(&mut self, id: ObjectId) -> bool {
        HashSet::insert(self, id)
    }
}

enum CompactUnpackedRevListSeenObjectIds {
    Sha1(HashSet<[u8; 20]>),
    Sha256(HashSet<[u8; 32]>),
}

impl CompactUnpackedRevListSeenObjectIds {
    fn with_capacity(algorithm: GitHashAlgorithm, capacity: usize) -> Self {
        match algorithm {
            GitHashAlgorithm::Sha1 => Self::Sha1(HashSet::with_capacity(capacity)),
            GitHashAlgorithm::Sha256 => Self::Sha256(HashSet::with_capacity(capacity)),
        }
    }

    fn insert(&mut self, id: ObjectId) -> bool {
        match self {
            Self::Sha1(seen) => seen.insert(
                id.as_bytes()
                    .try_into()
                    .expect("SHA-1 object id has a 20-byte digest"),
            ),
            Self::Sha256(seen) => seen.insert(
                id.as_bytes()
                    .try_into()
                    .expect("SHA-256 object id has a 32-byte digest"),
            ),
        }
    }
}

struct PackedRevListSeenObjectIds {
    ordinals: PackedObjectOrdinalLookup,
    bits: Vec<u8>,
}

impl PackedRevListSeenObjectIds {
    fn insert(&mut self, id: &ObjectId) -> Option<bool> {
        let position = self.ordinals.position(id)?;
        let byte = self.bits.get_mut(position / 8)?;
        let mask = 1_u8 << (position % 8);
        let inserted = *byte & mask == 0;
        *byte |= mask;
        Some(inserted)
    }
}

struct CompactRevListSeenObjectIds {
    packed: Option<PackedRevListSeenObjectIds>,
    unpacked: CompactUnpackedRevListSeenObjectIds,
}

impl CompactRevListSeenObjectIds {
    fn with_capacity(store: &LooseObjectStore, capacity: usize) -> io::Result<Self> {
        let ordinals = store.single_pack_object_ordinals()?;
        let unpacked_capacity = if ordinals.is_some() { 0 } else { capacity };
        let packed = ordinals.map(|ordinals| PackedRevListSeenObjectIds {
            bits: vec![0; ordinals.object_count().div_ceil(8)],
            ordinals,
        });
        Ok(Self {
            packed,
            unpacked: CompactUnpackedRevListSeenObjectIds::with_capacity(
                store.algorithm(),
                unpacked_capacity,
            ),
        })
    }
}

impl RevListSeenObjectIds for CompactRevListSeenObjectIds {
    fn insert(&mut self, id: ObjectId) -> bool {
        if let Some(inserted) = self.packed.as_mut().and_then(|packed| packed.insert(&id)) {
            return inserted;
        }
        self.unpacked.insert(id)
    }
}

pub(crate) struct CollectedCommit {
    pub(crate) id: ObjectId,
    pub(crate) commit: Arc<CommitObject>,
}

pub(crate) struct CollectedCommitMetadata {
    pub(crate) id: ObjectId,
    pub(crate) tree: ObjectId,
    pub(crate) parents: Vec<ObjectId>,
    pub(crate) author: Vec<u8>,
    pub(crate) committer: Vec<u8>,
    pub(crate) grep_matches: bool,
}

struct RevListChildrenBoundaryMetadata {
    external_parent_ids: Vec<ObjectId>,
    external_parent_children: Vec<Vec<usize>>,
    external_parent_indices_by_child: Vec<Vec<usize>>,
}

pub(crate) struct RevListChildrenTopology {
    pub(crate) ids: Vec<ObjectId>,
    pub(crate) parent_indices: Vec<Vec<usize>>,
    id_buckets: HashMap<u64, Vec<usize>>,
    boundary_metadata: Option<RevListChildrenBoundaryMetadata>,
}

pub(crate) struct RevListChildrenBoundaryHandle {
    pub(crate) id: ObjectId,
    pub(crate) candidate_index: Option<usize>,
    pub(crate) external_parent_index: Option<usize>,
}

struct RevListObjectIdIndex {
    buckets: HashMap<u64, Vec<usize>>,
}

impl RevListObjectIdIndex {
    fn new_metadata(metadata: &[CollectedCommitMetadata]) -> Self {
        let mut buckets = HashMap::with_capacity(metadata.len());
        for (index, entry) in metadata.iter().enumerate() {
            let mut hasher = DefaultHasher::new();
            entry.id.hash(&mut hasher);
            buckets
                .entry(hasher.finish())
                .or_insert_with(Vec::new)
                .push(index);
        }
        Self { buckets }
    }

    fn find_metadata(
        &self,
        metadata: &[CollectedCommitMetadata],
        target: &ObjectId,
    ) -> Option<usize> {
        let mut hasher = DefaultHasher::new();
        target.hash(&mut hasher);
        self.buckets.get(&hasher.finish()).and_then(|candidates| {
            candidates.iter().copied().find(|index| {
                metadata
                    .get(*index)
                    .is_some_and(|entry| &entry.id == target)
            })
        })
    }
}

impl RevListChildrenTopology {
    pub(crate) fn from_metadata(
        metadata: Vec<CollectedCommitMetadata>,
        include_boundary_metadata: bool,
    ) -> Self {
        let candidate_indices_by_id = RevListObjectIdIndex::new_metadata(&metadata);
        let mut boundary_metadata = include_boundary_metadata.then(|| {
            let external_parent_ids = Vec::new();
            let external_parent_children = Vec::new();
            let external_parent_indices_by_id = HashMap::new();
            let external_parent_indices_by_child = vec![Vec::new(); metadata.len()];
            (
                external_parent_ids,
                external_parent_children,
                external_parent_indices_by_id,
                external_parent_indices_by_child,
            )
        });
        let parent_indices = metadata
            .iter()
            .enumerate()
            .map(|(child_index, entry)| {
                let mut child_parent_indices = Vec::new();
                for parent in &entry.parents {
                    if let Some(parent_index) =
                        candidate_indices_by_id.find_metadata(&metadata, parent)
                    {
                        child_parent_indices.push(parent_index);
                    } else if let Some((
                        external_parent_ids,
                        external_parent_children,
                        external_parent_indices_by_id,
                        external_parent_indices_by_child,
                    )) = boundary_metadata.as_mut()
                    {
                        let external_parent_index =
                            if let Some(index) = external_parent_indices_by_id.get(parent) {
                                *index
                            } else {
                                let index = external_parent_ids.len();
                                external_parent_ids.push(parent.clone());
                                external_parent_children.push(Vec::new());
                                external_parent_indices_by_id.insert(parent.clone(), index);
                                index
                            };
                        external_parent_indices_by_child[child_index].push(external_parent_index);
                        external_parent_children[external_parent_index].push(child_index);
                    }
                }
                child_parent_indices
            })
            .collect();
        let mut ids = Vec::with_capacity(metadata.len());
        for entry in metadata {
            ids.push(entry.id);
        }
        let mut id_buckets = HashMap::with_capacity(ids.len());
        for (index, id) in ids.iter().enumerate() {
            let mut hasher = DefaultHasher::new();
            id.hash(&mut hasher);
            id_buckets
                .entry(hasher.finish())
                .or_insert_with(Vec::new)
                .push(index);
        }
        let boundary_metadata = boundary_metadata.map(
            |(
                external_parent_ids,
                external_parent_children,
                _external_parent_indices_by_id,
                external_parent_indices_by_child,
            )| RevListChildrenBoundaryMetadata {
                external_parent_ids,
                external_parent_children,
                external_parent_indices_by_child,
            },
        );
        Self {
            ids,
            parent_indices,
            id_buckets,
            boundary_metadata,
        }
    }

    pub(crate) fn take_ids(&mut self) -> Vec<ObjectId> {
        std::mem::take(&mut self.ids)
    }

    pub(crate) fn candidate_index_for_id(
        &self,
        target: &ObjectId,
        candidate_ids: &[ObjectId],
    ) -> Option<usize> {
        let mut hasher = DefaultHasher::new();
        target.hash(&mut hasher);
        self.id_buckets.get(&hasher.finish()).and_then(|indices| {
            indices
                .iter()
                .copied()
                .find(|index| candidate_ids.get(*index).is_some_and(|id| id == target))
        })
    }

    pub(crate) fn boundary_handles_for(
        &self,
        displayed_candidate_indices: &[usize],
        shown_ids: &HashSet<ObjectId>,
        candidate_ids: &[ObjectId],
    ) -> Vec<RevListChildrenBoundaryHandle> {
        let mut seen = HashSet::new();
        let mut handles = Vec::new();
        for child_index in displayed_candidate_indices.iter().copied().rev() {
            let Some(parent_indices) = self.parent_indices.get(child_index) else {
                continue;
            };
            for parent_index in parent_indices {
                let Some(parent_id) = candidate_ids.get(*parent_index) else {
                    continue;
                };
                if !shown_ids.contains(parent_id) && seen.insert(parent_id.clone()) {
                    handles.push(RevListChildrenBoundaryHandle {
                        id: parent_id.clone(),
                        candidate_index: Some(*parent_index),
                        external_parent_index: None,
                    });
                }
            }
            if let Some(boundary_metadata) = self.boundary_metadata.as_ref()
                && let Some(external_parent_indices) = boundary_metadata
                    .external_parent_indices_by_child
                    .get(child_index)
            {
                for external_parent_index in external_parent_indices {
                    let Some(parent_id) = boundary_metadata
                        .external_parent_ids
                        .get(*external_parent_index)
                    else {
                        continue;
                    };
                    if !shown_ids.contains(parent_id) && seen.insert(parent_id.clone()) {
                        handles.push(RevListChildrenBoundaryHandle {
                            id: parent_id.clone(),
                            candidate_index: None,
                            external_parent_index: Some(*external_parent_index),
                        });
                    }
                }
            }
        }
        handles
    }

    pub(crate) fn external_children_for_index(&self, index: usize) -> &[usize] {
        self.boundary_metadata
            .as_ref()
            .and_then(|metadata| metadata.external_parent_children.get(index))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

pub(crate) struct CollectedCommitOneline {
    pub(crate) id: ObjectId,
    pub(crate) parents: Vec<ObjectId>,
    pub(crate) subject: String,
}

#[derive(Clone)]
pub(crate) struct RevListObjectCommitCandidate {
    pub(crate) id: ObjectId,
    pub(crate) tree: ObjectId,
    pub(crate) parents: Vec<ObjectId>,
    pub(crate) author_timestamp: Option<i64>,
    pub(crate) committer_timestamp: Option<i64>,
    pub(crate) matches: bool,
    pub(crate) sequence: usize,
    pub(crate) is_boundary: bool,
}

pub(crate) struct RevListObjectBoundaryMetadata {
    pub(crate) id: ObjectId,
    pub(crate) tree: ObjectId,
    pub(crate) author_timestamp: Option<i64>,
    pub(crate) committer_timestamp: i64,
}

pub(crate) trait RevListObjectCommitPredicate {
    fn matches(&mut self, id: &ObjectId, commit: &CommitObject) -> Result<bool>;

    fn matches_metadata(
        &mut self,
        _id: &ObjectId,
        _author_timestamp: Option<i64>,
        _committer_timestamp: i64,
        _parent_count: usize,
    ) -> Result<bool>;

    fn needs_full_commit_object(&self) -> bool;

    fn first_parent_only(&self) -> bool {
        false
    }
}

pub(crate) struct CommitGraphRenderHint {
    pub(crate) id: ObjectId,
    pub(crate) parents: Arc<[ObjectId]>,
    pub(crate) committer_timestamp: i64,
}

struct CommitGraphPendingPosition {
    position: u32,
    timestamp: u64,
    sequence: u64,
}

impl PartialEq for CommitGraphPendingPosition {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
            && self.timestamp == other.timestamp
            && self.sequence == other.sequence
    }
}

impl Eq for CommitGraphPendingPosition {}

impl PartialOrd for CommitGraphPendingPosition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CommitGraphPendingPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp
            .cmp(&other.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
            .then_with(|| other.position.cmp(&self.position))
    }
}

pub(crate) struct CollectedCommitTree {
    pub(crate) id: ObjectId,
    pub(crate) tree: ObjectId,
    pub(crate) is_boundary: bool,
}

struct HistoryPathBuildCandidate {
    pub(crate) id_index: usize,
    pub(crate) tree_index: usize,
    pub(crate) parent_indices: Vec<usize>,
    pub(crate) traversal_parent_count: usize,
    pub(crate) author_timestamp: Option<i64>,
    pub(crate) committer_timestamp: i64,
}

pub(crate) struct HistoryPathCommitCandidate {
    pub(crate) id_index: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HistoryPathSelectionPolicy<'a> {
    pub(crate) full_history: bool,
    pub(crate) sparse: bool,
    pub(crate) first_parent: bool,
    pub(crate) topo_order: bool,
    pub(crate) rewrite_parents: bool,
    pub(crate) children: bool,
    pub(crate) missing_policy: RevListTreeMissingPolicy<'a>,
    pub(crate) additional_excluded: &'a HashSet<ObjectId>,
    pub(crate) follow: Option<&'a HistoryPathFollowPolicy>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FollowDirection {
    Forward,
    Reverse,
}

#[derive(Clone, Debug)]
pub(crate) struct HistoryPathFollowPolicy {
    initial_path: Vec<u8>,
    pub(crate) direction: FollowDirection,
}

const FOLLOW_BRANCH_STATE_LIMIT: usize = 4096;
const FOLLOW_PATH_BYTES_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FollowPathId(usize);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct FollowPathEntryIdentity {
    mode: TreeMode,
    id: ObjectId,
}

struct FollowPathValue {
    path: Vec<u8>,
    identity: Option<FollowPathEntryIdentity>,
}

struct FollowPathState {
    path: Vec<u8>,
    query: HistoryPathQuery,
    identity: Option<FollowPathEntryIdentity>,
    users: usize,
}

struct FollowPathInterner {
    states: Vec<Option<FollowPathState>>,
    free_ids: Vec<FollowPathId>,
    fingerprints: HashMap<u64, Vec<FollowPathId>>,
    live_path_bytes: usize,
    byte_limit: usize,
}

impl FollowPathInterner {
    fn new(initial_path: &[u8]) -> Result<Self> {
        Self::new_with_limit(initial_path, FOLLOW_PATH_BYTES_LIMIT)
    }

    #[cfg(test)]
    fn new_with_limit_for_test(initial_path: &[u8], byte_limit: usize) -> Result<Self> {
        Self::new_with_limit(initial_path, byte_limit)
    }

    fn new_with_limit(initial_path: &[u8], byte_limit: usize) -> Result<Self> {
        let mut interner = Self {
            states: Vec::new(),
            free_ids: Vec::new(),
            fingerprints: HashMap::new(),
            live_path_bytes: 0,
            byte_limit,
        };
        interner.intern(FollowPathValue {
            path: initial_path.to_vec(),
            identity: None,
        })?;
        Ok(interner)
    }

    fn find(&self, path: &[u8]) -> Option<FollowPathId> {
        self.find_value(&FollowPathValue {
            path: path.to_vec(),
            identity: None,
        })
    }

    fn find_value(&self, value: &FollowPathValue) -> Option<FollowPathId> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.path.hash(&mut hasher);
        value.identity.hash(&mut hasher);
        let fingerprint = hasher.finish();
        if let Some(candidates) = self.fingerprints.get(&fingerprint) {
            for candidate in candidates {
                if self
                    .states
                    .get(candidate.0)
                    .and_then(Option::as_ref)
                    .is_some_and(|state| {
                        state.path == value.path && state.identity == value.identity
                    })
                {
                    return Some(*candidate);
                }
            }
        }
        None
    }

    fn intern(&mut self, value: FollowPathValue) -> Result<(FollowPathId, bool)> {
        if let Some(path_id) = self.find_value(&value) {
            return Ok((path_id, false));
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.path.hash(&mut hasher);
        value.identity.hash(&mut hasher);
        let fingerprint = hasher.finish();
        let live_path_bytes = self
            .live_path_bytes
            .checked_add(value.path.len())
            .ok_or_else(|| follow_path_bytes_limit_error(self.byte_limit))?;
        if live_path_bytes > self.byte_limit {
            return Err(follow_path_bytes_limit_error(self.byte_limit));
        }
        let path_id = self
            .free_ids
            .pop()
            .unwrap_or_else(|| FollowPathId(self.states.len()));
        if path_id.0 == self.states.len() {
            self.states.push(None);
        }
        self.states[path_id.0] = Some(FollowPathState {
            query: HistoryPathQuery::compile(std::slice::from_ref(&value.path)),
            path: value.path,
            identity: value.identity,
            users: 0,
        });
        self.fingerprints
            .entry(fingerprint)
            .or_default()
            .push(path_id);
        self.live_path_bytes = live_path_bytes;
        Ok((path_id, true))
    }

    fn state(&self, path_id: FollowPathId) -> Result<&FollowPathState> {
        self.states
            .get(path_id.0)
            .and_then(Option::as_ref)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "--follow path state metadata missing".into(),
            })
    }

    fn retain(&mut self, path_id: FollowPathId) -> Result<()> {
        let state = self
            .states
            .get_mut(path_id.0)
            .and_then(Option::as_mut)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "--follow path state metadata missing".into(),
            })?;
        state.users = state.users.checked_add(1).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "--follow path state reference count overflow".into(),
        })?;
        Ok(())
    }

    fn release(&mut self, path_id: FollowPathId) -> Result<()> {
        let should_evict = self
            .states
            .get(path_id.0)
            .and_then(Option::as_ref)
            .map(|state| state.users == 1)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "--follow path state metadata missing".into(),
            })?;
        if !should_evict {
            let state = self
                .states
                .get_mut(path_id.0)
                .and_then(Option::as_mut)
                .expect("follow path state was checked above");
            state.users -= 1;
            return Ok(());
        }
        let state = self.states[path_id.0]
            .take()
            .expect("follow path state was checked above");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        state.path.hash(&mut hasher);
        state.identity.hash(&mut hasher);
        let fingerprint = hasher.finish();
        if let Entry::Occupied(mut entry) = self.fingerprints.entry(fingerprint) {
            entry.get_mut().retain(|candidate| *candidate != path_id);
            if entry.get().is_empty() {
                entry.remove();
            }
        }
        self.live_path_bytes -= state.path.len();
        self.free_ids.push(path_id);
        Ok(())
    }
}

fn follow_path_bytes_limit_error(limit: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("--follow exceeded the bounded live path byte limit ({limit})"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FollowBranchState {
    candidate_index: usize,
    path_id: FollowPathId,
    topology_rank: usize,
    priority: u64,
    direction: FollowDirection,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct FollowStateKey {
    candidate_index: usize,
    path_id: FollowPathId,
}

struct FollowStateBudget {
    limit: usize,
    scheduled: HashSet<FollowStateKey>,
}

impl FollowStateBudget {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            scheduled: HashSet::with_capacity(limit.min(FOLLOW_BRANCH_STATE_LIMIT)),
        }
    }

    fn schedule(&mut self, key: FollowStateKey) -> Result<bool> {
        if self.scheduled.contains(&key) {
            return Ok(false);
        }
        self.ensure_capacity()?;
        self.scheduled.insert(key);
        Ok(true)
    }

    fn ensure_capacity(&self) -> Result<()> {
        if self.scheduled.len() >= self.limit {
            return Err(follow_state_limit_error(self.limit));
        }
        Ok(())
    }

    fn complete(&mut self, key: FollowStateKey) {
        self.scheduled.remove(&key);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.scheduled.len()
    }
}

fn follow_state_limit_error(limit: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("--follow exceeded the bounded live branch state limit ({limit})"),
    }
}

impl Ord for FollowBranchState {
    fn cmp(&self, other: &Self) -> Ordering {
        let rank_order = match self.direction {
            FollowDirection::Forward => self.topology_rank.cmp(&other.topology_rank),
            FollowDirection::Reverse => other.topology_rank.cmp(&self.topology_rank),
        };
        rank_order
            .then_with(|| other.priority.cmp(&self.priority))
            .then_with(|| other.candidate_index.cmp(&self.candidate_index))
            .then_with(|| other.path_id.0.cmp(&self.path_id.0))
    }
}

impl PartialOrd for FollowBranchState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct FollowRankFrame {
    candidate_index: usize,
    expanded: bool,
}

fn follow_candidate_topology_ranks(
    candidates: &[HistoryPathBuildCandidate],
    candidate_indexes_by_id: &[usize],
) -> Vec<usize> {
    let mut ranks: Vec<usize> = vec![0; candidates.len()];
    let mut visiting = vec![false; candidates.len()];
    for start in 0..candidates.len() {
        if ranks[start] != 0 {
            continue;
        }
        let mut stack = vec![FollowRankFrame {
            candidate_index: start,
            expanded: false,
        }];
        while let Some(frame) = stack.pop() {
            if ranks[frame.candidate_index] != 0 {
                continue;
            }
            if frame.expanded {
                let candidate = &candidates[frame.candidate_index];
                let mut rank: usize = 1;
                for parent_id_index in candidate
                    .parent_indices
                    .iter()
                    .take(candidate.traversal_parent_count)
                {
                    let parent_index = candidate_indexes_by_id
                        .get(*parent_id_index)
                        .copied()
                        .unwrap_or(usize::MAX);
                    if parent_index != usize::MAX {
                        rank = rank.max(ranks[parent_index].saturating_add(1));
                    }
                }
                ranks[frame.candidate_index] = rank;
                visiting[frame.candidate_index] = false;
                continue;
            }
            if visiting[frame.candidate_index] {
                continue;
            }
            visiting[frame.candidate_index] = true;
            stack.push(FollowRankFrame {
                candidate_index: frame.candidate_index,
                expanded: true,
            });
            let candidate = &candidates[frame.candidate_index];
            for parent_id_index in candidate
                .parent_indices
                .iter()
                .take(candidate.traversal_parent_count)
                .rev()
            {
                let parent_index = candidate_indexes_by_id
                    .get(*parent_id_index)
                    .copied()
                    .unwrap_or(usize::MAX);
                if parent_index != usize::MAX && ranks[parent_index] == 0 && !visiting[parent_index]
                {
                    stack.push(FollowRankFrame {
                        candidate_index: parent_index,
                        expanded: false,
                    });
                }
            }
        }
    }
    ranks
}

struct FollowPathTransition {
    changed: bool,
    parent_path: FollowPathCandidate,
}

enum FollowPathCandidate {
    Existing(FollowPathId),
    Owned(FollowPathValue),
}

impl HistoryPathFollowPolicy {
    pub(crate) fn for_pathspecs(pathspecs: &[Vec<u8>], direction: FollowDirection) -> Result<Self> {
        let [path] = pathspecs else {
            return Err(CliError::Fatal {
                code: 128,
                message: "--follow requires exactly one pathspec".into(),
            });
        };
        if path.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "--follow requires exactly one pathspec".into(),
            });
        }
        if follow_pathspec_has_exclude_magic(path) {
            return Err(CliError::Fatal {
                code: 128,
                message: "--follow requires exactly one pathspec".into(),
            });
        }
        let (initial_path, literal) = if let Some(rest) = path.strip_prefix(b":(") {
            let Some(close) = rest.iter().position(|byte| *byte == b')') else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "pathspec magic not supported by --follow: 'unknown'".into(),
                });
            };
            let magic = &rest[..close];
            let mut literal = false;
            let mut unsupported = Vec::new();
            for token in magic.split(|byte| *byte == b',') {
                match token {
                    b"top" => {}
                    b"literal" => literal = true,
                    value if !value.is_empty() => {
                        unsupported.push(value);
                    }
                    _ => {}
                }
            }
            if !unsupported.is_empty() {
                unsupported.sort_by(|left, right| {
                    follow_magic_diagnostic_rank(left)
                        .cmp(&follow_magic_diagnostic_rank(right))
                        .then_with(|| left.cmp(right))
                });
                let unsupported = unsupported
                    .into_iter()
                    .map(follow_magic_diagnostic_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("pathspec magic not supported by --follow: {unsupported}"),
                });
            }
            (rest[close + 1..].to_vec(), literal)
        } else if let Some(rest) = path.strip_prefix(b":/") {
            (rest.to_vec(), false)
        } else if path.starts_with(b":") {
            return Err(CliError::Fatal {
                code: 128,
                message: "pathspec magic not supported by --follow: 'unknown'".into(),
            });
        } else {
            (path.clone(), false)
        };
        if initial_path.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "--follow requires exactly one pathspec".into(),
            });
        }
        if !literal
            && initial_path
                .iter()
                .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
        {
            return Err(CliError::Fatal {
                code: 128,
                message: "pathspec magic not supported by --follow: 'glob'".into(),
            });
        }
        Ok(Self {
            initial_path,
            direction,
        })
    }
}

fn follow_magic_diagnostic_rank(token: &[u8]) -> usize {
    match token {
        b"glob" => 0,
        b"icase" => 1,
        b"exclude" => 2,
        b"attr" => 3,
        _ => usize::MAX,
    }
}

fn follow_pathspec_has_exclude_magic(path: &[u8]) -> bool {
    let Some(rest) = path.strip_prefix(b":(") else {
        return path.starts_with(b":!") || path.starts_with(b":^");
    };
    let Some(close) = rest.iter().position(|byte| *byte == b')') else {
        return false;
    };
    rest[..close]
        .split(|byte| *byte == b',')
        .any(|token| token == b"exclude")
}

fn follow_magic_diagnostic_name(token: &[u8]) -> String {
    match token {
        b"exclude" => "'exclude' (mnemonic: '!')".into(),
        _ => format!("'{}'", String::from_utf8_lossy(token)),
    }
}

pub(crate) struct HistoryPathSelection {
    pub(crate) ids: Vec<ObjectId>,
    pub(crate) commits: Vec<HistoryPathCommitCandidate>,
    pub(crate) all_candidate_indices_by_id: Vec<usize>,
    pub(crate) child_metadata: Option<HistoryPathChildMetadata>,
    pub(crate) tree_indices: Vec<usize>,
    pub(crate) author_timestamps: Vec<Option<i64>>,
    pub(crate) committer_timestamps: Vec<i64>,
    pub(crate) parent_indices: Vec<Vec<usize>>,
    pub(crate) parent_treesame: Vec<Vec<bool>>,
    pub(crate) rewritten_parent_indices: Vec<Vec<usize>>,
    pub(crate) follow_candidate_rank: Option<Vec<usize>>,
}

pub(crate) struct HistoryPathChildMetadata {
    pub(crate) all_candidate_id_indices: Vec<usize>,
    pub(crate) all_parent_indices: Vec<Vec<usize>>,
    pub(crate) all_traversal_parent_counts: Vec<usize>,
    pub(crate) child_candidate_universe: Vec<bool>,
    pub(crate) all_author_timestamps: Vec<Option<i64>>,
    pub(crate) all_committer_timestamps: Vec<i64>,
}

const HISTORY_PATH_CHANGE_CACHE_ENTRY_LIMIT: usize = 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct HistoryTreeChangeKey {
    old_tree: Option<ObjectId>,
    new_tree: Option<ObjectId>,
    pathspec_fingerprint: u64,
}

struct HistoryTreeChangeCache {
    entries: HashMap<HistoryTreeChangeKey, bool>,
    fifo: VecDeque<HistoryTreeChangeKey>,
}

impl HistoryTreeChangeCache {
    fn new() -> Self {
        Self {
            entries: HashMap::with_capacity(HISTORY_PATH_CHANGE_CACHE_ENTRY_LIMIT),
            fifo: VecDeque::with_capacity(HISTORY_PATH_CHANGE_CACHE_ENTRY_LIMIT),
        }
    }

    fn get(&self, key: &HistoryTreeChangeKey) -> Option<bool> {
        self.entries.get(key).copied()
    }

    fn insert(&mut self, key: HistoryTreeChangeKey, value: bool) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key, value);
            return;
        }
        if self.entries.len() >= HISTORY_PATH_CHANGE_CACHE_ENTRY_LIMIT
            && let Some(evicted) = self.fifo.pop_front()
        {
            self.entries.remove(&evicted);
        }
        self.fifo.push_back(key.clone());
        self.entries.insert(key, value);
    }
}

fn intern_history_path_id(
    ids: &mut Vec<ObjectId>,
    id_indices: &mut HashMap<ObjectId, usize>,
    id: ObjectId,
) -> usize {
    if let Some(index) = id_indices.get(&id).copied() {
        return index;
    }
    let index = ids.len();
    id_indices.insert(id.clone(), index);
    ids.push(id);
    index
}

fn history_follow_find_entry(
    store: &LooseObjectStore,
    tree_id: &ObjectId,
    path: &[u8],
) -> Result<Option<(TreeMode, ObjectId)>> {
    let mut current_tree = tree_id.clone();
    let components = path
        .split(|byte| *byte == b'/')
        .filter(|component| !component.is_empty());
    let mut components = components.peekable();
    while let Some(component) = components.next() {
        let tree = read_history_tree_object(store, &current_tree)?;
        let mut cursor = 0;
        let mut found = None;
        while let Some(entry) = next_history_tree_entry(
            Some(tree.content.as_slice()),
            &mut cursor,
            store.algorithm(),
        )? {
            if entry.name == component {
                found = Some((entry.mode, entry.id.clone()));
                break;
            }
        }
        let Some((mode, id)) = found else {
            return Ok(None);
        };
        if components.peek().is_some() {
            if mode != TreeMode::Tree {
                return Ok(None);
            }
            current_tree = id;
        } else {
            return Ok(Some((mode, id)));
        }
    }
    Ok(None)
}

fn history_follow_find_path_in_changed_pair(
    store: &LooseObjectStore,
    old_tree: &ObjectId,
    new_tree: &ObjectId,
    target_mode: TreeMode,
    target_id: &ObjectId,
    current_path: &[u8],
    direction: FollowDirection,
) -> Result<Option<Vec<u8>>> {
    let mut prefix = Vec::new();
    let mut active_pairs = HashSet::new();
    history_follow_find_path_in_changed_pair_at(
        store,
        Some(old_tree),
        Some(new_tree),
        target_mode,
        target_id,
        current_path,
        &mut prefix,
        &mut active_pairs,
        direction,
    )
}

fn history_follow_find_path_in_changed_pair_at(
    store: &LooseObjectStore,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
    target_mode: TreeMode,
    target_id: &ObjectId,
    current_path: &[u8],
    prefix: &mut Vec<u8>,
    active_pairs: &mut HashSet<(Option<ObjectId>, Option<ObjectId>)>,
    direction: FollowDirection,
) -> Result<Option<Vec<u8>>> {
    if old_tree == new_tree {
        return Ok(None);
    }
    let pair = (old_tree.cloned(), new_tree.cloned());
    if !active_pairs.insert(pair.clone()) {
        return Ok(None);
    }
    let old_object = old_tree
        .map(|id| read_history_tree_object(store, id))
        .transpose()?;
    let new_object = new_tree
        .map(|id| read_history_tree_object(store, id))
        .transpose()?;
    let old_content = old_object.as_ref().map(|object| object.content.as_slice());
    let new_content = new_object.as_ref().map(|object| object.content.as_slice());
    let mut old_cursor = 0;
    let mut new_cursor = 0;
    let mut old_entry = next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
    let mut new_entry = next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
    let result = loop {
        let Some(old_value) = old_entry.take() else {
            if matches!(direction, FollowDirection::Reverse) {
                let reverse_result = loop {
                    let Some(new_value) = new_entry.take() else {
                        break None;
                    };
                    if let Some(path) = history_follow_find_entry_at(
                        store,
                        &new_value,
                        None,
                        target_mode,
                        target_id,
                        current_path,
                        prefix,
                        active_pairs,
                        direction,
                    )? {
                        break Some(path);
                    }
                    new_entry =
                        next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
                };
                if reverse_result.is_some() {
                    break reverse_result;
                }
            }
            break None;
        };
        let Some(new_value) = new_entry.take() else {
            if matches!(direction, FollowDirection::Forward) {
                if let Some(path) = history_follow_find_entry_at(
                    store,
                    &old_value,
                    None,
                    target_mode,
                    target_id,
                    current_path,
                    prefix,
                    active_pairs,
                    direction,
                )? {
                    break Some(path);
                }
            } else if let Some(path) = history_follow_find_entry_at(
                store,
                &old_value,
                None,
                target_mode,
                target_id,
                current_path,
                prefix,
                active_pairs,
                FollowDirection::Forward,
            )? {
                break Some(path);
            }
            old_entry = next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
            continue;
        };
        if old_value.name < new_value.name {
            if matches!(direction, FollowDirection::Forward) {
                if let Some(path) = history_follow_find_entry_at(
                    store,
                    &old_value,
                    None,
                    target_mode,
                    target_id,
                    current_path,
                    prefix,
                    active_pairs,
                    direction,
                )? {
                    break Some(path);
                }
            } else if let Some(path) = history_follow_find_entry_at(
                store,
                &old_value,
                None,
                target_mode,
                target_id,
                current_path,
                prefix,
                active_pairs,
                FollowDirection::Forward,
            )? {
                break Some(path);
            }
            old_entry = next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
            new_entry = Some(new_value);
            continue;
        }
        if new_value.name < old_value.name {
            if matches!(direction, FollowDirection::Reverse) {
                if let Some(path) = history_follow_find_entry_at(
                    store,
                    &new_value,
                    None,
                    target_mode,
                    target_id,
                    current_path,
                    prefix,
                    active_pairs,
                    direction,
                )? {
                    break Some(path);
                }
            }
            old_entry = Some(old_value);
            new_entry = next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
            continue;
        }
        if old_value.mode == new_value.mode && old_value.id == new_value.id {
            old_entry = next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
            new_entry = next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
            continue;
        }
        let path_start = prefix.len();
        let target_entry = match direction {
            FollowDirection::Forward => &old_value,
            FollowDirection::Reverse => &new_value,
        };
        prefix.extend_from_slice(target_entry.name);
        let result = if target_entry.mode == target_mode
            && target_entry.id == *target_id
            && prefix.as_slice() != current_path
        {
            Some(prefix.clone())
        } else if target_entry.mode == TreeMode::Tree {
            prefix.push(b'/');
            let (next_old_tree, next_new_tree) = match direction {
                FollowDirection::Forward => (
                    Some(&old_value.id),
                    (new_value.mode == TreeMode::Tree).then_some(&new_value.id),
                ),
                FollowDirection::Reverse => (
                    (old_value.mode == TreeMode::Tree).then_some(&old_value.id),
                    Some(&new_value.id),
                ),
            };
            history_follow_find_path_in_changed_pair_at(
                store,
                next_old_tree,
                next_new_tree,
                target_mode,
                target_id,
                current_path,
                prefix,
                active_pairs,
                direction,
            )?
        } else {
            None
        };
        prefix.truncate(path_start);
        if result.is_some() {
            break result;
        }
        old_entry = next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
        new_entry = next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
    };
    active_pairs.remove(&pair);
    Ok(result)
}

fn history_follow_find_entry_at(
    store: &LooseObjectStore,
    entry: &zmin_git_core::TreeEntryRef<'_>,
    other_tree: Option<&ObjectId>,
    target_mode: TreeMode,
    target_id: &ObjectId,
    current_path: &[u8],
    prefix: &mut Vec<u8>,
    active_pairs: &mut HashSet<(Option<ObjectId>, Option<ObjectId>)>,
    direction: FollowDirection,
) -> Result<Option<Vec<u8>>> {
    let path_start = prefix.len();
    prefix.extend_from_slice(entry.name);
    let result =
        if entry.mode == target_mode && entry.id == *target_id && prefix.as_slice() != current_path
        {
            Some(prefix.clone())
        } else if entry.mode == TreeMode::Tree {
            prefix.push(b'/');
            let (old_tree, new_tree) = match direction {
                FollowDirection::Forward => (Some(&entry.id), other_tree),
                FollowDirection::Reverse => (other_tree, Some(&entry.id)),
            };
            history_follow_find_path_in_changed_pair_at(
                store,
                old_tree,
                new_tree,
                target_mode,
                target_id,
                current_path,
                prefix,
                active_pairs,
                direction,
            )?
        } else {
            None
        };
    prefix.truncate(path_start);
    Ok(result)
}

fn history_follow_path_transition(
    store: &LooseObjectStore,
    paths: &FollowPathInterner,
    child_tree: &ObjectId,
    parent_tree: Option<&ObjectId>,
    path_id: FollowPathId,
    retain_parent_path: bool,
    cache: &mut HistoryTreeChangeCache,
    missing_policy: RevListTreeMissingPolicy<'_>,
    direction: FollowDirection,
) -> Result<FollowPathTransition> {
    let state = paths.state(path_id)?;
    let mut tree_path = Vec::new();
    let changed = history_tree_changed(
        store,
        &state.query,
        parent_tree,
        Some(child_tree),
        &mut tree_path,
        cache,
        missing_policy,
    )?;
    let Some(parent_tree) = parent_tree else {
        return Ok(FollowPathTransition {
            changed,
            parent_path: FollowPathCandidate::Existing(path_id),
        });
    };
    if !retain_parent_path {
        return Ok(FollowPathTransition {
            changed,
            parent_path: FollowPathCandidate::Existing(path_id),
        });
    }
    let transition_tree = match direction {
        FollowDirection::Forward => child_tree,
        FollowDirection::Reverse => parent_tree,
    };
    let reverse_child_identity = if matches!(direction, FollowDirection::Reverse) {
        history_follow_find_entry(store, child_tree, &state.path)?.and_then(|(mode, id)| {
            matches!(
                mode,
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink
            )
            .then_some(FollowPathEntryIdentity { mode, id })
        })
    } else {
        None
    };
    let current_entry = history_follow_find_entry(store, transition_tree, &state.path)?;
    let tracked_identity = state.identity.clone();
    let Some(current_entry) = current_entry else {
        if matches!(direction, FollowDirection::Reverse) && reverse_child_identity.is_none() {
            return Ok(FollowPathTransition {
                changed,
                parent_path: FollowPathCandidate::Existing(path_id),
            });
        }
        let Some(identity) = reverse_child_identity.clone().or(tracked_identity) else {
            return Ok(FollowPathTransition {
                changed,
                parent_path: FollowPathCandidate::Existing(path_id),
            });
        };
        let previous_path = history_follow_find_path_in_changed_pair(
            store,
            parent_tree,
            child_tree,
            identity.mode,
            &identity.id,
            &state.path,
            direction,
        )?;
        let predecessor_found = previous_path.is_some();
        let parent_path = previous_path
            .map(|path| {
                FollowPathCandidate::Owned(FollowPathValue {
                    path,
                    identity: Some(identity.clone()),
                })
            })
            .or_else(|| {
                reverse_child_identity.map(|_| {
                    FollowPathCandidate::Owned(FollowPathValue {
                        path: state.path.clone(),
                        identity: Some(identity),
                    })
                })
            })
            .unwrap_or(FollowPathCandidate::Existing(path_id));
        return Ok(FollowPathTransition {
            changed: changed || predecessor_found,
            parent_path,
        });
    };
    if !matches!(
        current_entry.0,
        TreeMode::File | TreeMode::Executable | TreeMode::Symlink
    ) {
        return Ok(FollowPathTransition {
            changed,
            parent_path: FollowPathCandidate::Existing(path_id),
        });
    }
    let identity = reverse_child_identity.unwrap_or(FollowPathEntryIdentity {
        mode: current_entry.0,
        id: current_entry.1,
    });
    let other_path_present = match direction {
        FollowDirection::Forward => {
            history_follow_find_entry(store, parent_tree, &state.path)?.is_some()
        }
        FollowDirection::Reverse => {
            history_follow_find_entry(store, child_tree, &state.path)?.is_some()
        }
    };
    if other_path_present {
        return Ok(FollowPathTransition {
            changed,
            parent_path: FollowPathCandidate::Owned(FollowPathValue {
                path: state.path.clone(),
                identity: Some(identity),
            }),
        });
    }
    let previous_path = history_follow_find_path_in_changed_pair(
        store,
        parent_tree,
        child_tree,
        identity.mode,
        &identity.id,
        &state.path,
        direction,
    )?;
    Ok(FollowPathTransition {
        changed,
        parent_path: previous_path
            .map(|path| {
                FollowPathCandidate::Owned(FollowPathValue {
                    path,
                    identity: Some(identity.clone()),
                })
            })
            .unwrap_or_else(|| {
                FollowPathCandidate::Owned(FollowPathValue {
                    path: state.path.clone(),
                    identity: Some(identity),
                })
            }),
    })
}

fn schedule_follow_branch(
    pending: &mut BinaryHeap<FollowBranchState>,
    budget: &mut FollowStateBudget,
    paths: &mut FollowPathInterner,
    candidate_index: usize,
    topology_rank: usize,
    path: FollowPathCandidate,
    priority: u64,
    direction: FollowDirection,
) -> Result<bool> {
    let (path_id, inserted) = match path {
        FollowPathCandidate::Existing(path_id) => (path_id, false),
        FollowPathCandidate::Owned(path) => {
            if let Some(path_id) = paths.find_value(&path) {
                (path_id, false)
            } else {
                budget.ensure_capacity()?;
                let path_id = paths.intern(path)?.0;
                (path_id, true)
            }
        }
    };
    let key = FollowStateKey {
        candidate_index,
        path_id,
    };
    let accepted = budget.schedule(key)?;
    if !accepted {
        if inserted {
            paths.release(path_id)?;
        }
        return Ok(false);
    }
    if let Err(error) = paths.retain(path_id) {
        budget.complete(key);
        if inserted {
            paths.release(path_id)?;
        }
        return Err(error);
    }
    pending.push(FollowBranchState {
        candidate_index,
        path_id,
        topology_rank,
        priority,
        direction,
    });
    Ok(true)
}

#[derive(Clone, Copy, Debug)]
struct FollowChildEdge {
    child_index: usize,
    parent_position: usize,
}

struct FollowChildAdjacency {
    offsets: Vec<usize>,
    edges: Vec<FollowChildEdge>,
}

struct HistoryPathFollowResult {
    changed_by_parent: Vec<Vec<bool>>,
    candidate_rank: Vec<usize>,
}

impl FollowChildAdjacency {
    fn new(candidates: &[HistoryPathBuildCandidate], candidate_indexes_by_id: &[usize]) -> Self {
        let mut counts = vec![0; candidates.len()];
        for candidate in candidates {
            for parent_id_index in candidate
                .parent_indices
                .iter()
                .take(candidate.traversal_parent_count)
            {
                let parent_index = candidate_indexes_by_id
                    .get(*parent_id_index)
                    .copied()
                    .unwrap_or(usize::MAX);
                if parent_index != usize::MAX {
                    counts[parent_index] += 1;
                }
            }
        }
        let mut offsets = vec![0; candidates.len() + 1];
        for (index, count) in counts.into_iter().enumerate() {
            offsets[index + 1] = offsets[index] + count;
        }
        let mut next = offsets[..candidates.len()].to_vec();
        let mut edges = vec![
            FollowChildEdge {
                child_index: 0,
                parent_position: 0,
            };
            offsets[candidates.len()]
        ];
        for (child_index, candidate) in candidates.iter().enumerate() {
            for (parent_position, parent_id_index) in candidate
                .parent_indices
                .iter()
                .take(candidate.traversal_parent_count)
                .enumerate()
            {
                let parent_index = candidate_indexes_by_id
                    .get(*parent_id_index)
                    .copied()
                    .unwrap_or(usize::MAX);
                if parent_index == usize::MAX {
                    continue;
                }
                let edge_index = next[parent_index];
                edges[edge_index] = FollowChildEdge {
                    child_index,
                    parent_position,
                };
                next[parent_index] += 1;
            }
        }
        Self { offsets, edges }
    }

    fn children(&self, parent_index: usize) -> &[FollowChildEdge] {
        let start = self.offsets[parent_index];
        let end = self.offsets[parent_index + 1];
        &self.edges[start..end]
    }
}

fn follow_reverse_boundary_candidates<'a>(
    candidates: &'a [HistoryPathBuildCandidate],
    candidate_indexes_by_id: &'a [usize],
) -> impl Iterator<Item = usize> + 'a {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            let has_candidate_parent = candidate
                .parent_indices
                .iter()
                .take(candidate.traversal_parent_count)
                .any(|parent_id_index| {
                    candidate_indexes_by_id
                        .get(*parent_id_index)
                        .is_some_and(|index| *index != usize::MAX)
                });
            (!has_candidate_parent).then_some(candidate_index)
        })
}

fn collect_history_follow_changed_by_parent(
    store: &LooseObjectStore,
    ids: &[ObjectId],
    candidates: &[HistoryPathBuildCandidate],
    candidate_indexes_by_id: &[usize],
    positive_root_indices: &[usize],
    follow: &HistoryPathFollowPolicy,
    cache: &mut HistoryTreeChangeCache,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<HistoryPathFollowResult> {
    if matches!(follow.direction, FollowDirection::Reverse) {
        return collect_history_follow_changed_by_parent_reverse(
            store,
            ids,
            candidates,
            candidate_indexes_by_id,
            follow,
            cache,
            missing_policy,
        );
    }
    let mut paths = FollowPathInterner::new(&follow.initial_path)?;
    let initial_path_id = FollowPathId(0);
    let mut pending = BinaryHeap::with_capacity(positive_root_indices.len());
    let mut budget = FollowStateBudget::new(FOLLOW_BRANCH_STATE_LIMIT);
    let topology_ranks = follow_candidate_topology_ranks(candidates, candidate_indexes_by_id);
    let mut priority = 0_u64;
    for root_id_index in positive_root_indices {
        let candidate_index = candidate_indexes_by_id
            .get(*root_id_index)
            .copied()
            .unwrap_or(usize::MAX);
        if candidate_index == usize::MAX {
            continue;
        }
        schedule_follow_branch(
            &mut pending,
            &mut budget,
            &mut paths,
            candidate_index,
            topology_ranks[candidate_index],
            FollowPathCandidate::Existing(initial_path_id),
            priority,
            FollowDirection::Forward,
        )?;
        priority += 1;
    }
    let mut changed_by_parent = candidates
        .iter()
        .map(|candidate| vec![false; candidate.traversal_parent_count])
        .collect::<Vec<_>>();
    let mut candidate_rank = vec![usize::MAX; candidates.len()];
    let mut next_candidate_rank = 0;
    // Parent ranks are strictly lower than their traversed children, so the
    // heap drains every possible child state before a completed key can be
    // encountered again. Completed keys therefore need not remain in an
    // unbounded historical set.
    while let Some(branch) = pending.pop() {
        if candidate_rank[branch.candidate_index] == usize::MAX {
            candidate_rank[branch.candidate_index] = next_candidate_rank;
            next_candidate_rank += 1;
        }
        let branch_key = FollowStateKey {
            candidate_index: branch.candidate_index,
            path_id: branch.path_id,
        };
        let candidate = candidates
            .get(branch.candidate_index)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "--follow branch candidate metadata missing".into(),
            })?;
        if candidate.traversal_parent_count == 0 {
            let state = paths.state(branch.path_id)?;
            let mut tree_path = Vec::new();
            changed_by_parent[branch.candidate_index].push(history_tree_changed(
                store,
                &state.query,
                None,
                Some(&ids[candidate.tree_index]),
                &mut tree_path,
                cache,
                missing_policy,
            )?);
            budget.complete(branch_key);
            paths.release(branch.path_id)?;
            continue;
        }
        for (parent_position, parent_id_index) in candidate
            .parent_indices
            .iter()
            .take(candidate.traversal_parent_count)
            .enumerate()
        {
            let parent_candidate_index = candidate_indexes_by_id
                .get(*parent_id_index)
                .copied()
                .unwrap_or(usize::MAX);
            let owned_parent_tree = if parent_candidate_index == usize::MAX {
                read_commit_tree_with_missing_policy(store, &ids[*parent_id_index], missing_policy)?
            } else {
                None
            };
            let parent_tree = if parent_candidate_index == usize::MAX {
                owned_parent_tree.as_ref()
            } else {
                Some(&ids[candidates[parent_candidate_index].tree_index])
            };
            let transition = history_follow_path_transition(
                store,
                &mut paths,
                &ids[candidate.tree_index],
                parent_tree,
                branch.path_id,
                parent_candidate_index != usize::MAX,
                cache,
                missing_policy,
                FollowDirection::Forward,
            )?;
            if let Some(changed) = changed_by_parent
                .get_mut(branch.candidate_index)
                .and_then(|changes| changes.get_mut(parent_position))
            {
                *changed |= transition.changed;
            }
            if parent_candidate_index != usize::MAX {
                schedule_follow_branch(
                    &mut pending,
                    &mut budget,
                    &mut paths,
                    parent_candidate_index,
                    topology_ranks[parent_candidate_index],
                    transition.parent_path,
                    priority,
                    FollowDirection::Forward,
                )?;
                priority += 1;
            }
        }
        budget.complete(branch_key);
        paths.release(branch.path_id)?;
    }
    Ok(HistoryPathFollowResult {
        changed_by_parent,
        candidate_rank,
    })
}

fn collect_history_follow_changed_by_parent_reverse(
    store: &LooseObjectStore,
    ids: &[ObjectId],
    candidates: &[HistoryPathBuildCandidate],
    candidate_indexes_by_id: &[usize],
    follow: &HistoryPathFollowPolicy,
    cache: &mut HistoryTreeChangeCache,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<HistoryPathFollowResult> {
    let mut paths = FollowPathInterner::new(&follow.initial_path)?;
    let initial_path_id = FollowPathId(0);
    let mut pending = BinaryHeap::new();
    let mut budget = FollowStateBudget::new(FOLLOW_BRANCH_STATE_LIMIT);
    let topology_ranks = follow_candidate_topology_ranks(candidates, candidate_indexes_by_id);
    let child_adjacency = FollowChildAdjacency::new(candidates, candidate_indexes_by_id);
    let mut changed_by_parent = candidates
        .iter()
        .map(|candidate| vec![false; candidate.traversal_parent_count])
        .collect::<Vec<_>>();
    let mut candidate_rank = vec![usize::MAX; candidates.len()];
    let mut next_candidate_rank = 0;
    let mut priority = 0_u64;
    for candidate_index in follow_reverse_boundary_candidates(candidates, candidate_indexes_by_id) {
        schedule_follow_branch(
            &mut pending,
            &mut budget,
            &mut paths,
            candidate_index,
            topology_ranks[candidate_index],
            FollowPathCandidate::Existing(initial_path_id),
            priority,
            FollowDirection::Reverse,
        )?;
        priority += 1;
    }

    while let Some(branch) = pending.pop() {
        if candidate_rank[branch.candidate_index] == usize::MAX {
            candidate_rank[branch.candidate_index] = next_candidate_rank;
            next_candidate_rank += 1;
        }
        let branch_key = FollowStateKey {
            candidate_index: branch.candidate_index,
            path_id: branch.path_id,
        };
        let candidate = candidates
            .get(branch.candidate_index)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "--follow branch candidate metadata missing".into(),
            })?;
        let mut reverse_root_visible = false;
        if candidate.traversal_parent_count == 0 {
            let state = paths.state(branch.path_id)?;
            let mut tree_path = Vec::new();
            reverse_root_visible = history_tree_changed(
                store,
                &state.query,
                None,
                Some(&ids[candidate.tree_index]),
                &mut tree_path,
                cache,
                missing_policy,
            )?;
        }
        // Reverse follow uses graph boundaries as state seeds. A boundary is
        // visible only when the requested path exists at that root; otherwise
        // visibility begins on a changed parent-to-child edge.
        for edge in child_adjacency.children(branch.candidate_index) {
            let child = candidates
                .get(edge.child_index)
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "--follow child candidate metadata missing".into(),
                })?;
            let transition = history_follow_path_transition(
                store,
                &paths,
                &ids[child.tree_index],
                Some(&ids[candidate.tree_index]),
                branch.path_id,
                true,
                cache,
                missing_policy,
                FollowDirection::Reverse,
            )?;
            if let Some(changed) = changed_by_parent
                .get_mut(edge.child_index)
                .and_then(|changes| changes.get_mut(edge.parent_position))
            {
                *changed |= transition.changed;
            }
            schedule_follow_branch(
                &mut pending,
                &mut budget,
                &mut paths,
                edge.child_index,
                topology_ranks[edge.child_index],
                transition.parent_path,
                priority,
                FollowDirection::Reverse,
            )?;
            priority += 1;
        }
        if candidate.traversal_parent_count == 0 {
            changed_by_parent[branch.candidate_index].push(reverse_root_visible);
        }
        budget.complete(branch_key);
        paths.release(branch.path_id)?;
    }
    Ok(HistoryPathFollowResult {
        changed_by_parent,
        candidate_rank,
    })
}

pub(crate) fn collect_history_path_selection(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    query: &HistoryPathQuery,
    policy: HistoryPathSelectionPolicy<'_>,
) -> Result<HistoryPathSelection> {
    let path_selection_missing_policy = policy.missing_policy.for_path_selection();
    let excluded = collect_excluded_commits_uncached_with_parent_policy(
        repo,
        store,
        &revs.exclude,
        revs.exclude_first_parent_only,
    )?;
    let mut excluded = excluded;
    excluded.extend(policy.additional_excluded.iter().cloned());
    let root_capacity = root_traversal_capacity_hint(revs.include.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut ids = Vec::with_capacity(commit_output_capacity_hint(None, root_capacity));
    let mut id_indices = HashMap::with_capacity(root_capacity);
    let mut positive_root_indices = Vec::with_capacity(revs.include.len());
    let mut sequence = 0_u64;
    for rev in &revs.include {
        let id = resolve_commitish(repo, store, rev)?;
        let id_index = intern_history_path_id(&mut ids, &mut id_indices, id.clone());
        positive_root_indices.push(id_index);
        if scheduled.insert(id_index) {
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    let shallow_commits = read_shallow_commits(repo)?;
    let mut candidates = Vec::with_capacity(commit_output_capacity_hint(None, scheduled.len()));
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        if excluded.contains(&id) {
            continue;
        }
        let id_index = intern_history_path_id(&mut ids, &mut id_indices, id);
        let tree_index = intern_history_path_id(&mut ids, &mut id_indices, pending_commit.tree);
        let parent_indices = pending_commit
            .parents
            .into_iter()
            .map(|parent| intern_history_path_id(&mut ids, &mut id_indices, parent))
            .collect::<Vec<_>>();
        let traversal_parent_count = if policy.first_parent {
            parent_indices.len().min(1)
        } else {
            parent_indices.len()
        };
        candidates.push(HistoryPathBuildCandidate {
            id_index,
            tree_index,
            parent_indices,
            traversal_parent_count,
            author_timestamp: pending_commit.author_timestamp,
            committer_timestamp: pending_commit.timestamp,
        });
        let candidate = candidates.last().expect("path candidate was appended");
        if shallow_commits.contains(&ids[candidate.id_index]) {
            continue;
        }
        reserve_commit_parent_traversal(
            &mut pending,
            &mut scheduled,
            candidate.traversal_parent_count,
        );
        for parent_index in candidate
            .parent_indices
            .iter()
            .take(candidate.traversal_parent_count)
        {
            let parent = &ids[*parent_index];
            if excluded.contains(parent) || !scheduled.insert(*parent_index) {
                continue;
            }
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, parent.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }

    let mut candidate_indexes_by_id = vec![usize::MAX; ids.len()];
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        candidate_indexes_by_id[candidate.id_index] = candidate_index;
    }
    let mut tree_cache = HistoryTreeChangeCache::new();
    let (changed_by_parent, follow_candidate_rank) = if let Some(follow) = policy.follow {
        let follow_result = collect_history_follow_changed_by_parent(
            store,
            &ids,
            &candidates,
            &candidate_indexes_by_id,
            &positive_root_indices,
            follow,
            &mut tree_cache,
            path_selection_missing_policy,
        )?;
        (
            follow_result.changed_by_parent,
            Some(follow_result.candidate_rank),
        )
    } else {
        let mut changed_by_parent = Vec::with_capacity(candidates.len());
        let mut path = Vec::new();
        for candidate in &candidates {
            let mut parent_changes = Vec::with_capacity(candidate.traversal_parent_count);
            for parent_index in candidate
                .parent_indices
                .iter()
                .take(candidate.traversal_parent_count)
            {
                let parent_tree = if let Some(&parent_candidate_index) = candidate_indexes_by_id
                    .get(*parent_index)
                    .filter(|index| **index != usize::MAX)
                {
                    Some(ids[candidates[parent_candidate_index].tree_index].clone())
                } else {
                    read_commit_tree_with_missing_policy(
                        store,
                        &ids[*parent_index],
                        path_selection_missing_policy,
                    )?
                };
                let changed = history_tree_changed(
                    store,
                    query,
                    parent_tree.as_ref(),
                    Some(&ids[candidate.tree_index]),
                    &mut path,
                    &mut tree_cache,
                    path_selection_missing_policy,
                )?;
                parent_changes.push(changed);
            }
            if candidate.traversal_parent_count == 0 {
                parent_changes.push(history_tree_changed(
                    store,
                    query,
                    None,
                    Some(&ids[candidate.tree_index]),
                    &mut path,
                    &mut tree_cache,
                    path_selection_missing_policy,
                )?);
            }
            changed_by_parent.push(parent_changes);
        }
        (changed_by_parent, None)
    };

    let mut visible = candidates
        .iter()
        .zip(changed_by_parent.iter())
        .enumerate()
        .map(|(_candidate_index, (candidate, parent_changes))| {
            let all_tree_same =
                !parent_changes.is_empty() && parent_changes.iter().all(|changed| !changed);
            if candidate.traversal_parent_count == 0 {
                parent_changes.first().copied().unwrap_or(false)
            } else if policy.follow.is_some() && policy.first_parent {
                parent_changes.first().copied().unwrap_or(false)
            } else if policy.follow.is_some() && candidate.traversal_parent_count > 1 {
                parent_changes.iter().all(|changed| *changed)
            } else if policy.full_history || policy.sparse {
                parent_changes.iter().copied().any(|changed| changed)
                    || (policy.full_history
                        && policy.rewrite_parents
                        && candidate.parent_indices.len() > 1
                        && all_tree_same)
            } else {
                parent_changes.iter().copied().all(|changed| changed)
            }
        })
        .collect::<Vec<_>>();

    if policy.sparse {
        for root_index in positive_root_indices {
            if let Some(&candidate_index) = candidate_indexes_by_id
                .get(root_index)
                .filter(|index| **index != usize::MAX)
            {
                visible[candidate_index] = true;
            }
        }
        let mut pending = candidates
            .iter()
            .enumerate()
            .filter_map(|(index, _)| visible[index].then_some(index))
            .collect::<Vec<_>>();
        let mut expanded = HashSet::with_capacity(pending.len());
        while let Some(candidate_index) = pending.pop() {
            if !expanded.insert(candidate_index) {
                continue;
            }
            let candidate = &candidates[candidate_index];
            let parent_changes = &changed_by_parent[candidate_index];
            let mut parent_indexes = Vec::new();
            if candidate.traversal_parent_count > 1 && !policy.full_history {
                let unchanged_parent_indexes = parent_changes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, changed)| (!changed).then_some(index))
                    .collect::<Vec<_>>();
                if unchanged_parent_indexes.len() == candidate.traversal_parent_count {
                    parent_indexes.push(0);
                } else if unchanged_parent_indexes.is_empty() {
                    parent_indexes.extend(0..candidate.traversal_parent_count);
                } else {
                    parent_indexes.extend(unchanged_parent_indexes);
                }
            } else {
                parent_indexes.extend(0..candidate.traversal_parent_count);
            }
            for parent_position in parent_indexes {
                let parent_index = candidate.parent_indices[parent_position];
                let parent_candidate_index = candidate_indexes_by_id[parent_index];
                if parent_candidate_index == usize::MAX || visible[parent_candidate_index] {
                    continue;
                }
                visible[parent_candidate_index] = true;
                pending.push(parent_candidate_index);
            }
        }
    }

    let mut rewritten_parent_indices = vec![Vec::new(); candidates.len()];
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        if !visible[candidate_index] {
            continue;
        }
        let mut parents = Vec::new();
        let mut emitted = HashSet::new();
        let first_parent_treesame = policy.first_parent
            && changed_by_parent[candidate_index].first().copied() == Some(false);
        if !policy.rewrite_parents {
            let parent_limit = if first_parent_treesame {
                candidate.parent_indices.len().min(1)
            } else {
                candidate.parent_indices.len()
            };
            parents.extend(candidate.parent_indices.iter().take(parent_limit).copied());
        } else if policy.follow.is_some() {
            parents.extend(candidate.parent_indices.iter().copied());
        } else if policy.sparse {
            let parent_limit = if policy.full_history {
                candidate.parent_indices.len()
            } else if first_parent_treesame {
                candidate.parent_indices.len().min(1)
            } else {
                candidate.parent_indices.len()
            };
            for parent_index in candidate.parent_indices.iter().take(parent_limit) {
                let parent_candidate_index = candidate_indexes_by_id[*parent_index];
                if policy.full_history
                    || (policy.first_parent && !first_parent_treesame)
                    || (parent_candidate_index != usize::MAX && visible[parent_candidate_index])
                {
                    parents.push(*parent_index);
                }
            }
        } else {
            for parent_index in &candidate.parent_indices {
                let mut visiting = HashSet::new();
                append_history_rewritten_parent(
                    *parent_index,
                    &visible,
                    &mut candidate_indexes_by_id,
                    &candidates,
                    &changed_by_parent,
                    policy.first_parent,
                    &mut ids,
                    &mut id_indices,
                    store,
                    &mut visiting,
                    &mut emitted,
                    &mut parents,
                )?;
            }
        }
        rewritten_parent_indices[candidate_index] = parents;
    }

    let mut all_simplified_child_excluded = vec![false; candidates.len()];
    if !policy.full_history && !policy.sparse && (policy.follow.is_none() || policy.topo_order) {
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            if candidate.parent_indices.len() < 2 {
                continue;
            }
            let unchanged_parent_positions = changed_by_parent[candidate_index]
                .iter()
                .enumerate()
                .filter_map(|(position, changed)| (!changed).then_some(position))
                .collect::<Vec<_>>();
            let hide_follow_topology_side = policy.follow.is_some() && policy.topo_order;
            if unchanged_parent_positions.is_empty()
                || (hide_follow_topology_side && unchanged_parent_positions.len() != 1)
            {
                continue;
            }
            let unchanged_parent_position = unchanged_parent_positions[0];
            for (position, parent_id_index) in candidate.parent_indices.iter().enumerate() {
                if position == unchanged_parent_position {
                    continue;
                }
                let parent_candidate_index = candidate_indexes_by_id[*parent_id_index];
                if parent_candidate_index != usize::MAX {
                    all_simplified_child_excluded[parent_candidate_index] = true;
                    visible[parent_candidate_index] = false;
                }
            }
        }
    }

    let all_candidate_id_indices = candidates
        .iter()
        .map(|candidate| candidate.id_index)
        .collect::<Vec<_>>();
    let mut all_candidate_indices_by_id = vec![usize::MAX; ids.len()];
    for (candidate_index, id_index) in all_candidate_id_indices.iter().copied().enumerate() {
        all_candidate_indices_by_id[id_index] = candidate_index;
    }
    let child_metadata = policy.children.then(|| {
        let all_parent_indices = candidates
            .iter()
            .map(|candidate| candidate.parent_indices.clone())
            .collect::<Vec<_>>();
        let all_traversal_parent_counts = candidates
            .iter()
            .map(|candidate| candidate.traversal_parent_count)
            .collect::<Vec<_>>();
        let child_candidate_universe = candidates
            .iter()
            .enumerate()
            .map(|(candidate_index, _)| {
                if policy.full_history {
                    true
                } else if policy.sparse {
                    visible[candidate_index]
                } else {
                    !all_simplified_child_excluded[candidate_index]
                }
            })
            .collect::<Vec<_>>();
        let all_author_timestamps = candidates
            .iter()
            .map(|candidate| candidate.author_timestamp)
            .collect::<Vec<_>>();
        let all_committer_timestamps = candidates
            .iter()
            .map(|candidate| candidate.committer_timestamp)
            .collect::<Vec<_>>();
        HistoryPathChildMetadata {
            all_candidate_id_indices,
            all_parent_indices,
            all_traversal_parent_counts,
            child_candidate_universe,
            all_author_timestamps,
            all_committer_timestamps,
        }
    });
    let mut commits = Vec::new();
    let mut parent_indices = Vec::new();
    let mut tree_indices = Vec::new();
    let mut author_timestamps = Vec::new();
    let mut committer_timestamps = Vec::new();
    let mut parent_treesame = Vec::new();
    let mut visible_rewritten_parent_indices = Vec::new();
    for (candidate_index, candidate) in candidates.into_iter().enumerate() {
        if !visible[candidate_index] {
            continue;
        }
        commits.push(HistoryPathCommitCandidate {
            id_index: candidate.id_index,
        });
        tree_indices.push(candidate.tree_index);
        author_timestamps.push(candidate.author_timestamp);
        committer_timestamps.push(candidate.committer_timestamp);
        parent_indices.push(candidate.parent_indices);
        parent_treesame.push(
            changed_by_parent[candidate_index]
                .iter()
                .map(|changed| !changed)
                .collect(),
        );
        visible_rewritten_parent_indices.push(rewritten_parent_indices[candidate_index].clone());
    }
    Ok(HistoryPathSelection {
        ids,
        commits,
        all_candidate_indices_by_id,
        child_metadata,
        tree_indices,
        author_timestamps,
        committer_timestamps,
        parent_indices,
        parent_treesame,
        rewritten_parent_indices: visible_rewritten_parent_indices,
        follow_candidate_rank,
    })
}

fn append_history_rewritten_parent(
    id_index: usize,
    visible: &[bool],
    candidate_indexes_by_id: &mut Vec<usize>,
    candidates: &[HistoryPathBuildCandidate],
    parent_changes: &[Vec<bool>],
    first_parent: bool,
    ids: &mut Vec<ObjectId>,
    id_indices: &mut HashMap<ObjectId, usize>,
    store: &LooseObjectStore,
    visiting: &mut HashSet<usize>,
    emitted: &mut HashSet<usize>,
    output: &mut Vec<usize>,
) -> Result<()> {
    let candidate_index = candidate_indexes_by_id[id_index];
    if candidate_index != usize::MAX && visible.get(candidate_index).copied().unwrap_or(false) {
        if first_parent
            && parent_changes
                .get(candidate_index)
                .and_then(|changes| changes.first())
                .copied()
                == Some(false)
            && let Some(parent_index) = candidates[candidate_index].parent_indices.first()
        {
            return append_history_rewritten_parent(
                *parent_index,
                visible,
                candidate_indexes_by_id,
                candidates,
                parent_changes,
                first_parent,
                ids,
                id_indices,
                store,
                visiting,
                emitted,
                output,
            );
        }
        if emitted.insert(id_index) {
            output.push(id_index);
        }
        return Ok(());
    }
    if !visiting.insert(id_index) {
        return Ok(());
    }
    if candidate_index != usize::MAX {
        for parent_index in candidates[candidate_index]
            .parent_indices
            .iter()
            .take(if first_parent { 1 } else { usize::MAX })
        {
            append_history_rewritten_parent(
                *parent_index,
                visible,
                candidate_indexes_by_id,
                candidates,
                parent_changes,
                first_parent,
                ids,
                id_indices,
                store,
                visiting,
                emitted,
                output,
            )?;
        }
    } else {
        if first_parent {
            if emitted.insert(id_index) {
                output.push(id_index);
            }
            return Ok(());
        }
        for parent in read_commit_parents_uncached(store, &ids[id_index])? {
            let parent_index = intern_history_path_id(ids, id_indices, parent);
            if candidate_indexes_by_id.len() <= parent_index {
                candidate_indexes_by_id.resize(ids.len(), usize::MAX);
            }
            append_history_rewritten_parent(
                parent_index,
                visible,
                candidate_indexes_by_id,
                candidates,
                parent_changes,
                first_parent,
                ids,
                id_indices,
                store,
                visiting,
                emitted,
                output,
            )?;
        }
    }
    Ok(())
}

fn history_tree_changed(
    store: &LooseObjectStore,
    query: &HistoryPathQuery,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
    prefix: &mut Vec<u8>,
    cache: &mut HistoryTreeChangeCache,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<bool> {
    if old_tree == new_tree {
        return Ok(false);
    }
    if !query.may_match_prefix(prefix) {
        return Ok(false);
    }
    let cache_key = prefix.is_empty().then(|| HistoryTreeChangeKey {
        old_tree: old_tree.cloned(),
        new_tree: new_tree.cloned(),
        pathspec_fingerprint: query.fingerprint(),
    });
    if let Some(key) = cache_key.as_ref()
        && let Some(changed) = cache.get(key)
    {
        return Ok(changed);
    }
    let old_object = match old_tree {
        Some(id) => read_history_tree_object_with_missing_policy(store, id, missing_policy)?,
        None => None,
    };
    let new_object = match new_tree {
        Some(id) => read_history_tree_object_with_missing_policy(store, id, missing_policy)?,
        None => None,
    };
    let old_content = old_object.as_ref().map(|object| object.content.as_slice());
    let new_content = new_object.as_ref().map(|object| object.content.as_slice());
    let mut old_cursor = 0;
    let mut new_cursor = 0;
    let mut old_entry = next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
    let mut new_entry = next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
    let mut changed = false;
    while old_entry.is_some() || new_entry.is_some() {
        match (old_entry.take(), new_entry.take()) {
            (Some(old), Some(new)) if old.name == new.name => {
                let path_start = prefix.len();
                if old.mode == TreeMode::Tree && new.mode == TreeMode::Tree {
                    prefix.extend_from_slice(old.name);
                    prefix.push(b'/');
                    changed = history_tree_changed(
                        store,
                        query,
                        Some(&old.id),
                        Some(&new.id),
                        prefix,
                        cache,
                        missing_policy,
                    )?;
                    prefix.truncate(path_start);
                } else if old.mode != new.mode || old.id != new.id {
                    prefix.extend_from_slice(old.name);
                    changed = query.matches_path(prefix);
                    if !changed && (old.mode == TreeMode::Tree || new.mode == TreeMode::Tree) {
                        prefix.push(b'/');
                        changed = history_tree_changed(
                            store,
                            query,
                            (old.mode == TreeMode::Tree).then_some(&old.id),
                            (new.mode == TreeMode::Tree).then_some(&new.id),
                            prefix,
                            cache,
                            missing_policy,
                        )?;
                    }
                    prefix.truncate(path_start);
                }
                old_entry =
                    next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
                new_entry =
                    next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
            }
            (Some(old), Some(new)) => {
                let old_name = old.name;
                let new_name = new.name;
                if old_name < new_name {
                    let path_start = prefix.len();
                    prefix.extend_from_slice(old_name);
                    changed = history_tree_entry_changed(
                        store,
                        query,
                        Some(&old.id),
                        old.mode,
                        prefix,
                        cache,
                        missing_policy,
                    )?;
                    prefix.truncate(path_start);
                    new_entry = Some(new);
                    old_entry =
                        next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
                } else {
                    let path_start = prefix.len();
                    prefix.extend_from_slice(new_name);
                    changed = history_tree_entry_changed(
                        store,
                        query,
                        Some(&new.id),
                        new.mode,
                        prefix,
                        cache,
                        missing_policy,
                    )?;
                    prefix.truncate(path_start);
                    old_entry = Some(old);
                    new_entry =
                        next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
                }
            }
            (Some(old), None) => {
                let path_start = prefix.len();
                prefix.extend_from_slice(old.name);
                changed = history_tree_entry_changed(
                    store,
                    query,
                    Some(&old.id),
                    old.mode,
                    prefix,
                    cache,
                    missing_policy,
                )?;
                prefix.truncate(path_start);
                old_entry =
                    next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
            }
            (None, Some(new)) => {
                let path_start = prefix.len();
                prefix.extend_from_slice(new.name);
                changed = history_tree_entry_changed(
                    store,
                    query,
                    Some(&new.id),
                    new.mode,
                    prefix,
                    cache,
                    missing_policy,
                )?;
                prefix.truncate(path_start);
                new_entry =
                    next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
            }
            (None, None) => break,
        }
        if changed {
            break;
        }
    }
    if let Some(key) = cache_key {
        cache.insert(key, changed);
    }
    Ok(changed)
}

fn history_tree_entry_changed(
    store: &LooseObjectStore,
    query: &HistoryPathQuery,
    tree: Option<&ObjectId>,
    mode: TreeMode,
    path: &mut Vec<u8>,
    cache: &mut HistoryTreeChangeCache,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<bool> {
    if query.matches_path(path) {
        return Ok(true);
    }
    if mode != TreeMode::Tree {
        return Ok(false);
    }
    let path_start = path.len();
    path.push(b'/');
    let changed = history_tree_changed(store, query, tree, None, path, cache, missing_policy)?;
    path.truncate(path_start);
    Ok(changed)
}

pub(crate) fn history_visit_changed_blobs<F>(
    store: &LooseObjectStore,
    query: &HistoryPathQuery,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
    mut visit: F,
) -> Result<bool>
where
    F: FnMut(&[u8], Option<(TreeMode, &ObjectId)>, Option<(TreeMode, &ObjectId)>) -> Result<bool>,
{
    let mut path = Vec::new();
    history_visit_changed_blobs_at(store, query, old_tree, new_tree, &mut path, &mut visit)
}

fn history_visit_changed_blobs_at<F>(
    store: &LooseObjectStore,
    query: &HistoryPathQuery,
    old_tree: Option<&ObjectId>,
    new_tree: Option<&ObjectId>,
    path: &mut Vec<u8>,
    visit: &mut F,
) -> Result<bool>
where
    F: FnMut(&[u8], Option<(TreeMode, &ObjectId)>, Option<(TreeMode, &ObjectId)>) -> Result<bool>,
{
    if old_tree == new_tree || !query.may_match_prefix(path) {
        return Ok(false);
    }
    let old_object = old_tree
        .map(|id| read_history_tree_object(store, id))
        .transpose()?;
    let new_object = new_tree
        .map(|id| read_history_tree_object(store, id))
        .transpose()?;
    let old_content = old_object.as_ref().map(|object| object.content.as_slice());
    let new_content = new_object.as_ref().map(|object| object.content.as_slice());
    let mut old_cursor = 0;
    let mut new_cursor = 0;
    let mut old_entry = next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
    let mut new_entry = next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
    while old_entry.is_some() || new_entry.is_some() {
        match (old_entry.take(), new_entry.take()) {
            (Some(old), Some(new)) if old.name == new.name => {
                let path_start = path.len();
                path.extend_from_slice(old.name);
                let matched = if old.mode == TreeMode::Tree && new.mode == TreeMode::Tree {
                    path.push(b'/');
                    history_visit_changed_blobs_at(
                        store,
                        query,
                        Some(&old.id),
                        Some(&new.id),
                        path,
                        visit,
                    )?
                } else if old.mode == TreeMode::Tree || new.mode == TreeMode::Tree {
                    let mut matched = false;
                    if old.mode == TreeMode::Tree {
                        path.push(b'/');
                        matched = history_visit_changed_blobs_at(
                            store,
                            query,
                            Some(&old.id),
                            None,
                            path,
                            visit,
                        )?;
                        path.pop();
                    }
                    if new.mode == TreeMode::Tree && !matched {
                        path.push(b'/');
                        matched = history_visit_changed_blobs_at(
                            store,
                            query,
                            None,
                            Some(&new.id),
                            path,
                            visit,
                        )?;
                        path.pop();
                    }
                    if old.mode == TreeMode::Tree
                        && new.mode != TreeMode::Tree
                        && !matched
                        && query.matches_path(path)
                    {
                        matched = visit(path, None, Some((new.mode, &new.id)))?;
                    } else if old.mode != TreeMode::Tree
                        && new.mode == TreeMode::Tree
                        && !matched
                        && query.matches_path(path)
                    {
                        matched = visit(path, Some((old.mode, &old.id)), None)?;
                    }
                    matched
                } else if old.mode != new.mode || old.id != new.id {
                    if query.matches_path(path) {
                        visit(path, Some((old.mode, &old.id)), Some((new.mode, &new.id)))?
                    } else {
                        false
                    }
                } else {
                    false
                };
                path.truncate(path_start);
                if matched {
                    return Ok(true);
                }
                old_entry =
                    next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
                new_entry =
                    next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
            }
            (Some(old), Some(new)) => {
                let old_name = old.name;
                let new_name = new.name;
                if old_name < new_name {
                    let path_start = path.len();
                    path.extend_from_slice(old_name);
                    let matched =
                        history_visit_changed_tree_entry(store, query, &old, None, path, visit)?;
                    path.truncate(path_start);
                    if matched {
                        return Ok(true);
                    }
                    new_entry = Some(new);
                    old_entry =
                        next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
                } else {
                    let path_start = path.len();
                    path.extend_from_slice(new_name);
                    let matched = history_visit_added_tree_entry(store, query, &new, path, visit)?;
                    path.truncate(path_start);
                    if matched {
                        return Ok(true);
                    }
                    old_entry = Some(old);
                    new_entry =
                        next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
                }
            }
            (Some(old), None) => {
                let path_start = path.len();
                path.extend_from_slice(old.name);
                let matched =
                    history_visit_changed_tree_entry(store, query, &old, None, path, visit)?;
                path.truncate(path_start);
                if matched {
                    return Ok(true);
                }
                old_entry =
                    next_history_tree_entry(old_content, &mut old_cursor, store.algorithm())?;
            }
            (None, Some(new)) => {
                let path_start = path.len();
                path.extend_from_slice(new.name);
                let matched = history_visit_added_tree_entry(store, query, &new, path, visit)?;
                path.truncate(path_start);
                if matched {
                    return Ok(true);
                }
                new_entry =
                    next_history_tree_entry(new_content, &mut new_cursor, store.algorithm())?;
            }
            (None, None) => break,
        }
    }
    Ok(false)
}

fn history_visit_changed_tree_entry<F>(
    store: &LooseObjectStore,
    query: &HistoryPathQuery,
    entry: &zmin_git_core::TreeEntryRef<'_>,
    other: Option<&zmin_git_core::TreeEntryRef<'_>>,
    path: &mut Vec<u8>,
    visit: &mut F,
) -> Result<bool>
where
    F: FnMut(&[u8], Option<(TreeMode, &ObjectId)>, Option<(TreeMode, &ObjectId)>) -> Result<bool>,
{
    if entry.mode == TreeMode::Tree {
        path.push(b'/');
        let matched = history_visit_changed_blobs_at(
            store,
            query,
            Some(&entry.id),
            other
                .filter(|value| value.mode == TreeMode::Tree)
                .map(|value| &value.id),
            path,
            visit,
        )?;
        path.pop();
        return Ok(matched);
    }
    if !query.matches_path(path) {
        return Ok(false);
    }
    visit(path, Some((entry.mode, &entry.id)), None)
}

fn history_visit_added_tree_entry<F>(
    store: &LooseObjectStore,
    query: &HistoryPathQuery,
    entry: &zmin_git_core::TreeEntryRef<'_>,
    path: &mut Vec<u8>,
    visit: &mut F,
) -> Result<bool>
where
    F: FnMut(&[u8], Option<(TreeMode, &ObjectId)>, Option<(TreeMode, &ObjectId)>) -> Result<bool>,
{
    if entry.mode == TreeMode::Tree {
        path.push(b'/');
        let matched =
            history_visit_changed_blobs_at(store, query, None, Some(&entry.id), path, visit)?;
        path.pop();
        return Ok(matched);
    }
    if !query.matches_path(path) {
        return Ok(false);
    }
    visit(path, None, Some((entry.mode, &entry.id)))
}

fn read_history_tree_object(store: &LooseObjectStore, id: &ObjectId) -> Result<LooseObject> {
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Tree {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a tree",
        )));
    }
    Ok(object)
}

fn read_history_tree_object_with_missing_policy(
    store: &LooseObjectStore,
    id: &ObjectId,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<Option<LooseObject>> {
    if missing_policy.excludes_promisor_object(id) {
        return Ok(None);
    }
    match read_history_tree_object(store, id) {
        Ok(object) => Ok(Some(object)),
        Err(CliError::Io(error))
            if error.kind() == io::ErrorKind::NotFound && missing_policy.fatal_on_missing_tree =>
        {
            Err(missing_tree_error(id))
        }
        Err(CliError::Io(error))
            if error.kind() == io::ErrorKind::NotFound
                && missing_policy.allows_missing_object(id) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn read_commit_tree_with_missing_policy(
    store: &LooseObjectStore,
    commit_id: &ObjectId,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<Option<ObjectId>> {
    let tree_id = read_commit_tree_uncached(store, commit_id)?;
    if read_history_tree_object_with_missing_policy(store, &tree_id, missing_policy)?.is_none() {
        return Ok(None);
    }
    Ok(Some(tree_id))
}

fn next_history_tree_entry<'a>(
    content: Option<&'a [u8]>,
    cursor: &mut usize,
    algorithm: GitHashAlgorithm,
) -> Result<Option<zmin_git_core::TreeEntryRef<'a>>> {
    let Some(content) = content else {
        return Ok(None);
    };
    decode_tree_entry_ref(algorithm, content, cursor).map_err(CliError::Io)
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RevListTreeMissingPolicy<'a> {
    pub(crate) allow_any: bool,
    pub(crate) exclude_promisor_objects: bool,
    pub(crate) emit_missing_objects: bool,
    pub(crate) fatal_on_missing_tree: bool,
    pub(crate) promised_objects: Option<&'a HashSet<ObjectId>>,
}

impl RevListTreeMissingPolicy<'_> {
    fn for_path_selection(self) -> Self {
        Self {
            exclude_promisor_objects: false,
            promised_objects: None,
            emit_missing_objects: false,
            fatal_on_missing_tree: true,
            ..self
        }
    }

    fn excludes_promisor_object(self, id: &ObjectId) -> bool {
        self.exclude_promisor_objects
            && self
                .promised_objects
                .is_some_and(|promised_objects| promised_objects.contains(id))
    }

    fn allows_missing_object(self, id: &ObjectId) -> bool {
        self.allow_any
            || self.emit_missing_objects
            || self
                .promised_objects
                .is_some_and(|promised_objects| promised_objects.contains(id))
    }

    fn emits_missing_object(self, id: &ObjectId) -> bool {
        self.emit_missing_objects
            && !self.excludes_promisor_object(id)
            && self.allows_missing_object(id)
    }
}

fn missing_tree_error(id: &ObjectId) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("unable to read tree ({id})"),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RevListResolvedRef {
    pub(crate) ref_name: String,
    pub(crate) id: ObjectId,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RevListRefSnapshot {
    pub(crate) head_id: Option<ObjectId>,
    pub(crate) refs: Vec<RevListResolvedRef>,
}

#[derive(Debug, Default)]
pub(crate) struct RevListRevs {
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) extra_objects: Vec<(ObjectId, String)>,
    pub(crate) symmetric_diff: Option<RevListSymmetricDiff>,
    pub(crate) exclude_first_parent_only: bool,
}

#[derive(Debug, Default)]
struct RevListRootCollector {
    ordered: Vec<String>,
    seen: HashSet<String>,
    seen_ids: HashSet<ObjectId>,
}

impl RevListRootCollector {
    fn push_direct(&mut self, root: String) {
        if self.seen.insert(root.clone()) {
            self.ordered.push(root);
        }
    }

    fn push_resolved(&mut self, root: String, id: ObjectId) {
        if !self.seen_ids.insert(id) {
            return;
        }
        if self.seen.insert(root.clone()) {
            self.ordered.push(root);
        }
    }

    fn push_peeled_tag(&mut self, id: ObjectId) {
        let root = id.to_hex();
        self.push_resolved(root, id);
    }

    fn finish(self, parsed: &mut RevListRevs) {
        parsed.include = self.ordered;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RevListSelectorPolarity {
    Include,
    Exclude,
}

impl RevListSelectorPolarity {
    fn from_not_mode(not_mode: bool) -> Self {
        if not_mode {
            Self::Exclude
        } else {
            Self::Include
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RevListRefSelector {
    All,
    Branches(Option<String>),
    Remotes(Option<String>),
    Tags(Option<String>),
    Glob(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RevListHiddenRefSection {
    Fetch,
    Receive,
    UploadPack,
}

impl RevListHiddenRefSection {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "fetch" => Ok(Self::Fetch),
            "receive" => Ok(Self::Receive),
            "uploadpack" => Ok(Self::UploadPack),
            other => Err(CliError::Fatal {
                code: 128,
                message: format!("unsupported section for hidden refs: {other}"),
            }),
        }
    }

    fn config_section(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Receive => "receive",
            Self::UploadPack => "uploadpack",
        }
    }
}

#[derive(Debug, Default)]
struct RevListSelectorState {
    not_mode: bool,
    pending_excludes: Vec<String>,
    pending_hidden: Option<RevListHiddenRefSection>,
}

impl RevListSelectorState {
    fn polarity(&self) -> RevListSelectorPolarity {
        RevListSelectorPolarity::from_not_mode(self.not_mode)
    }

    fn toggle_not(&mut self) {
        self.not_mode = !self.not_mode;
    }

    fn add_exclude(&mut self, pattern: String) {
        self.pending_excludes.push(pattern);
    }

    fn take_excludes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_excludes)
    }

    fn set_hidden(&mut self, section: RevListHiddenRefSection) -> Result<()> {
        if self.pending_hidden.is_some() {
            return Err(CliError::Fatal {
                code: 128,
                message: "--exclude-hidden= passed more than once".into(),
            });
        }
        self.pending_hidden = Some(section);
        Ok(())
    }

    fn take_hidden(&mut self) -> Option<RevListHiddenRefSection> {
        self.pending_hidden.take()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RevListSelectorEvent {
    ToggleNot,
    Exclude(String),
    ExcludeHidden(String),
    Select(RevListRefSelector),
    Revision(String),
}

#[derive(Debug, Clone)]
pub(crate) struct PeeledTagChain {
    pub(crate) commit: Option<ObjectId>,
    pub(crate) tags: Vec<ObjectId>,
    pub(crate) tag_names: Vec<String>,
    pub(crate) terminal: Option<ObjectId>,
}

#[derive(Debug)]
struct TagTargetTypeMismatch {
    tag: ObjectId,
    target: ObjectId,
    actual_kind: GitObjectKind,
    expected_kind: GitObjectKind,
}

impl TagTargetTypeMismatch {
    fn diagnostic_kind_pair(&self) -> (GitObjectKind, GitObjectKind) {
        if self.actual_kind == GitObjectKind::Commit {
            (self.actual_kind, self.expected_kind)
        } else if self.expected_kind == GitObjectKind::Commit {
            (self.expected_kind, self.actual_kind)
        } else {
            (self.actual_kind, self.expected_kind)
        }
    }
}

impl std::fmt::Display for TagTargetTypeMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "object {} is a {}, not a {}",
            self.target,
            self.expected_kind.as_str(),
            self.actual_kind.as_str()
        )
    }
}

impl std::error::Error for TagTargetTypeMismatch {}

pub(crate) fn ordered_rev_list_selector_events(raw_args: &[String]) -> Vec<RevListSelectorEvent> {
    let mut events = Vec::new();
    let mut cursor = HistoryArgCursor::new(raw_args);
    while let Some(token) = cursor.next() {
        match token {
            HistoryArgToken::EndOfOptions { .. } => break,
            HistoryArgToken::Revision { value, .. } => {
                events.push(RevListSelectorEvent::Revision(value.to_owned()));
            }
            HistoryArgToken::Option { name, value, .. } => {
                let HistoryOptionName::Long(name) = name else {
                    continue;
                };
                let event = match name {
                    "not" => Some(RevListSelectorEvent::ToggleNot),
                    "all" => Some(RevListSelectorEvent::Select(RevListRefSelector::All)),
                    "branches" | "heads" => Some(RevListSelectorEvent::Select(
                        RevListRefSelector::Branches(value.map(str::to_owned)),
                    )),
                    "remotes" => Some(RevListSelectorEvent::Select(RevListRefSelector::Remotes(
                        value.map(str::to_owned),
                    ))),
                    "tags" => Some(RevListSelectorEvent::Select(RevListRefSelector::Tags(
                        value.map(str::to_owned),
                    ))),
                    "glob" => value.map(|value| {
                        RevListSelectorEvent::Select(RevListRefSelector::Glob(value.to_owned()))
                    }),
                    "exclude" => value.map(|value| RevListSelectorEvent::Exclude(value.to_owned())),
                    "exclude-hidden" => {
                        value.map(|value| RevListSelectorEvent::ExcludeHidden(value.to_owned()))
                    }
                    _ => None,
                };
                if let Some(event) = event {
                    events.push(event);
                }
            }
        }
    }
    events
}

pub(crate) fn rev_list_selector_events_from_revisions(
    revisions: &[String],
) -> Vec<RevListSelectorEvent> {
    revisions
        .iter()
        .filter(|revision| revision.as_str() != "--")
        .cloned()
        .map(RevListSelectorEvent::Revision)
        .collect()
}

pub(crate) fn rev_list_selector_events_have_positive_selection(
    events: &[RevListSelectorEvent],
) -> bool {
    let mut not_mode = false;
    for event in events {
        match event {
            RevListSelectorEvent::ToggleNot => not_mode = !not_mode,
            RevListSelectorEvent::Select(RevListRefSelector::All) => return true,
            RevListSelectorEvent::Select(_) if !not_mode => return true,
            RevListSelectorEvent::Revision(_) if !not_mode => return true,
            RevListSelectorEvent::Exclude(_)
            | RevListSelectorEvent::ExcludeHidden(_)
            | RevListSelectorEvent::Select(_)
            | RevListSelectorEvent::Revision(_) => {}
        }
    }
    false
}

#[derive(Debug, Clone)]
pub(crate) struct RevListSymmetricDiff {
    pub(crate) left: String,
    pub(crate) right: String,
    pub(crate) merge_bases: Vec<ObjectId>,
}

pub(crate) fn collect_rev_list_revs(
    repo: &GitRepo,
    store: &LooseObjectStore,
    all: bool,
    revs: Vec<String>,
) -> Result<RevListRevs> {
    let events = rev_list_selector_events_from_revisions(&revs);
    collect_rev_list_revs_with_events(repo, store, all, events, None)
}

pub(crate) fn collect_rev_list_ref_snapshot(
    repo: &GitRepo,
    algorithm: GitHashAlgorithm,
) -> Result<RevListRefSnapshot> {
    let head_refs = RefStore::new(&repo.git_dir, algorithm);
    let refs = RefStore::new(repo_common_dir(repo), algorithm);
    let current_branch = current_branch_ref(&head_refs).map_err(|error| match error {
        CliError::Io(inner)
            if matches!(
                inner.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData
            ) =>
        {
            broken_current_branch_error()
        }
        other => other,
    })?;
    let mut snapshot = RevListRefSnapshot {
        head_id: current_branch
            .as_deref()
            .and_then(|branch| refs.resolve(branch).ok())
            .or_else(|| head_refs.resolve("HEAD").ok()),
        refs: Vec::new(),
    };
    refs.for_each_resolved_ref("refs/", |ref_name, id| {
        snapshot.refs.push(RevListResolvedRef {
            ref_name: ref_name.to_owned(),
            id: id.clone(),
        });
        Ok::<(), CliError>(())
    })?;
    Ok(snapshot)
}

fn repo_common_dir(repo: &GitRepo) -> &Path {
    repo.objects_dir.parent().unwrap_or(&repo.git_dir)
}

fn configured_hidden_ref_patterns(
    repo: &GitRepo,
    section: RevListHiddenRefSection,
) -> Result<Vec<String>> {
    let section_name = section.config_section();
    let entries = read_config_entries(repo).map_err(CliError::Io)?;
    let mut patterns = Vec::new();
    for entry in entries {
        let selected = entry.key == "hiderefs"
            && entry.subsection.is_empty()
            && (entry.section == "transfer" || entry.section == section_name);
        if !selected {
            continue;
        }
        if entry.implicit_bool {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("missing value for '{}.hiderefs'", entry.section),
            });
        }
        let mut pattern = entry.value;
        while pattern.ends_with('/') {
            pattern.pop();
        }
        patterns.push(pattern);
    }
    Ok(patterns)
}

fn active_git_namespace() -> Option<String> {
    std::env::var("GIT_NAMESPACE")
        .ok()
        .filter(|namespace| !namespace.is_empty())
}

fn hidden_ref_matches_with_namespace(
    ref_name: &str,
    patterns: &[String],
    namespace: Option<&str>,
) -> bool {
    for pattern in patterns.iter().rev() {
        let (negated, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, pattern.as_str()), |pattern| (true, pattern));
        let full_name = pattern.starts_with('^');
        let pattern = pattern.strip_prefix('^').unwrap_or(pattern);
        let Some(candidate) = (if full_name {
            Some(ref_name)
        } else {
            namespaced_ref_name(ref_name, namespace)
        }) else {
            continue;
        };
        if candidate.starts_with(pattern)
            && candidate
                .as_bytes()
                .get(pattern.len())
                .is_none_or(|byte| *byte == b'/')
        {
            return !negated;
        }
    }
    false
}

fn namespaced_ref_name<'a>(ref_name: &'a str, namespace: Option<&str>) -> Option<&'a str> {
    let Some(namespace) = namespace else {
        return Some(ref_name);
    };
    let mut candidate = ref_name;
    for component in namespace
        .split('/')
        .filter(|component| !component.is_empty())
    {
        let Some(rest) = candidate.strip_prefix("refs/namespaces/") else {
            return None;
        };
        let Some((prefix, rest)) = rest.split_once('/') else {
            return None;
        };
        if prefix != component {
            return None;
        }
        candidate = rest;
    }
    candidate.starts_with("refs/").then_some(candidate)
}

pub(crate) fn collect_rev_list_revs_with_snapshot(
    repo: &GitRepo,
    store: &LooseObjectStore,
    all: bool,
    revs: Vec<String>,
    ref_snapshot: Option<&RevListRefSnapshot>,
) -> Result<RevListRevs> {
    let events = rev_list_selector_events_from_revisions(&revs);
    collect_rev_list_revs_with_events(repo, store, all, events, ref_snapshot)
}

pub(crate) fn collect_rev_list_revs_with_events(
    repo: &GitRepo,
    store: &LooseObjectStore,
    all: bool,
    events: Vec<RevListSelectorEvent>,
    ref_snapshot: Option<&RevListRefSnapshot>,
) -> Result<RevListRevs> {
    let mut parsed = RevListRevs::default();
    let mut roots = RevListRootCollector::default();
    let mut selector_state = RevListSelectorState::default();
    let mut hidden_patterns = HashMap::<RevListHiddenRefSection, Vec<String>>::new();
    let commit_cache = CommitObjectCache::new(store);
    let namespace = active_git_namespace();
    let has_explicit_all = events
        .iter()
        .any(|event| matches!(event, RevListSelectorEvent::Select(RevListRefSelector::All)));
    let has_selector_selection = rev_list_selector_events_have_positive_selection(&events);
    if all && !has_explicit_all {
        collect_rev_list_ref_selector(
            store,
            &mut parsed,
            &mut roots,
            RevListRefSelector::All,
            selector_state.polarity(),
            &[],
            ref_snapshot,
            repo,
            None,
            namespace.as_deref(),
        )?;
    }
    for event in events {
        match event {
            RevListSelectorEvent::ToggleNot => selector_state.toggle_not(),
            RevListSelectorEvent::Exclude(pattern) => selector_state.add_exclude(pattern),
            RevListSelectorEvent::ExcludeHidden(section) => {
                selector_state.set_hidden(RevListHiddenRefSection::parse(&section)?)?;
            }
            RevListSelectorEvent::Select(selector) => {
                let hidden_section = matches!(
                    &selector,
                    RevListRefSelector::All | RevListRefSelector::Glob(_)
                )
                .then(|| selector_state.take_hidden())
                .flatten();
                let excludes = if hidden_section.is_some() {
                    selector_state.take_excludes()
                } else {
                    selector_state.take_excludes()
                };
                if selector_state.pending_hidden.is_some()
                    && !matches!(
                        &selector,
                        RevListRefSelector::All | RevListRefSelector::Glob(_)
                    )
                {
                    let selector_name = match &selector {
                        RevListRefSelector::Branches(_) => "--branches",
                        RevListRefSelector::Remotes(_) => "--remotes",
                        RevListRefSelector::Tags(_) => "--tags",
                        RevListRefSelector::All | RevListRefSelector::Glob(_) => unreachable!(),
                    };
                    return Err(
                        crate::cli::commands::history_commands::rev_list_selector_conflict_error(
                            selector_name,
                        ),
                    );
                }
                let hidden = if let Some(section) = hidden_section {
                    Some(
                        hidden_patterns
                            .entry(section)
                            .or_insert(configured_hidden_ref_patterns(repo, section)?),
                    )
                } else {
                    None
                };
                collect_rev_list_ref_selector(
                    store,
                    &mut parsed,
                    &mut roots,
                    selector,
                    selector_state.polarity(),
                    &excludes,
                    ref_snapshot,
                    repo,
                    hidden.map(|value| value.as_slice()),
                    namespace.as_deref(),
                )?;
            }
            RevListSelectorEvent::Revision(rev) => {
                let not_mode = selector_state.not_mode;
                if let Some(stripped) = rev.strip_prefix('^') {
                    if stripped.is_empty() {
                        return Err(CliError::Message("empty negative revision".into()));
                    }
                    if not_mode {
                        roots.push_direct(stripped.to_owned());
                    } else {
                        parsed.exclude.push(stripped.to_owned());
                    }
                } else if let Some((left, right)) = rev.split_once("...") {
                    if right.contains("...") {
                        return Err(ambiguous_revision_error(&rev));
                    }
                    let left = if left.is_empty() { "HEAD" } else { left };
                    let right = if right.is_empty() { "HEAD" } else { right };
                    let left_id = resolve_commitish_io(repo, store, left).map_err(|error| {
                        map_head_resolution_error(left, &error)
                            .unwrap_or_else(|| ambiguous_revision_error(left))
                    })?;
                    let right_id = resolve_commitish_io(repo, store, right).map_err(|error| {
                        map_head_resolution_error(right, &error)
                            .unwrap_or_else(|| ambiguous_revision_error(right))
                    })?;
                    let bases = merge_bases_all_cached(store, &commit_cache, &left_id, &right_id)?;
                    if not_mode {
                        parsed.exclude.push(left.to_owned());
                        parsed.exclude.push(right.to_owned());
                        for base in bases {
                            roots.push_direct(base.to_hex());
                        }
                    } else {
                        if parsed.symmetric_diff.is_none() {
                            parsed.symmetric_diff = Some(RevListSymmetricDiff {
                                left: left.to_owned(),
                                right: right.to_owned(),
                                merge_bases: bases.clone(),
                            });
                        }
                        roots.push_direct(left.to_owned());
                        roots.push_direct(right.to_owned());
                        parsed
                            .exclude
                            .extend(bases.into_iter().map(|base| base.to_hex()));
                    }
                } else if let Some((left, right)) = rev.split_once("..") {
                    let left = if left.is_empty() { "HEAD" } else { left };
                    let right = if right.is_empty() { "HEAD" } else { right };
                    if not_mode {
                        roots.push_direct(left.to_owned());
                        parsed.exclude.push(right.to_owned());
                    } else {
                        parsed.exclude.push(left.to_owned());
                        roots.push_direct(right.to_owned());
                    }
                } else if not_mode {
                    parsed.exclude.push(rev);
                } else {
                    roots.push_direct(rev);
                }
            }
        }
    }
    roots.finish(&mut parsed);
    let mut normalized_include = Vec::with_capacity(parsed.include.len());
    let mut normalized_ids = HashSet::with_capacity(parsed.include.len());
    for rev in std::mem::take(&mut parsed.include) {
        let direct_tag_chain = direct_revision_tag_chain(repo, store, &rev)?;
        match resolve_commitish_io(repo, store, &rev) {
            Ok(id) => {
                if let Some(chain) = direct_tag_chain.as_ref() {
                    append_tag_chain_extra_objects(&mut parsed, chain, None);
                }
                if normalized_ids.insert(id) {
                    normalized_include.push(rev);
                }
            }
            Err(error) => {
                if let Some(cli_error) = map_head_resolution_error(&rev, &error) {
                    return Err(cli_error);
                }
                if let Some(chain) = direct_tag_chain.as_ref()
                    && chain.commit.is_none()
                {
                    append_tag_chain_extra_objects(&mut parsed, chain, None);
                    continue;
                }
                let id = resolve_objectish(repo, &rev).map_err(|resolve_error| {
                    map_head_resolution_error(&rev, &resolve_error)
                        .unwrap_or_else(|| ambiguous_revision_error(&rev))
                })?;
                match object_kind_hint_or_read(store, &id) {
                    Ok(GitObjectKind::Commit) => {
                        if normalized_ids.insert(id) {
                            normalized_include.push(rev);
                        }
                    }
                    Ok(_) => parsed.extra_objects.push((id, String::new())),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        parsed.extra_objects.push((id, String::new()));
                    }
                    Err(error) => return Err(CliError::Io(error)),
                }
            }
        }
    }
    parsed.include = normalized_include;
    if parsed.include.is_empty() && parsed.extra_objects.is_empty() && !has_selector_selection {
        return Err(CliError::Message(
            "`rev-list` requires at least one positive revision".into(),
        ));
    }
    for rev in &parsed.exclude {
        resolve_commitish_io(repo, store, rev).map_err(|error| {
            map_head_resolution_error(rev, &error).unwrap_or_else(|| ambiguous_revision_error(rev))
        })?;
    }
    Ok(parsed)
}

fn direct_revision_tag_chain(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revision: &str,
) -> Result<Option<PeeledTagChain>> {
    let Ok(id) = resolve_objectish(repo, revision) else {
        return Ok(None);
    };
    let kind = match object_kind_hint_or_read(store, &id) {
        Ok(kind) => kind,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CliError::Io(error)),
    };
    if kind != GitObjectKind::Tag {
        return Ok(None);
    }
    peel_tag_chain_to_commit(store, &id)
        .map(Some)
        .map_err(|error| map_direct_tag_chain_error(revision, error))
}

fn map_direct_tag_chain_error(revision: &str, error: io::Error) -> CliError {
    if error.kind() == io::ErrorKind::InvalidData
        && let Some(mismatch) = error
            .get_ref()
            .and_then(|cause| cause.downcast_ref::<TagTargetTypeMismatch>())
    {
        return CliError::Stderr {
            code: 128,
            text: format!(
                "error: object {} is a {}, not a {}\nfatal: bad object {}\n",
                mismatch.target,
                mismatch.expected_kind.as_str(),
                mismatch.actual_kind.as_str(),
                mismatch.target
            ),
        };
    }
    if error.kind() == io::ErrorKind::InvalidData {
        return CliError::Fatal {
            code: 128,
            message: format!("bad object {revision}"),
        };
    }
    CliError::Fatal {
        code: 128,
        message: error.to_string(),
    }
}

fn append_tag_chain_extra_objects(
    parsed: &mut RevListRevs,
    chain: &PeeledTagChain,
    first_name: Option<&str>,
) {
    for (index, tag_id) in chain.tags.iter().enumerate() {
        let name = if let Some(first_name) = first_name {
            if index == 0 {
                first_name.to_owned()
            } else {
                String::new()
            }
        } else {
            chain.tag_names.get(index).cloned().unwrap_or_default()
        };
        parsed.extra_objects.push((tag_id.clone(), name));
    }
    if let Some(terminal) = &chain.terminal {
        parsed.extra_objects.push((terminal.clone(), String::new()));
    }
}

fn collect_rev_list_ref_selection(
    repo: &GitRepo,
    store: &LooseObjectStore,
    parsed: &mut RevListRevs,
    roots: &mut RevListRootCollector,
    prefix: &str,
    pattern: Option<&str>,
    not_mode: bool,
    ref_snapshot: Option<&RevListRefSnapshot>,
    full_name_patterns: bool,
    exclude_patterns: &[String],
    hidden_patterns: Option<&[String]>,
    namespace: Option<&str>,
) -> Result<()> {
    if let Some(snapshot) = ref_snapshot {
        return collect_rev_list_ref_selection_from_rows(
            store,
            parsed,
            roots,
            Some(snapshot.refs.as_slice()),
            prefix,
            pattern,
            not_mode,
            repo,
            full_name_patterns,
            exclude_patterns,
            hidden_patterns,
            namespace,
        );
    }
    let refs = RefStore::new(repo_common_dir(repo), store.algorithm());
    refs.for_each_resolved_ref(prefix, |ref_name, id| {
        if !rev_list_ref_selection_matches(ref_name, pattern, full_name_patterns, exclude_patterns)
            || hidden_patterns.is_some_and(|patterns| {
                hidden_ref_matches_with_namespace(ref_name, patterns, namespace)
            })
        {
            return Ok(());
        }
        append_rev_list_ref_selection(store, parsed, roots, ref_name, id, not_mode, prefix)
    })
}

fn collect_rev_list_ref_selection_from_rows(
    store: &LooseObjectStore,
    parsed: &mut RevListRevs,
    roots: &mut RevListRootCollector,
    rows: Option<&[RevListResolvedRef]>,
    prefix: &str,
    pattern: Option<&str>,
    not_mode: bool,
    repo: &GitRepo,
    full_name_patterns: bool,
    exclude_patterns: &[String],
    hidden_patterns: Option<&[String]>,
    namespace: Option<&str>,
) -> Result<()> {
    if let Some(rows) = rows {
        for row in rows {
            if !row.ref_name.starts_with(prefix)
                || !rev_list_ref_selection_matches(
                    &row.ref_name,
                    pattern,
                    full_name_patterns,
                    exclude_patterns,
                )
                || hidden_patterns.is_some_and(|patterns| {
                    hidden_ref_matches_with_namespace(&row.ref_name, patterns, namespace)
                })
            {
                continue;
            }
            append_rev_list_ref_selection(
                store,
                parsed,
                roots,
                &row.ref_name,
                &row.id,
                not_mode,
                prefix,
            )?;
        }
        return Ok(());
    }
    let refs = RefStore::new(repo_common_dir(repo), store.algorithm());
    refs.for_each_resolved_ref(prefix, |ref_name, id| {
        if !rev_list_ref_selection_matches(ref_name, pattern, full_name_patterns, exclude_patterns)
            || hidden_patterns.is_some_and(|patterns| {
                hidden_ref_matches_with_namespace(ref_name, patterns, namespace)
            })
        {
            return Ok(());
        }
        append_rev_list_ref_selection(store, parsed, roots, ref_name, id, not_mode, prefix)
    })
}

fn collect_rev_list_ref_selector(
    store: &LooseObjectStore,
    parsed: &mut RevListRevs,
    roots: &mut RevListRootCollector,
    selector: RevListRefSelector,
    polarity: RevListSelectorPolarity,
    exclude_patterns: &[String],
    ref_snapshot: Option<&RevListRefSnapshot>,
    repo: &GitRepo,
    hidden_patterns: Option<&[String]>,
    namespace: Option<&str>,
) -> Result<()> {
    let not_mode = matches!(polarity, RevListSelectorPolarity::Exclude);
    let is_all = matches!(selector, RevListRefSelector::All);
    let (prefix, pattern, full_name_patterns) = match selector {
        RevListRefSelector::All => ("refs/", None, true),
        RevListRefSelector::Branches(pattern) => ("refs/heads/", pattern, false),
        RevListRefSelector::Remotes(pattern) => ("refs/remotes/", pattern, false),
        RevListRefSelector::Tags(pattern) => ("refs/tags/", pattern, false),
        RevListRefSelector::Glob(pattern) if pattern.is_empty() => {
            return collect_rev_list_ref_selector(
                store,
                parsed,
                roots,
                RevListRefSelector::All,
                polarity,
                exclude_patterns,
                ref_snapshot,
                repo,
                hidden_patterns,
                namespace,
            );
        }
        RevListRefSelector::Glob(pattern) => {
            let pattern = if pattern.starts_with("refs/") {
                pattern
            } else {
                format!("refs/{pattern}")
            };
            let pattern = if pattern
                .bytes()
                .any(|byte| matches!(byte, b'*' | b'?' | b'['))
            {
                pattern
            } else {
                format!("{pattern}/*")
            };
            return collect_rev_list_ref_selection(
                repo,
                store,
                parsed,
                roots,
                "refs/",
                Some(&pattern),
                not_mode,
                ref_snapshot,
                true,
                exclude_patterns,
                hidden_patterns,
                namespace,
            );
        }
    };
    let pattern = if full_name_patterns {
        pattern
    } else {
        normalize_short_ref_selector_pattern(pattern)
    };
    let normalized_excludes = exclude_patterns.to_vec();
    collect_rev_list_ref_selection(
        repo,
        store,
        parsed,
        roots,
        prefix,
        pattern.as_deref(),
        not_mode,
        ref_snapshot,
        full_name_patterns,
        &normalized_excludes,
        hidden_patterns,
        namespace,
    )?;
    if is_all && let Some(head_id) = ref_snapshot.and_then(|snapshot| snapshot.head_id.clone()) {
        if not_mode {
            parsed.exclude.push(head_id.to_hex());
        } else {
            roots.push_resolved(head_id.to_hex(), head_id);
        }
    }
    Ok(())
}

fn append_rev_list_ref_selection(
    store: &LooseObjectStore,
    parsed: &mut RevListRevs,
    roots: &mut RevListRootCollector,
    ref_name: &str,
    id: &ObjectId,
    not_mode: bool,
    prefix: &str,
) -> Result<()> {
    let selection_name = if prefix == "refs/" {
        ref_name.to_owned()
    } else {
        short_ref_name(ref_name)
    };
    let kind = match object_kind_hint_or_read(store, id) {
        Ok(kind) => kind,
        Err(_) => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("bad object {selection_name}"),
            });
        }
    };
    if kind == GitObjectKind::Commit {
        if not_mode {
            parsed.exclude.push(ref_name.to_owned());
        } else {
            roots.push_resolved(ref_name.to_owned(), id.clone());
        }
    } else if kind == GitObjectKind::Tag {
        let peeled = match peel_tag_chain_to_commit(store, id) {
            Ok(peeled) => peeled,
            Err(error) if not_mode && error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: error.to_string(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                if let Some(mismatch) = error
                    .get_ref()
                    .and_then(|cause| cause.downcast_ref::<TagTargetTypeMismatch>())
                {
                    let mismatch_text = if prefix == "refs/" {
                        let (first_kind, second_kind) = mismatch.diagnostic_kind_pair();
                        format!(
                            "object {} is a {}, not a {}",
                            mismatch.target,
                            first_kind.as_str(),
                            second_kind.as_str()
                        )
                    } else {
                        mismatch.to_string()
                    };
                    let text = if prefix == "refs/"
                        && mismatch.actual_kind == GitObjectKind::Commit
                        && mismatch.expected_kind != GitObjectKind::Commit
                    {
                        format!(
                            "error: {mismatch_text}\nerror: bad tag pointer to {} in {}\nfatal: bad object {ref_name}\n",
                            mismatch.target, mismatch.tag
                        )
                    } else if prefix == "refs/" {
                        format!(
                            "error: {mismatch_text}\nfatal: bad object {}\n",
                            mismatch.target
                        )
                    } else {
                        format!(
                            "error: {mismatch_text}\nfatal: bad object {}\n",
                            mismatch.target
                        )
                    };
                    return Err(CliError::Stderr { code: 128, text });
                }
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("bad object {selection_name}"),
                });
            }
            Err(_) => {
                let name = if prefix == "refs/" {
                    ref_name.to_owned()
                } else {
                    short_ref_name(ref_name)
                };
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("bad object {name}"),
                });
            }
        };
        let commit_id = peeled.commit.clone();
        if let Some(commit_id) = commit_id {
            if not_mode {
                parsed.exclude.push(commit_id.to_hex());
            } else {
                roots.push_peeled_tag(commit_id);
            }
        }
        if not_mode {
            return Ok(());
        }
        append_tag_chain_extra_objects(parsed, &peeled, Some(&short_ref_name(ref_name)));
    } else if !not_mode {
        parsed.extra_objects.push((id.clone(), String::new()));
    }
    Ok(())
}

pub(crate) fn peel_tag_chain_to_commit(
    store: &LooseObjectStore,
    id: &ObjectId,
) -> io::Result<PeeledTagChain> {
    let mut current = id.clone();
    let mut visited = HashSet::new();
    let mut tags = Vec::new();
    let mut tag_names = Vec::new();
    loop {
        if !visited.insert(current.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tag nesting is cyclic",
            ));
        }
        let object = store.read_object(&current).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("bad object {}", current.to_hex()),
                )
            } else {
                error
            }
        })?;
        match object.kind {
            GitObjectKind::Commit => {
                return Ok(PeeledTagChain {
                    commit: Some(current),
                    tags,
                    tag_names,
                    terminal: None,
                });
            }
            GitObjectKind::Tag => {
                let tag_id = current.clone();
                tags.push(current);
                let tag = decode_tag(store.algorithm(), &object.content)?;
                tag_names.push(String::from_utf8_lossy(&tag.name).into_owned());
                let target = store.read_object(&tag.target).map_err(|error| {
                    if error.kind() == io::ErrorKind::NotFound {
                        io::Error::new(
                            io::ErrorKind::NotFound,
                            format!("bad object {}", tag.target.to_hex()),
                        )
                    } else {
                        error
                    }
                })?;
                if target.kind != tag.target_kind {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        TagTargetTypeMismatch {
                            tag: tag_id,
                            target: tag.target,
                            actual_kind: target.kind,
                            expected_kind: tag.target_kind,
                        },
                    ));
                }
                current = tag.target;
            }
            _ => {
                return Ok(PeeledTagChain {
                    commit: None,
                    tags,
                    tag_names,
                    terminal: Some(current),
                });
            }
        }
    }
}

fn rev_list_ref_selection_matches(
    ref_name: &str,
    pattern: Option<&str>,
    full_name_patterns: bool,
    exclude_patterns: &[String],
) -> bool {
    let matches_pattern = |pattern: &str| {
        if full_name_patterns {
            wildcard_match(pattern, ref_name)
        } else {
            wildcard_match(pattern, &short_ref_name(ref_name))
        }
    };
    pattern.map_or(true, matches_pattern)
        && !exclude_patterns
            .iter()
            .any(|pattern| matches_pattern(pattern))
}

fn normalize_short_ref_selector_pattern(pattern: Option<String>) -> Option<String> {
    let pattern = pattern?;
    if pattern.is_empty() {
        return None;
    }
    if pattern
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'['))
    {
        Some(pattern)
    } else {
        Some(format!("{pattern}/*"))
    }
}

pub(crate) fn collect_commits_cached<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    collect_commits_cached_with_parent_policy(repo, store, commit_cache, revs, max_count, false)
}

fn collect_commits_cached_with_parent_policy<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
    first_parent_only: bool,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_with_parent_policy(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        max_count,
        None,
        first_parent_only,
    )
}

pub(crate) fn collect_commits_from_ids_cached<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    roots: &[ObjectId],
    max_count: Option<usize>,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        max_count,
        None,
    )
}

fn collect_commits_cached_into_set<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    out: &mut HashSet<ObjectId>,
) -> Result<()>
where
    S: GitObjectStore + ?Sized,
{
    collect_commits_cached_into_set_with_parent_policy(repo, store, commit_cache, revs, out, false)
}

fn collect_commits_cached_into_set_with_parent_policy<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    out: &mut HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<()>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_into_set_with_parent_policy(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        out,
        first_parent_only,
    )
}

fn collect_commits_from_ids_cached_into_set<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    roots: &[ObjectId],
    out: &mut HashSet<ObjectId>,
) -> Result<()>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_into_set(repo, commit_cache, pending, scheduled, sequence, out)
}

fn collect_pending_commits<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    mut pending: BinaryHeap<HeapPendingCommit>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: Option<&HashSet<ObjectId>>,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    collect_pending_commits_with_parent_policy(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        max_count,
        excluded,
        false,
    )
}

fn collect_pending_commits_with_parent_policy<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    mut pending: BinaryHeap<HeapPendingCommit>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: Option<&HashSet<ObjectId>>,
    first_parent_only: bool,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled.len()));
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        let commit = pending_commit.commit;
        if excluded.is_some_and(|excluded| excluded.contains(&id)) {
            continue;
        }
        out.push(id.clone());
        if max_count.is_some_and(|max| out.len() >= max) {
            break;
        }
        if shallow_commits.contains(&id) {
            continue;
        }
        let parents = commit.parents.iter().take(if first_parent_only {
            1
        } else {
            commit.parents.len()
        });
        reserve_commit_parent_traversal(
            &mut pending,
            &mut scheduled,
            if first_parent_only {
                commit.parents.len().min(1)
            } else {
                commit.parents.len()
            },
        );
        for parent in parents {
            if excluded.is_some_and(|excluded| excluded.contains(parent)) {
                continue;
            }
            if scheduled.insert(parent.clone()) {
                pending.push(HeapPendingCommit::new(
                    read_pending_commit(commit_cache, parent.clone())?,
                    sequence,
                ));
                sequence += 1;
            }
        }
    }
    Ok(out)
}

fn count_pending_commits<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    mut pending: BinaryHeap<HeapPendingCommit>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
) -> Result<usize>
where
    S: GitObjectStore + ?Sized,
{
    let shallow_commits = read_shallow_commits(repo)?;
    let mut count = 0_usize;
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        let commit = pending_commit.commit;
        if excluded.contains(&id) {
            continue;
        }
        count += 1;
        if max_count.is_some_and(|max| count >= max) {
            break;
        }
        if shallow_commits.contains(&id) {
            continue;
        }
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, commit.parents.len());
        for parent in &commit.parents {
            if excluded.contains(parent) {
                continue;
            }
            if scheduled.insert(parent.clone()) {
                pending.push(HeapPendingCommit::new(
                    read_pending_commit(commit_cache, parent.clone())?,
                    sequence,
                ));
                sequence += 1;
            }
        }
    }
    Ok(count)
}

fn collect_pending_commits_into_set<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    mut pending: BinaryHeap<HeapPendingCommit>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    out: &mut HashSet<ObjectId>,
) -> Result<()>
where
    S: GitObjectStore + ?Sized,
{
    collect_pending_commits_into_set_with_parent_policy(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        out,
        false,
    )
}

fn collect_pending_commits_into_set_with_parent_policy<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    mut pending: BinaryHeap<HeapPendingCommit>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    out: &mut HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<()>
where
    S: GitObjectStore + ?Sized,
{
    let shallow_commits = read_shallow_commits(repo)?;
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        let commit = pending_commit.commit;
        if !out.insert(id.clone()) {
            continue;
        }
        if shallow_commits.contains(&id) {
            continue;
        }
        let parents = commit.parents.iter().take(if first_parent_only {
            1
        } else {
            commit.parents.len()
        });
        reserve_commit_parent_traversal(
            &mut pending,
            &mut scheduled,
            if first_parent_only {
                commit.parents.len().min(1)
            } else {
                commit.parents.len()
            },
        );
        for parent in parents {
            if scheduled.insert(parent.clone()) {
                pending.push(HeapPendingCommit::new(
                    read_pending_commit(commit_cache, parent.clone())?,
                    sequence,
                ));
                sequence += 1;
            }
        }
    }
    Ok(())
}

struct PendingCommit {
    id: ObjectId,
    commit: Arc<CommitObject>,
    timestamp: i64,
}

struct PendingCommitMetadata {
    metadata: CollectedCommitMetadata,
    timestamp: i64,
}

struct PendingRevListObjectCommit {
    candidate: RevListObjectCommitCandidate,
    timestamp: i64,
}

struct PendingCommitOneline {
    commit: CollectedCommitOneline,
    timestamp: i64,
}

struct HeapPendingCommit {
    pending: PendingCommit,
    sequence: u64,
}

struct HeapPendingCommitMetadata {
    pending: PendingCommitMetadata,
    sequence: u64,
}

struct HeapPendingRevListObjectCommit {
    pending: PendingRevListObjectCommit,
    sequence: u64,
}

struct HeapPendingCommitOneline {
    pending: PendingCommitOneline,
    sequence: u64,
}

impl HeapPendingCommit {
    fn new(pending: PendingCommit, sequence: u64) -> Self {
        Self { pending, sequence }
    }
}

impl HeapPendingCommitMetadata {
    fn new(pending: PendingCommitMetadata, sequence: u64) -> Self {
        Self { pending, sequence }
    }
}

impl HeapPendingRevListObjectCommit {
    fn new(pending: PendingRevListObjectCommit, sequence: u64) -> Self {
        Self { pending, sequence }
    }
}

impl HeapPendingCommitOneline {
    fn new(pending: PendingCommitOneline, sequence: u64) -> Self {
        Self { pending, sequence }
    }
}

impl PartialEq for HeapPendingCommit {
    fn eq(&self, other: &Self) -> bool {
        self.pending.timestamp == other.pending.timestamp && self.sequence == other.sequence
    }
}

impl Eq for HeapPendingCommit {}

impl PartialOrd for HeapPendingCommit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPendingCommit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.pending
            .timestamp
            .cmp(&other.pending.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialEq for HeapPendingCommitMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.pending.timestamp == other.pending.timestamp && self.sequence == other.sequence
    }
}

impl Eq for HeapPendingCommitMetadata {}

impl PartialOrd for HeapPendingCommitMetadata {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPendingCommitMetadata {
    fn cmp(&self, other: &Self) -> Ordering {
        self.pending
            .timestamp
            .cmp(&other.pending.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialEq for HeapPendingRevListObjectCommit {
    fn eq(&self, other: &Self) -> bool {
        self.pending.timestamp == other.pending.timestamp && self.sequence == other.sequence
    }
}

impl Eq for HeapPendingRevListObjectCommit {}

impl PartialOrd for HeapPendingRevListObjectCommit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPendingRevListObjectCommit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.pending
            .timestamp
            .cmp(&other.pending.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialEq for HeapPendingCommitOneline {
    fn eq(&self, other: &Self) -> bool {
        self.pending.timestamp == other.pending.timestamp && self.sequence == other.sequence
    }
}

impl Eq for HeapPendingCommitOneline {}

impl PartialOrd for HeapPendingCommitOneline {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPendingCommitOneline {
    fn cmp(&self, other: &Self) -> Ordering {
        self.pending
            .timestamp
            .cmp(&other.pending.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

fn read_pending_commit<S>(
    commit_cache: &CommitObjectCache<'_, S>,
    id: ObjectId,
) -> Result<PendingCommit>
where
    S: GitObjectStore + ?Sized,
{
    let commit = commit_cache.read_commit(&id)?;
    let timestamp = signature_timestamp(&commit.committer).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("commit {} has invalid committer timestamp", id.to_hex()),
    })?;
    Ok(PendingCommit {
        id,
        commit,
        timestamp,
    })
}

fn read_pending_commit_metadata(
    store: &LooseObjectStore,
    id: ObjectId,
) -> Result<PendingCommitMetadata> {
    let mut matches = |_id: &ObjectId, _commit: &CommitObject| Ok(true);
    read_pending_commit_metadata_with_predicate(store, id, &mut matches, false)
}

fn read_pending_commit_metadata_with_predicate<P>(
    store: &LooseObjectStore,
    id: ObjectId,
    predicate: &mut P,
    evaluate_predicate: bool,
) -> Result<PendingCommitMetadata>
where
    P: FnMut(&ObjectId, &CommitObject) -> Result<bool>,
{
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let (parents, author, committer, timestamp) =
        parse_commit_parents_author_committer_and_timestamp(id.algorithm(), &object.content)
            .map_err(CliError::Io)?;
    let tree = parse_commit_tree_id(id.algorithm(), &object.content).map_err(CliError::Io)?;
    let grep_matches = if evaluate_predicate {
        predicate(
            &id,
            &decode_commit(id.algorithm(), &object.content).map_err(CliError::Io)?,
        )?
    } else {
        true
    };
    Ok(PendingCommitMetadata {
        metadata: CollectedCommitMetadata {
            id,
            tree,
            parents,
            author,
            committer,
            grep_matches,
        },
        timestamp,
    })
}

fn read_pending_rev_list_object_commit<P>(
    store: &LooseObjectStore,
    id: ObjectId,
    predicate: &mut P,
) -> Result<PendingRevListObjectCommit>
where
    P: RevListObjectCommitPredicate,
{
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let links = decode_commit_links(id.algorithm(), &object.content).map_err(CliError::Io)?;
    let (author_timestamp, committer_timestamp) =
        parse_commit_signature_timestamps(&object.content).map_err(CliError::Io)?;
    let matches = if predicate.needs_full_commit_object() {
        let commit = decode_commit(id.algorithm(), &object.content).map_err(CliError::Io)?;
        predicate.matches(&id, &commit)?
    } else {
        predicate.matches_metadata(
            &id,
            author_timestamp,
            committer_timestamp,
            links.parents.len(),
        )?
    };
    Ok(PendingRevListObjectCommit {
        timestamp: committer_timestamp,
        candidate: RevListObjectCommitCandidate {
            id,
            tree: links.tree,
            parents: links.parents,
            author_timestamp,
            committer_timestamp: Some(committer_timestamp),
            matches,
            sequence: 0,
            is_boundary: false,
        },
    })
}

fn parse_commit_signature_timestamps(bytes: &[u8]) -> io::Result<(Option<i64>, i64)> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "commit missing header end"))?;
    let mut author_timestamp = None;
    let mut committer_timestamp = None;
    for line in bytes[..header_end].split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"author ") {
            author_timestamp = parse_committer_timestamp(value);
        } else if let Some(value) = line.strip_prefix(b"committer ") {
            committer_timestamp = parse_committer_timestamp(value);
        }
    }
    let committer_timestamp = committer_timestamp.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid committer timestamp",
        )
    })?;
    Ok((author_timestamp, committer_timestamp))
}

pub(crate) fn read_unfiltered_rev_list_object_commit_candidate(
    store: &LooseObjectStore,
    id: ObjectId,
) -> Result<RevListObjectCommitCandidate> {
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let links = decode_commit_links(id.algorithm(), &object.content).map_err(CliError::Io)?;
    let (author_timestamp, committer_timestamp) =
        parse_commit_signature_timestamps(&object.content).map_err(CliError::Io)?;
    Ok(RevListObjectCommitCandidate {
        id,
        tree: links.tree,
        parents: links.parents,
        author_timestamp,
        committer_timestamp: Some(committer_timestamp),
        matches: true,
        sequence: 0,
        is_boundary: true,
    })
}

pub(crate) fn read_rev_list_object_boundary_metadata(
    store: &LooseObjectStore,
    id: ObjectId,
) -> Result<RevListObjectBoundaryMetadata> {
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let tree = parse_commit_tree_id(id.algorithm(), &object.content).map_err(CliError::Io)?;
    let (author_timestamp, committer_timestamp) =
        parse_commit_signature_timestamps(&object.content).map_err(CliError::Io)?;
    Ok(RevListObjectBoundaryMetadata {
        id,
        tree,
        author_timestamp,
        committer_timestamp,
    })
}

fn read_pending_commit_oneline(
    store: &LooseObjectStore,
    id: ObjectId,
) -> Result<PendingCommitOneline> {
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let (parents, subject, timestamp) =
        parse_commit_parents_subject_and_timestamp(id.algorithm(), &object.content)
            .map_err(CliError::Io)?;
    Ok(PendingCommitOneline {
        commit: CollectedCommitOneline {
            id,
            parents,
            subject,
        },
        timestamp,
    })
}

pub(crate) fn read_commit_metadata_from_store<S>(
    store: &S,
    id: &ObjectId,
) -> Result<CollectedCommitMetadata>
where
    S: GitObjectStore + ?Sized,
{
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let (parents, author, committer, _) =
        parse_commit_parents_author_committer_and_timestamp(id.algorithm(), &object.content)
            .map_err(CliError::Io)?;
    let tree = parse_commit_tree_id(id.algorithm(), &object.content).map_err(CliError::Io)?;
    Ok(CollectedCommitMetadata {
        id: id.clone(),
        tree,
        parents,
        author,
        committer,
        grep_matches: true,
    })
}

pub(crate) fn read_commit_metadata(
    store: &LooseObjectStore,
    id: &ObjectId,
) -> Result<CollectedCommitMetadata> {
    read_commit_metadata_from_store(store, id)
}

pub(crate) fn read_shallow_commits(repo: &GitRepo) -> Result<HashSet<ObjectId>> {
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    let path = read_common_git_dir(&repo.git_dir)?.join("shallow");
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut commits = HashSet::new();
    let mut reader = io::BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        commits.insert(ObjectId::from_hex(algorithm, line).map_err(CliError::Io)?);
    }
    Ok(commits)
}

pub(crate) fn partial_clone_enabled(repo: &GitRepo) -> Result<bool> {
    let mut has_partial_clone = false;
    let mut has_promisor_remote = false;
    let mut version = None::<String>;
    for entry in read_common_config_entries(repo).map_err(CliError::Io)? {
        if entry.section == "core"
            && entry.subsection.is_empty()
            && entry.key.eq_ignore_ascii_case("repositoryformatversion")
        {
            version = Some(entry.value.clone());
        }
        if entry.section == "extensions"
            && entry.subsection.is_empty()
            && entry.key.eq_ignore_ascii_case("partialclone")
        {
            has_partial_clone = true;
        }
        if entry.section == "remote"
            && !entry.subsection.is_empty()
            && entry.key.eq_ignore_ascii_case("promisor")
            && matches!(entry.value.to_ascii_lowercase().as_str(), "true" | "1")
        {
            has_promisor_remote = true;
        }
    }
    let has_promisor_pack = repo
        .objects_dir
        .join("pack")
        .read_dir()
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(std::result::Result::ok))
        .any(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("promisor"));
    Ok(version.as_deref() == Some("1")
        && (has_partial_clone || has_promisor_remote || has_promisor_pack))
}

pub(crate) fn collect_promisor_object_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
) -> Result<HashSet<ObjectId>> {
    collect_promisor_object_ids_for_policy(
        repo,
        store,
        RevListPromisorObjectPolicy::PromisorPackObjects,
    )
}

pub(crate) fn collect_promised_missing_object_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
) -> Result<HashSet<ObjectId>> {
    collect_promisor_object_ids_for_policy(
        repo,
        store,
        RevListPromisorObjectPolicy::MissingPromisorObjects,
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RevListPromisorObjectPolicy {
    PromisorPackObjects,
    ExcludePromisorObjects,
    MissingPromisorObjects,
}

pub(crate) fn collect_promisor_object_ids_for_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    policy: RevListPromisorObjectPolicy,
) -> Result<HashSet<ObjectId>> {
    if !partial_clone_enabled(repo)? {
        return Ok(HashSet::new());
    }
    let pack_dir = repo.objects_dir.join("pack");
    let entries = match fs::read_dir(&pack_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(HashSet::new());
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut selected = HashSet::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("promisor") {
            continue;
        }
        let pack_path = path.with_extension("pack");
        if !pack_path.is_file() {
            continue;
        }
        let ids = decode_pack_index_object_ids_from_path(store.algorithm(), &pack_path)
            .map_err(CliError::Io)?;
        for id in ids {
            let present = store.contains_object(&id).map_err(CliError::Io)?;
            match policy {
                RevListPromisorObjectPolicy::PromisorPackObjects => {
                    selected.insert(id);
                }
                RevListPromisorObjectPolicy::ExcludePromisorObjects => {
                    selected.insert(id.clone());
                    if present {
                        let object = store.read_object(&id).map_err(CliError::Io)?;
                        collect_promised_missing_links(store, &object, &mut selected)
                            .map_err(CliError::Io)?;
                    }
                }
                RevListPromisorObjectPolicy::MissingPromisorObjects => {
                    if present {
                        let object = store.read_object(&id).map_err(CliError::Io)?;
                        collect_promised_missing_links(store, &object, &mut selected)
                            .map_err(CliError::Io)?;
                    } else {
                        selected.insert(id);
                    }
                }
            }
        }
    }
    Ok(selected)
}

fn collect_promised_missing_links(
    store: &LooseObjectStore,
    object: &LooseObject,
    promised: &mut HashSet<ObjectId>,
) -> io::Result<()> {
    match object.kind {
        GitObjectKind::Blob => {}
        GitObjectKind::Tree => {
            let entries = decode_tree(object.id.algorithm(), &object.content)?;
            for entry in entries {
                if entry.mode != TreeMode::Gitlink && !store.contains_object(&entry.id)? {
                    promised.insert(entry.id);
                }
            }
        }
        GitObjectKind::Commit => {
            let (tree, parents, _) =
                parse_commit_tree_parents_and_timestamp(object.id.algorithm(), &object.content)?;
            if !store.contains_object(&tree)? {
                promised.insert(tree);
            }
            for parent in parents {
                if !store.contains_object(&parent)? {
                    promised.insert(parent);
                }
            }
        }
        GitObjectKind::Tag => {
            let target = decode_tag(object.id.algorithm(), &object.content)?.target;
            if !store.contains_object(&target)? {
                promised.insert(target);
            }
        }
    }
    Ok(())
}

pub(crate) fn collect_commits_with_exclusions(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<ObjectId>> {
    let commit_cache = CommitObjectCache::new(store);
    collect_commits_with_exclusions_cached(repo, store, &commit_cache, revs, max_count)
}

pub(crate) fn count_commits_with_exclusions(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<usize> {
    let commit_cache = CommitObjectCache::new(store);
    let excluded = collect_excluded_commits_cached_with_parent_policy(
        repo,
        store,
        &commit_cache,
        &revs.exclude,
        revs.exclude_first_parent_only,
    )?;
    count_commits_cached_with_excluded(
        repo,
        store,
        &commit_cache,
        &revs.include,
        max_count,
        &excluded,
    )
}

pub(crate) fn collect_commits_with_exclusions_cached<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let excluded = collect_excluded_commits_cached_with_parent_policy(
        repo,
        store,
        commit_cache,
        &revs.exclude,
        revs.exclude_first_parent_only,
    )?;
    collect_commits_cached_with_excluded(
        repo,
        store,
        commit_cache,
        &revs.include,
        max_count,
        &excluded,
    )
}

pub(crate) fn collect_commits_from_ids_with_id_exclusions_cached<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    roots: &[ObjectId],
    exclude_roots: &[ObjectId],
    exclude_revs: &[String],
    max_count: Option<usize>,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let excluded = collect_excluded_commits_from_ids_cached(repo, commit_cache, exclude_roots)?;
    let excluded = collect_excluded_commits_cached_into(
        repo,
        store,
        commit_cache,
        exclude_revs,
        excluded,
        false,
    )?;
    collect_commits_from_ids_cached_with_excluded(repo, commit_cache, roots, max_count, &excluded)
}

fn collect_commits_cached_with_excluded<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        max_count,
        Some(excluded),
    )
}

fn count_commits_cached_with_excluded<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
) -> Result<usize>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    count_pending_commits(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        max_count,
        excluded,
    )
}

pub(crate) fn collect_commits_from_ids_cached_with_excluded<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    roots: &[ObjectId],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        max_count,
        Some(excluded),
    )
}

pub(crate) fn collect_commits_from_ids_cached_with_excluded_with_parent_policy<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    roots: &[ObjectId],
    excluded: &HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_with_parent_policy(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        None,
        Some(excluded),
        first_parent_only,
    )
}

pub(crate) fn collect_commits_from_ids_links_cached_with_excluded_with_parent_policy(
    repo: &GitRepo,
    links_cache: &mut CommitLinksCache<'_, LooseObjectStore>,
    roots: &[ObjectId],
    excluded: &HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<HashSet<ObjectId>> {
    let shallow_commits = read_shallow_commits(repo)?;
    let mut pending = roots.to_vec();
    let capacity = root_traversal_capacity_hint(roots.len());
    let mut scheduled = HashSet::with_capacity(capacity);
    let mut collected = HashSet::with_capacity(capacity);
    while let Some(id) = pending.pop() {
        if !scheduled.insert(id.clone()) || excluded.contains(&id) {
            continue;
        }
        collected.insert(id.clone());
        if shallow_commits.contains(&id) {
            continue;
        }
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        let parent_count = if first_parent_only {
            links.parents.len().min(1)
        } else {
            links.parents.len()
        };
        pending.extend(links.parents.iter().take(parent_count).cloned());
    }
    Ok(collected)
}

pub(crate) fn count_commits_from_ids_uncached_with_excluded(
    repo: &GitRepo,
    store: &LooseObjectStore,
    roots: &[ObjectId],
    excluded: &HashSet<ObjectId>,
) -> Result<usize> {
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, id.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }
    let shallow_commits = read_shallow_commits(repo)?;
    let mut count = 0usize;
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        if excluded.contains(&id) {
            continue;
        }
        count += 1;
        if shallow_commits.contains(&id) {
            continue;
        }
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, pending_commit.parents.len());
        for parent in pending_commit.parents {
            if excluded.contains(&parent) {
                continue;
            }
            if scheduled.insert(parent.clone()) {
                pending.push(HeapPendingCommitLite::new(
                    read_pending_commit_uncached(store, parent)?,
                    sequence,
                ));
                sequence += 1;
            }
        }
    }
    Ok(count)
}

pub(crate) fn read_commit_parents_uncached(
    store: &LooseObjectStore,
    id: &ObjectId,
) -> Result<Vec<ObjectId>> {
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    parse_commit_parents(id.algorithm(), &object.content).map_err(CliError::Io)
}

fn read_commit_parents_uncached_into(
    store: &LooseObjectStore,
    id: &ObjectId,
    out: &mut Vec<ObjectId>,
) -> Result<usize> {
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    out.clear();
    let header_end = object
        .content
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or(object.content.len());
    for line in object.content[..header_end].split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if line.starts_with(b"tree ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"parent ") {
            out.push(parse_commit_header_id(id.algorithm(), value, "parent")?);
            continue;
        }
        break;
    }
    Ok(out.len())
}

pub(crate) fn read_commit_tree_uncached(
    store: &LooseObjectStore,
    id: &ObjectId,
) -> Result<ObjectId> {
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    parse_commit_tree_id(id.algorithm(), &object.content).map_err(CliError::Io)
}

pub(crate) fn collect_commit_trees_with_exclusions_uncached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommitTree>> {
    collect_commit_trees_with_exclusions_uncached_with_parent_policy(
        repo, store, revs, max_count, false,
    )
}

pub(crate) fn collect_commit_trees_with_exclusions_uncached_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
    first_parent_only: bool,
) -> Result<Vec<CollectedCommitTree>> {
    let excluded = collect_excluded_commits_uncached_with_parent_policy(
        repo,
        store,
        &revs.exclude,
        first_parent_only,
    )?;
    collect_commit_trees_uncached_with_excluded(repo, store, &revs.include, max_count, &excluded)
}

pub(crate) fn collect_commit_trees_from_ids_uncached_with_excluded(
    repo: &GitRepo,
    store: &LooseObjectStore,
    roots: &[ObjectId],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
) -> Result<Vec<CollectedCommitTree>> {
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if excluded.contains(id) {
            continue;
        }
        if scheduled.insert(id.clone()) {
            let pending_commit = read_pending_commit_uncached(store, id.clone())?;
            pending.push(HeapPendingCommitLite::new(pending_commit, sequence));
            sequence += 1;
        }
    }
    collect_pending_commit_trees_uncached(
        repo,
        store,
        pending,
        scheduled,
        sequence,
        max_count,
        Some(excluded),
    )
}

fn collect_excluded_commits_uncached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
) -> Result<HashSet<ObjectId>> {
    collect_excluded_commits_uncached_with_parent_policy(repo, store, revs, false)
}

fn collect_excluded_commits_uncached_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    first_parent_only: bool,
) -> Result<HashSet<ObjectId>> {
    let mut excluded = HashSet::with_capacity(root_traversal_capacity_hint(revs.len()));
    collect_commits_uncached_into_set_with_parent_policy(
        repo,
        store,
        revs,
        &mut excluded,
        first_parent_only,
    )?;
    Ok(excluded)
}

fn collect_commits_uncached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    max_count: Option<usize>,
) -> Result<Vec<ObjectId>> {
    collect_commits_uncached_with_parent_policy(repo, store, revs, max_count, false)
}

fn collect_commits_uncached_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    max_count: Option<usize>,
    first_parent_only: bool,
) -> Result<Vec<ObjectId>> {
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_uncached_with_parent_policy(
        repo,
        store,
        pending,
        scheduled,
        sequence,
        max_count,
        None,
        first_parent_only,
    )
}

pub(crate) fn collect_commits_from_ids_uncached_with_excluded(
    repo: &GitRepo,
    store: &LooseObjectStore,
    roots: &[ObjectId],
    excluded: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>> {
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, id.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_uncached(
        repo,
        store,
        pending,
        scheduled,
        sequence,
        None,
        Some(excluded),
    )
}

pub(crate) fn collect_commits_from_ids_uncached_with_excluded_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    roots: &[ObjectId],
    excluded: &HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<Vec<ObjectId>> {
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, id.clone())?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_uncached_with_parent_policy(
        repo,
        store,
        pending,
        scheduled,
        sequence,
        None,
        Some(excluded),
        first_parent_only,
    )
}

fn collect_commits_uncached_into_set(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    out: &mut HashSet<ObjectId>,
) -> Result<()> {
    collect_commits_uncached_into_set_with_parent_policy(repo, store, revs, out, false)
}

fn collect_commits_uncached_into_set_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    out: &mut HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<()> {
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commits_uncached_into_set_with_parent_policy(
        repo,
        store,
        pending,
        scheduled,
        sequence,
        out,
        first_parent_only,
    )
}

fn collect_commit_trees_uncached_with_excluded(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
) -> Result<Vec<CollectedCommitTree>> {
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitLite::new(
                read_pending_commit_uncached(store, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commit_trees_uncached(
        repo,
        store,
        pending,
        scheduled,
        sequence,
        max_count,
        Some(excluded),
    )
}

fn collect_pending_commits_uncached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingCommitLite>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: Option<&HashSet<ObjectId>>,
) -> Result<Vec<ObjectId>> {
    collect_pending_commits_uncached_with_parent_policy(
        repo, store, pending, scheduled, sequence, max_count, excluded, false,
    )
}

fn collect_pending_commits_uncached_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingCommitLite>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: Option<&HashSet<ObjectId>>,
    first_parent_only: bool,
) -> Result<Vec<ObjectId>> {
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled.len()));
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        if excluded.is_some_and(|excluded| excluded.contains(&id)) {
            continue;
        }
        out.push(id.clone());
        if max_count.is_some_and(|max| out.len() >= max) {
            break;
        }
        if shallow_commits.contains(&id) {
            continue;
        }
        let parent_count = if first_parent_only {
            pending_commit.parents.len().min(1)
        } else {
            pending_commit.parents.len()
        };
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, parent_count);
        for parent in pending_commit.parents.into_iter().take(parent_count) {
            if scheduled.insert(parent.clone()) {
                pending.push(HeapPendingCommitLite::new(
                    read_pending_commit_uncached(store, parent)?,
                    sequence,
                ));
                sequence += 1;
            }
        }
    }
    Ok(out)
}

fn collect_pending_commits_uncached_into_set(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingCommitLite>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    out: &mut HashSet<ObjectId>,
) -> Result<()> {
    collect_pending_commits_uncached_into_set_with_parent_policy(
        repo, store, pending, scheduled, sequence, out, false,
    )
}

fn collect_pending_commits_uncached_into_set_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingCommitLite>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    out: &mut HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<()> {
    let shallow_commits = read_shallow_commits(repo)?;
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        if !out.insert(id.clone()) {
            continue;
        }
        if shallow_commits.contains(&id) {
            continue;
        }
        let parent_count = if first_parent_only {
            pending_commit.parents.len().min(1)
        } else {
            pending_commit.parents.len()
        };
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, parent_count);
        for parent in pending_commit.parents.into_iter().take(parent_count) {
            if scheduled.insert(parent.clone()) {
                pending.push(HeapPendingCommitLite::new(
                    read_pending_commit_uncached(store, parent)?,
                    sequence,
                ));
                sequence += 1;
            }
        }
    }
    Ok(())
}

fn collect_pending_commit_trees_uncached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingCommitLite>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: Option<&HashSet<ObjectId>>,
) -> Result<Vec<CollectedCommitTree>> {
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled.len()));
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        if excluded.is_some_and(|excluded| excluded.contains(&id)) {
            continue;
        }
        out.push(CollectedCommitTree {
            id: id.clone(),
            tree: pending_commit.tree,
            is_boundary: false,
        });
        if max_count.is_some_and(|max| out.len() >= max) {
            break;
        }
        if shallow_commits.contains(&id) {
            continue;
        }
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, pending_commit.parents.len());
        for parent in pending_commit.parents {
            if excluded.is_some_and(|excluded| excluded.contains(&parent)) {
                continue;
            }
            if scheduled.insert(parent.clone()) {
                pending.push(HeapPendingCommitLite::new(
                    read_pending_commit_uncached(store, parent)?,
                    sequence,
                ));
                sequence += 1;
            }
        }
    }
    Ok(out)
}

struct PendingCommitLite {
    id: ObjectId,
    tree: ObjectId,
    parents: Vec<ObjectId>,
    author_timestamp: Option<i64>,
    timestamp: i64,
}

struct HeapPendingCommitLite {
    pending: PendingCommitLite,
    sequence: u64,
}

impl HeapPendingCommitLite {
    fn new(pending: PendingCommitLite, sequence: u64) -> Self {
        Self { pending, sequence }
    }
}

impl PartialEq for HeapPendingCommitLite {
    fn eq(&self, other: &Self) -> bool {
        self.pending.timestamp == other.pending.timestamp && self.sequence == other.sequence
    }
}

impl Eq for HeapPendingCommitLite {}

impl PartialOrd for HeapPendingCommitLite {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPendingCommitLite {
    fn cmp(&self, other: &Self) -> Ordering {
        self.pending
            .timestamp
            .cmp(&other.pending.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

fn read_pending_commit_uncached(
    store: &LooseObjectStore,
    id: ObjectId,
) -> Result<PendingCommitLite> {
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let (tree, parents, timestamp) =
        parse_commit_tree_parents_and_timestamp(id.algorithm(), &object.content)
            .map_err(CliError::Io)?;
    let author_timestamp = parse_commit_author_timestamp(&object.content);
    Ok(PendingCommitLite {
        id,
        tree,
        parents,
        author_timestamp,
        timestamp,
    })
}

fn parse_commit_author_timestamp(bytes: &[u8]) -> Option<i64> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or(bytes.len());
    bytes[..header_end]
        .split(|byte| *byte == b'\n')
        .find_map(|line| line.strip_prefix(b"author "))
        .and_then(parse_committer_timestamp)
}

fn parse_commit_tree_parents_and_timestamp(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<(ObjectId, Vec<ObjectId>, i64)> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or(bytes.len());
    let mut tree = None;
    let mut parents = Vec::with_capacity(1);
    let mut timestamp = None;
    for line in bytes[..header_end].split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"tree ") {
            tree = Some(parse_commit_header_id(algorithm, value, "tree")?);
        } else if let Some(value) = line.strip_prefix(b"parent ") {
            parents.push(parse_commit_header_id(algorithm, value, "parent")?);
        } else if let Some(value) = line.strip_prefix(b"committer ") {
            timestamp = parse_committer_timestamp(value);
        }
    }
    let tree = tree
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "commit missing tree header"))?;
    let timestamp = timestamp.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid committer timestamp",
        )
    })?;
    Ok((tree, parents, timestamp))
}

fn parse_commit_parents_author_committer_and_timestamp(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<(Vec<ObjectId>, Vec<u8>, Vec<u8>, i64)> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or(bytes.len());
    let mut parents = Vec::with_capacity(1);
    let mut author = None;
    let mut committer = None;
    for line in bytes[..header_end].split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"parent ") {
            parents.push(parse_commit_header_id(algorithm, value, "parent")?);
        } else if let Some(value) = line.strip_prefix(b"author ") {
            author = Some(value.to_vec());
        } else if let Some(value) = line.strip_prefix(b"committer ") {
            committer = Some(value.to_vec());
        }
    }
    let author = author.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "commit missing author header")
    })?;
    let committer = committer.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "commit missing committer header",
        )
    })?;
    let timestamp = parse_committer_timestamp(&committer).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid committer timestamp",
        )
    })?;
    Ok((parents, author, committer, timestamp))
}

fn parse_commit_tree_id(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<ObjectId> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or(bytes.len());
    for line in bytes[..header_end].split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"tree ") {
            return parse_commit_header_id(algorithm, value, "tree");
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "commit missing tree header",
    ))
}

pub(crate) fn write_rev_list_object_ids_uncached<W: Write>(
    store: &LooseObjectStore,
    commits: &[CollectedCommitTree],
    extra_objects: &[ObjectId],
    excluded_commits: &[ObjectId],
    missing_policy: RevListTreeMissingPolicy<'_>,
    out: &mut W,
) -> Result<()> {
    let mut seen = CompactRevListSeenObjectIds::with_capacity(
        store,
        rev_list_seen_capacity_hint(commits.len(), extra_objects.len(), excluded_commits.len()),
    )
    .map_err(CliError::Io)?;
    let mut tree_cache = TreeObjectRefCache::transient(store);
    for commit_id in excluded_commits {
        let tree = match read_commit_tree_uncached(store, commit_id) {
            Ok(tree) => tree,
            Err(CliError::Io(error)) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        collect_rev_list_tree_object_leaf_ids(&mut tree_cache, &tree, &mut seen, missing_policy)?;
    }
    for id in extra_objects {
        write_rev_list_extra_object_ids_ordered(
            store,
            &mut tree_cache,
            id,
            &mut seen,
            missing_policy,
            out,
        )?;
    }
    for commit in commits {
        if commit.is_boundary {
            continue;
        }
        write_rev_list_tree_object_ref_ids_ordered(
            &mut tree_cache,
            &commit.tree,
            &mut seen,
            missing_policy,
            out,
        )?;
    }
    Ok(())
}

struct TreeObjectRefCache<'a> {
    store: &'a LooseObjectStore,
    trees: HashMap<ObjectId, Arc<Vec<TreeObjectRef>>>,
    entry_limit: usize,
}

impl<'a> TreeObjectRefCache<'a> {
    fn with_capacity(store: &'a LooseObjectStore, capacity: usize) -> Self {
        Self {
            store,
            trees: HashMap::with_capacity(capacity),
            entry_limit: TREE_OBJECT_REF_CACHE_ENTRY_LIMIT,
        }
    }

    fn transient(store: &'a LooseObjectStore) -> Self {
        Self {
            store,
            trees: HashMap::new(),
            entry_limit: 0,
        }
    }

    #[cfg(test)]
    fn with_entry_limit(store: &'a LooseObjectStore, capacity: usize, entry_limit: usize) -> Self {
        Self {
            store,
            trees: HashMap::with_capacity(capacity),
            entry_limit: entry_limit.max(1),
        }
    }

    fn read_tree(&mut self, tree_id: &ObjectId) -> Result<Arc<Vec<TreeObjectRef>>> {
        if let Some(entries) = self.trees.get(tree_id) {
            return Ok(Arc::clone(entries));
        }
        let entries = Arc::new(self.store.read_tree_refs(tree_id)?);
        if self.entry_limit == 0 {
            return Ok(entries);
        }
        if self.trees.len() >= self.entry_limit {
            self.trees.clear();
        }
        self.trees.insert(tree_id.clone(), Arc::clone(&entries));
        Ok(entries)
    }
}

fn parse_commit_parents(algorithm: GitHashAlgorithm, bytes: &[u8]) -> io::Result<Vec<ObjectId>> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or(bytes.len());
    let mut parents = Vec::with_capacity(1);
    for line in bytes[..header_end].split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"parent ") {
            parents.push(parse_commit_header_id(algorithm, value, "parent")?);
        }
    }
    Ok(parents)
}

fn parse_commit_parents_subject_and_timestamp(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<(Vec<ObjectId>, String, i64)> {
    let header_end = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .unwrap_or(bytes.len());
    let mut parents = Vec::with_capacity(1);
    let mut committer_timestamp = None;
    for line in bytes[..header_end].split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"parent ") {
            parents.push(parse_commit_header_id(algorithm, value, "parent")?);
        } else if let Some(value) = line.strip_prefix(b"committer ") {
            committer_timestamp = parse_committer_timestamp(value);
        }
    }
    let message = bytes
        .get(header_end.saturating_add(2)..)
        .unwrap_or_default();
    let first_line = message
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let first_line = first_line.strip_suffix(b"\r").unwrap_or(first_line);
    let subject = String::from_utf8_lossy(first_line).into_owned();
    let timestamp = committer_timestamp.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid committer timestamp",
        )
    })?;
    Ok((parents, subject, timestamp))
}

fn parse_commit_header_id(
    algorithm: GitHashAlgorithm,
    value: &[u8],
    label: &str,
) -> io::Result<ObjectId> {
    ObjectId::from_hex_bytes(algorithm, value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("commit {label} id is invalid"),
        )
    })
}

fn parse_committer_timestamp(signature: &[u8]) -> Option<i64> {
    let end = signature.iter().rposition(|byte| *byte == b' ')?;
    let start = signature[..end].iter().rposition(|byte| *byte == b' ')? + 1;
    std::str::from_utf8(&signature[start..end])
        .ok()?
        .parse()
        .ok()
}

fn collect_excluded_commits_cached<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
) -> Result<HashSet<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    collect_excluded_commits_cached_with_parent_policy(repo, store, commit_cache, revs, false)
}

fn collect_excluded_commits_cached_with_parent_policy<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    first_parent_only: bool,
) -> Result<HashSet<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    collect_excluded_commits_cached_into(
        repo,
        store,
        commit_cache,
        revs,
        HashSet::with_capacity(root_traversal_capacity_hint(revs.len())),
        first_parent_only,
    )
}

fn collect_excluded_commits_cached_into<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    mut excluded: HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<HashSet<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    if revs.is_empty() {
        return Ok(excluded);
    }
    collect_commits_cached_into_set_with_parent_policy(
        repo,
        store,
        commit_cache,
        revs,
        &mut excluded,
        first_parent_only,
    )?;
    Ok(excluded)
}

fn collect_excluded_commits_from_ids_cached<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    roots: &[ObjectId],
) -> Result<HashSet<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    if roots.is_empty() {
        return Ok(HashSet::new());
    }
    let mut excluded = HashSet::with_capacity(root_traversal_capacity_hint(roots.len()));
    collect_commits_from_ids_cached_into_set(repo, commit_cache, roots, &mut excluded)?;
    Ok(excluded)
}

pub(crate) fn collect_commit_objects_with_exclusions_cached<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    collect_commit_objects_with_exclusions_cached_with_parent_policy(
        repo,
        store,
        commit_cache,
        revs,
        max_count,
        false,
    )
}

pub(crate) fn collect_commit_objects_with_exclusions_cached_with_parent_policy<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &RevListRevs,
    max_count: Option<usize>,
    first_parent_only: bool,
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    let excluded = collect_excluded_commits_cached_with_parent_policy(
        repo,
        store,
        commit_cache,
        &revs.exclude,
        revs.exclude_first_parent_only,
    )?;
    collect_commit_objects_cached_with_excluded(
        repo,
        store,
        commit_cache,
        &revs.include,
        max_count,
        &excluded,
        first_parent_only,
    )
}

fn collect_commit_objects_cached_with_excluded<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    let root_capacity = root_traversal_capacity_hint(revs.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in revs {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommit::new(
                read_pending_commit(commit_cache, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commit_objects(
        repo,
        commit_cache,
        pending,
        scheduled,
        sequence,
        max_count,
        excluded,
        first_parent_only,
    )
}

pub(crate) fn collect_commit_metadata_with_exclusions(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommitMetadata>> {
    collect_commit_metadata_with_exclusions_with_parent_policy(repo, store, revs, max_count, false)
}

pub(crate) fn collect_commit_metadata_with_exclusions_with_parent_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
    first_parent_only: bool,
) -> Result<Vec<CollectedCommitMetadata>> {
    let mut predicate = |_id: &ObjectId, _commit: &CommitObject| Ok(true);
    collect_commit_metadata_with_exclusions_with_parent_policy_and_predicate(
        repo,
        store,
        revs,
        max_count,
        first_parent_only,
        &mut predicate,
        false,
    )
}

pub(crate) fn collect_commit_metadata_with_exclusions_with_parent_policy_and_predicate<P>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
    first_parent_only: bool,
    predicate: &mut P,
    evaluate_predicate: bool,
) -> Result<Vec<CollectedCommitMetadata>>
where
    P: FnMut(&ObjectId, &CommitObject) -> Result<bool>,
{
    let excluded = if revs.exclude.is_empty() {
        HashSet::new()
    } else {
        collect_rev_list_excluded_commits_uncached(repo, store, revs)?
            .into_iter()
            .collect::<HashSet<_>>()
    };
    let root_capacity = root_traversal_capacity_hint(revs.include.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in &revs.include {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitMetadata::new(
                read_pending_commit_metadata_with_predicate(
                    store,
                    id,
                    predicate,
                    evaluate_predicate,
                )?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commit_metadata(
        repo,
        store,
        pending,
        scheduled,
        sequence,
        max_count,
        &excluded,
        first_parent_only,
        predicate,
        evaluate_predicate,
    )
}

pub(crate) fn collect_rev_list_object_commit_candidates_with_excluded<P>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
    predicate: &mut P,
) -> Result<Vec<RevListObjectCommitCandidate>>
where
    P: RevListObjectCommitPredicate,
{
    let mut roots = Vec::with_capacity(revs.include.len());
    let mut seen = HashSet::with_capacity(revs.include.len());
    for rev in &revs.include {
        let id = resolve_commitish(repo, store, rev)?;
        if seen.insert(id.clone()) {
            roots.push(id);
        }
    }
    collect_rev_list_object_commit_candidates_from_ids_with_excluded(
        repo, store, &roots, max_count, excluded, predicate,
    )
}

pub(crate) fn collect_rev_list_object_commit_candidates_from_ids_with_excluded<P>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    roots: &[ObjectId],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
    predicate: &mut P,
) -> Result<Vec<RevListObjectCommitCandidate>>
where
    P: RevListObjectCommitPredicate,
{
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for id in roots {
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingRevListObjectCommit::new(
                read_pending_rev_list_object_commit(store, id.clone(), predicate)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_rev_list_object_commits(
        repo, store, pending, scheduled, sequence, max_count, excluded, predicate,
    )
}

pub(crate) fn collect_commit_oneline_with_exclusions(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommitOneline>> {
    let excluded = if revs.exclude.is_empty() {
        HashSet::new()
    } else {
        collect_rev_list_excluded_commits_uncached(repo, store, revs)?
            .into_iter()
            .collect::<HashSet<_>>()
    };
    let root_capacity = root_traversal_capacity_hint(revs.include.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = HashSet::with_capacity(root_capacity);
    let mut sequence = 0_u64;
    for rev in &revs.include {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitOneline::new(
                read_pending_commit_oneline(store, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commit_oneline(
        repo, store, pending, scheduled, sequence, max_count, &excluded,
    )
}

pub(crate) fn collect_commits_with_exclusions_commit_graph(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Option<Vec<ObjectId>>> {
    if max_count == Some(0) || !read_shallow_commits(repo)?.is_empty() {
        return Ok(None);
    }
    let Some(commit_graph) = CommitGraphIndex::open(repo)? else {
        return Ok(None);
    };
    let excluded = if revs.exclude.is_empty() {
        HashSet::new()
    } else {
        collect_rev_list_excluded_commits_uncached(repo, store, revs)?
            .into_iter()
            .collect::<HashSet<_>>()
    };
    let root_capacity = root_traversal_capacity_hint(revs.include.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = vec![false; commit_graph.commit_count()];
    let mut scheduled_count = 0usize;
    let mut parents = Vec::with_capacity(2);
    let mut sequence = 0_u64;
    for rev in &revs.include {
        let id = resolve_commitish(repo, store, rev)?;
        let Some(position) = commit_graph.position(&id) else {
            return Ok(None);
        };
        let Some(timestamp) = commit_graph.committer_timestamp(position) else {
            return Ok(None);
        };
        if mark_commit_graph_position_seen(&mut scheduled, position) {
            scheduled_count += 1;
            pending.push(CommitGraphPendingPosition {
                position,
                timestamp,
                sequence,
            });
            sequence += 1;
        }
    }

    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled_count));
    while let Some(entry) = pending.pop() {
        let Some(id) = commit_graph.id_at(entry.position) else {
            return Ok(None);
        };
        if excluded.contains(&id) {
            continue;
        }
        out.push(id);
        if max_count.is_some_and(|limit| out.len() >= limit) {
            break;
        }
        commit_graph.parent_positions(entry.position, &mut parents)?;
        let desired_spare = parents.len().min(REV_LIST_INITIAL_CAPACITY_LIMIT);
        let pending_spare = pending.capacity().saturating_sub(pending.len());
        if pending_spare < desired_spare {
            pending.reserve(desired_spare);
        }
        for parent_position in &parents {
            if mark_commit_graph_position_seen(&mut scheduled, *parent_position) {
                let Some(timestamp) = commit_graph.committer_timestamp(*parent_position) else {
                    return Ok(None);
                };
                pending.push(CommitGraphPendingPosition {
                    position: *parent_position,
                    timestamp,
                    sequence,
                });
                sequence += 1;
            }
        }
    }
    Ok(Some(out))
}

pub(crate) fn collect_commit_render_hints_with_exclusions_commit_graph(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Option<Vec<CommitGraphRenderHint>>> {
    let _trace = phase_trace("log.collect_commits.commit_graph_hints");
    if max_count == Some(0) || !read_shallow_commits(repo)?.is_empty() {
        return Ok(None);
    }
    let Some(commit_graph) = CommitGraphIndex::open(repo)? else {
        return Ok(None);
    };
    let excluded = {
        let _trace = phase_trace("log.collect_commits.commit_graph_hints.excluded");
        if revs.exclude.is_empty() {
            HashSet::new()
        } else {
            collect_rev_list_excluded_commits_uncached(repo, store, revs)?
                .into_iter()
                .collect::<HashSet<_>>()
        }
    };
    let root_capacity = root_traversal_capacity_hint(revs.include.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = vec![false; commit_graph.commit_count()];
    let mut scheduled_count = 0usize;
    let mut parent_positions = Vec::with_capacity(2);
    let mut sequence = 0_u64;
    {
        let _trace = phase_trace("log.collect_commits.commit_graph_hints.roots");
        for rev in &revs.include {
            let id = resolve_commitish(repo, store, rev)?;
            let Some(position) = commit_graph.position(&id) else {
                return Ok(None);
            };
            let Some(timestamp) = commit_graph.committer_timestamp(position) else {
                return Ok(None);
            };
            if mark_commit_graph_position_seen(&mut scheduled, position) {
                scheduled_count += 1;
                pending.push(CommitGraphPendingPosition {
                    position,
                    timestamp,
                    sequence,
                });
                sequence += 1;
            }
        }
    }

    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled_count));
    {
        let _trace = phase_trace("log.collect_commits.commit_graph_hints.walk");
        while let Some(entry) = pending.pop() {
            let Some(id) = commit_graph.id_at(entry.position) else {
                return Ok(None);
            };
            if excluded.contains(&id) {
                continue;
            }
            commit_graph.parent_positions(entry.position, &mut parent_positions)?;
            let mut parents = Vec::with_capacity(parent_positions.len());
            let desired_spare = parent_positions.len().min(REV_LIST_INITIAL_CAPACITY_LIMIT);
            let pending_spare = pending.capacity().saturating_sub(pending.len());
            if pending_spare < desired_spare {
                pending.reserve(desired_spare);
            }
            for parent_position in &parent_positions {
                let Some(parent_id) = commit_graph.id_at(*parent_position) else {
                    return Ok(None);
                };
                parents.push(parent_id);
                if mark_commit_graph_position_seen(&mut scheduled, *parent_position) {
                    let Some(timestamp) = commit_graph.committer_timestamp(*parent_position) else {
                        return Ok(None);
                    };
                    pending.push(CommitGraphPendingPosition {
                        position: *parent_position,
                        timestamp,
                        sequence,
                    });
                    sequence += 1;
                }
            }
            let Ok(committer_timestamp) = i64::try_from(entry.timestamp) else {
                return Ok(None);
            };
            out.push(CommitGraphRenderHint {
                id,
                parents: Arc::from(parents),
                committer_timestamp,
            });
            if max_count.is_some_and(|limit| out.len() >= limit) {
                break;
            }
        }
    }
    Ok(Some(out))
}

pub(crate) fn collect_commit_render_hints_from_ids_with_exclusions_commit_graph(
    repo: &GitRepo,
    roots: &[ObjectId],
    max_count: Option<usize>,
) -> Result<Option<Vec<CommitGraphRenderHint>>> {
    let _trace = phase_trace("log.collect_commits.commit_graph_hints_exact_roots");
    if max_count == Some(0) || !read_shallow_commits(repo)?.is_empty() {
        return Ok(None);
    }
    let Some(commit_graph) = CommitGraphIndex::open(repo)? else {
        return Ok(None);
    };
    let root_capacity = root_traversal_capacity_hint(roots.len());
    let mut pending = BinaryHeap::with_capacity(root_capacity);
    let mut scheduled = vec![false; commit_graph.commit_count()];
    let mut scheduled_count = 0usize;
    let mut parent_positions = Vec::with_capacity(2);
    let mut sequence = 0_u64;
    {
        let _trace = phase_trace("log.collect_commits.commit_graph_hints_exact_roots.roots");
        for id in roots {
            let Some(position) = commit_graph.position(id) else {
                return Ok(None);
            };
            let Some(timestamp) = commit_graph.committer_timestamp(position) else {
                return Ok(None);
            };
            if mark_commit_graph_position_seen(&mut scheduled, position) {
                scheduled_count += 1;
                pending.push(CommitGraphPendingPosition {
                    position,
                    timestamp,
                    sequence,
                });
                sequence += 1;
            }
        }
    }

    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled_count));
    {
        let _trace = phase_trace("log.collect_commits.commit_graph_hints_exact_roots.walk");
        while let Some(entry) = pending.pop() {
            let Some(id) = commit_graph.id_at(entry.position) else {
                return Ok(None);
            };
            commit_graph.parent_positions(entry.position, &mut parent_positions)?;
            let mut parents = Vec::with_capacity(parent_positions.len());
            let desired_spare = parent_positions.len().min(REV_LIST_INITIAL_CAPACITY_LIMIT);
            let pending_spare = pending.capacity().saturating_sub(pending.len());
            if pending_spare < desired_spare {
                pending.reserve(desired_spare);
            }
            for parent_position in &parent_positions {
                let Some(parent_id) = commit_graph.id_at(*parent_position) else {
                    return Ok(None);
                };
                parents.push(parent_id);
                if mark_commit_graph_position_seen(&mut scheduled, *parent_position) {
                    let Some(timestamp) = commit_graph.committer_timestamp(*parent_position) else {
                        return Ok(None);
                    };
                    pending.push(CommitGraphPendingPosition {
                        position: *parent_position,
                        timestamp,
                        sequence,
                    });
                    sequence += 1;
                }
            }
            let Ok(committer_timestamp) = i64::try_from(entry.timestamp) else {
                return Ok(None);
            };
            out.push(CommitGraphRenderHint {
                id,
                parents: Arc::from(parents),
                committer_timestamp,
            });
            if max_count.is_some_and(|limit| out.len() >= limit) {
                break;
            }
        }
    }
    Ok(Some(out))
}

fn collect_pending_commit_objects<S>(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, S>,
    mut pending: BinaryHeap<HeapPendingCommit>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
    first_parent_only: bool,
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled.len()));
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let id = pending_commit.id;
        let commit = pending_commit.commit;
        if excluded.contains(&id) {
            continue;
        }
        let is_shallow = shallow_commits.contains(&id);
        if !is_shallow {
            let parent_count = if first_parent_only {
                commit.parents.len().min(1)
            } else {
                commit.parents.len()
            };
            reserve_commit_parent_traversal(&mut pending, &mut scheduled, parent_count);
            for parent in commit.parents.iter().take(parent_count) {
                if excluded.contains(parent) {
                    continue;
                }
                if scheduled.insert(parent.clone()) {
                    let parent_pending = read_pending_commit(commit_cache, parent.clone())?;
                    pending.push(HeapPendingCommit::new(parent_pending, sequence));
                    sequence += 1;
                }
            }
        }
        out.push(CollectedCommit {
            id,
            commit: Arc::clone(&commit),
        });
        if max_count.is_some_and(|max| out.len() >= max) {
            break;
        }
        if is_shallow {
            continue;
        }
    }
    Ok(out)
}

fn collect_pending_commit_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingCommitMetadata>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
    first_parent_only: bool,
    predicate: &mut impl FnMut(&ObjectId, &CommitObject) -> Result<bool>,
    evaluate_predicate: bool,
) -> Result<Vec<CollectedCommitMetadata>> {
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled.len()));
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let metadata = pending_commit.metadata;
        if excluded.contains(&metadata.id) {
            continue;
        }
        let is_shallow = shallow_commits.contains(&metadata.id);
        if !is_shallow {
            let parent_count = if first_parent_only {
                metadata.parents.len().min(1)
            } else {
                metadata.parents.len()
            };
            reserve_commit_parent_traversal(&mut pending, &mut scheduled, parent_count);
            for parent in metadata.parents.iter().take(parent_count) {
                if excluded.contains(parent) {
                    continue;
                }
                if scheduled.insert(parent.clone()) {
                    pending.push(HeapPendingCommitMetadata::new(
                        read_pending_commit_metadata_with_predicate(
                            store,
                            parent.clone(),
                            predicate,
                            evaluate_predicate,
                        )?,
                        sequence,
                    ));
                    sequence += 1;
                }
            }
        }
        out.push(metadata);
        if max_count.is_some_and(|max| out.len() >= max) {
            break;
        }
    }
    Ok(out)
}

fn collect_pending_rev_list_object_commits<P>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingRevListObjectCommit>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
    predicate: &mut P,
) -> Result<Vec<RevListObjectCommitCandidate>>
where
    P: RevListObjectCommitPredicate,
{
    let shallow_commits = read_shallow_commits(repo)?;
    // Full-schedule callers intentionally traverse the complete candidate pool;
    // retain the bound only as an output-capacity hint for simple callers.
    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled.len()));
    let mut original_sequence = 0usize;
    while let Some(heap_entry) = pending.pop() {
        let mut pending_commit = heap_entry.pending;
        let id = pending_commit.candidate.id.clone();
        let is_excluded = excluded.contains(&id);
        let is_shallow = shallow_commits.contains(&id);
        if !is_excluded && !is_shallow {
            reserve_commit_parent_traversal(
                &mut pending,
                &mut scheduled,
                pending_commit.candidate.parents.len(),
            );
            let parents = if predicate.first_parent_only() {
                pending_commit.candidate.parents.iter().take(1)
            } else {
                pending_commit.candidate.parents.iter().take(usize::MAX)
            };
            for parent in parents {
                if scheduled.insert(parent.clone()) {
                    pending.push(HeapPendingRevListObjectCommit::new(
                        read_pending_rev_list_object_commit(store, parent.clone(), predicate)?,
                        sequence,
                    ));
                    sequence += 1;
                }
            }
        }
        if is_excluded {
            pending_commit.candidate.matches = false;
        }
        pending_commit.candidate.sequence = original_sequence;
        original_sequence += 1;
        out.push(pending_commit.candidate);
    }
    Ok(out)
}

fn collect_pending_commit_oneline(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut pending: BinaryHeap<HeapPendingCommitOneline>,
    mut scheduled: HashSet<ObjectId>,
    mut sequence: u64,
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
) -> Result<Vec<CollectedCommitOneline>> {
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::with_capacity(commit_output_capacity_hint(max_count, scheduled.len()));
    while let Some(heap_entry) = pending.pop() {
        let commit = heap_entry.pending.commit;
        if excluded.contains(&commit.id) {
            continue;
        }
        let is_shallow = shallow_commits.contains(&commit.id);
        if !is_shallow {
            reserve_commit_parent_traversal(&mut pending, &mut scheduled, commit.parents.len());
            for parent in &commit.parents {
                if excluded.contains(parent) {
                    continue;
                }
                if scheduled.insert(parent.clone()) {
                    pending.push(HeapPendingCommitOneline::new(
                        read_pending_commit_oneline(store, parent.clone())?,
                        sequence,
                    ));
                    sequence += 1;
                }
            }
        }
        out.push(commit);
        if max_count.is_some_and(|max| out.len() >= max) {
            break;
        }
    }
    Ok(out)
}

pub(crate) fn collect_rev_list_excluded_commits(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
) -> Result<Vec<ObjectId>> {
    collect_rev_list_excluded_commits_uncached(repo, store, revs)
}

#[cfg(test)]
pub(crate) fn collect_rev_list_excluded_commits_from_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
    exclude_roots: &[ObjectId],
    exclude_revs: &[String],
) -> Result<Vec<ObjectId>> {
    let commit_cache = CommitObjectCache::new(store);
    collect_rev_list_excluded_commits_from_ids_cached(
        repo,
        store,
        &commit_cache,
        exclude_roots,
        exclude_revs,
    )
}

pub(crate) fn collect_rev_list_excluded_commits_from_ids_cached<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    exclude_roots: &[ObjectId],
    exclude_revs: &[String],
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    if exclude_roots.is_empty() && exclude_revs.is_empty() {
        return Ok(Vec::new());
    }
    let mut excluded = collect_commits_from_ids_cached(repo, commit_cache, exclude_roots, None)?;
    if !exclude_revs.is_empty() {
        let mut seen = HashSet::with_capacity(rev_list_excluded_seen_capacity_hint(excluded.len()));
        seen.extend(excluded.iter().cloned());
        for id in collect_commits_cached(repo, store, commit_cache, exclude_revs, None)? {
            if seen.insert(id.clone()) {
                excluded.push(id);
            }
        }
    }
    Ok(excluded)
}

pub(crate) fn collect_rev_list_excluded_commits_uncached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
) -> Result<Vec<ObjectId>> {
    if revs.exclude.is_empty() {
        return Ok(Vec::new());
    }
    collect_commits_uncached_with_parent_policy(
        repo,
        store,
        &revs.exclude,
        None,
        revs.exclude_first_parent_only,
    )
}

pub(crate) fn count_rev_list_objects(
    store: &LooseObjectStore,
    commits: &[ObjectId],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
) -> Result<usize> {
    for_each_rev_list_object_line_with(
        store,
        commits,
        extra_objects,
        excluded_commits,
        RevListTreeMissingPolicy::default(),
        |_, _, _| Ok(()),
    )
}

pub(crate) fn count_rev_list_object_refs(
    store: &LooseObjectStore,
    commits: &[&ObjectId],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
) -> Result<usize> {
    for_each_rev_list_object_line_with_refs(
        store,
        commits,
        extra_objects,
        excluded_commits,
        RevListTreeMissingPolicy::default(),
        |_, _, _| Ok(()),
    )
}

fn rev_list_seen_capacity_hint(
    commits_len: usize,
    extra_objects_len: usize,
    excluded_commits_len: usize,
) -> usize {
    commits_len
        .saturating_add(extra_objects_len)
        .saturating_add(excluded_commits_len)
        .min(REV_LIST_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn reserve_rev_list_seen_spare(
    seen: &mut HashSet<ObjectId>,
    commits_len: usize,
    extra_objects_len: usize,
    excluded_commits_len: usize,
) {
    let desired_spare =
        rev_list_seen_capacity_hint(commits_len, extra_objects_len, excluded_commits_len);
    let spare = seen.capacity().saturating_sub(seen.len());
    if spare < desired_spare {
        seen.reserve(desired_spare);
    }
}

fn commit_output_capacity_hint(max_count: Option<usize>, scheduled_len: usize) -> usize {
    max_count
        .unwrap_or(scheduled_len)
        .min(REV_LIST_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn mark_commit_graph_position_seen(seen: &mut [bool], position: u32) -> bool {
    let Some(slot) = seen.get_mut(position as usize) else {
        return false;
    };
    if *slot {
        return false;
    }
    *slot = true;
    true
}

fn reserve_commit_parent_traversal<T, S>(
    pending: &mut BinaryHeap<T>,
    scheduled: &mut HashSet<S>,
    parents_len: usize,
) where
    S: std::hash::Hash + Eq,
{
    let desired_spare = parents_len.min(REV_LIST_INITIAL_CAPACITY_LIMIT);
    let pending_spare = pending.capacity().saturating_sub(pending.len());
    if pending_spare < desired_spare {
        pending.reserve(desired_spare);
    }
    let scheduled_spare = scheduled.capacity().saturating_sub(scheduled.len());
    if scheduled_spare < desired_spare {
        scheduled.reserve(desired_spare);
    }
}

fn reserve_commit_depth_parent_traversal<T>(
    pending: &mut VecDeque<T>,
    depths: &mut HashMap<ObjectId, usize>,
    parents_len: usize,
) {
    let desired_spare = parents_len.min(REV_LIST_INITIAL_CAPACITY_LIMIT);
    let pending_spare = pending.capacity().saturating_sub(pending.len());
    if pending_spare < desired_spare {
        pending.reserve(desired_spare);
    }
    let depth_spare = depths.capacity().saturating_sub(depths.len());
    if depth_spare < desired_spare {
        depths.reserve(desired_spare);
    }
}

fn root_traversal_capacity_hint(roots_len: usize) -> usize {
    roots_len.min(REV_LIST_INITIAL_CAPACITY_LIMIT)
}

fn rev_list_excluded_seen_capacity_hint(excluded_len: usize) -> usize {
    excluded_len.min(REV_LIST_INITIAL_CAPACITY_LIMIT)
}

fn tree_walk_stack_capacity_hint() -> usize {
    8
}

fn reserve_tree_walk_children<T>(pending: &mut Vec<T>, entries_len: usize) {
    let desired_spare = entries_len.min(REV_LIST_INITIAL_CAPACITY_LIMIT);
    let spare = pending.capacity().saturating_sub(pending.len());
    if spare < desired_spare {
        pending.reserve(desired_spare);
    }
}

fn tree_cache_capacity_hint(commits_len: usize, excluded_commits_len: usize) -> usize {
    commits_len
        .saturating_add(excluded_commits_len)
        .min(REV_LIST_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn reserve_ancestor_parent_traversal<T>(
    stack: &mut Vec<T>,
    scheduled: &mut HashSet<ObjectId>,
    parents_len: usize,
) {
    let desired_spare = parents_len.min(REV_LIST_INITIAL_CAPACITY_LIMIT);
    let stack_spare = stack.capacity().saturating_sub(stack.len());
    if stack_spare < desired_spare {
        stack.reserve(desired_spare);
    }
    let scheduled_spare = scheduled.capacity().saturating_sub(scheduled.len());
    if scheduled_spare < desired_spare {
        scheduled.reserve(desired_spare);
    }
}

fn schedule_ancestor_parent(
    stack: &mut Vec<ObjectId>,
    scheduled: &mut HashSet<ObjectId>,
    parent: &ObjectId,
) {
    if scheduled.insert(parent.clone()) {
        stack.push(parent.clone());
    }
}

fn common_merge_base_candidate_capacity(left_len: usize, right_len: usize) -> usize {
    left_len.min(right_len).min(REV_LIST_INITIAL_CAPACITY_LIMIT)
}

fn should_replace_merge_base_candidate(
    best: Option<&(usize, ObjectId)>,
    score: usize,
    id: &ObjectId,
) -> bool {
    match best {
        Some((best_score, best_id)) => {
            score < *best_score || (score == *best_score && id.as_bytes() < best_id.as_bytes())
        }
        None => true,
    }
}

pub(crate) fn for_each_rev_list_object_line_with<F>(
    store: &LooseObjectStore,
    commits: &[ObjectId],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
    missing_policy: RevListTreeMissingPolicy<'_>,
    visit: F,
) -> Result<usize>
where
    F: FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
{
    let commit_refs = commits.iter().collect::<Vec<_>>();
    for_each_rev_list_object_line_with_refs(
        store,
        &commit_refs,
        extra_objects,
        excluded_commits,
        missing_policy,
        visit,
    )
}

pub(crate) fn for_each_rev_list_object_line_with_refs<F>(
    store: &LooseObjectStore,
    commits: &[&ObjectId],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
    missing_policy: RevListTreeMissingPolicy<'_>,
    mut visit: F,
) -> Result<usize>
where
    F: FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
{
    let mut seen = CompactRevListSeenObjectIds::with_capacity(
        store,
        rev_list_seen_capacity_hint(commits.len(), extra_objects.len(), excluded_commits.len()),
    )
    .map_err(CliError::Io)?;
    let mut count = 0usize;
    let mut links_cache = CommitLinksCache::new(store);
    let mut ref_tree_cache = TreeObjectRefCache::with_capacity(
        store,
        tree_cache_capacity_hint(commits.len(), excluded_commits.len()),
    );
    for commit_id in excluded_commits {
        let links = match links_cache.read_links(commit_id) {
            Ok(links) => links,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        collect_rev_list_tree_object_leaf_ids(
            &mut ref_tree_cache,
            &links.tree,
            &mut seen,
            missing_policy,
        )?;
    }
    for (id, name) in extra_objects {
        for_each_rev_list_extra_object_line(
            store,
            id,
            name.as_bytes(),
            &mut seen,
            missing_policy,
            &mut visit,
            &mut count,
        )?;
    }
    let mut path = Vec::new();
    for commit_id in commits {
        let links = links_cache.read_links(commit_id).map_err(CliError::Io)?;
        for_each_rev_list_tree_object_line(
            store,
            &links.tree,
            &mut path,
            &mut seen,
            missing_policy,
            &mut visit,
            &mut count,
        )?;
    }
    Ok(count)
}

pub(crate) fn for_each_rev_list_object_line_with_trees<F>(
    store: &LooseObjectStore,
    commits: &[CollectedCommitTree],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
    missing_policy: RevListTreeMissingPolicy<'_>,
    mut visit: F,
) -> Result<usize>
where
    F: FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
{
    let mut seen = CompactRevListSeenObjectIds::with_capacity(
        store,
        rev_list_seen_capacity_hint(commits.len(), extra_objects.len(), excluded_commits.len()),
    )
    .map_err(CliError::Io)?;
    let mut count = 0usize;
    let mut ref_tree_cache = TreeObjectRefCache::with_capacity(
        store,
        tree_cache_capacity_hint(commits.len(), excluded_commits.len()),
    );
    for commit_id in excluded_commits {
        let tree = match read_commit_tree_uncached(store, commit_id) {
            Ok(tree) => tree,
            Err(CliError::Io(error)) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        collect_rev_list_tree_object_leaf_ids(
            &mut ref_tree_cache,
            &tree,
            &mut seen,
            missing_policy,
        )?;
    }
    for (id, name) in extra_objects {
        for_each_rev_list_extra_object_line(
            store,
            id,
            name.as_bytes(),
            &mut seen,
            missing_policy,
            &mut visit,
            &mut count,
        )?;
    }
    let mut path = Vec::new();
    for commit in commits {
        if commit.is_boundary {
            continue;
        }
        for_each_rev_list_tree_object_line(
            store,
            &commit.tree,
            &mut path,
            &mut seen,
            missing_policy,
            &mut visit,
            &mut count,
        )?;
    }
    Ok(count)
}

pub(crate) fn for_each_rev_list_object_id_with_trees<F>(
    store: &LooseObjectStore,
    commits: &[CollectedCommitTree],
    extra_objects: &[ObjectId],
    excluded_commits: &[ObjectId],
    missing_policy: RevListTreeMissingPolicy<'_>,
    mut visit: F,
) -> Result<usize>
where
    F: FnMut(&ObjectId, Option<GitObjectKind>) -> Result<()>,
{
    let named_extra_objects = extra_objects
        .iter()
        .cloned()
        .map(|id| (id, String::new()))
        .collect::<Vec<_>>();
    for_each_rev_list_object_line_with_trees(
        store,
        commits,
        &named_extra_objects,
        excluded_commits,
        missing_policy,
        |id, kind, _| visit(id, kind),
    )
}

pub(crate) fn for_each_history_path_object_line_with_trees<F>(
    store: &LooseObjectStore,
    trees: &[ObjectId],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
    query: &HistoryPathQuery,
    missing_policy: RevListTreeMissingPolicy<'_>,
    mut visit: F,
) -> Result<usize>
where
    F: FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
{
    for_each_history_path_object_line_with_roots(
        store,
        trees.iter(),
        extra_objects,
        excluded_commits,
        query,
        missing_policy,
        visit,
    )
}

pub(crate) fn for_each_history_path_object_line_with_tree_indices<F>(
    store: &LooseObjectStore,
    ids: &[ObjectId],
    tree_indices: &[usize],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
    query: &HistoryPathQuery,
    missing_policy: RevListTreeMissingPolicy<'_>,
    visit: F,
) -> Result<usize>
where
    F: FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
{
    let roots = tree_indices
        .iter()
        .map(|index| {
            ids.get(*index).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "path history tree index missing from shared object table".into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    for_each_history_path_object_line_with_roots(
        store,
        roots.into_iter(),
        extra_objects,
        excluded_commits,
        query,
        missing_policy,
        visit,
    )
}

fn for_each_history_path_object_line_with_roots<'a, I, F>(
    store: &LooseObjectStore,
    trees: I,
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
    query: &HistoryPathQuery,
    missing_policy: RevListTreeMissingPolicy<'_>,
    mut visit: F,
) -> Result<usize>
where
    I: Iterator<Item = &'a ObjectId>,
    F: FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
{
    let mut seen = CompactRevListSeenObjectIds::with_capacity(
        store,
        rev_list_seen_capacity_hint(0, extra_objects.len(), excluded_commits.len()),
    )
    .map_err(CliError::Io)?;
    let mut count = 0usize;
    for commit_id in excluded_commits {
        let tree = match read_commit_tree_uncached(store, commit_id) {
            Ok(tree) => tree,
            Err(CliError::Io(error)) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        collect_history_path_tree_leaf_ids(store, &tree, query, missing_policy, &mut seen)?;
    }
    for (id, name) in extra_objects {
        for_each_rev_list_extra_object_line(
            store,
            id,
            name.as_bytes(),
            &mut seen,
            missing_policy,
            &mut visit,
            &mut count,
        )?;
    }
    let mut path = Vec::new();
    for tree in trees {
        for_each_history_path_tree_object_line(
            store,
            tree,
            query,
            &mut path,
            &mut seen,
            missing_policy,
            &mut visit,
            &mut count,
        )?;
    }
    Ok(count)
}

fn collect_history_path_tree_leaf_ids(
    store: &LooseObjectStore,
    tree_id: &ObjectId,
    query: &HistoryPathQuery,
    missing_policy: RevListTreeMissingPolicy<'_>,
    seen: &mut impl RevListSeenObjectIds,
) -> Result<()> {
    struct PendingPathLeafTree {
        id: ObjectId,
        path_len: usize,
        content: Option<Vec<u8>>,
        cursor: usize,
    }

    let mut active_trees = HashSet::new();
    let mut path = Vec::new();
    let mut pending = vec![PendingPathLeafTree {
        id: tree_id.clone(),
        path_len: 0,
        content: None,
        cursor: 0,
    }];
    while !pending.is_empty() {
        let frame_index = pending.len() - 1;
        let frame_path_len = pending[frame_index].path_len;
        if !query.may_match_prefix(&path[..frame_path_len]) {
            let frame_id = pending[frame_index].id.clone();
            pending.pop();
            active_trees.remove(&frame_id);
            path.truncate(frame_path_len);
            continue;
        }
        if pending[frame_index].content.is_none() {
            let frame_id = pending[frame_index].id.clone();
            if !active_trees.insert(frame_id.clone()) {
                pending.pop();
                path.truncate(frame_path_len);
                continue;
            }
            let content = read_rev_list_tree_content(store, &frame_id, missing_policy)?;
            if content.is_none() {
                pending.pop();
                active_trees.remove(&frame_id);
                path.truncate(frame_path_len);
                continue;
            }
            pending[frame_index].content = content;
        }
        let next_entry = {
            let frame = &mut pending[frame_index];
            let content = frame
                .content
                .as_ref()
                .expect("path leaf tree content loaded before iteration");
            decode_tree_entry_ref(frame.id.algorithm(), content, &mut frame.cursor)
                .map_err(CliError::Io)?
                .map(|entry| {
                    path.truncate(frame.path_len);
                    if !path.is_empty() {
                        path.push(b'/');
                    }
                    path.extend_from_slice(entry.name);
                    (entry.id, entry.mode, path.len())
                })
        };
        let Some((entry_id, entry_mode, child_path_len)) = next_entry else {
            let frame_id = pending[frame_index].id.clone();
            pending.pop();
            active_trees.remove(&frame_id);
            path.truncate(frame_path_len);
            continue;
        };
        if entry_mode == TreeMode::Tree {
            pending.push(PendingPathLeafTree {
                id: entry_id,
                path_len: child_path_len,
                content: None,
                cursor: 0,
            });
        } else if query.matches_path(&path)
            && rev_list_object_is_visible(store, &entry_id, missing_policy)?
        {
            seen.insert(entry_id);
        }
    }
    Ok(())
}

fn for_each_history_path_tree_object_line(
    store: &LooseObjectStore,
    tree_id: &ObjectId,
    query: &HistoryPathQuery,
    path: &mut Vec<u8>,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
    visit: &mut dyn FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
    count: &mut usize,
) -> Result<()> {
    struct PendingPathTree {
        id: ObjectId,
        path_len: usize,
        content: Option<Vec<u8>>,
        cursor: usize,
    }

    if !query.may_match_prefix(path) {
        return Ok(());
    }
    let initial_path_len = path.len();
    let mut pending = vec![PendingPathTree {
        id: tree_id.clone(),
        path_len: initial_path_len,
        content: None,
        cursor: 0,
    }];
    let mut active_trees = HashSet::new();
    while !pending.is_empty() {
        let frame_index = pending.len() - 1;
        let frame_path_len = pending[frame_index].path_len;
        if pending[frame_index].content.is_none() {
            let frame_id = pending[frame_index].id.clone();
            if !active_trees.insert(frame_id.clone()) {
                path.truncate(frame_path_len);
                pending.pop();
                continue;
            }
            let Some(content) = read_rev_list_tree_content(store, &frame_id, missing_policy)?
            else {
                path.truncate(frame_path_len);
                pending.pop();
                active_trees.remove(&frame_id);
                continue;
            };
            if seen.insert(frame_id.clone()) {
                visit(&frame_id, Some(GitObjectKind::Tree), Some(path))?;
                *count += 1;
            }
            pending[frame_index].content = Some(content);
        }
        let next_entry = {
            let frame = &mut pending[frame_index];
            let content = frame
                .content
                .as_ref()
                .expect("path tree content loaded before iteration");
            decode_tree_entry_ref(frame.id.algorithm(), content, &mut frame.cursor)
                .map_err(CliError::Io)?
                .map(|entry| {
                    path.truncate(frame.path_len);
                    if !path.is_empty() {
                        path.push(b'/');
                    }
                    path.extend_from_slice(entry.name);
                    (entry.id, entry.mode, path.len())
                })
        };
        let Some((entry_id, entry_mode, child_path_len)) = next_entry else {
            let frame_id = pending[frame_index].id.clone();
            path.truncate(frame_path_len);
            pending.pop();
            active_trees.remove(&frame_id);
            continue;
        };
        if entry_mode == TreeMode::Tree {
            if query.may_match_prefix(&path) {
                pending.push(PendingPathTree {
                    id: entry_id,
                    path_len: child_path_len,
                    content: None,
                    cursor: 0,
                });
            }
            continue;
        }
        if query.matches_path(path)
            && rev_list_object_is_visible(store, &entry_id, missing_policy)?
            && seen.insert(entry_id.clone())
        {
            let kind = match entry_mode {
                TreeMode::Gitlink => GitObjectKind::Commit,
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink => GitObjectKind::Blob,
                TreeMode::Tree => unreachable!("tree entries are handled above"),
            };
            visit(&entry_id, Some(kind), Some(path))?;
            *count += 1;
        }
    }
    path.truncate(initial_path_len);
    Ok(())
}

fn for_each_rev_list_extra_object_line<F>(
    store: &LooseObjectStore,
    id: &ObjectId,
    name: &[u8],
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
    visit: &mut F,
    count: &mut usize,
) -> Result<()>
where
    F: FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
{
    let kind = match object_kind_hint_or_read(store, id) {
        Ok(kind) => kind,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound && missing_policy.emit_missing_objects =>
        {
            if seen.insert(id.clone()) {
                visit(id, None, Some(name))?;
                *count += 1;
            }
            return Ok(());
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    match kind {
        GitObjectKind::Tree => {
            let mut path = name.to_vec();
            for_each_rev_list_tree_object_line(
                store,
                id,
                &mut path,
                seen,
                missing_policy,
                visit,
                count,
            )
        }
        _ => {
            if seen.insert(id.clone()) {
                visit(id, Some(kind), Some(name))?;
                *count += 1;
            }
            Ok(())
        }
    }
}

pub(crate) fn collect_rev_list_object_ids_into_cached(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
    extra_objects: &[ObjectId],
    excluded_commits: &[ObjectId],
    seen: &mut HashSet<ObjectId>,
    objects: &mut Vec<ObjectId>,
) -> Result<()> {
    for_each_rev_list_object_id_into_cached(
        store,
        commit_cache,
        commits,
        extra_objects,
        excluded_commits,
        seen,
        |id| {
            objects.push(id.clone());
            Ok(())
        },
    )
}

pub(crate) fn for_each_rev_list_object_id_into_cached<F>(
    store: &LooseObjectStore,
    _commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
    extra_objects: &[ObjectId],
    excluded_commits: &[ObjectId],
    seen: &mut HashSet<ObjectId>,
    mut visit: F,
) -> Result<()>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    reserve_rev_list_seen_spare(
        seen,
        commits.len(),
        extra_objects.len(),
        excluded_commits.len(),
    );
    let mut tree_cache = TreeObjectRefCache::transient(store);
    let mut links_cache = CommitLinksCache::new(store);
    for commit_id in excluded_commits {
        let links = match links_cache.read_links(commit_id) {
            Ok(links) => links,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        collect_rev_list_tree_object_leaf_ids(
            &mut tree_cache,
            &links.tree,
            seen,
            RevListTreeMissingPolicy::default(),
        )?;
    }
    for id in extra_objects {
        for_each_rev_list_extra_object_id_into_cached(
            store,
            &mut links_cache,
            &mut tree_cache,
            id,
            seen,
            &mut visit,
        )?;
    }
    for commit_id in commits {
        let links = links_cache.read_links(commit_id).map_err(CliError::Io)?;
        for_each_rev_list_tree_id_ordered_into(&mut tree_cache, &links.tree, seen, &mut visit)?;
    }
    for commit_id in commits {
        let links = links_cache.read_links(commit_id).map_err(CliError::Io)?;
        for_each_rev_list_non_tree_object_id_ordered_into(
            &mut tree_cache,
            &links.tree,
            seen,
            &mut visit,
        )?;
    }
    Ok(())
}

fn for_each_rev_list_extra_object_id_into_cached<F>(
    store: &LooseObjectStore,
    links_cache: &mut CommitLinksCache<'_, LooseObjectStore>,
    tree_cache: &mut TreeObjectRefCache<'_>,
    id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
    visit: &mut F,
) -> Result<()>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    match object_kind_hint_or_read(store, id)? {
        GitObjectKind::Tree => {
            for_each_rev_list_tree_id_ordered_into(tree_cache, id, seen, visit)?;
            for_each_rev_list_non_tree_object_id_ordered_into(tree_cache, id, seen, visit)
        }
        GitObjectKind::Commit => {
            let links = links_cache.read_links(id).map_err(CliError::Io)?;
            for_each_rev_list_tree_id_ordered_into(tree_cache, &links.tree, seen, visit)?;
            for_each_rev_list_non_tree_object_id_ordered_into(tree_cache, &links.tree, seen, visit)
        }
        _ => {
            let _ = visit_rev_list_object_id(id, seen, visit)?;
            Ok(())
        }
    }
}

fn for_each_rev_list_tree_id_ordered_into<F>(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
    visit: &mut F,
) -> Result<()>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(tree_id.clone());
    while let Some(id) = pending.pop() {
        if !visit_rev_list_object_id(&id, seen, visit)? {
            continue;
        }
        let entries = tree_cache.read_tree(&id)?;
        reserve_tree_walk_children(&mut pending, entries.len());
        for entry in entries.iter().rev() {
            if entry.mode == TreeMode::Tree {
                pending.push(entry.id.clone());
            }
        }
    }
    Ok(())
}

fn for_each_rev_list_non_tree_object_id_ordered_into<F>(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
    visit: &mut F,
) -> Result<()>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(tree_id.clone());
    while let Some(id) = pending.pop() {
        let entries = tree_cache.read_tree(&id)?;
        reserve_tree_walk_children(&mut pending, entries.len());
        for entry in entries.iter() {
            if matches!(
                entry.mode,
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink
            ) {
                visit_rev_list_object_id(&entry.id, seen, visit)?;
            }
        }
        for entry in entries.iter().rev() {
            if entry.mode == TreeMode::Tree {
                pending.push(entry.id.clone());
            }
        }
    }
    Ok(())
}

pub(crate) fn for_each_rev_list_object_path_cached<F>(
    store: &LooseObjectStore,
    _commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
    excluded_commits: &[ObjectId],
    mut visit: F,
) -> Result<()>
where
    F: FnMut(&ObjectId, GitObjectKind, Option<&[u8]>) -> Result<()>,
{
    let mut seen = HashSet::with_capacity(rev_list_seen_capacity_hint(
        commits.len(),
        0,
        excluded_commits.len(),
    ));
    let mut ref_tree_cache = TreeObjectRefCache::with_capacity(
        store,
        tree_cache_capacity_hint(commits.len(), excluded_commits.len()),
    );
    let tree_cache = TreeObjectCache::new(store);
    let mut links_cache = CommitLinksCache::new(store);
    for commit_id in excluded_commits {
        let links = links_cache.read_links(commit_id).map_err(CliError::Io)?;
        collect_rev_list_tree_object_ids(&mut ref_tree_cache, &links.tree, &mut seen)?;
    }
    let mut path = Vec::new();
    for commit_id in commits {
        let links = links_cache.read_links(commit_id).map_err(CliError::Io)?;
        collect_rev_list_tree_object_paths(
            &tree_cache,
            &links.tree,
            &mut path,
            &mut seen,
            &mut visit,
        )?;
    }
    Ok(())
}

pub(crate) fn count_rev_list_objects_uncached(
    store: &LooseObjectStore,
    commits: &[CollectedCommitTree],
    extra_objects: &[(ObjectId, String)],
    excluded_commits: &[ObjectId],
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<usize> {
    let mut seen = CompactRevListSeenObjectIds::with_capacity(
        store,
        rev_list_seen_capacity_hint(commits.len(), extra_objects.len(), excluded_commits.len()),
    )
    .map_err(CliError::Io)?;
    let mut count = 0usize;
    let mut tree_cache = TreeObjectRefCache::with_capacity(
        store,
        tree_cache_capacity_hint(commits.len(), excluded_commits.len()),
    );
    for commit_id in excluded_commits {
        let tree = match read_commit_tree_uncached(store, commit_id) {
            Ok(tree) => tree,
            Err(CliError::Io(error)) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        collect_rev_list_tree_object_leaf_ids(&mut tree_cache, &tree, &mut seen, missing_policy)?;
    }
    for (id, _) in extra_objects {
        count +=
            count_rev_list_extra_object_ids(store, &mut tree_cache, id, &mut seen, missing_policy)?;
    }
    for commit in commits {
        if commit.is_boundary {
            continue;
        }
        count += count_rev_list_tree_ref_objects(
            &mut tree_cache,
            &commit.tree,
            &mut seen,
            missing_policy,
        )?;
    }
    Ok(count)
}

fn write_rev_list_tree_object_ref_ids_ordered<W: Write>(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
    out: &mut W,
) -> Result<()> {
    enum PendingTreeObject {
        Tree(ObjectId),
        Object(ObjectId),
    }

    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(PendingTreeObject::Tree(tree_id.clone()));
    while let Some(item) = pending.pop() {
        match item {
            PendingTreeObject::Tree(id) => {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Some(entries) =
                    read_rev_list_tree_ref_entries(tree_cache, &id, missing_policy)?
                else {
                    if missing_policy.emits_missing_object(&id) {
                        out.write_all(b"?")?;
                        id.write_hex_io(out)?;
                        out.write_all(b"\n")?;
                    }
                    continue;
                };
                id.write_hex_io(out)?;
                out.write_all(b"\n")?;
                reserve_tree_walk_children(&mut pending, entries.len());
                for entry in entries.iter().rev() {
                    if matches!(
                        entry.mode,
                        TreeMode::File
                            | TreeMode::Executable
                            | TreeMode::Symlink
                            | TreeMode::Gitlink
                    ) && rev_list_object_is_visible(tree_cache.store, &entry.id, missing_policy)?
                    {
                        pending.push(PendingTreeObject::Object(entry.id.clone()));
                    }
                }
                for entry in entries.iter().rev() {
                    if entry.mode == TreeMode::Tree {
                        pending.push(PendingTreeObject::Tree(entry.id.clone()));
                    }
                }
            }
            PendingTreeObject::Object(id) => {
                if seen.insert(id.clone()) {
                    id.write_hex_io(out)?;
                    out.write_all(b"\n")?;
                }
            }
        }
    }
    Ok(())
}

fn write_rev_list_extra_object_ids_ordered<W: Write>(
    store: &LooseObjectStore,
    tree_cache: &mut TreeObjectRefCache<'_>,
    id: &ObjectId,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
    out: &mut W,
) -> Result<()> {
    match object_kind_hint_or_read(store, id)? {
        GitObjectKind::Tree => {
            write_rev_list_tree_object_ref_ids_ordered(tree_cache, id, seen, missing_policy, out)
        }
        _ => {
            if seen.insert(id.clone()) {
                id.write_hex_io(out)?;
                out.write_all(b"\n")?;
            }
            Ok(())
        }
    }
}

fn collect_rev_list_tree_object_ref_ids(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<()> {
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(tree_id.clone());
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(entries) = read_rev_list_tree_ref_entries(tree_cache, &id, missing_policy)? else {
            continue;
        };
        reserve_tree_walk_children(&mut pending, entries.len());
        for entry in entries.iter() {
            match entry.mode {
                TreeMode::Tree => pending.push(entry.id.clone()),
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                    if rev_list_object_is_visible(tree_cache.store, &entry.id, missing_policy)? {
                        seen.insert(entry.id.clone());
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_rev_list_tree_object_leaf_ids(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<()> {
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(tree_id.clone());
    while let Some(id) = pending.pop() {
        let Some(entries) = read_rev_list_tree_ref_entries(tree_cache, &id, missing_policy)? else {
            continue;
        };
        reserve_tree_walk_children(&mut pending, entries.len());
        for entry in entries.iter() {
            match entry.mode {
                TreeMode::Tree => pending.push(entry.id.clone()),
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                    if rev_list_object_is_visible(tree_cache.store, &entry.id, missing_policy)? {
                        seen.insert(entry.id.clone());
                    }
                }
            }
        }
    }
    Ok(())
}

fn count_rev_list_tree_ref_objects(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<usize> {
    let mut count = 0usize;
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(tree_id.clone());
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(entries) = read_rev_list_tree_ref_entries(tree_cache, &id, missing_policy)? else {
            if missing_policy.emits_missing_object(&id) {
                count += 1;
            }
            continue;
        };
        count += 1;
        reserve_tree_walk_children(&mut pending, entries.len());
        for entry in entries.iter() {
            match entry.mode {
                TreeMode::Tree => pending.push(entry.id.clone()),
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                    if rev_list_object_is_visible(tree_cache.store, &entry.id, missing_policy)? {
                        count += usize::from(seen.insert(entry.id.clone()));
                    }
                }
            }
        }
    }
    Ok(count)
}

fn count_rev_list_extra_object_ids(
    store: &LooseObjectStore,
    tree_cache: &mut TreeObjectRefCache<'_>,
    id: &ObjectId,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<usize> {
    match object_kind_hint_or_read(store, id)? {
        GitObjectKind::Tree => {
            count_rev_list_tree_ref_objects(tree_cache, id, seen, missing_policy)
        }
        _ => Ok(usize::from(seen.insert(id.clone()))),
    }
}

#[cfg(test)]
fn for_each_rev_list_tree_object_id_ordered_into<F>(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
    visit: &mut F,
) -> Result<()>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    enum PendingTreeObject {
        Tree(ObjectId),
        Object(ObjectId),
    }

    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(PendingTreeObject::Tree(tree_id.clone()));
    while let Some(item) = pending.pop() {
        match item {
            PendingTreeObject::Tree(id) => {
                if !visit_rev_list_object_id(&id, seen, visit)? {
                    continue;
                }
                let entries = tree_cache.read_tree(&id)?;
                reserve_tree_walk_children(&mut pending, entries.len());
                for entry in entries.iter().rev() {
                    if matches!(
                        entry.mode,
                        TreeMode::File
                            | TreeMode::Executable
                            | TreeMode::Symlink
                            | TreeMode::Gitlink
                    ) {
                        pending.push(PendingTreeObject::Object(entry.id.clone()));
                    }
                }
                for entry in entries.iter().rev() {
                    if entry.mode == TreeMode::Tree {
                        pending.push(PendingTreeObject::Tree(entry.id.clone()));
                    }
                }
            }
            PendingTreeObject::Object(id) => {
                visit_rev_list_object_id(&id, seen, visit)?;
            }
        }
    }
    Ok(())
}

fn visit_rev_list_object_id<F>(
    id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
    visit: &mut F,
) -> Result<bool>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    if seen.insert(id.clone()) {
        visit(id)?;
        return Ok(true);
    }
    Ok(false)
}

fn collect_rev_list_tree_object_ids(
    tree_cache: &mut TreeObjectRefCache<'_>,
    tree_id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(tree_id.clone());
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let entries = tree_cache.read_tree(&id)?;
        reserve_tree_walk_children(&mut pending, entries.len());
        for entry in entries.iter() {
            match entry.mode {
                TreeMode::Tree => pending.push(entry.id.clone()),
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                    seen.insert(entry.id.clone());
                }
            }
        }
    }
    Ok(())
}

fn for_each_rev_list_tree_object_line(
    store: &LooseObjectStore,
    tree_id: &ObjectId,
    path: &mut Vec<u8>,
    seen: &mut impl RevListSeenObjectIds,
    missing_policy: RevListTreeMissingPolicy<'_>,
    visit: &mut dyn FnMut(&ObjectId, Option<GitObjectKind>, Option<&[u8]>) -> Result<()>,
    count: &mut usize,
) -> Result<()> {
    struct PendingTreeLine {
        id: ObjectId,
        path_len: usize,
        content: Option<Vec<u8>>,
        cursor: usize,
    }

    let initial_path_len = path.len();
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(PendingTreeLine {
        id: tree_id.clone(),
        path_len: initial_path_len,
        content: None,
        cursor: 0,
    });
    while !pending.is_empty() {
        let Some(frame) = pending.last_mut() else {
            break;
        };
        if frame.content.is_none() {
            if !seen.insert(frame.id.clone()) {
                path.truncate(frame.path_len);
                pending.pop();
                continue;
            }
            let Some(content) = read_rev_list_tree_content(store, &frame.id, missing_policy)?
            else {
                if missing_policy.emits_missing_object(&frame.id) {
                    visit(&frame.id, None, Some(path))?;
                    *count += 1;
                }
                path.truncate(frame.path_len);
                pending.pop();
                continue;
            };
            visit(&frame.id, Some(GitObjectKind::Tree), Some(path))?;
            *count += 1;
            frame.content = Some(content);
            continue;
        }

        let next_entry = (|| -> Result<Option<(ObjectId, TreeMode, usize)>> {
            let Some(frame) = pending.last_mut() else {
                return Ok(None);
            };
            let content = frame
                .content
                .as_ref()
                .expect("tree frame content loaded before iteration");
            let Some(entry) =
                decode_tree_entry_ref(frame.id.algorithm(), content, &mut frame.cursor)?
            else {
                return Ok(None);
            };
            path.truncate(frame.path_len);
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(entry.name);
            Ok(Some((entry.id, entry.mode, path.len())))
        })()?;
        let Some((entry_id, entry_mode, child_path_len)) = next_entry else {
            let frame = pending.last().expect("pending frame");
            path.truncate(frame.path_len);
            pending.pop();
            continue;
        };
        match entry_mode {
            TreeMode::Tree => pending.push(PendingTreeLine {
                id: entry_id,
                path_len: child_path_len,
                content: None,
                cursor: 0,
            }),
            TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                if rev_list_object_is_visible(store, &entry_id, missing_policy)?
                    && seen.insert(entry_id.clone())
                {
                    let kind = match entry_mode {
                        TreeMode::Gitlink => GitObjectKind::Commit,
                        TreeMode::File | TreeMode::Executable | TreeMode::Symlink => {
                            GitObjectKind::Blob
                        }
                        TreeMode::Tree => unreachable!("tree entries are handled above"),
                    };
                    visit(&entry_id, Some(kind), Some(path))?;
                    *count += 1;
                }
            }
        }
    }
    path.truncate(initial_path_len);
    Ok(())
}

fn read_rev_list_tree_ref_entries(
    tree_cache: &mut TreeObjectRefCache<'_>,
    id: &ObjectId,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<Option<Arc<Vec<TreeObjectRef>>>> {
    if missing_policy.excludes_promisor_object(id) {
        return Ok(None);
    }
    match tree_cache.read_tree(id) {
        Ok(entries) => Ok(Some(entries)),
        Err(CliError::Io(error))
            if error.kind() == io::ErrorKind::NotFound && missing_policy.fatal_on_missing_tree =>
        {
            Err(missing_tree_error(id))
        }
        Err(CliError::Io(error))
            if error.kind() == io::ErrorKind::NotFound
                && missing_policy.allows_missing_object(id) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn read_rev_list_tree_content(
    store: &LooseObjectStore,
    id: &ObjectId,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<Option<Vec<u8>>> {
    if missing_policy.excludes_promisor_object(id) {
        return Ok(None);
    }
    match store.read_object(id) {
        Ok(object) if object.kind == GitObjectKind::Tree => Ok(Some(object.content)),
        Ok(_) => Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a tree",
        ))),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound && missing_policy.fatal_on_missing_tree =>
        {
            Err(missing_tree_error(id))
        }
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && missing_policy.allows_missing_object(id) =>
        {
            Ok(None)
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn rev_list_object_is_visible(
    store: &LooseObjectStore,
    id: &ObjectId,
    missing_policy: RevListTreeMissingPolicy<'_>,
) -> Result<bool> {
    if missing_policy.excludes_promisor_object(id) {
        return Ok(false);
    }
    if !missing_policy.allows_missing_object(id) {
        return Ok(true);
    }
    let present = store.contains_object(id).map_err(CliError::Io)?;
    Ok(present || missing_policy.emits_missing_object(id))
}

fn collect_rev_list_tree_object_paths(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_id: &ObjectId,
    path: &mut Vec<u8>,
    seen: &mut HashSet<ObjectId>,
    visit: &mut dyn FnMut(&ObjectId, GitObjectKind, Option<&[u8]>) -> Result<()>,
) -> Result<()> {
    struct PendingTreePath {
        id: ObjectId,
        path_len: usize,
        entries: Option<Arc<[zmin_git_core::TreeEntry]>>,
        next: usize,
    }

    let initial_path_len = path.len();
    let mut pending = Vec::with_capacity(tree_walk_stack_capacity_hint());
    pending.push(PendingTreePath {
        id: tree_id.clone(),
        path_len: initial_path_len,
        entries: None,
        next: 0,
    });
    while !pending.is_empty() {
        let Some(frame) = pending.last_mut() else {
            break;
        };
        if frame.entries.is_none() {
            if !seen.insert(frame.id.clone()) {
                path.truncate(frame.path_len);
                pending.pop();
                continue;
            }
            let path = if path.is_empty() {
                None
            } else {
                Some(path.as_slice())
            };
            visit(&frame.id, GitObjectKind::Tree, path)?;
            frame.entries = Some(tree_cache.read_tree(&frame.id)?);
            continue;
        }

        let Some((entry_id, entry_mode, child_path_len)) = (|| {
            let frame = pending.last_mut()?;
            let entries = frame
                .entries
                .as_ref()
                .expect("tree frame entries loaded before iteration");
            if frame.next == entries.len() {
                return None;
            }
            let entry = &entries[frame.next];
            frame.next += 1;
            path.truncate(frame.path_len);
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(&entry.name);
            Some((entry.id.clone(), entry.mode, path.len()))
        })() else {
            let frame = pending.last().expect("pending frame");
            path.truncate(frame.path_len);
            pending.pop();
            continue;
        };
        match entry_mode {
            TreeMode::Tree => pending.push(PendingTreePath {
                id: entry_id,
                path_len: child_path_len,
                entries: None,
                next: 0,
            }),
            TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                if seen.insert(entry_id.clone()) {
                    visit(
                        &entry_id,
                        tree_entry_object_kind(entry_mode),
                        Some(path.as_slice()),
                    )?;
                }
            }
        }
    }
    path.truncate(initial_path_len);
    Ok(())
}

fn tree_entry_object_kind(mode: TreeMode) -> GitObjectKind {
    match mode {
        TreeMode::Tree => GitObjectKind::Tree,
        TreeMode::File | TreeMode::Executable | TreeMode::Symlink => GitObjectKind::Blob,
        TreeMode::Gitlink => GitObjectKind::Commit,
    }
}

pub(crate) fn commit_depths_cached(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    start: &ObjectId,
) -> Result<HashMap<ObjectId, usize>> {
    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    let mut depths = HashMap::with_capacity(1024);
    depths.insert(start.clone(), 0usize);
    let mut pending = VecDeque::from([start.clone()]);
    while let Some(id) = pending.pop_front() {
        let depth = depths[&id];
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        let parent_depth = depth.checked_add(1).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit depth overflow".into(),
        })?;
        reserve_commit_depth_parent_traversal(&mut pending, &mut depths, links.parents.len());
        for parent in &links.parents {
            if let Entry::Vacant(entry) = depths.entry(parent.clone()) {
                pending.push_back(entry.key().clone());
                entry.insert(parent_depth);
            }
        }
    }
    Ok(depths)
}

pub(crate) fn commit_depths_with_repo_cached(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    start: &ObjectId,
) -> Result<HashMap<ObjectId, usize>> {
    let shallow_commits = read_shallow_commits(repo)?;
    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    let mut depths = HashMap::with_capacity(1024);
    depths.insert(start.clone(), 0usize);
    let mut pending = VecDeque::from([start.clone()]);
    while let Some(id) = pending.pop_front() {
        if shallow_commits.contains(&id) {
            continue;
        }
        let depth = depths[&id];
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        let parent_depth = depth.checked_add(1).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit depth overflow".into(),
        })?;
        reserve_commit_depth_parent_traversal(&mut pending, &mut depths, links.parents.len());
        for parent in &links.parents {
            if let Entry::Vacant(entry) = depths.entry(parent.clone()) {
                pending.push_back(entry.key().clone());
                entry.insert(parent_depth);
            }
        }
    }
    Ok(depths)
}

pub(crate) fn best_merge_base_cached(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Option<ObjectId>> {
    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    if is_ancestor_commit_with_links_cache(&mut links_cache, right, left)? {
        return Ok(Some(right.clone()));
    }
    if is_ancestor_commit_with_links_cache(&mut links_cache, left, right)? {
        return Ok(Some(left.clone()));
    }

    let left_depths = commit_depths_with_links_cache(&mut links_cache, left)?;
    let right_depths = commit_depths_with_links_cache(&mut links_cache, right)?;
    let (scan_depths, lookup_depths) = if left_depths.len() <= right_depths.len() {
        (&left_depths, &right_depths)
    } else {
        (&right_depths, &left_depths)
    };
    let mut best = None::<(usize, ObjectId)>;
    for (id, scan_depth) in scan_depths {
        let Some(lookup_depth) = lookup_depths.get(id) else {
            continue;
        };
        let score = scan_depth + lookup_depth;
        if should_replace_merge_base_candidate(best.as_ref(), score, id) {
            best = Some((score, id.clone()));
        }
    }
    Ok(best.map(|(_, id)| id))
}

pub(crate) fn best_merge_base_with_repo_cached(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Option<ObjectId>> {
    let shallow_commits = read_shallow_commits(repo)?;
    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    if is_ancestor_links_cache_with_shallow(&mut links_cache, &shallow_commits, right, left)? {
        return Ok(Some(right.clone()));
    }
    if is_ancestor_links_cache_with_shallow(&mut links_cache, &shallow_commits, left, right)? {
        return Ok(Some(left.clone()));
    }

    let left_depths =
        commit_depths_with_links_cache_and_shallow(&mut links_cache, &shallow_commits, left)?;
    let right_depths =
        commit_depths_with_links_cache_and_shallow(&mut links_cache, &shallow_commits, right)?;
    let (scan_depths, lookup_depths) = if left_depths.len() <= right_depths.len() {
        (&left_depths, &right_depths)
    } else {
        (&right_depths, &left_depths)
    };
    let mut best = None::<(usize, ObjectId)>;
    for (id, scan_depth) in scan_depths {
        let Some(lookup_depth) = lookup_depths.get(id) else {
            continue;
        };
        let score = scan_depth + lookup_depth;
        if should_replace_merge_base_candidate(best.as_ref(), score, id) {
            best = Some((score, id.clone()));
        }
    }
    Ok(best.map(|(_, id)| id))
}

pub(crate) fn best_merge_base_with_commit_graph_cached(
    commit_graph: Option<&CommitGraphIndex>,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Option<ObjectId>> {
    if let Some(commit_graph) = commit_graph {
        if commit_graph.is_ancestor(right, left)? == Some(true) {
            return Ok(Some(right.clone()));
        }
        if commit_graph.is_ancestor(left, right)? == Some(true) {
            return Ok(Some(left.clone()));
        }
    }
    best_merge_base_cached(commit_cache, left, right)
}

pub(crate) fn merge_bases_all_cached(
    store: &LooseObjectStore,
    _commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Vec<ObjectId>> {
    let mut links_cache = CommitLinksCache::new(store);
    if is_ancestor_commit_with_links_cache(&mut links_cache, right, left)? {
        return Ok(vec![right.clone()]);
    }
    if is_ancestor_commit_with_links_cache(&mut links_cache, left, right)? {
        return Ok(vec![left.clone()]);
    }

    let left_depths = commit_depths_with_links_cache(&mut links_cache, left)?;
    let right_depths = commit_depths_with_links_cache(&mut links_cache, right)?;
    let (scan_depths, lookup_depths) = if left_depths.len() <= right_depths.len() {
        (&left_depths, &right_depths)
    } else {
        (&right_depths, &left_depths)
    };
    let common_ids = ordered_common_merge_base_candidates(store, &mut links_cache, left, right)?;
    let mut candidates = Vec::with_capacity(common_merge_base_candidate_capacity(
        left_depths.len(),
        right_depths.len(),
    ));
    for id in common_ids {
        if scan_depths.contains_key(&id) && lookup_depths.contains_key(&id) {
            candidates.push(id);
        }
    }

    let mut bases = Vec::new();
    for candidate in &candidates {
        let mut is_redundant = false;
        for other in &candidates {
            if candidate != other
                && is_ancestor_commit_with_links_cache(&mut links_cache, candidate, other)?
            {
                is_redundant = true;
                break;
            }
        }
        if !is_redundant {
            bases.push(candidate.clone());
        }
    }
    Ok(bases)
}

fn commit_depths_with_links_cache(
    links_cache: &mut CommitLinksCache<'_, LooseObjectStore>,
    start: &ObjectId,
) -> Result<HashMap<ObjectId, usize>> {
    let mut depths = HashMap::with_capacity(1024);
    depths.insert(start.clone(), 0usize);
    let mut pending = VecDeque::from([start.clone()]);
    while let Some(id) = pending.pop_front() {
        let depth = depths[&id];
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        let parent_depth = depth.checked_add(1).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit depth overflow".into(),
        })?;
        reserve_commit_depth_parent_traversal(&mut pending, &mut depths, links.parents.len());
        for parent in &links.parents {
            if let Entry::Vacant(entry) = depths.entry(parent.clone()) {
                pending.push_back(entry.key().clone());
                entry.insert(parent_depth);
            }
        }
    }
    Ok(depths)
}

fn commit_depths_with_links_cache_and_shallow(
    links_cache: &mut CommitLinksCache<'_, LooseObjectStore>,
    shallow_commits: &HashSet<ObjectId>,
    start: &ObjectId,
) -> Result<HashMap<ObjectId, usize>> {
    let mut depths = HashMap::with_capacity(1024);
    depths.insert(start.clone(), 0usize);
    let mut pending = VecDeque::from([start.clone()]);
    while let Some(id) = pending.pop_front() {
        if shallow_commits.contains(&id) {
            continue;
        }
        let depth = depths[&id];
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        let parent_depth = depth.checked_add(1).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit depth overflow".into(),
        })?;
        reserve_commit_depth_parent_traversal(&mut pending, &mut depths, links.parents.len());
        for parent in &links.parents {
            if let Entry::Vacant(entry) = depths.entry(parent.clone()) {
                pending.push_back(entry.key().clone());
                entry.insert(parent_depth);
            }
        }
    }
    Ok(depths)
}

fn is_ancestor_commit_with_links_cache(
    links_cache: &mut CommitLinksCache<'_, LooseObjectStore>,
    ancestor: &ObjectId,
    descendant: &ObjectId,
) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }
    let mut seen = HashSet::new();
    let mut pending = vec![descendant.clone()];
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        if links.parents.iter().any(|parent| parent == ancestor) {
            return Ok(true);
        }
        pending.extend(links.parents.iter().cloned());
    }
    Ok(false)
}

fn is_ancestor_links_cache_with_shallow(
    links_cache: &mut CommitLinksCache<'_, LooseObjectStore>,
    shallow_commits: &HashSet<ObjectId>,
    ancestor: &ObjectId,
    descendant: &ObjectId,
) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }
    let mut seen = HashSet::new();
    let mut pending = vec![descendant.clone()];
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) || shallow_commits.contains(&id) {
            continue;
        }
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        for parent in &links.parents {
            if parent == ancestor {
                return Ok(true);
            }
            pending.push(parent.clone());
        }
    }
    Ok(false)
}

#[derive(Clone, Eq, PartialEq)]
struct MergeBasePending {
    id: ObjectId,
    timestamp: i64,
    sequence: u64,
}

impl Ord for MergeBasePending {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp
            .cmp(&other.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for MergeBasePending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn read_commit_committer_timestamp(store: &LooseObjectStore, id: &ObjectId) -> Result<i64> {
    let object = store.read_object(id).map_err(CliError::Io)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    parse_commit_signature_timestamps(&object.content)
        .map(|(_, timestamp)| timestamp)
        .map_err(CliError::Io)
}

fn ordered_common_merge_base_candidates(
    store: &LooseObjectStore,
    links_cache: &mut CommitLinksCache<'_, LooseObjectStore>,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Vec<ObjectId>> {
    const LEFT_SIDE: u8 = 1;
    const RIGHT_SIDE: u8 = 2;
    const STALE: u8 = 4;

    let mut pending = BinaryHeap::new();
    let mut flags = HashMap::<ObjectId, u8>::new();
    let mut sequence = 0_u64;
    for (id, side) in [(left, LEFT_SIDE), (right, RIGHT_SIDE)] {
        let entry = flags.entry(id.clone()).or_insert(0);
        *entry |= side;
        let timestamp = read_commit_committer_timestamp(store, id)?;
        pending.push(MergeBasePending {
            id: id.clone(),
            timestamp,
            sequence,
        });
        sequence += 1;
    }

    let mut candidates = Vec::new();
    let mut result_ids = HashSet::new();
    while let Some(entry) = pending.pop() {
        let current_flags = flags.get(&entry.id).copied().unwrap_or_default();
        if current_flags & (LEFT_SIDE | RIGHT_SIDE) == (LEFT_SIDE | RIGHT_SIDE) {
            if result_ids.insert(entry.id.clone()) {
                candidates.push(entry.id.clone());
            }
            flags
                .entry(entry.id.clone())
                .and_modify(|value| *value |= STALE);
        }
        let propagation_flags = flags.get(&entry.id).copied().unwrap_or_default();
        let links = links_cache.read_links(&entry.id).map_err(CliError::Io)?;
        for parent in links.parents.iter() {
            let parent_flags = flags.entry(parent.clone()).or_insert(0);
            let new_flags = *parent_flags | propagation_flags;
            if new_flags == *parent_flags {
                continue;
            }
            *parent_flags = new_flags;
            let timestamp = read_commit_committer_timestamp(store, parent)?;
            pending.push(MergeBasePending {
                id: parent.clone(),
                timestamp,
                sequence,
            });
            sequence += 1;
        }
    }
    Ok(candidates)
}

pub(crate) fn best_multi_merge_base_cached(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
) -> Result<Option<ObjectId>> {
    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    let left_depths = commit_depths_with_links_cache(&mut links_cache, &commits[0])?;
    let mut other_depths = Vec::with_capacity(commits.len().saturating_sub(1));
    for commit in &commits[1..] {
        other_depths.push(commit_depths_with_links_cache(&mut links_cache, commit)?);
    }

    let mut best = None::<(usize, ObjectId)>;
    for (id, left_depth) in &left_depths {
        let Some(nearest_other_depth) = other_depths
            .iter()
            .filter_map(|depths| depths.get(id))
            .min()
        else {
            continue;
        };
        let score = *left_depth + *nearest_other_depth;
        if should_replace_merge_base_candidate(best.as_ref(), score, id) {
            best = Some((score, id.clone()));
        }
    }

    Ok(best.map(|(_, id)| id))
}

pub(crate) fn best_octopus_merge_base_cached(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
) -> Result<Option<ObjectId>> {
    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    let left_depths = commit_depths_with_links_cache(&mut links_cache, &commits[0])?;
    let mut other_depths = Vec::with_capacity(commits.len().saturating_sub(1));
    for commit in &commits[1..] {
        other_depths.push(commit_depths_with_links_cache(&mut links_cache, commit)?);
    }
    let mut scan_depths = &left_depths;
    for depths in &other_depths {
        if depths.len() < scan_depths.len() {
            scan_depths = depths;
        }
    }

    let mut best = None::<(usize, ObjectId)>;
    for id in scan_depths.keys() {
        let Some(left_depth) = left_depths.get(id) else {
            continue;
        };
        let mut score = *left_depth;
        let mut present_in_all = true;
        for depths in &other_depths {
            if let Some(depth) = depths.get(id) {
                score += depth;
            } else {
                present_in_all = false;
                break;
            }
        }
        if !present_in_all {
            continue;
        }
        if should_replace_merge_base_candidate(best.as_ref(), score, id) {
            best = Some((score, id.clone()));
        }
    }

    Ok(best.map(|(_, id)| id))
}

pub(crate) fn is_ancestor_commit_cached(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    ancestor: &ObjectId,
    descendant: &ObjectId,
) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }

    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    let mut seen: Option<HashSet<ObjectId>> = None;
    let mut stack = Vec::with_capacity(128);
    let mut current = descendant.clone();

    loop {
        let links = links_cache.read_links(&current).map_err(CliError::Io)?;

        if current == *ancestor {
            return Ok(true);
        }

        if links.parents.is_empty() {
            return Ok(false);
        }

        if links.parents.len() == 1 {
            if let Some(seen) = seen.as_mut() {
                if !seen.insert(current.clone()) {
                    break;
                }
            }

            let parent = &links.parents[0];
            if parent == ancestor {
                return Ok(true);
            }
            current = parent.clone();
            continue;
        }

        let seen_nodes = seen.get_or_insert_with(|| HashSet::with_capacity(128));
        if !seen_nodes.insert(current.clone()) {
            break;
        }

        reserve_ancestor_parent_traversal(&mut stack, seen_nodes, links.parents.len());
        for parent in &links.parents {
            if parent == ancestor {
                return Ok(true);
            }
            schedule_ancestor_parent(&mut stack, seen_nodes, parent);
        }
        break;
    }

    while let Some(id) = stack.pop() {
        if id == *ancestor {
            return Ok(true);
        }
        let Some(seen) = seen.as_mut() else {
            break;
        };
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        reserve_ancestor_parent_traversal(&mut stack, seen, links.parents.len());
        for parent in &links.parents {
            if parent == ancestor {
                return Ok(true);
            }
            schedule_ancestor_parent(&mut stack, seen, parent);
        }
    }
    Ok(false)
}

pub(crate) fn is_ancestor_commit_with_repo_cached(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    ancestor: &ObjectId,
    descendant: &ObjectId,
) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }

    let shallow_commits = read_shallow_commits(repo)?;
    let mut links_cache = CommitLinksCache::new(commit_cache.store());
    let mut seen: Option<HashSet<ObjectId>> = None;
    let mut stack = Vec::with_capacity(128);
    let mut current = descendant.clone();

    loop {
        if shallow_commits.contains(&current) {
            return Ok(false);
        }
        let links = links_cache.read_links(&current).map_err(CliError::Io)?;

        if current == *ancestor {
            return Ok(true);
        }

        if links.parents.is_empty() {
            return Ok(false);
        }

        if links.parents.len() == 1 {
            if let Some(seen) = seen.as_mut() {
                if !seen.insert(current.clone()) {
                    break;
                }
            }

            let parent = &links.parents[0];
            if parent == ancestor {
                return Ok(true);
            }
            current = parent.clone();
            continue;
        }

        let seen_nodes = seen.get_or_insert_with(|| HashSet::with_capacity(128));
        if !seen_nodes.insert(current.clone()) {
            break;
        }

        reserve_ancestor_parent_traversal(&mut stack, seen_nodes, links.parents.len());
        for parent in &links.parents {
            if parent == ancestor {
                return Ok(true);
            }
            schedule_ancestor_parent(&mut stack, seen_nodes, parent);
        }
        break;
    }

    while let Some(id) = stack.pop() {
        if id == *ancestor {
            return Ok(true);
        }
        if shallow_commits.contains(&id) {
            continue;
        }
        let Some(seen) = seen.as_mut() else {
            break;
        };
        let links = links_cache.read_links(&id).map_err(CliError::Io)?;
        reserve_ancestor_parent_traversal(&mut stack, seen, links.parents.len());
        for parent in &links.parents {
            if parent == ancestor {
                return Ok(true);
            }
            schedule_ancestor_parent(&mut stack, seen, parent);
        }
    }
    Ok(false)
}

pub(crate) fn is_ancestor_commit_uncached(
    store: &LooseObjectStore,
    ancestor: &ObjectId,
    descendant: &ObjectId,
) -> Result<bool> {
    if ancestor == descendant {
        return Ok(true);
    }

    let mut stack = Vec::with_capacity(128);
    let mut seen: Option<HashSet<ObjectId>> = None;
    let mut current = descendant.clone();
    let mut parents = Vec::with_capacity(2);

    loop {
        parents.clear();
        let parents_len = read_commit_parents_uncached_into(store, &current, &mut parents)?;

        if parents_len == 0 {
            return Ok(false);
        }
        if parents_len == 1 {
            if seen.as_ref().is_some_and(|seen| seen.contains(&current)) {
                return Ok(false);
            }
            let parent = &parents[0];
            if parent == ancestor {
                return Ok(true);
            }
            if let Some(seen) = seen.as_mut() {
                seen.insert(current.clone());
            }
            current = parent.clone();
            continue;
        }

        let seen_nodes = seen.get_or_insert_with(|| HashSet::with_capacity(128));
        if !seen_nodes.insert(current.clone()) {
            return Ok(false);
        }

        reserve_ancestor_parent_traversal(&mut stack, seen_nodes, parents_len);
        for parent in &parents[..parents_len] {
            if parent == ancestor {
                return Ok(true);
            }
            schedule_ancestor_parent(&mut stack, seen_nodes, parent);
        }
        break;
    }

    while let Some(id) = stack.pop() {
        if id == *ancestor {
            return Ok(true);
        }
        let Some(seen) = seen.as_mut() else {
            return Ok(false);
        };

        parents.clear();
        let parents_len = read_commit_parents_uncached_into(store, &id, &mut parents)?;
        reserve_ancestor_parent_traversal(&mut stack, seen, parents_len);
        for parent in &parents[..parents_len] {
            if parent == ancestor {
                return Ok(true);
            }
            schedule_ancestor_parent(&mut stack, seen, parent);
        }
    }
    Ok(false)
}

pub(crate) fn resolve_commitish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commitish: &str,
) -> Result<ObjectId> {
    resolve_commitish_io(repo, store, commitish).map_err(CliError::Io)
}

pub(crate) fn resolve_commitish_or_bad_revision(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commitish: &str,
) -> Result<ObjectId> {
    resolve_commitish_io(repo, store, commitish).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("bad revision '{commitish}'"),
    })
}

pub(crate) fn resolve_commitish_for_ancestor_check_with_graph_cached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commit_graph: Option<&CommitGraphIndex>,
    commitish: &str,
) -> Result<ObjectId> {
    if let Some((base, depth)) = commitish.split_once('~') {
        if base.is_empty() || depth.contains('~') || depth.contains('^') {
            return Err(CliError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("ambiguous argument '{commitish}'"),
            )));
        }
        let generations = if depth.is_empty() {
            1
        } else {
            depth.parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid parent shorthand: {commitish}"),
                )
            })?
        };
        let mut id = resolve_commitish_for_ancestor_check_with_graph_cached(
            repo,
            store,
            commit_cache,
            commit_graph,
            base,
        )?;
        if let Some(commit_graph) = commit_graph {
            if let Some(parent) = commit_graph.first_parent_after(&id, generations)? {
                return Ok(parent);
            }
        }
        for _ in 0..generations {
            let object = store.read_object(&id)?;
            if object.kind != GitObjectKind::Commit {
                return Err(CliError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("revision `{base}` is not a commit"),
                )));
            }
            let commit = commit_cache.read_loaded_commit(object)?;
            let Some(parent) = commit.parents.first() else {
                return Err(CliError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("revision '{commitish}' has no parent"),
                )));
            };
            id = parent.clone();
        }
        return Ok(id);
    }

    if let Some(id) =
        parse_full_object_id_if_commit(store, commitish).map_err(|err| CliError::Fatal {
            code: 128,
            message: err.to_string(),
        })?
    {
        return Ok(id);
    }

    let id = resolve_objectish(repo, commitish).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("Not a valid object name {commitish}"),
    })?;
    if commit_graph.is_some_and(|commit_graph| commit_graph.position(&id).is_some()) {
        return Ok(id);
    }
    match object_kind_hint_or_read(store, &id)? {
        GitObjectKind::Commit => Ok(id),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("Not a valid commit name `{commitish}`"),
        }),
    }
}

fn parse_full_object_id_if_commit(
    store: &LooseObjectStore,
    objectish: &str,
) -> io::Result<Option<ObjectId>> {
    if objectish.len() != store.algorithm().digest_len() * 2 {
        return Ok(None);
    }
    if let Ok(id) = ObjectId::from_hex(store.algorithm(), objectish) {
        if store.object_kind_hint(&id)?.is_none() {
            return Ok(None);
        }
        return parse_commit_full_object_id_if_commit(store, objectish, id);
    }
    Ok(None)
}

fn parse_commit_full_object_id_if_commit(
    store: &LooseObjectStore,
    input: &str,
    id: ObjectId,
) -> io::Result<Option<ObjectId>> {
    let object = store.read_object(&id)?;
    match object.kind {
        GitObjectKind::Commit => Ok(Some(id)),
        GitObjectKind::Tag => peel_tag_chain_to_commit(store, &id)
            .map(|chain| chain.commit)
            .map_err(|error| io::Error::new(error.kind(), format!("{input}: {error}"))),
        _ => Ok(None),
    }
}

pub(crate) fn resolve_treeish_or_invalid_object(
    repo: &GitRepo,
    store: &LooseObjectStore,
    treeish: &str,
) -> Result<ObjectId> {
    resolve_treeish(repo, store, treeish).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("Not a valid object name {treeish}"),
    })
}

pub(crate) fn ambiguous_revision_error(rev: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "ambiguous argument '{rev}': unknown revision or path not in the working tree.\n\
             Use '--' to separate paths from revisions, like this:\n\
             'git <command> [<revision>...] -- [<file>...]'"
        ),
    }
}

fn broken_current_branch_error() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "your current branch appears to be broken".into(),
    }
}

fn map_head_resolution_error(rev: &str, error: &io::Error) -> Option<CliError> {
    if matches!(rev, "HEAD" | "@")
        && (matches!(
            error.kind(),
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData
        ) || error.to_string().contains("reference broken"))
    {
        Some(broken_current_branch_error())
    } else {
        None
    }
}

pub(crate) fn resolve_commitish_io(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commitish: &str,
) -> io::Result<ObjectId> {
    let commit_cache = CommitObjectCache::new(store);
    resolve_commitish_io_cached(repo, store, &commit_cache, commitish)
}

pub(crate) fn resolve_commitish_io_cached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commitish: &str,
) -> io::Result<ObjectId> {
    let tilde_index = commitish.rfind('~');
    let caret_index = commitish.rfind('^').filter(|index| {
        let suffix = &commitish[index + 1..];
        suffix.is_empty() || suffix.bytes().all(|byte| byte.is_ascii_digit())
    });
    if matches!((tilde_index, caret_index), (Some(tilde), Some(caret)) if caret > tilde)
        || matches!((tilde_index, caret_index), (None, Some(_)))
    {
        let caret = caret_index.expect("checked above");
        let base = &commitish[..caret];
        let parent = &commitish[caret + 1..];
        if base.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("ambiguous argument '{commitish}'"),
            ));
        }
        let parent_index = if parent.is_empty() {
            1
        } else {
            parent.parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid parent shorthand: {commitish}"),
                )
            })?
        };
        let id = resolve_commitish_io_cached(repo, store, commit_cache, base)?;
        if parent_index == 0 {
            return Ok(id);
        }
        let object = store.read_object(&id)?;
        if object.kind != GitObjectKind::Commit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("revision `{base}` is not a commit"),
            ));
        }
        let commit = commit_cache.read_loaded_commit(object)?;
        let Some(parent) = commit.parents.get(parent_index - 1) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("revision '{commitish}' has no parent"),
            ));
        };
        return Ok(parent.clone());
    }
    if let Some((base, depth)) = commitish.split_once('~') {
        if base.is_empty() || depth.contains('~') || depth.contains('^') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("ambiguous argument '{commitish}'"),
            ));
        }
        let generations = if depth.is_empty() {
            1
        } else {
            depth.parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid parent shorthand: {commitish}"),
                )
            })?
        };
        let mut id = resolve_commitish_io_cached(repo, store, commit_cache, base)?;
        for _ in 0..generations {
            let object = store.read_object(&id)?;
            if object.kind != GitObjectKind::Commit {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("revision `{base}` is not a commit"),
                ));
            }
            let commit = commit_cache.read_loaded_commit(object)?;
            let Some(parent) = commit.parents.first() else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("revision '{commitish}' has no parent"),
                ));
            };
            id = parent.clone();
        }
        return Ok(id);
    }
    let id = resolve_objectish(repo, commitish)?;
    if let Some(commit_id) = parse_commit_full_object_id_if_commit(store, commitish, id.clone())? {
        return Ok(commit_id);
    }
    if object_kind_hint_or_read(store, &id)? == GitObjectKind::Tag {
        return peel_tag_chain_to_commit(store, &id)
            .map_err(|error| io::Error::new(error.kind(), format!("{commitish}: {error}")))?
            .commit
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("revision `{commitish}` is not a commit"),
                )
            });
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("revision `{commitish}` is not a commit"),
    ))
}

fn object_kind_hint_or_read(store: &LooseObjectStore, id: &ObjectId) -> io::Result<GitObjectKind> {
    match store.object_kind_hint(id)? {
        Some(kind) => Ok(kind),
        None => store.read_object(id).map(|object| object.kind),
    }
}
pub(crate) fn collect_reflog_roots(repo: &GitRepo, roots: &mut Vec<ObjectId>) -> Result<()> {
    let logs_dir = repo.git_dir.join("logs");
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    collect_reflog_roots_from_path(&logs_dir, roots, algorithm)
}

pub(crate) fn collect_reflog_roots_from_path(
    path: &std::path::Path,
    roots: &mut Vec<ObjectId>,
    algorithm: GitHashAlgorithm,
) -> Result<()> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_reflog_roots_from_path(&entry?.path(), roots, algorithm)?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        let file = fs::File::open(path)?;
        let mut reader = io::BufReader::new(file);
        let mut line = String::new();
        while reader.read_line(&mut line)? != 0 {
            collect_reflog_line_roots(&line, roots, algorithm);
            line.clear();
        }
    }
    Ok(())
}

pub(crate) fn collect_reflog_line_roots(
    line: &str,
    roots: &mut Vec<ObjectId>,
    algorithm: GitHashAlgorithm,
) {
    let mut fields = line.split_whitespace();
    let Some(old) = fields.next() else {
        return;
    };
    let Some(new) = fields.next() else {
        return;
    };
    for id in [old, new] {
        if id.bytes().all(|byte| byte == b'0') {
            continue;
        }
        if let Ok(id) = ObjectId::from_hex(algorithm, id) {
            roots.push(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BinaryHeap, HashSet};

    use tempfile::TempDir;
    use zmin_git_core::{CommitBuilder, GitObjectSink, Signature, TreeEntry, encode_tree};

    use super::*;

    #[test]
    fn rev_list_seen_capacity_hint_accounts_for_known_inputs() {
        assert_eq!(rev_list_seen_capacity_hint(3, 2, 1), 6);
        assert_eq!(rev_list_seen_capacity_hint(0, 0, 0), 1);
        assert_eq!(
            rev_list_seen_capacity_hint(usize::MAX, 1, 1),
            REV_LIST_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn commit_output_capacity_hint_prefers_explicit_limit() {
        assert_eq!(commit_output_capacity_hint(Some(4), 128), 4);
        assert_eq!(commit_output_capacity_hint(None, 3), 3);
        assert_eq!(commit_output_capacity_hint(Some(0), 128), 1);
        assert_eq!(commit_output_capacity_hint(None, 0), 1);
        assert_eq!(
            commit_output_capacity_hint(Some(usize::MAX), 128),
            REV_LIST_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(
            commit_output_capacity_hint(None, usize::MAX),
            REV_LIST_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn follow_state_budget_deduplicates_and_releases_live_states() {
        let first = FollowStateKey {
            candidate_index: 1,
            path_id: FollowPathId(2),
        };
        let second = FollowStateKey {
            candidate_index: 2,
            path_id: FollowPathId(3),
        };
        let third = FollowStateKey {
            candidate_index: 3,
            path_id: FollowPathId(4),
        };
        let mut budget = FollowStateBudget::new(2);
        assert!(budget.schedule(first).expect("first state admitted"));
        assert!(!budget.schedule(first).expect("duplicate state admitted"));
        assert!(budget.schedule(second).expect("second state admitted"));
        assert_eq!(budget.len(), 2);
        let error = budget
            .schedule(third)
            .expect_err("live state limit must fail");
        match error {
            CliError::Fatal { code, message } => {
                assert_eq!(code, 128);
                assert_eq!(
                    message,
                    "--follow exceeded the bounded live branch state limit (2)"
                );
            }
            other => panic!("unexpected follow budget error: {other:?}"),
        }
        budget.complete(first);
        assert_eq!(budget.len(), 1);
        assert!(budget.schedule(third).expect("released state admitted"));
        assert_eq!(budget.len(), 2);
    }

    #[test]
    fn follow_path_interner_releases_dead_transition_paths() {
        let mut paths = FollowPathInterner::new(b"new-file.txt").expect("initial path");
        let initial = FollowPathId(0);
        let mut pending = BinaryHeap::new();
        let mut budget = FollowStateBudget::new(1);
        assert!(
            schedule_follow_branch(
                &mut pending,
                &mut budget,
                &mut paths,
                7,
                1,
                FollowPathCandidate::Owned(FollowPathValue {
                    path: b"old-file.txt".to_vec(),
                    identity: None,
                }),
                0,
                FollowDirection::Forward,
            )
            .expect("transition state admitted")
        );
        let branch = pending.pop().expect("transition branch");
        assert!(paths.find(b"old-file.txt").is_some());
        assert_eq!(
            paths.live_path_bytes,
            b"new-file.txt".len() + b"old-file.txt".len()
        );
        budget.complete(FollowStateKey {
            candidate_index: branch.candidate_index,
            path_id: branch.path_id,
        });
        paths
            .release(branch.path_id)
            .expect("release transition path");
        assert!(paths.find(b"old-file.txt").is_none());
        assert_eq!(paths.live_path_bytes, b"new-file.txt".len());
        paths.retain(initial).expect("retain initial path");
        paths.release(initial).expect("release initial path");
        assert_eq!(paths.live_path_bytes, 0);
    }

    #[test]
    fn follow_path_interner_enforces_injected_live_byte_limit() {
        let mut paths = FollowPathInterner::new_with_limit_for_test(b"root", 4)
            .expect("initial path fits byte limit");
        let error = paths
            .intern(FollowPathValue {
                path: b"next".to_vec(),
                identity: None,
            })
            .expect_err("second live path must exceed byte limit");
        match error {
            CliError::Fatal { code, message } => {
                assert_eq!(code, 128);
                assert_eq!(
                    message,
                    "--follow exceeded the bounded live path byte limit (4)"
                );
            }
            other => panic!("unexpected follow path budget error: {other:?}"),
        }
    }

    #[test]
    fn commit_parent_traversal_reserve_is_bounded() {
        let mut pending = BinaryHeap::<usize>::new();
        let mut scheduled = HashSet::<ObjectId>::new();

        reserve_commit_parent_traversal(&mut pending, &mut scheduled, usize::MAX);

        assert!(pending.capacity() >= REV_LIST_INITIAL_CAPACITY_LIMIT);
        assert!(scheduled.capacity() >= REV_LIST_INITIAL_CAPACITY_LIMIT);
    }

    #[test]
    fn commit_parent_traversal_reserve_does_not_grow_with_enough_spare_capacity() {
        let mut pending = BinaryHeap::<usize>::with_capacity(4);
        pending.push(1);
        let pending_capacity = pending.capacity();
        let mut scheduled = HashSet::<ObjectId>::with_capacity(4);
        scheduled.insert(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );
        let scheduled_capacity = scheduled.capacity();

        reserve_commit_parent_traversal(&mut pending, &mut scheduled, 2);

        assert_eq!(pending.capacity(), pending_capacity);
        assert_eq!(scheduled.capacity(), scheduled_capacity);
    }

    #[test]
    fn commit_depth_parent_traversal_reserve_is_bounded() {
        let mut pending = VecDeque::<ObjectId>::new();
        let mut depths = HashMap::<ObjectId, usize>::new();

        reserve_commit_depth_parent_traversal(&mut pending, &mut depths, usize::MAX);

        assert!(pending.capacity() >= REV_LIST_INITIAL_CAPACITY_LIMIT);
        assert!(depths.capacity() >= REV_LIST_INITIAL_CAPACITY_LIMIT);
    }

    #[test]
    fn commit_depth_parent_traversal_reserve_does_not_grow_with_enough_spare_capacity() {
        let mut pending = VecDeque::<ObjectId>::with_capacity(4);
        pending.push_back(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );
        let pending_capacity = pending.capacity();
        let mut depths = HashMap::<ObjectId, usize>::with_capacity(4);
        depths.insert(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "2222222222222222222222222222222222222222",
            )
            .expect("object id"),
            0,
        );
        let depths_capacity = depths.capacity();

        reserve_commit_depth_parent_traversal(&mut pending, &mut depths, 2);

        assert_eq!(pending.capacity(), pending_capacity);
        assert_eq!(depths.capacity(), depths_capacity);
    }

    #[test]
    fn root_traversal_capacity_hint_is_bounded() {
        assert_eq!(root_traversal_capacity_hint(0), 0);
        assert_eq!(root_traversal_capacity_hint(3), 3);
        assert_eq!(
            root_traversal_capacity_hint(usize::MAX),
            REV_LIST_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn rev_list_root_collector_deduplicates_and_preserves_source_order() {
        let first = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "1111111111111111111111111111111111111111",
        )
        .expect("first object id");
        let second = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "2222222222222222222222222222222222222222",
        )
        .expect("second object id");
        let mut roots = RevListRootCollector::default();
        roots.push_direct("refs/heads/main".to_owned());
        roots.push_peeled_tag(first.clone());
        roots.push_peeled_tag(second.clone());
        roots.push_peeled_tag(first);

        let mut parsed = RevListRevs::default();
        roots.finish(&mut parsed);

        assert_eq!(
            parsed.include,
            vec![
                "refs/heads/main".to_owned(),
                "1111111111111111111111111111111111111111".to_owned(),
                second.to_hex(),
            ]
        );
    }

    #[test]
    fn negative_annotated_tag_root_adds_peeled_commit_to_excludes() {
        let dir = TempDir::new().expect("temporary object store");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let commit = store
            .write_object(
                GitObjectKind::Commit,
                b"tree 1111111111111111111111111111111111111111\n\ncommit\n",
            )
            .expect("write commit object");
        let tag_content = format!(
            "object {}\ntype commit\ntag excluded\ntagger Bench <bench@example.test> 1 +0000\n\nexcluded\n",
            commit.to_hex()
        );
        let tag = store
            .write_object(GitObjectKind::Tag, tag_content.as_bytes())
            .expect("write tag object");
        let mut parsed = RevListRevs::default();
        let mut roots = RevListRootCollector::default();

        append_rev_list_ref_selection(
            &store,
            &mut parsed,
            &mut roots,
            "refs/tags/excluded",
            &tag,
            true,
            "refs/tags/",
        )
        .expect("collect negative tag root");

        assert_eq!(parsed.exclude, vec![commit.to_hex()]);
        assert!(parsed.extra_objects.is_empty());
    }

    #[test]
    fn rev_list_excluded_seen_capacity_hint_is_bounded() {
        assert_eq!(rev_list_excluded_seen_capacity_hint(0), 0);
        assert_eq!(rev_list_excluded_seen_capacity_hint(3), 3);
        assert_eq!(
            rev_list_excluded_seen_capacity_hint(usize::MAX),
            REV_LIST_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn rev_list_seen_reserve_does_not_grow_when_spare_capacity_is_enough() {
        let mut seen = HashSet::with_capacity(8);
        seen.insert(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );
        let capacity = seen.capacity();

        reserve_rev_list_seen_spare(&mut seen, 2, 2, 1);

        assert_eq!(seen.capacity(), capacity);
    }

    #[test]
    fn rev_list_seen_reserve_grows_when_spare_capacity_is_insufficient() {
        let mut seen = HashSet::with_capacity(1);
        seen.insert(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );

        reserve_rev_list_seen_spare(&mut seen, 2, 2, 1);

        assert!(seen.capacity().saturating_sub(seen.len()) >= 5);
    }

    #[test]
    fn tree_traversal_capacity_hints_are_nonzero_and_bounded() {
        assert_eq!(tree_walk_stack_capacity_hint(), 8);
        assert_eq!(tree_cache_capacity_hint(0, 0), 1);
        assert_eq!(tree_cache_capacity_hint(2, 3), 5);
        assert_eq!(
            tree_cache_capacity_hint(usize::MAX, 1),
            REV_LIST_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn ancestor_parent_scheduler_deduplicates_before_stack_push() {
        let parent = ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "1111111111111111111111111111111111111111",
        )
        .expect("object id");
        let mut stack = Vec::new();
        let mut scheduled = HashSet::new();

        schedule_ancestor_parent(&mut stack, &mut scheduled, &parent);
        schedule_ancestor_parent(&mut stack, &mut scheduled, &parent);

        assert_eq!(stack, vec![parent]);
        assert_eq!(scheduled.len(), 1);
    }

    #[test]
    fn ancestor_parent_traversal_reserve_does_not_grow_with_enough_spare_capacity() {
        let mut stack = Vec::<ObjectId>::with_capacity(4);
        stack.push(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );
        let stack_capacity = stack.capacity();
        let mut scheduled = HashSet::<ObjectId>::with_capacity(4);
        scheduled.insert(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "2222222222222222222222222222222222222222",
            )
            .expect("object id"),
        );
        let scheduled_capacity = scheduled.capacity();

        reserve_ancestor_parent_traversal(&mut stack, &mut scheduled, 2);

        assert_eq!(stack.capacity(), stack_capacity);
        assert_eq!(scheduled.capacity(), scheduled_capacity);
    }

    #[test]
    fn reflog_roots_are_collected_from_streamed_lines() {
        let dir = TempDir::new().expect("temp dir");
        let log_path = dir.path().join("HEAD");
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let width = algorithm.digest_len() * 2;
            let old_id = "1".repeat(width);
            let new_id = "2".repeat(width);
            let zero_id = "0".repeat(width);
            fs::write(
                &log_path,
                format!(
                    "{zero_id} {old_id} user <u@example.com> 1 +0000\tinit\n\
                     {old_id} {new_id} user <u@example.com> 2 +0000\tcommit\n\
                     invalid line\n"
                ),
            )
            .expect("write reflog");

            let mut roots = Vec::new();
            collect_reflog_roots_from_path(&log_path, &mut roots, algorithm)
                .expect("collect reflog roots");

            assert_eq!(
                roots.iter().map(ObjectId::to_hex).collect::<Vec<_>>(),
                vec![old_id.clone(), old_id, new_id]
            );
        }
    }

    #[test]
    fn reflog_line_roots_decode_the_active_algorithm() {
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let width = algorithm.digest_len() * 2;
            let new_id = "b".repeat(width);
            let line = format!(
                "{} {new_id} user <u@example.com> 1 +0000\tcommit",
                "0".repeat(width)
            );
            let mut roots = Vec::new();
            collect_reflog_line_roots(&line, &mut roots, algorithm);
            assert_eq!(
                roots,
                vec![ObjectId::from_hex(algorithm, &new_id).expect("new id")]
            );
        }
    }

    #[test]
    fn rev_list_all_uses_loose_ref_over_stale_packed_ref() {
        let dir = TempDir::new().expect("temp dir");
        let git_dir = dir.path().join(".git");
        let objects_dir = git_dir.join("objects");
        fs::create_dir_all(&objects_dir).expect("objects dir");
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let stale = write_test_commit(&store, &tree, &[], 1, "stale");
        let live = write_test_commit(&store, &tree, &[], 2, "live");
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/main\n", stale.to_hex()),
        )
        .expect("write packed refs");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/main", &live)
            .expect("write loose ref");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir,
            objects_dir,
            index_path: dir.path().join(".git/index"),
        };

        let revs = collect_rev_list_revs(&repo, &store, true, Vec::new()).expect("rev-list revs");

        assert_eq!(revs.include, vec!["refs/heads/main".to_owned()]);
        assert!(revs.exclude.is_empty());
        assert!(revs.extra_objects.is_empty());
        assert_eq!(refs.resolve("refs/heads/main").expect("resolve main"), live);
    }

    #[test]
    fn commit_object_collection_applies_exclusions_during_traversal() {
        let dir = TempDir::new().expect("temp dir");
        let git_dir = dir.path().join(".git");
        let objects_dir = git_dir.join("objects");
        fs::create_dir_all(&objects_dir).expect("objects dir");
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let child = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "child");
        let excluded_side = write_test_commit(&store, &tree, &[], 3, "excluded side");
        let refs = RevListRevs {
            include: vec![child.to_hex(), excluded_side.to_hex()],
            exclude: vec![excluded_side.to_hex()],
            extra_objects: Vec::new(),
            symmetric_diff: None,
            exclude_first_parent_only: false,
        };
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir,
            objects_dir,
            index_path: dir.path().join(".git/index"),
        };
        let commit_cache = CommitObjectCache::new(&store);

        let commits = collect_commit_objects_with_exclusions_cached(
            &repo,
            &store,
            &commit_cache,
            &refs,
            None,
        )
        .expect("collect commit objects");

        assert_eq!(
            commits
                .iter()
                .map(|commit| commit.id.clone())
                .collect::<Vec<_>>(),
            vec![child, root]
        );
        assert_eq!(commits[0].commit.message, b"child\n");
        assert_eq!(commits[1].commit.message, b"root\n");
    }

    #[test]
    fn common_merge_base_candidate_capacity_uses_smaller_side_and_is_bounded() {
        assert_eq!(common_merge_base_candidate_capacity(2, 8), 2);
        assert_eq!(common_merge_base_candidate_capacity(8, 2), 2);
        assert_eq!(
            common_merge_base_candidate_capacity(usize::MAX, usize::MAX),
            REV_LIST_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn tree_ref_cache_bounds_retained_entries() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
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
        let mut cache = TreeObjectRefCache::with_entry_limit(&store, 1, 1);

        cache.read_tree(&first_tree).expect("read first tree");
        cache.read_tree(&second_tree).expect("read second tree");

        assert_eq!(cache.trees.len(), 1);
        assert!(cache.trees.contains_key(&second_tree));
    }

    #[test]
    fn tree_walk_child_reserve_uses_known_entry_count() {
        let mut pending = Vec::<ObjectId>::with_capacity(tree_walk_stack_capacity_hint());

        reserve_tree_walk_children(&mut pending, 32);

        assert!(pending.capacity().saturating_sub(pending.len()) >= 32);
    }

    #[test]
    fn tree_walk_child_reserve_is_bounded_for_large_trees() {
        let mut pending = Vec::<ObjectId>::with_capacity(tree_walk_stack_capacity_hint());

        reserve_tree_walk_children(&mut pending, usize::MAX);

        assert!(
            pending.capacity() <= REV_LIST_INITIAL_CAPACITY_LIMIT + tree_walk_stack_capacity_hint()
        );
    }

    #[test]
    fn tree_walk_child_reserve_does_not_grow_when_spare_capacity_is_enough() {
        let mut pending = Vec::<ObjectId>::with_capacity(4);
        pending.push(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );
        let capacity = pending.capacity();

        reserve_tree_walk_children(&mut pending, 2);

        assert_eq!(pending.capacity(), capacity);
    }

    #[test]
    fn tree_walk_child_reserve_grows_when_spare_capacity_is_insufficient() {
        let mut pending = Vec::<ObjectId>::with_capacity(1);
        pending.push(
            ObjectId::from_hex(
                GitHashAlgorithm::Sha1,
                "1111111111111111111111111111111111111111",
            )
            .expect("object id"),
        );

        reserve_tree_walk_children(&mut pending, 5);

        assert!(pending.capacity().saturating_sub(pending.len()) >= 5);
    }

    #[test]
    fn tree_ref_collection_and_count_use_iterative_traversal() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"deep blob\n")
            .expect("write blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "file.txt", blob.clone()).expect("tree entry")
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "a", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "b", leaf_tree.clone()).expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let mut tree_cache =
            TreeObjectRefCache::with_capacity(&store, tree_cache_capacity_hint(1, 0));
        let mut seen = HashSet::new();

        collect_rev_list_tree_object_ref_ids(
            &mut tree_cache,
            &root_tree,
            &mut seen,
            RevListTreeMissingPolicy::default(),
        )
        .expect("collect tree refs");

        assert!(seen.contains(&root_tree));
        assert!(seen.contains(&leaf_tree));
        assert!(seen.contains(&blob));
        assert_eq!(seen.len(), 3);

        let mut counted = HashSet::new();
        let count = count_rev_list_tree_ref_objects(
            &mut tree_cache,
            &root_tree,
            &mut counted,
            RevListTreeMissingPolicy::default(),
        )
        .expect("count tree refs");
        assert_eq!(count, 3);
        assert_eq!(counted, seen);
    }

    #[test]
    fn tree_id_collection_uses_iterative_traversal_for_duplicate_trees() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"shared blob\n")
            .expect("write blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "file.txt", blob.clone()).expect("tree entry")
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "first", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "second", leaf_tree.clone())
                        .expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let mut tree_cache =
            TreeObjectRefCache::with_capacity(&store, tree_cache_capacity_hint(1, 0));
        let mut seen = HashSet::new();

        collect_rev_list_tree_object_ids(&mut tree_cache, &root_tree, &mut seen)
            .expect("collect tree ids");

        assert!(seen.contains(&root_tree));
        assert!(seen.contains(&leaf_tree));
        assert!(seen.contains(&blob));
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn tree_object_count_skips_duplicate_subtrees() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"shared blob\n")
            .expect("write blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "file.txt", blob.clone()).expect("tree entry")
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "first", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "second", leaf_tree.clone())
                        .expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let mut tree_cache =
            TreeObjectRefCache::with_capacity(&store, tree_cache_capacity_hint(1, 0));
        let mut seen = HashSet::new();

        let count = count_rev_list_tree_ref_objects(
            &mut tree_cache,
            &root_tree,
            &mut seen,
            RevListTreeMissingPolicy::default(),
        )
        .expect("count tree refs");

        assert_eq!(count, 3);
        assert_eq!(seen, HashSet::from([root_tree, leaf_tree, blob]));
    }

    #[test]
    fn object_id_visitor_matches_writer_for_duplicate_trees_and_extra_objects() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"shared blob\n")
            .expect("write blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "file.txt", blob.clone()).expect("tree entry")
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "first", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "second", leaf_tree.clone())
                        .expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let commit = CollectedCommitTree {
            id: root_tree.clone(),
            tree: root_tree.clone(),
            is_boundary: false,
        };
        let extra_objects = vec![leaf_tree.clone(), blob.clone()];
        let mut written = Vec::new();
        write_rev_list_object_ids_uncached(
            &store,
            std::slice::from_ref(&commit),
            &extra_objects,
            &[],
            RevListTreeMissingPolicy::default(),
            &mut written,
        )
        .expect("write object ids");
        let writer_ids = written
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                ObjectId::from_hex(
                    GitHashAlgorithm::Sha1,
                    std::str::from_utf8(line).expect("writer id UTF-8"),
                )
                .expect("writer id")
            })
            .collect::<Vec<_>>();
        let mut visited = Vec::new();
        for_each_rev_list_object_id_with_trees(
            &store,
            std::slice::from_ref(&commit),
            &extra_objects,
            &[],
            RevListTreeMissingPolicy::default(),
            |id, _| {
                visited.push(id.clone());
                Ok(())
            },
        )
        .expect("visit object ids");

        assert_eq!(visited, writer_ids);
    }

    #[test]
    fn tree_object_path_collection_preserves_preorder_without_recursion() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let first_blob = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second_blob = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "first.txt", first_blob.clone())
                        .expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "second.txt", second_blob.clone())
                        .expect("tree entry"),
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "a", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "b", leaf_tree.clone()).expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let tree_cache = TreeObjectCache::new(&store);
        let mut path = Vec::new();
        let mut seen = HashSet::new();
        let mut paths = Vec::new();

        collect_rev_list_tree_object_paths(
            &tree_cache,
            &root_tree,
            &mut path,
            &mut seen,
            &mut |_, _, object_path| {
                paths.push(object_path.map(|path| String::from_utf8_lossy(path).into_owned()));
                Ok(())
            },
        )
        .expect("collect object paths");

        assert_eq!(
            paths,
            vec![
                None,
                Some("a".to_owned()),
                Some("a/first.txt".to_owned()),
                Some("a/second.txt".to_owned()),
            ]
        );
        assert_eq!(path, b"");
    }

    #[test]
    fn tree_object_line_collection_preserves_preorder_without_recursion() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let first_blob = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second_blob = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "first.txt", first_blob.clone())
                        .expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "second.txt", second_blob.clone())
                        .expect("tree entry"),
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "a", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "b", leaf_tree.clone()).expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let mut path = Vec::new();
        let mut seen = HashSet::new();
        let mut lines = Vec::new();

        let mut count = 0usize;
        for_each_rev_list_tree_object_line(
            &store,
            &root_tree,
            &mut path,
            &mut seen,
            RevListTreeMissingPolicy::default(),
            &mut |id, _, path| {
                lines.push(format!(
                    "{} {}",
                    id.to_hex(),
                    String::from_utf8_lossy(path.unwrap_or_default())
                ));
                Ok(())
            },
            &mut count,
        )
        .expect("collect object lines");

        assert_eq!(
            lines,
            vec![
                format!("{} ", root_tree.to_hex()),
                format!("{} a", leaf_tree.to_hex()),
                format!("{} a/first.txt", first_blob.to_hex()),
                format!("{} a/second.txt", second_blob.to_hex()),
            ]
        );
        assert_eq!(count, lines.len());
        assert_eq!(path, b"");
    }

    #[test]
    fn rev_list_object_line_reuses_and_resets_path_buffer_across_commits() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let first_blob = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second_blob = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let first_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(
                    &[TreeEntry::new(TreeMode::File, "a.txt", first_blob.clone())
                        .expect("tree entry")],
                )
                .expect("encode first tree"),
            )
            .expect("write first tree");
        let second_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(
                    &[TreeEntry::new(TreeMode::File, "b.txt", second_blob.clone())
                        .expect("tree entry")],
                )
                .expect("encode second tree"),
            )
            .expect("write second tree");
        let first_commit = write_test_commit(&store, &first_tree, &[], 1, "first");
        let second_commit = write_test_commit(&store, &second_tree, &[], 2, "second");
        let mut paths = Vec::new();

        for_each_rev_list_object_line_with(
            &store,
            &[first_commit, second_commit],
            &[],
            &[],
            RevListTreeMissingPolicy::default(),
            |_, _, path| {
                paths.push(path.map(|path| String::from_utf8_lossy(path).into_owned()));
                Ok(())
            },
        )
        .expect("collect object lines");

        assert_eq!(
            paths,
            vec![
                Some(String::new()),
                Some("a.txt".to_owned()),
                Some(String::new()),
                Some("b.txt".to_owned()),
            ]
        );
    }

    #[test]
    fn rev_list_object_line_with_trees_reuses_commit_tree_ids() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let first_blob = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second_blob = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let first_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(
                    &[TreeEntry::new(TreeMode::File, "a.txt", first_blob.clone())
                        .expect("tree entry")],
                )
                .expect("encode first tree"),
            )
            .expect("write first tree");
        let second_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(
                    &[TreeEntry::new(TreeMode::File, "b.txt", second_blob.clone())
                        .expect("tree entry")],
                )
                .expect("encode second tree"),
            )
            .expect("write second tree");
        let first_commit = write_test_commit(&store, &first_tree, &[], 1, "first");
        let second_commit = write_test_commit(&store, &second_tree, &[], 2, "second");
        let commits = vec![
            CollectedCommitTree {
                id: first_commit,
                tree: first_tree,
                is_boundary: false,
            },
            CollectedCommitTree {
                id: second_commit,
                tree: second_tree,
                is_boundary: false,
            },
        ];
        let mut paths = Vec::new();

        for_each_rev_list_object_line_with_trees(
            &store,
            &commits,
            &[],
            &[],
            RevListTreeMissingPolicy::default(),
            |_, _, path| {
                paths.push(path.map(|path| String::from_utf8_lossy(path).into_owned()));
                Ok(())
            },
        )
        .expect("collect object lines from commit trees");

        assert_eq!(
            paths,
            vec![
                Some(String::new()),
                Some("a.txt".to_owned()),
                Some(String::new()),
                Some("b.txt".to_owned()),
            ]
        );
    }

    #[test]
    fn ordered_tree_id_iteration_preserves_preorder_with_duplicate_trees() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let first_blob = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second_blob = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "first.txt", first_blob.clone())
                        .expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "second.txt", second_blob.clone())
                        .expect("tree entry"),
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "a", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "b", leaf_tree.clone()).expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let mut tree_cache =
            TreeObjectRefCache::with_capacity(&store, tree_cache_capacity_hint(1, 0));
        let mut seen = HashSet::new();
        let mut visited = Vec::new();

        for_each_rev_list_tree_object_id_ordered_into(
            &mut tree_cache,
            &root_tree,
            &mut seen,
            &mut |id| {
                visited.push(id.clone());
                Ok(())
            },
        )
        .expect("iterate tree ids");

        assert_eq!(visited, vec![root_tree, leaf_tree, first_blob, second_blob]);
    }

    #[test]
    fn ordered_tree_id_writer_preserves_preorder_without_recursion() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let first_blob = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first blob");
        let second_blob = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second blob");
        let leaf_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "first.txt", first_blob.clone())
                        .expect("tree entry"),
                    TreeEntry::new(TreeMode::File, "second.txt", second_blob.clone())
                        .expect("tree entry"),
                ])
                .expect("encode leaf tree"),
            )
            .expect("write leaf tree");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::Tree, "a", leaf_tree.clone()).expect("tree entry"),
                    TreeEntry::new(TreeMode::Tree, "b", leaf_tree.clone()).expect("tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let mut tree_cache =
            TreeObjectRefCache::with_capacity(&store, tree_cache_capacity_hint(1, 0));
        let mut seen = HashSet::new();
        let mut out = Vec::new();

        write_rev_list_tree_object_ref_ids_ordered(
            &mut tree_cache,
            &root_tree,
            &mut seen,
            RevListTreeMissingPolicy::default(),
            &mut out,
        )
        .expect("write ordered tree ids");

        let lines = std::str::from_utf8(&out)
            .expect("utf8 output")
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(
            lines,
            vec![
                root_tree.to_hex(),
                leaf_tree.to_hex(),
                first_blob.to_hex(),
                second_blob.to_hex(),
            ]
        );
    }

    #[test]
    fn ordered_tree_id_iteration_uses_seen_for_excluded_objects() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path(), GitHashAlgorithm::Sha1);
        let excluded_blob = store
            .write_object(GitObjectKind::Blob, b"excluded\n")
            .expect("write excluded blob");
        let included_blob = store
            .write_object(GitObjectKind::Blob, b"included\n")
            .expect("write included blob");
        let root_tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[
                    TreeEntry::new(TreeMode::File, "excluded.txt", excluded_blob.clone())
                        .expect("excluded tree entry"),
                    TreeEntry::new(TreeMode::File, "included.txt", included_blob.clone())
                        .expect("included tree entry"),
                ])
                .expect("encode root tree"),
            )
            .expect("write root tree");
        let mut tree_cache =
            TreeObjectRefCache::with_capacity(&store, tree_cache_capacity_hint(1, 0));
        let mut seen = HashSet::from([excluded_blob.clone()]);
        let mut visited = Vec::new();

        for_each_rev_list_tree_object_id_ordered_into(
            &mut tree_cache,
            &root_tree,
            &mut seen,
            &mut |id| {
                visited.push(id.clone());
                Ok(())
            },
        )
        .expect("iterate tree ids with excluded object");

        assert_eq!(visited, vec![root_tree, included_blob]);
        assert!(seen.contains(&excluded_blob));
    }

    #[test]
    fn commit_collection_schedules_duplicate_roots_once() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let signature = Signature::new("A", "a@example.test", 1, "+0000").expect("signature");
        let commit = store
            .write_object(
                GitObjectKind::Commit,
                &CommitBuilder::new(tree, signature.clone(), signature)
                    .message("root\n")
                    .expect("commit message")
                    .encode()
                    .expect("encode commit"),
            )
            .expect("write commit");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };
        let commit_cache = CommitObjectCache::new(&store);

        let commits = collect_commits_from_ids_cached(
            &repo,
            &commit_cache,
            &[commit.clone(), commit.clone()],
            None,
        )
        .expect("collect commits");

        assert_eq!(commits, vec![commit]);
    }

    #[test]
    fn empty_exclude_collection_returns_without_traversal() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };
        let revs = RevListRevs {
            include: Vec::new(),
            exclude: Vec::new(),
            extra_objects: Vec::new(),
            symmetric_diff: None,
            exclude_first_parent_only: false,
        };
        let commit_cache = CommitObjectCache::new(&store);

        let excluded = collect_excluded_commits_cached(&repo, &store, &commit_cache, &revs.exclude)
            .expect("empty cached excludes");
        let rev_list_excluded = collect_rev_list_excluded_commits(&repo, &store, &revs)
            .expect("empty rev-list excludes");
        let rev_list_excluded_uncached =
            collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)
                .expect("empty uncached rev-list excludes");

        assert!(excluded.is_empty());
        assert!(rev_list_excluded.is_empty());
        assert!(rev_list_excluded_uncached.is_empty());
    }

    #[test]
    fn commit_collection_applies_exclusions_during_traversal_before_max_count() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let excluded_parent = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "b");
        let middle = write_test_commit(
            &store,
            &tree,
            std::slice::from_ref(&excluded_parent),
            3,
            "c",
        );
        let head = write_test_commit(&store, &tree, std::slice::from_ref(&middle), 4, "head");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };
        let commit_cache = CommitObjectCache::new(&store);
        let excluded = HashSet::from([excluded_parent, root]);

        let commits = collect_commits_from_ids_cached_with_excluded(
            &repo,
            &commit_cache,
            std::slice::from_ref(&head),
            Some(2),
            &excluded,
        )
        .expect("collect with excluded");

        assert_eq!(commits, vec![head, middle]);
    }

    #[test]
    fn id_exclusion_collection_avoids_revision_string_roundtrip() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let middle = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "middle");
        let head = write_test_commit(&store, &tree, std::slice::from_ref(&middle), 3, "head");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };
        let commit_cache = CommitObjectCache::new(&store);

        let commits = collect_commits_from_ids_with_id_exclusions_cached(
            &repo,
            &store,
            &commit_cache,
            std::slice::from_ref(&head),
            std::slice::from_ref(&middle),
            &[],
            None,
        )
        .expect("collect with id excludes");

        assert_eq!(commits, vec![head]);
    }

    #[test]
    fn id_exclusion_set_collects_history_without_output_vector() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let middle = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "middle");
        let head = write_test_commit(&store, &tree, std::slice::from_ref(&middle), 3, "head");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };
        let commit_cache = CommitObjectCache::new(&store);

        let excluded = collect_excluded_commits_from_ids_cached(
            &repo,
            &commit_cache,
            std::slice::from_ref(&head),
        )
        .expect("excluded set");

        assert_eq!(excluded.len(), 3);
        assert!(excluded.contains(&head));
        assert!(excluded.contains(&middle));
        assert!(excluded.contains(&root));
    }

    #[test]
    fn uncached_exclusion_set_collects_history_without_output_vector() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let middle = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "middle");
        let head = write_test_commit(&store, &tree, std::slice::from_ref(&middle), 3, "head");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };

        let excluded = collect_excluded_commits_uncached(&repo, &store, &[head.to_hex()])
            .expect("uncached excluded set");

        assert_eq!(excluded.len(), 3);
        assert!(excluded.contains(&head));
        assert!(excluded.contains(&middle));
        assert!(excluded.contains(&root));
    }

    #[test]
    fn rev_list_excluded_commits_from_ids_deduplicates_string_excludes() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let middle = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "middle");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };

        let commit_cache = CommitObjectCache::new(&store);

        let excluded = collect_rev_list_excluded_commits_from_ids_cached(
            &repo,
            &store,
            &commit_cache,
            std::slice::from_ref(&middle),
            &[root.to_hex()],
        )
        .expect("collect mixed excludes with cache");
        let wrapped = collect_rev_list_excluded_commits_from_ids(
            &repo,
            &store,
            std::slice::from_ref(&middle),
            &[root.to_hex()],
        )
        .expect("collect mixed excludes through wrapper");

        assert_eq!(excluded, vec![middle, root]);
        assert_eq!(wrapped, excluded);
    }

    #[test]
    fn shallow_commits_stream_lines_without_materializing_file() {
        let dir = TempDir::new().expect("temp dir");
        for algorithm in [GitHashAlgorithm::Sha1, GitHashAlgorithm::Sha256] {
            let suffix = match algorithm {
                GitHashAlgorithm::Sha1 => "sha1",
                GitHashAlgorithm::Sha256 => "sha256",
            };
            let root = dir.path().join(suffix);
            fs::create_dir_all(&root).expect("create shallow fixture");
            if algorithm == GitHashAlgorithm::Sha256 {
                fs::write(
                    root.join("config"),
                    "[extensions]\n\tobjectFormat = sha256\n",
                )
                .expect("write SHA-256 config");
            }
            let store = LooseObjectStore::new(root.join("objects"), algorithm);
            let tree = store
                .write_object(
                    GitObjectKind::Tree,
                    &encode_tree(&[]).expect("encode empty tree"),
                )
                .expect("write tree");
            let root_commit = write_test_commit(&store, &tree, &[], 1, "root");
            let head =
                write_test_commit(&store, &tree, std::slice::from_ref(&root_commit), 2, "head");
            fs::write(
                root.join("shallow"),
                format!("\n{}\n{}\n", root_commit.to_hex(), head.to_hex()),
            )
            .expect("write shallow");
            let repo = GitRepo {
                root: root.clone(),
                git_dir: root.clone(),
                objects_dir: root.join("objects"),
                index_path: root.join("index"),
            };

            let shallow = read_shallow_commits(&repo).expect("read shallow commits");

            assert_eq!(shallow.len(), 2);
            assert!(shallow.contains(&root_commit));
            assert!(shallow.contains(&head));
        }
    }

    #[test]
    fn linked_worktree_sha256_shallow_boundary_uses_common_git_dir() {
        let dir = TempDir::new().expect("temp dir");
        let common_git_dir = dir.path().join("common.git");
        let worktree_git_dir = common_git_dir.join("worktrees/main");
        fs::create_dir_all(&worktree_git_dir).expect("create linked worktree git dir");
        fs::write(worktree_git_dir.join("commondir"), "../..\n")
            .expect("write linked worktree commondir");
        fs::write(
            common_git_dir.join("config"),
            "[extensions]\n\tobjectFormat = sha256\n",
        )
        .expect("write SHA-256 config");

        let store = LooseObjectStore::new(common_git_dir.join("objects"), GitHashAlgorithm::Sha256);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let head = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "head");
        fs::write(
            common_git_dir.join("shallow"),
            format!("{}\n", head.to_hex()),
        )
        .expect("write common shallow boundary");

        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: worktree_git_dir,
            objects_dir: common_git_dir.join("objects"),
            index_path: dir.path().join("index"),
        };
        let commit_cache = CommitObjectCache::new(&store);

        assert!(is_ancestor_commit_cached(&commit_cache, &root, &head).expect("plain ancestor"));
        assert!(
            !is_ancestor_commit_with_repo_cached(&repo, &commit_cache, &root, &head,)
                .expect("linked-worktree shallow-aware ancestor")
        );
    }

    #[test]
    fn commit_depths_use_nearest_parent_distance_in_merge_graph() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let long_1 = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "long 1");
        let long_2 = write_test_commit(&store, &tree, std::slice::from_ref(&long_1), 3, "long 2");
        let short = write_test_commit(&store, &tree, std::slice::from_ref(&root), 4, "short");
        let merge = write_test_commit(&store, &tree, &[long_2.clone(), short.clone()], 5, "merge");
        let commit_cache = CommitObjectCache::new(&store);

        let depths = commit_depths_cached(&commit_cache, &merge).expect("commit depths");

        assert_eq!(depths.get(&merge), Some(&0));
        assert_eq!(depths.get(&short), Some(&1));
        assert_eq!(depths.get(&long_2), Some(&1));
        assert_eq!(depths.get(&root), Some(&2));
    }

    #[test]
    fn uncached_is_ancestor_returns_true_through_merge_descendant() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let main_1 = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "main 1");
        let branch_1 =
            write_test_commit(&store, &tree, std::slice::from_ref(&main_1), 3, "branch 1");
        let merge_into_branch = write_test_commit(
            &store,
            &tree,
            &[branch_1.clone(), main_1.clone()],
            4,
            "merge main into branch",
        );
        let branch_tip = write_test_commit(
            &store,
            &tree,
            std::slice::from_ref(&merge_into_branch),
            5,
            "branch tip",
        );
        let main_2 = write_test_commit(&store, &tree, std::slice::from_ref(&main_1), 6, "main 2");
        let main_merge = write_test_commit(
            &store,
            &tree,
            &[main_2, branch_tip.clone()],
            7,
            "merge branch into main",
        );

        assert!(
            is_ancestor_commit_uncached(&store, &branch_tip, &main_merge)
                .expect("uncached merge ancestor check")
        );
    }

    #[test]
    fn cached_is_ancestor_processes_stacked_merge_parents() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let main_1 = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "main 1");
        let branch_1 =
            write_test_commit(&store, &tree, std::slice::from_ref(&main_1), 3, "branch 1");
        let merge_into_branch = write_test_commit(
            &store,
            &tree,
            &[branch_1, main_1.clone()],
            4,
            "merge main into branch",
        );
        let branch_tip = write_test_commit(
            &store,
            &tree,
            std::slice::from_ref(&merge_into_branch),
            5,
            "branch tip",
        );
        let main_2 = write_test_commit(&store, &tree, std::slice::from_ref(&main_1), 6, "main 2");
        let main_merge = write_test_commit(
            &store,
            &tree,
            &[main_2, branch_tip],
            7,
            "merge branch into main",
        );
        let commit_cache = CommitObjectCache::new(&store);

        assert!(
            is_ancestor_commit_cached(&commit_cache, &merge_into_branch, &main_merge)
                .expect("cached merge ancestor check")
        );
    }

    #[test]
    fn shallow_cached_ancestor_check_stops_at_boundary() {
        let dir = TempDir::new().expect("temp dir");
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let head = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "head");
        fs::write(dir.path().join("shallow"), format!("{}\n", head.to_hex())).expect("shallow");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };
        let commit_cache = CommitObjectCache::new(&store);

        assert!(is_ancestor_commit_cached(&commit_cache, &root, &head).expect("plain ancestor"));
        assert!(
            !is_ancestor_commit_with_repo_cached(&repo, &commit_cache, &root, &head)
                .expect("shallow-aware ancestor")
        );
        assert_eq!(
            best_merge_base_with_repo_cached(&repo, &commit_cache, &root, &head)
                .expect("shallow-aware merge base"),
            None
        );
    }

    fn write_test_commit(
        store: &LooseObjectStore,
        tree: &ObjectId,
        parents: &[ObjectId],
        timestamp: i64,
        message: &str,
    ) -> ObjectId {
        let author = Signature::new("A", "a@example.test", timestamp, "+0000").expect("author");
        let committer =
            Signature::new("C", "c@example.test", timestamp, "+0000").expect("committer");
        let mut builder = CommitBuilder::new(tree.clone(), author, committer);
        for parent in parents {
            builder = builder.parent(parent.clone());
        }
        store
            .write_object(
                GitObjectKind::Commit,
                &builder
                    .message(format!("{message}\n"))
                    .expect("commit message")
                    .encode()
                    .expect("encode commit"),
            )
            .expect("write commit")
    }
}
