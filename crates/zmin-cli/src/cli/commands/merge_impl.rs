use super::*;

pub(crate) struct MergeOptions {
    pub(crate) abort: bool,
    pub(crate) continue_: bool,
    pub(crate) ff_only: bool,
    pub(crate) no_ff: bool,
    pub(crate) show_diffstat: bool,
    pub(crate) no_commit: bool,
    pub(crate) log_limit: Option<usize>,
    pub(crate) squash: bool,
    pub(crate) signoff: bool,
    pub(crate) gpg_sign: Option<String>,
    pub(crate) no_gpg_sign: bool,
    pub(crate) quiet: bool,
    pub(crate) allow_unrelated_histories: bool,
    pub(crate) strategies: Vec<String>,
    pub(crate) strategy_options: Vec<String>,
    pub(crate) commits: Vec<String>,
    pub(crate) commit_label: Option<String>,
    pub(crate) commit_source: Option<String>,
}

pub(crate) fn merge(options: MergeOptions) -> Result<()> {
    let MergeOptions {
        abort,
        continue_,
        ff_only,
        no_ff,
        show_diffstat,
        no_commit,
        log_limit,
        squash,
        signoff,
        gpg_sign,
        no_gpg_sign,
        quiet,
        allow_unrelated_histories,
        strategies,
        strategy_options,
        commits,
        commit_label,
        commit_source,
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

    let commit_cache = CommitObjectCache::new(&store);
    if ff_only && !no_ff {
        return merge_ff_only(&repo, &store, &commit_cache, &commits[0]);
    }
    let mode = MergeCommitMode { no_commit, squash };
    if !strategies.is_empty() {
        return merge_with_strategy(
            &repo,
            &store,
            &commit_cache,
            &commits[0],
            commit_label.as_deref(),
            &strategies,
            &strategy_options,
            allow_unrelated_histories,
            !no_ff && !squash,
            show_diffstat,
            log_limit,
            mode,
            signoff,
            gpg_sign.as_deref(),
            no_gpg_sign,
            quiet,
            commit_source.as_deref(),
        );
    }
    merge_commit(
        &repo,
        &store,
        &commit_cache,
        &commits[0],
        commit_label.as_deref(),
        "ort",
        &strategy_options,
        allow_unrelated_histories,
        !no_ff && !squash,
        show_diffstat,
        log_limit,
        mode,
        signoff,
        gpg_sign.as_deref(),
        no_gpg_sign,
        quiet,
        commit_source.as_deref(),
    )
}

fn merge_ff_only(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target: &str,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let target_id = resolve_commitish(repo, store, target)?;
    if head_id == target_id
        || is_ancestor_commit_with_repo_cached(repo, commit_cache, &target_id, &head_id)?
    {
        println!("Already up to date.");
        return Ok(());
    }
    if is_ancestor_commit_with_repo_cached(repo, commit_cache, &head_id, &target_id)? {
        return fast_forward_to_cached(repo, store, commit_cache, target, "merge", true);
    }
    if best_merge_base_with_repo_cached(repo, commit_cache, &head_id, &target_id)?.is_none() {
        return Err(CliError::Fatal {
            code: 128,
            message: "refusing to merge unrelated histories".into(),
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: "Not possible to fast-forward, aborting.".into(),
    })
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
    remove_file_if_exists(&repo.git_dir.join("AUTO_MERGE"))?;
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
    remove_file_if_exists(&repo.git_dir.join("MERGE_MODE"))?;
    remove_file_if_exists(&repo.git_dir.join("AUTO_MERGE"))?;
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
    strategy_options: &[String],
    allow_unrelated_histories: bool,
    allow_fast_forward: bool,
    show_diffstat: bool,
    log_limit: Option<usize>,
    mode: MergeCommitMode,
    signoff: bool,
    gpg_sign: Option<&str>,
    no_gpg_sign: bool,
    quiet: bool,
    commit_source: Option<&str>,
) -> Result<()> {
    if strategies.len() == 1 && strategies[0] == "ours" {
        return merge_ours_strategy(
            repo,
            store,
            commit_cache,
            target,
            target_label,
            signoff,
            gpg_sign,
            no_gpg_sign,
            quiet,
        );
    }
    if strategies.len() == 1 && matches!(strategies[0].as_str(), "ort" | "recursive") {
        return merge_commit(
            repo,
            store,
            commit_cache,
            target,
            target_label,
            &strategies[0],
            strategy_options,
            allow_unrelated_histories,
            allow_fast_forward,
            show_diffstat,
            log_limit,
            mode,
            signoff,
            gpg_sign,
            no_gpg_sign,
            quiet,
            commit_source,
        );
    }
    Err(CliError::Stderr {
        code: 1,
        text: format!(
            "Could not find merge strategy '{}'.\nAvailable strategies are: octopus ours recursive resolve subtree.\n",
            strategies.first().map(String::as_str).unwrap_or_default()
        ),
    })
}

fn merge_ours_strategy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target: &str,
    target_label: Option<&str>,
    signoff: bool,
    gpg_sign: Option<&str>,
    no_gpg_sign: bool,
    quiet: bool,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let target_id = resolve_commitish(repo, store, target)?;
    if head_id == target_id
        || is_ancestor_commit_with_repo_cached(repo, commit_cache, &target_id, &head_id)?
    {
        if !quiet {
            println!("Already up to date.");
        }
        return Ok(());
    }
    let head_commit = commit_cache.read_commit(&head_id)?;
    let author = signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = format!(
        "Merge branch '{}'\n",
        merge_display_name_or_label(repo, target, target_label)
    );
    let mut message = message.into_bytes();
    if signoff {
        super::commit_commands::append_commit_signoff(&mut message, &committer)?;
    }
    let mut builder = CommitBuilder::new(head_commit.tree.clone(), author, committer.clone())
        .parent(head_id)
        .parent(target_id)
        .message(message)?;
    if !no_gpg_sign {
        if let Some(signature) =
            super::commit_commands::commit_tree_gpg_signature(repo, &builder, gpg_sign)?
        {
            builder = builder.gpg_signature(signature)?;
        }
    }
    let commit = builder.encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    if !quiet {
        println!("Merge made by the 'ours' strategy.");
    }
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
    strategy_options: &[String],
    allow_unrelated_histories: bool,
    allow_fast_forward: bool,
    show_diffstat: bool,
    log_limit: Option<usize>,
    mode: MergeCommitMode,
    signoff: bool,
    gpg_sign: Option<&str>,
    no_gpg_sign: bool,
    quiet: bool,
    commit_source: Option<&str>,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let target_id = resolve_commitish(repo, store, target)?;
    if head_id == target_id
        || is_ancestor_commit_with_repo_cached(repo, commit_cache, &target_id, &head_id)?
    {
        if !quiet {
            println!("Already up to date.");
        }
        return Ok(());
    }
    if allow_fast_forward
        && is_ancestor_commit_with_repo_cached(repo, commit_cache, &head_id, &target_id)?
    {
        return fast_forward_to_cached(repo, store, commit_cache, target, "merge", false);
    }

    let base_id = best_merge_base_with_repo_cached(repo, commit_cache, &head_id, &target_id)?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let target_commit = commit_cache.read_commit(&target_id)?;
    let tree_cache = TreeObjectCache::new(store);
    let base = if let Some(base_id) = base_id.as_ref() {
        let base_commit = commit_cache.read_commit(base_id)?;
        read_commit_tree_index_cached(&tree_cache, &base_commit)?
    } else if allow_unrelated_histories {
        GitIndex::new()
    } else {
        return Err(CliError::Fatal {
            code: 128,
            message: "refusing to merge unrelated histories".into(),
        });
    };
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
            if let Some(resolved) =
                resolve_strategy_option_conflicts(store, &index, &files, strategy_options)?
            {
                for path in &resolved.auto_merged_paths {
                    if !quiet {
                        println!("Auto-merging {}", String::from_utf8_lossy(path));
                    }
                }
                resolved.index
            } else {
                remove_tracked_paths_missing_from_target(repo, &ours, &index)?;
                checkout_merged_stage_zero(repo, store, &index)?;
                for file in files {
                    write_worktree_file(repo, &file.path, &file.content)?;
                    match &file.kind {
                        MergeConflictKind::Binary => {
                            if !quiet {
                                println!(
                                    "warning: Cannot merge binary files: {} (HEAD vs. {})",
                                    String::from_utf8_lossy(&file.path),
                                    merge_display_name(repo, target)
                                );
                                println!("Auto-merging {}", String::from_utf8_lossy(&file.path));
                            }
                            eprintln!(
                                "CONFLICT (content): Merge conflict in {}",
                                String::from_utf8_lossy(&file.path)
                            );
                        }
                        MergeConflictKind::Content => {
                            if !quiet {
                                println!("Auto-merging {}", String::from_utf8_lossy(&file.path));
                            }
                            eprintln!(
                                "CONFLICT (content): Merge conflict in {}",
                                String::from_utf8_lossy(&file.path)
                            );
                        }
                        MergeConflictKind::ModifyDelete { message } => {
                            if !quiet {
                                println!("{message}");
                            }
                        }
                        MergeConflictKind::RenameDelete { message } => {
                            if !quiet {
                                println!("{message}");
                            }
                        }
                    }
                }
                index.write_to_path(&repo.index_path)?;
                write_merge_state(repo, &target_id, &merge_display_name(repo, target), true)?;
                eprintln!("Automatic merge failed; fix conflicts and then commit the result.");
                return Err(CliError::Exit(1));
            }
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
        write_auto_merge(repo, store, &merged)?;
        write_squash_message(repo, commit_cache, &target_id)?;
        println!("Squash commit -- not updating HEAD");
        eprintln!("Automatic merge went well; stopped before committing as requested");
        return Ok(());
    }
    if mode.no_commit {
        write_auto_merge(repo, store, &merged)?;
        let target_label = merge_display_name_or_label(repo, target, target_label);
        write_merge_state(repo, &target_id, &target_label, false)?;
        eprintln!("Automatic merge went well; stopped before committing as requested");
        return Ok(());
    }
    let tree = write_tree_from_index(store, &merged)?;
    let author = signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = build_merge_commit_message(
        repo,
        commit_cache,
        &head_id,
        &target_id,
        base_id.as_ref(),
        target,
        target_label,
        commit_source,
        log_limit,
    )?;
    let mut message = message.into_bytes();
    if signoff {
        super::commit_commands::append_commit_signoff(&mut message, &committer)?;
    }
    let mut builder = CommitBuilder::new(tree, author, committer)
        .parent(head_id.clone())
        .parent(target_id.clone())
        .message(message)?;
    if !no_gpg_sign {
        if let Some(signature) =
            super::commit_commands::commit_tree_gpg_signature(repo, &builder, gpg_sign)?
        {
            builder = builder.gpg_signature(signature)?;
        }
    }
    let commit = builder.encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    if !quiet {
        println!("Merge made by the '{strategy_label}' strategy.");
    }
    if show_diffstat && !quiet {
        print_merge_commit_stat(repo, store, &ours, &merged)?;
    }
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

fn build_merge_commit_message(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    head_id: &ObjectId,
    target_id: &ObjectId,
    base_id: Option<&ObjectId>,
    target: &str,
    target_label: Option<&str>,
    commit_source: Option<&str>,
    log_limit: Option<usize>,
) -> Result<String> {
    let label = merge_display_name_or_label(repo, target, target_label);
    let mut message = build_merge_commit_subject(repo, target, &label, commit_source)?;
    let Some(log_limit) = log_limit else {
        return Ok(message);
    };
    let Some(base_id) = base_id else {
        return Ok(message);
    };
    let subjects = collect_merge_log_subjects(commit_cache, target_id, base_id, head_id)?;
    if subjects.is_empty() {
        return Ok(message);
    }
    message.push('\n');
    if subjects.len() > log_limit {
        message.push_str(&format!("* {}: ({} commits)\n", label, subjects.len()));
        for subject in subjects.iter().take(log_limit) {
            message.push_str("  ");
            message.push_str(subject);
            message.push('\n');
        }
        message.push_str("  ...\n");
        return Ok(message);
    }
    message.push_str(&format!("* {}:\n", label));
    for subject in subjects {
        message.push_str("  ");
        message.push_str(&subject);
        message.push('\n');
    }
    Ok(message)
}

fn build_merge_commit_subject(
    repo: &GitRepo,
    target: &str,
    label: &str,
    commit_source: Option<&str>,
) -> Result<String> {
    if let Some(source) = commit_source
        && source != "."
    {
        return Ok(format!("Merge branch '{label}' of {source}\n"));
    }
    if symbolic_full_ref_name(repo, target)?
        .as_deref()
        .is_some_and(|name| name.starts_with("refs/remotes/"))
    {
        return Ok(format!("Merge remote-tracking branch '{label}'\n"));
    }
    Ok(format!("Merge branch '{label}'\n"))
}

struct ResolvedStrategyOptionConflicts {
    index: GitIndex,
    auto_merged_paths: Vec<Vec<u8>>,
}

fn resolve_strategy_option_conflicts(
    store: &LooseObjectStore,
    index: &GitIndex,
    files: &[MergeConflictFile],
    strategy_options: &[String],
) -> Result<Option<ResolvedStrategyOptionConflicts>> {
    let prefer_stage = if merge_tree_uses_theirs_strategy_options(strategy_options) {
        Some(3)
    } else if merge_tree_uses_ours_strategy_options(strategy_options) {
        Some(2)
    } else {
        None
    };
    let Some(prefer_stage) = prefer_stage else {
        return Ok(None);
    };
    let mut entries = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .cloned()
        .collect::<Vec<_>>();
    let mut auto_merged_paths = Vec::new();
    for file in files {
        if !matches!(file.kind, MergeConflictKind::Content) {
            return Ok(None);
        }
        let chosen = index
            .entry(&file.path, prefer_stage)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!(
                    "missing stage {prefer_stage} entry for {}",
                    String::from_utf8_lossy(&file.path)
                ),
            })?;
        let _ = read_index_entry_content(store, chosen)?;
        let mut merged = chosen.clone();
        merged.stage = 0;
        entries.push(merged);
        auto_merged_paths.push(file.path.clone());
    }
    Ok(Some(ResolvedStrategyOptionConflicts {
        index: GitIndex::from_entries(entries)?,
        auto_merged_paths,
    }))
}

