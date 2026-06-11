use super::*;

pub(crate) struct MergeConflictFile {
    pub(crate) path: Vec<u8>,
    pub(crate) content: Vec<u8>,
    pub(crate) kind: MergeConflictKind,
}

pub(crate) enum MergeConflictKind {
    Content,
    Binary,
    ModifyDelete { message: String },
    RenameDelete { message: String },
}

pub(crate) enum MergeIndexResult {
    Clean(GitIndex),
    Conflicted {
        index: GitIndex,
        files: Vec<MergeConflictFile>,
    },
}

pub(crate) fn merge_indexes(
    store: &LooseObjectStore,
    base: &GitIndex,
    ours: &GitIndex,
    theirs: &GitIndex,
    target_label: &str,
) -> Result<MergeIndexResult> {
    let mut has_conflicts = false;
    let mut files = Vec::new();
    let index = merge_indexes_inner(store, base, ours, theirs, target_label, &mut files)?;
    if !files.is_empty() {
        has_conflicts = true;
    }
    if has_conflicts {
        Ok(MergeIndexResult::Conflicted { index, files })
    } else {
        Ok(MergeIndexResult::Clean(index))
    }
}

fn merge_indexes_inner(
    store: &LooseObjectStore,
    base: &GitIndex,
    ours: &GitIndex,
    theirs: &GitIndex,
    target_label: &str,
    files: &mut Vec<MergeConflictFile>,
) -> Result<GitIndex> {
    let mut paths = BTreeSet::new();
    for index in [base, ours, theirs] {
        paths.extend(
            index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0)
                .map(|entry| entry.path.to_vec()),
        );
    }

    let mut entries = Vec::new();
    let mut consumed_paths = BTreeSet::new();
    let merge_context = MergeWorktreeContext {
        store,
        base,
        ours,
        theirs,
        target_label,
    };
    merge_exact_rename_delete_conflicts(&merge_context, &mut entries, files, &mut consumed_paths)?;
    for path in paths {
        if consumed_paths.contains(&path) {
            continue;
        }
        let base_entry = find_index_entry(base, &path);
        let our_entry = find_index_entry(ours, &path);
        let their_entry = find_index_entry(theirs, &path);
        if merge_tree_same_entry(our_entry, their_entry) {
            if let Some(entry) = our_entry {
                entries.push(entry.clone());
            }
        } else if merge_tree_same_entry(base_entry, our_entry) {
            if let Some(entry) = their_entry {
                entries.push(entry.clone());
            }
        } else if merge_tree_same_entry(base_entry, their_entry) {
            if let Some(entry) = our_entry {
                entries.push(entry.clone());
            }
        } else {
            merge_content_entries(
                store,
                &path,
                MergeEntrySet {
                    base: base_entry,
                    ours: our_entry,
                    theirs: their_entry,
                },
                target_label,
                &mut entries,
                files,
            )?;
        }
    }
    Ok(GitIndex::from_entries(entries)?)
}

struct MergeWorktreeContext<'a> {
    store: &'a LooseObjectStore,
    base: &'a GitIndex,
    ours: &'a GitIndex,
    theirs: &'a GitIndex,
    target_label: &'a str,
}

fn merge_exact_rename_delete_conflicts(
    context: &MergeWorktreeContext<'_>,
    entries: &mut Vec<IndexEntry>,
    files: &mut Vec<MergeConflictFile>,
    consumed_paths: &mut BTreeSet<Vec<u8>>,
) -> Result<()> {
    for base_entry in context
        .base
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
    {
        if let Some(our_rename) = exact_rename_entry(base_entry, context.ours)
            && find_index_entry(context.theirs, &base_entry.path).is_none()
            && find_index_entry(context.theirs, &our_rename.path).is_none()
        {
            push_conflict_stage_at_path(entries, base_entry, 1, &our_rename.path);
            push_conflict_stage_at_path(entries, our_rename, 2, &our_rename.path);
            files.push(MergeConflictFile {
                path: our_rename.path.to_vec(),
                content: read_index_entry_content(context.store, our_rename)?,
                kind: MergeConflictKind::RenameDelete {
                    message: format!(
                        "CONFLICT (rename/delete): {} renamed to {} in HEAD, but deleted in {}.",
                        String::from_utf8_lossy(&base_entry.path),
                        String::from_utf8_lossy(&our_rename.path),
                        context.target_label
                    ),
                },
            });
            consumed_paths.insert(base_entry.path.to_vec());
            consumed_paths.insert(our_rename.path.to_vec());
        }
        if let Some(their_rename) = exact_rename_entry(base_entry, context.theirs)
            && find_index_entry(context.ours, &base_entry.path).is_none()
            && find_index_entry(context.ours, &their_rename.path).is_none()
        {
            push_conflict_stage_at_path(entries, base_entry, 1, &their_rename.path);
            push_conflict_stage_at_path(entries, their_rename, 3, &their_rename.path);
            files.push(MergeConflictFile {
                path: their_rename.path.to_vec(),
                content: read_index_entry_content(context.store, their_rename)?,
                kind: MergeConflictKind::RenameDelete {
                    message: format!(
                        "CONFLICT (rename/delete): {} renamed to {} in {}, but deleted in HEAD.",
                        String::from_utf8_lossy(&base_entry.path),
                        String::from_utf8_lossy(&their_rename.path),
                        context.target_label
                    ),
                },
            });
            consumed_paths.insert(base_entry.path.to_vec());
            consumed_paths.insert(their_rename.path.to_vec());
        }
    }
    Ok(())
}

