use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::attributes::{
    AttributeValue, GitAttributes, apply_eol_smudge_to_crlf, apply_ident_smudge,
};
use crate::index::{GitIndex, IndexEntry, IndexMode};
use crate::loose::LooseObject;
use crate::object::{GitObjectKind, ObjectId};
use crate::object_store::{GitObjectStore, ObjectStorageHint};

const STREAM_CHECKOUT_BLOB_MIN_BYTES: usize = 1024 * 1024;
#[cfg(unix)]
const STREAM_CHECKOUT_DISABLE_AFTER_MISSES: usize = 128;
#[cfg(not(unix))]
const STREAM_CHECKOUT_DISABLE_AFTER_MISSES: usize = 16;
const PARALLEL_FRESH_CHECKOUT_MIN_ENTRIES: usize = 256;
const PARALLEL_FRESH_CHECKOUT_MAX_WORKERS: usize = 2;
const CHECKOUT_METADATA_INITIAL_CAPACITY_LIMIT: usize = 8192;

#[derive(Debug, Clone, Copy, Default)]
struct CheckoutPhaseTotals {
    dir_prep: Duration,
    path_prep: Duration,
    stream_write: Duration,
    object_locate: Duration,
    object_read: Duration,
    object_read_loose: Duration,
    object_read_packed: Duration,
    object_read_unknown: Duration,
    materialize: Duration,
    materialize_content: Duration,
    materialize_write: Duration,
    materialize_symlink: Duration,
    materialize_file_open: Duration,
    materialize_file_open_max: Duration,
    materialize_file_bytes: Duration,
    materialize_file_bytes_max: Duration,
    materialize_file_close: Duration,
    materialize_file_close_max: Duration,
    materialize_file_write_direct: Duration,
    materialize_file_write_direct_max: Duration,
    materialize_chmod: Duration,
    parallel_worker_elapsed: Duration,
    parallel_worker_elapsed_max: Duration,
    parallel_worker_object_read_max: Duration,
    parallel_worker_materialize_max: Duration,
    parallel_worker_file_open_max: Duration,
    parallel_worker_file_bytes_max: Duration,
    parallel_worker_file_close_max: Duration,
    parallel_worker_file_write_direct_max: Duration,
    metadata: Duration,
    entries: usize,
    stream_attempts: usize,
    stream_written: usize,
    stream_skipped_after_disable: usize,
    object_read_loose_count: usize,
    object_read_packed_count: usize,
    object_read_unknown_count: usize,
    materialized_regular_files: usize,
    materialized_executable_files: usize,
    materialized_file_bytes: u64,
    materialized_file_max_bytes: u64,
    parallel_worker_count: usize,
    parallel_worker_entries_min: usize,
    parallel_worker_entries_max: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct CheckoutBlobRules {
    ident: bool,
    eol: CheckoutEolRule,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum CheckoutEolRule {
    #[default]
    None,
    TextCrlf,
    AutoCrlf,
}

#[derive(Debug, Clone, Copy)]
struct CheckoutContentRules<'a> {
    attributes: &'a GitAttributes,
    has_rules: bool,
}

impl<'a> CheckoutContentRules<'a> {
    fn new(attributes: &'a GitAttributes) -> Self {
        Self {
            attributes,
            has_rules: !attributes.is_empty(),
        }
    }