fn collect_merge_log_subjects(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target_id: &ObjectId,
    base_id: &ObjectId,
    head_id: &ObjectId,
) -> Result<Vec<String>> {
    let mut subjects = Vec::new();
    let mut current = target_id.clone();
    while current != *base_id && current != *head_id {
        let commit = commit_cache.read_commit(&current)?;
        subjects.push(commit_subject(&commit.message));
        let Some(parent) = commit.parents.first() else {
            break;
        };
        current = parent.clone();
    }
    Ok(subjects)
}

fn write_merge_state(
    repo: &GitRepo,
    target_id: &ObjectId,
    target_label: &str,
    conflicted: bool,
) -> Result<()> {
    fs::write(
        repo.git_dir.join("MERGE_HEAD"),
        format!("{}\n", target_id.to_hex()),
    )?;
    let message = if conflicted {
        format!("Merge branch '{}'\n\n# Conflicts:\n", target_label)
    } else {
        format!("Merge branch '{}'\n", target_label)
    };
    fs::write(repo.git_dir.join("MERGE_MSG"), message)?;
    fs::write(repo.git_dir.join("MERGE_MODE"), "")?;
    Ok(())
}

fn write_auto_merge(repo: &GitRepo, store: &LooseObjectStore, index: &GitIndex) -> Result<()> {
    let tree = write_tree_from_index(store, index)?;
    fs::write(repo.git_dir.join("AUTO_MERGE"), tree.to_hex() + "\n")?;
    Ok(())
}