fn exact_rename_entry<'a>(base_entry: &IndexEntry, side: &'a GitIndex) -> Option<&'a IndexEntry> {
    side.entries().iter().find(|entry| {
        entry.stage == 0
            && entry.path != base_entry.path
            && entry.id == base_entry.id
            && entry.mode == base_entry.mode
    })
}

struct MergeEntrySet<'a> {
    base: Option<&'a IndexEntry>,
    ours: Option<&'a IndexEntry>,
    theirs: Option<&'a IndexEntry>,
}

fn merge_content_entries(
    store: &LooseObjectStore,
    path: &[u8],
    entry_set: MergeEntrySet<'_>,
    target_label: &str,
    entries: &mut Vec<IndexEntry>,
    files: &mut Vec<MergeConflictFile>,
) -> Result<()> {
    let Some(base) = entry_set.base else {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "Automatic merge failed; non-file conflict in {}",
                String::from_utf8_lossy(path)
            ),
        });
    };
    if let (Some(ours), None) = (entry_set.ours, entry_set.theirs) {
        let ours_content = read_index_entry_content(store, ours)?;
        push_conflict_stage(entries, base, 1);
        push_conflict_stage(entries, ours, 2);
        files.push(MergeConflictFile {
            path: path.to_vec(),
            content: ours_content,
            kind: MergeConflictKind::ModifyDelete {
                message: format!(
                    "CONFLICT (modify/delete): {} deleted in {} and modified in HEAD.  Version HEAD of {} left in tree.",
                    String::from_utf8_lossy(path),
                    target_label,
                    String::from_utf8_lossy(path)
                ),
            },
        });
        return Ok(());
    }
    if let (None, Some(theirs)) = (entry_set.ours, entry_set.theirs) {
        let theirs_content = read_index_entry_content(store, theirs)?;
        push_conflict_stage(entries, base, 1);
        push_conflict_stage(entries, theirs, 3);
        files.push(MergeConflictFile {
            path: path.to_vec(),
            content: theirs_content,
            kind: MergeConflictKind::ModifyDelete {
                message: format!(
                    "CONFLICT (modify/delete): {} deleted in HEAD and modified in {}.  Version {} of {} left in tree.",
                    String::from_utf8_lossy(path),
                    target_label,
                    target_label,
                    String::from_utf8_lossy(path)
                ),
            },
        });
        return Ok(());
    }
    let (Some(ours), Some(theirs)) = (entry_set.ours, entry_set.theirs) else {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "Automatic merge failed; non-file conflict in {}",
                String::from_utf8_lossy(path)
            ),
        });
    };
    if base.mode == IndexMode::Gitlink
        || ours.mode == IndexMode::Gitlink
        || theirs.mode == IndexMode::Gitlink
    {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "Automatic merge failed; gitlink conflict in {}",
                String::from_utf8_lossy(path)
            ),
        });
    }
    let base_content = read_index_entry_content(store, base)?;
    let ours_content = read_index_entry_content(store, ours)?;
    let theirs_content = read_index_entry_content(store, theirs)?;
    if is_binary_content(&base_content)
        || is_binary_content(&ours_content)
        || is_binary_content(&theirs_content)
    {
        push_conflict_stage(entries, base, 1);
        push_conflict_stage(entries, ours, 2);
        push_conflict_stage(entries, theirs, 3);
        files.push(MergeConflictFile {
            path: path.to_vec(),
            content: ours_content,
            kind: MergeConflictKind::Binary,
        });
        return Ok(());
    }
    let labels = MergeFileLabels {
        current: "HEAD".to_owned(),
        ancestor: "base".to_owned(),
        other: target_label.to_owned(),
    };
    let merged = merge_file_core(&ours_content, &base_content, &theirs_content, &labels);
    if merged.conflicts != 0 {
        push_conflict_stage(entries, base, 1);
        push_conflict_stage(entries, ours, 2);
        push_conflict_stage(entries, theirs, 3);
        files.push(MergeConflictFile {
            path: path.to_vec(),
            content: merged.content,
            kind: MergeConflictKind::Content,
        });
        return Ok(());
    }
    let id = store.write_object(GitObjectKind::Blob, &merged.content)?;
    entries.push(IndexEntry::new(
        path.to_vec(),
        id,
        ours.mode,
        merged.content.len().min(u32::MAX as usize) as u32,
    )?);
    Ok(())
}