    fn blob_rules_for(&self, path: &[u8]) -> CheckoutBlobRules {
        if !self.has_rules {
            return CheckoutBlobRules::default();
        }
        CheckoutBlobRules {
            ident: self.attributes.is_set(path, "ident"),
            eol: attributes_eol_rule(self.attributes, path),
        }
    }
}

impl CheckoutPhaseTotals {
    fn add(&mut self, other: Self) {
        self.dir_prep += other.dir_prep;
        self.path_prep += other.path_prep;
        self.stream_write += other.stream_write;
        self.object_locate += other.object_locate;
        self.object_read += other.object_read;
        self.object_read_loose += other.object_read_loose;
        self.object_read_packed += other.object_read_packed;
        self.object_read_unknown += other.object_read_unknown;
        self.materialize += other.materialize;
        self.materialize_content += other.materialize_content;
        self.materialize_write += other.materialize_write;
        self.materialize_symlink += other.materialize_symlink;
        self.materialize_file_open += other.materialize_file_open;
        self.materialize_file_open_max = self
            .materialize_file_open_max
            .max(other.materialize_file_open_max);
        self.materialize_file_bytes += other.materialize_file_bytes;
        self.materialize_file_bytes_max = self
            .materialize_file_bytes_max
            .max(other.materialize_file_bytes_max);
        self.materialize_file_close += other.materialize_file_close;
        self.materialize_file_close_max = self
            .materialize_file_close_max
            .max(other.materialize_file_close_max);
        self.materialize_file_write_direct += other.materialize_file_write_direct;
        self.materialize_file_write_direct_max = self
            .materialize_file_write_direct_max
            .max(other.materialize_file_write_direct_max);
        self.materialize_chmod += other.materialize_chmod;
        self.parallel_worker_elapsed += other.parallel_worker_elapsed;
        self.parallel_worker_elapsed_max = self
            .parallel_worker_elapsed_max
            .max(other.parallel_worker_elapsed_max);
        self.parallel_worker_object_read_max = self
            .parallel_worker_object_read_max
            .max(other.parallel_worker_object_read_max);
        self.parallel_worker_materialize_max = self
            .parallel_worker_materialize_max
            .max(other.parallel_worker_materialize_max);
        self.parallel_worker_file_open_max = self
            .parallel_worker_file_open_max
            .max(other.parallel_worker_file_open_max);
        self.parallel_worker_file_bytes_max = self
            .parallel_worker_file_bytes_max
            .max(other.parallel_worker_file_bytes_max);
        self.parallel_worker_file_close_max = self
            .parallel_worker_file_close_max
            .max(other.parallel_worker_file_close_max);
        self.parallel_worker_file_write_direct_max = self
            .parallel_worker_file_write_direct_max
            .max(other.parallel_worker_file_write_direct_max);
        self.metadata += other.metadata;
        self.entries += other.entries;
        self.stream_attempts += other.stream_attempts;
        self.stream_written += other.stream_written;
        self.stream_skipped_after_disable += other.stream_skipped_after_disable;
        self.object_read_loose_count += other.object_read_loose_count;
        self.object_read_packed_count += other.object_read_packed_count;
        self.object_read_unknown_count += other.object_read_unknown_count;
        self.materialized_regular_files += other.materialized_regular_files;
        self.materialized_executable_files += other.materialized_executable_files;
        self.materialized_file_bytes += other.materialized_file_bytes;
        self.materialized_file_max_bytes = self
            .materialized_file_max_bytes
            .max(other.materialized_file_max_bytes);
        self.parallel_worker_count += other.parallel_worker_count;
        if other.parallel_worker_entries_min != 0 {
            self.parallel_worker_entries_min = if self.parallel_worker_entries_min == 0 {
                other.parallel_worker_entries_min
            } else {
                self.parallel_worker_entries_min
                    .min(other.parallel_worker_entries_min)
            };
        }
        self.parallel_worker_entries_max = self
            .parallel_worker_entries_max
            .max(other.parallel_worker_entries_max);
    }
}

#[derive(Debug, Clone, Copy)]
struct CheckoutTraceContext {
    label: &'static str,
    enabled: bool,
}

impl CheckoutTraceContext {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            enabled: checkout_phase_trace_enabled(),
        }
    }

    fn emit(self, totals: CheckoutPhaseTotals) {
        if !self.enabled {
            return;
        }
        emit_checkout_phase_line(self.label, "dir_prep", totals.dir_prep, totals.entries);
        emit_checkout_phase_line(self.label, "path_prep", totals.path_prep, totals.entries);
        emit_checkout_phase_line(
            self.label,
            "stream_write",
            totals.stream_write,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "object_locate",
            totals.object_locate,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "object_read",
            totals.object_read,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "object_read_loose",
            totals.object_read_loose,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "object_read_packed",
            totals.object_read_packed,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "object_read_unknown",
            totals.object_read_unknown,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize",
            totals.materialize,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_content",
            totals.materialize_content,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_write",
            totals.materialize_write,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_symlink",
            totals.materialize_symlink,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_open",
            totals.materialize_file_open,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_open_max",
            totals.materialize_file_open_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_bytes",
            totals.materialize_file_bytes,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_bytes_max",
            totals.materialize_file_bytes_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_close",
            totals.materialize_file_close,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_close_max",
            totals.materialize_file_close_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_write_direct",
            totals.materialize_file_write_direct,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_file_write_direct_max",
            totals.materialize_file_write_direct_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "materialize_chmod",
            totals.materialize_chmod,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_elapsed",
            totals.parallel_worker_elapsed,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_elapsed_max",
            totals.parallel_worker_elapsed_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_object_read_max",
            totals.parallel_worker_object_read_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_materialize_max",
            totals.parallel_worker_materialize_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_file_open_max",
            totals.parallel_worker_file_open_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_file_bytes_max",
            totals.parallel_worker_file_bytes_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_file_close_max",
            totals.parallel_worker_file_close_max,
            totals.entries,
        );
        emit_checkout_phase_line(
            self.label,
            "parallel_worker_file_write_direct_max",
            totals.parallel_worker_file_write_direct_max,
            totals.entries,
        );
        emit_checkout_phase_line(self.label, "metadata", totals.metadata, totals.entries);
        emit_checkout_metric_line(
            self.label,
            "stream_attempts",
            totals.stream_attempts,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "stream_written",
            totals.stream_written,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "stream_skipped_after_disable",
            totals.stream_skipped_after_disable,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "object_read_loose",
            totals.object_read_loose_count,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "object_read_packed",
            totals.object_read_packed_count,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "object_read_unknown",
            totals.object_read_unknown_count,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "materialized_regular_files",
            totals.materialized_regular_files,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "materialized_executable_files",
            totals.materialized_executable_files,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "materialized_file_bytes",
            totals.materialized_file_bytes,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "materialized_file_max_bytes",
            totals.materialized_file_max_bytes,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "parallel_worker_count",
            totals.parallel_worker_count,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "parallel_worker_entries_min",
            totals.parallel_worker_entries_min,
            totals.entries,
        );
        emit_checkout_metric_line(
            self.label,
            "parallel_worker_entries_max",
            totals.parallel_worker_entries_max,
            totals.entries,
        );
    }
}

#[derive(Debug)]
struct CheckoutStreamProbe {
    attempts: AtomicUsize,
    misses: AtomicUsize,
    written: AtomicUsize,
    disabled: AtomicBool,
}

impl CheckoutStreamProbe {
    fn new() -> Self {
        Self {
            attempts: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            written: AtomicUsize::new(0),
            disabled: AtomicBool::new(false),
        }
    }

    fn should_attempt(&self) -> bool {
        !self.disabled.load(Ordering::Relaxed)
    }

