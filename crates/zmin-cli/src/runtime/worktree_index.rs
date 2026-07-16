use super::*;
use encoding_rs::{Encoding, SHIFT_JIS};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use zmin_git_core::GitObjectStore;
use zmin_primitives::Error as PrimitiveError;
use zmin_primitives::git_runtime::GitRefsStore;

const SMALL_WORKTREE_BLOB_READ_BYTES: usize = 64 * 1024;
const BULK_CHECKIN_HASH_BUFFER_BYTES: usize = 16 * 1024;
const PARALLEL_STAGE_REGULAR_MIN_FILES: usize = 128;
const PARALLEL_STAGE_REGULAR_MAX_WORKERS: usize = 4;
const PARALLEL_TRACKED_SCAN_MIN_FILES: usize = 1024;
const PARALLEL_TRACKED_SCAN_MAX_WORKERS: usize = 8;
const PARALLEL_STATUS_SCAN_MIN_FILES: usize = 512;
const PARALLEL_STATUS_SCAN_MAX_WORKERS: usize = 2;

#[derive(Debug, Clone)]
pub(crate) struct BulkCheckinCandidate {
    pub(crate) path: PathBuf,
    pub(crate) relative: Vec<u8>,
    pub(crate) size: u64,
}

#[derive(Debug, Clone)]
struct BulkCheckinObject {
    path: PathBuf,
    id: ObjectId,
    size: u64,
}

pub(crate) fn read_repo_index(repo: &GitRepo) -> Result<GitIndex> {
    let index = read_repo_index_raw(repo)?;
    if index_has_sparse_directories(&index) {
        Ok(expand_repo_sparse_index(repo, &index)?)
    } else {
        Ok(index)
    }
}

pub(crate) fn read_repo_index_raw(repo: &GitRepo) -> Result<GitIndex> {
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    if repo.index_path.exists() {
        read_index_with_algorithm(&repo.index_path, algorithm).map_err(map_read_index_error)
    } else {
        Ok(GitIndex::new_with_algorithm(algorithm))
    }
}

pub(crate) fn index_has_sparse_directories(index: &GitIndex) -> bool {
    index
        .entries()
        .iter()
        .any(|entry| entry.stage == 0 && entry.mode == IndexMode::Tree)
}

pub(crate) fn sparse_index_path_requires_expansion(index: &GitIndex, path: &[u8]) -> bool {
    !path.iter().any(|byte| matches!(byte, b'*' | b'?' | b'['))
        && index.entries().iter().any(|entry| {
            entry.stage == 0
                && entry.mode == IndexMode::Tree
                && path.starts_with(&entry.path)
                && path.len() > entry.path.len()
        })
}

pub(crate) fn refresh_materialized_sparse_index_entries(
    repo: &GitRepo,
    expanded_index: &mut GitIndex,
) -> Result<bool> {
    let mut changed = false;
    let entries = expanded_index
        .entries()
        .iter()
        .cloned()
        .map(|mut entry| {
            if entry.stage == 0
                && entry.skip_worktree()
                && path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path))
            {
                entry.set_skip_worktree(false);
                changed = true;
            }
            entry
        })
        .collect::<Vec<_>>();
    if !changed {
        return Ok(false);
    }
    *expanded_index = GitIndex::from_entries(entries)?;
    expanded_index.write_to_path(&repo.index_path)?;
    Ok(true)
}

pub(crate) fn expand_repo_sparse_index(repo: &GitRepo, index: &GitIndex) -> io::Result<GitIndex> {
    if !index_has_sparse_directories(index) {
        return Ok(index.clone());
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), index.hash_algorithm());
    let tree_cache = TreeObjectCache::new(&store);
    let mut entries = Vec::new();
    for entry in index.entries() {
        if entry.stage != 0 || entry.mode != IndexMode::Tree {
            entries.push(entry.clone());
            continue;
        }
        let prefix = entry.path.strip_suffix(b"/").unwrap_or(&entry.path);
        let subtree = tree_cache.read_tree_to_index(&entry.id)?;
        for child in subtree.entries() {
            let mut expanded = child.clone();
            let mut path = Vec::with_capacity(prefix.len() + 1 + child.path.len());
            path.extend_from_slice(prefix);
            path.push(b'/');
            path.extend_from_slice(&child.path);
            expanded.path = path;
            expanded.set_skip_worktree(true);
            entries.push(expanded);
        }
    }
    Ok(GitIndex::from_entries(entries)?)
}

fn map_read_index_error(error: std::io::Error) -> CliError {
    if let Some(version) = bad_index_version(&error) {
        return CliError::Stderr {
            code: 128,
            text: format!("error: bad index version {version}\nfatal: index file corrupt\n"),
        };
    }
    if let Some(extension_error) = unknown_required_index_extension(&error) {
        return CliError::Stderr {
            code: 128,
            text: format!("error: {extension_error}\nfatal: index file corrupt\n"),
        };
    }
    CliError::Io(error)
}

fn bad_index_version(error: &std::io::Error) -> Option<String> {
    error
        .to_string()
        .strip_prefix("bad index version ")
        .map(str::to_owned)
}

fn unknown_required_index_extension(error: &std::io::Error) -> Option<String> {
    let message = error.to_string();
    if message.starts_with("index uses ")
        && message.ends_with(" extension, which we do not understand")
    {
        Some(message)
    } else {
        None
    }
}

pub(crate) fn stage_tracked_worktree_changes(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
) -> Result<()> {
    let _ = stage_tracked_worktree_changes_matching(repo, store, index, &[], &HashSet::new())?;
    Ok(())
}

pub(crate) fn stage_tracked_worktree_changes_matching(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    pathspecs: &[Vec<u8>],
    already_staged: &HashSet<Vec<u8>>,
) -> Result<bool> {
    let index_mtime = repo_index_mtime(repo)?;
    let stage_options = WorktreeStageOptions::load(repo)?;
    let mut trace = TrackedWorktreeTrace::new();
    let parallel_regular_scan = try_scan_tracked_regular_files_parallel(
        repo,
        index,
        pathspecs,
        already_staged,
        index_mtime,
        &stage_options,
        ParallelTrackedScanPolicy::Stage,
        true,
        &mut trace,
    )?;
    let mut changed = false;
    let unmerged_paths = index
        .entries()
        .iter()
        .filter(|entry| entry.stage != 0 && pathspec_matches(&entry.path, pathspecs))
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    for path in unmerged_paths {
        if already_staged.contains(&path) {
            continue;
        }
        let absolute = worktree_path_for_index_entry(&repo.root, &path);
        if path_exists(&absolute) {
            stage_file_with_mode_and_index_mtime_and_options(
                repo,
                store,
                index,
                &absolute,
                None,
                index_mtime,
                &stage_options,
            )?;
        } else {
            index.remove_path(&path)?;
        }
        changed = true;
    }
    let mut entry_idx = 0;
    while entry_idx < index.entries().len() {
        let entry = &index.entries()[entry_idx];
        if entry.stage != 0 {
            trace.skipped_non_stage_zero += 1;
            entry_idx += 1;
            continue;
        }
        trace.entries += 1;
        let path = entry.path.to_vec();
        if !pathspec_matches(&path, pathspecs) {
            trace.skipped_pathspec += 1;
            entry_idx = next_index_position_after_path(index, &path);
            continue;
        }
        if already_staged.contains(&path) {
            trace.skipped_already_staged += 1;
            entry_idx = next_index_position_after_path(index, &path);
            continue;
        }
        if entry.skip_worktree() {
            trace.skipped_worktree += 1;
            entry_idx = next_index_position_after_path(index, &path);
            continue;
        }
        if let Some(scan) = parallel_regular_scan.as_ref()
            && let Some(action) = scan.action_for_path(&path)
        {
            match action {
                ParallelTrackedRegularAction::Unchanged => {
                    entry_idx = next_index_position_after_path(index, &path);
                    continue;
                }
                ParallelTrackedRegularAction::Deleted => {
                    trace.deleted += 1;
                    index.remove_path(&path)?;
                    changed = true;
                    entry_idx = next_index_position_after_path(index, &path);
                    continue;
                }
                ParallelTrackedRegularAction::RemovedDir => {
                    trace.removed_dirs += 1;
                    index.remove_path(&path)?;
                    changed = true;
                    entry_idx = next_index_position_after_path(index, &path);
                    continue;
                }
                ParallelTrackedRegularAction::ContentHash(id) if id == &entry.id => {
                    entry_idx = next_index_position_after_path(index, &path);
                    continue;
                }
                ParallelTrackedRegularAction::ModeChanged
                | ParallelTrackedRegularAction::Modified
                | ParallelTrackedRegularAction::NeedsStage(_)
                | ParallelTrackedRegularAction::ContentHash(_) => {
                    trace.modified += 1;
                    let stage_started = trace.started();
                    stage_file_with_mode_and_index_mtime_and_options(
                        repo,
                        store,
                        index,
                        &worktree_path_for_index_entry(&repo.root, &path),
                        None,
                        index_mtime,
                        &stage_options,
                    )?;
                    trace.record_stage_file(stage_started);
                    trace.restaged += 1;
                    changed = true;
                    entry_idx = next_index_position_after_path(index, &path);
                    continue;
                }
            }
        }
        let absolute = worktree_path_for_index_entry(&repo.root, &path);
        let metadata_started = trace.started();
        let metadata = match fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(_) => {
                trace.record_metadata(metadata_started);
                trace.deleted += 1;
                index.remove_path(&path)?;
                changed = true;
                entry_idx = next_index_position_after_path(index, &path);
                continue;
            }
        };
        trace.record_metadata(metadata_started);
        if metadata.is_dir() && index.entries()[entry_idx].mode != IndexMode::Gitlink {
            trace.removed_dirs += 1;
            index.remove_path(&path)?;
            changed = true;
            entry_idx = next_index_position_after_path(index, &path);
            continue;
        }
        let stage_needed_started = trace.started();
        let stage_needed = {
            let entry = &index.entries()[entry_idx];
            tracked_entry_needs_stage(
                &absolute,
                &metadata,
                entry,
                index_mtime,
                &stage_options,
                Some(&mut trace),
            )?
        };
        trace.record_stage_needed_check(stage_needed_started);
        if stage_needed {
            trace.modified += 1;
            let stage_started = trace.started();
            stage_file_with_mode_and_index_mtime_and_options(
                repo,
                store,
                index,
                &absolute,
                None,
                index_mtime,
                &stage_options,
            )?;
            trace.record_stage_file(stage_started);
            trace.restaged += 1;
            changed = true;
        }
        entry_idx = next_index_position_after_path(index, &path);
    }
    trace.emit();
    Ok(changed)
}

pub(crate) fn renormalize_tracked_worktree_changes_matching(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<bool> {
    let stage_options = WorktreeStageOptions::load(repo)?;
    let mut changed = false;
    let mut selected = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && pathspec_matches(&entry.path, pathspecs))
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    selected.sort();
    selected.dedup();
    for path in selected {
        let absolute = worktree_path_for_index_entry(&repo.root, &path);
        let Ok(metadata) = fs::symlink_metadata(&absolute) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Some(before) = find_index_entry(index, &path).cloned() else {
            continue;
        };
        let mode = if before.mode == IndexMode::Executable && !stage_options.filemode_enabled() {
            IndexMode::Executable
        } else {
            stage_options.index_mode_for_metadata(&metadata)
        };
        let content =
            stage_options.renormalize_staged_worktree_content(repo, &path, fs::read(&absolute)?)?;
        stage_resolved_content(store, index, path.clone(), content, mode, &metadata)?;
        if before.id
            != find_index_entry(index, &path)
                .map(|entry| entry.id.clone())
                .unwrap_or_else(|| before.id.clone())
            || before.mode
                != find_index_entry(index, &path)
                    .map(|entry| entry.mode)
                    .unwrap_or(before.mode)
        {
            changed = true;
        }
    }
    Ok(changed)
}

struct ParallelTrackedRegularScan {
    actions: Vec<Option<ParallelTrackedRegularAction>>,
    path_positions: Option<HashMap<Vec<u8>, usize>>,
}

#[derive(Clone, Copy)]
enum ParallelTrackedScanPolicy {
    Stage,
    Status,
}

#[derive(Clone, Copy)]
enum WorktreeContentComparison {
    Raw,
    RawIfNoCr,
    Converted,
}

impl ParallelTrackedScanPolicy {
    fn min_files(self) -> usize {
        match self {
            Self::Stage => PARALLEL_TRACKED_SCAN_MIN_FILES,
            Self::Status => PARALLEL_STATUS_SCAN_MIN_FILES,
        }
    }

    fn max_workers(self) -> usize {
        match self {
            Self::Stage => PARALLEL_TRACKED_SCAN_MAX_WORKERS,
            Self::Status => PARALLEL_STATUS_SCAN_MAX_WORKERS,
        }
    }
}

impl ParallelTrackedRegularScan {
    fn action_for_path(&self, path: &[u8]) -> Option<&ParallelTrackedRegularAction> {
        let position = *self.path_positions.as_ref()?.get(path)?;
        self.actions[position].as_ref()
    }
}

enum ParallelTrackedRegularAction {
    Unchanged,
    Deleted,
    RemovedDir,
    ModeChanged,
    Modified,
    NeedsStage(u64),
    ContentHash(ObjectId),
}

struct ParallelTrackedRegularCandidate<'a> {
    index_position: usize,
    absolute: PathBuf,
    entry: &'a IndexEntry,
}

#[derive(Default)]
struct ParallelTrackedRegularChunk {
    actions: Vec<(usize, ParallelTrackedRegularAction)>,
    metadata_seconds: f64,
    modified_check_seconds: f64,
    content_hash_seconds: f64,
    stat_safe: u64,
    mode_changed: u64,
    content_hashes: u64,
}

#[derive(Default)]
struct ParallelTrackedContentHashChunk {
    actions: Vec<(usize, ParallelTrackedRegularAction)>,
    seconds: f64,
    hashes: u64,
}

#[derive(Clone, Copy)]
struct ParallelTrackedHashCandidate {
    candidate_index: usize,
    detect_cr: bool,
}

#[derive(Default)]
struct ParallelTrackedHashBucket {
    candidates: Vec<ParallelTrackedHashCandidate>,
    bytes: u64,
}

fn try_scan_tracked_regular_files_parallel(
    repo: &GitRepo,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
    already_staged: &HashSet<Vec<u8>>,
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
    policy: ParallelTrackedScanPolicy,
    preserve_path_lookup: bool,
    trace: &mut TrackedWorktreeTrace,
) -> Result<Option<ParallelTrackedRegularScan>> {
    if !pathspecs.is_empty() || !already_staged.is_empty() {
        return Ok(None);
    }
    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(policy.max_workers());
    if workers <= 1 {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for (index_position, entry) in index.entries().iter().enumerate() {
        if entry.stage != 0 || entry.skip_worktree() {
            continue;
        }
        if !matches!(entry.mode, IndexMode::File | IndexMode::Executable) {
            continue;
        }
        candidates.push(ParallelTrackedRegularCandidate {
            index_position,
            absolute: worktree_path_for_index_entry(&repo.root, &entry.path),
            entry,
        });
    }
    if candidates.len() < policy.min_files() {
        return Ok(None);
    }
    let chunks = std::thread::scope(|scope| {
        let candidates = &candidates;
        let mut handles = Vec::new();
        for worker in 0..workers {
            handles.push(scope.spawn(move || -> Result<ParallelTrackedRegularChunk> {
                let mut result = ParallelTrackedRegularChunk {
                    actions: Vec::with_capacity(candidates.len().div_ceil(workers)),
                    ..ParallelTrackedRegularChunk::default()
                };
                for candidate in candidates.iter().skip(worker).step_by(workers) {
                    let metadata_started = Instant::now();
                    let metadata = match fs::symlink_metadata(&candidate.absolute) {
                        Ok(metadata) => metadata,
                        Err(_) => {
                            result.metadata_seconds += metadata_started.elapsed().as_secs_f64();
                            result.actions.push((
                                candidate.index_position,
                                ParallelTrackedRegularAction::Deleted,
                            ));
                            continue;
                        }
                    };
                    result.metadata_seconds += metadata_started.elapsed().as_secs_f64();
                    let modified_started = Instant::now();
                    let action = parallel_tracked_regular_action(
                        &metadata,
                        &candidate.entry,
                        index_mtime,
                        stage_options.filemode_enabled(),
                        stage_options.stat_options(),
                        &mut result,
                    );
                    result.modified_check_seconds += modified_started.elapsed().as_secs_f64();
                    result.actions.push((candidate.index_position, action));
                }
                Ok(result)
            }));
        }
        let mut chunks = Vec::with_capacity(handles.len());
        for handle in handles {
            chunks.push(handle.join().map_err(|_| {
                CliError::Message("parallel tracked scan worker panicked".into())
            })??);
        }
        Ok::<_, CliError>(chunks)
    })?;
    let mut actions = (0..index.entries().len()).map(|_| None).collect::<Vec<_>>();
    for chunk in chunks {
        trace.metadata_seconds += chunk.metadata_seconds;
        trace.modified_check_seconds += chunk.modified_check_seconds;
        trace.content_hash_seconds += chunk.content_hash_seconds;
        trace.stat_safe += chunk.stat_safe;
        trace.mode_changed += chunk.mode_changed;
        trace.content_hashes += chunk.content_hashes;
        for (index_position, action) in chunk.actions {
            actions[index_position] = Some(action);
        }
    }
    let mut hash_candidates = Vec::new();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        let Some(ParallelTrackedRegularAction::NeedsStage(_)) =
            actions[candidate.index_position].as_ref()
        else {
            continue;
        };
        let size = match actions[candidate.index_position].as_ref() {
            Some(ParallelTrackedRegularAction::NeedsStage(size)) => *size,
            _ => 0,
        };
        if matches!(policy, ParallelTrackedScanPolicy::Status)
            && index_entry_has_stat_cache(candidate.entry)
            && u32::try_from(size).is_ok_and(|size| size != candidate.entry.size)
        {
            actions[candidate.index_position] = Some(ParallelTrackedRegularAction::Modified);
            continue;
        }
        let conversion_started = Instant::now();
        let comparison = stage_options.content_comparison(repo, &candidate.entry.path)?;
        trace.modified_check_seconds += conversion_started.elapsed().as_secs_f64();
        if matches!(comparison, WorktreeContentComparison::Raw) {
            hash_candidates.push(ParallelTrackedHashCandidate {
                candidate_index,
                detect_cr: false,
            });
        } else if matches!(comparison, WorktreeContentComparison::RawIfNoCr) {
            hash_candidates.push(ParallelTrackedHashCandidate {
                candidate_index,
                detect_cr: true,
            });
        }
    }
    let hash_workers = workers.min(hash_candidates.len());
    hash_candidates.sort_unstable_by_key(|hash_candidate| {
        let candidate = &candidates[hash_candidate.candidate_index];
        let size = match actions[candidate.index_position].as_ref() {
            Some(ParallelTrackedRegularAction::NeedsStage(size)) => *size,
            _ => 0,
        };
        std::cmp::Reverse(size)
    });
    let mut hash_buckets = (0..hash_workers)
        .map(|_| ParallelTrackedHashBucket::default())
        .collect::<Vec<_>>();
    for hash_candidate in hash_candidates {
        let candidate = &candidates[hash_candidate.candidate_index];
        let size = match actions[candidate.index_position].as_ref() {
            Some(ParallelTrackedRegularAction::NeedsStage(size)) => *size,
            _ => 0,
        };
        let bucket = hash_buckets
            .iter_mut()
            .min_by_key(|bucket| bucket.bytes)
            .expect("hash worker count follows non-empty candidates");
        bucket.candidates.push(hash_candidate);
        bucket.bytes = bucket.bytes.saturating_add(size);
    }
    let hash_chunks = std::thread::scope(|scope| {
        let actions = &actions;
        let candidates = &candidates;
        let mut handles = Vec::new();
        for bucket in &hash_buckets {
            handles.push(
                scope.spawn(move || -> Result<ParallelTrackedContentHashChunk> {
                    let mut result = ParallelTrackedContentHashChunk {
                        actions: Vec::with_capacity(bucket.candidates.len()),
                        ..ParallelTrackedContentHashChunk::default()
                    };
                    for hash_candidate in &bucket.candidates {
                        let candidate = &candidates[hash_candidate.candidate_index];
                        let Some(ParallelTrackedRegularAction::NeedsStage(size)) =
                            actions[candidate.index_position].as_ref()
                        else {
                            continue;
                        };
                        let started = Instant::now();
                        let (id, usable) = if hash_candidate.detect_cr {
                            let (id, has_cr) =
                                hash_worktree_file_blob_detect_cr(&candidate.absolute, *size)?;
                            (id, !has_cr)
                        } else {
                            (hash_worktree_file_blob(&candidate.absolute, *size)?, true)
                        };
                        result.seconds += started.elapsed().as_secs_f64();
                        result.hashes += 1;
                        if usable {
                            result.actions.push((
                                candidate.index_position,
                                ParallelTrackedRegularAction::ContentHash(id),
                            ));
                        }
                    }
                    Ok(result)
                }),
            );
        }
        let mut chunks = Vec::with_capacity(handles.len());
        for handle in handles {
            chunks.push(handle.join().map_err(|_| {
                CliError::Message("parallel tracked hash worker panicked".into())
            })??);
        }
        Ok::<_, CliError>(chunks)
    })?;
    for chunk in hash_chunks {
        trace.content_hash_seconds += chunk.seconds;
        trace.content_hashes += chunk.hashes;
        for (index_position, action) in chunk.actions {
            actions[index_position] = Some(action);
        }
    }
    let path_positions = preserve_path_lookup.then(|| {
        candidates
            .iter()
            .map(|candidate| (candidate.entry.path.clone(), candidate.index_position))
            .collect()
    });
    Ok(Some(ParallelTrackedRegularScan {
        actions,
        path_positions,
    }))
}

fn parallel_tracked_regular_action(
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    index_mtime: Option<IndexTimestamp>,
    filemode_enabled: bool,
    stat_options: IndexStatOptions,
    chunk: &mut ParallelTrackedRegularChunk,
) -> ParallelTrackedRegularAction {
    if metadata.is_dir() {
        chunk.mode_changed += 1;
        return ParallelTrackedRegularAction::RemovedDir;
    }
    if !metadata.is_file()
        || parallel_index_mode_for_metadata(metadata, filemode_enabled) != entry.mode
    {
        chunk.mode_changed += 1;
        return ParallelTrackedRegularAction::ModeChanged;
    }
    if index_mtime
        .is_some_and(|mtime| index_entry_stat_match_is_safe(metadata, entry, mtime, stat_options))
    {
        chunk.stat_safe += 1;
        return ParallelTrackedRegularAction::Unchanged;
    }
    ParallelTrackedRegularAction::NeedsStage(metadata.len())
}

fn parallel_index_mode_for_metadata(metadata: &fs::Metadata, filemode_enabled: bool) -> IndexMode {
    if filemode_enabled {
        index_mode_for_metadata(metadata)
    } else {
        IndexMode::File
    }
}

#[derive(Default)]
struct TrackedWorktreeTrace {
    enabled: bool,
    entries: u64,
    skipped_non_stage_zero: u64,
    skipped_pathspec: u64,
    skipped_already_staged: u64,
    skipped_worktree: u64,
    deleted: u64,
    removed_dirs: u64,
    stat_safe: u64,
    mode_changed: u64,
    content_hashes: u64,
    converted_hashes: u64,
    stat_unsafe: u64,
    symlink_checks: u64,
    gitlink_checks: u64,
    modified: u64,
    restaged: u64,
    metadata_seconds: f64,
    modified_check_seconds: f64,
    content_hash_seconds: f64,
    conversion_seconds: f64,
    stage_file_seconds: f64,
}

impl TrackedWorktreeTrace {
    fn new() -> Self {
        Self {
            enabled: phase_trace_enabled(),
            ..Self::default()
        }
    }