fn print_merge_commit_stat(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
) -> Result<()> {
    let entries = diff_indexes(old_index, new_index)?;
    let context = DiffIndexContext {
        repo,
        store,
        old_index,
        new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    print_stat_entries(
        &context,
        &entries,
        DiffStatOptions {
            whitespace_mode: DiffWhitespaceMode::None,
            relative_prefix: None,
            ignore_matching_lines: &[],
            ignore_blank_lines: false,
            compact_summary: false,
            color: false,
        },
    )?;
    print_summary_entries(old_index, new_index, &entries, None)
}

fn write_squash_message(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    target_id: &ObjectId,
) -> Result<()> {
    let target = commit_cache.read_commit(target_id)?;
    let author_name = signature_name(&target.author);
    let author_email = signature_email(&target.author);
    let date = signature_log_date(&target.author)?;
    let body = String::from_utf8_lossy(&target.message)
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    fs::write(
        repo.git_dir.join("SQUASH_MSG"),
        format!(
            "Squashed commit of the following:\n\ncommit {}\nAuthor: {} <{}>\nDate:   {}\n\n{}",
            target_id.to_hex(),
            author_name,
            author_email,
            date,
            body
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MergeFileConflictStyle {
    Markers,
    Ours,
    Theirs,
    Union,
}

impl MergeFileConflictStyle {
    pub(crate) fn from_flags(ours: bool, theirs: bool, union: bool) -> Self {
        if union {
            Self::Union
        } else if theirs {
            Self::Theirs
        } else if ours {
            Self::Ours
        } else {
            Self::Markers
        }
    }
}

pub(crate) fn merge_file_command(
    stdout: bool,
    _quiet: bool,
    conflict_style: MergeFileConflictStyle,
    diff3: bool,
    marker_size: Option<String>,
    diff_algorithm: Option<String>,
    object_id: bool,
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
    let marker_len = effective_merge_file_marker_size(marker_size)?;
    validate_merge_file_diff_algorithm(diff_algorithm.as_deref())?;
    let (current_content, base_content, other_content) = if object_id {
        read_merge_file_object_inputs(&current, &base, &other)?
    } else {
        (fs::read(&current)?, fs::read(&base)?, fs::read(&other)?)
    };
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
    let mut result = merge_file_core(
        &current_content,
        &base_content,
        &other_content,
        &merge_labels,
    );
    if result.conflicts > 0 {
        match conflict_style {
            MergeFileConflictStyle::Markers => {
                if diff3 {
                    result.content = merge_file_diff3_content(
                        &current_content,
                        &base_content,
                        &other_content,
                        &merge_labels,
                        marker_len.unwrap_or(7),
                    );
                } else if let Some(marker_len) = marker_len {
                    result.content = merge_file_marker_content(
                        &current_content,
                        &base_content,
                        &other_content,
                        &merge_labels,
                        marker_len,
                    );
                }
            }
            MergeFileConflictStyle::Ours => {
                result.content = current_content.clone();
                result.conflicts = 0;
            }
            MergeFileConflictStyle::Theirs => {
                result.content = other_content.clone();
                result.conflicts = 0;
            }
            MergeFileConflictStyle::Union => {
                result.content =
                    merge_file_union_content(&current_content, &base_content, &other_content);
                result.conflicts = 0;
            }
        }
    }
    if object_id && !stdout {
        let repo = find_repo()?;
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        let id = store.write_object(GitObjectKind::Blob, &result.content)?;
        println!("{}", id.to_hex());
    } else if stdout || object_id {
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

fn read_merge_file_object_inputs(
    current: &Path,
    base: &Path,
    other: &Path,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    Ok((
        read_merge_file_blob_object(&store, &current.to_string_lossy())?,
        read_merge_file_blob_object(&store, &base.to_string_lossy())?,
        read_merge_file_blob_object(&store, &other.to_string_lossy())?,
    ))
}

fn read_merge_file_blob_object(store: &LooseObjectStore, value: &str) -> Result<Vec<u8>> {
    let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, value).map_err(|_| CliError::Stderr {
        code: 255,
        text: format!("error: object '{value}' does not exist\n"),
    })?;
    let object = store.read_object(&id).map_err(|_| CliError::Stderr {
        code: 255,
        text: format!("error: object '{value}' does not exist\n"),
    })?;
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Stderr {
            code: 255,
            text: format!("error: object '{value}' is not a blob\n"),
        });
    }
    Ok(object.content)
}

fn validate_merge_file_diff_algorithm(value: Option<&str>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if matches!(value, "myers" | "minimal" | "patience" | "histogram") {
        return Ok(());
    }
    Err(CliError::Stderr {
        code: 129,
        text: "error: option diff-algorithm accepts \"myers\", \"minimal\", \"patience\" and \"histogram\"\n"
            .into(),
    })
}

fn parse_merge_file_marker_size(value: &str) -> Result<usize> {
    let Some(marker_len) = parse_scaled_usize(value) else {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: option `marker-size' expects an integer value with an optional k/m/g suffix\n"
                .into(),
        });
    };
    Ok(if marker_len == 0 { 7 } else { marker_len })
}

fn effective_merge_file_marker_size(parsed: Option<String>) -> Result<Option<usize>> {
    let raw_args: Vec<String> = std::env::args()
        .skip_while(|arg| arg != "merge-file")
        .collect();
    let mut effective = parsed;
    let mut index = 0usize;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--marker-size" => {
                if let Some(value) = raw_args.get(index + 1) {
                    effective = Some(value.clone());
                    index += 2;
                    continue;
                }
            }
            "--no-marker-size" => {
                effective = None;
                index += 1;
                continue;
            }
            arg => {
                if let Some(value) = arg.strip_prefix("--marker-size=") {
                    effective = Some(value.to_owned());
                }
            }
        }
        index += 1;
    }
    effective
        .map(|value| parse_merge_file_marker_size(&value))
        .transpose()
}

fn parse_scaled_usize(value: &str) -> Option<usize> {
    let (digits, multiplier) = match value.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&value[..value.len() - 1], 1024usize),
        Some(b'm' | b'M') => (&value[..value.len() - 1], 1024usize.checked_mul(1024)?),
        Some(b'g' | b'G') => (
            &value[..value.len() - 1],
            1024usize.checked_mul(1024)?.checked_mul(1024)?,
        ),
        _ => (value, 1usize),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()?.checked_mul(multiplier)
}

fn merge_file_diff3_content(
    current: &[u8],
    ancestor: &[u8],
    other: &[u8],
    labels: &MergeFileLabels,
    marker_len: usize,
) -> Vec<u8> {
    let current_lines = merge_file_split_lines(current);
    let ancestor_lines = merge_file_split_lines(ancestor);
    let other_lines = merge_file_split_lines(other);
    let (prefix_len, suffix_len) =
        merge_file_common_edges(&current_lines, &ancestor_lines, &other_lines);
    let mut out = Vec::new();
    merge_file_append_lines(&mut out, &current_lines[..prefix_len]);
    merge_file_append_marker(&mut out, b'<', marker_len, Some(&labels.current));
    merge_file_append_lines(
        &mut out,
        &current_lines[prefix_len..current_lines.len() - suffix_len],
    );
    merge_file_append_marker(&mut out, b'|', marker_len, Some(&labels.ancestor));
    merge_file_append_lines(
        &mut out,
        &ancestor_lines[prefix_len..ancestor_lines.len() - suffix_len],
    );
    merge_file_append_marker(&mut out, b'=', marker_len, None);
    merge_file_append_lines(
        &mut out,
        &other_lines[prefix_len..other_lines.len() - suffix_len],
    );
    merge_file_append_marker(&mut out, b'>', marker_len, Some(&labels.other));
    merge_file_append_lines(&mut out, &current_lines[current_lines.len() - suffix_len..]);
    out
}

fn merge_file_marker_content(
    current: &[u8],
    ancestor: &[u8],
    other: &[u8],
    labels: &MergeFileLabels,
    marker_len: usize,
) -> Vec<u8> {
    let current_lines = merge_file_split_lines(current);
    let ancestor_lines = merge_file_split_lines(ancestor);
    let other_lines = merge_file_split_lines(other);
    let (prefix_len, suffix_len) =
        merge_file_common_edges(&current_lines, &ancestor_lines, &other_lines);
    let mut out = Vec::new();
    merge_file_append_lines(&mut out, &current_lines[..prefix_len]);
    merge_file_append_marker(&mut out, b'<', marker_len, Some(&labels.current));
    merge_file_append_lines(
        &mut out,
        &current_lines[prefix_len..current_lines.len() - suffix_len],
    );
    merge_file_append_marker(&mut out, b'=', marker_len, None);
    merge_file_append_lines(
        &mut out,
        &other_lines[prefix_len..other_lines.len() - suffix_len],
    );
    merge_file_append_marker(&mut out, b'>', marker_len, Some(&labels.other));
    merge_file_append_lines(&mut out, &current_lines[current_lines.len() - suffix_len..]);
    out
}

fn merge_file_append_marker(out: &mut Vec<u8>, byte: u8, len: usize, label: Option<&str>) {
    out.extend(std::iter::repeat_n(byte, len));
    if let Some(label) = label {
        out.push(b' ');
        out.extend_from_slice(label.as_bytes());
    }
    out.push(b'\n');
}

fn merge_file_union_content(current: &[u8], ancestor: &[u8], other: &[u8]) -> Vec<u8> {
    let current_lines = merge_file_split_lines(current);
    let ancestor_lines = merge_file_split_lines(ancestor);
    let other_lines = merge_file_split_lines(other);
    let (prefix_len, suffix_len) =
        merge_file_common_edges(&current_lines, &ancestor_lines, &other_lines);
    let mut out = Vec::new();
    merge_file_append_lines(&mut out, &current_lines[..prefix_len]);
    merge_file_append_lines(
        &mut out,
        &current_lines[prefix_len..current_lines.len() - suffix_len],
    );
    merge_file_append_lines(
        &mut out,
        &other_lines[prefix_len..other_lines.len() - suffix_len],
    );
    merge_file_append_lines(&mut out, &current_lines[current_lines.len() - suffix_len..]);
    out
}

fn merge_file_common_edges<'a>(
    current_lines: &[&'a [u8]],
    ancestor_lines: &[&'a [u8]],
    other_lines: &[&'a [u8]],
) -> (usize, usize) {
    let mut prefix_len = 0usize;
    while prefix_len < current_lines.len()
        && prefix_len < ancestor_lines.len()
        && prefix_len < other_lines.len()
        && current_lines[prefix_len] == ancestor_lines[prefix_len]
        && ancestor_lines[prefix_len] == other_lines[prefix_len]
    {
        prefix_len += 1;
    }
    let mut suffix_len = 0usize;
    while prefix_len + suffix_len < current_lines.len()
        && prefix_len + suffix_len < ancestor_lines.len()
        && prefix_len + suffix_len < other_lines.len()
        && current_lines[current_lines.len() - suffix_len - 1]
            == ancestor_lines[ancestor_lines.len() - suffix_len - 1]
        && ancestor_lines[ancestor_lines.len() - suffix_len - 1]
            == other_lines[other_lines.len() - suffix_len - 1]
    {
        suffix_len += 1;
    }
    (prefix_len, suffix_len)
}

fn merge_file_split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            lines.push(&bytes[start..=index]);
            start = index + 1;
        }
    }
    if start < bytes.len() {
        lines.push(&bytes[start..]);
    }
    lines
}

fn merge_file_append_lines(out: &mut Vec<u8>, lines: &[&[u8]]) {
    for line in lines {
        out.extend_from_slice(line);
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
    all: u8,
    paths: Vec<String>,
) -> Result<()> {
    if merge_program != "git-merge-one-file" && merge_program != "merge-one-file" {
        return Err(CliError::Fatal {
            code: 129,
            message: "merge-index currently supports git-merge-one-file only".into(),
        });
    }
    if all == 0 && paths.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "merge-index requires -a or at least one path".into(),
        });
    }
    if all == 0 {
        for path in &paths {
            let _ = normalize_git_path(path)?;
        }
        return Ok(());
    }
    let repo = find_repo()?;
    let index = read_repo_index(&repo)?;
    let selected = merge_index_unmerged_paths(&index);
    let mut failed = false;
    for _ in 0..all {
        for path in &selected {
            let (base, ours, theirs) = merge_index_stages(&index, path);
            let path_text = String::from_utf8_lossy(path);
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
    }
    if failed {
        Err(CliError::Exit(1))
    } else {
        Ok(())
    }
}

pub(crate) fn mergetool(
    tool: Option<&str>,
    tool_help: bool,
    no_prompt: bool,
    prompt: bool,
    orderfile: Option<PathBuf>,
    paths: Vec<PathBuf>,
) -> Result<()> {
    if tool_help {
        return show_mergetool_tool_help();
    }
    let repo = find_repo()?;
    let tool = match tool {
        Some(tool) => tool.to_owned(),
        None => read_config_value(&repo, "merge.tool")?.ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "no merge tool configured; set merge.tool or pass --tool".into(),
        })?,
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut index = read_repo_index(&repo)?;
    let selected = selected_mergetool_paths(&repo, &index, orderfile.as_deref(), &paths)?;
    if selected.is_empty() {
        println!("No files need merging");
        return Ok(());
    }
    println!("Merging:");
    for path in &selected {
        println!("{}", String::from_utf8_lossy(path));
    }
    println!();
    let command = read_config_value(&repo, &format!("mergetool.{tool}.cmd"))?.ok_or_else(|| {
        CliError::Stderr {
            code: 1,
            text: format!("error: mergetool.{tool}.cmd not set for tool '{tool}'\n"),
        }
    })?;
    let prompt = prompt && !no_prompt;
    for (path_index, path) in selected.into_iter().enumerate() {
        if path_index > 0 {
            println!();
        }
        run_mergetool_path(&repo, &store, &mut index, &tool, &command, prompt, &path)?;
    }
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn selected_mergetool_paths(
    repo: &GitRepo,
    index: &GitIndex,
    orderfile: Option<&Path>,
    paths: &[PathBuf],
) -> Result<Vec<Vec<u8>>> {
    let mut selected = merge_index_unmerged_paths(index);
    if let Some(orderfile) = orderfile {
        selected = apply_mergetool_order_file(selected, orderfile)?;
    }
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

fn apply_mergetool_order_file(paths: Vec<Vec<u8>>, orderfile: &Path) -> Result<Vec<Vec<u8>>> {
    let patterns = read_diff_order_patterns(orderfile)?;
    if patterns.is_empty() {
        return Ok(paths);
    }
    let mut ranked = paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let rank = patterns
                .iter()
                .position(|pattern| diff_order_pattern_matches(pattern, &path))
                .unwrap_or(usize::MAX);
            (rank, index, path)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, index, _)| (*rank, *index));
    Ok(ranked.into_iter().map(|(_, _, path)| path).collect())
}

fn show_mergetool_tool_help() -> Result<()> {
    let output = ProcessCommand::new(stock_git_binary())
        .args(["mergetool", "--tool-help"])
        .output()
        .map_err(CliError::Io)?;
    io::stdout()
        .write_all(&output.stdout)
        .map_err(CliError::Io)?;
    io::stderr()
        .write_all(&output.stderr)
        .map_err(CliError::Io)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::Exit(output.status.code().unwrap_or(1)))
    }
}

fn stock_git_binary() -> &'static Path {
    static STOCK_GIT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    STOCK_GIT.get_or_init(resolve_stock_git_binary).as_path()
}

fn resolve_stock_git_binary() -> PathBuf {
    for candidate in stock_git_candidates() {
        if is_stock_git_binary(&candidate) {
            return candidate;
        }
    }
    for path in std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .flat_map(|dir| {
            stock_git_names()
                .into_iter()
                .map(move |name| dir.join(name))
        })
    {
        if is_stock_git_binary(&path) {
            return path;
        }
    }
    PathBuf::from("/usr/bin/git")
}

fn stock_git_candidates() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            PathBuf::from(r"C:\Program Files\Git\cmd\git.exe"),
            PathBuf::from(r"C:\Program Files\Git\bin\git.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Git\cmd\git.exe"),
            PathBuf::from(r"C:\Program Files (x86)\Git\bin\git.exe"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/usr/bin/git"), PathBuf::from("/bin/git")]
    }
}

fn stock_git_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["git.exe", "git"]
    } else {
        vec!["git"]
    }
}

fn is_stock_git_binary(path: &Path) -> bool {
    let Ok(output) = ProcessCommand::new(path).arg("--version").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let version = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    version.starts_with("git version ") && !version.contains("zmin")
}

fn run_mergetool_path(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    tool: &str,
    command: &str,
    prompt: bool,
    path: &[u8],
) -> Result<()> {
    let (base, ours, theirs) = merge_index_stages(index, path);
    let stages = MergetoolStages { base, ours, theirs };
    let path_text = String::from_utf8_lossy(path);
    println!("Normal merge conflict for '{path_text}':");
    println!("  {{local}}: modified file");
    println!("  {{remote}}: modified file");
    if prompt {
        print!("Hit return to start merge resolution tool ({tool}): ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
    }
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
        fs::copy(&merged_path, mergetool_backup_path(&merged_path))?;
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

fn mergetool_backup_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_default();
    name.push(".orig");
    path.with_file_name(name)
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
    if options.write_tree {
        return merge_tree_write_tree(options);
    }
    if options.messages
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
            code: 128,
            message: "--trivial-merge is incompatible with all other options".into(),
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

fn merge_tree_write_tree(options: MergeTreeOptions) -> Result<()> {
    if options.trivial_merge {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: options '--write-tree' and '--trivial-merge' cannot be used together\n"
                .into(),
        });
    }
    if options.stdin {
        return merge_tree_write_tree_stdin(&options);
    }
    if options.args.len() != 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git merge-tree --write-tree <branch1> <branch2>".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let result = merge_tree_write_tree_once(
        &repo,
        &store,
        &commit_cache,
        &options,
        &options.args[0],
        &options.args[1],
    )?;
    merge_tree_emit_write_tree_result(&result, &options, false)?;
    if result.conflicted {
        return Err(CliError::Exit(1));
    }
    Ok(())
}

fn merge_tree_write_tree_stdin(options: &MergeTreeOptions) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut out = io::stdout().lock();
    for line in input.lines() {
        let mut parts = line.split_whitespace();
        let Some(ours) = parts.next() else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("malformed input line: '{line}'."),
            });
        };
        let Some(theirs) = parts.next() else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("malformed input line: '{line}'."),
            });
        };
        if parts.next().is_some() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("malformed input line: '{line}'."),
            });
        }
        let result =
            merge_tree_write_tree_once(&repo, &store, &commit_cache, options, ours, theirs)?;
        out.write_all(b"0\0")?;
        merge_tree_write_result_to(&mut out, &result, options, true, true)?;
        out.write_all(b"\0")?;
    }
    Ok(())
}

struct MergeTreeWriteResult {
    tree: ObjectId,
    conflicted: bool,
    stages: Vec<IndexEntry>,
    messages: Vec<MergeTreeConflictMessage>,
}

struct MergeTreeConflictMessage {
    path: Vec<u8>,
    reason: &'static str,
    text: String,
}

fn merge_tree_write_tree_once(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    options: &MergeTreeOptions,
    ours: &str,
    theirs: &str,
) -> Result<MergeTreeWriteResult> {
    let ours_id = resolve_commitish(repo, store, ours)?;
    let theirs_id = resolve_commitish(repo, store, theirs)?;
    let ours_commit = commit_cache.read_commit(&ours_id)?;
    if merge_tree_uses_ours_strategy(options) {
        return Ok(MergeTreeWriteResult {
            tree: ours_commit.tree.clone(),
            conflicted: false,
            stages: Vec::new(),
            messages: Vec::new(),
        });
    }
    let theirs_commit = commit_cache.read_commit(&theirs_id)?;
    if merge_tree_uses_theirs_strategy(options) {
        return Ok(MergeTreeWriteResult {
            tree: theirs_commit.tree.clone(),
            conflicted: false,
            stages: Vec::new(),
            messages: Vec::new(),
        });
    }
    let tree_cache = TreeObjectCache::new(store);
    let base = if let Some(base) = &options.merge_base {
        merge_tree_resolve_merge_base_index(repo, store, &tree_cache, base)?
    } else {
        if let Some(base_id) = best_merge_base_cached(commit_cache, &ours_id, &theirs_id)? {
            let base_commit = commit_cache.read_commit(&base_id)?;
            read_commit_tree_index_cached(&tree_cache, &base_commit)?
        } else if options.allow_unrelated_histories {
            GitIndex::new()
        } else {
            return Err(CliError::Fatal {
                code: 128,
                message: "refusing to merge unrelated histories".into(),
            });
        }
    };
    let ours_index = read_commit_tree_index_cached(&tree_cache, &ours_commit)?;
    let theirs_index = read_commit_tree_index_cached(&tree_cache, &theirs_commit)?;
    match merge_indexes(store, &base, &ours_index, &theirs_index, theirs)? {
        MergeIndexResult::Clean(index) => Ok(MergeTreeWriteResult {
            tree: write_tree_from_index(store, &index)?,
            conflicted: false,
            stages: Vec::new(),
            messages: Vec::new(),
        }),
        MergeIndexResult::Conflicted { index, files } => {
            let tree = write_tree_from_index(
                store,
                &merge_tree_automerge_index(
                    store,
                    &index,
                    &files,
                    &ours_id.to_hex(),
                    &theirs_id.to_hex(),
                )?,
            )?;
            let messages = files
                .iter()
                .map(|file| MergeTreeConflictMessage {
                    path: file.path.clone(),
                    reason: match file.kind {
                        MergeConflictKind::Content => "CONFLICT (contents)",
                        MergeConflictKind::Binary => "CONFLICT (binary)",
                        MergeConflictKind::ModifyDelete { .. } => "CONFLICT (modify/delete)",
                        MergeConflictKind::RenameDelete { .. } => "CONFLICT (rename/delete)",
                    },
                    text: merge_tree_conflict_message_text(file),
                })
                .collect();
            Ok(MergeTreeWriteResult {
                tree,
                conflicted: true,
                stages: index
                    .entries()
                    .iter()
                    .filter(|entry| entry.stage != 0)
                    .cloned()
                    .collect(),
                messages,
            })
        }
    }
}

fn merge_tree_uses_ours_strategy(options: &MergeTreeOptions) -> bool {
    merge_tree_uses_ours_strategy_options(&options.strategy_options)
}

fn merge_tree_uses_theirs_strategy(options: &MergeTreeOptions) -> bool {
    merge_tree_uses_theirs_strategy_options(&options.strategy_options)
}

fn merge_tree_uses_ours_strategy_options(strategy_options: &[String]) -> bool {
    strategy_options.iter().any(|option| option == "ours")
}

fn merge_tree_uses_theirs_strategy_options(strategy_options: &[String]) -> bool {
    strategy_options.iter().any(|option| option == "theirs")
}

fn merge_tree_resolve_merge_base_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    merge_base: &str,
) -> Result<GitIndex> {
    let tree = resolve_treeish(repo, store, merge_base).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("could not parse as tree '{merge_base}'"),
    })?;
    Ok(tree_cache.read_tree_to_index(&tree)?)
}

fn merge_tree_automerge_index(
    store: &LooseObjectStore,
    index: &GitIndex,
    files: &[MergeConflictFile],
    ours_label: &str,
    theirs_label: &str,
) -> Result<GitIndex> {
    let mut entries = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .cloned()
        .collect::<Vec<_>>();
    for file in files {
        let mode = index
            .entry(&file.path, 2)
            .or_else(|| index.entry(&file.path, 3))
            .or_else(|| index.entry(&file.path, 1))
            .map(|entry| entry.mode)
            .unwrap_or(IndexMode::File);
        let content =
            merge_tree_automerge_file_content(store, index, file, ours_label, theirs_label)?;
        let id = store.write_object(GitObjectKind::Blob, &content)?;
        entries.push(IndexEntry::new(
            file.path.clone(),
            id,
            mode,
            content.len().min(u32::MAX as usize) as u32,
        )?);
    }
    Ok(GitIndex::from_entries(entries)?)
}

fn merge_tree_automerge_file_content(
    store: &LooseObjectStore,
    index: &GitIndex,
    file: &MergeConflictFile,
    ours_label: &str,
    theirs_label: &str,
) -> Result<Vec<u8>> {
    if !matches!(file.kind, MergeConflictKind::Content) {
        return Ok(file.content.clone());
    }
    let Some(ours) = index.entry(&file.path, 2) else {
        return Ok(file.content.clone());
    };
    let Some(theirs) = index.entry(&file.path, 3) else {
        return Ok(file.content.clone());
    };
    let ours_content = read_index_entry_content(store, ours)?;
    let theirs_content = read_index_entry_content(store, theirs)?;
    let mut content = Vec::new();
    content.extend_from_slice(b"<<<<<<< ");
    content.extend_from_slice(ours_label.as_bytes());
    content.push(b'\n');
    content.extend_from_slice(&ours_content);
    if !ours_content.ends_with(b"\n") {
        content.push(b'\n');
    }
    content.extend_from_slice(b"=======\n");
    content.extend_from_slice(&theirs_content);
    if !theirs_content.ends_with(b"\n") {
        content.push(b'\n');
    }
    content.extend_from_slice(b">>>>>>> ");
    content.extend_from_slice(theirs_label.as_bytes());
    content.push(b'\n');
    Ok(content)
}

fn merge_tree_conflict_message_text(file: &MergeConflictFile) -> String {
    match &file.kind {
        MergeConflictKind::Content => format!(
            "CONFLICT (content): Merge conflict in {}\n",
            String::from_utf8_lossy(&file.path)
        ),
        MergeConflictKind::Binary => format!(
            "CONFLICT (binary): Merge conflict in {}\n",
            String::from_utf8_lossy(&file.path)
        ),
        MergeConflictKind::ModifyDelete { message }
        | MergeConflictKind::RenameDelete { message } => {
            let mut message = message.clone();
            if !message.ends_with('\n') {
                message.push('\n');
            }
            message
        }
    }
}

fn merge_tree_emit_write_tree_result(
    result: &MergeTreeWriteResult,
    options: &MergeTreeOptions,
    force_nul: bool,
) -> Result<()> {
    let mut out = io::stdout().lock();
    merge_tree_write_result_to(&mut out, result, options, force_nul, false)
}

fn merge_tree_write_result_to<W: Write>(
    out: &mut W,
    result: &MergeTreeWriteResult,
    options: &MergeTreeOptions,
    force_nul: bool,
    stdin_record: bool,
) -> Result<()> {
    if options.quiet {
        return Ok(());
    }
    let nul = force_nul || options.nul_terminated;
    let sep = if nul { b"\0" as &[u8] } else { b"\n" as &[u8] };
    out.write_all(result.tree.to_hex().as_bytes())?;
    out.write_all(sep)?;
    if result.conflicted {
        if options.name_only {
            for message in &result.messages {
                out.write_all(&message.path)?;
                out.write_all(sep)?;
            }
        } else {
            for entry in &result.stages {
                write!(
                    out,
                    "{} {} {}\t{}",
                    index_mode_octal(entry.mode),
                    entry.id.to_hex(),
                    entry.stage,
                    String::from_utf8_lossy(&entry.path)
                )?;
                out.write_all(sep)?;
            }
        }
        if merge_tree_should_emit_messages(options) {
            out.write_all(sep)?;
            for message in &result.messages {
                if nul {
                    out.write_all(b"1\0")?;
                    out.write_all(&message.path)?;
                    out.write_all(b"\0Auto-merging\0Auto-merging ")?;
                    out.write_all(&message.path)?;
                    out.write_all(b"\n\0")?;
                    out.write_all(b"1\0")?;
                    out.write_all(&message.path)?;
                    out.write_all(b"\0")?;
                    out.write_all(message.reason.as_bytes())?;
                    out.write_all(b"\0")?;
                    out.write_all(message.text.as_bytes())?;
                    out.write_all(b"\0")?;
                } else {
                    writeln!(
                        out,
                        "Auto-merging {}",
                        String::from_utf8_lossy(&message.path)
                    )?;
                    write!(out, "{}", message.text)?;
                }
            }
        }
    }
    if stdin_record && !result.conflicted {
        out.write_all(sep)?;
    }
    Ok(())
}

fn merge_tree_should_emit_messages(options: &MergeTreeOptions) -> bool {
    !options.no_messages
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