    fn record(&self, written: bool) {
        self.attempts.fetch_add(1, Ordering::Relaxed);
        if written {
            self.written.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let misses = self.misses.fetch_add(1, Ordering::Relaxed) + 1;
        if misses >= STREAM_CHECKOUT_DISABLE_AFTER_MISSES
            && self.written.load(Ordering::Relaxed) == 0
        {
            self.disabled.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CheckoutIndexOptions {
    pub force: bool,
}

pub fn checkout_index<S: GitObjectStore>(
    store: &S,
    index: &GitIndex,
    worktree_root: impl AsRef<Path>,
    options: CheckoutIndexOptions,
) -> io::Result<()> {
    let worktree_root = worktree_root.as_ref();
    let attributes = GitAttributes::load_from_root(worktree_root)?;
    for entry in index.entries() {
        checkout_entry(store, entry, worktree_root, &attributes, options)?;
    }
    Ok(())
}

pub fn checkout_index_fresh<S: GitObjectStore + Sync>(
    store: &S,
    index: &GitIndex,
    worktree_root: impl AsRef<Path>,
) -> io::Result<()> {
    let _ = checkout_index_fresh_with_metadata(store, index, worktree_root)?;
    Ok(())
}

pub fn checkout_index_fresh_with_metadata<S: GitObjectStore + Sync>(
    store: &S,
    index: &GitIndex,
    worktree_root: impl AsRef<Path>,
) -> io::Result<GitIndex> {
    let trace = CheckoutTraceContext::new("checkout_index_fresh_with_metadata");
    let worktree_root = worktree_root.as_ref();
    let attributes = GitAttributes::load_from_root(worktree_root)?;
    let content_rules = CheckoutContentRules::new(&attributes);
    let stream_probe = CheckoutStreamProbe::new();
    if should_parallel_checkout(index.entries().len()) {
        let (entries, totals) = checkout_index_fresh_parallel(
            store,
            index,
            worktree_root,
            content_rules,
            &stream_probe,
            trace.enabled,
        )?;
        trace.emit(totals);
        return GitIndex::from_trusted_sorted_entries(entries);
    }

    let mut last_created_dir = None;
    let mut entries = Vec::with_capacity(checkout_metadata_initial_capacity(index.entries().len()));
    let mut totals = CheckoutPhaseTotals::default();
    for entry in index.entries() {
        entries.push(checkout_entry_fresh(
            store,
            entry,
            worktree_root,
            content_rules,
            &stream_probe,
            &mut last_created_dir,
            trace.enabled,
            &mut totals,
        )?);
    }
    trace.emit(totals);
    GitIndex::from_trusted_sorted_entries(entries)
}

pub fn checkout_index_fresh_into_metadata<S: GitObjectStore + Sync>(
    store: &S,
    index: GitIndex,
    worktree_root: impl AsRef<Path>,
) -> io::Result<GitIndex> {
    let trace = CheckoutTraceContext::new("checkout_index_fresh_into_metadata");
    let worktree_root = worktree_root.as_ref();
    let attributes = GitAttributes::load_from_root(worktree_root)?;
    let content_rules = CheckoutContentRules::new(&attributes);
    let stream_probe = CheckoutStreamProbe::new();
    if should_parallel_checkout(index.entries().len()) {
        let (entries, totals) = checkout_index_fresh_into_metadata_parallel(
            store,
            index,
            worktree_root,
            content_rules,
            &stream_probe,
            trace.enabled,
        )?;
        trace.emit(totals);
        return GitIndex::from_trusted_sorted_entries(entries);
    }

    let mut last_created_dir = None;
    let mut entries = index.into_trusted_sorted_entries();
    let mut totals = CheckoutPhaseTotals::default();
    for entry in &mut entries {
        checkout_entry_fresh_in_place(
            store,
            entry,
            worktree_root,
            content_rules,
            &stream_probe,
            &mut last_created_dir,
            trace.enabled,
            &mut totals,
        )?;
    }
    trace.emit(totals);
    GitIndex::from_trusted_sorted_entries(entries)
}

fn should_parallel_checkout(entries: usize) -> bool {
    entries >= PARALLEL_FRESH_CHECKOUT_MIN_ENTRIES
        && std::thread::available_parallelism().is_ok_and(|threads| threads.get() > 1)
}

fn checkout_metadata_initial_capacity(entries: usize) -> usize {
    entries.min(CHECKOUT_METADATA_INITIAL_CAPACITY_LIMIT).max(1)
}

fn checkout_index_fresh_parallel<S: GitObjectStore + Sync>(
    store: &S,
    index: &GitIndex,
    worktree_root: &Path,
    content_rules: CheckoutContentRules<'_>,
    stream_probe: &CheckoutStreamProbe,
    trace_enabled: bool,
) -> io::Result<(Vec<IndexEntry>, CheckoutPhaseTotals)> {
    let mut totals = CheckoutPhaseTotals::default();
    let dir_start = trace_enabled.then(Instant::now);
    create_fresh_checkout_dirs(index, worktree_root)?;
    record_phase_elapsed(&mut totals.dir_prep, dir_start);

    let entries = index.entries();
    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(entries.len())
        .min(PARALLEL_FRESH_CHECKOUT_MAX_WORKERS);
    let chunk_size = entries.len().div_ceil(workers);
    let mut checked_out = Vec::with_capacity(checkout_metadata_initial_capacity(entries.len()));

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in entries.chunks(chunk_size) {
            handles.push(scope.spawn(
                move || -> io::Result<(Vec<IndexEntry>, CheckoutPhaseTotals)> {
                    let worker_start = trace_enabled.then(Instant::now);
                    let worker_entries = chunk.len();
                    let mut chunk_entries =
                        Vec::with_capacity(checkout_metadata_initial_capacity(chunk.len()));
                    let mut chunk_totals = CheckoutPhaseTotals::default();
                    for entry in chunk {
                        let path_start = trace_enabled.then(Instant::now);
                        let target = checkout_target_path(worktree_root, &entry.path)?;
                        record_phase_elapsed(&mut chunk_totals.path_prep, path_start);
                        chunk_entries.push(checkout_entry_fresh_prepared(
                            store,
                            entry,
                            target,
                            content_rules,
                            stream_probe,
                            trace_enabled,
                            &mut chunk_totals,
                        )?);
                    }
                    record_parallel_worker(&mut chunk_totals, worker_start, worker_entries);
                    Ok((chunk_entries, chunk_totals))
                },
            ));
        }

        for handle in handles {
            let (mut chunk_entries, chunk_totals) = handle
                .join()
                .map_err(|_| io::Error::other("parallel checkout worker panicked"))??;
            checked_out.append(&mut chunk_entries);
            totals.add(chunk_totals);
        }
        Ok::<(), io::Error>(())
    })?;

    Ok((checked_out, totals))
}

fn checkout_index_fresh_into_metadata_parallel<S: GitObjectStore + Sync>(
    store: &S,
    index: GitIndex,
    worktree_root: &Path,
    content_rules: CheckoutContentRules<'_>,
    stream_probe: &CheckoutStreamProbe,
    trace_enabled: bool,
) -> io::Result<(Vec<IndexEntry>, CheckoutPhaseTotals)> {
    let mut entries = index.into_trusted_sorted_entries();
    let mut totals = CheckoutPhaseTotals::default();
    let dir_start = trace_enabled.then(Instant::now);
    create_fresh_checkout_dirs_for_entries(&entries, worktree_root)?;
    record_phase_elapsed(&mut totals.dir_prep, dir_start);

    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(entries.len())
        .min(PARALLEL_FRESH_CHECKOUT_MAX_WORKERS);
    let chunk_size = entries.len().div_ceil(workers);

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in entries.chunks_mut(chunk_size) {
            handles.push(scope.spawn(move || -> io::Result<CheckoutPhaseTotals> {
                let worker_start = trace_enabled.then(Instant::now);
                let worker_entries = chunk.len();
                let mut chunk_totals = CheckoutPhaseTotals::default();
                for entry in chunk {
                    let path_start = trace_enabled.then(Instant::now);
                    let target = checkout_target_path(worktree_root, &entry.path)?;
                    record_phase_elapsed(&mut chunk_totals.path_prep, path_start);
                    checkout_entry_fresh_prepared_in_place(
                        store,
                        entry,
                        target,
                        content_rules,
                        stream_probe,
                        trace_enabled,
                        &mut chunk_totals,
                    )?;
                }
                record_parallel_worker(&mut chunk_totals, worker_start, worker_entries);
                Ok(chunk_totals)
            }));
        }

        for handle in handles {
            let chunk_totals = handle
                .join()
                .map_err(|_| io::Error::other("parallel checkout worker panicked"))??;
            totals.add(chunk_totals);
        }
        Ok::<(), io::Error>(())
    })?;

    Ok((entries, totals))
}

fn create_fresh_checkout_dirs(index: &GitIndex, worktree_root: &Path) -> io::Result<()> {
    create_fresh_checkout_dirs_for_entries(index.entries(), worktree_root)
}

