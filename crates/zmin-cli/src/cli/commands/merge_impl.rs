use super::*;

pub(crate) struct MergeOptions {
    pub(crate) abort: bool,
    pub(crate) continue_: bool,
    pub(crate) ff_only: bool,
    pub(crate) no_ff: bool,
    pub(crate) no_commit: bool,
    pub(crate) squash: bool,
    pub(crate) strategies: Vec<String>,
    pub(crate) commits: Vec<String>,
    pub(crate) commit_label: Option<String>,
}

pub(crate) fn merge(options: MergeOptions) -> Result<()> {
    let MergeOptions {
        abort,
        continue_,
        ff_only,
        no_ff,
        no_commit,
        squash,
        strategies,
        commits,
        commit_label,
    } = options;
    if abort && continue_ {
        return Err(CliError::Fatal {
            code: 129,
            message: "cannot use --abort and --continue with merge".into(),
        });
    }
    if abort {
        return merge_abort();
    }
    if continue_ {
        return merge_continue();
    }
    if commits.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "`merge --ff-only` requires exactly one commit".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if resolve_commitish_io(&repo, &store, &commits[0]).is_err() {
        return Err(CliError::Stderr {
            code: 1,
            text: format!("merge: {} - not something we can merge\n", commits[0]),
        });
    }
    if !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 1,
            message: "local changes would be overwritten by merge".into(),
        });
    }

    if ff_only && !no_ff {
        return fast_forward_to(&repo, &store, &commits[0], "merge", ff_only);
    }
    let commit_cache = CommitObjectCache::new(&store);
    let mode = MergeCommitMode { no_commit, squash };
    if !strategies.is_empty() {
        return merge_with_strategy(
            &repo,
            &store,
            &commit_cache,
            &commits[0],
            commit_label.as_deref(),
            &strategies,
            !no_ff && !squash,
            mode,
        );
    }
    merge_commit(
        &repo,
        &store,
        &commit_cache,
        &commits[0],
        commit_label.as_deref(),
        "ort",
        !no_ff && !squash,
        mode,
    )
}

fn merge_abort() -> Result<()> {
    let repo = find_repo()?;
    let merge_head_path = repo.git_dir.join("MERGE_HEAD");
    if !merge_head_path.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: "There is no merge to abort (MERGE_HEAD missing).".into(),
        });
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let current_index = read_repo_index(&repo)?;
    let head_index = tree_cache.read_tree_to_index(&head_commit.tree)?;
    remove_tracked_paths_missing_from_target(&repo, &current_index, &head_index)?;
    head_index.write_to_path(&repo.index_path)?;
    checkout_index(
        &store,
        &head_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    remove_file_if_exists(&merge_head_path)?;
    remove_file_if_exists(&repo.git_dir.join("MERGE_MSG"))?;
    remove_file_if_exists(&repo.git_dir.join("MERGE_MODE"))?;
    Ok(())
}

fn merge_continue() -> Result<()> {
    let repo = find_repo()?;
    let merge_head_path = repo.git_dir.join("MERGE_HEAD");
    let merge_message_path = repo.git_dir.join("MERGE_MSG");
    if !merge_head_path.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: "There is no merge in progress (MERGE_HEAD missing).".into(),
        });
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let unmerged = merge_index_unmerged_paths(&index);
    if !unmerged.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "Committing is not possible because you have unmerged files: {}",
                unmerged
                    .iter()
                    .map(|path| String::from_utf8_lossy(path).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let merge_head = fs::read_to_string(&merge_head_path)?;
    let merge_head_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, merge_head.trim())?;
    let message = clean_merge_message(&fs::read_to_string(&merge_message_path)?)?;
    let tree = write_tree_from_index(&store, &index)?;
    let author = signature_from_identity(&repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
    let commit = CommitBuilder::new(tree, author, committer)
        .parent(head_id)
        .parent(merge_head_id)
        .message(message.clone().into_bytes())?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    remove_file_if_exists(&merge_head_path)?;
    remove_file_if_exists(&merge_message_path)?;
    println!(
        "[{}] {}",
        short_object_id(&id),
        commit_subject(message.as_bytes())
    );
    Ok(())
}

