use super::*;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use zmin_git_core::GitObjectStore;
use zmin_primitives::Error as PrimitiveError;
use zmin_primitives::git_runtime::GitRefsStore;

const SMALL_WORKTREE_BLOB_READ_BYTES: usize = 64 * 1024;
const PARALLEL_STAGE_REGULAR_MIN_FILES: usize = 128;
const PARALLEL_STAGE_REGULAR_MAX_WORKERS: usize = 4;

pub(crate) fn read_repo_index(repo: &GitRepo) -> Result<GitIndex> {
    if repo.index_path.exists() {
        read_index(&repo.index_path).map_err(map_read_index_error)
    } else {
        Ok(GitIndex::new())
    }
}

fn map_read_index_error(error: std::io::Error) -> CliError {
    if let Some(version) = bad_index_version(&error) {
        return CliError::Stderr {
            code: 128,
            text: format!("error: bad index version {version}\nfatal: index file corrupt\n"),
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

pub(crate) fn stage_tracked_worktree_changes(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
) -> Result<()> {
    stage_tracked_worktree_changes_matching(repo, store, index, &[], &HashSet::new())
}

pub(crate) fn stage_tracked_worktree_changes_matching(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    pathspecs: &[Vec<u8>],
    already_staged: &HashSet<Vec<u8>>,
) -> Result<()> {
    let index_mtime = repo_index_mtime(repo)?;
    let stage_options = WorktreeStageOptions::load(repo)?;
    let mut trace = TrackedWorktreeTrace::new();
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
        let absolute = worktree_path_for_index_entry(&repo.root, &path);
        let metadata_started = trace.started();
        let metadata = match fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(_) => {
                trace.record_metadata(metadata_started);
                trace.deleted += 1;
                index.remove_path(&path)?;
                entry_idx = next_index_position_after_path(index, &path);
                continue;
            }
        };
        trace.record_metadata(metadata_started);
        if metadata.is_dir() && index.entries()[entry_idx].mode != IndexMode::Gitlink {
            trace.removed_dirs += 1;
            index.remove_path(&path)?;
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
        }
        entry_idx = next_index_position_after_path(index, &path);
    }
    trace.emit();
    Ok(())
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
        if !self.enabled {
            return;
        }
        phase_trace_emit(
            "add.stage_tracked.detail",
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
                metadata.is_file()
                    && hash_worktree_file_blob(&absolute, metadata.len())? == entry.id
            }
            IndexMode::Symlink => symlink_content_matches(&absolute, &entry)?,
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
    worktree_diff_index_snapshot_with_missing(repo, index, false)
}

pub(crate) fn worktree_diff_index_snapshot_with_missing(
    repo: &GitRepo,
    index: &GitIndex,
    keep_missing: bool,
) -> Result<GitIndex> {
    worktree_index_snapshot_with_missing(repo, index, keep_missing)
}

pub(crate) fn worktree_stat_dirty_diff_entries(
    repo: &GitRepo,
    index: &GitIndex,
) -> Result<Vec<IndexDiffEntry>> {
    let mut entries = Vec::new();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        let Ok(metadata) = fs::symlink_metadata(&absolute) else {
            continue;
        };
        if entry.mode == IndexMode::Gitlink
            || !(metadata.is_file() || metadata.file_type().is_symlink())
            || index_entry_stat_matches(&metadata, entry)
        {
            continue;
        }
        let worktree_entry = worktree_index_entry_for_existing_entry(repo, &absolute, entry)?;
        if worktree_entry.id == entry.id && worktree_entry.mode == entry.mode {
            entries.push(IndexDiffEntry {
                status: IndexDiffStatus::Modified,
                path: entry.path.clone(),
                old_path: None,
                similarity: None,
            });
        }
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

pub(crate) fn collect_add_files(
    root: &std::path::Path,
    path: &std::path::Path,
    ignore: &GitIgnore,
    force: bool,
    files: &mut Vec<PathBuf>,
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
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            collect_add_files(root, &entry.path(), ignore, force, files)?;
        }
    } else if metadata.is_file() || metadata.file_type().is_symlink() {
        files.push(path.to_path_buf());
    }
    Ok(())
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

pub(crate) fn stage_intent_to_add_file(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &std::path::Path,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let relative = repo_relative_path(&repo.root, path)?;
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
        if stage_options.needs_content_conversion(relative) {
            return Ok(false);
        }
        if let Some(existing) = find_index_entry(index, relative) {
            if index_mtime
                .is_some_and(|mtime| index_entry_stat_match_is_safe(&metadata, existing, mtime))
            {
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
                        "error: '{display}/' does not have a commit checked out\nerror: unable to index file '{display}/'\nfatal: adding files failed\n"
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
        && index_mtime
            .is_some_and(|mtime| index_entry_stat_match_is_safe(&metadata, existing, mtime))
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
        && stage_options.needs_content_conversion(&relative)
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
    remove_index_parent_file_entries(index, &relative)?;
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

fn remove_index_parent_file_entries(index: &mut GitIndex, path: &[u8]) -> Result<()> {
    for (idx, byte) in path.iter().enumerate() {
        if *byte == b'/' {
            index.remove_path(&path[..idx])?;
        }
    }
    Ok(())
}

pub(crate) fn repo_object_format(repo: &GitRepo) -> Result<GitHashAlgorithm> {
    let Some(entry) = read_local_config_entries(repo)?
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
        value => Err(CliError::Fatal {
            code: 128,
            message: format!("unsupported object format '{value}'"),
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
    let commit = commit_cache.read_commit(&head)?;
    Ok(tree_cache.read_tree_to_index(&commit.tree)?)
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
    Ok(Some(commit_cache.read_commit(&head)?.tree.clone()))
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
    let commit = commit_cache.read_commit(&head)?;
    Ok(tree_cache.read_tree_to_index(&commit.tree)?)
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
    for entry in index.entries() {
        if entry.stage != 0 {
            return Err(CliError::Message(
                "status cannot inspect an index with unresolved conflicts".into(),
            ));
        }
        let path = worktree_path_for_index_entry(&repo.root, &entry.path);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                statuses.push((entry.path.to_vec(), 'D'));
                continue;
            }
        };
        if worktree_entry_modified_with_metadata(
            repo,
            &path,
            &metadata,
            entry,
            index_mtime,
            &stage_options,
            None,
        )? {
            statuses.push((entry.path.to_vec(), 'M'));
        }
    }
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
            if index_mtime
                .is_some_and(|mtime| index_entry_stat_match_is_safe(metadata, entry, mtime))
            {
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
            symlink_entry_modified_with_metadata(path, metadata, entry)
        }
        IndexMode::Gitlink => {
            if let Some(trace) = trace.as_deref_mut() {
                trace.gitlink_checks += 1;
            }
            Ok(!path.is_dir())
        }
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
            if index_mtime
                .is_some_and(|mtime| index_entry_stat_match_is_safe(&metadata, entry, mtime))
            {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.stat_safe += 1;
                }
                return Ok(false);
            }
            if stage_options.needs_content_conversion(&entry.path) {
                let started = trace.as_ref().and_then(|trace| trace.started());
                let content =
                    stage_options.clean_worktree_content(repo, &entry.path, fs::read(path)?)?;
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
            symlink_entry_modified_with_metadata(path, metadata, entry)
        }
        IndexMode::Gitlink => {
            if let Some(trace) = trace.as_deref_mut() {
                trace.gitlink_checks += 1;
            }
            Ok(!path.is_dir())
        }
    }
}

pub(crate) struct WorktreeStageOptions {
    content_rules: WorktreeContentRules,
    filemode_enabled: bool,
    symlinks_enabled: bool,
}

impl WorktreeStageOptions {
    pub(crate) fn load(repo: &GitRepo) -> Result<Self> {
        Ok(Self {
            content_rules: WorktreeContentRules::load(repo)?,
            filemode_enabled: repo_filemode_enabled(repo)?,
            symlinks_enabled: repo_symlinks_enabled(repo)?,
        })
    }

    fn index_mode_for_metadata(&self, metadata: &fs::Metadata) -> IndexMode {
        if self.filemode_enabled {
            index_mode_for_metadata(metadata)
        } else {
            IndexMode::File
        }
    }

    fn filemode_enabled(&self) -> bool {
        self.filemode_enabled
    }

    fn symlinks_enabled(&self) -> bool {
        self.symlinks_enabled
    }

    fn needs_content_conversion(&self, relative: &[u8]) -> bool {
        self.content_rules.needs_content_conversion(relative)
    }

    fn clean_staged_worktree_content(
        &self,
        repo: &GitRepo,
        store: &LooseObjectStore,
        index: &GitIndex,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.content_rules
            .clean_staged_worktree_content(repo, store, index, relative, content)
    }

    fn clean_worktree_content(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.content_rules
            .clean_worktree_content(repo, relative, content)
    }

    fn smudge_checkout_content(&self, relative: &[u8], content: &[u8]) -> Option<Vec<u8>> {
        self.content_rules
            .smudge_checkout_content(relative, content)
    }

    fn may_smudge_checkout_entries(&self) -> bool {
        self.content_rules.may_smudge_checkout_entries()
    }

    fn attributes(&self) -> &GitAttributes {
        &self.content_rules.attributes
    }
}

pub(crate) struct WorktreeContentRules {
    attributes: GitAttributes,
    core_autocrlf: CoreAutoCrlf,
    core_eol: CoreEol,
}

impl WorktreeContentRules {
    pub(crate) fn load(repo: &GitRepo) -> Result<Self> {
        let entries = read_config_entries(repo)?;
        Ok(Self {
            attributes: GitAttributes::load_from_root(&repo.root)?,
            core_autocrlf: core_autocrlf_from_config(&entries),
            core_eol: core_eol_from_config(&entries),
        })
    }

    fn needs_content_conversion(&self, relative: &[u8]) -> bool {
        self.attributes.is_set(relative, "ident")
            || self.crlf_action(relative) != CrlfAction::Binary
            || worktree_filter_name(&self.attributes, relative).is_some()
    }

    fn clean_staged_worktree_content(
        &self,
        repo: &GitRepo,
        store: &LooseObjectStore,
        index: &GitIndex,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.clean_worktree_content_inner(repo, Some((store, index)), relative, content, true)
    }

    fn clean_worktree_content(
        &self,
        repo: &GitRepo,
        relative: &[u8],
        content: Vec<u8>,
    ) -> Result<Vec<u8>> {
        self.clean_worktree_content_inner(repo, None, relative, content, false)
    }

    fn clean_worktree_content_inner(
        &self,
        repo: &GitRepo,
        index_context: Option<(&LooseObjectStore, &GitIndex)>,
        relative: &[u8],
        content: Vec<u8>,
        emit_warnings: bool,
    ) -> Result<Vec<u8>> {
        let content = if self.attributes.is_set(relative, "ident") {
            apply_ident_clean(&content)
        } else {
            content
        };
        let content = self.clean_crlf_content(index_context, relative, content, emit_warnings)?;
        apply_worktree_filter(repo, &self.attributes, relative, "clean", &[], content)
    }

    fn clean_crlf_content(
        &self,
        index_context: Option<(&LooseObjectStore, &GitIndex)>,
        relative: &[u8],
        content: Vec<u8>,
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
            emit_crlf_roundtrip_warning(relative, action, &stats, &new_stats);
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

    fn smudge_checkout_content(&self, relative: &[u8], content: &[u8]) -> Option<Vec<u8>> {
        let action = self.crlf_action(relative);
        if !action.output_crlf() || content.is_empty() {
            return None;
        }
        let stats = CrlfStats::gather(&content);
        if stats.lonelf == 0
            || (action.is_auto() && (stats.lonecr > 0 || stats.crlf > 0 || stats.is_binary()))
        {
            return None;
        }
        Some(zmin_git_core::apply_eol_smudge_to_crlf(content))
    }

    fn may_smudge_checkout_entries(&self) -> bool {
        !self.attributes.is_empty() || self.core_autocrlf == CoreAutoCrlf::True
    }

    fn crlf_action(&self, relative: &[u8]) -> CrlfAction {
        crlf_action_for_path(
            &self.attributes,
            relative,
            self.core_autocrlf,
            self.core_eol,
        )
    }
}

pub(crate) fn clean_worktree_content(
    repo: &GitRepo,
    relative: &[u8],
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    WorktreeContentRules::load(repo)?.clean_worktree_content(repo, relative, content)
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
    if !content_rules.may_smudge_checkout_entries() {
        return Ok(());
    }
    let attributes = content_rules.attributes();
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
    if let Some(smudged) = options.smudge_checkout_content(&entry.path, &content) {
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

pub(crate) fn smudge_worktree_content(
    repo: &GitRepo,
    relative: &[u8],
    blob_id: &ObjectId,
    checkout_metadata: &WorktreeCheckoutMetadata,
    content: Vec<u8>,
) -> Result<Vec<u8>> {
    let rules = WorktreeContentRules::load(repo)?;
    let content = rules
        .smudge_checkout_content(relative, &content)
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
            Some(AttributeValue::Set) => Some(text_action(core_autocrlf, core_eol)),
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
    if text_eol_is_crlf(core_autocrlf, core_eol) {
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

fn emit_crlf_roundtrip_warning(
    relative: &[u8],
    action: CrlfAction,
    old_stats: &CrlfStats,
    new_stats: &CrlfStats,
) {
    let path = String::from_utf8_lossy(relative);
    let lf_to_crlf = old_stats.lonelf > 0 && new_stats.lonelf == 0;
    let crlf_to_lf = old_stats.crlf > 0 && new_stats.crlf == 0;
    if action.output_crlf() && lf_to_crlf {
        eprintln!(
            "warning: in the working copy of '{path}', LF will be replaced by CRLF the next time Git touches it"
        );
    } else if crlf_to_lf {
        eprintln!(
            "warning: in the working copy of '{path}', CRLF will be replaced by LF the next time Git touches it"
        );
    } else if lf_to_crlf {
        eprintln!(
            "warning: in the working copy of '{path}', LF will be replaced by CRLF the next time Git touches it"
        );
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
        .write_all(&content);
    drop(child.stdin.take());
    let output = child.wait_with_output().map_err(CliError::Io)?;
    if output.status.success() {
        if let Err(error) = write_result
            && error.kind() != io::ErrorKind::BrokenPipe
        {
            return Err(CliError::Io(error));
        }
        Ok(output.stdout)
    } else {
        Err(CliError::Stderr {
            code: output.status.code().unwrap_or(1),
            text: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
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
    if !filters.contains_key(&key) {
        let process = start_worktree_process_filter(repo, command)?;
        filters.insert(key.clone(), process);
    }
    let required = worktree_filter_required(repo, filter)?;
    let status = {
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
        process.send_request(direction, relative, metadata, content)
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
        ProcessFilterResponse::Delayed => Ok(Some(WorktreeFilterResult::Delayed { key })),
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

fn start_worktree_process_filter(repo: &GitRepo, command: &str) -> Result<ProcessFilter> {
    let mut child = ProcessCommand::new(git_shell_command_path())
        .arg("-c")
        .arg(command)
        .current_dir(&repo.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(CliError::Io)?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| CliError::Message("filter process stdin unavailable".to_owned()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::Message("filter process stdout unavailable".to_owned()))?;
    let mut process = ProcessFilter {
        child,
        stdin: Some(stdin),
        stdout: io::BufReader::new(stdout),
        capabilities: HashSet::new(),
        aborted: false,
    };
    process.handshake()?;
    Ok(process)
}

enum ProcessFilterResponse {
    Content(Vec<u8>),
    Delayed,
    Rejected { abort: bool },
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

impl ProcessFilter {
    fn handshake(&mut self) -> Result<()> {
        self.write_pkt_line(b"git-filter-client")?;
        self.write_pkt_line(b"version=2")?;
        self.write_flush()?;
        self.expect_pkt_line(b"git-filter-server", "expected git-filter-server")?;
        self.expect_pkt_line(b"version=2", "expected filter protocol version=2")?;
        self.expect_flush("expected filter protocol version flush")?;

        for capability in ["clean", "smudge", "delay"] {
            self.write_pkt_line(format!("capability={capability}").as_bytes())?;
        }
        self.write_flush()?;
        while let Some(payload) = self.read_pkt_line()? {
            let line = process_filter_text_payload(&payload)?;
            let Some(capability) = line.strip_prefix("capability=") else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("unexpected filter capability response '{line}'"),
                });
            };
            match capability {
                "clean" | "smudge" | "delay" => {
                    self.capabilities.insert(capability.to_owned());
                }
                _ => {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: format!("unsupported filter capability '{capability}'"),
                    });
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
        for item in metadata {
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
                _ => {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: format!("unsupported filter process status '{value}'"),
                    });
                }
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
        stdin.write_all(b"0000").map_err(CliError::Io)?;
        stdin.flush().map_err(CliError::Io)
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
    writer.write_all(&header).map_err(CliError::Io)?;
    writer.write_all(payload).map_err(CliError::Io)
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
fn symlink_entry_modified_with_metadata(
    path: &std::path::Path,
    metadata: &fs::Metadata,
    entry: &IndexEntry,
) -> Result<bool> {
    use std::os::unix::ffi::OsStrExt;

    if !metadata.file_type().is_symlink() {
        return Ok(true);
    }
    if index_entry_stat_matches(&metadata, entry) {
        return Ok(false);
    }
    let target = fs::read_link(path)?;
    Ok(hash_object(
        GitHashAlgorithm::Sha1,
        GitObjectKind::Blob,
        target.as_os_str().as_bytes(),
    ) != entry.id)
}

#[cfg(not(unix))]
fn symlink_entry_modified_with_metadata(
    path: &std::path::Path,
    metadata: &fs::Metadata,
    entry: &IndexEntry,
) -> Result<bool> {
    if !index_entry_stat_matches(&metadata, entry) {
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
) -> bool {
    index_entry_stat_matches(metadata, entry) && index_entry_mtime_older_than(entry, index_mtime)
}

fn index_entry_mtime_older_than(entry: &IndexEntry, timestamp: IndexTimestamp) -> bool {
    entry.mtime_seconds < timestamp.seconds
        || (entry.mtime_seconds == timestamp.seconds
            && entry.mtime_nanoseconds < timestamp.nanoseconds)
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
    entry.size = metadata.len().min(u32::MAX as u64) as u32;
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
    entry.size = metadata.len().min(u32::MAX as u64) as u32;
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
    entry.size = metadata.file_size().min(u32::MAX as u64) as u32;
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
    index_entry_stat_matches_platform(metadata, entry)
}

#[cfg(unix)]
fn index_entry_stat_matches_platform(metadata: &fs::Metadata, entry: &IndexEntry) -> bool {
    use std::os::unix::fs::MetadataExt;

    entry.ctime_seconds != 0
        && entry.mtime_seconds != 0
        && entry.size == metadata.len().min(u32::MAX as u64) as u32
        && entry.ctime_seconds == u32_from_i64_lossy(metadata.ctime())
        && entry.ctime_nanoseconds == u32_from_i64_lossy(metadata.ctime_nsec())
        && entry.mtime_seconds == u32_from_i64_lossy(metadata.mtime())
        && entry.mtime_nanoseconds == u32_from_i64_lossy(metadata.mtime_nsec())
        && entry.dev == u32_from_u64_lossy(metadata.dev())
        && entry.ino == u32_from_u64_lossy(metadata.ino())
        && entry.uid == metadata.uid()
        && entry.gid == metadata.gid()
}

#[cfg(all(not(unix), not(windows)))]
fn index_entry_stat_matches_platform(_metadata: &fs::Metadata, _entry: &IndexEntry) -> bool {
    false
}

#[cfg(windows)]
fn index_entry_stat_matches_platform(metadata: &fs::Metadata, entry: &IndexEntry) -> bool {
    use std::os::windows::fs::MetadataExt;

    let (ctime_seconds, ctime_nanoseconds) =
        windows_filetime_to_index_time(metadata.creation_time());
    let (mtime_seconds, mtime_nanoseconds) =
        windows_filetime_to_index_time(metadata.last_write_time());
    entry.ctime_seconds != 0
        && entry.mtime_seconds != 0
        && entry.size == metadata.file_size().min(u32::MAX as u64) as u32
        && entry.ctime_seconds == ctime_seconds
        && entry.ctime_nanoseconds == ctime_nanoseconds
        && entry.mtime_seconds == mtime_seconds
        && entry.mtime_nanoseconds == mtime_nanoseconds
        && entry.dev == 0
        && entry.ino == 0
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
    let old_index = read_head_index_with_caches(repo, &commit_cache, &tree_cache)?;
    let mut new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;

    remove_tracked_paths_missing_from_target(repo, &old_index, &new_index)?;
    checkout_index(
        store,
        &new_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    smudge_worktree_filter_entries_with_metadata(repo, &new_index, metadata)?;
    refresh_tracked_index_metadata_matching(repo, &mut new_index, &[])?;
    new_index.write_to_path(&repo.index_path)?;
    Ok(())
}

pub(crate) fn checkout_fresh_worktree(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
) -> Result<()> {
    let _trace = phase_trace("checkout_fresh_worktree");
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
    let _trace = phase_trace("checkout_fresh.write_index");
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
    checkout_clean_worktree_transition_inner(repo, store, target_id, metadata, false)
}

pub(crate) fn checkout_clean_worktree_transition_after_clean_check_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    checkout_clean_worktree_transition_inner(repo, store, target_id, metadata, true)
}

fn checkout_clean_worktree_transition_inner(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
    metadata: &WorktreeCheckoutMetadata,
    preserve_unchanged_metadata: bool,
) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let target_commit = commit_cache.read_commit(target_id)?;
    let old_index = read_repo_index(repo)?;
    let mut new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
    verify_checkout_transition_clean(repo, &old_index, &new_index)?;
    remove_tracked_paths_missing_from_target(repo, &old_index, &new_index)?;

    let checkout_entries = changed_stage_zero_entries(&old_index, &new_index);
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
    if preserve_unchanged_metadata {
        preserve_unchanged_stage_zero_entries(&old_index, &mut new_index)?;
        if !checkout_paths.is_empty() {
            refresh_tracked_index_metadata_matching(repo, &mut new_index, &checkout_paths)?;
        }
    } else {
        refresh_tracked_index_metadata_matching(repo, &mut new_index, &[])?;
    }
    new_index.write_to_path(&repo.index_path)?;
    Ok(())
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

fn verify_checkout_transition_clean(
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
    let mut text = String::from(
        "error: Your local changes to the following files would be overwritten by checkout:\n",
    );
    for path in modified {
        text.push('\t');
        text.push_str(&String::from_utf8_lossy(&path));
        text.push('\n');
    }
    text.push_str(
        "Please commit your changes or stash them before you switch branches.\nAborting\n",
    );
    Err(CliError::Stderr { code: 1, text })
}

pub(crate) fn checkout_worktree_updates_to_index_with_metadata(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    metadata: &WorktreeCheckoutMetadata,
) -> Result<()> {
    let mut checkout_entries = Vec::new();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
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
            old_entries
                .get(entry.path.as_slice())
                .is_none_or(|old| old.id != entry.id || old.mode != entry.mode)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> ObjectId {
        ObjectId::new(GitHashAlgorithm::Sha1, &[byte; 20])
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
        };
        assert!(!empty_binary_rules.may_smudge_checkout_entries());

        let empty_input_rules = WorktreeContentRules {
            attributes: GitAttributes::default(),
            core_autocrlf: CoreAutoCrlf::Input,
            core_eol: CoreEol::Crlf,
        };
        assert!(!empty_input_rules.may_smudge_checkout_entries());

        let autocrlf_rules = WorktreeContentRules {
            attributes: GitAttributes::default(),
            core_autocrlf: CoreAutoCrlf::True,
            core_eol: CoreEol::Unset,
        };
        assert!(autocrlf_rules.may_smudge_checkout_entries());

        let attribute_rules = WorktreeContentRules {
            attributes: GitAttributes::parse("*.txt text\n"),
            core_autocrlf: CoreAutoCrlf::False,
            core_eol: CoreEol::Unset,
        };
        assert!(attribute_rules.may_smudge_checkout_entries());
    }
}