fn create_fresh_checkout_dirs_for_entries(
    entries: &[IndexEntry],
    worktree_root: &Path,
) -> io::Result<()> {
    let mut last_created_dir = None;
    for entry in entries {
        let target = checkout_target_path(worktree_root, &entry.path)?;
        let dir = if entry.mode == IndexMode::Gitlink {
            Some(target)
        } else {
            target.parent().map(|parent| parent.to_path_buf())
        };
        create_dir_once(&mut last_created_dir, dir.as_deref())?;
    }
    Ok(())
}

fn checkout_entry<S: GitObjectStore>(
    store: &S,
    entry: &IndexEntry,
    worktree_root: &Path,
    attributes: &GitAttributes,
    options: CheckoutIndexOptions,
) -> io::Result<()> {
    if entry.stage != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot checkout an unmerged index entry",
        ));
    }

    let target = checkout_target_path(worktree_root, &entry.path)?;
    if entry.mode == IndexMode::Gitlink {
        return checkout_gitlink(&target, options.force);
    }
    let target_metadata = fs::symlink_metadata(&target).ok();
    let mut target_exists = target_metadata.is_some();
    if target_exists && !options.force {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    if let Some(metadata) = target_metadata.as_ref()
        && metadata.is_dir()
        && matches!(
            entry.mode,
            IndexMode::File | IndexMode::Executable | IndexMode::Symlink
        )
    {
        remove_checkout_path(&target, metadata)?;
        target_exists = false;
    }
    if let Some(metadata) = target_metadata.as_ref()
        && metadata.file_type().is_symlink()
        && matches!(entry.mode, IndexMode::File | IndexMode::Executable)
    {
        remove_checkout_path(&target, metadata)?;
        target_exists = false;
    }
    if let Some(parent) = target.parent() {
        ensure_checkout_dir(parent, options.force)?;
    }

    if entry.mode == IndexMode::Gitlink {
        return checkout_gitlink(&target, options.force);
    }

    let object = store.read_object(&entry.id)?;
    if object.kind != GitObjectKind::Blob {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "index entry does not point to a blob object",
        ));
    }

    let content = checkout_blob_content(attributes, entry, &object.content);
    match entry.mode {
        IndexMode::File => write_regular_file(&target, &content, false, target_exists),
        IndexMode::Executable => write_regular_file(&target, &content, true, target_exists),
        IndexMode::Symlink => write_symlink(&target, &object.content, options.force),
        IndexMode::Gitlink => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "index entry unexpectedly marked as gitlink",
        )),
    }
}

fn checkout_entry_fresh<S: GitObjectStore>(
    store: &S,
    entry: &IndexEntry,
    worktree_root: &Path,
    content_rules: CheckoutContentRules<'_>,
    stream_probe: &CheckoutStreamProbe,
    last_created_dir: &mut Option<PathBuf>,
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<IndexEntry> {
    if entry.stage != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot checkout an unmerged index entry",
        ));
    }

    let path_start = trace_enabled.then(Instant::now);
    let target = checkout_target_path(worktree_root, &entry.path)?;
    if entry.mode == IndexMode::Gitlink {
        create_dir_once(last_created_dir, Some(&target))?;
        record_phase_elapsed(&mut totals.path_prep, path_start);
        totals.entries += 1;
        return Ok(entry.clone());
    }
    create_dir_once(last_created_dir, target.parent())?;
    record_phase_elapsed(&mut totals.path_prep, path_start);

    checkout_entry_fresh_prepared(
        store,
        entry,
        target,
        content_rules,
        stream_probe,
        trace_enabled,
        totals,
    )
}

fn checkout_entry_fresh_in_place<S: GitObjectStore>(
    store: &S,
    entry: &mut IndexEntry,
    worktree_root: &Path,
    content_rules: CheckoutContentRules<'_>,
    stream_probe: &CheckoutStreamProbe,
    last_created_dir: &mut Option<PathBuf>,
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<()> {
    if entry.stage != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot checkout an unmerged index entry",
        ));
    }

    let path_start = trace_enabled.then(Instant::now);
    let target = checkout_target_path(worktree_root, &entry.path)?;
    if entry.mode == IndexMode::Gitlink {
        create_dir_once(last_created_dir, Some(&target))?;
        record_phase_elapsed(&mut totals.path_prep, path_start);
        totals.entries += 1;
        return Ok(());
    }
    create_dir_once(last_created_dir, target.parent())?;
    record_phase_elapsed(&mut totals.path_prep, path_start);
    checkout_entry_fresh_prepared_in_place(
        store,
        entry,
        target,
        content_rules,
        stream_probe,
        trace_enabled,
        totals,
    )
}

fn create_dir_once(last_created_dir: &mut Option<PathBuf>, dir: Option<&Path>) -> io::Result<()> {
    let Some(dir) = dir else {
        return Ok(());
    };
    if last_created_dir.as_deref() == Some(dir) {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    *last_created_dir = Some(dir.to_path_buf());
    Ok(())
}

fn checkout_entry_fresh_prepared<S: GitObjectStore>(
    store: &S,
    entry: &IndexEntry,
    target: PathBuf,
    content_rules: CheckoutContentRules<'_>,
    stream_probe: &CheckoutStreamProbe,
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<IndexEntry> {
    if entry.mode == IndexMode::Gitlink {
        totals.entries += 1;
        return Ok(entry.clone());
    }

    let rules = if matches!(entry.mode, IndexMode::File | IndexMode::Executable) {
        content_rules.blob_rules_for(&entry.path)
    } else {
        CheckoutBlobRules::default()
    };
    let stream_candidate = !rules.ident
        && rules.eol == CheckoutEolRule::None
        && matches!(entry.mode, IndexMode::File | IndexMode::Executable);
    let stream_start = trace_enabled.then(Instant::now);
    let stream_allowed = stream_candidate && stream_probe.should_attempt();
    if stream_allowed {
        totals.stream_attempts += 1;
    } else if stream_candidate {
        totals.stream_skipped_after_disable += 1;
    }
    if stream_allowed {
        let written =
            store.try_write_blob_to_path(&entry.id, STREAM_CHECKOUT_BLOB_MIN_BYTES, &target)?;
        stream_probe.record(written);
        if written {
            totals.stream_written += 1;
            record_phase_elapsed(&mut totals.stream_write, stream_start);
            let metadata_start = trace_enabled.then(Instant::now);
            if entry.mode == IndexMode::Executable {
                set_executable(&target, true)?;
            }
            let mut updated = entry.clone();
            let metadata = fs::symlink_metadata(&target)?;
            apply_checkout_metadata(&mut updated, &metadata);
            record_phase_elapsed(&mut totals.metadata, metadata_start);
            totals.entries += 1;
            return Ok(updated);
        }
    }
    record_phase_elapsed(&mut totals.stream_write, stream_start);

    let object = read_checkout_object(store, &entry.id, trace_enabled, totals)?;
    if object.kind != GitObjectKind::Blob {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "index entry does not point to a blob object",
        ));
    }

    let materialize_start = trace_enabled.then(Instant::now);
    let mut updated = entry.clone();
    match entry.mode {
        IndexMode::File => {
            let content_start = trace_enabled.then(Instant::now);
            let content = checkout_blob_content_with_rules(rules, entry, &object.content);
            record_phase_elapsed(&mut totals.materialize_content, content_start);
            let write_start = trace_enabled.then(Instant::now);
            write_regular_file_fresh(&target, &content, false, trace_enabled, totals)?;
            record_materialized_file_output(totals, content.len(), false, trace_enabled);
            record_phase_elapsed(&mut totals.materialize_write, write_start);
            if apply_fresh_regular_file_metadata(&mut updated, content.len()) {
                record_phase_elapsed(&mut totals.materialize, materialize_start);
                totals.entries += 1;
                return Ok(updated);
            }
        }
        IndexMode::Executable => {
            let content_start = trace_enabled.then(Instant::now);
            let content = checkout_blob_content_with_rules(rules, entry, &object.content);
            record_phase_elapsed(&mut totals.materialize_content, content_start);
            let write_start = trace_enabled.then(Instant::now);
            write_regular_file_fresh(&target, &content, true, trace_enabled, totals)?;
            record_materialized_file_output(totals, content.len(), true, trace_enabled);
            record_phase_elapsed(&mut totals.materialize_write, write_start);
            if apply_fresh_regular_file_metadata(&mut updated, content.len()) {
                record_phase_elapsed(&mut totals.materialize, materialize_start);
                totals.entries += 1;
                return Ok(updated);
            }
        }
        IndexMode::Symlink => {
            let symlink_start = trace_enabled.then(Instant::now);
            if !skip_fresh_symlink_directory_collision(&target)? {
                write_symlink_fresh(&target, &object.content)?;
            }
            record_phase_elapsed(&mut totals.materialize_symlink, symlink_start);
        }
        IndexMode::Gitlink => unreachable!("handled before reading object"),
    };
    record_phase_elapsed(&mut totals.materialize, materialize_start);
    let metadata_start = trace_enabled.then(Instant::now);
    let metadata = fs::symlink_metadata(&target)?;
    apply_checkout_metadata(&mut updated, &metadata);
    record_phase_elapsed(&mut totals.metadata, metadata_start);
    totals.entries += 1;
    Ok(updated)
}