fn push_conflict_stage(entries: &mut Vec<IndexEntry>, source: &IndexEntry, stage: u8) {
    push_conflict_stage_at_path(entries, source, stage, &source.path);
}

fn push_conflict_stage_at_path(
    entries: &mut Vec<IndexEntry>,
    source: &IndexEntry,
    stage: u8,
    path: &[u8],
) {
    let mut entry = source.clone();
    entry.stage = stage;
    entry.path = path.to_vec();
    entries.push(entry);
}

pub(crate) fn checkout_merged_stage_zero(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
) -> Result<()> {
    let checkout = stage_zero_index(index)?;
    checkout_index(
        store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    Ok(())
}

pub(crate) fn stage_zero_index(index: &GitIndex) -> Result<GitIndex> {
    Ok(GitIndex::from_entries(
        index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0)
            .cloned()
            .collect(),
    )?)
}

pub(crate) fn merge_index_unmerged_paths(index: &GitIndex) -> Vec<Vec<u8>> {
    let mut paths = index
        .entries()
        .iter()
        .filter(|entry| entry.stage != 0)
        .map(|entry| entry.path.to_vec())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

pub(crate) fn merge_index_stages<'a>(
    index: &'a GitIndex,
    path: &[u8],
) -> (
    Option<&'a IndexEntry>,
    Option<&'a IndexEntry>,
    Option<&'a IndexEntry>,
) {
    (
        index.entry(path, 1),
        index.entry(path, 2),
        index.entry(path, 3),
    )
}

pub(crate) fn worktree_clean(repo: &GitRepo, _store: &LooseObjectStore) -> Result<bool> {
    let head_index = read_head_index(repo)?;
    let index = if repo.index_path.exists() {
        read_index(&repo.index_path)?
    } else {
        GitIndex::new()
    };
    Ok(diff_indexes(&head_index, &index)?.is_empty() && worktree_status(repo, &index)?.is_empty())
}

pub(crate) fn rm_path_matches(
    index: &GitIndex,
    relative: &[u8],
    recursive: bool,
) -> Result<Vec<Vec<u8>>> {
    if find_index_entry(index, relative).is_some() {
        return Ok(vec![relative.to_vec()]);
    }
    let pathspec_matches = matching_index_entries(index, relative);
    if !pathspec_matches.is_empty() {
        return Ok(pathspec_matches
            .into_iter()
            .map(|entry| entry.path.to_vec())
            .collect());
    }
    let prefix = path_dir_prefix(relative);
    let matches = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.path.starts_with(&prefix))
        .map(|entry| entry.path.to_vec())
        .collect::<Vec<_>>();
    if !matches.is_empty() && !recursive {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "not removing '{}' recursively without -r",
                String::from_utf8_lossy(relative)
            ),
        });
    }
    Ok(matches)
}

pub(crate) fn ensure_rm_safe(
    repo: &GitRepo,
    head_index: &GitIndex,
    index: &GitIndex,
    path: &[u8],
    cached: bool,
) -> Result<()> {
    let Some(index_entry) = find_index_entry(index, path) else {
        return Ok(());
    };
    if let Some(head_entry) = find_index_entry(head_index, path)
        && (head_entry.id != index_entry.id || head_entry.mode != index_entry.mode)
    {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "'{}' has staged content different from HEAD",
                String::from_utf8_lossy(path)
            ),
        });
    }
    if !cached {
        let worktree_path = repo.root.join(String::from_utf8_lossy(path).as_ref());
        if path_exists(&worktree_path) {
            let metadata = fs::symlink_metadata(&worktree_path)?;
            let content = if metadata.file_type().is_symlink() {
                read_symlink_content(&worktree_path)?
            } else if metadata.is_file() {
                fs::read(&worktree_path)?
            } else {
                Vec::new()
            };
            let worktree_id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content);
            if worktree_id != index_entry.id {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!(
                        "'{}' has local modifications",
                        String::from_utf8_lossy(path)
                    ),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn mv_target_path(
    source: &Path,
    destination: &Path,
    multiple_sources: bool,
) -> Result<PathBuf> {
    if multiple_sources {
        let file_name = source.file_name().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad source '{}'", source.display()),
        })?;
        Ok(destination.join(file_name))
    } else {
        Ok(destination.to_path_buf())
    }
}