    fn started(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    fn record_metadata(&mut self, started: Option<Instant>) {
        self.metadata_seconds += elapsed_seconds(started);
    }

    fn record_stage_needed_check(&mut self, started: Option<Instant>) {
        self.modified_check_seconds += elapsed_seconds(started);
    }

    fn record_content_hash(&mut self, started: Option<Instant>) {
        self.content_hash_seconds += elapsed_seconds(started);
    }

    fn record_conversion(&mut self, started: Option<Instant>) {
        self.conversion_seconds += elapsed_seconds(started);
    }

    fn record_stage_file(&mut self, started: Option<Instant>) {
        self.stage_file_seconds += elapsed_seconds(started);
    }

    fn emit(&self) {
        self.emit_with_label("add.stage_tracked.detail");
    }

    fn emit_with_label(&self, label: &'static str) {
        if !self.enabled {
            return;
        }
        phase_trace_emit(
            label,
            self.metadata_seconds + self.modified_check_seconds + self.stage_file_seconds,
            &[
                ("entries", self.entries.to_string()),
                (
                    "skipped_non_stage_zero",
                    self.skipped_non_stage_zero.to_string(),
                ),
                ("skipped_pathspec", self.skipped_pathspec.to_string()),
                (
                    "skipped_already_staged",
                    self.skipped_already_staged.to_string(),
                ),
                ("skipped_worktree", self.skipped_worktree.to_string()),
                ("deleted", self.deleted.to_string()),
                ("removed_dirs", self.removed_dirs.to_string()),
                ("stat_safe", self.stat_safe.to_string()),
                ("mode_changed", self.mode_changed.to_string()),
                ("content_hashes", self.content_hashes.to_string()),
                ("converted_hashes", self.converted_hashes.to_string()),
                ("stat_unsafe", self.stat_unsafe.to_string()),
                ("symlink_checks", self.symlink_checks.to_string()),
                ("gitlink_checks", self.gitlink_checks.to_string()),
                ("modified", self.modified.to_string()),
                ("restaged", self.restaged.to_string()),
                ("metadata_seconds", format!("{:.6}", self.metadata_seconds)),
                (
                    "modified_check_seconds",
                    format!("{:.6}", self.modified_check_seconds),
                ),
                (
                    "content_hash_seconds",
                    format!("{:.6}", self.content_hash_seconds),
                ),
                (
                    "conversion_seconds",
                    format!("{:.6}", self.conversion_seconds),
                ),
                (
                    "stage_file_seconds",
                    format!("{:.6}", self.stage_file_seconds),
                ),
            ],
        );
    }
}

fn elapsed_seconds(started: Option<Instant>) -> f64 {
    started
        .map(|started| started.elapsed().as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Default)]
pub(crate) struct StageFilesTrace {
    enabled: bool,
    files: u64,
    regular_files: u64,
    symlinks: u64,
    gitlinks: u64,
    converted_files: u64,
    stat_safe: u64,
    streamed_files: u64,
    small_existing_files: u64,
    unmerged_replacements: u64,
    unchanged_id_refreshes: u64,
    errors: u64,
    metadata_seconds: f64,
    read_seconds: f64,
    object_write_seconds: f64,
    parent_cleanup_seconds: f64,
    upsert_seconds: f64,
}

impl StageFilesTrace {
    pub(crate) fn new() -> Self {
        Self {
            enabled: phase_trace_enabled(),
            ..Self::default()
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    fn started(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    fn record_metadata(&mut self, started: Option<Instant>) {
        self.metadata_seconds += elapsed_seconds(started);
    }

    fn record_read(&mut self, started: Option<Instant>) {
        self.read_seconds += elapsed_seconds(started);
    }

    fn record_object_write(&mut self, started: Option<Instant>) {
        self.object_write_seconds += elapsed_seconds(started);
    }

    fn record_parent_cleanup(&mut self, started: Option<Instant>) {
        self.parent_cleanup_seconds += elapsed_seconds(started);
    }

    fn record_upsert(&mut self, started: Option<Instant>) {
        self.upsert_seconds += elapsed_seconds(started);
    }

    pub(crate) fn record_error(&mut self) {
        self.errors += 1;
    }

    pub(crate) fn emit(&self) {
        if !self.enabled {
            return;
        }
        phase_trace_emit(
            "add.stage_files.detail",
            self.metadata_seconds
                + self.read_seconds
                + self.object_write_seconds
                + self.parent_cleanup_seconds
                + self.upsert_seconds,
            &[
                ("files", self.files.to_string()),
                ("regular_files", self.regular_files.to_string()),
                ("symlinks", self.symlinks.to_string()),
                ("gitlinks", self.gitlinks.to_string()),
                ("converted_files", self.converted_files.to_string()),
                ("stat_safe", self.stat_safe.to_string()),
                ("streamed_files", self.streamed_files.to_string()),
                (
                    "small_existing_files",
                    self.small_existing_files.to_string(),
                ),
                (
                    "unmerged_replacements",
                    self.unmerged_replacements.to_string(),
                ),
                (
                    "unchanged_id_refreshes",
                    self.unchanged_id_refreshes.to_string(),
                ),
                ("errors", self.errors.to_string()),
                ("metadata_seconds", format!("{:.6}", self.metadata_seconds)),
                ("read_seconds", format!("{:.6}", self.read_seconds)),
                (
                    "object_write_seconds",
                    format!("{:.6}", self.object_write_seconds),
                ),
                (
                    "parent_cleanup_seconds",
                    format!("{:.6}", self.parent_cleanup_seconds),
                ),
                ("upsert_seconds", format!("{:.6}", self.upsert_seconds)),
            ],
        );
    }
}

pub(crate) fn refresh_tracked_index_metadata_matching(
    repo: &GitRepo,
    index: &mut GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let symlinks_enabled = repo_symlinks_enabled(repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let stage_options = WorktreeStageOptions::load(repo)?;
    let entries = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && pathspec_matches(&entry.path, pathspecs))
        .cloned()
        .collect::<Vec<_>>();
    for entry in entries {
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        let metadata = match fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let content_matches = match entry.mode {
            IndexMode::File | IndexMode::Executable => {
                if !metadata.is_file() {
                    false
                } else if stage_options.needs_content_conversion(repo, &entry.path)? {
                    let content = clean_worktree_content_for_comparison_against_index(
                        repo,
                        &store,
                        index,
                        &entry.path,
                        fs::read(&absolute)?,
                    )?;
                    hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content) == entry.id
                } else {
                    hash_worktree_file_blob(&absolute, metadata.len())? == entry.id
                }
            }
            IndexMode::Symlink => {
                symlink_content_matches_with_mode(&absolute, &entry, symlinks_enabled)?
            }
            IndexMode::Tree => false,
            IndexMode::Gitlink => false,
        };
        if content_matches {
            let mut refreshed = entry;
            apply_index_entry_metadata(&mut refreshed, &metadata);
            index.upsert(refreshed)?;
        }
    }
    Ok(())
}

pub(crate) fn refresh_tracked_index_metadata_after_checkout(
    repo: &GitRepo,
    index: &mut GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let symlinks_enabled = repo_symlinks_enabled(repo)?;
    let entries = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && pathspec_matches(&entry.path, pathspecs))
        .cloned()
        .collect::<Vec<_>>();
    for entry in entries {
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        let Ok(metadata) = fs::symlink_metadata(&absolute) else {
            continue;
        };
        let mode_matches = match entry.mode {
            IndexMode::File | IndexMode::Executable => metadata.is_file(),
            IndexMode::Symlink => {
                if symlinks_enabled {
                    metadata.file_type().is_symlink()
                } else {
                    metadata.is_file()
                }
            }
            IndexMode::Tree | IndexMode::Gitlink => false,
        };
        if mode_matches {
            let mut refreshed = entry;
            apply_index_entry_metadata(&mut refreshed, &metadata);
            index.upsert(refreshed)?;
        }
    }
    Ok(())
}

pub(crate) fn upsert_index_content(
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: Vec<u8>,
    content: Vec<u8>,
    mode: IndexMode,
) -> Result<()> {
    let size = content.len().min(u32::MAX as usize) as u32;
    let id = store.write_object(GitObjectKind::Blob, &content)?;
    index.upsert(IndexEntry::new(path, id, mode, size)?)?;
    Ok(())
}

pub(crate) fn worktree_index_snapshot(repo: &GitRepo, index: &GitIndex) -> Result<GitIndex> {
    worktree_index_snapshot_with_missing(repo, index, false)
}

fn worktree_index_snapshot_with_missing(
    repo: &GitRepo,
    index: &GitIndex,
    keep_missing: bool,
) -> Result<GitIndex> {
    let mut snapshot = index.clone();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        if entry.skip_worktree() {
            continue;
        }
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        if path_exists(&absolute) {
            if entry.mode == IndexMode::Gitlink {
                snapshot.upsert(worktree_gitlink_index_entry(entry, &absolute)?)?;
            } else if fs::symlink_metadata(&absolute)
                .map(|metadata| metadata.is_file() || metadata.file_type().is_symlink())
                .unwrap_or(false)
            {
                snapshot.upsert(worktree_index_entry_for_existing_entry(
                    repo, &absolute, entry,
                )?)?;
            } else if !keep_missing {
                snapshot.remove_path(&entry.path)?;
            } else {
                continue;
            }
        } else if !keep_missing {
            snapshot.remove_path(&entry.path)?;
        }
    }
    Ok(snapshot)
}

pub(crate) fn worktree_diff_index_snapshot(repo: &GitRepo, index: &GitIndex) -> Result<GitIndex> {
    worktree_diff_index_snapshot_with_options(repo, index, false, false)
}

pub(crate) fn worktree_diff_index_snapshot_ignoring_gitlinks(
    repo: &GitRepo,
    index: &GitIndex,
) -> Result<GitIndex> {
    worktree_diff_index_snapshot_with_options(repo, index, false, true)
}

pub(crate) fn worktree_diff_index_snapshot_with_missing(
    repo: &GitRepo,
    index: &GitIndex,
    keep_missing: bool,
) -> Result<GitIndex> {
    worktree_diff_index_snapshot_with_options(repo, index, keep_missing, false)
}

fn worktree_diff_index_snapshot_with_options(
    repo: &GitRepo,
    index: &GitIndex,
    keep_missing: bool,
    ignore_gitlinks: bool,
) -> Result<GitIndex> {
    let mut snapshot = index.clone();
    let stage_options = WorktreeStageOptions::load(repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        if entry.skip_worktree() {
            continue;
        }
        if ignore_gitlinks && entry.mode == IndexMode::Gitlink {
            continue;
        }
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        if path_exists(&absolute) {
            if entry.mode == IndexMode::Gitlink {
                snapshot.upsert(worktree_gitlink_index_entry(entry, &absolute)?)?;
            } else if fs::symlink_metadata(&absolute)
                .map(|metadata| metadata.is_file() || metadata.file_type().is_symlink())
                .unwrap_or(false)
            {
                match worktree_diff_index_entry_for_existing_entry(
                    repo,
                    &store,
                    index,
                    &stage_options,
                    &absolute,
                    entry,
                ) {
                    Ok(worktree_entry) => snapshot.upsert(worktree_entry)?,
                    Err(error) if should_soften_diff_worktree_encoding_error(&error) => {
                        emit_softened_diff_worktree_encoding_error(&entry.path, &error);
                        snapshot.upsert(entry.clone())?;
                    }
                    Err(error) => return Err(error),
                }
            } else if !keep_missing {
                snapshot.remove_path(&entry.path)?;
            }
        } else if !keep_missing {
            snapshot.remove_path(&entry.path)?;
        }
    }
    Ok(snapshot)
}

pub(crate) fn worktree_stat_dirty_diff_entries(
    repo: &GitRepo,
    index: &GitIndex,
) -> Result<Vec<IndexDiffEntry>> {
    let mut entries = Vec::new();
    let stat_options = IndexStatOptions::from_config(&read_config_entries(repo)?)?;
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        let Ok(metadata) = fs::symlink_metadata(&absolute) else {
            continue;
        };
        if entry.mode == IndexMode::Gitlink
            || !(metadata.is_file() || metadata.file_type().is_symlink())
            || index_entry_stat_matches_with_options(&metadata, entry, stat_options)
        {
            continue;
        }
        entries.push(IndexDiffEntry {
            status: IndexDiffStatus::Modified,
            path: entry.path.clone(),
            old_path: None,
            similarity: None,
        });
    }
    Ok(entries)
}

fn next_index_position_after_path(index: &GitIndex, path: &[u8]) -> usize {
    index
        .entries()
        .partition_point(|entry| entry.path.as_slice() <= path)
}

fn worktree_gitlink_index_entry(entry: &IndexEntry, path: &std::path::Path) -> Result<IndexEntry> {
    let state = submodule_head_state(path, &entry.id, false)
        .ok_or_else(|| CliError::Message(format!("not a git repository: {}", path.display())))?;
    Ok(IndexEntry::new(
        entry.path.to_vec(),
        state.id,
        IndexMode::Gitlink,
        0,
    )?)
}

pub(crate) fn worktree_index_entry(repo: &GitRepo, path: &std::path::Path) -> Result<IndexEntry> {
    let metadata = fs::symlink_metadata(path)?;
    let relative = repo_relative_path(&repo.root, path)?;
    let (id, mode, size) = if metadata.file_type().is_symlink() {
        let content = read_symlink_content(path)?;
        (
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content),
            IndexMode::Symlink,
            content.len(),
        )
    } else if metadata.is_file() {
        (
            hash_worktree_file_blob(path, metadata.len())?,
            index_mode_for_worktree_metadata(repo, &metadata)?,
            worktree_file_size_usize(metadata.len())?,
        )
    } else {
        return Err(CliError::Message(format!(
            "{} is not a file",
            path.display()
        )));
    };
    let mut entry = IndexEntry::new(relative, id, mode, size.min(u32::MAX as usize) as u32)?;
    apply_index_entry_metadata(&mut entry, &metadata);
    Ok(entry)
}

fn worktree_index_entry_for_existing_entry(
    repo: &GitRepo,
    path: &std::path::Path,
    entry: &IndexEntry,
) -> Result<IndexEntry> {
    let metadata = fs::symlink_metadata(path)?;
    if entry.mode == IndexMode::Symlink
        && metadata.is_file()
        && !metadata.file_type().is_symlink()
        && !repo_symlinks_enabled(repo)?
    {
        let content = fs::read(path)?;
        let mut worktree_entry = IndexEntry::new(
            entry.path.clone(),
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content),
            IndexMode::Symlink,
            content.len().min(u32::MAX as usize) as u32,
        )?;
        apply_index_entry_metadata(&mut worktree_entry, &metadata);
        return Ok(worktree_entry);
    }
    worktree_index_entry(repo, path)
}

fn worktree_diff_index_entry_for_existing_entry(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    stage_options: &WorktreeStageOptions,
    path: &std::path::Path,
    entry: &IndexEntry,
) -> Result<IndexEntry> {
    let metadata = fs::symlink_metadata(path)?;
    if entry.mode == IndexMode::Symlink
        && metadata.is_file()
        && !metadata.file_type().is_symlink()
        && !repo_symlinks_enabled(repo)?
    {
        let content = fs::read(path)?;
        let mut worktree_entry = IndexEntry::new(
            entry.path.clone(),
            hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content),
            IndexMode::Symlink,
            content.len().min(u32::MAX as usize) as u32,
        )?;
        apply_index_entry_metadata(&mut worktree_entry, &metadata);
        return Ok(worktree_entry);
    }
    if metadata.file_type().is_symlink() {
        return worktree_index_entry_for_existing_entry(repo, path, entry);
    }
    let relative = repo_relative_path(&repo.root, path)?;
    let content = clean_worktree_content_for_comparison_against_index(
        repo,
        store,
        index,
        &relative,
        fs::read(path)?,
    )?;
    let mut worktree_entry = IndexEntry::new(
        entry.path.clone(),
        hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content),
        stage_options.index_mode_for_metadata(&metadata),
        content.len().min(u32::MAX as usize) as u32,
    )?;
    apply_index_entry_metadata(&mut worktree_entry, &metadata);
    Ok(worktree_entry)
}

pub(crate) fn collect_add_files(
    root: &std::path::Path,
    path: &std::path::Path,
    ignore: &GitIgnore,
    force: bool,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    collect_add_files_inner(root, path, ignore, force, files, false)
}

pub(crate) fn collect_add_files_ignore_errors(
    root: &std::path::Path,
    path: &std::path::Path,
    ignore: &GitIgnore,
    force: bool,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    collect_add_files_inner(root, path, ignore, force, files, true)
}

fn collect_add_files_inner(
    root: &std::path::Path,
    path: &std::path::Path,
    ignore: &GitIgnore,
    force: bool,
    files: &mut Vec<PathBuf>,
    ignore_errors: bool,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let relative = repo_relative_path(root, path)?;
    if !force && ignore.is_ignored(&relative, metadata.is_dir()) {
        return Ok(());
    }
    if metadata.is_dir() {
        if canonical_or_absolute(path.to_path_buf()) != canonical_or_absolute(root.to_path_buf())
            && exact_repo_at(path).is_some()
        {
            files.push(path.to_path_buf());
            return Ok(());
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) if ignore_errors => {
                warn_could_not_open_directory(&relative, &error);
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if ignore_errors => {
                    warn_could_not_open_directory(&relative, &error);
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if entry.file_name() == ".git" {
                continue;
            }
            collect_add_files_inner(root, &entry.path(), ignore, force, files, ignore_errors)?;
        }
    } else if metadata.is_file() || metadata.file_type().is_symlink() {
        files.push(path.to_path_buf());
    }
    Ok(())
}

fn warn_could_not_open_directory(relative: &[u8], error: &std::io::Error) {
    let mut display = String::from_utf8_lossy(relative).into_owned();
    if !display.ends_with('/') {
        display.push('/');
    }
    let message = match error.kind() {
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_owned(),
        _ => error.to_string(),
    };
    eprintln!("warning: could not open directory '{display}': {message}");
}

pub(crate) fn stage_file(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &std::path::Path,
) -> Result<()> {
    let index_mtime = repo_index_mtime(repo)?;
    let stage_options = WorktreeStageOptions::load(repo)?;
    stage_file_with_mode_and_index_mtime_and_options(
        repo,
        store,
        index,
        path,
        None,
        index_mtime,
        &stage_options,
    )
}

pub(crate) fn bulk_checkin_candidate(
    repo: &GitRepo,
    stage_options: &WorktreeStageOptions,
    path: &Path,
    relative: &[u8],
    threshold: u64,
) -> Result<Option<BulkCheckinCandidate>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.len() <= threshold
        || stage_options.needs_content_conversion(repo, relative)?
    {
        return Ok(None);
    }
    Ok(Some(BulkCheckinCandidate {
        path: path.to_path_buf(),
        relative: relative.to_vec(),
        size: metadata.len(),
    }))
}

pub(crate) fn stage_bulk_checkin_candidates(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    candidates: &[BulkCheckinCandidate],
    stage_options: &WorktreeStageOptions,
) -> Result<()> {
    if candidates.is_empty() {
        return Ok(());
    }
    if let [candidate] = candidates {
        return stage_single_bulk_checkin_candidate(repo, store, index, candidate, stage_options);
    }
    let packed = PackedObjectStore::new(repo.objects_dir.clone(), index.hash_algorithm());
    let mut objects = Vec::with_capacity(candidates.len());
    let mut seen = HashSet::with_capacity(candidates.len());
    for candidate in candidates {
        let metadata = fs::symlink_metadata(&candidate.path)?;
        if !metadata.is_file() || metadata.len() != candidate.size {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "{}: file changed while adding it to the index",
                    candidate.path.display()
                ),
            });
        }
        let id = hash_bulk_checkin_blob(index.hash_algorithm(), &candidate.path, candidate.size)?;
        stage_bulk_checkin_index_entry(
            index,
            candidate,
            &metadata,
            id.clone(),
            stage_options.index_mode_for_metadata(&metadata),
        )?;
        if !seen.insert(id.clone()) {
            continue;
        }
        if packed.contains_object(&id)? {
            remove_file_if_exists(&store.loose_object_path(&id)?)?;
            continue;
        }
        if store.loose_object_path(&id)?.is_file() {
            continue;
        }
        objects.push(BulkCheckinObject {
            path: candidate.path.clone(),
            id,
            size: candidate.size,
        });
    }
    if objects.is_empty() {
        return Ok(());
    }

    let compression_level = bulk_checkin_compression_level(repo)?;
    let pack_size_limit = bulk_checkin_pack_size_limit(repo)?;
    let groups = bulk_checkin_groups(&objects, pack_size_limit);
    let pack_dir = repo.objects_dir.join("pack");
    fs::create_dir_all(&pack_dir)?;
    for group in groups {
        write_bulk_checkin_pack(index.hash_algorithm(), &pack_dir, group, compression_level)?;
    }
    Ok(())
}

fn stage_single_bulk_checkin_candidate(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    candidate: &BulkCheckinCandidate,
    stage_options: &WorktreeStageOptions,
) -> Result<()> {
    let metadata = fs::symlink_metadata(&candidate.path)?;
    if !metadata.is_file() || metadata.len() != candidate.size {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "{}: file changed while adding it to the index",
                candidate.path.display()
            ),
        });
    }
    let algorithm = index.hash_algorithm();
    let pack_dir = repo.objects_dir.join("pack");
    fs::create_dir_all(&pack_dir)?;
    let temp_pack = unique_temp_sibling(&pack_dir.join("bulk-checkin.pack"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_pack)?;
        let (indexed, id) = write_single_undeltified_blob_pack_with_options(
            algorithm,
            candidate.size,
            bulk_checkin_compression_level(repo)?,
            |out| {
                let mut input = fs::File::open(&candidate.path)?;
                io::copy(&mut input, out)?;
                Ok(())
            },
            &mut file,
        )?;
        file.flush()?;
        stage_bulk_checkin_index_entry(
            index,
            candidate,
            &metadata,
            id.clone(),
            stage_options.index_mode_for_metadata(&metadata),
        )?;
        let packed = PackedObjectStore::new(repo.objects_dir.clone(), algorithm);
        if packed.contains_object(&id)? || store.loose_object_path(&id)?.is_file() {
            remove_file_if_exists(&temp_pack)?;
            return Ok(());
        }
        let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
        let pack_path = pack_dir.join(format!("{pack_name}.pack"));
        if pack_path.exists() {
            remove_file_if_exists(&temp_pack)?;
        } else {
            fs::rename(&temp_pack, &pack_path)?;
        }
        write_content_addressed_file(&pack_dir.join(format!("{pack_name}.idx")), &indexed.index)?;
        write_content_addressed_file(
            &pack_dir.join(format!("{pack_name}.rev")),
            &indexed.reverse_index,
        )?;
        Ok::<_, CliError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result
}

fn hash_bulk_checkin_blob(algorithm: GitHashAlgorithm, path: &Path, size: u64) -> Result<ObjectId> {
    let content_len = usize::try_from(size).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("{} is too large to index", path.display()),
    })?;
    let mut hasher = GitObjectHash::new(algorithm);
    hasher.update_object_header(GitObjectKind::Blob, content_len);
    let mut file = fs::File::open(path)?;
    let mut buffer = [0_u8; BULK_CHECKIN_HASH_BUFFER_BYTES];
    let mut read = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        read = read.saturating_add(count as u64);
    }
    if read != size {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "{}: file changed while adding it to the index",
                path.display()
            ),
        });
    }
    Ok(hasher.finalize())
}

fn stage_bulk_checkin_index_entry(
    index: &mut GitIndex,
    candidate: &BulkCheckinCandidate,
    metadata: &fs::Metadata,
    id: ObjectId,
    mode: IndexMode,
) -> Result<()> {
    if let Some(existing) = find_index_entry(index, &candidate.relative)
        && existing.id == id
    {
        let mut entry = existing.clone();
        entry.set_mode(mode);
        apply_index_entry_metadata(&mut entry, metadata);
        index.upsert(entry)?;
        return Ok(());
    }
    remove_index_path_dir_conflicts(index, &candidate.relative)?;
    let mut entry = IndexEntry::new(
        candidate.relative.clone(),
        id,
        mode,
        candidate.size.min(u32::MAX as u64) as u32,
    )?;
    apply_index_entry_metadata(&mut entry, metadata);
    index.upsert(entry)?;
    Ok(())
}

fn bulk_checkin_groups(
    objects: &[BulkCheckinObject],
    pack_size_limit: Option<u64>,
) -> Vec<&[BulkCheckinObject]> {
    let Some(limit) = pack_size_limit.filter(|limit| *limit > 0) else {
        return vec![objects];
    };
    let mut groups = Vec::new();
    let mut start = 0usize;
    let mut bytes = 0u64;
    for (index, object) in objects.iter().enumerate() {
        if index > start && bytes.saturating_add(object.size) > limit {
            groups.push(&objects[start..index]);
            start = index;
            bytes = 0;
        }
        bytes = bytes.saturating_add(object.size);
    }
    groups.push(&objects[start..]);
    groups
}

fn write_bulk_checkin_pack(
    algorithm: GitHashAlgorithm,
    pack_dir: &Path,
    objects: &[BulkCheckinObject],
    compression_level: u32,
) -> Result<()> {
    let sources = objects
        .iter()
        .map(|object| PackBlobSource {
            id: object.id.clone(),
            size: object.size,
        })
        .collect::<Vec<_>>();
    let temp_pack = unique_temp_sibling(&pack_dir.join("bulk-checkin.pack"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_pack)?;
        let mut source_index = 0usize;
        let indexed = write_undeltified_blob_pack_with_options(
            algorithm,
            &sources,
            compression_level,
            |source, out| {
                let object = objects.get(source_index).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing bulk-checkin source")
                })?;
                source_index += 1;
                if object.id != source.id {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "bulk-checkin source order changed",
                    ));
                }
                let mut input = fs::File::open(&object.path)?;
                io::copy(&mut input, out)?;
                Ok(())
            },
            &mut file,
        )?;
        file.flush()?;
        let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
        let pack_path = pack_dir.join(format!("{pack_name}.pack"));
        if pack_path.exists() {
            remove_file_if_exists(&temp_pack)?;
        } else {
            fs::rename(&temp_pack, &pack_path)?;
        }
        write_content_addressed_file(&pack_dir.join(format!("{pack_name}.idx")), &indexed.index)?;
        write_content_addressed_file(
            &pack_dir.join(format!("{pack_name}.rev")),
            &indexed.reverse_index,
        )?;
        Ok::<_, CliError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result
}

pub(crate) fn stage_intent_to_add_file(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &std::path::Path,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let relative = canonical_index_relative_path(
        index,
        repo_relative_path(&repo.root, path)?,
        WorktreeStageOptions::load(repo)?.ignore_case(),
    );
    if !metadata.is_file() && !metadata.file_type().is_symlink() {
        return Err(CliError::Message(format!(
            "{} is not a file",
            path.display()
        )));
    }
    let mode = if metadata.file_type().is_symlink() {
        IndexMode::Symlink
    } else {
        WorktreeStageOptions::load(repo)?.index_mode_for_metadata(&metadata)
    };
    let id = store.write_object(GitObjectKind::Blob, &[])?;
    let mut entry = IndexEntry::new(relative, id, mode, 0)?;
    entry.set_intent_to_add(true);
    remove_index_path_dir_conflicts(index, &entry.path)?;
    index.upsert(entry)?;
    Ok(())
}