fn clean_merge_message(message: &str) -> Result<String> {
    let mut cleaned = message
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    while cleaned.ends_with('\n') {
        cleaned.pop();
    }
    if cleaned.trim().is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "empty merge commit message".into(),
        });
    }
    cleaned.push('\n');
    Ok(cleaned)
}

fn merge_with_strategy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target: &str,
    target_label: Option<&str>,
    strategies: &[String],
    allow_fast_forward: bool,
    mode: MergeCommitMode,
) -> Result<()> {
    if strategies.len() == 1 && strategies[0] == "ours" {
        return merge_ours_strategy(repo, store, commit_cache, target, target_label);
    }
    if strategies.len() == 1 && matches!(strategies[0].as_str(), "ort" | "recursive") {
        return merge_commit(
            repo,
            store,
            commit_cache,
            target,
            target_label,
            &strategies[0],
            allow_fast_forward,
            mode,
        );
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("merge strategy not supported: {}", strategies.join(", ")),
    })
}

fn merge_ours_strategy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target: &str,
    target_label: Option<&str>,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let target_id = resolve_commitish(repo, store, target)?;
    if head_id == target_id || is_ancestor_commit_cached(commit_cache, &target_id, &head_id)? {
        println!("Already up to date.");
        return Ok(());
    }
    let head_commit = commit_cache.read_commit(&head_id)?;
    let author = signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = format!(
        "Merge branch '{}'\n",
        merge_display_name_or_label(repo, target, target_label)
    );
    let commit = CommitBuilder::new(head_commit.tree.clone(), author, committer)
        .parent(head_id)
        .parent(target_id)
        .message(message.into_bytes())?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    println!("Merge made by the 'ours' strategy.");
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct MergeCommitMode {
    no_commit: bool,
    squash: bool,
}

fn merge_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target: &str,
    target_label: Option<&str>,
    strategy_label: &str,
    allow_fast_forward: bool,
    mode: MergeCommitMode,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let target_id = resolve_commitish(repo, store, target)?;
    if head_id == target_id || is_ancestor_commit_cached(commit_cache, &target_id, &head_id)? {
        println!("Already up to date.");
        return Ok(());
    }
    if allow_fast_forward && is_ancestor_commit_cached(commit_cache, &head_id, &target_id)? {
        return fast_forward_to_cached(repo, store, commit_cache, target, "merge", false);
    }

    let Some(base_id) = best_merge_base_cached(commit_cache, &head_id, &target_id)? else {
        return Err(CliError::Fatal {
            code: 128,
            message: "refusing to merge unrelated histories".into(),
        });
    };
    let head_commit = commit_cache.read_commit(&head_id)?;
    let target_commit = commit_cache.read_commit(&target_id)?;
    let base_commit = commit_cache.read_commit(&base_id)?;
    let tree_cache = TreeObjectCache::new(store);
    let base = read_commit_tree_index_cached(&tree_cache, &base_commit)?;
    let ours = read_commit_tree_index_cached(&tree_cache, &head_commit)?;
    let theirs = read_commit_tree_index_cached(&tree_cache, &target_commit)?;
    let merge_result = merge_indexes(
        store,
        &base,
        &ours,
        &theirs,
        &merge_display_name(repo, target),
    )?;
    let mut merged = match merge_result {
        MergeIndexResult::Clean(merged) => merged,
        MergeIndexResult::Conflicted { index, files } => {
            remove_tracked_paths_missing_from_target(repo, &ours, &index)?;
            checkout_merged_stage_zero(repo, store, &index)?;
            for file in files {
                write_worktree_file(repo, &file.path, &file.content)?;
                match &file.kind {
                    MergeConflictKind::Binary => {
                        println!(
                            "warning: Cannot merge binary files: {} (HEAD vs. {})",
                            String::from_utf8_lossy(&file.path),
                            merge_display_name(repo, target)
                        );
                        println!("Auto-merging {}", String::from_utf8_lossy(&file.path));
                        eprintln!(
                            "CONFLICT (content): Merge conflict in {}",
                            String::from_utf8_lossy(&file.path)
                        );
                    }
                    MergeConflictKind::Content => {
                        println!("Auto-merging {}", String::from_utf8_lossy(&file.path));
                        eprintln!(
                            "CONFLICT (content): Merge conflict in {}",
                            String::from_utf8_lossy(&file.path)
                        );
                    }
                    MergeConflictKind::ModifyDelete { message } => {
                        println!("{message}");
                    }
                    MergeConflictKind::RenameDelete { message } => {
                        println!("{message}");
                    }
                }
            }
            index.write_to_path(&repo.index_path)?;
            write_merge_state(repo, &target_id, &merge_display_name(repo, target))?;
            eprintln!("Automatic merge failed; fix conflicts and then commit the result.");
            return Err(CliError::Exit(1));
        }
    };
    remove_tracked_paths_missing_from_target(repo, &ours, &merged)?;
    let checkout_metadata = WorktreeCheckoutMetadata {
        ref_name: None,
        treeish: Some(target_id.clone()),
    };
    checkout_worktree_updates_to_index_with_metadata(repo, store, &merged, &checkout_metadata)?;
    refresh_tracked_index_metadata_matching(repo, &mut merged, &[])?;
    merged.write_to_path(&repo.index_path)?;
    if mode.squash {
        write_squash_message(repo, commit_cache, &target_id)?;
        println!("Squash commit -- not updating HEAD");
        eprintln!("Automatic merge went well; stopped before committing as requested");
        return Ok(());
    }
    if mode.no_commit {
        let target_label = merge_display_name_or_label(repo, target, target_label);
        write_merge_state(repo, &target_id, &target_label)?;
        eprintln!("Automatic merge went well; stopped before committing as requested");
        return Ok(());
    }
    let tree = write_tree_from_index(store, &merged)?;
    let author = signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = format!(
        "Merge branch '{}'\n",
        merge_display_name_or_label(repo, target, target_label)
    );
    let commit = CommitBuilder::new(tree, author, committer)
        .parent(head_id.clone())
        .parent(target_id.clone())
        .message(message.into_bytes())?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    println!("Merge made by the '{strategy_label}' strategy.");
    Ok(())
}