fn checkout_entry_fresh_prepared_in_place<S: GitObjectStore>(
    store: &S,
    entry: &mut IndexEntry,
    target: PathBuf,
    content_rules: CheckoutContentRules<'_>,
    stream_probe: &CheckoutStreamProbe,
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<()> {
    if entry.mode == IndexMode::Gitlink {
        totals.entries += 1;
        return Ok(());
    }

    let rules = if matches!(entry.mode, IndexMode::File | IndexMode::Executable) {
        content_rules.blob_rules_for(&entry.path)
    } else {
        CheckoutBlobRules::default()
    };
    let stream_candidate = !rules.ident
        && rules.eol == CheckoutEolRule::None
        && matches!(entry.mode, IndexMode::File | IndexMode::Executable);
    let stream_start = trace_enabled.then(Instant::now);
    let stream_allowed = stream_candidate && stream_probe.should_attempt();
    if stream_allowed {
        totals.stream_attempts += 1;
    } else if stream_candidate {
        totals.stream_skipped_after_disable += 1;
    }
    if stream_allowed {
        let written =
            store.try_write_blob_to_path(&entry.id, STREAM_CHECKOUT_BLOB_MIN_BYTES, &target)?;
        stream_probe.record(written);
        if written {
            totals.stream_written += 1;
            record_phase_elapsed(&mut totals.stream_write, stream_start);
            let metadata_start = trace_enabled.then(Instant::now);
            if entry.mode == IndexMode::Executable {
                set_executable(&target, true)?;
            }
            let metadata = fs::symlink_metadata(&target)?;
            apply_checkout_metadata(entry, &metadata);
            record_phase_elapsed(&mut totals.metadata, metadata_start);
            totals.entries += 1;
            return Ok(());
        }
    }
    record_phase_elapsed(&mut totals.stream_write, stream_start);

    let object = read_checkout_object(store, &entry.id, trace_enabled, totals)?;
    if object.kind != GitObjectKind::Blob {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "index entry does not point to a blob object",
        ));
    }

    let materialize_start = trace_enabled.then(Instant::now);
    match entry.mode {
        IndexMode::File => {
            let content_start = trace_enabled.then(Instant::now);
            let content = checkout_blob_content_with_rules(rules, entry, &object.content);
            record_phase_elapsed(&mut totals.materialize_content, content_start);
            let write_start = trace_enabled.then(Instant::now);
            write_regular_file_fresh(&target, &content, false, trace_enabled, totals)?;
            record_materialized_file_output(totals, content.len(), false, trace_enabled);
            record_phase_elapsed(&mut totals.materialize_write, write_start);
            if apply_fresh_regular_file_metadata(entry, content.len()) {
                record_phase_elapsed(&mut totals.materialize, materialize_start);
                totals.entries += 1;
                return Ok(());
            }
        }
        IndexMode::Executable => {
            let content_start = trace_enabled.then(Instant::now);
            let content = checkout_blob_content_with_rules(rules, entry, &object.content);
            record_phase_elapsed(&mut totals.materialize_content, content_start);
            let write_start = trace_enabled.then(Instant::now);
            write_regular_file_fresh(&target, &content, true, trace_enabled, totals)?;
            record_materialized_file_output(totals, content.len(), true, trace_enabled);
            record_phase_elapsed(&mut totals.materialize_write, write_start);
            if apply_fresh_regular_file_metadata(entry, content.len()) {
                record_phase_elapsed(&mut totals.materialize, materialize_start);
                totals.entries += 1;
                return Ok(());
            }
        }
        IndexMode::Symlink => {
            let symlink_start = trace_enabled.then(Instant::now);
            if !skip_fresh_symlink_directory_collision(&target)? {
                write_symlink_fresh(&target, &object.content)?;
            }
            record_phase_elapsed(&mut totals.materialize_symlink, symlink_start);
        }
        IndexMode::Gitlink => unreachable!("handled before reading object"),
    };
    record_phase_elapsed(&mut totals.materialize, materialize_start);
    let metadata_start = trace_enabled.then(Instant::now);
    let metadata = fs::symlink_metadata(&target)?;
    apply_checkout_metadata(entry, &metadata);
    record_phase_elapsed(&mut totals.metadata, metadata_start);
    totals.entries += 1;
    Ok(())
}

fn checkout_gitlink(target: &Path, force: bool) -> io::Result<()> {
    if target.is_dir() {
        return Ok(());
    }
    if target.exists() {
        if !force {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", target.display()),
            ));
        }
        fs::remove_file(target)?;
    }
    fs::create_dir_all(target)
}