pub(crate) fn stage_file_with_mode_and_index_mtime_and_options(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &std::path::Path,
    mode_override: Option<IndexMode>,
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
) -> Result<()> {
    stage_file_with_mode_and_index_mtime_options_and_trace(
        repo,
        store,
        index,
        path,
        mode_override,
        index_mtime,
        stage_options,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn stage_file_with_trace(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &std::path::Path,
    mode_override: Option<IndexMode>,
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
    trace: &mut StageFilesTrace,
) -> Result<()> {
    stage_file_with_mode_and_index_mtime_options_and_trace(
        repo,
        store,
        index,
        path,
        mode_override,
        index_mtime,
        stage_options,
        Some(trace),
    )
}

pub(crate) fn try_stage_regular_files_parallel(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    files: &[(PathBuf, Vec<u8>)],
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
    trace: &mut StageFilesTrace,
) -> Result<bool> {
    if files.len() < PARALLEL_STAGE_REGULAR_MIN_FILES {
        return Ok(false);
    }
    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(PARALLEL_STAGE_REGULAR_MAX_WORKERS);
    if workers <= 1 {
        return Ok(false);
    }

    let metadata_started = trace.started();
    let mut candidates = Vec::with_capacity(files.len());
    for (path, relative) in files {
        let metadata = fs::symlink_metadata(path)?;
        let file_type = metadata.file_type();
        if !metadata.is_file() || file_type.is_symlink() {
            return Ok(false);
        }
        if stage_options.needs_content_conversion(repo, relative)? {
            return Ok(false);
        }
        if let Some(existing) = find_index_entry(index, relative) {
            if index_mtime.is_some_and(|mtime| {
                index_entry_stat_match_is_safe(
                    &metadata,
                    existing,
                    mtime,
                    stage_options.stat_options(),
                )
            }) {
                return Ok(false);
            }
            return Ok(false);
        }
        if index.entry(relative, 1).is_some()
            || index.entry(relative, 2).is_some()
            || index.entry(relative, 3).is_some()
        {
            return Ok(false);
        }
        let mode = stage_options.index_mode_for_metadata(&metadata);
        let size = worktree_file_size_usize(metadata.len())?;
        candidates.push(ParallelStageRegularCandidate {
            path: path.clone(),
            relative: relative.clone(),
            metadata,
            mode,
            size,
        });
    }
    trace.files += candidates.len() as u64;
    trace.regular_files += candidates.len() as u64;
    trace.streamed_files += candidates.len() as u64;
    trace.record_metadata(metadata_started);

    let chunk_len = candidates.len().div_ceil(workers).max(1);
    let object_started = trace.started();
    let staged_chunks = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in candidates.chunks(chunk_len) {
            let store = store.clone();
            handles.push(
                scope.spawn(move || -> Result<Vec<ParallelStagedRegularFile>> {
                    let mut staged = Vec::with_capacity(chunk.len());
                    for candidate in chunk {
                        let id = store.write_streamed_blob_content(candidate.size, |writer| {
                            let mut file = fs::File::open(&candidate.path)?;
                            io::copy(&mut file, writer)?;
                            Ok(())
                        })?;
                        staged.push(ParallelStagedRegularFile {
                            relative: candidate.relative.clone(),
                            metadata: candidate.metadata.clone(),
                            id,
                            mode: candidate.mode,
                            size: candidate.size,
                        });
                    }
                    Ok(staged)
                }),
            );
        }
        let mut staged_chunks = Vec::with_capacity(handles.len());
        for handle in handles {
            let staged = handle
                .join()
                .map_err(|_| CliError::Message("parallel add worker panicked".into()))??;
            staged_chunks.push(staged);
        }
        Ok::<_, CliError>(staged_chunks)
    })?;
    trace.record_object_write(object_started);

    let parent_cleanup_started = trace.started();
    for staged in staged_chunks.iter().flatten() {
        remove_index_parent_file_entries(index, &staged.relative)?;
    }
    trace.record_parent_cleanup(parent_cleanup_started);

    let upsert_started = trace.started();
    for staged in staged_chunks.into_iter().flatten() {
        let mut entry = IndexEntry::new(
            staged.relative,
            staged.id,
            staged.mode,
            staged.size.min(u32::MAX as usize) as u32,
        )?;
        apply_index_entry_metadata(&mut entry, &staged.metadata);
        index.upsert(entry)?;
    }
    trace.record_upsert(upsert_started);
    Ok(true)
}

struct ParallelStageRegularCandidate {
    path: PathBuf,
    relative: Vec<u8>,
    metadata: fs::Metadata,
    mode: IndexMode,
    size: usize,
}

struct ParallelStagedRegularFile {
    relative: Vec<u8>,
    metadata: fs::Metadata,
    id: ObjectId,
    mode: IndexMode,
    size: usize,
}

#[allow(clippy::too_many_arguments)]
fn stage_file_with_mode_and_index_mtime_options_and_trace(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &std::path::Path,
    mode_override: Option<IndexMode>,
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
    mut trace: Option<&mut StageFilesTrace>,
) -> Result<()> {
    if let Some(trace) = trace.as_deref_mut() {
        trace.files += 1;
    }
    let metadata_started = trace.as_ref().and_then(|trace| trace.started());
    let metadata = fs::symlink_metadata(path)?;
    if let Some(trace) = trace.as_deref_mut() {
        trace.record_metadata(metadata_started);
    }
    let relative = repo_relative_path(&repo.root, path)?;
    let relative = canonical_index_relative_path(index, relative, stage_options.ignore_case());
    let file_type = metadata.file_type();
    if metadata.is_dir()
        && canonical_or_absolute(path.to_path_buf()) != canonical_or_absolute(repo.root.clone())
        && let Some(nested_repo) = exact_repo_at(path)
    {
        if let Some(trace) = trace.as_deref_mut() {
            trace.gitlinks += 1;
        }
        let parent_algorithm = repo_object_format(repo)?;
        let nested_algorithm = repo_object_format(&nested_repo)?;
        if parent_algorithm != nested_algorithm {
            return Err(CliError::Stderr {
                code: 128,
                text: "error: cannot add a submodule of a different hash algorithm\n".to_owned(),
            });
        }
        let head = match RefStore::new(&nested_repo.git_dir, GitHashAlgorithm::Sha1).resolve("HEAD")
        {
            Ok(head) => head,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let display = String::from_utf8_lossy(&relative);
                return Err(CliError::Stderr {
                    code: 128,
                    text: format!(
                        "error: '{display}/' does not have a commit checked out\nfatal: adding files failed\n"
                    ),
                });
            }
            Err(error) => return Err(CliError::Io(error)),
        };
        index.remove_dir(&relative)?;
        index.upsert(IndexEntry::new(relative, head, IndexMode::Gitlink, 0)?)?;
        return Ok(());
    }
    let mut mode = if file_type.is_symlink() {
        if let Some(trace) = trace.as_deref_mut() {
            trace.symlinks += 1;
        }
        IndexMode::Symlink
    } else if metadata.is_file() {
        if let Some(trace) = trace.as_deref_mut() {
            trace.regular_files += 1;
        }
        stage_options.index_mode_for_metadata(&metadata)
    } else {
        return Err(CliError::Message(format!(
            "{} is not a file",
            path.display()
        )));
    };
    if let Some(mode_override) = mode_override {
        mode = mode_override;
    }

    let unmerged_mode = index
        .entry(&relative, 2)
        .or_else(|| index.entry(&relative, 1))
        .or_else(|| index.entry(&relative, 3))
        .map(|entry| entry.mode);
    if let Some(existing_mode) = unmerged_mode {
        if let Some(resolve_undo) = resolve_undo_from_unmerged_entries(index, &relative) {
            index.upsert_resolve_undo(resolve_undo)?;
        }
        if let Some(trace) = trace.as_deref_mut() {
            trace.unmerged_replacements += 1;
        }
        if existing_mode == IndexMode::Executable && !stage_options.filemode_enabled() {
            mode = IndexMode::Executable;
        } else if existing_mode == IndexMode::Symlink && !stage_options.symlinks_enabled() {
            mode = IndexMode::Symlink;
        }
        index.remove_path(&relative)?;
    }
    if mode_override.is_none()
        && let Some(existing_mode) = find_index_entry(index, &relative).map(|entry| entry.mode)
    {
        if existing_mode == IndexMode::Executable && !stage_options.filemode_enabled() {
            mode = IndexMode::Executable;
        } else if existing_mode == IndexMode::Symlink && !stage_options.symlinks_enabled() {
            mode = IndexMode::Symlink;
        }
    }
    if mode_override.is_none()
        && matches!(mode, IndexMode::File | IndexMode::Executable)
        && let Some(existing) = find_index_entry(index, &relative)
        && existing.mode == mode
        && index_mtime.is_some_and(|mtime| {
            index_entry_stat_match_is_safe(&metadata, existing, mtime, stage_options.stat_options())
        })
    {
        if let Some(trace) = trace.as_deref_mut() {
            trace.stat_safe += 1;
        }
        return Ok(());
    }

    if file_type.is_symlink() {
        let read_started = trace.as_ref().and_then(|trace| trace.started());
        let content = read_symlink_content(path)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.record_read(read_started);
        }
        let object_started = trace.as_ref().and_then(|trace| trace.started());
        stage_resolved_content(store, index, relative, content, mode, &metadata)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.record_object_write(object_started);
        }
        return Ok(());
    }

    if matches!(mode, IndexMode::File | IndexMode::Executable)
        && stage_options.needs_content_conversion(repo, &relative)?
    {
        if let Some(trace) = trace.as_deref_mut() {
            trace.converted_files += 1;
        }
        let read_started = trace.as_ref().and_then(|trace| trace.started());
        let content = stage_options.clean_staged_worktree_content(
            repo,
            store,
            index,
            &relative,
            fs::read(path)?,
        )?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.record_read(read_started);
        }
        let object_started = trace.as_ref().and_then(|trace| trace.started());
        stage_resolved_content(store, index, relative, content, mode, &metadata)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.record_object_write(object_started);
        }
        return Ok(());
    }

    let size = worktree_file_size_usize(metadata.len())?;
    let existing_entry = find_index_entry(index, &relative).cloned();
    if size <= SMALL_WORKTREE_BLOB_READ_BYTES && existing_entry.is_some() {
        if let Some(trace) = trace.as_deref_mut() {
            trace.small_existing_files += 1;
        }
        let read_started = trace.as_ref().and_then(|trace| trace.started());
        let content = fs::read(path)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.record_read(read_started);
        }
        let object_started = trace.as_ref().and_then(|trace| trace.started());
        let id = store.write_object(GitObjectKind::Blob, &content)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.record_object_write(object_started);
        }
        if let Some(existing) = existing_entry.as_ref()
            && id == existing.id
        {
            let mut entry = existing.clone();
            entry.set_mode(mode);
            apply_index_entry_metadata(&mut entry, &metadata);
            let upsert_started = trace.as_ref().and_then(|trace| trace.started());
            index.upsert(entry)?;
            if let Some(trace) = trace.as_deref_mut() {
                trace.unchanged_id_refreshes += 1;
                trace.record_upsert(upsert_started);
            }
            return Ok(());
        }
        let mut entry = IndexEntry::new(
            relative,
            id,
            mode,
            content.len().min(u32::MAX as usize) as u32,
        )?;
        apply_index_entry_metadata(&mut entry, &metadata);
        let upsert_started = trace.as_ref().and_then(|trace| trace.started());
        index.upsert(entry)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.record_upsert(upsert_started);
        }
        return Ok(());
    }
    if let Some(trace) = trace.as_deref_mut() {
        trace.streamed_files += 1;
    }
    let object_started = trace.as_ref().and_then(|trace| trace.started());
    let id = store.write_streamed_blob_content(size, |writer| {
        let mut file = fs::File::open(path)?;
        io::copy(&mut file, writer)?;
        Ok(())
    })?;
    if let Some(trace) = trace.as_deref_mut() {
        trace.record_object_write(object_started);
    }
    if let Some(existing) = existing_entry.as_ref()
        && id == existing.id
    {
        let mut entry = existing.clone();
        entry.set_mode(mode);
        apply_index_entry_metadata(&mut entry, &metadata);
        let upsert_started = trace.as_ref().and_then(|trace| trace.started());
        index.upsert(entry)?;
        if let Some(trace) = trace.as_deref_mut() {
            trace.unchanged_id_refreshes += 1;
            trace.record_upsert(upsert_started);
        }
        return Ok(());
    }

    let parent_cleanup_started = trace.as_ref().and_then(|trace| trace.started());
    remove_index_path_dir_conflicts(index, &relative)?;
    if let Some(trace) = trace.as_deref_mut() {
        trace.record_parent_cleanup(parent_cleanup_started);
    }
    let mut entry = IndexEntry::new(relative, id, mode, size.min(u32::MAX as usize) as u32)?;
    apply_index_entry_metadata(&mut entry, &metadata);
    let upsert_started = trace.as_ref().and_then(|trace| trace.started());
    index.upsert(entry)?;
    if let Some(trace) = trace.as_deref_mut() {
        trace.record_upsert(upsert_started);
    }
    Ok(())
}

pub(crate) fn remove_index_parent_file_entries(index: &mut GitIndex, path: &[u8]) -> Result<()> {
    for (idx, byte) in path.iter().enumerate() {
        if *byte == b'/' {
            index.remove_path(&path[..idx])?;
        }
    }
    Ok(())
}

pub(crate) fn resolve_undo_from_unmerged_entries(
    index: &GitIndex,
    path: &[u8],
) -> Option<zmin_git_core::ResolveUndoEntry> {
    let mut stages = [None, None, None];
    let mut found = false;
    for stage in 1..=3 {
        let Some(entry) = index.entry(path, stage) else {
            continue;
        };
        found = true;
        stages[(stage - 1) as usize] = Some(zmin_git_core::ResolveUndoStage {
            mode: entry.mode,
            id: entry.id.clone(),
        });
    }
    found.then(|| zmin_git_core::ResolveUndoEntry {
        path: path.to_vec(),
        stages,
    })
}

pub(crate) fn repo_object_format(repo: &GitRepo) -> Result<GitHashAlgorithm> {
    let Some(entry) = read_validated_repository_format_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "extensions"
                && entry.subsection.is_empty()
                && entry.key == "objectformat"
        })
    else {
        return Ok(GitHashAlgorithm::Sha1);
    };
    match entry.value.as_str() {
        "sha1" => Ok(GitHashAlgorithm::Sha1),
        "sha256" => Ok(GitHashAlgorithm::Sha256),
        value => Err(CliError::Stderr {
            code: 128,
            text: format!(
                "error: invalid value for 'extensions.objectformat': '{value}'\nfatal: bad config line {} in file .git/config\n",
                entry.line.expect("local config entries carry line numbers")
            ),
        }),
    }
}

fn index_mode_for_worktree_metadata(repo: &GitRepo, metadata: &fs::Metadata) -> Result<IndexMode> {
    if repo_filemode_enabled(repo)? {
        Ok(index_mode_for_metadata(metadata))
    } else {
        Ok(IndexMode::File)
    }
}

fn repo_filemode_enabled(repo: &GitRepo) -> Result<bool> {
    if let Some(value) = global_command_config_value("core", "filemode") {
        return parse_git_bool(&value).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{value}'"),
        });
    }
    if let Some(entry) = read_local_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "core" && entry.subsection.is_empty() && entry.key == "filemode"
        })
    {
        return entry.bool_value().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{}'", entry.value),
        });
    }
    Ok(default_repo_filemode_enabled())
}

fn repo_symlinks_enabled(repo: &GitRepo) -> Result<bool> {
    if let Some(value) = global_command_config_value("core", "symlinks") {
        return parse_git_bool(&value).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{value}'"),
        });
    }
    if let Some(entry) = read_local_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "core" && entry.subsection.is_empty() && entry.key == "symlinks"
        })
    {
        return entry.bool_value().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{}'", entry.value),
        });
    }
    Ok(default_repo_symlinks_enabled())
}

#[cfg(unix)]
fn default_repo_filemode_enabled() -> bool {
    true
}

#[cfg(not(unix))]
fn default_repo_filemode_enabled() -> bool {
    false
}

#[cfg(unix)]
fn default_repo_symlinks_enabled() -> bool {
    true
}

#[cfg(not(unix))]
fn default_repo_symlinks_enabled() -> bool {
    false
}

fn stage_resolved_content(
    store: &LooseObjectStore,
    index: &mut GitIndex,
    relative: Vec<u8>,
    content: Vec<u8>,
    mode: IndexMode,
    metadata: &fs::Metadata,
) -> Result<()> {
    remove_index_path_dir_conflicts(index, &relative)?;
    if let Some(existing) = find_index_entry(index, &relative) {
        let id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content);
        if id == existing.id {
            let mut entry = existing.clone();
            entry.set_mode(mode);
            apply_index_entry_metadata(&mut entry, metadata);
            index.upsert(entry)?;
            return Ok(());
        }
        let mut entry = IndexEntry::new(
            relative,
            store.write_object(GitObjectKind::Blob, &content)?,
            mode,
            content.len().min(u32::MAX as usize) as u32,
        )?;
        apply_index_entry_metadata(&mut entry, metadata);
        index.upsert(entry)?;
        return Ok(());
    }
    let id = store.write_object(GitObjectKind::Blob, &content)?;
    let mut entry = IndexEntry::new(
        relative,
        id,
        mode,
        content.len().min(u32::MAX as usize) as u32,
    )?;
    apply_index_entry_metadata(&mut entry, metadata);
    index.upsert(entry)?;
    Ok(())
}

fn remove_index_path_dir_conflicts(index: &mut GitIndex, path: &[u8]) -> Result<()> {
    remove_index_parent_file_entries(index, path)?;
    index.remove_dir(path)?;
    Ok(())
}

fn canonical_index_relative_path(
    index: &GitIndex,
    relative: Vec<u8>,
    ignore_case: bool,
) -> Vec<u8> {
    if !ignore_case {
        return relative;
    }
    find_index_entry_ignorecase(index, &relative)
        .map(|entry| entry.path.clone())
        .unwrap_or(relative)
}

fn find_index_entry_ignorecase<'a>(index: &'a GitIndex, path: &[u8]) -> Option<&'a IndexEntry> {
    find_index_entry(index, path).or_else(|| {
        index
            .entries()
            .iter()
            .find(|entry| entry.stage == 0 && path_eq_ignore_ascii_case(&entry.path, path))
    })
}

fn path_eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

fn hash_worktree_file_blob(path: &std::path::Path, size: u64) -> Result<ObjectId> {
    let mut file = fs::File::open(path)?;
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update_object_header(GitObjectKind::Blob, worktree_file_size_usize(size)?);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize())
}

fn hash_worktree_file_blob_detect_cr(
    path: &std::path::Path,
    size: u64,
) -> Result<(ObjectId, bool)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update_object_header(GitObjectKind::Blob, worktree_file_size_usize(size)?);
    let mut buffer = [0_u8; 64 * 1024];
    let mut has_cr = false;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        has_cr |= chunk.contains(&b'\r');
        hasher.update(chunk);
    }
    Ok((hasher.finalize(), has_cr))
}

fn worktree_file_size_usize(size: u64) -> Result<usize> {
    usize::try_from(size)
        .map_err(|_| CliError::Message("worktree file is too large for this platform".to_string()))
}

#[cfg(unix)]
pub(crate) fn read_symlink_content(path: &std::path::Path) -> Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;

    Ok(fs::read_link(path)?.as_os_str().as_bytes().to_vec())
}

#[cfg(not(unix))]
pub(crate) fn read_symlink_content(path: &std::path::Path) -> Result<Vec<u8>> {
    Ok(fs::read(path)?)
}

pub(crate) fn read_head_index(repo: &GitRepo) -> Result<GitIndex> {
    let runtime = CliPrimitiveRuntime::new_default(repo);
    read_head_index_from_primitive_stores(
        runtime.refs_store_adapter(),
        runtime.object_store_adapter(),
    )
}

pub(crate) fn read_head_index_with_caches(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
) -> Result<GitIndex> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head = match refs.resolve("HEAD") {
        Ok(head) => head,
        Err(_) => return Ok(GitIndex::new()),
    };
    let links = commit_cache.read_commit_links(&head)?;
    Ok(tree_cache.read_tree_to_index(&links.tree)?)
}

pub(crate) fn read_head_tree_id_from_primitive_stores(
    refs: &dyn GitRefsStore,
    store: &dyn GitObjectStore,
) -> Result<Option<ObjectId>> {
    let head = match refs.read_ref(&"HEAD".to_owned()) {
        Ok(Some(id)) => id,
        Ok(None) => return Ok(None),
        Err(error) if is_not_found_ref_error(&error) => return Ok(None),
        Err(error) => {
            return Err(map_primitive_error(
                error,
                "read HEAD reference for status head tree",
            ));
        }
    };

    let head = parse_primitive_object_id(&head)?;
    let commit_cache = CommitObjectCache::new(store);
    Ok(Some(commit_cache.read_commit_links(&head)?.tree.clone()))
}

pub(crate) fn read_head_index_from_primitive_stores(
    refs: &dyn GitRefsStore,
    store: &dyn GitObjectStore,
) -> Result<GitIndex> {
    let head = match refs.read_ref(&"HEAD".to_owned()) {
        Ok(Some(raw_id)) => parse_primitive_object_id(&raw_id)?,
        Ok(None) => return Ok(GitIndex::new()),
        Err(error) if is_not_found_ref_error(&error) => return Ok(GitIndex::new()),
        Err(error) => {
            return Err(map_primitive_error(
                error,
                "read HEAD reference for worktree index",
            ));
        }
    };

    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let links = commit_cache.read_commit_links(&head)?;
    Ok(tree_cache.read_tree_to_index(&links.tree)?)
}

fn parse_primitive_object_id(raw_oid: &str) -> Result<ObjectId> {
    let algorithm = match raw_oid.len() {
        40 => GitHashAlgorithm::Sha1,
        64 => GitHashAlgorithm::Sha256,
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid object id '{raw_oid}' from primitive ref store"),
            });
        }
    };
    ObjectId::from_hex(algorithm, raw_oid).map_err(CliError::Io)
}

fn map_primitive_error(error: PrimitiveError, context: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("{context}: {error}"),
    }
}

fn is_not_found_ref_error(error: &PrimitiveError) -> bool {
    let details = error.to_string();
    details.contains("not found") || details.contains("no such file")
}

pub(crate) fn worktree_status(repo: &GitRepo, index: &GitIndex) -> Result<Vec<(Vec<u8>, char)>> {
    let mut statuses = Vec::new();
    let index_mtime = repo_index_mtime(repo)?;
    let stage_options = WorktreeStageOptions::load(repo)?;
    let mut trace = TrackedWorktreeTrace::new();
    let parallel_regular_scan = try_scan_tracked_regular_files_parallel(
        repo,
        index,
        &[],
        &HashSet::new(),
        index_mtime,
        &stage_options,
        ParallelTrackedScanPolicy::Status,
        false,
        &mut trace,
    )?;
    for (entry_position, entry) in index.entries().iter().enumerate() {
        if entry.stage != 0 {
            return Err(CliError::Message(
                "status cannot inspect an index with unresolved conflicts".into(),
            ));
        }
        trace.entries += 1;
        if entry.skip_worktree()
            && !path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path))
        {
            trace.skipped_worktree += 1;
            continue;
        }
        if let Some(scan) = parallel_regular_scan.as_ref()
            && matches!(entry.mode, IndexMode::File | IndexMode::Executable)
            && let Some(action) = scan.actions[entry_position].as_ref()
        {
            match action {
                ParallelTrackedRegularAction::Unchanged => continue,
                ParallelTrackedRegularAction::Deleted => {
                    trace.deleted += 1;
                    statuses.push((entry.path.to_vec(), 'D'));
                    continue;
                }
                ParallelTrackedRegularAction::RemovedDir => {
                    trace.removed_dirs += 1;
                    trace.modified += 1;
                    statuses.push((entry.path.to_vec(), 'M'));
                    continue;
                }
                ParallelTrackedRegularAction::ModeChanged => {
                    trace.modified += 1;
                    statuses.push((entry.path.to_vec(), 'M'));
                    continue;
                }
                ParallelTrackedRegularAction::Modified => {
                    trace.modified += 1;
                    statuses.push((entry.path.to_vec(), 'M'));
                    continue;
                }
                ParallelTrackedRegularAction::ContentHash(id) => {
                    if id != &entry.id {
                        trace.modified += 1;
                        statuses.push((entry.path.to_vec(), 'M'));
                    }
                    continue;
                }
                ParallelTrackedRegularAction::NeedsStage(_) => {}
            }
        }
        let path = worktree_path_for_index_entry(&repo.root, &entry.path);
        let metadata_started = trace.started();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                trace.record_metadata(metadata_started);
                trace.deleted += 1;
                statuses.push((entry.path.to_vec(), 'D'));
                continue;
            }
        };
        trace.record_metadata(metadata_started);
        let modified_started = trace.started();
        let modified = worktree_entry_modified_with_metadata(
            repo,
            &path,
            &metadata,
            entry,
            index_mtime,
            &stage_options,
            Some(&mut trace),
        )?;
        trace.record_stage_needed_check(modified_started);
        if modified {
            trace.modified += 1;
            statuses.push((entry.path.to_vec(), 'M'));
        }
    }
    trace.emit_with_label("status.worktree_status.detail");
    Ok(statuses)
}

#[cfg(unix)]
pub(crate) fn worktree_path_for_index_entry(root: &std::path::Path, path: &[u8]) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    root.join(std::path::Path::new(OsStr::from_bytes(path)))
}

#[cfg(not(unix))]
pub(crate) fn worktree_path_for_index_entry(root: &std::path::Path, path: &[u8]) -> PathBuf {
    root.join(String::from_utf8_lossy(path).as_ref())
}

pub(crate) fn worktree_entry_modified(
    repo: &GitRepo,
    path: &std::path::Path,
    entry: &IndexEntry,
) -> Result<bool> {
    let stage_options = WorktreeStageOptions::load(repo)?;
    worktree_entry_modified_with_index_mtime(repo, path, entry, None, &stage_options)
}