fn merge_display_name(repo: &GitRepo, target: &str) -> String {
    abbrev_ref_name(repo, target).unwrap_or_else(|_| target.to_owned())
}

fn merge_display_name_or_label(repo: &GitRepo, target: &str, target_label: Option<&str>) -> String {
    target_label
        .map(str::to_owned)
        .unwrap_or_else(|| merge_display_name(repo, target))
}

fn write_merge_state(repo: &GitRepo, target_id: &ObjectId, target_label: &str) -> Result<()> {
    fs::write(
        repo.git_dir.join("MERGE_HEAD"),
        format!("{}\n", target_id.to_hex()),
    )?;
    fs::write(
        repo.git_dir.join("MERGE_MSG"),
        format!("Merge branch '{}'\n\n# Conflicts:\n", target_label),
    )?;
    Ok(())
}

fn write_squash_message(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target_id: &ObjectId,
) -> Result<()> {
    let target = commit_cache.read_commit(target_id)?;
    let subject = commit_subject(&target.message);
    fs::write(
        repo.git_dir.join("SQUASH_MSG"),
        format!(
            "Squashed commit of the following:\n\ncommit {}\n{}\n",
            target_id.to_hex(),
            subject
        ),
    )?;
    remove_file_if_exists(&repo.git_dir.join("MERGE_HEAD"))?;
    remove_file_if_exists(&repo.git_dir.join("MERGE_MSG"))?;
    remove_file_if_exists(&repo.git_dir.join("MERGE_MODE"))?;
    Ok(())
}

fn write_worktree_file(repo: &GitRepo, path: &[u8], content: &[u8]) -> Result<()> {
    let absolute = repo.root.join(String::from_utf8_lossy(path).as_ref());
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(absolute, content)?;
    Ok(())
}

pub(crate) fn merge_file_command(
    stdout: bool,
    labels: Vec<String>,
    current: PathBuf,
    base: PathBuf,
    other: PathBuf,
) -> Result<()> {
    if labels.len() > 3 {
        return Err(CliError::Fatal {
            code: 129,
            message: "merge-file accepts at most three -L labels".into(),
        });
    }
    let current_content = fs::read(&current)?;
    let base_content = fs::read(&base)?;
    let other_content = fs::read(&other)?;
    let merge_labels = MergeFileLabels {
        current: labels
            .first()
            .cloned()
            .unwrap_or_else(|| current.display().to_string()),
        ancestor: labels
            .get(1)
            .cloned()
            .unwrap_or_else(|| base.display().to_string()),
        other: labels
            .get(2)
            .cloned()
            .unwrap_or_else(|| other.display().to_string()),
    };
    let result = merge_file_core(
        &current_content,
        &base_content,
        &other_content,
        &merge_labels,
    );
    if stdout {
        io::stdout().write_all(&result.content)?;
    } else {
        fs::write(&current, &result.content)?;
    }
    if result.conflicts == 0 {
        Ok(())
    } else {
        Err(CliError::Exit(result.conflicts.min(127) as i32))
    }
}