fn checkout_blob_content<'a>(
    attributes: &GitAttributes,
    entry: &IndexEntry,
    content: &'a [u8],
) -> Cow<'a, [u8]> {
    let rules = CheckoutContentRules::new(attributes).blob_rules_for(&entry.path);
    checkout_blob_content_with_rules(rules, entry, content)
}

fn checkout_blob_content_with_rules<'a>(
    rules: CheckoutBlobRules,
    entry: &IndexEntry,
    content: &'a [u8],
) -> Cow<'a, [u8]> {
    if rules.ident {
        let content = apply_ident_smudge(content, &entry.id);
        let content = checkout_eol_content(rules.eol, &content)
            .unwrap_or_else(|| Cow::Owned(content.clone()));
        Cow::Owned(content.into_owned())
    } else {
        checkout_eol_content(rules.eol, content).unwrap_or(Cow::Borrowed(content))
    }
}

fn checkout_eol_content<'a>(rule: CheckoutEolRule, content: &'a [u8]) -> Option<Cow<'a, [u8]>> {
    match rule {
        CheckoutEolRule::None => None,
        CheckoutEolRule::TextCrlf => Some(Cow::Owned(apply_eol_smudge_to_crlf(content))),
        CheckoutEolRule::AutoCrlf => {
            if auto_crlf_output_would_convert(content) {
                Some(Cow::Owned(apply_eol_smudge_to_crlf(content)))
            } else {
                None
            }
        }
    }
}

fn auto_crlf_output_would_convert(content: &[u8]) -> bool {
    let mut has_lf = false;
    let mut has_crlf = false;
    let mut has_lone_cr = false;
    let mut nul_count = 0usize;
    let mut printable_count = 0usize;
    let mut nonprintable_count = 0usize;
    let mut index = 0usize;
    while index < content.len() {
        match content[index] {
            b'\r' if content.get(index + 1) == Some(&b'\n') => {
                has_crlf = true;
                index += 2;
                continue;
            }
            b'\r' => has_lone_cr = true,
            b'\n' => has_lf = true,
            127 => nonprintable_count += 1,
            byte if byte < 32 => match byte {
                b'\x08' | b'\t' | b'\x1b' | b'\x0c' => printable_count += 1,
                0 => {
                    nul_count += 1;
                    nonprintable_count += 1;
                }
                _ => nonprintable_count += 1,
            },
            _ => printable_count += 1,
        }
        index += 1;
    }
    if content.last() == Some(&b'\x1a') {
        nonprintable_count = nonprintable_count.saturating_sub(1);
    }
    has_lf
        && !has_crlf
        && !has_lone_cr
        && nul_count == 0
        && (printable_count >> 7) >= nonprintable_count
}

fn attributes_eol_rule(attributes: &GitAttributes, path: &[u8]) -> CheckoutEolRule {
    let values = attributes.check(path, &["text".to_owned(), "eol".to_owned()]);
    if values
        .iter()
        .any(|(name, value)| name == "text" && *value == AttributeValue::Unset)
    {
        return CheckoutEolRule::None;
    }
    let eol_crlf = values
        .iter()
        .any(|(name, value)| name == "eol" && *value == AttributeValue::Value("crlf".to_owned()));
    if !eol_crlf {
        return CheckoutEolRule::None;
    }
    if values
        .iter()
        .any(|(name, value)| name == "text" && *value == AttributeValue::Value("auto".to_owned()))
    {
        CheckoutEolRule::AutoCrlf
    } else {
        CheckoutEolRule::TextCrlf
    }
}

