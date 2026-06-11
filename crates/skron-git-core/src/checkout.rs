use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::index::{GitIndex, IndexEntry, IndexMode};
use crate::object::GitObjectKind;
use crate::object_store::GitObjectStore;

const STREAM_CHECKOUT_BLOB_MIN_BYTES: usize = 1024 * 1024;
const PARALLEL_FRESH_CHECKOUT_MIN_ENTRIES: usize = 512;
const PARALLEL_FRESH_CHECKOUT_MAX_WORKERS: usize = 2;
const CHECKOUT_METADATA_INITIAL_CAPACITY_LIMIT: usize = 8192;

#[derive(Debug, Clone, Copy, Default)]
struct CheckoutPhaseTotals {
    path_prep: Duration,
    stream_write: Duration,
    object_read: Duration,
    materialize: Duration,
    metadata: Duration,
    entries: usize,
}

impl CheckoutPhaseTotals {
    fn add(&mut self, other: Self) {
        self.path_prep += other.path_prep;
        self.stream_write += other.stream_write;
        self.object_read += other.object_read;
        self.materialize += other.materialize;
        self.metadata += other.metadata;
        self.entries += other.entries;
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
        emit_checkout_phase_line(self.label, "path_prep", totals.path_prep, totals.entries);
        emit_checkout_phase_line(
            self.label,
            "stream_write",
            totals.stream_write,
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
            "materialize",
            totals.materialize,
            totals.entries,
        );
        emit_checkout_phase_line(self.label, "metadata", totals.metadata, totals.entries);
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
    for entry in index.entries() {
        checkout_entry(store, entry, worktree_root.as_ref(), options)?;
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
    if should_parallel_checkout(index.entries().len()) {
        let (entries, totals) =
            checkout_index_fresh_parallel(store, index, worktree_root.as_ref(), trace.enabled)?;
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
            worktree_root.as_ref(),
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
    if should_parallel_checkout(index.entries().len()) {
        let (entries, totals) = checkout_index_fresh_into_metadata_parallel(
            store,
            index,
            worktree_root.as_ref(),
            trace.enabled,
        )?;
        trace.emit(totals);
        return GitIndex::from_trusted_sorted_entries(entries);
    }

    let mut last_created_dir = None;
    let worktree_root = worktree_root.as_ref();
    let mut entries = index.into_trusted_sorted_entries();
    let mut totals = CheckoutPhaseTotals::default();
    for entry in &mut entries {
        checkout_entry_fresh_in_place(
            store,
            entry,
            worktree_root,
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
    trace_enabled: bool,
) -> io::Result<(Vec<IndexEntry>, CheckoutPhaseTotals)> {
    create_fresh_checkout_dirs(index, worktree_root)?;

    let entries = index.entries();
    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(entries.len())
        .min(PARALLEL_FRESH_CHECKOUT_MAX_WORKERS);
    let chunk_size = entries.len().div_ceil(workers);
    let mut checked_out = Vec::with_capacity(checkout_metadata_initial_capacity(entries.len()));

    let mut totals = CheckoutPhaseTotals::default();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in entries.chunks(chunk_size) {
            handles.push(scope.spawn(
                move || -> io::Result<(Vec<IndexEntry>, CheckoutPhaseTotals)> {
                    let mut chunk_entries =
                        Vec::with_capacity(checkout_metadata_initial_capacity(chunk.len()));
                    let mut chunk_totals = CheckoutPhaseTotals::default();
                    for entry in chunk {
                        let target = checkout_target_path(worktree_root, &entry.path)?;
                        chunk_entries.push(checkout_entry_fresh_prepared(
                            store,
                            entry,
                            target,
                            trace_enabled,
                            &mut chunk_totals,
                        )?);
                    }
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
    trace_enabled: bool,
) -> io::Result<(Vec<IndexEntry>, CheckoutPhaseTotals)> {
    let mut entries = index.into_trusted_sorted_entries();
    create_fresh_checkout_dirs_for_entries(&entries, worktree_root)?;

    let workers = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
        .min(entries.len())
        .min(PARALLEL_FRESH_CHECKOUT_MAX_WORKERS);
    let chunk_size = entries.len().div_ceil(workers);

    let mut totals = CheckoutPhaseTotals::default();
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in entries.chunks_mut(chunk_size) {
            handles.push(scope.spawn(move || -> io::Result<CheckoutPhaseTotals> {
                let mut chunk_totals = CheckoutPhaseTotals::default();
                for entry in chunk {
                    let target = checkout_target_path(worktree_root, &entry.path)?;
                    checkout_entry_fresh_prepared_in_place(
                        store,
                        entry,
                        target,
                        trace_enabled,
                        &mut chunk_totals,
                    )?;
                }
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
    let target_exists = target.exists();
    if target_exists && !options.force {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
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

    match entry.mode {
        IndexMode::File => write_regular_file(&target, &object.content, false, target_exists),
        IndexMode::Executable => write_regular_file(&target, &object.content, true, target_exists),
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

    checkout_entry_fresh_prepared(store, entry, target, trace_enabled, totals)
}

fn checkout_entry_fresh_in_place<S: GitObjectStore>(
    store: &S,
    entry: &mut IndexEntry,
    worktree_root: &Path,
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
    checkout_entry_fresh_prepared_in_place(store, entry, target, trace_enabled, totals)
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
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<IndexEntry> {
    if entry.mode == IndexMode::Gitlink {
        totals.entries += 1;
        return Ok(entry.clone());
    }

    let stream_start = trace_enabled.then(Instant::now);
    if matches!(entry.mode, IndexMode::File | IndexMode::Executable)
        && store.try_write_blob_to_path(&entry.id, STREAM_CHECKOUT_BLOB_MIN_BYTES, &target)?
    {
        record_phase_elapsed(&mut totals.stream_write, stream_start);
        let metadata_start = trace_enabled.then(Instant::now);
        if entry.mode == IndexMode::Executable {
            set_executable(&target, true)?;
        }
        let metadata = fs::symlink_metadata(&target)?;
        let mut updated = entry.clone();
        apply_checkout_metadata(&mut updated, &metadata);
        record_phase_elapsed(&mut totals.metadata, metadata_start);
        totals.entries += 1;
        return Ok(updated);
    }
    record_phase_elapsed(&mut totals.stream_write, stream_start);

    let read_start = trace_enabled.then(Instant::now);
    let object = store.read_object(&entry.id)?;
    record_phase_elapsed(&mut totals.object_read, read_start);
    if object.kind != GitObjectKind::Blob {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "index entry does not point to a blob object",
        ));
    }

    let materialize_start = trace_enabled.then(Instant::now);
    match entry.mode {
        IndexMode::File => write_regular_file(&target, &object.content, false, false)?,
        IndexMode::Executable => write_regular_file(&target, &object.content, true, false)?,
        IndexMode::Symlink => write_symlink_fresh(&target, &object.content)?,
        IndexMode::Gitlink => unreachable!("handled before reading object"),
    };
    record_phase_elapsed(&mut totals.materialize, materialize_start);
    let metadata_start = trace_enabled.then(Instant::now);
    let metadata = fs::symlink_metadata(&target)?;
    let mut updated = entry.clone();
    apply_checkout_metadata(&mut updated, &metadata);
    record_phase_elapsed(&mut totals.metadata, metadata_start);
    totals.entries += 1;
    Ok(updated)
}

fn checkout_entry_fresh_prepared_in_place<S: GitObjectStore>(
    store: &S,
    entry: &mut IndexEntry,
    target: PathBuf,
    trace_enabled: bool,
    totals: &mut CheckoutPhaseTotals,
) -> io::Result<()> {
    if entry.mode == IndexMode::Gitlink {
        totals.entries += 1;
        return Ok(());
    }

    let stream_start = trace_enabled.then(Instant::now);
    if matches!(entry.mode, IndexMode::File | IndexMode::Executable)
        && store.try_write_blob_to_path(&entry.id, STREAM_CHECKOUT_BLOB_MIN_BYTES, &target)?
    {
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
    record_phase_elapsed(&mut totals.stream_write, stream_start);

    let read_start = trace_enabled.then(Instant::now);
    let object = store.read_object(&entry.id)?;
    record_phase_elapsed(&mut totals.object_read, read_start);
    if object.kind != GitObjectKind::Blob {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "index entry does not point to a blob object",
        ));
    }

    let materialize_start = trace_enabled.then(Instant::now);
    match entry.mode {
        IndexMode::File => write_regular_file(&target, &object.content, false, false)?,
        IndexMode::Executable => write_regular_file(&target, &object.content, true, false)?,
        IndexMode::Symlink => write_symlink_fresh(&target, &object.content)?,
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

fn record_phase_elapsed(total: &mut Duration, start: Option<Instant>) {
    if let Some(start) = start {
        *total += start.elapsed();
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
        "skron-checkout-phase\t{label}\tphase={phase}\tseconds={:.6}\tentries={entries}",
        elapsed.as_secs_f64()
    );
    if let Some(path) = std::env::var_os("SKRON_PHASE_TRACE_FILE") {
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
    std::env::var_os("SKRON_CHECKOUT_PHASE_TRACE").is_some_and(|value| !value.is_empty())
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
fn write_symlink_fresh(_path: &Path, _content: &[u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink checkout is not supported on this platform yet",
    ))
}

#[cfg(unix)]
fn write_symlink(path: &Path, content: &[u8], force: bool) -> io::Result<()> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    if force && path.exists() {
        fs::remove_file(path)?;
    }
    let target = PathBuf::from(OsString::from_vec(content.to_vec()));
    symlink(target, path)
}

#[cfg(not(unix))]
fn write_symlink(_path: &Path, _content: &[u8], _force: bool) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink checkout is not supported on this platform yet",
    ))
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

    use tempfile::TempDir;

    use super::*;
    use crate::{
        GitHashAlgorithm, GitIndex, GitObjectKind, GitObjectSink, InMemoryObjectStore, IndexEntry,
        LooseObjectStore,
    };

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