pub(crate) fn merge_one_file(
    orig_blob: &str,
    our_blob: &str,
    their_blob: &str,
    path: &str,
    orig_mode: &str,
    our_mode: &str,
    their_mode: &str,
) -> Result<()> {
    merge_one_file_impl(
        orig_blob, our_blob, their_blob, path, orig_mode, our_mode, their_mode, false,
    )
}

#[allow(clippy::too_many_arguments)]
fn merge_one_file_impl(
    orig_blob: &str,
    our_blob: &str,
    their_blob: &str,
    path: &str,
    _orig_mode: &str,
    our_mode: &str,
    their_mode: &str,
    quiet: bool,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut index = read_repo_index(&repo)?;
    let relative = normalize_git_path(path)?.into_bytes();
    if relative.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "merge-one-file path is empty".into(),
        });
    }
    let base = read_optional_blob(&store, orig_blob)?;
    let Some(ours) = read_optional_blob(&store, our_blob)? else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("ERROR: {path}: Not handling case {orig_blob} ->  -> {their_blob}"),
        });
    };
    let Some(theirs) = read_optional_blob(&store, their_blob)? else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("ERROR: {path}: Not handling case {orig_blob} -> {our_blob} -> "),
        });
    };
    if base.is_none() && ours == theirs {
        if !quiet {
            println!("Adding {path}");
        }
        let absolute = repo.root.join(String::from_utf8_lossy(&relative).as_ref());
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&absolute, &ours)?;
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, our_blob)?;
        let mode = parse_index_mode(our_mode)?;
        let mut entry = IndexEntry::new(
            relative.clone(),
            id,
            mode,
            ours.len().min(u32::MAX as usize) as u32,
        )?;
        if let Ok(metadata) = fs::symlink_metadata(&absolute) {
            apply_index_entry_metadata(&mut entry, &metadata);
        }
        index.remove_path(&relative)?;
        index.upsert(entry)?;
        index.write_to_path(&repo.index_path)?;
        return Ok(());
    }
    let base = base.unwrap_or_default();
    let merge_labels = MergeFileLabels {
        current: ".merge_file_ours".to_owned(),
        ancestor: ".merge_file_base".to_owned(),
        other: ".merge_file_theirs".to_owned(),
    };
    let result = merge_file_core(&ours, &base, &theirs, &merge_labels);
    if !quiet {
        println!("Auto-merging {path}");
    }
    let absolute = repo.root.join(String::from_utf8_lossy(&relative).as_ref());
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&absolute, &result.content)?;
    let id = store.write_object(GitObjectKind::Blob, &result.content)?;
    let mode = if result.content == theirs {
        parse_index_mode(their_mode)?
    } else {
        parse_index_mode(our_mode)?
    };
    let mut entry = IndexEntry::new(
        relative.clone(),
        id,
        mode,
        result.content.len().min(u32::MAX as usize) as u32,
    )?;
    if let Ok(metadata) = fs::symlink_metadata(&absolute) {
        apply_index_entry_metadata(&mut entry, &metadata);
    }
    index.upsert(entry)?;
    index.write_to_path(&repo.index_path)?;
    if result.conflicts == 0 {
        Ok(())
    } else {
        if !quiet {
            eprintln!("ERROR: content conflict in {path}");
        }
        Err(CliError::Exit(result.conflicts.min(127) as i32))
    }
}