fn tracked_entry_needs_stage(
    path: &std::path::Path,
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
    mut trace: Option<&mut TrackedWorktreeTrace>,
) -> Result<bool> {
    match entry.mode {
        IndexMode::File | IndexMode::Executable => {
            if !metadata.is_file() || stage_options.index_mode_for_metadata(metadata) != entry.mode
            {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.mode_changed += 1;
                }
                return Ok(true);
            }
            if index_mtime.is_some_and(|mtime| {
                index_entry_stat_match_is_safe(metadata, entry, mtime, stage_options.stat_options())
            }) {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.stat_safe += 1;
                }
                return Ok(false);
            }
            if let Some(trace) = trace.as_deref_mut() {
                trace.stat_unsafe += 1;
            }
            Ok(true)
        }
        IndexMode::Symlink => {
            if let Some(trace) = trace.as_deref_mut() {
                trace.symlink_checks += 1;
            }
            symlink_entry_modified_with_metadata(
                path,
                metadata,
                entry,
                stage_options.symlinks_enabled(),
                stage_options.stat_options(),
            )
        }
        IndexMode::Gitlink => {
            if let Some(trace) = trace.as_deref_mut() {
                trace.gitlink_checks += 1;
            }
            if !metadata.is_dir() {
                return Ok(true);
            }
            Ok(submodule_head_state(path, &entry.id, false)
                .is_some_and(|state| state.id != entry.id))
        }
        IndexMode::Tree => Ok(false),
    }
}

fn worktree_entry_modified_with_index_mtime(
    repo: &GitRepo,
    path: &std::path::Path,
    entry: &IndexEntry,
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    worktree_entry_modified_with_metadata(
        repo,
        path,
        &metadata,
        entry,
        index_mtime,
        stage_options,
        None,
    )
}

fn worktree_entry_modified_with_metadata(
    repo: &GitRepo,
    path: &std::path::Path,
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    index_mtime: Option<IndexTimestamp>,
    stage_options: &WorktreeStageOptions,
    mut trace: Option<&mut TrackedWorktreeTrace>,
) -> Result<bool> {
    match entry.mode {
        IndexMode::File | IndexMode::Executable => {
            if !metadata.is_file() || stage_options.index_mode_for_metadata(&metadata) != entry.mode
            {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.mode_changed += 1;
                }
                return Ok(true);
            }
            if index_mtime.is_some_and(|mtime| {
                index_entry_stat_match_is_safe(
                    &metadata,
                    entry,
                    mtime,
                    stage_options.stat_options(),
                )
            }) {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.stat_safe += 1;
                }
                return Ok(false);
            }
            if index_entry_has_stat_cache(entry)
                && u32::try_from(metadata.len()).is_ok_and(|size| size != entry.size)
            {
                return Ok(true);
            }
            let comparison = stage_options.content_comparison(repo, &entry.path)?;
            if matches!(comparison, WorktreeContentComparison::RawIfNoCr) {
                let started = trace.as_ref().and_then(|trace| trace.started());
                let (raw_id, has_cr) = hash_worktree_file_blob_detect_cr(path, metadata.len())?;
                if let Some(trace) = trace.as_deref_mut() {
                    trace.content_hashes += 1;
                    trace.record_content_hash(started);
                }
                if !has_cr {
                    return Ok(raw_id != entry.id);
                }
            }
            if !matches!(comparison, WorktreeContentComparison::Raw) {
                let started = trace.as_ref().and_then(|trace| trace.started());
                let content = stage_options.clean_worktree_content_for_comparison(
                    repo,
                    &entry.path,
                    fs::read(path)?,
                )?;
                if let Some(trace) = trace.as_deref_mut() {
                    trace.converted_hashes += 1;
                    trace.record_conversion(started);
                }
                return Ok(
                    hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content) != entry.id,
                );
            }
            let started = trace.as_ref().and_then(|trace| trace.started());
            let modified = hash_worktree_file_blob(path, metadata.len())? != entry.id;
            if let Some(trace) = trace.as_deref_mut() {
                trace.content_hashes += 1;
                trace.record_content_hash(started);
            }
            Ok(modified)
        }
        IndexMode::Symlink => {
            if let Some(trace) = trace.as_deref_mut() {
                trace.symlink_checks += 1;
            }
            symlink_entry_modified_with_metadata(
                path,
                metadata,
                entry,
                stage_options.symlinks_enabled(),
                stage_options.stat_options(),
            )
        }
        IndexMode::Gitlink => {
            if let Some(trace) = trace.as_deref_mut() {
                trace.gitlink_checks += 1;
            }
            if !metadata.is_dir() {
                return Ok(true);
            }
            Ok(submodule_head_state(path, &entry.id, false)
                .is_some_and(|state| state.id != entry.id))
        }
        IndexMode::Tree => Ok(false),
    }
}

pub(crate) struct WorktreeStageOptions {
    content_rules: Mutex<Option<WorktreeContentRules>>,
    attribute_base_rules: Mutex<Option<WorktreeAttributeBaseRules>>,
    path_content_rules_cache: Mutex<HashMap<Vec<u8>, Arc<WorktreeContentRules>>>,
    emitted_attribute_warnings: Mutex<HashSet<String>>,
    core_autocrlf: CoreAutoCrlf,
    core_eol: CoreEol,
    core_safecrlf: CoreSafeCrlf,
    roundtrip_encodings: Option<HashSet<String>>,
    root_index_attributes: Option<GitAttributes>,
    ignore_case: bool,
    filemode_enabled: bool,
    symlinks_enabled: bool,
    stat_options: IndexStatOptions,
}

impl WorktreeStageOptions {
    pub(crate) fn load(repo: &GitRepo) -> Result<Self> {
        let entries = read_config_entries(repo)?;
        Ok(Self {
            content_rules: Mutex::new(None),
            attribute_base_rules: Mutex::new(None),
            path_content_rules_cache: Mutex::new(HashMap::new()),
            emitted_attribute_warnings: Mutex::new(HashSet::new()),
            core_autocrlf: core_autocrlf_from_config(&entries),
            core_eol: core_eol_from_config(&entries),
            core_safecrlf: core_safecrlf_from_config(&entries),
            roundtrip_encodings: core_check_roundtrip_encodings_from_config(&entries),
            root_index_attributes: None,
            ignore_case: core_ignorecase_from_config(&entries),
            filemode_enabled: repo_filemode_enabled(repo)?,
            symlinks_enabled: repo_symlinks_enabled(repo)?,
            stat_options: IndexStatOptions::from_config(&entries)?,
        })
    }

    pub(crate) fn load_for_index(
        repo: &GitRepo,
        store: &LooseObjectStore,
        index: &GitIndex,
    ) -> Result<Self> {
        let entries = read_config_entries(repo)?;
        Ok(Self {
            content_rules: Mutex::new(None),
            attribute_base_rules: Mutex::new(None),
            path_content_rules_cache: Mutex::new(HashMap::new()),
            emitted_attribute_warnings: Mutex::new(HashSet::new()),
            core_autocrlf: core_autocrlf_from_config(&entries),
            core_eol: core_eol_from_config(&entries),
            core_safecrlf: core_safecrlf_from_config(&entries),
            roundtrip_encodings: core_check_roundtrip_encodings_from_config(&entries),
            root_index_attributes: load_index_root_attributes(store, index)?,
            ignore_case: core_ignorecase_from_config(&entries),
            filemode_enabled: repo_filemode_enabled(repo)?,
            symlinks_enabled: repo_symlinks_enabled(repo)?,
            stat_options: IndexStatOptions::from_config(&entries)?,
        })
    }

    fn with_content_rules<T>(
        &self,
        repo: &GitRepo,
        apply: impl FnOnce(&WorktreeContentRules) -> Result<T>,
    ) -> Result<T> {
        let mut content_rules = self
            .content_rules
            .lock()
            .map_err(|_| CliError::Message("worktree content rules mutex poisoned".into()))?;
        if content_rules.is_none() {
            *content_rules = Some(WorktreeContentRules::load_with_config(
                repo,
                self.core_autocrlf,
                self.core_eol,
                self.core_safecrlf,
                self.roundtrip_encodings.clone(),
                self.root_index_attributes.clone(),
            )?);
        }
        apply(content_rules.as_ref().expect("content rules initialized"))
    }

    fn path_content_rules(
        &self,
        repo: &GitRepo,
        relative: &[u8],
    ) -> Result<Arc<WorktreeContentRules>> {
        let parent = relative
            .iter()
            .rposition(|byte| *byte == b'/')
            .map(|index| &relative[..index])
            .unwrap_or_default();
        if let Some(rules) = self
            .path_content_rules_cache
            .lock()
            .map_err(|_| CliError::Message("path content rules mutex poisoned".into()))?
            .get(parent)
            .cloned()
        {
            return Ok(rules);
        }
        let (mut attributes, info_attributes) = {
            let mut base_rules = self
                .attribute_base_rules
                .lock()
                .map_err(|_| CliError::Message("attribute base rules mutex poisoned".into()))?;
            if base_rules.is_none() {
                *base_rules = Some(load_worktree_attribute_base_rules(repo, self.ignore_case)?);
            }
            let base_rules = base_rules
                .as_ref()
                .expect("attribute base rules initialized");
            (
                base_rules.before_worktree.clone(),
                base_rules.after_worktree.clone(),
            )
        };
        load_attributes_for_relative_path(
            &mut attributes,
            &repo.root,
            relative,
            self.ignore_case,
            self.root_index_attributes.as_ref(),
        )?;
        attributes.append(info_attributes);
        self.emit_new_attribute_warnings(&attributes)?;
        let rules = Arc::new(WorktreeContentRules {
            attributes,
            core_autocrlf: self.core_autocrlf,
            core_eol: self.core_eol,
            core_safecrlf: self.core_safecrlf,
            roundtrip_encodings: self.roundtrip_encodings.clone(),
        });
        self.path_content_rules_cache
            .lock()
            .map_err(|_| CliError::Message("path content rules mutex poisoned".into()))?
            .insert(parent.to_vec(), rules.clone());
        Ok(rules)
    }

    fn emit_new_attribute_warnings(&self, attributes: &GitAttributes) -> Result<()> {
        let mut emitted = self
            .emitted_attribute_warnings
            .lock()
            .map_err(|_| CliError::Message("attribute warning mutex poisoned".into()))?;
        for warning in attributes.warnings() {
            if emitted.insert(warning.clone()) {
                eprintln!("{warning}");
            }
        }
        Ok(())
    }

    pub(crate) fn index_mode_for_metadata(&self, metadata: &fs::Metadata) -> IndexMode {
        if self.filemode_enabled {
            index_mode_for_metadata(metadata)
        } else {
            IndexMode::File
        }
    }

    pub(crate) fn filemode_enabled(&self) -> bool {
        self.filemode_enabled
    }

    pub(crate) fn symlinks_enabled(&self) -> bool {
        self.symlinks_enabled
    }

    pub(crate) fn stat_options(&self) -> IndexStatOptions {
        self.stat_options
    }

    pub(crate) fn ignore_case(&self) -> bool {
        self.ignore_case
    }

    pub(crate) fn needs_content_conversion(&self, repo: &GitRepo, relative: &[u8]) -> Result<bool> {
        Ok(self
            .path_content_rules(repo, relative)?
            .needs_content_conversion(relative))
    }

    fn content_comparison(
        &self,
        repo: &GitRepo,
        relative: &[u8],
    ) -> Result<WorktreeContentComparison> {
        let rules = self.path_content_rules(repo, relative)?;
        if !rules.needs_content_conversion(relative) {
            return Ok(WorktreeContentComparison::Raw);
        }
        if rules.can_use_raw_blob_hash_when_no_cr(relative) {
            return Ok(WorktreeContentComparison::RawIfNoCr);
        }
        Ok(WorktreeContentComparison::Converted)
    }

    pub(crate) fn clean_staged_worktree_content(
        &self,
        repo: &GitRepo,
        store: &LooseObjectStore,
        index: &GitIndex,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.path_content_rules(repo, relative)?
            .clean_staged_worktree_content(repo, store, index, relative, content)
    }

    pub(crate) fn renormalize_staged_worktree_content(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let rules = self.path_content_rules(repo, relative)?;
        rules.clean_worktree_content_inner(repo, None, relative, content, self.core_safecrlf, true)
    }

    fn clean_worktree_content(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.path_content_rules(repo, relative)?
            .clean_worktree_content(repo, relative, content)
    }

    fn clean_worktree_content_for_comparison(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.path_content_rules(repo, relative)?
            .clean_worktree_content_for_comparison(repo, relative, content)
    }

    fn smudge_checkout_content(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        self.with_content_rules(repo, |content_rules| {
            content_rules.smudge_checkout_content(relative, content)
        })
    }

    fn may_smudge_checkout_entries(&self, repo: &GitRepo) -> Result<bool> {
        self.with_content_rules(repo, |content_rules| {
            Ok(content_rules.may_smudge_checkout_entries())
        })
    }

    fn attributes(&self, repo: &GitRepo) -> Result<GitAttributes> {
        self.with_content_rules(repo, |content_rules| Ok(content_rules.attributes.clone()))
    }
}

struct WorktreeAttributeBaseRules {
    before_worktree: GitAttributes,
    after_worktree: GitAttributes,
}

#[derive(Clone)]
pub(crate) struct WorktreeContentRules {
    attributes: GitAttributes,
    core_autocrlf: CoreAutoCrlf,
    core_eol: CoreEol,
    core_safecrlf: CoreSafeCrlf,
    roundtrip_encodings: Option<HashSet<String>>,
}

impl WorktreeContentRules {
    pub(crate) fn load(repo: &GitRepo) -> Result<Self> {
        let entries = read_config_entries(repo)?;
        Self::load_with_config(
            repo,
            core_autocrlf_from_config(&entries),
            core_eol_from_config(&entries),
            core_safecrlf_from_config(&entries),
            core_check_roundtrip_encodings_from_config(&entries),
            None,
        )
    }

    fn load_with_config(
        repo: &GitRepo,
        core_autocrlf: CoreAutoCrlf,
        core_eol: CoreEol,
        core_safecrlf: CoreSafeCrlf,
        roundtrip_encodings: Option<HashSet<String>>,
        root_index_attributes: Option<GitAttributes>,
    ) -> Result<Self> {
        let attributes = load_repo_attributes(repo, root_index_attributes.as_ref())?;
        emit_attribute_warnings(&attributes);
        Ok(Self {
            attributes,
            core_autocrlf,
            core_eol,
            core_safecrlf,
            roundtrip_encodings,
        })
    }

    fn needs_content_conversion(&self, relative: &[u8]) -> bool {
        self.attributes.is_set(relative, "ident")
            || self
                .working_tree_encoding(relative)
                .ok()
                .flatten()
                .is_some()
            || self.crlf_action(relative) != CrlfAction::Binary
            || worktree_filter_name(&self.attributes, relative).is_some()
    }

    fn can_use_raw_blob_hash_when_no_cr(&self, relative: &[u8]) -> bool {
        !self.attributes.is_set(relative, "ident")
            && self
                .working_tree_encoding(relative)
                .ok()
                .flatten()
                .is_none()
            && worktree_filter_name(&self.attributes, relative).is_none()
            && self.crlf_action(relative) != CrlfAction::Binary
    }

    fn clean_staged_worktree_content(
        &self,
        repo: &GitRepo,
        store: &LooseObjectStore,
        index: &GitIndex,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.clean_worktree_content_inner(
            repo,
            Some((store, index)),
            relative,
            content,
            self.core_safecrlf,
            true,
        )
    }

    fn clean_worktree_content(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.clean_worktree_content_inner(repo, None, relative, content, CoreSafeCrlf::False, false)
    }

    fn clean_worktree_content_for_comparison(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.clean_worktree_content_inner(
            repo,
            None,
            relative,
            content,
            self.core_safecrlf.demote_for_worktree_compare(),
            false,
        )
    }

    fn clean_worktree_content_inner(
        &self,
        repo: &GitRepo,
        index_context: Option<(&LooseObjectStore, &GitIndex)>,
        relative: &[u8],
        content: Vec<u8>,
        safecrlf: CoreSafeCrlf,
        emit_warnings: bool,
    ) -> Result<Vec<u8>> {
        let content = if let Some(encoding) = self.working_tree_encoding_spec(relative)? {
            let content = decode_working_tree_encoding_content(relative, &encoding.kind, &content)?;
            self.maybe_check_working_tree_roundtrip_encoding(relative, &encoding, &content)?;
            content
        } else {
            content
        };
        let content = if self.attributes.is_set(relative, "ident") {
            apply_ident_clean(&content)
        } else {
            content
        };
        let content =
            self.clean_crlf_content(index_context, relative, content, safecrlf, emit_warnings)?;
        apply_worktree_filter(repo, &self.attributes, relative, "clean", &[], content)
    }

    fn clean_crlf_content(
        &self,
        index_context: Option<(&LooseObjectStore, &GitIndex)>,
        relative: &[u8],
        content: Vec<u8>,
        safecrlf: CoreSafeCrlf,
        emit_warnings: bool,
    ) -> Result<Vec<u8>> {
        let action = self.crlf_action(relative);
        if action == CrlfAction::Binary || content.is_empty() {
            return Ok(content);
        }
        let stats = CrlfStats::gather(&content);
        let mut convert_crlf_to_lf = stats.crlf > 0;
        if action.is_auto() {
            if stats.is_binary() {
                return Ok(content);
            }
            if let Some((store, index)) = index_context
                && index_has_crlf(store, index, relative)?
            {
                convert_crlf_to_lf = false;
            }
        }
        if emit_warnings {
            let mut new_stats = stats;
            if convert_crlf_to_lf {
                new_stats.lonelf += new_stats.crlf;
                new_stats.crlf = 0;
            }
            if will_convert_lf_to_crlf(&new_stats, action) {
                new_stats.crlf += new_stats.lonelf;
                new_stats.lonelf = 0;
            }
            enforce_safecrlf(relative, action, &stats, &new_stats, safecrlf)?;
            if safecrlf.emits_warning() {
                emit_crlf_roundtrip_warning(relative, action, &stats, &new_stats);
            }
        }
        if !convert_crlf_to_lf {
            return Ok(content);
        }
        Ok(match action {
            CrlfAction::AutoInput | CrlfAction::AutoCrlf => clean_auto_crlf_to_lf(&content),
            CrlfAction::TextInput | CrlfAction::TextCrlf => apply_eol_clean_to_lf(&content),
            CrlfAction::Binary => content,
        })
    }

    fn smudge_checkout_content(&self, relative: &[u8], content: &[u8]) -> Result<Option<Vec<u8>>> {
        let original = content;
        let mut content = content.to_vec();
        let action = self.crlf_action(relative);
        if action.output_crlf() && !content.is_empty() {
            let stats = CrlfStats::gather(&content);
            if stats.lonelf > 0
                && (!action.is_auto()
                    || (stats.lonecr == 0 && stats.crlf == 0 && !stats.is_binary()))
            {
                content = zmin_git_core::apply_eol_smudge_to_crlf(&content);
            }
        }
        if let Some(encoding) = self.working_tree_encoding_spec(relative)? {
            content = encode_working_tree_encoding_content(relative, &encoding.kind, &content)?;
        }
        if content != original {
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }

    fn may_smudge_checkout_entries(&self) -> bool {
        !self.attributes.is_empty()
            || self.core_autocrlf == CoreAutoCrlf::True
            || self.working_tree_encoding(b"").ok().flatten().is_some()
    }

    fn crlf_action(&self, relative: &[u8]) -> CrlfAction {
        crlf_action_for_path(
            &self.attributes,
            relative,
            self.core_autocrlf,
            self.core_eol,
        )
    }

    fn working_tree_encoding(&self, relative: &[u8]) -> Result<Option<WorkingTreeEncoding>> {
        Ok(self
            .working_tree_encoding_spec(relative)?
            .map(|encoding| encoding.kind))
    }

    fn working_tree_encoding_spec(
        &self,
        relative: &[u8],
    ) -> Result<Option<WorkingTreeEncodingSpec>> {
        let value = self
            .attributes
            .check(relative, &["working-tree-encoding".to_owned()])
            .into_iter()
            .next()
            .map(|(_, value)| value)
            .unwrap_or(AttributeValue::Unspecified);
        parse_working_tree_encoding_value(relative, value)
    }

    fn maybe_check_working_tree_roundtrip_encoding(
        &self,
        relative: &[u8],
        encoding: &WorkingTreeEncodingSpec,
        decoded: &[u8],
    ) -> Result<()> {
        if !self.should_check_roundtrip_encoding(&encoding.kind) {
            return Ok(());
        }
        emit_roundtrip_trace_if_enabled(encoding)?;
        let reencoded = encode_working_tree_encoding_content(relative, &encoding.kind, decoded)?;
        let redecooded =
            decode_working_tree_encoding_content(relative, &encoding.kind, &reencoded)?;
        if redecooded != decoded {
            return Err(CliError::Stderr {
                code: 128,
                text: format!(
                    "fatal: {}: encoding round-trip failed for {}\n",
                    String::from_utf8_lossy(relative),
                    encoding.kind.display_name()
                ),
            });
        }
        Ok(())
    }

    fn should_check_roundtrip_encoding(&self, encoding: &WorkingTreeEncoding) -> bool {
        let Some(encodings) = &self.roundtrip_encodings else {
            return matches!(encoding, WorkingTreeEncoding::ShiftJis);
        };
        encodings.contains(&encoding.config_key().to_ascii_lowercase())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkingTreeEncoding {
    Utf8,
    Utf16 { endian: UtfEndian, bom: BomMode },
    Utf32 { endian: UtfEndian, bom: BomMode },
    ShiftJis,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkingTreeEncodingSpec {
    kind: WorkingTreeEncoding,
    declared_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UtfEndian {
    Little,
    Big,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BomMode {
    None,
    Required,
}

fn parse_working_tree_encoding_value(
    relative: &[u8],
    value: AttributeValue,
) -> Result<Option<WorkingTreeEncodingSpec>> {
    match value {
        AttributeValue::Unset | AttributeValue::Unspecified => Ok(None),
        AttributeValue::Set => Err(CliError::Stderr {
            code: 128,
            text: format!(
                "fatal: {}: true/false are no valid working-tree-encodings\n",
                String::from_utf8_lossy(relative)
            ),
        }),
        AttributeValue::Value(value) if value.is_empty() => Ok(None),
        AttributeValue::Value(value) => Ok(Some(WorkingTreeEncodingSpec {
            kind: parse_working_tree_encoding_name(relative, &value)?,
            declared_name: value,
        })),
    }
}

fn parse_working_tree_encoding_name(relative: &[u8], value: &str) -> Result<WorkingTreeEncoding> {
    let normalized = value.to_ascii_uppercase();
    match normalized.as_str() {
        "UTF-8" => Ok(WorkingTreeEncoding::Utf8),
        "UTF-16" => Ok(WorkingTreeEncoding::Utf16 {
            endian: UtfEndian::Big,
            bom: BomMode::Required,
        }),
        "UTF-16LE" => Ok(WorkingTreeEncoding::Utf16 {
            endian: UtfEndian::Little,
            bom: BomMode::None,
        }),
        "UTF-16BE" => Ok(WorkingTreeEncoding::Utf16 {
            endian: UtfEndian::Big,
            bom: BomMode::None,
        }),
        "UTF-16LE-BOM" => Ok(WorkingTreeEncoding::Utf16 {
            endian: UtfEndian::Little,
            bom: BomMode::Required,
        }),
        "UTF-16BE-BOM" => Ok(WorkingTreeEncoding::Utf16 {
            endian: UtfEndian::Big,
            bom: BomMode::Required,
        }),
        "UTF-32" => Ok(WorkingTreeEncoding::Utf32 {
            endian: UtfEndian::Big,
            bom: BomMode::Required,
        }),
        "UTF-32LE" => Ok(WorkingTreeEncoding::Utf32 {
            endian: UtfEndian::Little,
            bom: BomMode::None,
        }),
        "UTF-32BE" => Ok(WorkingTreeEncoding::Utf32 {
            endian: UtfEndian::Big,
            bom: BomMode::None,
        }),
        "UTF-32LE-BOM" => Ok(WorkingTreeEncoding::Utf32 {
            endian: UtfEndian::Little,
            bom: BomMode::Required,
        }),
        "UTF-32BE-BOM" => Ok(WorkingTreeEncoding::Utf32 {
            endian: UtfEndian::Big,
            bom: BomMode::Required,
        }),
        "SHIFT-JIS" => Ok(WorkingTreeEncoding::ShiftJis),
        _ => Err(failed_to_encode_message(relative, value, "UTF-8")),
    }
}

fn decode_working_tree_encoding_content(
    relative: &[u8],
    encoding: &WorkingTreeEncoding,
    content: &[u8],
) -> Result<Vec<u8>> {
    match encoding {
        WorkingTreeEncoding::Utf8 => Ok(content.to_vec()),
        WorkingTreeEncoding::Utf16 { endian, bom } => {
            decode_utf16_content(relative, *endian, *bom, content)
        }
        WorkingTreeEncoding::Utf32 { endian, bom } => {
            decode_utf32_content(relative, *endian, *bom, content)
        }
        WorkingTreeEncoding::ShiftJis => decode_encoding_rs_content(relative, SHIFT_JIS, content),
    }
}

fn encode_working_tree_encoding_content(
    relative: &[u8],
    encoding: &WorkingTreeEncoding,
    content: &[u8],
) -> Result<Vec<u8>> {
    match encoding {
        WorkingTreeEncoding::Utf8 => Ok(content.to_vec()),
        WorkingTreeEncoding::Utf16 { endian, bom } => {
            encode_utf16_content(relative, *endian, *bom, content)
        }
        WorkingTreeEncoding::Utf32 { endian, bom } => {
            encode_utf32_content(relative, *endian, *bom, content)
        }
        WorkingTreeEncoding::ShiftJis => encode_encoding_rs_content(relative, SHIFT_JIS, content),
    }
}

fn decode_utf16_content(
    relative: &[u8],
    endian: UtfEndian,
    bom: BomMode,
    content: &[u8],
) -> Result<Vec<u8>> {
    let (content, endian) = match bom {
        BomMode::Required => decode_required_bom(content, "utf-16", b"\xFF\xFE", b"\xFE\xFF")?,
        BomMode::None => {
            let encoded_as = match endian {
                UtfEndian::Little => "utf-16LE",
                UtfEndian::Big => "utf-16be",
            };
            reject_prohibited_bom(content, encoded_as, "UTF-16")?;
            (content, endian)
        }
    };
    if content.len() % 2 != 0 {
        return Err(failed_to_encode_message(relative, "UTF-16", "UTF-8"));
    }
    let mut units = Vec::with_capacity(content.len() / 2);
    for chunk in content.chunks_exact(2) {
        let unit = match endian {
            UtfEndian::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
            UtfEndian::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
        };
        units.push(unit);
    }
    String::from_utf16(&units)
        .map(|value| value.into_bytes())
        .map_err(|_| failed_to_encode_message(relative, "UTF-16", "UTF-8"))
}

fn decode_utf32_content(
    relative: &[u8],
    endian: UtfEndian,
    bom: BomMode,
    content: &[u8],
) -> Result<Vec<u8>> {
    let (content, endian) = match bom {
        BomMode::Required => {
            decode_required_bom(content, "utf-32", b"\xFF\xFE\x00\x00", b"\x00\x00\xFE\xFF")?
        }
        BomMode::None => {
            let encoded_as = match endian {
                UtfEndian::Little => "utf-32LE",
                UtfEndian::Big => "utf-32be",
            };
            reject_prohibited_bom(content, encoded_as, "UTF-32")?;
            (content, endian)
        }
    };
    if content.len() % 4 != 0 {
        return Err(failed_to_encode_message(relative, "UTF-32", "UTF-8"));
    }
    let mut decoded = String::with_capacity(content.len() / 4);
    for chunk in content.chunks_exact(4) {
        let unit = match endian {
            UtfEndian::Little => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            UtfEndian::Big => u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
        };
        let Some(ch) = char::from_u32(unit) else {
            return Err(failed_to_encode_message(relative, "UTF-32", "UTF-8"));
        };
        decoded.push(ch);
    }
    Ok(decoded.into_bytes())
}

fn encode_utf16_content(
    relative: &[u8],
    endian: UtfEndian,
    bom: BomMode,
    content: &[u8],
) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(content)
        .map_err(|_| failed_to_encode_message(relative, "UTF-8", "UTF-16"))?;
    let mut encoded = Vec::with_capacity(content.len() * 2 + 2);
    if bom == BomMode::Required {
        match endian {
            UtfEndian::Little => encoded.extend_from_slice(b"\xFF\xFE"),
            UtfEndian::Big => encoded.extend_from_slice(b"\xFE\xFF"),
        }
    }
    for unit in text.encode_utf16() {
        let bytes = match endian {
            UtfEndian::Little => unit.to_le_bytes(),
            UtfEndian::Big => unit.to_be_bytes(),
        };
        encoded.extend_from_slice(&bytes);
    }
    Ok(encoded)
}

fn encode_utf32_content(
    relative: &[u8],
    endian: UtfEndian,
    bom: BomMode,
    content: &[u8],
) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(content)
        .map_err(|_| failed_to_encode_message(relative, "UTF-8", "UTF-32"))?;
    let mut encoded = Vec::with_capacity(content.len() * 4 + 4);
    if bom == BomMode::Required {
        match endian {
            UtfEndian::Little => encoded.extend_from_slice(b"\xFF\xFE\x00\x00"),
            UtfEndian::Big => encoded.extend_from_slice(b"\x00\x00\xFE\xFF"),
        }
    }
    for ch in text.chars() {
        let bytes = match endian {
            UtfEndian::Little => (ch as u32).to_le_bytes(),
            UtfEndian::Big => (ch as u32).to_be_bytes(),
        };
        encoded.extend_from_slice(&bytes);
    }
    Ok(encoded)
}

fn decode_encoding_rs_content(
    relative: &[u8],
    encoding: &'static Encoding,
    content: &[u8],
) -> Result<Vec<u8>> {
    let (decoded, _, had_errors) = encoding.decode(content);
    if had_errors {
        return Err(failed_to_encode_message(relative, encoding.name(), "UTF-8"));
    }
    Ok(decoded.into_owned().into_bytes())
}

fn encode_encoding_rs_content(
    relative: &[u8],
    encoding: &'static Encoding,
    content: &[u8],
) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(content)
        .map_err(|_| failed_to_encode_message(relative, "UTF-8", encoding.name()))?;
    let (encoded, _, had_errors) = encoding.encode(text);
    if had_errors {
        return Err(failed_to_encode_message(relative, "UTF-8", encoding.name()));
    }
    Ok(encoded.into_owned())
}

fn decode_required_bom<'a>(
    content: &'a [u8],
    display: &str,
    little_bom: &[u8],
    big_bom: &[u8],
) -> Result<(&'a [u8], UtfEndian)> {
    if let Some(rest) = content.strip_prefix(little_bom) {
        return Ok((rest, UtfEndian::Little));
    }
    if let Some(rest) = content.strip_prefix(big_bom) {
        return Ok((rest, UtfEndian::Big));
    }
    Err(CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: BOM is required in '{}' if encoded as {}\nuse {}BE or {}LE as working-tree-encoding\n",
            display,
            display,
            display.to_ascii_uppercase(),
            display.to_ascii_uppercase()
        ),
    })
}

