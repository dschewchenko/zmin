use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use zmin_git_core::{
    CommitObject, CommitObjectCache, GitHashAlgorithm, GitObjectKind, GitObjectStore, LooseObject,
    LooseObjectStore, ObjectId, PackedObjectOrdinalLookup, TreeMode, TreeObjectCache,
    TreeObjectRef, decode_pack_index_object_ids_from_path, decode_tag, decode_tree,
    decode_tree_entry_ref,
};

use super::{
    CliError, CommitGraphIndex, GitRepo, RefStore, Result, current_branch_ref, phase_trace,
    read_common_config_entries, resolve_objectish, resolve_treeish, short_ref_name,
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
    pub(crate) parents: Vec<ObjectId>,
    pub(crate) author: Vec<u8>,
    pub(crate) committer: Vec<u8>,
}

pub(crate) struct CollectedCommitOneline {
    pub(crate) id: ObjectId,
    pub(crate) parents: Vec<ObjectId>,
    pub(crate) subject: String,
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
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RevListTreeMissingPolicy<'a> {
    pub(crate) allow_any: bool,
    pub(crate) exclude_promisor_objects: bool,
    pub(crate) emit_missing_objects: bool,
    pub(crate) promised_objects: Option<&'a HashSet<ObjectId>>,
}

impl RevListTreeMissingPolicy<'_> {
    fn excludes_promisor_object(self, id: &ObjectId) -> bool {
        self.exclude_promisor_objects
            && self
                .promised_objects
                .is_some_and(|promised_objects| promised_objects.contains(id))
    }

    fn allows_missing_object(self, id: &ObjectId) -> bool {
        self.allow_any
            || self
                .promised_objects
                .is_some_and(|promised_objects| promised_objects.contains(id))
    }

    fn emits_missing_object(self, id: &ObjectId) -> bool {
        self.emit_missing_objects && self.allows_missing_object(id)
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
    collect_rev_list_revs_with_snapshot(repo, store, all, revs, None)
}