fn ensure_checkout_dir(path: &Path, force: bool) -> io::Result<()> {
    if !force {
        return fs::create_dir_all(path);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(metadata) => {
            if let Some(parent) = path.parent() {
                ensure_checkout_dir(parent, true)?;
            }
            remove_checkout_path(path, &metadata)?;
            fs::create_dir(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                ensure_checkout_dir(parent, true)?;
            }
            match fs::create_dir(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn remove_checkout_path(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn skip_fresh_symlink_directory_collision(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn write_regular_file(
    path: &Path,
    content: &[u8],
    executable: bool,
    target_existed: bool,
) -> io::Result<()> {
    fs::write(path, content)?;
    if !executable && !target_existed {
        return Ok(());
    }
    set_executable(path, executable)
}

fn read_checkout_object<S: GitObjectStore>(
    store: &S,
    id: &ObjectId,
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<LooseObject> {
    let storage = if trace_enabled {
        let locate_start = Some(Instant::now());
        let storage = store
            .object_storage_hint(id)
            .unwrap_or(ObjectStorageHint::Unknown);
        record_phase_elapsed(&mut totals.object_locate, locate_start);
        storage
    } else {
        ObjectStorageHint::Unknown
    };

    let read_start = trace_enabled.then(Instant::now);
    let object = store.read_object(id)?;
    let elapsed = read_start.map(|start| start.elapsed());
    if let Some(elapsed) = elapsed {
        totals.object_read += elapsed;
        match storage {
            ObjectStorageHint::Loose => {
                totals.object_read_loose += elapsed;
                totals.object_read_loose_count += 1;
            }
            ObjectStorageHint::Packed => {
                totals.object_read_packed += elapsed;
                totals.object_read_packed_count += 1;
            }
            ObjectStorageHint::Unknown => {
                totals.object_read_unknown += elapsed;
                totals.object_read_unknown_count += 1;
            }
        }
    }
    Ok(object)
}

fn write_regular_file_fresh(
    path: &Path,
    content: &[u8],
    executable: bool,
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<()> {
    use std::io::Write;

    if !trace_enabled {
        let mut file = fs::File::create(path)?;
        file.write_all(content)?;
        drop(file);
        if !executable {
            return Ok(());
        }
        return set_executable(path, executable);
    }

    let write_start = Some(Instant::now());
    let mut file = fs::File::create(path)?;
    file.write_all(content)?;
    drop(file);
    record_phase_elapsed_with_max(
        &mut totals.materialize_file_write_direct,
        &mut totals.materialize_file_write_direct_max,
        write_start,
    );

    if !executable {
        return Ok(());
    }
    let chmod_start = trace_enabled.then(Instant::now);
    set_executable(path, executable)?;
    record_phase_elapsed(&mut totals.materialize_chmod, chmod_start);
    Ok(())
}

fn record_materialized_file_output(
    totals: &mut CheckoutPhaseTotals,
    bytes: usize,
    executable: bool,
    trace_enabled: bool,
) {
    if !trace_enabled {
        return;
    }
    if executable {
        totals.materialized_executable_files += 1;
    } else {
        totals.materialized_regular_files += 1;
    }
    let bytes = bytes as u64;
    totals.materialized_file_bytes += bytes;
    totals.materialized_file_max_bytes = totals.materialized_file_max_bytes.max(bytes);
}

fn record_phase_elapsed(total: &mut Duration, start: Option<Instant>) {
    if let Some(start) = start {
        *total += start.elapsed();
    }
}

fn record_phase_elapsed_with_max(total: &mut Duration, max: &mut Duration, start: Option<Instant>) {
    if let Some(start) = start {
        let elapsed = start.elapsed();
        *total += elapsed;
        *max = (*max).max(elapsed);
    }
}

fn record_parallel_worker(
    totals: &mut CheckoutPhaseTotals,
    start: Option<Instant>,
    entries: usize,
) {
    if let Some(start) = start {
        let elapsed = start.elapsed();
        totals.parallel_worker_elapsed += elapsed;
        totals.parallel_worker_elapsed_max = totals.parallel_worker_elapsed_max.max(elapsed);
        totals.parallel_worker_object_read_max = totals
            .parallel_worker_object_read_max
            .max(totals.object_read);
        totals.parallel_worker_materialize_max = totals
            .parallel_worker_materialize_max
            .max(totals.materialize);
        totals.parallel_worker_file_open_max = totals
            .parallel_worker_file_open_max
            .max(totals.materialize_file_open);
        totals.parallel_worker_file_bytes_max = totals
            .parallel_worker_file_bytes_max
            .max(totals.materialize_file_bytes);
        totals.parallel_worker_file_close_max = totals
            .parallel_worker_file_close_max
            .max(totals.materialize_file_close);
        totals.parallel_worker_file_write_direct_max = totals
            .parallel_worker_file_write_direct_max
            .max(totals.materialize_file_write_direct);
        totals.parallel_worker_count += 1;
        totals.parallel_worker_entries_min = entries;
        totals.parallel_worker_entries_max = entries;
    }
}

fn emit_checkout_phase_line(
    label: &'static str,
    phase: &'static str,
    elapsed: Duration,
    entries: usize,
) {
    use std::io::Write;

    let line = format!(
        "zmin-checkout-phase\t{label}\tphase={phase}\tseconds={:.6}\tentries={entries}",
        elapsed.as_secs_f64()
    );
    if let Some(path) = std::env::var_os("ZMIN_PHASE_TRACE_FILE") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
            return;
        }
    }
    eprintln!("{line}");
}

fn emit_checkout_metric_line(
    label: &'static str,
    metric: &'static str,
    value: impl std::fmt::Display,
    entries: usize,
) {
    use std::io::Write;

    let line =
        format!("zmin-checkout-metric\t{label}\tmetric={metric}\tvalue={value}\tentries={entries}");
    if let Some(path) = std::env::var_os("ZMIN_PHASE_TRACE_FILE") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{line}");
            return;
        }
    }
    eprintln!("{line}");
}

fn checkout_phase_trace_enabled() -> bool {
    std::env::var_os("ZMIN_CHECKOUT_PHASE_TRACE").is_some_and(|value| !value.is_empty())
}

#[cfg(unix)]
fn write_symlink_fresh(path: &Path, content: &[u8]) -> io::Result<()> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    let target = PathBuf::from(OsString::from_vec(content.to_vec()));
    symlink(target, path)
}

#[cfg(not(unix))]
fn write_symlink_fresh(path: &Path, content: &[u8]) -> io::Result<()> {
    fs::write(path, content)
}

#[cfg(unix)]
fn write_symlink(path: &Path, content: &[u8], force: bool) -> io::Result<()> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    if force && fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path)?;
    }
    let target = PathBuf::from(OsString::from_vec(content.to_vec()));
    symlink(target, path)
}

#[cfg(not(unix))]
fn write_symlink(path: &Path, content: &[u8], force: bool) -> io::Result<()> {
    if force && fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path)?;
    }
    fs::write(path, content)
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    let mut mode = permissions.mode();
    if executable {
        mode |= 0o111;
    } else {
        mode &= !0o111;
    }
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn apply_checkout_metadata(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    use std::os::unix::fs::MetadataExt;

    entry.ctime_seconds = u32_from_i64_lossy(metadata.ctime());
    entry.ctime_nanoseconds = u32_from_i64_lossy(metadata.ctime_nsec());
    entry.mtime_seconds = u32_from_i64_lossy(metadata.mtime());
    entry.mtime_nanoseconds = u32_from_i64_lossy(metadata.mtime_nsec());
    entry.dev = metadata.dev() as u32;
    entry.ino = metadata.ino() as u32;
    entry.uid = metadata.uid();
    entry.gid = metadata.gid();
    entry.size = metadata.len().min(u32::MAX as u64) as u32;
}

#[cfg(not(unix))]
fn apply_checkout_metadata(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    entry.size = metadata.len().min(u32::MAX as u64) as u32;
}

#[cfg(unix)]
fn apply_fresh_regular_file_metadata(_entry: &mut IndexEntry, _content_len: usize) -> bool {
    false
}

#[cfg(not(unix))]
fn apply_fresh_regular_file_metadata(entry: &mut IndexEntry, content_len: usize) -> bool {
    entry.size = content_len.min(u32::MAX as usize) as u32;
    true
}

#[cfg(unix)]
fn u32_from_i64_lossy(value: i64) -> u32 {
    if value <= 0 { 0 } else { value as u32 }
}

#[cfg(unix)]
fn checkout_target_path(root: &Path, path: &[u8]) -> io::Result<PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    Ok(root.join(Path::new(OsStr::from_bytes(path))))
}