fn reject_prohibited_bom(content: &[u8], display: &str, suggested: &str) -> Result<()> {
    let has_bom = content.starts_with(b"\xFF\xFE")
        || content.starts_with(b"\xFE\xFF")
        || content.starts_with(b"\xFF\xFE\x00\x00")
        || content.starts_with(b"\x00\x00\xFE\xFF");
    if has_bom {
        return Err(CliError::Stderr {
            code: 128,
            text: format!(
                "fatal: BOM is prohibited in '{}' if encoded as {}\nuse {} as working-tree-encoding\n",
                display, display, suggested
            ),
        });
    }
    Ok(())
}

fn failed_to_encode_message(relative: &[u8], from: &str, to: &str) -> CliError {
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: {}: failed to encode content from {} to {}\n",
            String::from_utf8_lossy(relative),
            from,
            to
        ),
    }
}

fn should_soften_diff_worktree_encoding_error(error: &CliError) -> bool {
    match error {
        CliError::Stderr { text, .. } => {
            text.contains("BOM is required")
                || text.contains("BOM is prohibited")
                || text.contains("failed to encode content")
        }
        _ => false,
    }
}

fn emit_softened_diff_worktree_encoding_error(relative: &[u8], error: &CliError) {
    if let CliError::Stderr { text, .. } = error {
        let path = String::from_utf8_lossy(relative);
        for (index, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            if index == 0 {
                let normalized = line.replacen("fatal:", "error:", 1);
                if let Some(encoding) = normalized
                    .strip_prefix("error: BOM is required in '")
                    .and_then(|rest| rest.strip_suffix("' if encoded as utf-16"))
                {
                    eprintln!("error: BOM is required in '{path}' if encoded as {encoding}");
                } else if let Some(encoding) = normalized
                    .strip_prefix("error: BOM is prohibited in '")
                    .and_then(|rest| rest.strip_suffix("' if encoded as utf-16"))
                {
                    eprintln!("error: BOM is prohibited in '{path}' if encoded as {encoding}");
                } else {
                    eprintln!("{normalized}");
                }
            } else {
                eprintln!("hint: {line}");
            }
        }
    }
}

fn emit_roundtrip_trace_if_enabled(encoding: &WorkingTreeEncodingSpec) -> Result<()> {
    let Some(value) = std::env::var_os("GIT_TRACE") else {
        return Ok(());
    };
    let rendered = value.to_string_lossy();
    if rendered.is_empty() || rendered == "0" || rendered.eq_ignore_ascii_case("false") {
        return Ok(());
    }
    let line = format!(
        "trace: Checking roundtrip encoding for {}\n",
        encoding.declared_name
    );
    if rendered == "1" || rendered == "2" || rendered.eq_ignore_ascii_case("true") {
        std::io::stderr()
            .lock()
            .write_all(line.as_bytes())
            .map_err(CliError::Io)?;
        return Ok(());
    }
    let mut trace = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::path::PathBuf::from(value))
        .map_err(CliError::Io)?;
    trace.write_all(line.as_bytes()).map_err(CliError::Io)?;
    Ok(())
}

fn core_check_roundtrip_encodings_from_config(entries: &[ConfigEntry]) -> Option<HashSet<String>> {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == "core"
                && entry.subsection.is_empty()
                && entry.key == "checkroundtripencoding"
        })
        .map(|entry| {
            entry
                .value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| item.to_ascii_lowercase())
                .collect::<HashSet<_>>()
        })
}

impl WorkingTreeEncoding {
    fn config_key(&self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16 { .. } => "UTF-16",
            Self::Utf32 { .. } => "UTF-32",
            Self::ShiftJis => "SHIFT-JIS",
        }
    }

    fn display_name(&self) -> &'static str {
        self.config_key()
    }
}

fn emit_attribute_warnings(attributes: &GitAttributes) {
    for warning in attributes.warnings() {
        eprintln!("{warning}");
    }
}

pub(crate) fn load_repo_attributes(
    repo: &GitRepo,
    root_index_attributes: Option<&GitAttributes>,
) -> Result<GitAttributes> {
    let entries = read_config_entries(repo)?;
    let ignore_case = core_ignorecase_from_config(&entries);
    load_attributes_with_strategy(repo, None, ignore_case, root_index_attributes)
}

fn load_worktree_attribute_base_rules(
    repo: &GitRepo,
    ignore_case: bool,
) -> Result<WorktreeAttributeBaseRules> {
    let mut before_worktree = GitAttributes::default();
    before_worktree.set_ignore_case(ignore_case);
    if let Some(path) = read_config_value(repo, "core.attributesfile")?
        .map(|value| expand_attribute_config_path(&value))
        .or_else(|| git_attr_global_path().ok().flatten().map(PathBuf::from))
    {
        append_attribute_file(
            &mut before_worktree,
            "",
            &path,
            &path.to_string_lossy(),
            ignore_case,
            true,
        )?;
    }
    if let Some(path) = git_attr_system_path().map(PathBuf::from) {
        append_attribute_file(
            &mut before_worktree,
            "",
            &path,
            &path.to_string_lossy(),
            ignore_case,
            true,
        )?;
    }

    let mut after_worktree = GitAttributes::default();
    after_worktree.set_ignore_case(ignore_case);
    append_attribute_file(
        &mut after_worktree,
        "",
        &repo.git_dir.join("info").join("attributes"),
        "info/attributes",
        ignore_case,
        true,
    )?;
    Ok(WorktreeAttributeBaseRules {
        before_worktree,
        after_worktree,
    })
}

fn load_attributes_with_strategy(
    repo: &GitRepo,
    relative: Option<&[u8]>,
    ignore_case: bool,
    root_index_attributes: Option<&GitAttributes>,
) -> Result<GitAttributes> {
    let mut attributes = GitAttributes::default();
    attributes.set_ignore_case(ignore_case);

    if let Some(path) = read_config_value(repo, "core.attributesfile")?
        .map(|value| expand_attribute_config_path(&value))
        .or_else(|| git_attr_global_path().ok().flatten().map(PathBuf::from))
    {
        append_attribute_file(
            &mut attributes,
            "",
            &path,
            &path.to_string_lossy(),
            ignore_case,
            true,
        )?;
    }
    if let Some(path) = git_attr_system_path().map(PathBuf::from) {
        append_attribute_file(
            &mut attributes,
            "",
            &path,
            &path.to_string_lossy(),
            ignore_case,
            true,
        )?;
    }
    match relative {
        Some(relative) => load_attributes_for_relative_path(
            &mut attributes,
            &repo.root,
            relative,
            ignore_case,
            root_index_attributes,
        )?,
        None => load_attributes_from_dir_recursive(
            &mut attributes,
            &repo.root,
            &repo.root,
            ignore_case,
            root_index_attributes,
        )?,
    }
    append_attribute_file(
        &mut attributes,
        "",
        &repo.git_dir.join("info").join("attributes"),
        "info/attributes",
        ignore_case,
        true,
    )?;
    Ok(attributes)
}

fn load_attributes_for_relative_path(
    attributes: &mut GitAttributes,
    root: &Path,
    relative: &[u8],
    ignore_case: bool,
    root_index_attributes: Option<&GitAttributes>,
) -> Result<()> {
    append_root_attribute_source(attributes, root, ignore_case, root_index_attributes)?;

    let absolute = worktree_path_for_index_entry(root, relative);
    let Some(parent) = absolute.parent() else {
        return Ok(());
    };
    let mut directories = Vec::new();
    let mut current = parent;
    while current != root {
        directories.push(current.to_path_buf());
        let Some(next) = current.parent() else {
            break;
        };
        current = next;
    }
    directories.reverse();
    for dir in directories {
        let base = repo_relative_path(root, &dir)
            .ok()
            .map(|path| String::from_utf8_lossy(&path).into_owned())
            .unwrap_or_default();
        append_attribute_file(
            attributes,
            &base,
            &dir.join(".gitattributes"),
            &attribute_source_label(&base, ".gitattributes"),
            ignore_case,
            false,
        )?;
    }
    Ok(())
}

fn core_ignorecase_from_config(entries: &[ConfigEntry]) -> bool {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == "core" && entry.subsection.is_empty() && entry.key == "ignorecase"
        })
        .and_then(|entry| entry.bool_value())
        .unwrap_or(false)
}

fn expand_attribute_config_path(value: &str) -> PathBuf {
    if value == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(value)
}

fn load_attributes_from_dir_recursive(
    attributes: &mut GitAttributes,
    root: &Path,
    dir: &Path,
    ignore_case: bool,
    root_index_attributes: Option<&GitAttributes>,
) -> Result<()> {
    let base = repo_relative_path(root, dir)
        .ok()
        .map(|path| String::from_utf8_lossy(&path).into_owned())
        .unwrap_or_default();
    if dir == root {
        append_root_attribute_source(attributes, root, ignore_case, root_index_attributes)?;
    } else {
        append_attribute_file(
            attributes,
            &base,
            &dir.join(".gitattributes"),
            &attribute_source_label(&base, ".gitattributes"),
            ignore_case,
            false,
        )?;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => return Err(CliError::Io(error)),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => return Err(CliError::Io(error)),
        };
        if metadata.is_dir() {
            load_attributes_from_dir_recursive(
                attributes,
                root,
                &entry.path(),
                ignore_case,
                root_index_attributes,
            )?;
        }
    }
    Ok(())
}

fn append_root_attribute_source(
    attributes: &mut GitAttributes,
    root: &Path,
    ignore_case: bool,
    root_index_attributes: Option<&GitAttributes>,
) -> Result<()> {
    let path = root.join(".gitattributes");
    match fs::metadata(&path) {
        Ok(_) => append_attribute_file(attributes, "", &path, ".gitattributes", ignore_case, false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(root_attributes) = root_index_attributes {
                attributes.append(root_attributes.clone());
            }
            Ok(())
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn load_index_root_attributes(
    store: &LooseObjectStore,
    index: &GitIndex,
) -> Result<Option<GitAttributes>> {
    let Some(entry) = index.entry(b".gitattributes", 0) else {
        return Ok(None);
    };
    if !matches!(entry.mode, IndexMode::File | IndexMode::Executable) {
        return Ok(None);
    }
    let content = read_index_entry_content(store, entry)?;
    let mut attributes = GitAttributes::parse_with_base_and_source_and_case(
        &String::from_utf8_lossy(&content),
        "",
        ".gitattributes",
        false,
    );
    Ok(Some({
        attributes.set_ignore_case(false);
        attributes
    }))
}

fn append_attribute_file(
    attributes: &mut GitAttributes,
    base: &str,
    path: &Path,
    source: &str,
    ignore_case: bool,
    follow_symlinks: bool,
) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    if metadata.file_type().is_symlink() && !follow_symlinks {
        eprintln!(
            "unable to access '{}': symbolic link not supported",
            path.display()
        );
        return Ok(());
    }
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            eprintln!("unable to access '{}': {}", path.display(), error);
            return Ok(());
        }
    };
    attributes.append(GitAttributes::parse_with_base_and_source_and_case(
        &content,
        base,
        source,
        ignore_case,
    ));
    Ok(())
}

fn attribute_source_label(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_owned()
    } else {
        format!("{base}/{name}")
    }
}

pub(crate) fn clean_worktree_content(
    repo: &GitRepo,
    relative: &[u8],
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    WorktreeContentRules::load(repo)?.clean_worktree_content(repo, relative, content)
}

pub(crate) fn clean_worktree_content_for_comparison_against_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    relative: &[u8],
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    let stage_options = WorktreeStageOptions::load(repo)?;
    let content_rules = stage_options.path_content_rules(repo, relative)?;
    content_rules.clean_worktree_content_inner(
        repo,
        Some((store, index)),
        relative,
        content,
        content_rules.core_safecrlf.demote_for_worktree_compare(),
        false,
    )
}

pub(crate) fn smudge_worktree_filter_entries(repo: &GitRepo, entries: &GitIndex) -> Result<()> {
    smudge_worktree_filter_entries_with_metadata(
        repo,
        entries,
        &WorktreeCheckoutMetadata::default(),
    )
}

pub(crate) fn smudge_worktree_filter_entries_with_metadata(
    repo: &GitRepo,
    entries: &GitIndex,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    let content_rules = WorktreeStageOptions::load(repo)?;
    smudge_worktree_filter_entries_with_options(repo, entries, metadata, &content_rules)
}

pub(crate) fn smudge_worktree_filter_entries_with_metadata_for_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    entries: &GitIndex,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    let content_rules = WorktreeStageOptions::load_for_index(repo, store, index)?;
    smudge_worktree_filter_entries_with_options(repo, entries, metadata, &content_rules)
}

fn smudge_worktree_filter_entries_with_options(
    repo: &GitRepo,
    entries: &GitIndex,
    metadata: &WorktreeCheckoutMetadata,
    content_rules: &WorktreeStageOptions,
) -> Result<()> {
    if !content_rules.may_smudge_checkout_entries(repo)? {
        return Ok(());
    }
    let attributes = content_rules.attributes(repo)?;
    if attributes.is_empty() {
        for entry in entries.entries().iter().filter(|entry| entry.stage == 0) {
            smudge_checkout_content_entry(repo, &content_rules, entry)?;
        }
        return Ok(());
    }
    let mut delayed = Vec::new();
    for entry in entries.entries().iter().filter(|entry| entry.stage == 0) {
        smudge_checkout_content_entry(repo, &content_rules, entry)?;
        let path = worktree_path_for_index_entry(&repo.root, &entry.path);
        if let Some(key) = smudge_worktree_filter_entry_at_path_with_metadata(
            repo,
            &attributes,
            entry,
            &path,
            metadata,
            true,
        )? {
            delayed.push(DelayedSmudgeEntry {
                entry: entry.clone(),
                path,
                key,
            });
        }
    }
    complete_delayed_smudge_filters(repo, &attributes, delayed, metadata)
}

fn smudge_checkout_content_entry(
    repo: &GitRepo,
    options: &WorktreeStageOptions,
    entry: &IndexEntry,
) -> Result<()> {
    if !matches!(entry.mode, IndexMode::File | IndexMode::Executable) {
        return Ok(());
    }
    let path = worktree_path_for_index_entry(&repo.root, &entry.path);
    let content = fs::read(&path)?;
    if let Some(smudged) = options.smudge_checkout_content(repo, &entry.path, &content)? {
        fs::write(path, smudged)?;
    }
    Ok(())
}

struct DelayedSmudgeEntry {
    entry: IndexEntry,
    path: PathBuf,
    key: ProcessFilterKey,
}

pub(crate) fn smudge_worktree_filter_entry_at_path(
    repo: &GitRepo,
    entry: &IndexEntry,
    path: &std::path::Path,
) -> Result<()> {
    let attributes = GitAttributes::load_from_root(&repo.root)?;
    smudge_worktree_filter_entry_at_path_with_metadata(
        repo,
        &attributes,
        entry,
        path,
        &WorktreeCheckoutMetadata::default(),
        true,
    )?;
    Ok(())
}

fn smudge_worktree_filter_entry_at_path_with_metadata(
    repo: &GitRepo,
    attributes: &GitAttributes,
    entry: &IndexEntry,
    path: &std::path::Path,
    checkout_metadata: &WorktreeCheckoutMetadata,
    allow_delay: bool,
) -> Result<Option<ProcessFilterKey>> {
    if !matches!(entry.mode, IndexMode::File | IndexMode::Executable) {
        return Ok(None);
    }
    if worktree_filter_name(attributes, &entry.path).is_none() {
        return Ok(None);
    }
    let content = fs::read(path)?;
    match smudge_worktree_filter_content_result_with_attributes(
        repo,
        attributes,
        &entry.path,
        &entry.id,
        checkout_metadata,
        content,
        allow_delay,
    )? {
        WorktreeFilterResult::Content(content) => {
            fs::write(path, content)?;
            Ok(None)
        }
        WorktreeFilterResult::Delayed { key } => Ok(Some(key)),
    }
}

pub(crate) fn smudge_worktree_filter_content(
    repo: &GitRepo,
    relative: &[u8],
    blob_id: &ObjectId,
    checkout_metadata: &WorktreeCheckoutMetadata,
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    match smudge_worktree_filter_content_result(
        repo,
        relative,
        blob_id,
        checkout_metadata,
        content,
        false,
    )? {
        WorktreeFilterResult::Content(content) => Ok(content),
        WorktreeFilterResult::Delayed { .. } => Err(CliError::Fatal {
            code: 128,
            message: "filter process delayed response is not supported here".to_owned(),
        }),
    }
}

pub(crate) fn smudge_worktree_filter_content_with_attributes(
    repo: &GitRepo,
    attributes: &GitAttributes,
    relative: &[u8],
    blob_id: &ObjectId,
    checkout_metadata: &WorktreeCheckoutMetadata,
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    match smudge_worktree_filter_content_result_with_attributes(
        repo,
        attributes,
        relative,
        blob_id,
        checkout_metadata,
        content,
        false,
    )? {
        WorktreeFilterResult::Content(content) => Ok(content),
        WorktreeFilterResult::Delayed { .. } => Err(CliError::Fatal {
            code: 128,
            message: "filter process delayed response is not supported here".to_owned(),
        }),
    }
}

pub(crate) fn smudge_worktree_content_with_attributes(
    repo: &GitRepo,
    attributes: &GitAttributes,
    relative: &[u8],
    blob_id: &ObjectId,
    checkout_metadata: &WorktreeCheckoutMetadata,
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    let entries = read_config_entries(repo)?;
    let rules = WorktreeContentRules {
        attributes: attributes.clone(),
        core_autocrlf: core_autocrlf_from_config(&entries),
        core_eol: core_eol_from_config(&entries),
        core_safecrlf: core_safecrlf_from_config(&entries),
        roundtrip_encodings: core_check_roundtrip_encodings_from_config(&entries),
    };
    let content = rules
        .smudge_checkout_content(relative, &content)?
        .unwrap_or(content);
    match smudge_worktree_filter_content_result_with_attributes(
        repo,
        &rules.attributes,
        relative,
        blob_id,
        checkout_metadata,
        content,
        false,
    )? {
        WorktreeFilterResult::Content(content) => Ok(content),
        WorktreeFilterResult::Delayed { .. } => Err(CliError::Fatal {
            code: 128,
            message: "filter process delayed response is not supported here".to_owned(),
        }),
    }
}

pub(crate) fn smudge_worktree_content(
    repo: &GitRepo,
    relative: &[u8],
    blob_id: &ObjectId,
    checkout_metadata: &WorktreeCheckoutMetadata,
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    let rules = WorktreeContentRules::load(repo)?;
    let content = rules
        .smudge_checkout_content(relative, &content)?
        .unwrap_or(content);
    match smudge_worktree_filter_content_result_with_attributes(
        repo,
        &rules.attributes,
        relative,
        blob_id,
        checkout_metadata,
        content,
        false,
    )? {
        WorktreeFilterResult::Content(content) => Ok(content),
        WorktreeFilterResult::Delayed { .. } => Err(CliError::Fatal {
            code: 128,
            message: "filter process delayed response is not supported here".to_owned(),
        }),
    }
}

fn smudge_worktree_filter_content_result(
    repo: &GitRepo,
    relative: &[u8],
    blob_id: &ObjectId,
    checkout_metadata: &WorktreeCheckoutMetadata,
    content: Vec<u8>,
    allow_delay: bool,
) -> Result<WorktreeFilterResult> {
    let attributes = GitAttributes::load_from_root(&repo.root)?;
    smudge_worktree_filter_content_result_with_attributes(
        repo,
        &attributes,
        relative,
        blob_id,
        checkout_metadata,
        content,
        allow_delay,
    )
}

fn smudge_worktree_filter_content_result_with_attributes(
    repo: &GitRepo,
    attributes: &GitAttributes,
    relative: &[u8],
    blob_id: &ObjectId,
    checkout_metadata: &WorktreeCheckoutMetadata,
    content: Vec<u8>,
    allow_delay: bool,
) -> Result<WorktreeFilterResult> {
    if worktree_filter_name(&attributes, relative).is_none() {
        return Ok(WorktreeFilterResult::Content(content));
    }
    let mut metadata = checkout_metadata.process_filter_items();
    metadata.push(format!("blob={}", blob_id.to_hex()));
    if allow_delay {
        metadata.push("can-delay=1".to_owned());
    }
    apply_worktree_filter_result(repo, attributes, relative, "smudge", &metadata, content)
}