pub(crate) fn merge_index(
    one_shot: bool,
    quiet: bool,
    merge_program: &str,
    all: bool,
    paths: Vec<String>,
) -> Result<()> {
    if merge_program != "git-merge-one-file" && merge_program != "merge-one-file" {
        return Err(CliError::Fatal {
            code: 129,
            message: "merge-index currently supports git-merge-one-file only".into(),
        });
    }
    if !all && paths.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "merge-index requires -a or at least one path".into(),
        });
    }
    let repo = find_repo()?;
    let index = read_repo_index(&repo)?;
    let selected = if all {
        merge_index_unmerged_paths(&index)
    } else {
        paths
            .iter()
            .map(|path| Ok(normalize_git_path(path)?.into_bytes()))
            .collect::<Result<Vec<_>>>()?
    };
    let mut failed = false;
    for path in selected {
        let (base, ours, theirs) = merge_index_stages(&index, &path);
        let path_text = String::from_utf8_lossy(&path);
        let result = merge_one_file_impl(
            &base
                .as_ref()
                .map(|entry| entry.id.to_hex())
                .unwrap_or_default(),
            &ours
                .as_ref()
                .map(|entry| entry.id.to_hex())
                .unwrap_or_default(),
            &theirs
                .as_ref()
                .map(|entry| entry.id.to_hex())
                .unwrap_or_default(),
            &path_text,
            base.as_ref()
                .map(|entry| index_mode_octal(entry.mode))
                .unwrap_or(""),
            ours.as_ref()
                .map(|entry| index_mode_octal(entry.mode))
                .unwrap_or(""),
            theirs
                .as_ref()
                .map(|entry| index_mode_octal(entry.mode))
                .unwrap_or(""),
            quiet,
        );
        if let Err(error) = result {
            failed = true;
            if !one_shot {
                return Err(error);
            }
        }
    }
    if failed {
        Err(CliError::Exit(1))
    } else {
        Ok(())
    }
}

pub(crate) fn mergetool(tool: Option<&str>, paths: Vec<PathBuf>) -> Result<()> {
    let repo = find_repo()?;
    let tool = match tool {
        Some(tool) => tool.to_owned(),
        None => read_config_value(&repo, "merge.tool")?.ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "no merge tool configured; set merge.tool or pass --tool".into(),
        })?,
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let command = read_config_value(&repo, &format!("mergetool.{tool}.cmd"))?.ok_or_else(|| {
        CliError::Fatal {
            code: 1,
            message: format!("merge tool '{}' is not configured", tool),
        }
    })?;
    let mut index = read_repo_index(&repo)?;
    let selected = selected_mergetool_paths(&repo, &index, &paths)?;
    if selected.is_empty() {
        println!("No files need merging");
        return Ok(());
    }
    println!("Merging:");
    for path in &selected {
        println!("{}", String::from_utf8_lossy(path));
    }
    println!();
    for path in selected {
        run_mergetool_path(&repo, &store, &mut index, &command, &path)?;
    }
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn selected_mergetool_paths(
    repo: &GitRepo,
    index: &GitIndex,
    paths: &[PathBuf],
) -> Result<Vec<Vec<u8>>> {
    let mut selected = merge_index_unmerged_paths(index);
    if paths.is_empty() {
        return Ok(selected);
    }
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    selected.retain(|path| pathspec_matches(path, &pathspecs));
    Ok(selected)
}

fn run_mergetool_path(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    command: &str,
    path: &[u8],
) -> Result<()> {
    let (base, ours, theirs) = merge_index_stages(index, path);
    let stages = MergetoolStages { base, ours, theirs };
    let path_text = String::from_utf8_lossy(path);
    println!("Normal merge conflict for '{path_text}':");
    println!("  {{local}}: modified file");
    println!("  {{remote}}: modified file");
    let temp_root = diff_commands::create_difftool_temp_root()?;
    let result = run_mergetool_command_for_path(repo, store, command, &temp_root, path, &stages)
        .and_then(|()| stage_mergetool_result(repo, store, index, path));
    let cleanup = fs::remove_dir_all(&temp_root);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) if error.kind() != io::ErrorKind::NotFound => Err(CliError::Io(error)),
        (Ok(()), _) => Ok(()),
    }
}

struct MergetoolStages<'a> {
    base: Option<&'a IndexEntry>,
    ours: Option<&'a IndexEntry>,
    theirs: Option<&'a IndexEntry>,
}