#[cfg(not(unix))]
fn checkout_target_path(root: &Path, path: &[u8]) -> io::Result<PathBuf> {
    let path = std::str::from_utf8(path).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "non-utf8 index paths are not supported on this platform",
        )
    })?;
    Ok(root.join(path))
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::TempDir;

    use super::*;
    use crate::{
        GitHashAlgorithm, GitIndex, GitObjectKind, GitObjectSink, InMemoryObjectStore, IndexEntry,
        LooseObject, LooseObjectStore, ObjectId,
    };

    struct StreamProbeTestStore {
        inner: InMemoryObjectStore,
        attempts: AtomicUsize,
        successful_streams: usize,
    }

    impl StreamProbeTestStore {
        fn new(successful_streams: usize) -> Self {
            Self {
                inner: InMemoryObjectStore::new(GitHashAlgorithm::Sha1),
                attempts: AtomicUsize::new(0),
                successful_streams,
            }
        }

        fn write_blob(&self, content: &[u8]) -> ObjectId {
            self.inner
                .write_object(GitObjectKind::Blob, content)
                .expect("write test blob")
        }

        fn attempts(&self) -> usize {
            self.attempts.load(Ordering::SeqCst)
        }
    }

    impl GitObjectStore for StreamProbeTestStore {
        fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
            self.inner.read_object(id)
        }

        fn try_write_blob_to_path(
            &self,
            id: &ObjectId,
            _min_bytes: usize,
            path: &Path,
        ) -> io::Result<bool> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt >= self.successful_streams {
                return Ok(false);
            }
            let object = self.inner.read_object(id)?;
            fs::write(path, object.content)?;
            Ok(true)
        }
    }

    #[test]
    fn checkout_metadata_initial_capacity_is_bounded() {
        assert_eq!(
            checkout_metadata_initial_capacity(usize::MAX),
            CHECKOUT_METADATA_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(checkout_metadata_initial_capacity(2), 2);
        assert_eq!(checkout_metadata_initial_capacity(0), 1);
    }

    #[test]
    fn fresh_checkout_stops_repeated_failed_stream_probes() {
        let repo = git_init();
        let store = StreamProbeTestStore::new(0);
        let mut entries = Vec::new();
        for index in 0..(STREAM_CHECKOUT_DISABLE_AFTER_MISSES + 32) {
            let blob = store.write_blob(format!("small blob {index}\n").as_bytes());
            entries.push(
                IndexEntry::new(format!("file-{index}.txt"), blob, IndexMode::File, 12)
                    .expect("entry"),
            );
        }
        let index = GitIndex::from_entries(entries).expect("index");

        let checked_out =
            checkout_index_fresh_into_metadata(&store, index, repo.path()).expect("fresh checkout");

        assert_eq!(
            store.attempts(),
            STREAM_CHECKOUT_DISABLE_AFTER_MISSES,
            "stream probes should stop after repeated misses"
        );
        assert_eq!(
            checked_out.entries().len(),
            STREAM_CHECKOUT_DISABLE_AFTER_MISSES + 32
        );
    }

    #[test]
    fn fresh_checkout_keeps_stream_probes_after_success() {
        let repo = git_init();
        let store = StreamProbeTestStore::new(1);
        let mut entries = Vec::new();
        for index in 0..(STREAM_CHECKOUT_DISABLE_AFTER_MISSES + 32) {
            let blob = store.write_blob(format!("small blob {index}\n").as_bytes());
            entries.push(
                IndexEntry::new(format!("file-{index}.txt"), blob, IndexMode::File, 12)
                    .expect("entry"),
            );
        }
        let entry_count = entries.len();
        let index = GitIndex::from_entries(entries).expect("index");

        let checked_out =
            checkout_index_fresh_into_metadata(&store, index, repo.path()).expect("fresh checkout");

        assert_eq!(
            store.attempts(),
            entry_count,
            "a successful stream write should keep later stream probes enabled"
        );
        assert_eq!(checked_out.entries().len(), entry_count);
    }

    #[test]
    fn checks_out_regular_files_from_index() {
        let repo = git_init();
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = store
            .write_object(GitObjectKind::Blob, b"hello\n")
            .expect("write first blob");
        let second = store
            .write_object(GitObjectKind::Blob, b"nested\n")
            .expect("write second blob");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("README.md", first, IndexMode::File, 6).expect("first entry"),
            IndexEntry::new("docs/guide.md", second, IndexMode::File, 7).expect("second entry"),
        ])
        .expect("index");

        checkout_index(
            &store,
            &index,
            repo.path(),
            CheckoutIndexOptions { force: false },
        )
        .expect("checkout index");

        assert_eq!(
            fs::read(repo.path().join("README.md")).expect("read first"),
            b"hello\n"
        );
        assert_eq!(
            fs::read(repo.path().join("docs/guide.md")).expect("read second"),
            b"nested\n"
        );
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"from index\n")
            .expect("write blob");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("README.md", blob, IndexMode::File, 11).expect("entry"),
        ])
        .expect("index");
        fs::write(repo.path().join("README.md"), b"existing\n").expect("write existing");

        let error = checkout_index(
            &store,
            &index,
            repo.path(),
            CheckoutIndexOptions { force: false },
        )
        .expect_err("checkout should refuse overwrite");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read(repo.path().join("README.md")).expect("read existing"),
            b"existing\n"
        );
    }

    #[test]
    fn force_overwrites_existing_files() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"from index\n")
            .expect("write blob");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("README.md", blob, IndexMode::File, 11).expect("entry"),
        ])
        .expect("index");
        fs::write(repo.path().join("README.md"), b"existing\n").expect("write existing");

        checkout_index(
            &store,
            &index,
            repo.path(),
            CheckoutIndexOptions { force: true },
        )
        .expect("checkout index");

        assert_eq!(
            fs::read(repo.path().join("README.md")).expect("read overwritten"),
            b"from index\n"
        );
    }

    #[test]
    fn force_replaces_directory_with_index_file() {
        let repo = git_init();
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"from index\n")
            .expect("write blob");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("path0", blob, IndexMode::File, 11).expect("entry"),
        ])
        .expect("index");
        fs::create_dir(repo.path().join("path0")).expect("create path0 dir");
        fs::write(repo.path().join("path0/file0"), b"worktree\n").expect("write nested");

        checkout_index(
            &store,
            &index,
            repo.path(),
            CheckoutIndexOptions { force: true },
        )
        .expect("checkout index");

        assert_eq!(
            fs::read(repo.path().join("path0")).expect("read file"),
            b"from index\n"
        );
    }

    #[test]
    fn force_replaces_parent_file_with_directory_for_nested_index_file() {
        let repo = git_init();
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let blob = store
            .write_object(GitObjectKind::Blob, b"from index\n")
            .expect("write blob");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("path1/file1", blob, IndexMode::File, 11).expect("entry"),
        ])
        .expect("index");
        fs::write(repo.path().join("path1"), b"worktree\n").expect("write parent file");

        checkout_index(
            &store,
            &index,
            repo.path(),
            CheckoutIndexOptions { force: true },
        )
        .expect("checkout index");

        assert!(repo.path().join("path1").is_dir());
        assert_eq!(
            fs::read(repo.path().join("path1/file1")).expect("read nested file"),
            b"from index\n"
        );
    }

    #[test]
    fn checks_out_gitlinks_as_empty_directories_without_object_read() {
        let repo = git_init();
        let store = LooseObjectStore::new(repo.path().join(".git/objects"), GitHashAlgorithm::Sha1);
        let commit = crate::ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            "0123456789012345678901234567890123456789",
        )
        .expect("commit id");
        let index = GitIndex::from_entries(vec![
            IndexEntry::new("deps/sub", commit, IndexMode::Gitlink, 0).expect("gitlink entry"),
        ])
        .expect("index");

        checkout_index(
            &store,
            &index,
            repo.path(),
            CheckoutIndexOptions { force: false },
        )
        .expect("checkout gitlink");

        assert!(repo.path().join("deps/sub").is_dir());
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
}