fn complete_delayed_smudge_filters(
    repo: &GitRepo,
    attributes: &GitAttributes,
    mut delayed: Vec<DelayedSmudgeEntry>,
    checkout_metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    let mut delayed_keys = Vec::<ProcessFilterKey>::new();
    for entry in &delayed {
        if !delayed_keys.contains(&entry.key) {
            delayed_keys.push(entry.key.clone());
        }
    }
    while !delayed.is_empty() {
        let mut keys = Vec::<ProcessFilterKey>::new();
        for entry in &delayed {
            if !keys.contains(&entry.key) {
                keys.push(entry.key.clone());
            }
        }
        let mut progressed = false;
        for key in keys {
            for path in list_available_filter_blobs(&key)? {
                let Some(index) = delayed
                    .iter()
                    .position(|entry| entry.key == key && entry.entry.path == path)
                else {
                    return Err(CliError::Stderr {
                        code: 128,
                        text: format!(
                            "error: external filter '{}' signaled that '{}' is now available although it has not been delayed earlier\n",
                            key.command,
                            String::from_utf8_lossy(&path)
                        ),
                    });
                };
                let delayed_entry = delayed.remove(index);
                let result = smudge_worktree_filter_content_result_with_attributes(
                    repo,
                    attributes,
                    &delayed_entry.entry.path,
                    &delayed_entry.entry.id,
                    checkout_metadata,
                    Vec::new(),
                    false,
                )?;
                let WorktreeFilterResult::Content(content) = result else {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "filter process returned nested delayed response".to_owned(),
                    });
                };
                fs::write(&delayed_entry.path, content)?;
                progressed = true;
            }
        }
        if !progressed {
            let path = delayed
                .first()
                .map(|entry| String::from_utf8_lossy(&entry.entry.path).into_owned())
                .unwrap_or_else(|| "unknown".to_owned());
            return Err(CliError::Stderr {
                code: 128,
                text: format!("error: '{path}' was not filtered properly\n"),
            });
        }
    }
    for key in delayed_keys {
        let _ = list_available_filter_blobs(&key)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorktreeCheckoutMetadata {
    pub(crate) ref_name: Option<String>,
    pub(crate) treeish: Option<ObjectId>,
}

impl WorktreeCheckoutMetadata {
    fn process_filter_items(&self) -> Vec<String> {
        let mut items = Vec::new();
        if let Some(ref_name) = &self.ref_name {
            items.push(format!("ref={ref_name}"));
        }
        if let Some(treeish) = &self.treeish {
            items.push(format!("treeish={}", treeish.to_hex()));
        }
        items
    }
}

fn worktree_filter_name(attributes: &GitAttributes, relative: &[u8]) -> Option<String> {
    attributes
        .check(relative, &["filter".to_owned()])
        .into_iter()
        .find_map(|(_, value)| match value {
            AttributeValue::Value(name) if !name.is_empty() => Some(name),
            _ => None,
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoreAutoCrlf {
    False,
    True,
    Input,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CoreEol {
    Unset,
    Lf,
    Crlf,
    Native,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CoreSafeCrlf {
    Unset,
    False,
    Warn,
    True,
}

impl CoreSafeCrlf {
    fn demote_for_worktree_compare(self) -> Self {
        match self {
            Self::True => Self::Warn,
            other => other,
        }
    }

    fn emits_warning(self) -> bool {
        !matches!(self, Self::False)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrlfAction {
    Binary,
    TextInput,
    TextCrlf,
    AutoInput,
    AutoCrlf,
}

impl CrlfAction {
    fn is_auto(self) -> bool {
        matches!(self, Self::AutoInput | Self::AutoCrlf)
    }

    fn output_crlf(self) -> bool {
        matches!(self, Self::TextCrlf | Self::AutoCrlf)
    }
}

#[derive(Clone, Copy)]
struct CrlfStats {
    nul: usize,
    lonecr: usize,
    lonelf: usize,
    crlf: usize,
    printable: usize,
    nonprintable: usize,
}

impl CrlfStats {
    fn gather(content: &[u8]) -> Self {
        let mut stats = Self {
            nul: 0,
            lonecr: 0,
            lonelf: 0,
            crlf: 0,
            printable: 0,
            nonprintable: 0,
        };
        let mut index = 0usize;
        while index < content.len() {
            match content[index] {
                b'\r' if content.get(index + 1) == Some(&b'\n') => {
                    stats.crlf += 1;
                    index += 2;
                    continue;
                }
                b'\r' => stats.lonecr += 1,
                b'\n' => stats.lonelf += 1,
                127 => stats.nonprintable += 1,
                byte if byte < 32 => match byte {
                    b'\x08' | b'\t' | b'\x1b' | b'\x0c' => stats.printable += 1,
                    0 => {
                        stats.nul += 1;
                        stats.nonprintable += 1;
                    }
                    _ => stats.nonprintable += 1,
                },
                _ => stats.printable += 1,
            }
            index += 1;
        }
        if content.last() == Some(&b'\x1a') {
            stats.nonprintable = stats.nonprintable.saturating_sub(1);
        }
        stats
    }

    fn is_binary(self) -> bool {
        self.lonecr > 0 || self.nul > 0 || (self.printable >> 7) < self.nonprintable
    }
}

fn core_autocrlf_from_config(entries: &[ConfigEntry]) -> CoreAutoCrlf {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == "core" && entry.subsection.is_empty() && entry.key == "autocrlf"
        })
        .map(|entry| {
            if entry.value.eq_ignore_ascii_case("input") {
                CoreAutoCrlf::Input
            } else if entry.bool_value().unwrap_or(false) {
                CoreAutoCrlf::True
            } else {
                CoreAutoCrlf::False
            }
        })
        .unwrap_or(CoreAutoCrlf::False)
}

fn core_eol_from_config(entries: &[ConfigEntry]) -> CoreEol {
    entries
        .iter()
        .rev()
        .find(|entry| entry.section == "core" && entry.subsection.is_empty() && entry.key == "eol")
        .map(|entry| match entry.value.to_ascii_lowercase().as_str() {
            "lf" => CoreEol::Lf,
            "crlf" => CoreEol::Crlf,
            "native" => CoreEol::Native,
            _ => CoreEol::Unset,
        })
        .unwrap_or(CoreEol::Unset)
}

fn core_safecrlf_from_config(entries: &[ConfigEntry]) -> CoreSafeCrlf {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == "core" && entry.subsection.is_empty() && entry.key == "safecrlf"
        })
        .map(|entry| {
            if entry.value.eq_ignore_ascii_case("warn") {
                CoreSafeCrlf::Warn
            } else if entry.bool_value().unwrap_or(false) {
                CoreSafeCrlf::True
            } else {
                CoreSafeCrlf::False
            }
        })
        .unwrap_or(CoreSafeCrlf::Unset)
}

fn crlf_action_for_path(
    attributes: &GitAttributes,
    relative: &[u8],
    core_autocrlf: CoreAutoCrlf,
    core_eol: CoreEol,
) -> CrlfAction {
    let values = attributes.check(
        relative,
        &["crlf".to_owned(), "text".to_owned(), "eol".to_owned()],
    );
    let text = values
        .iter()
        .find(|(name, _)| name == "text")
        .map(|(_, value)| value);
    let crlf = values
        .iter()
        .find(|(name, _)| name == "crlf")
        .map(|(_, value)| value);
    let eol = values
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("eol", AttributeValue::Value(value)) if value == "lf" => Some(CoreEol::Lf),
            ("eol", AttributeValue::Value(value)) if value == "crlf" => Some(CoreEol::Crlf),
            _ => None,
        });

    let mut action = match text {
        Some(AttributeValue::Set) => Some(text_action(core_autocrlf, core_eol)),
        Some(AttributeValue::Unset) => Some(CrlfAction::Binary),
        Some(AttributeValue::Value(value)) if value == "auto" => {
            Some(auto_action(core_autocrlf, core_eol))
        }
        Some(AttributeValue::Value(value)) if value == "input" => Some(CrlfAction::TextInput),
        _ => match crlf {
            Some(AttributeValue::Set) => Some(CrlfAction::TextCrlf),
            Some(AttributeValue::Unset) => Some(CrlfAction::Binary),
            Some(AttributeValue::Value(value)) if value == "input" => Some(CrlfAction::TextInput),
            Some(AttributeValue::Value(value)) if value == "auto" => {
                Some(auto_action(core_autocrlf, core_eol))
            }
            _ => None,
        },
    };

    if action != Some(CrlfAction::Binary) {
        if eol == Some(CoreEol::Lf) {
            action = Some(match action {
                Some(CrlfAction::AutoInput | CrlfAction::AutoCrlf) => CrlfAction::AutoInput,
                _ => CrlfAction::TextInput,
            });
        } else if eol == Some(CoreEol::Crlf) {
            action = Some(match action {
                Some(CrlfAction::AutoInput | CrlfAction::AutoCrlf) => CrlfAction::AutoCrlf,
                _ => CrlfAction::TextCrlf,
            });
        }
    }

    action.unwrap_or_else(|| match core_autocrlf {
        CoreAutoCrlf::False => CrlfAction::Binary,
        CoreAutoCrlf::True => CrlfAction::AutoCrlf,
        CoreAutoCrlf::Input => CrlfAction::AutoInput,
    })
}

fn text_action(core_autocrlf: CoreAutoCrlf, core_eol: CoreEol) -> CrlfAction {
    match core_autocrlf {
        CoreAutoCrlf::True => return CrlfAction::TextCrlf,
        CoreAutoCrlf::Input => return CrlfAction::TextInput,
        CoreAutoCrlf::False => {}
    }
    if explicit_text_eol_is_crlf(core_eol) {
        CrlfAction::TextCrlf
    } else {
        CrlfAction::TextInput
    }
}

fn auto_action(core_autocrlf: CoreAutoCrlf, core_eol: CoreEol) -> CrlfAction {
    if text_eol_is_crlf(core_autocrlf, core_eol) {
        CrlfAction::AutoCrlf
    } else {
        CrlfAction::AutoInput
    }
}

fn text_eol_is_crlf(core_autocrlf: CoreAutoCrlf, core_eol: CoreEol) -> bool {
    match core_autocrlf {
        CoreAutoCrlf::True => return true,
        CoreAutoCrlf::Input => return false,
        CoreAutoCrlf::False => {}
    }
    match core_eol {
        CoreEol::Crlf => true,
        CoreEol::Native if cfg!(windows) => true,
        CoreEol::Unset if cfg!(windows) => true,
        _ => false,
    }
}

fn explicit_text_eol_is_crlf(core_eol: CoreEol) -> bool {
    match core_eol {
        CoreEol::Crlf => true,
        CoreEol::Native if cfg!(windows) => true,
        CoreEol::Unset if cfg!(windows) => true,
        _ => false,
    }
}

fn index_has_crlf(store: &LooseObjectStore, index: &GitIndex, relative: &[u8]) -> Result<bool> {
    let Some(entry) = find_index_entry(index, relative) else {
        return Ok(false);
    };
    let object = store.read_object(&entry.id)?;
    let stats = CrlfStats::gather(&object.content);
    Ok(!stats.is_binary() && stats.crlf > 0)
}

fn will_convert_lf_to_crlf(stats: &CrlfStats, action: CrlfAction) -> bool {
    if !action.output_crlf() || stats.lonelf == 0 {
        return false;
    }
    if action.is_auto() && (stats.lonecr > 0 || stats.crlf > 0 || stats.is_binary()) {
        return false;
    }
    true
}

fn enforce_safecrlf(
    relative: &[u8],
    action: CrlfAction,
    old_stats: &CrlfStats,
    new_stats: &CrlfStats,
    safecrlf: CoreSafeCrlf,
) -> Result<()> {
    let Some(message) = safecrlf_error_message(relative, action, old_stats, new_stats) else {
        return Ok(());
    };
    match safecrlf {
        CoreSafeCrlf::Unset | CoreSafeCrlf::False | CoreSafeCrlf::Warn => Ok(()),
        CoreSafeCrlf::True => Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            message,
        ))),
    }
}

fn safecrlf_error_message(
    relative: &[u8],
    action: CrlfAction,
    old_stats: &CrlfStats,
    new_stats: &CrlfStats,
) -> Option<String> {
    let path = String::from_utf8_lossy(relative);
    let lf_to_crlf = old_stats.lonelf > 0 && new_stats.lonelf == 0;
    let crlf_to_lf = old_stats.crlf > 0 && new_stats.crlf == 0;
    if action.output_crlf() && lf_to_crlf {
        Some(format!("LF would be replaced by CRLF in {path}"))
    } else if crlf_to_lf {
        Some(format!("CRLF would be replaced by LF in {path}"))
    } else if lf_to_crlf {
        Some(format!("LF would be replaced by CRLF in {path}"))
    } else {
        None
    }
}

fn emit_crlf_roundtrip_warning(
    relative: &[u8],
    action: CrlfAction,
    old_stats: &CrlfStats,
    new_stats: &CrlfStats,
) {
    let Some(message) = roundtrip_warning_message(relative, action, old_stats, new_stats) else {
        return;
    };
    eprintln!("{message}");
}

fn roundtrip_warning_message(
    relative: &[u8],
    action: CrlfAction,
    old_stats: &CrlfStats,
    new_stats: &CrlfStats,
) -> Option<String> {
    let path = String::from_utf8_lossy(relative);
    let lf_to_crlf = old_stats.lonelf > 0 && new_stats.lonelf == 0;
    let crlf_to_lf = old_stats.crlf > 0 && new_stats.crlf == 0;
    if action.output_crlf() && lf_to_crlf {
        Some(format!(
            "warning: in the working copy of '{path}', LF will be replaced by CRLF the next time Git touches it"
        ))
    } else if crlf_to_lf {
        Some(format!(
            "warning: in the working copy of '{path}', CRLF will be replaced by LF the next time Git touches it"
        ))
    } else if lf_to_crlf {
        Some(format!(
            "warning: in the working copy of '{path}', LF will be replaced by CRLF the next time Git touches it"
        ))
    } else {
        None
    }
}

fn clean_auto_crlf_to_lf(content: &[u8]) -> Vec<u8> {
    content
        .iter()
        .filter(|byte| **byte != b'\r')
        .copied()
        .collect()
}

fn apply_worktree_filter(
    repo: &GitRepo,
    attributes: &GitAttributes,
    relative: &[u8],
    direction: &str,
    metadata: &[String],
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    match apply_worktree_filter_result(repo, attributes, relative, direction, metadata, content)? {
        WorktreeFilterResult::Content(content) => Ok(content),
        WorktreeFilterResult::Delayed { .. } => Err(CliError::Fatal {
            code: 128,
            message: "filter process delayed response is not supported here".to_owned(),
        }),
    }
}

enum WorktreeFilterResult {
    Content(Vec<u8>),
    Delayed { key: ProcessFilterKey },
}

fn apply_worktree_filter_result(
    repo: &GitRepo,
    attributes: &GitAttributes,
    relative: &[u8],
    direction: &str,
    metadata: &[String],
    content: Vec<u8>,
) -> Result<WorktreeFilterResult> {
    let Some(filter) = worktree_filter_name(attributes, relative) else {
        return Ok(WorktreeFilterResult::Content(content));
    };
    if let Some(command) = read_config_value(repo, &format!("filter.{filter}.process"))? {
        let required = worktree_filter_required(repo, &filter)?;
        if let Some(filtered) = run_worktree_process_filter(
            repo, &filter, &command, relative, direction, metadata, &content,
        )? {
            return Ok(filtered);
        }
        if read_config_value(repo, &format!("filter.{filter}.{direction}"))?.is_none() {
            if required {
                return Err(worktree_filter_failed_error(&filter, relative, direction));
            }
            return Ok(WorktreeFilterResult::Content(content));
        }
    }
    let key = format!("filter.{filter}.{direction}");
    let Some(command) = read_config_value(repo, &key)? else {
        if worktree_filter_required(repo, &filter)? {
            return Err(worktree_filter_failed_error(&filter, relative, direction));
        }
        return Ok(WorktreeFilterResult::Content(content));
    };
    let command = expand_worktree_filter_command(&command, relative);
    run_worktree_filter_command(repo, &command, content).map(WorktreeFilterResult::Content)
}

fn worktree_filter_failed_error(filter: &str, relative: &[u8], direction: &str) -> CliError {
    let filter_display = if direction == "clean" {
        format!("'{filter}'")
    } else {
        filter.to_owned()
    };
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: {}: {direction} filter {filter_display} failed\n",
            String::from_utf8_lossy(relative)
        ),
    }
}

fn worktree_filter_required(repo: &GitRepo, filter: &str) -> Result<bool> {
    let Some(value) = read_config_value(repo, &format!("filter.{filter}.required"))? else {
        return Ok(false);
    };
    parse_git_bool(&value).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("bad boolean config value '{value}'"),
    })
}

fn expand_worktree_filter_command(command: &str, relative: &[u8]) -> String {
    if !command.contains("%f") {
        return command.to_owned();
    }
    command.replace("%f", &shell_quote_filter_path(relative))
}

fn shell_quote_filter_path(relative: &[u8]) -> String {
    let path = String::from_utf8_lossy(relative);
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('\'');
    for ch in path.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn run_worktree_filter_command(repo: &GitRepo, command: &str, content: Vec<u8>) -> Result<Vec<u8>> {
    let mut child = ProcessCommand::new(git_shell_command_path())
        .arg("-c")
        .arg(command)
        .current_dir(&repo.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(CliError::Io)?;
    let write_result = child
        .stdin
        .as_mut()
        .ok_or_else(|| CliError::Message("filter stdin unavailable".to_owned()))?
        .pipe_write_all_ignoring_sigpipe(&content);
    drop(child.stdin.take());
    let output = child.wait_with_output().map_err(CliError::Io)?;
    let ignored_broken_pipe = matches!(
        &write_result,
        Err(CliError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe
    );
    if output.status.success() || (ignored_broken_pipe && exit_status_is_sigpipe(&output.status)) {
        if let Err(error) = write_result
            && !matches!(&error, CliError::Io(io_error) if io_error.kind() == io::ErrorKind::BrokenPipe)
        {
            return Err(error);
        }
        Ok(output.stdout)
    } else {
        Err(CliError::Stderr {
            code: output.status.code().unwrap_or(1),
            text: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(unix)]
fn exit_status_is_sigpipe(status: &std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;

    status.signal() == Some(libc::SIGPIPE)
}

#[cfg(not(unix))]
fn exit_status_is_sigpipe(_status: &std::process::ExitStatus) -> bool {
    false
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ProcessFilterKey {
    root: PathBuf,
    filter: String,
    command: String,
}

struct ProcessFilter {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: io::BufReader<std::process::ChildStdout>,
    capabilities: HashSet<String>,
    aborted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessFilterStatus {
    Success,
    Error,
    Abort,
    Delayed,
    Invalid,
}

static WORKTREE_PROCESS_FILTERS: OnceLock<Mutex<HashMap<ProcessFilterKey, ProcessFilter>>> =
    OnceLock::new();

fn worktree_process_filters() -> &'static Mutex<HashMap<ProcessFilterKey, ProcessFilter>> {
    WORKTREE_PROCESS_FILTERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn list_available_filter_blobs(key: &ProcessFilterKey) -> Result<Vec<Vec<u8>>> {
    let mut filters = worktree_process_filters()
        .lock()
        .map_err(|_| CliError::Message("filter process lock is poisoned".to_owned()))?;
    let Some(process) = filters.get_mut(key) else {
        return Ok(Vec::new());
    };
    if process.aborted || !process.capabilities.contains("delay") {
        return Ok(Vec::new());
    }
    process.list_available_blobs()
}

fn run_worktree_process_filter(
    repo: &GitRepo,
    filter: &str,
    command: &str,
    relative: &[u8],
    direction: &str,
    metadata: &[String],
    content: &[u8],
) -> Result<Option<WorktreeFilterResult>> {
    let key = ProcessFilterKey {
        root: repo.root.clone(),
        filter: filter.to_owned(),
        command: command.to_owned(),
    };
    let mut filters = worktree_process_filters()
        .lock()
        .map_err(|_| CliError::Message("filter process lock is poisoned".to_owned()))?;
    let required = worktree_filter_required(repo, filter)?;
    if !filters.contains_key(&key) {
        let process = match start_worktree_process_filter(repo, command) {
            Ok(process) => process,
            Err(ProcessFilterStartError::Unavailable) if required => {
                return Err(worktree_filter_failed_error(filter, relative, direction));
            }
            Err(ProcessFilterStartError::Unavailable) => {
                eprintln!("error: external filter '{command}' failed");
                return Ok(Some(WorktreeFilterResult::Content(content.to_vec())));
            }
            Err(ProcessFilterStartError::Protocol(error)) => return Err(error),
        };
        filters.insert(key.clone(), process);
    }
    let delay_requested = metadata.iter().any(|item| item == "can-delay=1");
    let (status, delay_capable) = {
        let process = filters
            .get_mut(&key)
            .ok_or_else(|| CliError::Message("filter process disappeared".to_owned()))?;
        if process.aborted {
            if required {
                return Err(worktree_filter_failed_error(filter, relative, direction));
            }
            return Ok(Some(WorktreeFilterResult::Content(content.to_vec())));
        }
        if !process.capabilities.contains(direction) {
            return Ok(None);
        }
        (
            process.send_request(direction, relative, metadata, content),
            process.capabilities.contains("delay"),
        )
    };
    let status = match status {
        Ok(status) => status,
        Err(error) if process_filter_request_write_failed(&error) => {
            filters.remove(&key);
            if required {
                return Err(worktree_filter_failed_error(filter, relative, direction));
            }
            eprintln!("error: external filter '{command}' failed");
            return Ok(Some(WorktreeFilterResult::Content(content.to_vec())));
        }
        Err(error) => return Err(error),
    };
    match status {
        ProcessFilterResponse::Content(content) => Ok(Some(WorktreeFilterResult::Content(content))),
        ProcessFilterResponse::Delayed if delay_requested && delay_capable => {
            Ok(Some(WorktreeFilterResult::Delayed { key }))
        }
        ProcessFilterResponse::Delayed => {
            filters.remove(&key);
            if required {
                return Err(worktree_filter_protocol_failed_error(
                    command, filter, relative, direction,
                ));
            }
            eprintln!("error: external filter '{command}' failed");
            Ok(Some(WorktreeFilterResult::Content(content.to_vec())))
        }
        ProcessFilterResponse::InvalidStatus => {
            filters.remove(&key);
            if required {
                return Err(worktree_filter_protocol_failed_error(
                    command, filter, relative, direction,
                ));
            }
            eprintln!("error: external filter '{command}' failed");
            Ok(Some(WorktreeFilterResult::Content(content.to_vec())))
        }
        ProcessFilterResponse::Rejected { abort } => {
            if abort && let Some(process) = filters.get_mut(&key) {
                process.aborted = true;
            }
            if required {
                Err(worktree_filter_failed_error(filter, relative, direction))
            } else {
                Ok(Some(WorktreeFilterResult::Content(content.to_vec())))
            }
        }
    }
}

enum ProcessFilterStartError {
    Unavailable,
    Protocol(CliError),
}

enum ProcessFilterHandshakeError {
    NoResponse,
    Protocol(CliError),
}

fn start_worktree_process_filter(
    repo: &GitRepo,
    command: &str,
) -> std::result::Result<ProcessFilter, ProcessFilterStartError> {
    let mut child = ProcessCommand::new(git_shell_command_path())
        .arg("-c")
        .arg(command)
        .current_dir(&repo.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| ProcessFilterStartError::Unavailable)?;
    let stdin = child
        .stdin
        .take()
        .ok_or(ProcessFilterStartError::Unavailable)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(ProcessFilterStartError::Unavailable)?;
    let mut process = ProcessFilter {
        child,
        stdin: Some(stdin),
        stdout: io::BufReader::new(stdout),
        capabilities: HashSet::new(),
        aborted: false,
    };
    match process.handshake(command) {
        Ok(()) => {}
        Err(ProcessFilterHandshakeError::NoResponse) => {
            return Err(ProcessFilterStartError::Unavailable);
        }
        Err(ProcessFilterHandshakeError::Protocol(error)) => {
            return Err(ProcessFilterStartError::Protocol(error));
        }
    }
    Ok(process)
}

enum ProcessFilterResponse {
    Content(Vec<u8>),
    Delayed,
    InvalidStatus,
    Rejected { abort: bool },
}

fn worktree_filter_protocol_failed_error(
    command: &str,
    filter: &str,
    relative: &[u8],
    direction: &str,
) -> CliError {
    let filter_display = if direction == "clean" {
        format!("'{filter}'")
    } else {
        filter.to_owned()
    };
    CliError::Stderr {
        code: 128,
        text: format!(
            "error: external filter '{command}' failed\n\
             error: {command} died of signal 15\n\
             fatal: {}: {direction} filter {filter_display} failed\n",
            String::from_utf8_lossy(relative)
        ),
    }
}

fn process_filter_request_write_failed(error: &CliError) -> bool {
    matches!(
        error,
        CliError::Io(error)
            if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
            )
    )
}

fn process_filter_handshake_io_error(error: CliError) -> ProcessFilterHandshakeError {
    if process_filter_request_write_failed(&error) {
        ProcessFilterHandshakeError::NoResponse
    } else {
        ProcessFilterHandshakeError::Protocol(error)
    }
}

impl ProcessFilter {
    fn handshake(&mut self, command: &str) -> std::result::Result<(), ProcessFilterHandshakeError> {
        self.write_pkt_line(b"git-filter-client")
            .map_err(process_filter_handshake_io_error)?;
        self.write_pkt_line(b"version=2")
            .map_err(process_filter_handshake_io_error)?;
        self.write_flush()
            .map_err(process_filter_handshake_io_error)?;
        let server = self
            .read_pkt_line()
            .map_err(process_filter_handshake_io_error)?
            .ok_or(ProcessFilterHandshakeError::NoResponse)?;
        if process_filter_control_payload(&server) != b"git-filter-server" {
            return Err(ProcessFilterHandshakeError::Protocol(CliError::Fatal {
                code: 128,
                message: "expected git-filter-server".to_owned(),
            }));
        }
        self.expect_pkt_line(b"version=2", "expected filter protocol version=2")
            .map_err(ProcessFilterHandshakeError::Protocol)?;
        self.expect_flush("expected filter protocol version flush")
            .map_err(ProcessFilterHandshakeError::Protocol)?;

        for capability in ["clean", "smudge", "delay"] {
            self.write_pkt_line(format!("capability={capability}").as_bytes())
                .map_err(ProcessFilterHandshakeError::Protocol)?;
        }
        self.write_flush()
            .map_err(ProcessFilterHandshakeError::Protocol)?;
        while let Some(payload) = self
            .read_pkt_line()
            .map_err(ProcessFilterHandshakeError::Protocol)?
        {
            let line = process_filter_text_payload(&payload)
                .map_err(ProcessFilterHandshakeError::Protocol)?;
            let Some(capability) = line.strip_prefix("capability=") else {
                return Err(ProcessFilterHandshakeError::Protocol(CliError::Fatal {
                    code: 128,
                    message: format!("unexpected filter capability response '{line}'"),
                }));
            };
            match capability {
                "clean" | "smudge" | "delay" => {
                    self.capabilities.insert(capability.to_owned());
                }
                _ => {
                    return Err(ProcessFilterHandshakeError::Protocol(CliError::Fatal {
                        code: 128,
                        message: format!(
                            "subprocess '{command}' requested unsupported capability '{capability}'"
                        ),
                    }));
                }
            }
        }
        Ok(())
    }

    fn send_request(
        &mut self,
        direction: &str,
        relative: &[u8],
        metadata: &[String],
        content: &[u8],
    ) -> Result<ProcessFilterResponse> {
        self.write_pkt_line(format!("command={direction}").as_bytes())?;
        self.write_pkt_line(format!("pathname={}", String::from_utf8_lossy(relative)).as_bytes())?;
        let delay_capable = self.capabilities.contains("delay");
        for item in metadata
            .iter()
            .filter(|item| item.as_str() != "can-delay=1" || delay_capable)
        {
            self.write_pkt_line(item.as_bytes())?;
        }
        self.write_flush()?;
        self.write_packetized_content(content)?;
        let mut status = self.read_status_list()?.ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "filter process response did not include a status".to_owned(),
        })?;
        match status {
            ProcessFilterStatus::Success => {}
            ProcessFilterStatus::Error => {
                return Ok(ProcessFilterResponse::Rejected { abort: false });
            }
            ProcessFilterStatus::Abort => {
                return Ok(ProcessFilterResponse::Rejected { abort: true });
            }
            ProcessFilterStatus::Delayed => {
                return Ok(ProcessFilterResponse::Delayed);
            }
            ProcessFilterStatus::Invalid => {
                return Ok(ProcessFilterResponse::InvalidStatus);
            }
        }
        let filtered = self.read_packetized_content()?;
        if let Some(final_status) = self.read_status_list()? {
            status = final_status;
        }
        match status {
            ProcessFilterStatus::Success => Ok(ProcessFilterResponse::Content(filtered)),
            ProcessFilterStatus::Error => Ok(ProcessFilterResponse::Rejected { abort: false }),
            ProcessFilterStatus::Abort => Ok(ProcessFilterResponse::Rejected { abort: true }),
            ProcessFilterStatus::Delayed => Ok(ProcessFilterResponse::Delayed),
            ProcessFilterStatus::Invalid => Ok(ProcessFilterResponse::InvalidStatus),
        }
    }

    fn list_available_blobs(&mut self) -> Result<Vec<Vec<u8>>> {
        self.write_pkt_line(b"command=list_available_blobs")?;
        self.write_flush()?;
        let mut paths = Vec::new();
        while let Some(payload) = self.read_pkt_line()? {
            let line = process_filter_text_payload(&payload)?;
            let Some(pathname) = line.strip_prefix("pathname=") else {
                continue;
            };
            paths.push(pathname.as_bytes().to_vec());
        }
        match self.read_status_list()? {
            Some(ProcessFilterStatus::Success) => Ok(paths),
            Some(ProcessFilterStatus::Error | ProcessFilterStatus::Abort) => Ok(Vec::new()),
            Some(ProcessFilterStatus::Delayed) => Err(CliError::Fatal {
                code: 128,
                message: "filter process list_available_blobs returned delayed".to_owned(),
            }),
            Some(ProcessFilterStatus::Invalid) => Err(CliError::Fatal {
                code: 128,
                message: "filter process list_available_blobs returned invalid status".to_owned(),
            }),
            None => Err(CliError::Fatal {
                code: 128,
                message: "filter process list_available_blobs did not include a status".to_owned(),
            }),
        }
    }

    fn read_status_list(&mut self) -> Result<Option<ProcessFilterStatus>> {
        let mut status = None;
        while let Some(payload) = self.read_pkt_line()? {
            let line = process_filter_text_payload(&payload)?;
            let Some(value) = line.strip_prefix("status=") else {
                continue;
            };
            status = Some(match value {
                "success" => ProcessFilterStatus::Success,
                "error" => ProcessFilterStatus::Error,
                "abort" => ProcessFilterStatus::Abort,
                "delayed" => ProcessFilterStatus::Delayed,
                _ => ProcessFilterStatus::Invalid,
            });
        }
        Ok(status)
    }

    fn read_packetized_content(&mut self) -> Result<Vec<u8>> {
        let mut content = Vec::new();
        while let Some(payload) = self.read_pkt_line()? {
            content.extend_from_slice(&payload);
        }
        Ok(content)
    }

    fn write_packetized_content(&mut self, content: &[u8]) -> Result<()> {
        const MAX_FILTER_PKT_PAYLOAD: usize = 65_516;
        for chunk in content.chunks(MAX_FILTER_PKT_PAYLOAD) {
            self.write_pkt_line(chunk)?;
        }
        self.write_flush()
    }

    fn expect_pkt_line(&mut self, expected: &[u8], message: &'static str) -> Result<()> {
        let Some(payload) = self.read_pkt_line()? else {
            return Err(CliError::Fatal {
                code: 128,
                message: message.to_owned(),
            });
        };
        if process_filter_control_payload(&payload) == expected {
            Ok(())
        } else {
            Err(CliError::Fatal {
                code: 128,
                message: message.to_owned(),
            })
        }
    }

    fn expect_flush(&mut self, message: &'static str) -> Result<()> {
        if self.read_pkt_line()?.is_none() {
            Ok(())
        } else {
            Err(CliError::Fatal {
                code: 128,
                message: message.to_owned(),
            })
        }
    }

    fn write_pkt_line(&mut self, payload: &[u8]) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| CliError::Message("filter process stdin unavailable".to_owned()))?;
        write_process_filter_pkt_line(stdin, payload)
    }

    fn write_flush(&mut self) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| CliError::Message("filter process stdin unavailable".to_owned()))?;
        stdin.pipe_write_all_ignoring_sigpipe(b"0000")?;
        stdin.pipe_flush_ignoring_sigpipe()
    }

    fn read_pkt_line(&mut self) -> Result<Option<Vec<u8>>> {
        read_process_filter_pkt_line(&mut self.stdout)
    }
}

impl Drop for ProcessFilter {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

pub(crate) fn shutdown_worktree_filter_processes() -> Result<()> {
    let mut filters = worktree_process_filters()
        .lock()
        .map_err(|_| CliError::Message("filter process lock is poisoned".to_owned()))?;
    filters.clear();
    Ok(())
}

fn process_filter_text_payload(payload: &[u8]) -> Result<&str> {
    std::str::from_utf8(process_filter_control_payload(payload)).map_err(|_| CliError::Fatal {
        code: 128,
        message: "filter process sent non-utf8 control packet".to_owned(),
    })
}

fn process_filter_control_payload(payload: &[u8]) -> &[u8] {
    payload.strip_suffix(b"\n").unwrap_or(payload)
}

fn write_process_filter_pkt_line<W: Write>(writer: &mut W, payload: &[u8]) -> Result<()> {
    let len = payload
        .len()
        .checked_add(4)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "filter process pkt-line length overflow".to_owned(),
        })?;
    if len > 0xffff {
        return Err(CliError::Fatal {
            code: 128,
            message: "filter process pkt-line payload is too large".to_owned(),
        });
    }
    let mut header = [0_u8; 4];
    write_process_filter_pkt_len(&mut header, len);
    writer.pipe_write_all_ignoring_sigpipe(&header)?;
    writer.pipe_write_all_ignoring_sigpipe(payload)
}