fn run_mergetool_command_for_path(
    repo: &GitRepo,
    store: &LooseObjectStore,
    command: &str,
    temp_root: &std::path::Path,
    path: &[u8],
    stages: &MergetoolStages<'_>,
) -> Result<()> {
    let base_path = mergetool_stage_path(store, temp_root, "base", path, stages.base)?;
    let local_path = mergetool_stage_path(store, temp_root, "local", path, stages.ours)?;
    let remote_path = mergetool_stage_path(store, temp_root, "remote", path, stages.theirs)?;
    let merged_path = repo.root.join(String::from_utf8_lossy(path).as_ref());
    if path_exists(&merged_path) {
        fs::copy(&merged_path, merged_path.with_extension("txt.orig"))?;
    }
    let mut process = mergetool_shell(command);
    let status = process
        .current_dir(&repo.root)
        .env("BASE", mergetool_env_path(&base_path))
        .env("LOCAL", mergetool_env_path(&local_path))
        .env("REMOTE", mergetool_env_path(&remote_path))
        .env("MERGED", mergetool_env_path(&merged_path))
        .status()
        .map_err(CliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Exit(status.code().unwrap_or(1)))
    }
}

fn mergetool_stage_path(
    store: &LooseObjectStore,
    temp_root: &std::path::Path,
    side: &str,
    path: &[u8],
    entry: Option<&IndexEntry>,
) -> Result<PathBuf> {
    match entry {
        Some(entry) => diff_commands::write_difftool_temp_file(
            temp_root,
            side,
            path,
            &read_index_entry_content(store, entry)?,
        ),
        None => Ok(diff_commands::null_device_path()),
    }
}

fn stage_mergetool_result(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &[u8],
) -> Result<()> {
    let merged_path = repo.root.join(String::from_utf8_lossy(path).as_ref());
    index.remove_path(path)?;
    stage_file(repo, store, index, &merged_path)
}

fn mergetool_shell(command: &str) -> ProcessCommand {
    #[cfg(not(windows))]
    {
        let mut process = ProcessCommand::new("sh");
        process.arg("-c").arg(command);
        process
    }
    #[cfg(windows)]
    {
        let mut process = ProcessCommand::new("sh");
        process.arg("-c").arg(command);
        process
    }
}

fn mergetool_env_path(path: &std::path::Path) -> String {
    mergetool_env_path_string(path.display().to_string())
}

#[cfg(windows)]
fn mergetool_env_path_string(value: String) -> String {
    value.replace('\\', "/")
}

#[cfg(not(windows))]
fn mergetool_env_path_string(value: String) -> String {
    value
}

fn read_optional_blob(store: &LooseObjectStore, id: &str) -> Result<Option<Vec<u8>>> {
    if id.is_empty() {
        return Ok(None);
    }
    let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?;
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("{id} is not a blob"),
        });
    }
    Ok(Some(object.content))
}

pub(crate) fn merge_tree_command(options: MergeTreeOptions) -> Result<()> {
    if options.write_tree
        || options.messages
        || options.no_messages
        || options.quiet
        || options.nul_terminated
        || options.name_only
        || options.allow_unrelated_histories
        || options.stdin
        || options.merge_base.is_some()
        || !options.strategy_options.is_empty()
    {
        return Err(CliError::Fatal {
            code: 129,
            message: "merge-tree currently supports the trivial three-tree form".into(),
        });
    }
    let _ = options.trivial_merge;
    if options.args.len() != 3 {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git merge-tree <base-tree> <branch1> <branch2>".into(),
        });
    }

    let repo = find_repo()?;
    let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
    let base_tree = resolve_treeish(&repo, &store, &options.args[0])?;
    let ours_tree = resolve_treeish(&repo, &store, &options.args[1])?;
    let theirs_tree = resolve_treeish(&repo, &store, &options.args[2])?;
    let tree_cache = TreeObjectCache::new(&store);
    let base = tree_cache.read_tree_to_index(&base_tree)?;
    let ours = tree_cache.read_tree_to_index(&ours_tree)?;
    let theirs = tree_cache.read_tree_to_index(&theirs_tree)?;

    let mut paths = BTreeSet::new();
    for index in [&base, &ours, &theirs] {
        paths.extend(
            index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0)
                .map(|entry| entry.path.clone()),
        );
    }

    for path in paths {
        let base_entry = find_index_entry(&base, &path);
        let our_entry = find_index_entry(&ours, &path);
        let their_entry = find_index_entry(&theirs, &path);
        if merge_tree_same_entry(their_entry, base_entry)
            || merge_tree_same_entry(their_entry, our_entry)
        {
            continue;
        }
        if merge_tree_same_entry(our_entry, base_entry) {
            merge_tree_print_remote_change(&store, &path, base_entry, our_entry, their_entry)?;
        } else {
            merge_tree_print_conflict(&store, &path, base_entry, our_entry, their_entry)?;
        }
    }
    Ok(())
}