pub(crate) fn collect_rev_list_ref_snapshot(repo: &GitRepo) -> Result<RevListRefSnapshot> {
    let head_refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let refs = RefStore::new(repo_common_dir(repo), GitHashAlgorithm::Sha1);
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

pub(crate) fn collect_rev_list_revs_with_snapshot(
    repo: &GitRepo,
    store: &LooseObjectStore,
    all: bool,
    revs: Vec<String>,
    ref_snapshot: Option<&RevListRefSnapshot>,
) -> Result<RevListRevs> {
    let mut parsed = RevListRevs::default();
    let mut not_mode = false;
    let commit_cache = CommitObjectCache::new(store);
    if all {
        collect_rev_list_ref_selection_from_rows(
            store,
            &mut parsed,
            ref_snapshot.map(|snapshot| snapshot.refs.as_slice()),
            "refs/",
            None,
            false,
            repo,
        )?;
    }
    for rev in revs {
        if rev == "--not" {
            not_mode = !not_mode;
        } else if rev == "--branches" || rev == "--heads" {
            collect_rev_list_ref_selection(
                repo,
                store,
                &mut parsed,
                "refs/heads/",
                None,
                not_mode,
                ref_snapshot,
            )?;
        } else if let Some(pattern) = rev
            .strip_prefix("--branches=")
            .or_else(|| rev.strip_prefix("--heads="))
        {
            collect_rev_list_ref_selection(
                repo,
                store,
                &mut parsed,
                "refs/heads/",
                Some(pattern),
                not_mode,
                ref_snapshot,
            )?;
        } else if rev == "--remotes" {
            collect_rev_list_ref_selection(
                repo,
                store,
                &mut parsed,
                "refs/remotes/",
                None,
                not_mode,
                ref_snapshot,
            )?;
        } else if let Some(pattern) = rev.strip_prefix("--remotes=") {
            collect_rev_list_ref_selection(
                repo,
                store,
                &mut parsed,
                "refs/remotes/",
                Some(pattern),
                not_mode,
                ref_snapshot,
            )?;
        } else if rev == "--tags" {
            collect_rev_list_ref_selection(
                repo,
                store,
                &mut parsed,
                "refs/tags/",
                None,
                not_mode,
                ref_snapshot,
            )?;
        } else if let Some(pattern) = rev.strip_prefix("--tags=") {
            collect_rev_list_ref_selection(
                repo,
                store,
                &mut parsed,
                "refs/tags/",
                Some(pattern),
                not_mode,
                ref_snapshot,
            )?;
        } else if let Some(stripped) = rev.strip_prefix('^') {
            if stripped.is_empty() {
                return Err(CliError::Message("empty negative revision".into()));
            }
            if not_mode {
                parsed.include.push(stripped.to_owned());
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
            let bases = merge_bases_all_cached(&commit_cache, &left_id, &right_id)?;
            if not_mode {
                parsed.exclude.push(left.to_owned());
                parsed.exclude.push(right.to_owned());
                parsed
                    .include
                    .extend(bases.into_iter().map(|base| base.to_hex()));
            } else {
                if parsed.symmetric_diff.is_none() {
                    parsed.symmetric_diff = Some(RevListSymmetricDiff {
                        left: left.to_owned(),
                        right: right.to_owned(),
                        merge_bases: bases.clone(),
                    });
                }
                parsed.include.push(left.to_owned());
                parsed.include.push(right.to_owned());
                parsed
                    .exclude
                    .extend(bases.into_iter().map(|base| base.to_hex()));
            }
        } else if let Some((left, right)) = rev.split_once("..") {
            let left = if left.is_empty() { "HEAD" } else { left };
            let right = if right.is_empty() { "HEAD" } else { right };
            if not_mode {
                parsed.include.push(left.to_owned());
                parsed.exclude.push(right.to_owned());
            } else {
                parsed.exclude.push(left.to_owned());
                parsed.include.push(right.to_owned());
            }
        } else if not_mode {
            parsed.exclude.push(rev);
        } else {
            parsed.include.push(rev);
        }
    }
    let mut normalized_include = Vec::with_capacity(parsed.include.len());
    for rev in std::mem::take(&mut parsed.include) {
        match resolve_commitish_io(repo, store, &rev) {
            Ok(_) => normalized_include.push(rev),
            Err(error) => {
                if let Some(cli_error) = map_head_resolution_error(&rev, &error) {
                    return Err(cli_error);
                }
                let id = resolve_objectish(repo, &rev).map_err(|resolve_error| {
                    map_head_resolution_error(&rev, &resolve_error)
                        .unwrap_or_else(|| ambiguous_revision_error(&rev))
                })?;
                match object_kind_hint_or_read(store, &id) {
                    Ok(GitObjectKind::Commit) => normalized_include.push(rev),
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
    if parsed.include.is_empty() && parsed.extra_objects.is_empty() {
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

fn collect_rev_list_ref_selection(
    repo: &GitRepo,
    store: &LooseObjectStore,
    parsed: &mut RevListRevs,
    prefix: &str,
    pattern: Option<&str>,
    not_mode: bool,
    ref_snapshot: Option<&RevListRefSnapshot>,
) -> Result<()> {
    if let Some(snapshot) = ref_snapshot {
        return collect_rev_list_ref_selection_from_rows(
            store,
            parsed,
            Some(snapshot.refs.as_slice()),
            prefix,
            pattern,
            not_mode,
            repo,
        );
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    refs.for_each_resolved_ref(prefix, |ref_name, id| {
        if !rev_list_ref_selection_matches(ref_name, pattern) {
            return Ok(());
        }
        if let Ok(kind) = object_kind_hint_or_read(store, id) {
            if kind == GitObjectKind::Commit {
                if not_mode {
                    parsed.exclude.push(ref_name.to_owned());
                } else {
                    parsed.include.push(ref_name.to_owned());
                }
            } else if !not_mode {
                let name = if kind == GitObjectKind::Tag {
                    short_ref_name(ref_name)
                } else {
                    String::new()
                };
                parsed.extra_objects.push((id.clone(), name));
            }
        }
        Ok::<(), CliError>(())
    })
}

fn collect_rev_list_ref_selection_from_rows(
    store: &LooseObjectStore,
    parsed: &mut RevListRevs,
    rows: Option<&[RevListResolvedRef]>,
    prefix: &str,
    pattern: Option<&str>,
    not_mode: bool,
    repo: &GitRepo,
) -> Result<()> {
    if let Some(rows) = rows {
        for row in rows {
            if !row.ref_name.starts_with(prefix)
                || !rev_list_ref_selection_matches(&row.ref_name, pattern)
            {
                continue;
            }
            if let Ok(kind) = object_kind_hint_or_read(store, &row.id) {
                if kind == GitObjectKind::Commit {
                    if not_mode {
                        parsed.exclude.push(row.ref_name.clone());
                    } else {
                        parsed.include.push(row.ref_name.clone());
                    }
                } else if !not_mode {
                    let name = if kind == GitObjectKind::Tag {
                        short_ref_name(&row.ref_name)
                    } else {
                        String::new()
                    };
                    parsed.extra_objects.push((row.id.clone(), name));
                }
            }
        }
        return Ok(());
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    refs.for_each_resolved_ref(prefix, |ref_name, id| {
        if !rev_list_ref_selection_matches(ref_name, pattern) {
            return Ok(());
        }
        if let Ok(kind) = object_kind_hint_or_read(store, id) {
            if kind == GitObjectKind::Commit {
                if not_mode {
                    parsed.exclude.push(ref_name.to_owned());
                } else {
                    parsed.include.push(ref_name.to_owned());
                }
            } else if !not_mode {
                let name = if kind == GitObjectKind::Tag {
                    short_ref_name(ref_name)
                } else {
                    String::new()
                };
                parsed.extra_objects.push((id.clone(), name));
            }
        }
        Ok::<(), CliError>(())
    })
}

fn rev_list_ref_selection_matches(ref_name: &str, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern else {
        return true;
    };
    wildcard_match(pattern, ref_name)
        || wildcard_match(pattern, &short_ref_name(ref_name))
        || ref_name
            .strip_prefix("refs/remotes/")
            .is_some_and(|short| wildcard_match(pattern, short))
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
        None,
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
    collect_pending_commits_into_set(repo, commit_cache, pending, scheduled, sequence, out)
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
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, commit.parents.len());
        for parent in &commit.parents {
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
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, commit.parents.len());
        for parent in &commit.parents {
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
    Ok(PendingCommitMetadata {
        metadata: CollectedCommitMetadata {
            id,
            parents,
            author,
            committer,
        },
        timestamp,
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
    Ok(CollectedCommitMetadata {
        id: id.clone(),
        parents,
        author,
        committer,
    })
}

pub(crate) fn read_commit_metadata(
    store: &LooseObjectStore,
    id: &ObjectId,
) -> Result<CollectedCommitMetadata> {
    read_commit_metadata_from_store(store, id)
}

pub(crate) fn read_shallow_commits(repo: &GitRepo) -> Result<HashSet<ObjectId>> {
    let path = repo.git_dir.join("shallow");
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
        commits.insert(ObjectId::from_hex(GitHashAlgorithm::Sha1, line).map_err(CliError::Io)?);
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
    collect_promisor_object_ids_with_mode(repo, store, true)
}

pub(crate) fn collect_promised_missing_object_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
) -> Result<HashSet<ObjectId>> {
    collect_promisor_object_ids_with_mode(repo, store, false)
}

fn collect_promisor_object_ids_with_mode(
    repo: &GitRepo,
    store: &LooseObjectStore,
    include_present_promisor_objects: bool,
) -> Result<HashSet<ObjectId>> {
    if !partial_clone_enabled(repo)? {
        return Ok(HashSet::new());
    }
    let pack_dir = repo.objects_dir.join("pack");
    let entries = match fs::read_dir(&pack_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut promised = HashSet::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("promisor") {
            continue;
        }
        let pack_path = path.with_extension("pack");
        if !pack_path.is_file() {
            continue;
        }
        let ids = decode_pack_index_object_ids_from_path(GitHashAlgorithm::Sha1, &pack_path)
            .map_err(CliError::Io)?;
        for id in ids {
            let present = store.contains_object(&id).map_err(CliError::Io)?;
            if include_present_promisor_objects || !present {
                promised.insert(id.clone());
            }
            if !present {
                continue;
            }
            let object = store.read_object(&id).map_err(CliError::Io)?;
            collect_promised_missing_links(store, &object, &mut promised).map_err(CliError::Io)?;
        }
    }
    Ok(promised)
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
    let excluded = collect_excluded_commits_cached(repo, store, &commit_cache, &revs.exclude)?;
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
    let excluded = collect_excluded_commits_cached(repo, store, commit_cache, &revs.exclude)?;
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
    let excluded =
        collect_excluded_commits_cached_into(repo, store, commit_cache, exclude_revs, excluded)?;
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
    let excluded = collect_excluded_commits_uncached(repo, store, &revs.exclude)?;
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
    let mut excluded = HashSet::with_capacity(root_traversal_capacity_hint(revs.len()));
    collect_commits_uncached_into_set(repo, store, revs, &mut excluded)?;
    Ok(excluded)
}

fn collect_commits_uncached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    max_count: Option<usize>,
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
    collect_pending_commits_uncached(repo, store, pending, scheduled, sequence, max_count, None)
}

fn collect_commits_uncached_into_set(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &[String],
    out: &mut HashSet<ObjectId>,
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
    collect_pending_commits_uncached_into_set(repo, store, pending, scheduled, sequence, out)
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
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, pending_commit.parents.len());
        for parent in pending_commit.parents {
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
        reserve_commit_parent_traversal(&mut pending, &mut scheduled, pending_commit.parents.len());
        for parent in pending_commit.parents {
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
    Ok(PendingCommitLite {
        id,
        tree,
        parents,
        timestamp,
    })
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
        collect_rev_list_tree_object_ref_ids(&mut tree_cache, &tree, &mut seen, missing_policy)?;
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
    collect_excluded_commits_cached_into(
        repo,
        store,
        commit_cache,
        revs,
        HashSet::with_capacity(root_traversal_capacity_hint(revs.len())),
    )
}

fn collect_excluded_commits_cached_into<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    mut excluded: HashSet<ObjectId>,
) -> Result<HashSet<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    if revs.is_empty() {
        return Ok(excluded);
    }
    collect_commits_cached_into_set(repo, store, commit_cache, revs, &mut excluded)?;
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
    let excluded = collect_excluded_commits_cached(repo, store, commit_cache, &revs.exclude)?;
    collect_commit_objects_cached_with_excluded(
        repo,
        store,
        commit_cache,
        &revs.include,
        max_count,
        &excluded,
    )
}

fn collect_commit_objects_cached_with_excluded<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
    excluded: &HashSet<ObjectId>,
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
    )
}

pub(crate) fn collect_commit_metadata_with_exclusions(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommitMetadata>> {
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
                read_pending_commit_metadata(store, id)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    collect_pending_commit_metadata(
        repo, store, pending, scheduled, sequence, max_count, &excluded,
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
            reserve_commit_parent_traversal(&mut pending, &mut scheduled, commit.parents.len());
            for parent in &commit.parents {
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
            reserve_commit_parent_traversal(&mut pending, &mut scheduled, metadata.parents.len());
            for parent in &metadata.parents {
                if excluded.contains(parent) {
                    continue;
                }
                if scheduled.insert(parent.clone()) {
                    pending.push(HeapPendingCommitMetadata::new(
                        read_pending_commit_metadata(store, parent.clone())?,
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
    if revs.exclude.is_empty() {
        return Ok(Vec::new());
    }
    let commit_cache = CommitObjectCache::new(store);
    collect_commits_cached(repo, store, &commit_cache, &revs.exclude, None)
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
    collect_commits_uncached(repo, store, &revs.exclude, None)
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
    let commit_cache = CommitObjectCache::new(store);
    let mut ref_tree_cache = TreeObjectRefCache::with_capacity(
        store,
        tree_cache_capacity_hint(commits.len(), excluded_commits.len()),
    );
    for commit_id in excluded_commits {
        let commit = match commit_cache.read_commit(commit_id) {
            Ok(commit) => commit,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        collect_rev_list_tree_object_ref_ids(
            &mut ref_tree_cache,
            &commit.tree,
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
        let commit = commit_cache.read_commit(commit_id)?;
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
        collect_rev_list_tree_object_ref_ids(
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
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
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
    for commit_id in excluded_commits {
        let commit = match commit_cache.read_commit(commit_id) {
            Ok(commit) => commit,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        collect_rev_list_tree_object_ref_ids(
            &mut tree_cache,
            &commit.tree,
            seen,
            RevListTreeMissingPolicy::default(),
        )?;
    }
    for id in extra_objects {
        for_each_rev_list_extra_object_id_into_cached(
            store,
            commit_cache,
            &mut tree_cache,
            id,
            seen,
            &mut visit,
        )?;
    }
    for commit_id in commits {
        let commit = commit_cache.read_commit(commit_id)?;
        for_each_rev_list_tree_id_ordered_into(&mut tree_cache, &commit.tree, seen, &mut visit)?;
    }
    for commit_id in commits {
        let commit = commit_cache.read_commit(commit_id)?;
        for_each_rev_list_non_tree_object_id_ordered_into(
            &mut tree_cache,
            &commit.tree,
            seen,
            &mut visit,
        )?;
    }
    Ok(())
}

fn for_each_rev_list_extra_object_id_into_cached<F>(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
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
            let commit = commit_cache.read_commit(id)?;
            for_each_rev_list_tree_id_ordered_into(tree_cache, &commit.tree, seen, visit)?;
            for_each_rev_list_non_tree_object_id_ordered_into(tree_cache, &commit.tree, seen, visit)
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
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
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
    for commit_id in excluded_commits {
        let commit = commit_cache.read_commit(commit_id)?;
        collect_rev_list_tree_object_ids(&mut ref_tree_cache, &commit.tree, &mut seen)?;
    }
    let mut path = Vec::new();
    for commit_id in commits {
        let commit = commit_cache.read_commit(commit_id)?;
        collect_rev_list_tree_object_paths(
            &tree_cache,
            &commit.tree,
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
        collect_rev_list_tree_object_ref_ids(&mut tree_cache, &tree, &mut seen, missing_policy)?;
    }
    for (id, _) in extra_objects {
        count +=
            count_rev_list_extra_object_ids(store, &mut tree_cache, id, &mut seen, missing_policy)?;
    }
    for commit in commits {
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
    let mut depths = HashMap::with_capacity(1024);
    depths.insert(start.clone(), 0usize);
    let mut pending = VecDeque::from([start.clone()]);
    while let Some(id) = pending.pop_front() {
        let depth = depths[&id];
        let links = commit_cache.read_commit_links(&id)?;
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
    let mut depths = HashMap::with_capacity(1024);
    depths.insert(start.clone(), 0usize);
    let mut pending = VecDeque::from([start.clone()]);
    while let Some(id) = pending.pop_front() {
        if shallow_commits.contains(&id) {
            continue;
        }
        let depth = depths[&id];
        let links = commit_cache.read_commit_links(&id)?;
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
    if is_ancestor_commit_cached(commit_cache, right, left)? {
        return Ok(Some(right.clone()));
    }
    if is_ancestor_commit_cached(commit_cache, left, right)? {
        return Ok(Some(left.clone()));
    }

    let left_depths = commit_depths_cached(commit_cache, left)?;
    let right_depths = commit_depths_cached(commit_cache, right)?;
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
    if is_ancestor_commit_with_repo_cached(repo, commit_cache, right, left)? {
        return Ok(Some(right.clone()));
    }
    if is_ancestor_commit_with_repo_cached(repo, commit_cache, left, right)? {
        return Ok(Some(left.clone()));
    }

    let left_depths = commit_depths_with_repo_cached(repo, commit_cache, left)?;
    let right_depths = commit_depths_with_repo_cached(repo, commit_cache, right)?;
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
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Vec<ObjectId>> {
    if is_ancestor_commit_cached(commit_cache, right, left)? {
        return Ok(vec![right.clone()]);
    }
    if is_ancestor_commit_cached(commit_cache, left, right)? {
        return Ok(vec![left.clone()]);
    }

    let left_depths = commit_depths_cached(commit_cache, left)?;
    let right_depths = commit_depths_cached(commit_cache, right)?;
    let (scan_depths, lookup_depths) = if left_depths.len() <= right_depths.len() {
        (&left_depths, &right_depths)
    } else {
        (&right_depths, &left_depths)
    };
    let mut candidates = Vec::with_capacity(common_merge_base_candidate_capacity(
        left_depths.len(),
        right_depths.len(),
    ));
    for id in scan_depths.keys() {
        if lookup_depths.contains_key(id) {
            candidates.push(id.clone());
        }
    }
    candidates.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

    let mut bases = Vec::new();
    for candidate in &candidates {
        let mut is_redundant = false;
        for other in &candidates {
            if candidate != other && is_ancestor_commit_cached(commit_cache, candidate, other)? {
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

pub(crate) fn best_multi_merge_base_cached(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
) -> Result<Option<ObjectId>> {
    let left_depths = commit_depths_cached(commit_cache, &commits[0])?;
    let mut other_depths = Vec::with_capacity(commits.len().saturating_sub(1));
    for commit in &commits[1..] {
        other_depths.push(commit_depths_cached(commit_cache, commit)?);
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
    let left_depths = commit_depths_cached(commit_cache, &commits[0])?;
    let mut other_depths = Vec::with_capacity(commits.len().saturating_sub(1));
    for commit in &commits[1..] {
        other_depths.push(commit_depths_cached(commit_cache, commit)?);
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

    let mut seen: Option<HashSet<ObjectId>> = None;
    let mut stack = Vec::with_capacity(128);
    let mut current = descendant.clone();

    loop {
        let links = commit_cache.read_commit_links(&current)?;

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
        let links = commit_cache.read_commit_links(&id)?;
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
    let mut seen: Option<HashSet<ObjectId>> = None;
    let mut stack = Vec::with_capacity(128);
    let mut current = descendant.clone();

    loop {
        if shallow_commits.contains(&current) {
            return Ok(false);
        }
        let links = commit_cache.read_commit_links(&current)?;

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
        let links = commit_cache.read_commit_links(&id)?;
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
    match objectish.len() {
        40 => {
            if let Ok(id) = ObjectId::from_hex(GitHashAlgorithm::Sha1, objectish) {
                if store.object_kind_hint(&id)?.is_none() {
                    return Ok(None);
                }
                return parse_commit_full_object_id_if_commit(store, objectish, id);
            }
        }
        64 => {
            if let Ok(id) = ObjectId::from_hex(GitHashAlgorithm::Sha256, objectish) {
                if store.object_kind_hint(&id)?.is_none() {
                    return Ok(None);
                }
                return parse_commit_full_object_id_if_commit(store, objectish, id);
            }
        }
        _ => {}
    }
    Ok(None)
}

fn parse_commit_full_object_id_if_commit(
    store: &LooseObjectStore,
    input: &str,
    id: ObjectId,
) -> io::Result<Option<ObjectId>> {
    let mut object_id = id;
    for _ in 0..8 {
        let object = store.read_object(&object_id)?;
        match object.kind {
            GitObjectKind::Commit => {
                return Ok(Some(object_id));
            }
            GitObjectKind::Tag => {
                object_id =
                    zmin_git_core::decode_tag(GitHashAlgorithm::Sha1, &object.content)?.target;
            }
            _ => {
                return Ok(None);
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("ambiguous object id `{input}`"),
    ))
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
    collect_reflog_roots_from_path(&logs_dir, roots)
}

pub(crate) fn collect_reflog_roots_from_path(
    path: &std::path::Path,
    roots: &mut Vec<ObjectId>,
) -> Result<()> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            collect_reflog_roots_from_path(&entry?.path(), roots)?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        let file = fs::File::open(path)?;
        let mut reader = io::BufReader::new(file);
        let mut line = String::new();
        while reader.read_line(&mut line)? != 0 {
            collect_reflog_line_roots(&line, roots);
            line.clear();
        }
    }
    Ok(())
}

pub(crate) fn collect_reflog_line_roots(line: &str, roots: &mut Vec<ObjectId>) {
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
        if let Ok(id) = ObjectId::from_hex(GitHashAlgorithm::Sha1, id) {
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
        let old_id = "1111111111111111111111111111111111111111";
        let new_id = "2222222222222222222222222222222222222222";
        fs::write(
            &log_path,
            format!(
                "0000000000000000000000000000000000000000 {old_id} user <u@example.com> 1 +0000\tinit\n\
                 {old_id} {new_id} user <u@example.com> 2 +0000\tcommit\n\
                 invalid line\n"
            ),
        )
        .expect("write reflog");

        let mut roots = Vec::new();
        collect_reflog_roots_from_path(&log_path, &mut roots).expect("collect reflog roots");

        assert_eq!(
            roots.iter().map(ObjectId::to_hex).collect::<Vec<_>>(),
            vec![old_id.to_owned(), old_id.to_owned(), new_id.to_owned()]
        );
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
            },
            CollectedCommitTree {
                id: second_commit,
                tree: second_tree,
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
        let store = LooseObjectStore::new(dir.path().join("objects"), GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, "root");
        let head = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, "head");
        fs::write(
            dir.path().join("shallow"),
            format!("\n{}\n{}\n", root.to_hex(), head.to_hex()),
        )
        .expect("write shallow");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir: dir.path().to_path_buf(),
            objects_dir: dir.path().join("objects"),
            index_path: dir.path().join("index"),
        };

        let shallow = read_shallow_commits(&repo).expect("read shallow commits");

        assert_eq!(shallow.len(), 2);
        assert!(shallow.contains(&root));
        assert!(shallow.contains(&head));
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