trait PipeWriteExt: Write {
    fn pipe_write_all_ignoring_sigpipe(&mut self, buf: &[u8]) -> Result<()> {
        write_pipe_ignoring_sigpipe(|| self.write_all(buf)).map_err(CliError::Io)
    }

    fn pipe_flush_ignoring_sigpipe(&mut self) -> Result<()> {
        write_pipe_ignoring_sigpipe(|| self.flush()).map_err(CliError::Io)
    }
}

impl<T: Write + ?Sized> PipeWriteExt for T {}

#[cfg(unix)]
fn write_pipe_ignoring_sigpipe<F>(write: F) -> io::Result<()>
where
    F: FnOnce() -> io::Result<()>,
{
    unsafe {
        let previous = libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        let result = write();
        libc::signal(libc::SIGPIPE, previous);
        result
    }
}

#[cfg(not(unix))]
fn write_pipe_ignoring_sigpipe<F>(write: F) -> io::Result<()>
where
    F: FnOnce() -> io::Result<()>,
{
    write()
}

fn read_process_filter_pkt_line<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut header = [0_u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) => return Err(CliError::Io(error)),
    }
    let len = parse_process_filter_pkt_len(&header)?;
    if len == 0 {
        return Ok(None);
    }
    if len < 4 {
        return Err(CliError::Fatal {
            code: 128,
            message: "invalid filter process pkt-line length".to_owned(),
        });
    }
    let mut payload = vec![0_u8; len - 4];
    reader.read_exact(&mut payload).map_err(CliError::Io)?;
    Ok(Some(payload))
}

fn parse_process_filter_pkt_len(header: &[u8; 4]) -> Result<usize> {
    let mut len = 0_usize;
    for byte in header {
        let Some(value) = process_filter_hex_value(*byte) else {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid filter process pkt-line header".to_owned(),
            });
        };
        len = (len << 4) | usize::from(value);
    }
    Ok(len)
}

fn process_filter_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn write_process_filter_pkt_len(out: &mut [u8; 4], len: usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out[0] = HEX[(len >> 12) & 0x0f];
    out[1] = HEX[(len >> 8) & 0x0f];
    out[2] = HEX[(len >> 4) & 0x0f];
    out[3] = HEX[len & 0x0f];
}

#[cfg(unix)]
fn symlink_content_matches(path: &std::path::Path, entry: &IndexEntry) -> Result<bool> {
    use std::os::unix::ffi::OsStrExt;

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_symlink() {
        return Ok(false);
    }
    let target = fs::read_link(path)?;
    Ok(hash_object(
        GitHashAlgorithm::Sha1,
        GitObjectKind::Blob,
        target.as_os_str().as_bytes(),
    ) == entry.id)
}

#[cfg(not(unix))]
fn symlink_content_matches(path: &std::path::Path, entry: &IndexEntry) -> Result<bool> {
    let content = fs::read(path)?;
    Ok(hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content) == entry.id)
}

#[cfg(unix)]
fn symlink_content_matches_with_mode(
    path: &std::path::Path,
    entry: &IndexEntry,
    symlinks_enabled: bool,
) -> Result<bool> {
    use std::os::unix::ffi::OsStrExt;

    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(path)?;
        return Ok(hash_object(
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            target.as_os_str().as_bytes(),
        ) == entry.id);
    }
    if !symlinks_enabled && metadata.is_file() {
        let content = fs::read(path)?;
        return Ok(hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content) == entry.id);
    }
    Ok(false)
}

#[cfg(not(unix))]
fn symlink_content_matches_with_mode(
    path: &std::path::Path,
    entry: &IndexEntry,
    _symlinks_enabled: bool,
) -> Result<bool> {
    symlink_content_matches(path, entry)
}

#[cfg(unix)]
fn symlink_entry_modified_with_metadata(
    path: &std::path::Path,
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    symlinks_enabled: bool,
    stat_options: IndexStatOptions,
) -> Result<bool> {
    use std::os::unix::ffi::OsStrExt;

    if metadata.file_type().is_symlink() {
        if index_entry_stat_matches_with_options(metadata, entry, stat_options) {
            return Ok(false);
        }
        let target = fs::read_link(path)?;
        return Ok(hash_object(
            GitHashAlgorithm::Sha1,
            GitObjectKind::Blob,
            target.as_os_str().as_bytes(),
        ) != entry.id);
    }
    if !symlinks_enabled && metadata.is_file() {
        if index_entry_stat_matches_with_options(metadata, entry, stat_options) {
            return Ok(false);
        }
        let content = fs::read(path)?;
        return Ok(hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content) != entry.id);
    }
    if !metadata.file_type().is_symlink() {
        return Ok(true);
    }
    Ok(true)
}

#[cfg(not(unix))]
fn symlink_entry_modified_with_metadata(
    path: &std::path::Path,
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    _symlinks_enabled: bool,
    stat_options: IndexStatOptions,
) -> Result<bool> {
    if !index_entry_stat_matches_with_options(metadata, entry, stat_options) {
        return Ok(true);
    }
    let content = fs::read(path)?;
    Ok(hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content) != entry.id)
}

pub(crate) fn apply_index_entry_metadata(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    apply_index_entry_metadata_platform(entry, metadata);
}

#[derive(Clone, Copy)]
pub(crate) struct IndexTimestamp {
    seconds: u32,
    nanoseconds: u32,
}

pub(crate) fn repo_index_mtime(repo: &GitRepo) -> Result<Option<IndexTimestamp>> {
    match fs::symlink_metadata(&repo.index_path) {
        Ok(metadata) => Ok(Some(metadata_mtime_index_timestamp(&metadata))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn index_entry_stat_match_is_safe(
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    index_mtime: IndexTimestamp,
    options: IndexStatOptions,
) -> bool {
    index_entry_stat_matches_with_options(metadata, entry, options)
        && index_entry_mtime_older_than(entry, index_mtime)
}

fn index_entry_mtime_older_than(entry: &IndexEntry, timestamp: IndexTimestamp) -> bool {
    entry.mtime_seconds < timestamp.seconds
        || (entry.mtime_seconds == timestamp.seconds
            && entry.mtime_nanoseconds < timestamp.nanoseconds)
}

fn index_entry_has_stat_cache(entry: &IndexEntry) -> bool {
    entry.ctime_seconds != 0
        || entry.ctime_nanoseconds != 0
        || entry.mtime_seconds != 0
        || entry.mtime_nanoseconds != 0
        || entry.dev != 0
        || entry.ino != 0
        || entry.uid != 0
        || entry.gid != 0
        || entry.size != 0
}

#[cfg(unix)]
fn apply_index_entry_metadata_platform(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    use std::os::unix::fs::MetadataExt;

    entry.ctime_seconds = u32_from_i64_lossy(metadata.ctime());
    entry.ctime_nanoseconds = u32_from_i64_lossy(metadata.ctime_nsec());
    entry.mtime_seconds = u32_from_i64_lossy(metadata.mtime());
    entry.mtime_nanoseconds = u32_from_i64_lossy(metadata.mtime_nsec());
    entry.dev = u32_from_u64_lossy(metadata.dev());
    entry.ino = u32_from_u64_lossy(metadata.ino());
    entry.uid = metadata.uid();
    entry.gid = metadata.gid();
    entry.size = index_stat_size(metadata.len());
}

#[cfg(unix)]
fn metadata_mtime_index_timestamp(metadata: &fs::Metadata) -> IndexTimestamp {
    use std::os::unix::fs::MetadataExt;

    IndexTimestamp {
        seconds: u32_from_i64_lossy(metadata.mtime()),
        nanoseconds: u32_from_i64_lossy(metadata.mtime_nsec()),
    }
}

#[cfg(all(not(unix), not(windows)))]
fn apply_index_entry_metadata_platform(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    entry.size = index_stat_size(metadata.len());
}

#[cfg(all(not(unix), not(windows)))]
fn metadata_mtime_index_timestamp(_metadata: &fs::Metadata) -> IndexTimestamp {
    IndexTimestamp {
        seconds: 0,
        nanoseconds: 0,
    }
}

#[cfg(windows)]
fn apply_index_entry_metadata_platform(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    use std::os::windows::fs::MetadataExt;

    let (ctime_seconds, ctime_nanoseconds) =
        windows_filetime_to_index_time(metadata.creation_time());
    let (mtime_seconds, mtime_nanoseconds) =
        windows_filetime_to_index_time(metadata.last_write_time());
    entry.ctime_seconds = ctime_seconds;
    entry.ctime_nanoseconds = ctime_nanoseconds;
    entry.mtime_seconds = mtime_seconds;
    entry.mtime_nanoseconds = mtime_nanoseconds;
    entry.dev = 0;
    entry.ino = 0;
    entry.uid = 0;
    entry.gid = 0;
    entry.size = index_stat_size(metadata.file_size());
}

#[cfg(windows)]
fn metadata_mtime_index_timestamp(metadata: &fs::Metadata) -> IndexTimestamp {
    use std::os::windows::fs::MetadataExt;

    let (seconds, nanoseconds) = windows_filetime_to_index_time(metadata.last_write_time());
    IndexTimestamp {
        seconds,
        nanoseconds,
    }
}

pub(crate) fn index_entry_stat_matches(metadata: &fs::Metadata, entry: &IndexEntry) -> bool {
    index_entry_stat_matches_with_options(metadata, entry, IndexStatOptions::default())
}

pub(crate) fn index_entry_stat_matches_with_options(
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    options: IndexStatOptions,
) -> bool {
    index_entry_stat_matches_platform(metadata, entry, options)
}

#[cfg(unix)]
fn index_entry_stat_matches_platform(
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    options: IndexStatOptions,
) -> bool {
    use std::os::unix::fs::MetadataExt;

    entry.size == index_stat_size(metadata.len())
        && entry.mtime_seconds == u32_from_i64_lossy(metadata.mtime())
        && (!options.check_stat()
            || entry.mtime_nanoseconds == u32_from_i64_lossy(metadata.mtime_nsec()))
        && (!options.trust_ctime()
            || !options.check_stat()
            || (entry.ctime_seconds == u32_from_i64_lossy(metadata.ctime())
                && entry.ctime_nanoseconds == u32_from_i64_lossy(metadata.ctime_nsec())))
        // Upstream Git deliberately ignores st_dev unless it is compiled with
        // USE_STDEV because network filesystems can report a different device
        // to different clients. Mainstream macOS and Linux builds leave that
        // option disabled, so a stock-Git-created index must not be rejected
        // solely because its recorded device differs from Rust's metadata.
        && (!options.check_stat()
            || (entry.ino == u32_from_u64_lossy(metadata.ino())
                && entry.uid == metadata.uid()
                && entry.gid == metadata.gid()))
}

#[cfg(all(not(unix), not(windows)))]
fn index_entry_stat_matches_platform(
    _metadata: &fs::Metadata,
    _entry: &IndexEntry,
    _options: IndexStatOptions,
) -> bool {
    false
}

#[cfg(windows)]
fn index_entry_stat_matches_platform(
    metadata: &fs::Metadata,
    entry: &IndexEntry,
    options: IndexStatOptions,
) -> bool {
    use std::os::windows::fs::MetadataExt;

    let (ctime_seconds, ctime_nanoseconds) =
        windows_filetime_to_index_time(metadata.creation_time());
    let (mtime_seconds, mtime_nanoseconds) =
        windows_filetime_to_index_time(metadata.last_write_time());
    entry.size == index_stat_size(metadata.file_size())
        && entry.mtime_seconds == mtime_seconds
        && (!options.check_stat() || entry.mtime_nanoseconds == mtime_nanoseconds)
        && (!options.trust_ctime()
            || !options.check_stat()
            || (entry.ctime_seconds == ctime_seconds
                && entry.ctime_nanoseconds == ctime_nanoseconds))
        && (!options.check_stat() || entry.ino == 0)
}

#[cfg(windows)]
fn windows_filetime_to_index_time(filetime: u64) -> (u32, u32) {
    const WINDOWS_TICKS_PER_SECOND: u64 = 10_000_000;
    const WINDOWS_TO_UNIX_SECONDS: u64 = 11_644_473_600;

    let seconds = filetime / WINDOWS_TICKS_PER_SECOND;
    let unix_seconds = seconds.saturating_sub(WINDOWS_TO_UNIX_SECONDS);
    let nanoseconds = (filetime % WINDOWS_TICKS_PER_SECOND) * 100;
    (
        u32_from_u64_lossy(unix_seconds),
        u32_from_u64_lossy(nanoseconds),
    )
}

#[cfg(unix)]
fn u32_from_i64_lossy(value: i64) -> u32 {
    if value <= 0 { 0 } else { value as u32 }
}

#[cfg(any(unix, windows))]
fn u32_from_u64_lossy(value: u64) -> u32 {
    value as u32
}

fn index_stat_size(size: u64) -> u32 {
    let truncated = size as u32;
    if truncated == 0 && size != 0 {
        0x8000_0000
    } else {
        truncated
    }
}

pub(crate) fn path_exists(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
pub(crate) fn checkout_worktree(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
) -> Result<()> {
    checkout_worktree_with_metadata(repo, store, target_id, &WorktreeCheckoutMetadata::default())
}

pub(crate) fn checkout_worktree_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let target_commit = commit_cache.read_commit(target_id)?;
    let old_index = read_repo_index(repo)?;
    let mut new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
    let sparse_checkout = apply_repo_sparse_checkout_bits(repo, &mut new_index)?;

    remove_tracked_paths_missing_from_target(repo, &old_index, &new_index)?;
    if sparse_checkout {
        remove_newly_skipped_worktree_paths(repo, &old_index, &new_index)?;
    }
    let checkout_index_entries = sparse_checkout_checkout_index(&new_index)?;
    checkout_index(
        store,
        &checkout_index_entries,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    smudge_worktree_filter_entries_with_metadata(repo, &checkout_index_entries, metadata)?;
    refresh_tracked_index_metadata_after_checkout(repo, &mut new_index, &[])?;
    new_index.refresh_cache_tree();
    new_index.write_to_path(&repo.index_path)?;
    Ok(())
}

pub(crate) fn checkout_fresh_worktree(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
) -> Result<()> {
    checkout_fresh_worktree_inner(repo, store, target_id, true)
}

pub(crate) fn checkout_fresh_worktree_plain(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
) -> Result<()> {
    let _trace = phase_trace("checkout_fresh_worktree");
    let target_tree = {
        let _trace = phase_trace("checkout_fresh.read_commit_links");
        let commit_cache = CommitObjectCache::new(store);
        commit_cache.read_commit_links(target_id)?.tree.clone()
    };
    let _trace = phase_trace("checkout_fresh.read_tree_to_index");
    let mut new_index = read_tree_to_index_uncached(store, &target_tree)?;
    drop(_trace);
    let _trace = phase_trace("checkout_fresh.checkout_index");
    checkout_index(
        store,
        &new_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    drop(_trace);
    let _trace = phase_trace("checkout_fresh.write_index");
    refresh_tracked_index_metadata_after_checkout(repo, &mut new_index, &[])?;
    new_index.refresh_cache_tree();
    new_index.write_to_path(&repo.index_path)?;
    drop(_trace);
    let _trace = phase_trace("checkout_fresh.smudge_filters");
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let checkout_metadata = WorktreeCheckoutMetadata {
        ref_name: current_branch_ref(&refs)?,
        treeish: Some(target_id.clone()),
    };
    smudge_worktree_filter_entries_with_metadata(repo, &new_index, &checkout_metadata)?;
    Ok(())
}

fn checkout_fresh_worktree_inner(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    packed_first: bool,
) -> Result<()> {
    let _trace = phase_trace("checkout_fresh_worktree");
    let new_index = if packed_first {
        let checkout_store = store.packed_first();
        let target_tree = {
            let _trace = phase_trace("checkout_fresh.read_commit_links");
            let commit_cache = CommitObjectCache::new(&checkout_store);
            commit_cache.read_commit_links(target_id)?.tree.clone()
        };
        let _trace = phase_trace("checkout_fresh.read_tree_to_index");
        let new_index = read_tree_to_index_uncached(&checkout_store, &target_tree)?;
        drop(_trace);
        let _trace = phase_trace("checkout_fresh.checkout_index");
        let new_index = checkout_index_fresh_into_metadata(&checkout_store, new_index, &repo.root)?;
        drop(_trace);
        new_index
    } else {
        let target_tree = {
            let _trace = phase_trace("checkout_fresh.read_commit_links");
            let commit_cache = CommitObjectCache::new(store);
            commit_cache.read_commit_links(target_id)?.tree.clone()
        };
        let _trace = phase_trace("checkout_fresh.read_tree_to_index");
        let new_index = read_tree_to_index_uncached(store, &target_tree)?;
        drop(_trace);
        let _trace = phase_trace("checkout_fresh.checkout_index");
        let new_index = checkout_index_fresh_into_metadata(store, new_index, &repo.root)?;
        drop(_trace);
        new_index
    };
    let _trace = phase_trace("checkout_fresh.write_index");
    let mut new_index = new_index;
    new_index.refresh_cache_tree();
    new_index.write_to_path(&repo.index_path)?;
    drop(_trace);
    let _trace = phase_trace("checkout_fresh.smudge_filters");
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let checkout_metadata = WorktreeCheckoutMetadata {
        ref_name: current_branch_ref(&refs)?,
        treeish: Some(target_id.clone()),
    };
    smudge_worktree_filter_entries_with_metadata(repo, &new_index, &checkout_metadata)?;
    Ok(())
}

pub(crate) fn checkout_clean_worktree_transition(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
) -> Result<()> {
    checkout_clean_worktree_transition_with_metadata(
        repo,
        store,
        target_id,
        &WorktreeCheckoutMetadata::default(),
    )
}

pub(crate) fn checkout_clean_worktree_transition_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    checkout_clean_worktree_transition_inner(repo, store, target_id, metadata, false, true, false)
}

pub(crate) fn checkout_clean_worktree_replacement_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    checkout_clean_worktree_transition_inner(repo, store, target_id, metadata, false, false, false)
}

pub(crate) fn checkout_clean_missing_index_transition_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    checkout_clean_worktree_transition_inner(repo, store, target_id, metadata, false, false, true)
}

pub(crate) fn checkout_clean_worktree_transition_after_clean_check_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    checkout_clean_worktree_transition_inner(repo, store, target_id, metadata, true, true, false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CheckoutTransitionPathStatus {
    Added,
    Deleted,
    Modified,
}

struct CheckoutTransitionPathUpdate {
    status: CheckoutTransitionPathStatus,
    path: Vec<u8>,
}

struct CheckoutTransitionIndex {
    index: GitIndex,
    updates: Vec<CheckoutTransitionPathUpdate>,
}

fn checkout_clean_worktree_transition_inner(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
    preserve_unchanged_metadata: bool,
    preserve_index_changes: bool,
    protect_untracked_additions: bool,
) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let target_commit = commit_cache.read_commit(target_id)?;
    let old_index = read_repo_index(repo)?;
    let mut target_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
    let sparse_checkout = apply_repo_sparse_checkout_bits(repo, &mut target_index)?;
    let transition = if preserve_index_changes {
        let head_index = read_head_index_with_caches(repo, &commit_cache, &tree_cache)?;
        let sparse_index = config_bool_enabled(repo, "index.sparse")?;
        merge_checkout_transition_index(
            &head_index,
            &old_index,
            &target_index,
            sparse_checkout,
            sparse_index,
        )?
    } else {
        CheckoutTransitionIndex {
            index: target_index,
            updates: Vec::new(),
        }
    };
    let mut new_index = transition.index;
    if protect_untracked_additions {
        verify_checkout_untracked_additions(repo, &old_index, &new_index)?;
    }
    verify_checkout_transition_clean(repo, &old_index, &new_index)?;
    remove_tracked_paths_missing_from_target(repo, &old_index, &new_index)?;
    if sparse_checkout {
        remove_newly_skipped_worktree_paths(repo, &old_index, &new_index)?;
    }
    let mut sparse_warning_paths = Vec::new();
    let checkout_entries = changed_stage_zero_entries(&old_index, &new_index)
        .into_iter()
        .filter(|entry| {
            if entry.skip_worktree() {
                return false;
            }
            if sparse_checkout
                && old_index.entry(&entry.path, 0).is_none()
                && path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path))
            {
                sparse_warning_paths.push(entry.path.clone());
                return false;
            }
            true
        })
        .collect::<Vec<_>>();
    let checkout_paths = checkout_entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    if !checkout_entries.is_empty() {
        let checkout = GitIndex::from_entries(checkout_entries)?;
        checkout_index(
            store,
            &checkout,
            &repo.root,
            CheckoutIndexOptions { force: true },
        )?;
        smudge_worktree_filter_entries_with_metadata(repo, &checkout, metadata)?;
    }
    if !sparse_warning_paths.is_empty() {
        print_sparse_checkout_update_warning(&sparse_warning_paths);
    }
    if preserve_unchanged_metadata {
        preserve_unchanged_stage_zero_entries(&old_index, &mut new_index)?;
        if !checkout_paths.is_empty() {
            refresh_tracked_index_metadata_after_checkout(repo, &mut new_index, &checkout_paths)?;
        }
    } else {
        refresh_tracked_index_metadata_after_checkout(repo, &mut new_index, &[])?;
    }
    new_index.refresh_cache_tree();
    new_index.write_to_path(&repo.index_path)?;
    print_checkout_transition_path_updates(&transition.updates);
    Ok(())
}