pub(crate) fn mv_index_moves(
    index: &GitIndex,
    source: &[u8],
    target: &[u8],
) -> Result<Vec<(Vec<u8>, IndexEntry)>> {
    let mut moves = Vec::new();
    if let Some(entry) = find_index_entry(index, source) {
        let mut moved = entry.clone();
        moved.path = target.to_vec();
        moves.push((source.to_vec(), moved));
        return Ok(moves);
    }
    let prefix = path_dir_prefix(source);
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.path.starts_with(&prefix))
    {
        let suffix = &entry.path[prefix.len()..];
        let mut moved = entry.clone();
        moved.path = path_join_bytes(target, suffix);
        moves.push((entry.path.to_vec(), moved));
    }
    Ok(moves)
}

pub(crate) fn ensure_mv_destination_available(
    index: &GitIndex,
    target: &[u8],
    force: bool,
) -> Result<()> {
    if force {
        return Ok(());
    }
    if find_index_entry(index, target).is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "destination '{}' already exists",
                String::from_utf8_lossy(target)
            ),
        });
    }
    Ok(())
}

pub(crate) fn rename_worktree_path(source: &Path, target: &Path, force: bool) -> Result<()> {
    if path_exists(target) {
        if !force {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("destination '{}' already exists", target.display()),
            });
        }
        if fs::symlink_metadata(target)?.is_dir() {
            fs::remove_dir_all(target)?;
        } else {
            fs::remove_file(target)?;
        }
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(source, target)?;
    Ok(())
}

pub(crate) fn apply_index_moves(
    index: &mut GitIndex,
    moves: Vec<(Vec<u8>, IndexEntry)>,
) -> Result<()> {
    for (source, _) in &moves {
        index.remove_path(source)?;
        index.remove_dir(source)?;
    }
    for (_, entry) in moves {
        index.upsert(entry)?;
    }
    Ok(())
}

pub(crate) fn merge_tree_same_entry(left: Option<&IndexEntry>, right: Option<&IndexEntry>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.mode == right.mode && left.id == right.id,
        (None, None) => true,
        _ => false,
    }
}

pub(crate) fn fast_forward_to(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target: &str,
    operation: &str,
    ff_only: bool,
) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    fast_forward_to_cached(repo, store, &commit_cache, target, operation, ff_only)
}

pub(crate) fn fast_forward_to_cached(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target: &str,
    operation: &str,
    _ff_only: bool,
) -> Result<()> {
    if !worktree_clean(repo, store)? {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("local changes would be overwritten by {operation}"),
        });
    }

    let current_id = match resolve_commitish_io(repo, store, "HEAD") {
        Ok(id) => Some(id),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(CliError::Io(error)),
    };
    let target_id = resolve_commitish(repo, store, target)?;
    if current_id.as_ref() == Some(&target_id) {
        println!("Already up to date.");
        return Ok(());
    }
    if let Some(current_id) = &current_id
        && !is_ancestor_commit_cached(commit_cache, current_id, &target_id)?
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "Not possible to fast-forward, aborting.".into(),
        });
    }

    let target_commit = commit_cache.read_commit(&target_id)?;
    checkout_clean_worktree_transition(repo, store, &target_id)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if let Some(current_id) = &current_id {
        write_pseudoref(repo, "ORIG_HEAD", current_id)?;
    }
    let reflog_message = format!(
        "{operation} {}: Fast-forward",
        abbrev_ref_name(repo, target).unwrap_or_else(|_| target.to_owned())
    );
    update_head_to_commit_with_reflog(repo, &refs, &target_id, &reflog_message)?;
    if let Some(current_id) = &current_id {
        println!(
            "Updating {}..{}",
            short_object_id(current_id),
            short_object_id(&target_id)
        );
    }
    println!("Fast-forward");
    println!(
        "{} {}",
        short_object_id(&target_id),
        commit_subject(&target_commit.message)
    );
    Ok(())
}