fn merge_tree_print_remote_change(
    store: &LooseObjectStore,
    path: &[u8],
    base: Option<&IndexEntry>,
    ours: Option<&IndexEntry>,
    theirs: Option<&IndexEntry>,
) -> Result<()> {
    let path_text = String::from_utf8_lossy(path);
    match (base, theirs) {
        (None, Some(theirs)) => {
            println!("added in remote");
            merge_tree_print_entry_line("their", theirs, &path_text);
        }
        (Some(base), None) => {
            println!("removed in remote");
            merge_tree_print_entry_line("base", base, &path_text);
            if let Some(ours) = ours {
                merge_tree_print_entry_line("our", ours, &path_text);
            }
        }
        (Some(_), Some(theirs)) => {
            println!("merged");
            merge_tree_print_entry_line("result", theirs, &path_text);
            if let Some(ours) = ours {
                merge_tree_print_entry_line("our", ours, &path_text);
            }
        }
        (None, None) => return Ok(()),
    }
    merge_tree_print_diff(store, path, base, theirs)
}

fn merge_tree_print_conflict(
    store: &LooseObjectStore,
    path: &[u8],
    base: Option<&IndexEntry>,
    ours: Option<&IndexEntry>,
    theirs: Option<&IndexEntry>,
) -> Result<()> {
    let (Some(base), Some(ours), Some(theirs)) = (base, ours, theirs) else {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "merge-tree cannot yet render non-file conflict for {}",
                String::from_utf8_lossy(path)
            ),
        });
    };
    println!("changed in both");
    let path_text = String::from_utf8_lossy(path);
    merge_tree_print_entry_line("base", base, &path_text);
    merge_tree_print_entry_line("our", ours, &path_text);
    merge_tree_print_entry_line("their", theirs, &path_text);

    let base_content = read_index_entry_content(store, base)?;
    let ours_content = read_index_entry_content(store, ours)?;
    let theirs_content = read_index_entry_content(store, theirs)?;
    if is_binary_content(&base_content)
        || is_binary_content(&ours_content)
        || is_binary_content(&theirs_content)
    {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "merge-tree cannot yet render binary conflict for {}",
                String::from_utf8_lossy(path)
            ),
        });
    }
    let mut merged = Vec::new();
    merged.extend_from_slice(b"<<<<<<< .our\n");
    merged.extend_from_slice(&ours_content);
    if !ours_content.ends_with(b"\n") {
        merged.push(b'\n');
    }
    merged.extend_from_slice(b"=======\n");
    merged.extend_from_slice(&theirs_content);
    if !theirs_content.ends_with(b"\n") {
        merged.push(b'\n');
    }
    merged.extend_from_slice(b">>>>>>> .their\n");
    print_unified_full_file_hunk(&ours_content, &merged, &path_text)
}

fn merge_tree_print_diff(
    store: &LooseObjectStore,
    path: &[u8],
    old_entry: Option<&IndexEntry>,
    new_entry: Option<&IndexEntry>,
) -> Result<()> {
    let old_content = old_entry
        .map(|entry| read_index_entry_content(store, entry))
        .transpose()?
        .unwrap_or_default();
    let new_content = new_entry
        .map(|entry| read_index_entry_content(store, entry))
        .transpose()?
        .unwrap_or_default();
    if old_content.is_empty() && new_content.is_empty() {
        return Ok(());
    }
    if is_binary_content(&old_content) || is_binary_content(&new_content) {
        println!("Binary files {} differ", String::from_utf8_lossy(path));
        return Ok(());
    }
    print_unified_full_file_hunk(&old_content, &new_content, &String::from_utf8_lossy(path))
}

fn merge_tree_print_entry_line(label: &str, entry: &IndexEntry, path: &str) {
    println!(
        "  {label:<6} {} {} {path}",
        index_mode_octal(entry.mode),
        entry.id.to_hex()
    );
}