fn merge_checkout_transition_index(
    head_index: &GitIndex,
    current_index: &GitIndex,
    target_index: &GitIndex,
    sparse_checkout: bool,
    sparse_index: bool,
) -> Result<CheckoutTransitionIndex> {
    let paths = head_index
        .entries()
        .iter()
        .chain(current_index.entries())
        .chain(target_index.entries())
        .filter(|entry| entry.stage == 0)
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::with_capacity(paths.len());
    let mut locally_changed = HashSet::new();
    let mut conflicts = Vec::new();
    let mut reported_changes = BTreeMap::new();

    for path in paths {
        let head = head_index.entry(&path, 0);
        let current = current_index.entry(&path, 0);
        let target = target_index.entry(&path, 0);
        let current_changed = !checkout_index_content_matches(current, head);
        let target_changed = !checkout_index_content_matches(target, head);
        if current_changed {
            locally_changed.insert(path.clone());
        }

        let selected = if !current_changed {
            target
        } else if !target_changed {
            if let Some(status) = checkout_transition_path_status(current, head) {
                reported_changes.insert(path.clone(), status);
            }
            current
        } else if checkout_index_content_matches(current, target) {
            current
        } else {
            conflicts.push(path.clone());
            None
        };
        if let Some(entry) = selected {
            entries.push(entry.clone());
        }
    }

    if !conflicts.is_empty() {
        return Err(checkout_overwrite_error(conflicts));
    }

    resolve_checkout_directory_file_transitions(
        head_index,
        target_index,
        &locally_changed,
        sparse_checkout,
        sparse_index,
        &mut entries,
        &mut reported_changes,
    );
    let updates = reported_changes
        .into_iter()
        .map(|(path, status)| CheckoutTransitionPathUpdate { status, path })
        .collect();
    Ok(CheckoutTransitionIndex {
        index: GitIndex::from_entries(entries)?,
        updates,
    })
}

fn checkout_transition_path_status(
    current: Option<&IndexEntry>,
    head: Option<&IndexEntry>,
) -> Option<CheckoutTransitionPathStatus> {
    match (current, head) {
        (Some(_), None) => Some(CheckoutTransitionPathStatus::Added),
        (None, Some(_)) => Some(CheckoutTransitionPathStatus::Deleted),
        (Some(_), Some(_)) => Some(CheckoutTransitionPathStatus::Modified),
        (None, None) => None,
    }
}

fn checkout_index_content_matches(left: Option<&IndexEntry>, right: Option<&IndexEntry>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.id == right.id && left.mode == right.mode,
        (None, None) => true,
        _ => false,
    }
}

fn resolve_checkout_directory_file_transitions(
    head_index: &GitIndex,
    target_index: &GitIndex,
    locally_changed: &HashSet<Vec<u8>>,
    sparse_checkout: bool,
    sparse_index: bool,
    entries: &mut Vec<IndexEntry>,
    reported_changes: &mut BTreeMap<Vec<u8>, CheckoutTransitionPathStatus>,
) {
    let target_files = target_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode != IndexMode::Tree)
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    for target_path in target_files {
        let mut descendant_prefix = target_path.clone();
        descendant_prefix.push(b'/');
        let local_descendants = entries
            .iter()
            .filter(|entry| {
                locally_changed.contains(&entry.path) && entry.path.starts_with(&descendant_prefix)
            })
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        if local_descendants.is_empty() {
            continue;
        }

        let has_adjacent_prefix_peer = head_index.entries().iter().any(|entry| {
            entry.path.starts_with(&target_path)
                && entry.path.len() > target_path.len()
                && entry.path[target_path.len()] != b'/'
        });
        let preserve_local_descendants =
            !sparse_index && (sparse_checkout || has_adjacent_prefix_peer);
        if preserve_local_descendants {
            entries.retain(|entry| entry.path != target_path);
            reported_changes.insert(target_path, CheckoutTransitionPathStatus::Deleted);
        } else {
            entries.retain(|entry| !local_descendants.contains(&entry.path));
            for path in local_descendants {
                reported_changes.remove(&path);
            }
        }
    }
}

fn print_checkout_transition_path_updates(updates: &[CheckoutTransitionPathUpdate]) {
    for update in updates {
        let status = match update.status {
            CheckoutTransitionPathStatus::Added => 'A',
            CheckoutTransitionPathStatus::Deleted => 'D',
            CheckoutTransitionPathStatus::Modified => 'M',
        };
        println!("{status}\t{}", String::from_utf8_lossy(&update.path));
    }
}

fn preserve_unchanged_stage_zero_entries(
    old_index: &GitIndex,
    new_index: &mut GitIndex,
) -> Result<()> {
    let preserved = new_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .filter_map(|entry| {
            old_index
                .entry(&entry.path, 0)
                .and_then(|old| (old.id == entry.id && old.mode == entry.mode).then(|| old.clone()))
        })
        .collect::<Vec<_>>();
    for entry in preserved {
        new_index.upsert(entry)?;
    }
    Ok(())
}

pub(crate) fn verify_checkout_transition_clean(
    repo: &GitRepo,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> Result<()> {
    let mut modified = Vec::new();
    for entry in old_index.entries().iter().filter(|entry| entry.stage == 0) {
        let target_entry = new_index.entry(&entry.path, 0);
        if target_entry.is_some_and(|target| target.id == entry.id && target.mode == entry.mode) {
            continue;
        }
        let path = worktree_path_for_index_entry(&repo.root, &entry.path);
        if path_exists(&path) && worktree_entry_modified(repo, &path, entry)? {
            modified.push(entry.path.clone());
        }
    }
    if modified.is_empty() {
        return Ok(());
    }
    Err(checkout_overwrite_error(modified))
}

fn verify_checkout_untracked_additions(
    repo: &GitRepo,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> Result<()> {
    let untracked = new_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && !entry.skip_worktree())
        .filter(|entry| old_index.entry(&entry.path, 0).is_none())
        .filter(|entry| path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path)))
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    if !untracked.is_empty() {
        return Err(checkout_untracked_overwrite_error(untracked));
    }
    Ok(())
}

fn checkout_untracked_overwrite_error(paths: Vec<Vec<u8>>) -> CliError {
    let mut text = String::from(
        "error: The following untracked working tree files would be overwritten by checkout:\n",
    );
    for path in paths {
        text.push('\t');
        text.push_str(&String::from_utf8_lossy(&path));
        text.push('\n');
    }
    text.push_str("Please move or remove them before you switch branches.\nAborting\n");
    CliError::Stderr { code: 1, text }
}

fn checkout_overwrite_error(paths: Vec<Vec<u8>>) -> CliError {
    let mut text = String::from(
        "error: Your local changes to the following files would be overwritten by checkout:\n",
    );
    for path in paths {
        text.push('\t');
        text.push_str(&String::from_utf8_lossy(&path));
        text.push('\n');
    }
    text.push_str(
        "Please commit your changes or stash them before you switch branches.\nAborting\n",
    );
    CliError::Stderr { code: 1, text }
}

pub(crate) fn checkout_worktree_updates_to_index_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    let mut checkout_entries = Vec::new();
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && !entry.skip_worktree())
    {
        let path = worktree_path_for_index_entry(&repo.root, &entry.path);
        if !path_exists(&path) || worktree_entry_modified(repo, &path, entry)? {
            checkout_entries.push(entry.clone());
        }
    }
    if checkout_entries.is_empty() {
        return Ok(());
    }
    let checkout = GitIndex::from_entries(checkout_entries)?;
    checkout_index(
        store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    smudge_worktree_filter_entries_with_metadata(repo, &checkout, metadata)?;
    Ok(())
}

fn changed_stage_zero_entries(old_index: &GitIndex, new_index: &GitIndex) -> Vec<IndexEntry> {
    let old_entries = old_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .map(|entry| (entry.path.as_slice(), entry))
        .collect::<HashMap<_, _>>();

    new_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .filter(|entry| {
            old_entries.get(entry.path.as_slice()).is_none_or(|old| {
                old.id != entry.id
                    || old.mode != entry.mode
                    || old.skip_worktree() != entry.skip_worktree()
            })
        })
        .cloned()
        .collect()
}

fn sparse_checkout_checkout_index(index: &GitIndex) -> Result<GitIndex> {
    Ok(GitIndex::from_entries(
        index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0 && !entry.skip_worktree())
            .cloned()
            .collect(),
    )?)
}

pub(crate) fn apply_repo_sparse_checkout_bits(repo: &GitRepo, index: &mut GitIndex) -> Result<bool> {
    if !repo_sparse_checkout_active(repo)? {
        return Ok(false);
    }
    let patterns = repo_sparse_checkout_patterns(repo)?;
    let cone_mode = config_bool_enabled(repo, "core.sparseCheckoutCone")?;
    let matcher = GitIgnore::parse(&patterns.join("\n"));
    let entries = index
        .entries()
        .iter()
        .cloned()
        .map(|mut entry| {
            if entry.stage == 0 {
                entry.set_skip_worktree(!repo_sparse_path_matches(
                    &entry.path,
                    &matcher,
                    cone_mode,
                ));
            }
            entry
        })
        .collect::<Vec<_>>();
    *index = GitIndex::from_entries(entries)?;
    Ok(true)
}

fn repo_sparse_checkout_active(repo: &GitRepo) -> Result<bool> {
    Ok(repo.git_dir.join("info/sparse-checkout").exists()
        && config_bool_enabled(repo, "core.sparseCheckout")?)
}

fn repo_sparse_checkout_patterns(repo: &GitRepo) -> Result<Vec<String>> {
    let raw = match fs::read_to_string(repo.git_dir.join("info/sparse-checkout")) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn repo_sparse_path_matches(path: &[u8], matcher: &GitIgnore, cone_mode: bool) -> bool {
    if cone_mode && !path.contains(&b'/') {
        return true;
    }
    repo_sparse_path_match(path, matcher, cone_mode).is_some_and(|(_, is_negation)| !is_negation)
}

fn repo_sparse_path_match(
    path: &[u8],
    matcher: &GitIgnore,
    cone_mode: bool,
) -> Option<(usize, bool)> {
    let mut best = matcher
        .match_path(path, false)
        .map(|matched| (matched.line_number, matched.is_negation));
    for ancestor in repo_sparse_path_ancestors(path) {
        let candidate = matcher.match_path(&ancestor, true).and_then(|matched| {
            if cone_mode && !matched.is_negation && matched.pattern == "/*" {
                None
            } else {
                Some((matched.line_number, matched.is_negation))
            }
        });
        if repo_sparse_match_is_newer(candidate.as_ref(), best.as_ref()) {
            best = candidate;
        }
    }
    best
}

fn repo_sparse_match_is_newer(
    candidate: Option<&(usize, bool)>,
    current: Option<&(usize, bool)>,
) -> bool {
    match (candidate, current) {
        (Some(candidate), Some(current)) => candidate.0 >= current.0,
        (Some(_), None) => true,
        _ => false,
    }
}

fn repo_sparse_path_ancestors(path: &[u8]) -> Vec<Vec<u8>> {
    path.iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'/')
        .map(|(index, _)| path[..index].to_vec())
        .collect()
}

fn remove_newly_skipped_worktree_paths(
    repo: &GitRepo,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> Result<()> {
    for entry in new_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.skip_worktree())
    {
        if old_index.entry(&entry.path, 0).is_some() {
            remove_worktree_path(repo, &entry.path)?;
        }
    }
    Ok(())
}

pub(crate) fn print_sparse_checkout_update_warning(paths: &[Vec<u8>]) {
    eprintln!(
        "warning: The following paths were already present and thus not updated despite sparse patterns:"
    );
    for path in paths {
        eprintln!("\t{}", String::from_utf8_lossy(path));
    }
    eprintln!();
    eprintln!("After fixing the above paths, you may want to run `git sparse-checkout reapply`.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> ObjectId {
        ObjectId::new(GitHashAlgorithm::Sha1, &[byte; 20])
    }

    fn index_entry(path: &str, byte: u8) -> IndexEntry {
        IndexEntry::new(path, oid(byte), IndexMode::File, 0).expect("index entry")
    }

    fn index_with_entries(entries: Vec<IndexEntry>) -> GitIndex {
        GitIndex::from_entries(entries).expect("index")
    }

    #[test]
    fn checkout_transition_preserves_staged_change_when_target_keeps_head_path() {
        let head = index_with_entries(vec![index_entry("file", 1)]);
        let current = index_with_entries(vec![index_entry("file", 2)]);
        let target = index_with_entries(vec![index_entry("file", 1), index_entry("other", 3)]);

        let transition = merge_checkout_transition_index(&head, &current, &target, false, false)
            .expect("merge checkout index");

        assert_eq!(transition.index.entry(b"file", 0).expect("file").id, oid(2));
        assert_eq!(
            transition.index.entry(b"other", 0).expect("other").id,
            oid(3)
        );
        assert_eq!(transition.updates.len(), 1);
        assert_eq!(transition.updates[0].path, b"file");
        assert_eq!(
            transition.updates[0].status,
            CheckoutTransitionPathStatus::Modified
        );
    }

    #[test]
    fn checkout_transition_rejects_staged_and_target_changes_to_same_path() {
        let head = index_with_entries(vec![index_entry("file", 1)]);
        let current = index_with_entries(vec![index_entry("file", 2)]);
        let target = index_with_entries(vec![index_entry("file", 3)]);

        let error = merge_checkout_transition_index(&head, &current, &target, false, false)
            .err()
            .expect("checkout conflict");

        assert!(matches!(
            error,
            CliError::Stderr { text, .. } if text.contains("file")
        ));
    }

    #[test]
    fn checkout_transition_matches_git_directory_file_ordering_and_sparse_modes() {
        let head_with_peer =
            index_with_entries(vec![index_entry("folder/a", 1), index_entry("folder-", 2)]);
        let current_with_peer = index_with_entries(vec![
            index_entry("folder/a", 1),
            index_entry("folder/local", 3),
            index_entry("folder-", 2),
        ]);
        let target_with_peer =
            index_with_entries(vec![index_entry("folder", 3), index_entry("folder-", 2)]);
        let ordered = merge_checkout_transition_index(
            &head_with_peer,
            &current_with_peer,
            &target_with_peer,
            false,
            false,
        )
        .expect("ordered D/F transition");
        assert!(ordered.index.entry(b"folder", 0).is_none());
        assert!(ordered.index.entry(b"folder/local", 0).is_some());
        assert_eq!(ordered.updates.len(), 2);

        let head = index_with_entries(vec![index_entry("folder/a", 1)]);
        let current = index_with_entries(vec![
            index_entry("folder/a", 1),
            index_entry("folder/local", 3),
        ]);
        let target = index_with_entries(vec![index_entry("folder", 3)]);
        let full = merge_checkout_transition_index(&head, &current, &target, false, false)
            .expect("full D/F transition");
        assert!(full.index.entry(b"folder", 0).is_some());
        assert!(full.index.entry(b"folder/local", 0).is_none());
        assert!(full.updates.is_empty());

        let sparse = merge_checkout_transition_index(&head, &current, &target, true, false)
            .expect("sparse D/F transition");
        assert!(sparse.index.entry(b"folder", 0).is_none());
        assert!(sparse.index.entry(b"folder/local", 0).is_some());
        assert_eq!(sparse.updates.len(), 2);

        let sparse_index = merge_checkout_transition_index(&head, &current, &target, true, true)
            .expect("sparse-index D/F transition");
        assert!(sparse_index.index.entry(b"folder", 0).is_some());
        assert!(sparse_index.index.entry(b"folder/local", 0).is_none());
        assert!(sparse_index.updates.is_empty());
    }

    #[test]
    fn index_stat_size_matches_git_32_bit_munging() {
        assert_eq!(index_stat_size(0), 0);
        assert_eq!(index_stat_size(u32::MAX as u64), u32::MAX);
        assert_eq!(index_stat_size(u32::MAX as u64 + 1), 0x8000_0000);
        assert_eq!(index_stat_size(u32::MAX as u64 + 2), 1);
    }

    #[test]
    fn zeroed_tree_index_entry_has_no_usable_stat_cache() {
        let mut entry =
            IndexEntry::new("tracked.txt", oid(1), IndexMode::File, 0).expect("index entry");
        assert!(!index_entry_has_stat_cache(&entry));

        entry.size = 1;
        assert!(index_entry_has_stat_cache(&entry));
    }

    #[test]
    fn sparse_patterns_match_root_files_like_git() {
        let full = GitIgnore::parse("/*\n");
        assert!(repo_sparse_path_matches(b"a", &full, false));
        assert!(repo_sparse_path_matches(b"nested/a", &full, false));

        let selective = GitIgnore::parse("!/*\n/a\n/c\n");
        assert!(repo_sparse_path_matches(b"a", &selective, false));
        assert!(!repo_sparse_path_matches(b"b", &selective, false));
        assert!(repo_sparse_path_matches(b"c", &selective, false));
    }

    #[cfg(unix)]
    #[test]
    fn index_stat_match_ignores_device_like_mainstream_git() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("tracked.txt");
        fs::write(&path, b"tracked\n").expect("write fixture");
        let metadata = fs::metadata(&path).expect("fixture metadata");
        let mut entry =
            IndexEntry::new("tracked.txt", oid(1), IndexMode::File, 0).expect("index entry");
        apply_index_entry_metadata(&mut entry, &metadata);
        entry.dev = entry.dev.wrapping_add(1);

        assert!(index_entry_stat_matches(&metadata, &entry));
    }

    #[test]
    fn next_index_position_after_path_skips_all_stages_for_path() {
        let mut conflict = IndexEntry::new("b.txt", oid(2), IndexMode::File, 0).expect("entry");
        conflict.stage = 2;
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("a.txt", oid(1), IndexMode::File, 0).expect("entry"),
            IndexEntry::new("b.txt", oid(3), IndexMode::File, 0).expect("entry"),
            conflict,
            IndexEntry::new("c.txt", oid(4), IndexMode::File, 0).expect("entry"),
        ])
        .expect("index");

        assert_eq!(next_index_position_after_path(&index, b"a.txt"), 1);
        assert_eq!(next_index_position_after_path(&index, b"b.txt"), 3);
        assert_eq!(next_index_position_after_path(&index, b"bb.txt"), 3);
    }

    #[test]
    fn content_rules_skip_smudge_only_when_checkout_cannot_rewrite_content() {
        let empty_binary_rules = WorktreeContentRules {
            attributes: GitAttributes::default(),
            core_autocrlf: CoreAutoCrlf::False,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(!empty_binary_rules.may_smudge_checkout_entries());

        let empty_input_rules = WorktreeContentRules {
            attributes: GitAttributes::default(),
            core_autocrlf: CoreAutoCrlf::Input,
            core_eol: CoreEol::Crlf,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(!empty_input_rules.may_smudge_checkout_entries());

        let autocrlf_rules = WorktreeContentRules {
            attributes: GitAttributes::default(),
            core_autocrlf: CoreAutoCrlf::True,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(autocrlf_rules.may_smudge_checkout_entries());

        let attribute_rules = WorktreeContentRules {
            attributes: GitAttributes::parse("*.txt text\n"),
            core_autocrlf: CoreAutoCrlf::False,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(attribute_rules.may_smudge_checkout_entries());
    }

    #[test]
    fn content_rules_allow_raw_hash_shortcut_only_for_crlf_only_paths() {
        let simple_input_rules = WorktreeContentRules {
            attributes: GitAttributes::default(),
            core_autocrlf: CoreAutoCrlf::Input,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(simple_input_rules.can_use_raw_blob_hash_when_no_cr(b"src/main.rs"));

        let ident_rules = WorktreeContentRules {
            attributes: GitAttributes::parse("*.rs ident\n"),
            core_autocrlf: CoreAutoCrlf::Input,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(!ident_rules.can_use_raw_blob_hash_when_no_cr(b"src/main.rs"));

        let filter_rules = WorktreeContentRules {
            attributes: GitAttributes::parse("*.rs filter=lfs\n"),
            core_autocrlf: CoreAutoCrlf::Input,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(!filter_rules.can_use_raw_blob_hash_when_no_cr(b"src/main.rs"));

        let binary_rules = WorktreeContentRules {
            attributes: GitAttributes::default(),
            core_autocrlf: CoreAutoCrlf::False,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };
        assert!(!binary_rules.can_use_raw_blob_hash_when_no_cr(b"src/main.rs"));
    }

    #[test]
    fn crlf_attribute_forces_crlf_checkout_even_without_core_eol_override() {
        let rules = WorktreeContentRules {
            attributes: GitAttributes::parse("t* crlf\n"),
            core_autocrlf: CoreAutoCrlf::False,
            core_eol: CoreEol::Unset,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };

        let smudged = rules
            .smudge_checkout_content(b"three", b"hello\n")
            .expect("smudge")
            .expect("content changed");
        assert_eq!(smudged, b"hello\r\n");
    }

    #[test]
    fn text_attribute_obeys_core_autocrlf_before_core_eol() {
        let rules = WorktreeContentRules {
            attributes: GitAttributes::parse("one text\n"),
            core_autocrlf: CoreAutoCrlf::True,
            core_eol: CoreEol::Lf,
            core_safecrlf: CoreSafeCrlf::False,
            roundtrip_encodings: None,
        };

        let smudged = rules
            .smudge_checkout_content(b"one", b"hello\n")
            .expect("smudge")
            .expect("content changed");
        assert_eq!(smudged, b"hello\r\n");
    }

    #[test]
    fn working_tree_encoding_utf16_and_utf32_roundtrip() {
        let utf16 = WorkingTreeEncoding::Utf16 {
            endian: UtfEndian::Little,
            bom: BomMode::Required,
        };
        let utf32 = WorkingTreeEncoding::Utf32 {
            endian: UtfEndian::Big,
            bom: BomMode::None,
        };
        let path = b"demo.txt";
        let original = "Test Тест\n".as_bytes();

        let utf16_bytes =
            encode_working_tree_encoding_content(path, &utf16, original).expect("encode utf16");
        let utf32_bytes =
            encode_working_tree_encoding_content(path, &utf32, original).expect("encode utf32");

        assert_eq!(
            decode_working_tree_encoding_content(path, &utf16, &utf16_bytes).expect("decode utf16"),
            original
        );
        assert_eq!(
            decode_working_tree_encoding_content(path, &utf32, &utf32_bytes).expect("decode utf32"),
            original
        );
    }

    #[test]
    fn working_tree_encoding_rejects_prohibited_and_missing_bom() {
        let path = b"demo.txt";
        let utf16be = WorkingTreeEncoding::Utf16 {
            endian: UtfEndian::Big,
            bom: BomMode::None,
        };
        let utf32 = WorkingTreeEncoding::Utf32 {
            endian: UtfEndian::Little,
            bom: BomMode::Required,
        };

        let utf16_with_bom = b"\xFE\xFF\0A".to_vec();
        let utf32_without_bom = b"A\0\0\0".to_vec();

        let err = decode_working_tree_encoding_content(path, &utf16be, &utf16_with_bom)
            .expect_err("utf16 bom should be rejected");
        assert!(format!("{err:?}").contains("BOM is prohibited"));

        let err = decode_working_tree_encoding_content(path, &utf32, &utf32_without_bom)
            .expect_err("utf32 missing bom should fail");
        assert!(format!("{err:?}").contains("BOM is required"));
    }
}
