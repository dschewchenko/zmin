use super::*;
use skron_git_core::GitObjectStore;
use skron_primitives::Error as PrimitiveError;
use skron_primitives::git_runtime::GitRefsStore;

pub(crate) fn read_repo_index(repo: &GitRepo) -> Result<GitIndex> {
    if repo.index_path.exists() {
        Ok(read_index(&repo.index_path)?)
    } else {
        Ok(GitIndex::new())
    }
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
    let mut entry_idx = 0;
    while entry_idx < index.entries().len() {
        let entry = &index.entries()[entry_idx];
        if entry.stage != 0 {
            entry_idx += 1;
            continue;
        }
        let path = entry.path.to_vec();
        if !pathspec_matches(&path, pathspecs) {
            entry_idx = next_index_position_after_path(index, &path);
            continue;
        }
        if already_staged.contains(&path) {
            entry_idx = next_index_position_after_path(index, &path);
            continue;
        }
        let absolute = worktree_path_for_index_entry(&repo.root, &path);
        if path_exists(&absolute) {
            let Some(entry) = find_index_entry(index, &path) else {
                entry_idx = next_index_position_after_path(index, &path);
                continue;
            };
            if worktree_entry_modified(&absolute, entry)? {
                stage_file(repo, store, index, &absolute)?;
            }
        } else {
            index.remove_path(&path)?;
        }
        entry_idx = next_index_position_after_path(index, &path);
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
    let mut snapshot = index.clone();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        if path_exists(&absolute) {
            if entry.mode == IndexMode::Gitlink {
                snapshot.upsert(worktree_gitlink_index_entry(entry, &absolute)?)?;
            } else {
                snapshot.upsert(worktree_index_entry(repo, &absolute)?)?;
            }
        } else {
            snapshot.remove_path(&entry.path)?;
        }
    }
    Ok(snapshot)
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
            index_mode_for_metadata(&metadata),
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

pub(crate) fn collect_add_files(
    root: &std::path::Path,
    path: &std::path::Path,
    ignore: &GitIgnore,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let relative = repo_relative_path(root, path)?;
    if ignore.is_ignored(&relative, metadata.is_dir()) {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            collect_add_files(root, &entry.path(), ignore, files)?;
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
    let metadata = fs::symlink_metadata(path)?;
    let relative = repo_relative_path(&repo.root, path)?;
    let file_type = metadata.file_type();
    let mode = if file_type.is_symlink() {
        IndexMode::Symlink
    } else if metadata.is_file() {
        index_mode_for_metadata(&metadata)
    } else {
        return Err(CliError::Message(format!(
            "{} is not a file",
            path.display()
        )));
    };

    let had_unmerged = index
        .entries()
        .iter()
        .any(|entry| entry.path.as_slice() == relative.as_slice() && entry.stage != 0);
    if had_unmerged {
        index.remove_path(&relative)?;
    }

    if let Some(existing) = find_index_entry(index, &relative)
        && existing.mode == mode
        && index_entry_stat_matches(&metadata, existing)
    {
        return Ok(());
    }

    if file_type.is_symlink() {
        let content = read_symlink_content(path)?;
        stage_resolved_content(store, index, relative, content, mode, &metadata)?;
        return Ok(());
    }

    let size = worktree_file_size_usize(metadata.len())?;
    let id = store.write_streamed_blob_content(size, |writer| {
        let mut file = fs::File::open(path)?;
        io::copy(&mut file, writer)?;
        Ok(())
    })?;
    if let Some(existing) = find_index_entry(index, &relative)
        && id == existing.id
    {
        let mut entry = existing.clone();
        entry.mode = mode;
        apply_index_entry_metadata(&mut entry, &metadata);
        index.upsert(entry)?;
        return Ok(());
    }

    let mut entry = IndexEntry::new(relative, id, mode, size.min(u32::MAX as usize) as u32)?;
    apply_index_entry_metadata(&mut entry, &metadata);
    index.upsert(entry)?;
    Ok(())
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
            entry.mode = mode;
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

pub(crate) fn read_head_tree_id(
    repo: &GitRepo,
    store: &LooseObjectStore,
) -> Result<Option<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head = match refs.resolve("HEAD") {
        Ok(head) => head,
        Err(_) => return Ok(None),
    };
    let commit_cache = CommitObjectCache::new(store);
    Ok(Some(commit_cache.read_commit(&head)?.tree.clone()))
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
    for entry in index.entries() {
        if entry.stage != 0 {
            return Err(CliError::Message(
                "status cannot inspect an index with unresolved conflicts".into(),
            ));
        }
        let path = worktree_path_for_index_entry(&repo.root, &entry.path);
        if !path_exists(&path) {
            statuses.push((entry.path.to_vec(), 'D'));
            continue;
        }
        if worktree_entry_modified(&path, entry)? {
            statuses.push((entry.path.to_vec(), 'M'));
        }
    }
    Ok(statuses)
}

#[cfg(unix)]
fn worktree_path_for_index_entry(root: &std::path::Path, path: &[u8]) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    root.join(std::path::Path::new(OsStr::from_bytes(path)))
}

#[cfg(not(unix))]
fn worktree_path_for_index_entry(root: &std::path::Path, path: &[u8]) -> PathBuf {
    root.join(String::from_utf8_lossy(path).as_ref())
}

pub(crate) fn worktree_entry_modified(path: &std::path::Path, entry: &IndexEntry) -> Result<bool> {
    match entry.mode {
        IndexMode::File | IndexMode::Executable => {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_file() || index_mode_for_metadata(&metadata) != entry.mode {
                return Ok(true);
            }
            if index_entry_stat_matches(&metadata, entry) {
                return Ok(false);
            }
            Ok(hash_worktree_file_blob(path, metadata.len())? != entry.id)
        }
        IndexMode::Symlink => symlink_entry_modified(path, entry),
        IndexMode::Gitlink => Ok(!path.is_dir()),
    }
}

#[cfg(unix)]
fn symlink_entry_modified(path: &std::path::Path, entry: &IndexEntry) -> Result<bool> {
    use std::os::unix::ffi::OsStrExt;

    let metadata = fs::symlink_metadata(path)?;
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
fn symlink_entry_modified(_path: &std::path::Path, _entry: &IndexEntry) -> Result<bool> {
    Ok(true)
}

pub(crate) fn apply_index_entry_metadata(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    apply_index_entry_metadata_platform(entry, metadata);
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

#[cfg(not(unix))]
fn apply_index_entry_metadata_platform(entry: &mut IndexEntry, metadata: &fs::Metadata) {
    entry.size = metadata.len().min(u32::MAX as u64) as u32;
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

#[cfg(not(unix))]
fn index_entry_stat_matches_platform(_metadata: &fs::Metadata, _entry: &IndexEntry) -> bool {
    false
}

#[cfg(unix)]
fn u32_from_i64_lossy(value: i64) -> u32 {
    if value <= 0 { 0 } else { value as u32 }
}

#[cfg(unix)]
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
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let target_commit = commit_cache.read_commit(target_id)?;
    let old_index = read_head_index_with_caches(repo, &commit_cache, &tree_cache)?;
    let new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;

    remove_tracked_paths_missing_from_target(repo, &old_index, &new_index)?;
    new_index.write_to_path(&repo.index_path)?;
    checkout_index(
        store,
        &new_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
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
    Ok(())
}

pub(crate) fn checkout_clean_worktree_transition(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_id: &ObjectId,
) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let target_commit = commit_cache.read_commit(target_id)?;
    let old_index = read_repo_index(repo)?;
    let new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
    remove_tracked_paths_missing_from_target(repo, &old_index, &new_index)?;
    new_index.write_to_path(&repo.index_path)?;

    let checkout_entries = changed_stage_zero_entries(&old_index, &new_index);
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
    Ok(())
}

pub(crate) fn checkout_worktree_updates_to_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
) -> Result<()> {
    let mut checkout_entries = Vec::new();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        let path = worktree_path_for_index_entry(&repo.root, &entry.path);
        if !path_exists(&path) || worktree_entry_modified(&path, entry)? {
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
}
