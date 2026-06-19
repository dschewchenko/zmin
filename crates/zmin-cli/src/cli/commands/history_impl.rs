use super::patch_commands::{
    ApplyFilePatch, PatchAnswers, apply_hunks_to_content, parse_apply_patches, select_patch_hunks,
};
use super::sequencer_commands::apply_tree_delta;
use super::*;
use std::io::{Read, Seek};

const REFLOG_REVERSE_READ_CHUNK_SIZE: usize = 16 * 1024;

pub(crate) fn run_replay(
    contained: bool,
    advance: Option<String>,
    onto: Option<String>,
    revision_ranges: Vec<String>,
) -> Result<()> {
    if contained && advance.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--advance' and '--contained' cannot be used together".into(),
        });
    }
    if advance.is_none() && onto.is_none() {
        return Err(CliError::Stderr {
            code: 129,
            text: replay_usage_error("exactly one of --onto, --advance, or --revert is required"),
        });
    }
    if revision_ranges.is_empty() {
        return Err(CliError::Stderr {
            code: 129,
            text: replay_usage_error("need a revision range"),
        });
    }

    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let revs = collect_rev_list_revs(&repo, &store, false, revision_ranges)?;
    let mut commits =
        collect_commits_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?;
    commits.reverse();
    let Some(first) = commits.first() else {
        return Ok(());
    };
    let first_commit = commit_cache.read_commit(first)?;
    let base = first_commit
        .parents
        .first()
        .cloned()
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "cannot replay a root commit yet".into(),
        })?;
    let onto_id = match onto {
        Some(onto) => resolve_commitish(&repo, &store, &onto)?,
        None => base,
    };
    let new_tip =
        replay_commit_chain(&repo, &store, &commit_cache, &tree_cache, &commits, onto_id)?;
    if let Some(branch) = advance {
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let branch_ref = branch_ref_name(&branch)?;
        refs.resolve(&branch_ref)?;
        refs.write_ref(&branch_ref, &new_tip)?;
    }
    Ok(())
}

fn replay_usage_error(message: &str) -> String {
    format!(
        "error: {message}\n\
         usage: (EXPERIMENTAL!) git replay ([--contained] --onto=<newbase> | --advance=<branch> | --revert=<branch>)\n\
         \x20      [--ref=<ref>] [--ref-action=<mode>] <revision-range>\n\n\
         \x20   --[no-]contained      update all branches that point at commits in <revision-range>\n\
         \x20   --onto <revision>     replay onto given commit\n\
         \x20   --advance <branch>    make replay advance given branch\n\
         \x20   --revert <branch>     revert commits onto given branch\n\
         \x20   --ref <branch>        reference to update with result\n\
         \x20   --ref-action <mode>   control ref update behavior (update|print)\n"
    )
}

fn replay_commit_chain(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
    mut parent: ObjectId,
) -> Result<ObjectId> {
    for commit_id in commits {
        let original = commit_cache.read_commit(commit_id)?;
        if original.parents.len() != 1 {
            return Err(CliError::Fatal {
                code: 128,
                message: "replay currently supports linear non-merge commits only".into(),
            });
        }
        let base_index =
            read_treeish_index_cached(repo, store, tree_cache, &original.parents[0].to_hex())?;
        let patch_index = tree_cache.read_tree_to_index(&original.tree)?;
        let current_index = read_treeish_index_cached(repo, store, tree_cache, &parent.to_hex())?;
        let next_index = apply_tree_delta(&base_index, &patch_index, &current_index)?;
        let tree = write_tree_from_index(store, &next_index)?;
        let author = signature_from_commit_bytes(&original.author)?;
        let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
        let encoded = CommitBuilder::new(tree, author, committer)
            .parent(parent)
            .message(original.message.clone())?
            .encode()?;
        parent = store.write_object(GitObjectKind::Commit, &encoded)?;
    }
    Ok(parent)
}

pub(crate) fn run_history(command: HistoryCommand) -> Result<()> {
    match command {
        HistoryCommand::Reword {
            commit,
            dry_run,
            update_refs,
        } => history_reword(&commit, dry_run, update_refs.as_deref()),
        HistoryCommand::Split {
            commit,
            dry_run,
            update_refs,
            pathspecs,
        } => history_split(&commit, dry_run, update_refs.as_deref(), pathspecs),
    }
}

fn history_reword(commit: &str, dry_run: bool, update_refs: Option<&str>) -> Result<()> {
    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let commit_id = resolve_commitish(&repo, &store, commit)?;
    history_validate_linear_descendants(&commit_cache, &commit_id)?;
    let original = commit_cache.read_commit(&commit_id)?;
    let message = edit_history_message(&repo, &original.message)?;
    let replacement =
        history_rewrite_single_commit(&store, &original, original.parents.clone(), message)?;
    let updates = history_ref_updates(HistoryRefUpdateContext {
        repo: &repo,
        refs: &refs,
        store: &store,
        commit_cache: &commit_cache,
        tree_cache: &tree_cache,
        original_id: &commit_id,
        replacement_id: &replacement,
        update_refs,
    })?;
    for (ref_name, old_id, new_id) in updates {
        if dry_run {
            println!(
                "update {} {} {}",
                ref_name,
                new_id.to_hex(),
                old_id.to_hex()
            );
        } else {
            refs.write_ref(&ref_name, &new_id)?;
        }
    }
    Ok(())
}

fn history_validate_linear_descendants(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commit_id: &ObjectId,
) -> Result<()> {
    let commit = commit_cache.read_commit(commit_id)?;
    if commit.parents.len() > 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "history does not support merge commits".into(),
        });
    }
    Ok(())
}

fn history_rewrite_single_commit(
    store: &LooseObjectStore,
    original: &zmin_git_core::CommitObject,
    parents: Vec<ObjectId>,
    message: Vec<u8>,
) -> Result<ObjectId> {
    let mut builder = CommitBuilder::new(
        original.tree.clone(),
        signature_from_commit_bytes(&original.author)?,
        signature_from_commit_bytes(&original.committer)?,
    );
    for parent in parents {
        builder = builder.parent(parent);
    }
    let encoded = builder.message(message)?.encode()?;
    Ok(store.write_object(GitObjectKind::Commit, &encoded)?)
}

struct HistoryRefUpdateContext<'a> {
    repo: &'a GitRepo,
    refs: &'a RefStore,
    store: &'a LooseObjectStore,
    commit_cache: &'a CommitObjectCache<'a, LooseObjectStore>,
    tree_cache: &'a TreeObjectCache<'a, LooseObjectStore>,
    original_id: &'a ObjectId,
    replacement_id: &'a ObjectId,
    update_refs: Option<&'a str>,
}

fn history_ref_updates(
    context: HistoryRefUpdateContext<'_>,
) -> Result<Vec<(String, ObjectId, ObjectId)>> {
    let HistoryRefUpdateContext {
        repo,
        refs,
        store,
        commit_cache,
        tree_cache,
        original_id,
        replacement_id,
        update_refs,
    } = context;
    let mode = update_refs.unwrap_or("branches");
    let candidates = match mode {
        "branches" => {
            let mut candidates = Vec::new();
            refs.for_each_ref_name("refs/heads/", |ref_name| {
                candidates.push(ref_name.to_owned());
                Ok::<(), CliError>(())
            })?;
            candidates
        }
        "head" => current_branch_ref(refs)?.into_iter().collect(),
        other => {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("invalid --update-refs value '{other}'"),
            });
        }
    };
    let mut updates = Vec::new();
    for ref_name in candidates {
        let old_tip = refs.resolve(&ref_name)?;
        if !is_ancestor_commit_cached(commit_cache, original_id, &old_tip)? {
            continue;
        }
        let new_tip = history_rewrite_tip(
            repo,
            store,
            commit_cache,
            tree_cache,
            original_id,
            replacement_id,
            &old_tip,
        )?;
        updates.push((ref_name, old_tip, new_tip));
    }
    Ok(updates)
}

fn history_rewrite_tip(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    original_id: &ObjectId,
    replacement_id: &ObjectId,
    old_tip: &ObjectId,
) -> Result<ObjectId> {
    if old_tip == original_id {
        return Ok(replacement_id.clone());
    }
    let range = format!("{}..{}", original_id.to_hex(), old_tip.to_hex());
    let revs = collect_rev_list_revs(repo, store, false, vec![range])?;
    let mut commits =
        collect_commits_with_exclusions_cached(repo, store, commit_cache, &revs, None)?;
    commits.reverse();
    replay_commit_chain(
        repo,
        store,
        commit_cache,
        tree_cache,
        &commits,
        replacement_id.clone(),
    )
}

fn history_split(
    commit: &str,
    dry_run: bool,
    update_refs: Option<&str>,
    pathspecs: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let commit_id = resolve_commitish(&repo, &store, commit)?;
    history_validate_linear_descendants(&commit_cache, &commit_id)?;
    let original = commit_cache.read_commit(&commit_id)?;
    let parent = original
        .parents
        .first()
        .cloned()
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "cannot split a root commit yet".into(),
        })?;
    let pathspecs = pathspecs
        .into_iter()
        .filter(|pathspec| pathspec != "--")
        .map(|pathspec| path_arg_to_repo_relative_allow_root(&repo, Path::new(&pathspec)))
        .collect::<Result<Vec<_>>>()?;
    let base_index = read_treeish_index_cached(&repo, &store, &tree_cache, &parent.to_hex())?;
    let original_index = tree_cache.read_tree_to_index(&original.tree)?;
    let all_entries = diff_indexes(&base_index, &original_index)?;
    let total_count =
        history_split_hunk_count(&repo, &store, &base_index, &original_index, &all_entries)?;
    let entries = all_entries
        .iter()
        .filter(|entry| diff_entry_matches_pathspec(entry, &pathspecs))
        .cloned()
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "no changes match the requested split".into(),
        });
    }
    let patches = history_split_patches(&repo, &store, &base_index, &original_index, &entries)?;
    let mut answers = PatchAnswers::read()?;
    let mut split_index = base_index.clone();
    let mut selected_count = 0_usize;
    for patch in patches {
        if patch.rename {
            return Err(CliError::Fatal {
                code: 128,
                message: "history split does not support rename hunks yet".into(),
            });
        }
        let target_path = patch
            .new_path
            .as_ref()
            .or(patch.old_path.as_ref())
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "patch has no target path".into(),
            })?
            .clone();
        let selected_hunks = select_patch_hunks(&patch, &mut answers)?;
        if selected_hunks.is_empty() {
            continue;
        }
        selected_count += selected_hunks.len();
        let base_entry = find_index_entry(&base_index, &target_path);
        let base = base_entry
            .map(|entry| read_index_entry_content(&store, entry))
            .transpose()?
            .unwrap_or_default();
        if patch.deleted && selected_hunks.len() == patch.hunks.len() {
            split_index.remove_path(&target_path)?;
            continue;
        }
        let content = apply_hunks_to_content(&base, &selected_hunks, &target_path)?;
        let mode = patch
            .new_mode
            .or_else(|| find_index_entry(&original_index, &target_path).map(|entry| entry.mode))
            .or_else(|| base_entry.map(|entry| entry.mode))
            .unwrap_or(IndexMode::File);
        upsert_index_content(&store, &mut split_index, target_path, content, mode)?;
    }
    if selected_count == 0 {
        return Err(CliError::Stderr {
            code: 1,
            text: "No changes selected\n".into(),
        });
    }
    if selected_count == total_count {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot split all hunks out of a commit".into(),
        });
    }
    let split_tree = write_tree_from_index(&store, &split_index)?;
    let split_message = edit_history_message(&repo, b"split-out commit\n")?;
    let rewritten_message = edit_history_message(&repo, &original.message)?;
    let split_commit = CommitBuilder::new(
        split_tree,
        signature_from_commit_bytes(&original.author)?,
        signature_from_commit_bytes(&original.committer)?,
    )
    .parent(parent)
    .message(split_message)?
    .encode()?;
    let split_id = store.write_object(GitObjectKind::Commit, &split_commit)?;
    let replacement =
        history_rewrite_single_commit(&store, &original, vec![split_id], rewritten_message)?;
    let updates = history_ref_updates(HistoryRefUpdateContext {
        repo: &repo,
        refs: &refs,
        store: &store,
        commit_cache: &commit_cache,
        tree_cache: &tree_cache,
        original_id: &commit_id,
        replacement_id: &replacement,
        update_refs,
    })?;
    for (ref_name, old_id, new_id) in updates {
        if dry_run {
            println!(
                "update {} {} {}",
                ref_name,
                new_id.to_hex(),
                old_id.to_hex()
            );
        } else {
            refs.write_ref(&ref_name, &new_id)?;
        }
    }
    Ok(())
}

fn history_split_hunk_count(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base_index: &GitIndex,
    original_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<usize> {
    Ok(
        history_split_patches(repo, store, base_index, original_index, entries)?
            .iter()
            .map(|patch| patch.hunks.len())
            .sum(),
    )
}

fn history_split_patches(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base_index: &GitIndex,
    original_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<Vec<ApplyFilePatch>> {
    let mut patch_bytes = Vec::new();
    write_patch_entries(
        &mut patch_bytes,
        repo,
        store,
        base_index,
        original_index,
        entries,
        PatchFormatOptions::cached(),
    )?;
    parse_apply_patches(&patch_bytes)
}

pub(crate) fn reflog(args: Vec<String>) -> Result<()> {
    let repo = find_repo()?;
    let mut args = args.into_iter().peekable();
    let command = args
        .next_if(|arg| {
            matches!(
                arg.as_str(),
                "show" | "list" | "exists" | "expire" | "delete" | "drop"
            )
        })
        .unwrap_or_else(|| "show".to_owned());
    match command.as_str() {
        "drop" => reflog_drop(&repo, args.collect()),
        "delete" => reflog_delete(&repo, args.collect()),
        "expire" => {
            let args = args.collect::<Vec<_>>();
            if args.iter().any(|arg| arg == "-h" || arg == "--help") {
                print!("{}", reflog_expire_usage());
                return Err(CliError::Exit(129));
            }
            reflog_expire(args)
        }
        "show" => {
            let mut date_mode = ReflogDateMode::Index;
            let mut no_abbrev_commit = false;
            let mut format = None;
            let mut ref_name = None;
            let mut pathspecs = Vec::new();
            let mut in_pathspecs = false;
            for arg in args {
                if in_pathspecs {
                    pathspecs.push(arg);
                } else if arg == "--" {
                    in_pathspecs = true;
                } else if arg == "-h" || arg == "--help" {
                    print!("{}", reflog_show_usage());
                    return Err(CliError::Exit(129));
                } else if arg == "--date=iso" {
                    date_mode = ReflogDateMode::Iso;
                } else if arg == "--date=unix" {
                    date_mode = ReflogDateMode::Unix;
                } else if arg == "--date=raw" {
                    date_mode = ReflogDateMode::Raw;
                } else if arg == "--no-abbrev-commit" {
                    no_abbrev_commit = true;
                } else if arg == "--date" {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "reflog --date requires --date=<format>".into(),
                    });
                } else if arg.starts_with("--date=") {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("unsupported reflog date format '{arg}'"),
                    });
                } else if let Some(value) = arg.strip_prefix("--format=") {
                    format = Some(value.to_owned());
                } else {
                    ref_name = Some(arg);
                }
            }
            reflog_show(
                &repo,
                ReflogShowOptions {
                    ref_name: ref_name.as_deref().unwrap_or("HEAD"),
                    date_mode,
                    no_abbrev_commit,
                    format: format.as_deref(),
                    pathspecs: &pathspecs,
                },
            )
        }
        "list" => {
            if let Some(arg) = args.next() {
                return Err(CliError::Stderr {
                    code: 1,
                    text: format!("error: list does not accept arguments: '{arg}'\n"),
                });
            }
            reflog_list(&repo)
        }
        "exists" => {
            let Some(ref_name) = args.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "reflog exists requires a ref".into(),
                });
            };
            if reflog_path(&repo, &ref_name)?.is_file() {
                Ok(())
            } else {
                Err(CliError::Exit(1))
            }
        }
        _ => Err(CliError::Fatal {
            code: 129,
            message: "reflog command was not supported".into(),
        }),
    }
}

fn reflog_expire_usage() -> &'static str {
    "usage: git reflog expire [--expire=<time>] [--expire-unreachable=<time>] [--rewrite] [--updateref] [--stale-fix] [--dry-run | -n] [--verbose] [--all [--single-worktree] | <refs>...]\n"
}

fn reflog_show_usage() -> &'static str {
    "usage: git reflog [show] [<log-options>] [<ref>]\n"
}

fn reflog_drop(repo: &GitRepo, args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: drop requires at least one ref\n".into(),
        });
    }
    if args.iter().any(|arg| arg == "--all") {
        if args.iter().any(|arg| !arg.starts_with('-')) {
            return Err(CliError::Stderr {
                code: 129,
                text: "usage: references specified along with --all\n".into(),
            });
        }
        for path in reflog_expire_paths(repo, &args)? {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(CliError::Io(error)),
            }
        }
        return Ok(());
    }
    let mut errors = String::new();
    for ref_name in args {
        let path = reflog_path(repo, &ref_name)?;
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                errors.push_str(&format!("error: reflog could not be found: '{ref_name}'\n"));
            }
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CliError::Stderr {
            code: 128,
            text: errors,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum ReflogDeleteSelector {
    Index(usize),
    Date(i64),
}

fn reflog_delete(repo: &GitRepo, args: Vec<String>) -> Result<()> {
    let selectors = args
        .into_iter()
        .filter(|arg| arg != "--rewrite" && arg != "--updateref")
        .collect::<Vec<_>>();
    if selectors.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "reflog delete requires at least one selector".into(),
        });
    }
    for selector in selectors {
        let (ref_name, selector) = parse_reflog_delete_selector(&selector)?;
        reflog_delete_one(repo, &ref_name, selector)?;
    }
    Ok(())
}

fn parse_reflog_delete_selector(raw: &str) -> Result<(String, ReflogDeleteSelector)> {
    let Some((ref_name, selector)) = raw.split_once("@{") else {
        return Err(ambiguous_revision_error(raw));
    };
    let Some(selector) = selector.strip_suffix('}') else {
        return Err(ambiguous_revision_error(raw));
    };
    let ref_name = if ref_name.is_empty() {
        "HEAD"
    } else {
        ref_name
    }
    .to_owned();
    if let Ok(index) = selector.parse::<usize>() {
        return Ok((ref_name, ReflogDeleteSelector::Index(index)));
    }
    let timestamp =
        parse_reflog_delete_date(selector).ok_or_else(|| ambiguous_revision_error(raw))?;
    Ok((ref_name, ReflogDeleteSelector::Date(timestamp)))
}

fn parse_reflog_delete_date(value: &str) -> Option<i64> {
    if let Ok((timestamp, _)) = parse_git_date(value) {
        return Some(timestamp);
    }
    chrono::DateTime::parse_from_str(value, "%d.%m.%Y.%H:%M:%S.%z")
        .ok()
        .map(|datetime| datetime.timestamp())
}

fn reflog_delete_one(repo: &GitRepo, ref_name: &str, selector: ReflogDeleteSelector) -> Result<()> {
    let path = reflog_path(repo, ref_name)?;
    let content = fs::read_to_string(&path)?;
    let mut lines = content.lines().map(str::to_owned).collect::<Vec<_>>();
    let Some(remove_index) = reflog_delete_line_index(&lines, selector) else {
        return Err(ambiguous_revision_error(&format!(
            "{ref_name}@{{{selector:?}}}"
        )));
    };
    lines.remove(remove_index);
    let output = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    fs::write(path, output).map_err(CliError::Io)
}

fn reflog_delete_line_index(lines: &[String], selector: ReflogDeleteSelector) -> Option<usize> {
    match selector {
        ReflogDeleteSelector::Index(index) => lines.len().checked_sub(index + 1),
        ReflogDeleteSelector::Date(timestamp) => lines.iter().rposition(|line| {
            parse_reflog_entry(line)
                .map(|entry| entry.timestamp <= timestamp)
                .unwrap_or(false)
        }),
    }
}

fn reflog_expire(args: Vec<String>) -> Result<()> {
    let dry_run = args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--dry-run" | "-n"));
    if dry_run {
        return Ok(());
    }
    let repo = find_repo()?;
    if reflog_expire_apply_pattern_config(&repo, &args)? {
        return Ok(());
    }
    if reflog_expire_policy_is_never(&repo, &args)? {
        return reflog_expire_noop(&repo, &args);
    }
    let stale_fix = args.iter().any(|arg| arg == "--stale-fix");
    if stale_fix {
        let refs = reflog_expire_refs(&repo, &args)?;
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        for ref_name in refs {
            reflog_expire_stale_fix_ref(&repo, &store, &ref_name)?;
        }
        return Ok(());
    }
    if let Some(expire_timestamp) = reflog_expire_timestamp(&args)? {
        return reflog_expire_by_timestamp(&repo, &args, expire_timestamp);
    }
    if args.iter().any(|arg| arg == "--all") {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 129,
        message: "reflog expire is not supported".into(),
    })
}

fn reflog_expire_policy_is_never(repo: &GitRepo, args: &[String]) -> Result<bool> {
    let mut expire = None::<String>;
    let mut expire_unreachable = None::<String>;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--expire" => expire = iter.next().cloned(),
            "--expire-unreachable" => expire_unreachable = iter.next().cloned(),
            value if value.starts_with("--expire=") => {
                expire = Some(value["--expire=".len()..].to_owned());
            }
            value if value.starts_with("--expire-unreachable=") => {
                expire_unreachable = Some(value["--expire-unreachable=".len()..].to_owned());
            }
            _ => {}
        }
    }
    let expire = expire
        .or(read_config_value(&repo, "gc.reflogexpire")?)
        .unwrap_or_default();
    let expire_unreachable = expire_unreachable
        .or(read_config_value(&repo, "gc.reflogexpireunreachable")?)
        .unwrap_or_default();
    Ok(reflog_expire_policy_value_is_never(&expire)
        && reflog_expire_policy_value_is_never(&expire_unreachable))
}

fn reflog_expire_apply_pattern_config(repo: &GitRepo, args: &[String]) -> Result<bool> {
    if reflog_expire_has_explicit_policy_arg(args) || args.iter().any(|arg| arg == "--all") {
        return Ok(false);
    }
    let refs = reflog_expire_refs(repo, args)?;
    if refs.is_empty() {
        return Ok(false);
    }
    let entries = read_config_entries(repo)?;
    if !entries.iter().any(|entry| {
        entry.section == "gc" && !entry.subsection.is_empty() && entry.key == "reflogexpire"
    }) {
        return Ok(false);
    }
    let verbose = args.iter().any(|arg| arg == "--verbose");
    for ref_name in refs {
        let canonical = reflog_expire_canonical_ref_name(repo, &ref_name)?;
        let expire = reflog_expire_config_value_for_ref(&entries, &canonical, "reflogexpire")
            .unwrap_or_default();
        if reflog_expire_policy_value_is_never(&expire) {
            reflog_expire_keep_ref(repo, &ref_name, verbose)?;
        } else if reflog_expire_policy_value_is_now(&expire) {
            let path = reflog_path(repo, &ref_name)?;
            if !path.is_file() {
                return Err(reflog_not_found_error(&ref_name));
            }
            reflog_expire_path_by_timestamp(&path, i64::MAX, verbose)?;
        } else {
            return Ok(false);
        }
    }
    Ok(true)
}

fn reflog_expire_has_explicit_policy_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        matches!(arg.as_str(), "--expire" | "--expire-unreachable")
            || arg.starts_with("--expire=")
            || arg.starts_with("--expire-unreachable=")
    })
}

fn reflog_expire_canonical_ref_name(repo: &GitRepo, ref_name: &str) -> Result<String> {
    if ref_name == "HEAD" || ref_name.starts_with("refs/") {
        return Ok(ref_name.to_owned());
    }
    if ref_name == "stash" {
        return Ok("refs/stash".to_owned());
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if let Some(branch) = branch_checkout_ref(&refs, ref_name)? {
        return Ok(branch);
    }
    Ok(ref_name.to_owned())
}

fn reflog_expire_config_value_for_ref(
    entries: &[ConfigEntry],
    ref_name: &str,
    key: &str,
) -> Option<String> {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == "gc"
                && entry.key == key
                && (entry.subsection.is_empty()
                    || reflog_expire_config_pattern_matches(&entry.subsection, ref_name))
        })
        .map(|entry| entry.value.clone())
}

fn reflog_expire_config_pattern_matches(pattern: &str, ref_name: &str) -> bool {
    if pattern
        .as_bytes()
        .iter()
        .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
    {
        wildcard_match(pattern, ref_name)
    } else {
        pattern == ref_name
    }
}

fn reflog_expire_timestamp(args: &[String]) -> Result<Option<i64>> {
    let mut expire = None::<String>;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--expire" => expire = iter.next().cloned(),
            value if value.starts_with("--expire=") => {
                expire = Some(value["--expire=".len()..].to_owned());
            }
            _ => {}
        }
    }
    let Some(expire) = expire else {
        return Ok(None);
    };
    let (timestamp, _) = parse_git_date(&expire)?;
    Ok(Some(timestamp))
}

fn reflog_expire_by_timestamp(repo: &GitRepo, args: &[String], timestamp: i64) -> Result<()> {
    let paths = reflog_expire_paths(repo, args)?;
    let verbose = args.iter().any(|arg| arg == "--verbose");
    for path in paths {
        reflog_expire_path_by_timestamp(&path, timestamp, verbose)?;
    }
    Ok(())
}

fn reflog_expire_paths(repo: &GitRepo, args: &[String]) -> Result<Vec<PathBuf>> {
    if args.iter().any(|arg| arg == "--all") {
        let mut paths = Vec::new();
        collect_reflog_file_paths(&repo.git_dir.join("logs"), &mut paths)?;
        if !args.iter().any(|arg| arg == "--single-worktree") {
            collect_linked_worktree_reflog_paths(repo, &mut paths)?;
        }
        paths.sort();
        return Ok(paths);
    }
    Ok(reflog_expire_refs(repo, args)?
        .into_iter()
        .map(|ref_name| reflog_path(repo, &ref_name))
        .collect::<Result<Vec<_>>>()?)
}

fn collect_linked_worktree_reflog_paths(repo: &GitRepo, paths: &mut Vec<PathBuf>) -> Result<()> {
    let worktrees = repo.git_dir.join("worktrees");
    let entries = match fs::read_dir(worktrees) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    for entry in entries {
        let logs = entry?.path().join("logs");
        collect_reflog_file_paths(&logs, paths)?;
    }
    Ok(())
}

fn collect_reflog_file_paths(path: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_reflog_file_paths(&path, paths)?;
        } else if path.is_file() {
            paths.push(path);
        }
    }
    Ok(())
}

fn reflog_expire_path_by_timestamp(path: &Path, timestamp: i64, verbose: bool) -> Result<()> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut kept = String::new();
    for line in content.lines() {
        let expire = parse_reflog_entry(line)
            .map(|entry| entry.timestamp <= timestamp)
            .unwrap_or(false);
        if expire {
            if verbose {
                println!("prune {line}");
            }
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    fs::write(path, kept).map_err(CliError::Io)
}

fn reflog_expire_policy_value_is_never(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "never" | "false" | "no" | "off" | "0"
    )
}

fn reflog_expire_policy_value_is_now(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "now" | "all")
}

fn reflog_expire_noop(repo: &GitRepo, args: &[String]) -> Result<()> {
    let refs = reflog_expire_refs(repo, args)?;
    let verbose = args.iter().any(|arg| arg == "--verbose");
    for ref_name in refs {
        reflog_expire_keep_ref(repo, &ref_name, verbose)?;
    }
    Ok(())
}

fn reflog_expire_keep_ref(repo: &GitRepo, ref_name: &str, verbose: bool) -> Result<()> {
    let path = reflog_path(repo, ref_name)?;
    let content = fs::read_to_string(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            reflog_not_found_error(ref_name)
        } else {
            CliError::Io(error)
        }
    })?;
    if verbose {
        for line in content.lines() {
            println!("keep {line}");
        }
    }
    Ok(())
}

fn reflog_not_found_error(ref_name: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("error: reflog could not be found: '{ref_name}'"),
    }
}

fn reflog_expire_refs(repo: &GitRepo, args: &[String]) -> Result<Vec<String>> {
    if args.iter().any(|arg| arg == "--all") {
        let logs_dir = repo.git_dir.join("logs");
        let mut names = Vec::new();
        collect_reflog_names(&logs_dir, &logs_dir, &mut names)?;
        return Ok(names);
    }
    let mut refs = Vec::new();
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--"
            || arg.starts_with("--")
            || matches!(
                arg.as_str(),
                "-n" | "--dry-run" | "--verbose" | "--single-worktree"
            )
        {
            if matches!(arg.as_str(), "--expire" | "--expire-unreachable") {
                iter.next();
            }
            continue;
        }
        refs.push(arg.to_owned());
    }
    Ok(refs)
}

fn reflog_expire_stale_fix_ref(
    repo: &GitRepo,
    store: &LooseObjectStore,
    ref_name: &str,
) -> Result<()> {
    let path = reflog_path(repo, ref_name)?;
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut kept = String::new();
    for line in content.lines() {
        let Some(entry) = parse_reflog_entry(line) else {
            kept.push_str(line);
            kept.push('\n');
            continue;
        };
        let mut seen = HashSet::new();
        if reflog_object_graph_complete(store, &entry.old_id, &mut seen)
            && reflog_object_graph_complete(store, &entry.new_id, &mut seen)
        {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    fs::write(path, kept).map_err(CliError::Io)
}

fn reflog_object_graph_complete(
    store: &LooseObjectStore,
    id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
) -> bool {
    if *id == zero_object_id() || !seen.insert(id.clone()) {
        return true;
    }
    let object = match store.packed_first().read_object(id) {
        Ok(object) => object,
        Err(_) => return false,
    };
    match object.kind {
        GitObjectKind::Blob => true,
        GitObjectKind::Tree => {
            let packed_first_store = store.packed_first();
            let entries = match read_tree(&packed_first_store, id) {
                Ok(entries) => entries,
                Err(_) => return false,
            };
            entries
                .iter()
                .all(|entry| reflog_object_graph_complete(store, &entry.id, seen))
        }
        GitObjectKind::Commit => {
            let commit = match decode_commit(id.algorithm(), &object.content) {
                Ok(commit) => commit,
                Err(_) => return false,
            };
            reflog_object_graph_complete(store, &commit.tree, seen)
                && commit
                    .parents
                    .iter()
                    .all(|parent| reflog_object_graph_complete(store, parent, seen))
        }
        GitObjectKind::Tag => {
            let tag = match decode_tag(id.algorithm(), &object.content) {
                Ok(tag) => tag,
                Err(_) => return false,
            };
            reflog_object_graph_complete(store, &tag.target, seen)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReflogDateMode {
    Index,
    Iso,
    Unix,
    Raw,
}

#[derive(Debug, Clone, Copy)]
struct ReflogShowOptions<'a> {
    ref_name: &'a str,
    date_mode: ReflogDateMode,
    no_abbrev_commit: bool,
    format: Option<&'a str>,
    pathspecs: &'a [String],
}

fn reflog_show(repo: &GitRepo, options: ReflogShowOptions<'_>) -> Result<()> {
    let ref_name = options.ref_name;
    if !reflog_show_pathspecs_match(repo, options.pathspecs)? {
        return Ok(());
    }
    let path = reflog_path(repo, ref_name)?;
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if ref_name == "HEAD" {
                return Ok(());
            }
            return Err(ambiguous_revision_error(ref_name));
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    let display = reflog_display_name(ref_name, options.no_abbrev_commit);
    let object_len = if options.no_abbrev_commit {
        GitHashAlgorithm::Sha1.digest_len() * 2
    } else {
        7
    };
    let mut index = 0usize;
    for_each_reflog_line_rev(file, |line| {
        let Some(entry) = parse_reflog_entry(line) else {
            return Ok(());
        };
        if options.format == Some("%H") {
            println!("{}", entry.new_id.to_hex());
            index += 1;
            return Ok(());
        }
        let selector = reflog_selector(index, &entry, options.date_mode)?;
        println!(
            "{} {}@{{{}}}: {}",
            short_object_id_len(&entry.new_id, object_len),
            display,
            selector,
            entry.message
        );
        index += 1;
        Ok(())
    })?;
    Ok(())
}

fn reflog_show_pathspecs_match(repo: &GitRepo, pathspecs: &[String]) -> Result<bool> {
    if pathspecs.is_empty() {
        return Ok(true);
    }
    let head_index = read_head_index(repo).ok();
    for pathspec in pathspecs {
        let relative = path_arg_to_repo_relative_allow_root(repo, Path::new(pathspec))?;
        if relative.is_empty() {
            return Ok(true);
        }
        let worktree_path = repo.root.join(String::from_utf8_lossy(&relative).as_ref());
        if path_exists(&worktree_path) {
            return Ok(true);
        }
        if head_index
            .as_ref()
            .and_then(|index| find_index_entry(index, &relative))
            .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn for_each_reflog_line_rev<F>(mut file: fs::File, mut on_line: F) -> Result<()>
where
    F: FnMut(&str) -> Result<()>,
{
    let mut position = file.seek(io::SeekFrom::End(0))?;
    let file_len = position;
    let mut chunk = vec![0u8; REFLOG_REVERSE_READ_CHUNK_SIZE];
    let mut suffix = Vec::new();
    while position > 0 {
        let read_len = usize::try_from(position.min(chunk.len() as u64)).unwrap_or(chunk.len());
        position -= read_len as u64;
        file.seek(io::SeekFrom::Start(position))?;
        file.read_exact(&mut chunk[..read_len])?;
        let bytes = &chunk[..read_len];
        let mut end = read_len;
        while let Some(newline) = bytes[..end].iter().rposition(|byte| *byte == b'\n') {
            let line = &bytes[newline + 1..end];
            let is_trailing_newline = position + end as u64 == file_len && line.is_empty();
            if !is_trailing_newline {
                emit_reflog_line(line, &suffix, &mut on_line)?;
            }
            suffix.clear();
            end = newline;
        }
        if end > 0 {
            prepend_reflog_line_prefix(&mut suffix, &bytes[..end]);
        }
    }
    if !suffix.is_empty() {
        emit_reflog_line(&[], &suffix, &mut on_line)?;
    }
    Ok(())
}

fn emit_reflog_line<F>(line: &[u8], suffix: &[u8], on_line: &mut F) -> Result<()>
where
    F: FnMut(&str) -> Result<()>,
{
    if suffix.is_empty() {
        return on_line(reflog_line_utf8(line)?);
    }
    let mut joined = Vec::with_capacity(line.len() + suffix.len());
    joined.extend_from_slice(line);
    joined.extend_from_slice(suffix);
    on_line(reflog_line_utf8(&joined)?)
}

fn prepend_reflog_line_prefix(suffix: &mut Vec<u8>, prefix: &[u8]) {
    if suffix.is_empty() {
        suffix.extend_from_slice(prefix);
        return;
    }
    let mut joined = Vec::with_capacity(prefix.len() + suffix.len());
    joined.extend_from_slice(prefix);
    joined.extend_from_slice(suffix);
    *suffix = joined;
}

fn reflog_line_utf8(line: &[u8]) -> Result<&str> {
    std::str::from_utf8(line).map_err(|error| {
        CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("reflog contains invalid UTF-8: {error}"),
        ))
    })
}

fn reflog_selector(index: usize, entry: &ReflogEntry, mode: ReflogDateMode) -> Result<String> {
    match mode {
        ReflogDateMode::Index => Ok(index.to_string()),
        ReflogDateMode::Iso => reflog_iso_selector(entry.timestamp, &entry.timezone),
        ReflogDateMode::Unix => Ok(entry.timestamp.to_string()),
        ReflogDateMode::Raw => Ok(format!("{} {}", entry.timestamp, entry.timezone)),
    }
}

fn reflog_list(repo: &GitRepo) -> Result<()> {
    let mut names = BTreeSet::new();
    let logs_dir = repo.git_dir.join("logs");
    let mut local_names = Vec::new();
    collect_reflog_names(&logs_dir, &logs_dir, &mut local_names)?;
    names.extend(local_names);
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    if common_git_dir != repo.git_dir {
        let common_logs_dir = common_git_dir.join("logs");
        let mut common_names = Vec::new();
        collect_reflog_names(&common_logs_dir, &common_logs_dir, &mut common_names)?;
        names.extend(
            common_names
                .into_iter()
                .filter(|name| name != "HEAD" && !name.starts_with("refs/worktree/")),
        );
    }
    for name in names {
        println!("{name}");
    }
    Ok(())
}

fn collect_reflog_names(
    root: &std::path::Path,
    path: &std::path::Path,
    names: &mut Vec<String>,
) -> Result<()> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_reflog_names(root, &path, names)?;
        } else if path.is_file() {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            names.push(name);
        }
    }
    Ok(())
}

pub(crate) struct ReflogEntry {
    pub(crate) old_id: ObjectId,
    pub(crate) new_id: ObjectId,
    pub(crate) timestamp: i64,
    pub(crate) timezone: String,
    pub(crate) message: String,
}

pub(crate) fn parse_reflog_entry(line: &str) -> Option<ReflogEntry> {
    let (header, message) = line.split_once('\t').unwrap_or((line, ""));
    let mut fields = header.split_whitespace();
    let old_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, fields.next()?).ok()?;
    let new_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, fields.next()?).ok()?;
    let timezone = fields.next_back()?.to_owned();
    let timestamp = fields.next_back()?.parse().ok()?;
    fields.next()?;
    Some(ReflogEntry {
        old_id,
        new_id,
        timestamp,
        timezone,
        message: message.to_owned(),
    })
}

fn reflog_iso_selector(timestamp: i64, timezone: &str) -> Result<String> {
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "reflog entry has invalid timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "reflog entry timestamp is out of range".into(),
    })?;
    Ok(utc
        .with_timezone(&offset)
        .format("%Y-%m-%d %H:%M:%S %z")
        .to_string())
}

fn reflog_path(repo: &GitRepo, ref_name: &str) -> Result<PathBuf> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let normalized = if ref_name == "HEAD" || ref_name.starts_with("refs/") {
        ref_name.to_owned()
    } else if ref_name == "stash" {
        "refs/stash".to_owned()
    } else if let Some(ref_name) = branch_checkout_ref(&refs, ref_name)? {
        ref_name
    } else {
        ref_name.to_owned()
    };
    Ok(repo.git_dir.join("logs").join(normalized))
}

fn reflog_display_name(ref_name: &str, full_ref_name: bool) -> String {
    if full_ref_name {
        ref_name.to_owned()
    } else {
        ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned()
    }
}

pub(crate) fn shortlog(
    committer: bool,
    numbered: bool,
    summary: bool,
    email: bool,
    no_merges: bool,
    revs: Vec<String>,
) -> Result<()> {
    if revs.is_empty() {
        return Ok(());
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let revs = collect_rev_list_revs(&repo, &store, false, revs)?;
    let commit_cache = CommitObjectCache::new(&store);
    let commits =
        collect_commit_objects_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for entry in commits.iter().rev() {
        let commit = entry.commit.as_ref();
        if no_merges && commit.parents.len() > 1 {
            continue;
        }
        let signature = if committer {
            &commit.committer
        } else {
            &commit.author
        };
        let mut key = signature_name(signature);
        if email {
            key.push_str(" <");
            key.push_str(&signature_email(signature));
            key.push('>');
        }
        groups
            .entry(key)
            .or_default()
            .push(commit_subject(&commit.message));
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    if numbered {
        groups.sort_by(|left, right| {
            right
                .1
                .len()
                .cmp(&left.1.len())
                .then_with(|| left.0.cmp(&right.0))
        });
    } else {
        groups.sort_by(|left, right| left.0.cmp(&right.0));
    }
    for (idx, (name, subjects)) in groups.iter().enumerate() {
        if summary {
            println!("{:6}\t{}", subjects.len(), name);
            continue;
        }
        println!("{} ({}):", name, subjects.len());
        for subject in subjects {
            println!("      {subject}");
        }
        if idx + 1 < groups.len() {
            println!();
        }
    }
    Ok(())
}

pub(crate) fn request_pull(start: &str, url: &str, end: Option<&str>) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let end = end.unwrap_or("HEAD");
    let start_id = resolve_objectish(&repo, start)?;
    let end_id = resolve_objectish(&repo, end)?;
    let start_commit = commit_cache.read_commit(&start_id)?;
    let end_commit = commit_cache.read_commit(&end_id)?;
    let revs = collect_rev_list_revs(&repo, &store, false, vec![format!("{start}..{end}")])?;
    let commits =
        collect_commits_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?;

    println!("The following changes since commit {}:", start_id.to_hex());
    println!();
    println!("  {}", request_pull_commit_line(&start_commit)?);
    println!();
    println!("are available in the Git repository at:");
    println!();
    println!("  {url} {end}");
    println!();
    println!("for you to fetch changes up to {}:", end_id.to_hex());
    println!();
    println!("  {}", request_pull_commit_line(&end_commit)?);
    println!();
    println!("----------------------------------------------------------------");
    print_request_pull_shortlog(&commit_cache, &commits)?;
    println!();
    let old_index = tree_cache.read_tree_to_index(&start_commit.tree)?;
    let new_index = tree_cache.read_tree_to_index(&end_commit.tree)?;
    let entries = diff_indexes(&old_index, &new_index)?;
    let context = DiffIndexContext {
        repo: &repo,
        store: &store,
        old_index: &old_index,
        new_index: &new_index,
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
        },
    )?;
    print_summary_entries(&old_index, &new_index, &entries, None)
}

fn request_pull_commit_line(commit: &zmin_git_core::CommitObject) -> Result<String> {
    Ok(format!(
        "{} ({})",
        commit_subject(&commit.message),
        signature_blame_date(&commit.author)?
    ))
}

fn print_request_pull_shortlog(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
) -> Result<()> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for id in commits.iter().rev() {
        let commit = commit_cache.read_commit(id)?;
        let name = signature_name(&commit.author);
        groups
            .entry(name)
            .or_default()
            .push(commit_subject(&commit.message));
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|left, right| left.0.cmp(&right.0));
    for (idx, (name, subjects)) in groups.iter().enumerate() {
        println!("{} ({}):", name, subjects.len());
        for subject in subjects {
            println!("      {subject}");
        }
        if idx + 1 < groups.len() {
            println!();
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct BlameLine {
    commit: ObjectId,
    line_no: usize,
    content: Vec<u8>,
    boundary: bool,
}

#[derive(Debug, Clone)]
struct BlameOptions {
    rev: Option<String>,
    path: String,
    contents_path: Option<String>,
    porcelain: bool,
    line_porcelain: bool,
    incremental: bool,
    show_filename: bool,
    show_number: bool,
    show_email: bool,
    show_stats: bool,
    root: bool,
    abbrev_width: Option<usize>,
    ignore_whitespace: bool,
    line_range: Option<(usize, usize)>,
}

pub(crate) fn blame(long: bool, root: bool, annotate: bool, args: Vec<String>) -> Result<()> {
    let options = parse_blame_args(args)?;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let rev = options.rev.as_deref().unwrap_or("HEAD");
    let head = resolve_commitish(&repo, &store, &rev)?;
    let path_bytes = normalize_git_path(&options.path)?.into_bytes();
    let final_lines = if let Some(contents_path) = options.contents_path.as_deref() {
        Some(split_blame_contents(fs::read(contents_path)?))
    } else {
        None
    };
    let mut lines = blame_lines(
        &store,
        &commit_cache,
        &head,
        &path_bytes,
        final_lines,
        options.ignore_whitespace,
    )?;
    if let Some((start, end)) = options.line_range {
        lines.retain(|line| (start..=end).contains(&line.line_no));
    }
    let effective_root = root || options.root;
    if options.incremental {
        print_incremental_blame_lines(&commit_cache, &lines, &path_bytes, effective_root)
    } else if options.porcelain || options.line_porcelain {
        print_porcelain_blame_lines(
            &commit_cache,
            &lines,
            &path_bytes,
            effective_root,
            options.line_porcelain,
        )
    } else if annotate {
        print_annotate_lines(&commit_cache, &lines)
    } else {
        print_blame_lines(
            &commit_cache,
            &lines,
            &path_bytes,
            long,
            effective_root,
            &options,
        )?;
        if options.show_stats {
            print_blame_stats(&lines);
        }
        Ok(())
    }
}

fn parse_blame_args(args: Vec<String>) -> Result<BlameOptions> {
    let mut rev = None;
    let mut porcelain = false;
    let mut line_porcelain = false;
    let mut incremental = false;
    let mut show_filename = false;
    let mut show_number = false;
    let mut show_email = false;
    let mut show_stats = false;
    let mut root = false;
    let mut contents_path = None;
    let mut abbrev_width = None;
    let mut ignore_whitespace = false;
    let mut line_range = None;
    let mut positionals = Vec::new();
    let mut after_separator = false;
    let mut cursor = 0;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !after_separator && arg == "--" {
            after_separator = true;
            cursor += 1;
            continue;
        }
        if !after_separator && arg.starts_with('-') {
            match arg.as_str() {
                "-p" | "--porcelain" => porcelain = true,
                "--incremental" => incremental = true,
                "--line-porcelain" => {
                    porcelain = true;
                    line_porcelain = true;
                }
                "-f" | "--show-name" => show_filename = true,
                "-n" | "--show-number" => show_number = true,
                "-e" | "--show-email" => show_email = true,
                "--root" => root = true,
                "--show-stats" => show_stats = true,
                "-w" => ignore_whitespace = true,
                "-M" | "-C" | "--find-renames" | "--find-copies" => {}
                "--contents" => {
                    cursor += 1;
                    let Some(path) = args.get(cursor) else {
                        return Err(CliError::Fatal {
                            code: 129,
                            message: "blame --contents requires a file".into(),
                        });
                    };
                    contents_path = Some(path.clone());
                }
                "-L" => {
                    cursor += 1;
                    let Some(value) = args.get(cursor) else {
                        return Err(CliError::Fatal {
                            code: 129,
                            message: "blame -L requires a range".into(),
                        });
                    };
                    line_range = Some(parse_blame_line_range(value)?);
                }
                _ => {
                    if let Some(value) = arg.strip_prefix("-L") {
                        line_range = Some(parse_blame_line_range(value)?);
                        cursor += 1;
                        continue;
                    }
                    if let Some(value) = arg.strip_prefix("-M")
                        && value.chars().all(|ch| ch.is_ascii_digit())
                    {
                        cursor += 1;
                        continue;
                    }
                    if let Some(value) = arg.strip_prefix("-C")
                        && value.chars().all(|ch| ch.is_ascii_digit())
                    {
                        cursor += 1;
                        continue;
                    }
                    if let Some(path) = arg.strip_prefix("--contents=") {
                        contents_path = Some(path.to_owned());
                        cursor += 1;
                        continue;
                    }
                    if let Some(value) = arg.strip_prefix("--abbrev=") {
                        abbrev_width = Some(parse_blame_abbrev(value)?);
                        cursor += 1;
                        continue;
                    }
                    if arg == "--abbrev" {
                        cursor += 1;
                        let Some(value) = args.get(cursor) else {
                            return Err(CliError::Fatal {
                                code: 129,
                                message: "blame --abbrev requires a value".into(),
                            });
                        };
                        abbrev_width = Some(parse_blame_abbrev(value)?);
                        cursor += 1;
                        continue;
                    }
                    if arg == "--date=iso" {
                        cursor += 1;
                        continue;
                    }
                    if arg == "--date" {
                        cursor += 1;
                        let Some(value) = args.get(cursor) else {
                            return Err(CliError::Fatal {
                                code: 129,
                                message: "blame --date requires a value".into(),
                            });
                        };
                        if value == "iso" {
                            cursor += 1;
                            continue;
                        }
                    }
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("unsupported blame option '{arg}'"),
                    });
                }
            }
            cursor += 1;
            continue;
        }
        positionals.push(arg.clone());
        cursor += 1;
    }
    let path = match positionals.as_slice() {
        [path] => path.clone(),
        [rev_arg, path] => {
            rev = Some(rev_arg.clone());
            path.clone()
        }
        _ => {
            return Err(CliError::Fatal {
                code: 129,
                message: "blame requires a file path".into(),
            });
        }
    };
    Ok(BlameOptions {
        rev,
        path,
        contents_path,
        porcelain,
        line_porcelain,
        incremental,
        show_filename,
        show_number,
        show_email,
        show_stats,
        root,
        abbrev_width,
        ignore_whitespace,
        line_range,
    })
}

fn parse_blame_abbrev(value: &str) -> Result<usize> {
    let abbrev = value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 129,
        message: format!("invalid blame abbrev '{value}'"),
    })?;
    Ok(abbrev.saturating_add(1).clamp(5, 40))
}

fn parse_blame_line_range(value: &str) -> Result<(usize, usize)> {
    let (start, end) = value.split_once(',').unwrap_or((value, ""));
    let start = start.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 129,
        message: format!("unsupported blame line range '{value}'"),
    })?;
    let end = if end.is_empty() {
        usize::MAX
    } else if let Some(count) = end.strip_prefix('+') {
        let count = count.parse::<usize>().map_err(|_| CliError::Fatal {
            code: 129,
            message: format!("unsupported blame line range '{value}'"),
        })?;
        start.saturating_add(count.saturating_sub(1))
    } else if let Some(count) = end.strip_prefix('-') {
        let count = count.parse::<usize>().map_err(|_| CliError::Fatal {
            code: 129,
            message: format!("unsupported blame line range '{value}'"),
        })?;
        start.saturating_sub(count.saturating_sub(1))
    } else {
        end.parse::<usize>().map_err(|_| CliError::Fatal {
            code: 129,
            message: format!("unsupported blame line range '{value}'"),
        })?
    };
    if start == 0 || end < start {
        return Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported blame line range '{value}'"),
        });
    }
    Ok((start, end))
}

fn blame_lines(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    head: &ObjectId,
    path: &[u8],
    final_lines_override: Option<Vec<Vec<u8>>>,
    ignore_whitespace: bool,
) -> Result<Vec<BlameLine>> {
    let final_lines = match final_lines_override {
        Some(lines) => lines,
        None => commit_file_lines_cached(store, commit_cache, head, path)?,
    };
    let mut out = Vec::with_capacity(final_lines.len());
    for (idx, content) in final_lines.into_iter().enumerate() {
        let mut owner = head.clone();
        loop {
            let commit = commit_cache.read_commit(&owner)?;
            let Some(parent) = commit.parents.first() else {
                break;
            };
            let parent_lines = commit_file_lines_cached(store, commit_cache, parent, path)?;
            if parent_lines
                .get(idx)
                .is_some_and(|parent| blame_line_matches(parent, &content, ignore_whitespace))
            {
                owner = parent.clone();
            } else {
                break;
            }
        }
        let boundary = commit_cache.read_commit(&owner)?.parents.is_empty();
        out.push(BlameLine {
            commit: owner,
            line_no: idx + 1,
            content,
            boundary,
        });
    }
    Ok(out)
}

fn split_blame_contents(contents: Vec<u8>) -> Vec<Vec<u8>> {
    contents
        .split_inclusive(|byte| *byte == b'\n')
        .map(|line| line.to_vec())
        .collect()
}

fn print_blame_stats(lines: &[BlameLine]) {
    let unique_commits = lines
        .iter()
        .map(|line| line.commit.clone())
        .collect::<HashSet<_>>();
    let boundary_commits = lines
        .iter()
        .filter(|line| line.boundary)
        .map(|line| line.commit.clone())
        .collect::<HashSet<_>>();
    let commit_count = unique_commits.len().saturating_sub(boundary_commits.len());
    println!("num read blob: {}", unique_commits.len());
    println!("num get patch: {commit_count}");
    println!("num commits: {commit_count}");
}

fn blame_line_matches(parent: &[u8], current: &[u8], ignore_whitespace: bool) -> bool {
    if !ignore_whitespace {
        return parent == current;
    }
    parent
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .eq(current
            .iter()
            .copied()
            .filter(|byte| !byte.is_ascii_whitespace()))
}

fn commit_file_lines_cached(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commit_id: &ObjectId,
    path: &[u8],
) -> Result<Vec<Vec<u8>>> {
    let commit = commit_cache.read_commit(commit_id)?;
    let Some(entry) = find_tree_entry(store, &commit.tree, path)? else {
        return Ok(Vec::new());
    };
    let object = store.read_object(&entry.id)?;
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "{} is not a file in commit {}",
                String::from_utf8_lossy(path),
                commit_id
            ),
        });
    }
    Ok(split_diff_lines(&object.content)
        .into_iter()
        .map(|line| line.to_vec())
        .collect())
}

fn print_blame_lines(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    lines: &[BlameLine],
    path: &[u8],
    long: bool,
    root: bool,
    options: &BlameOptions,
) -> Result<()> {
    for line in lines {
        let commit = commit_cache.read_commit(&line.commit)?;
        let display_id = blame_display_id(
            &line.commit,
            line.boundary && !root,
            long,
            options.abbrev_width,
        );
        let author = if options.show_email {
            format!("<{}>", signature_email(&commit.author))
        } else {
            signature_name(&commit.author)
        };
        let date = signature_blame_date(&commit.author)?;
        print!("{display_id}");
        if options.show_filename {
            print!(" {}", String::from_utf8_lossy(path));
        }
        if options.show_number {
            print!(" {}", line.line_no);
        }
        print!(" ({author} {date} {}) ", line.line_no);
        io::stdout().write_all(&line.content)?;
        if !line.content.ends_with(b"\n") {
            println!();
        }
    }
    Ok(())
}

fn print_annotate_lines(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    lines: &[BlameLine],
) -> Result<()> {
    for line in lines {
        let commit = commit_cache.read_commit(&line.commit)?;
        let author = signature_name(&commit.author);
        let date = signature_blame_date(&commit.author)?;
        print!(
            "{}\t({author:>10}\t{date}\t{})",
            short_object_id_len(&line.commit, 8),
            line.line_no
        );
        io::stdout().write_all(&line.content)?;
        if !line.content.ends_with(b"\n") {
            println!();
        }
    }
    Ok(())
}

fn print_porcelain_blame_lines(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    lines: &[BlameLine],
    path: &[u8],
    root: bool,
    repeat_metadata: bool,
) -> Result<()> {
    let mut described = HashSet::new();
    for (index, line) in lines.iter().enumerate() {
        let group_len = blame_group_len(lines, index);
        if blame_starts_group(lines, index) {
            println!(
                "{} {} {} {group_len}",
                line.commit.to_hex(),
                line.line_no,
                line.line_no
            );
        } else {
            println!("{} {} {}", line.commit.to_hex(), line.line_no, line.line_no);
        }
        let describe_commit = repeat_metadata || described.insert(line.commit.clone());
        if describe_commit {
            let commit = commit_cache.read_commit(&line.commit)?;
            print_blame_porcelain_commit(&commit, line.boundary && !root, path)?;
        }
        print!("\t");
        io::stdout().write_all(&line.content)?;
        if !line.content.ends_with(b"\n") {
            println!();
        }
    }
    Ok(())
}

fn print_incremental_blame_lines(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    lines: &[BlameLine],
    path: &[u8],
    root: bool,
) -> Result<()> {
    let mut groups = Vec::new();
    for (index, _) in lines.iter().enumerate() {
        if blame_starts_group(lines, index) {
            let commit = commit_cache.read_commit(&lines[index].commit)?;
            let commit_time = signature_timestamp_timezone(&commit.committer)
                .map(|(time, _)| time)
                .unwrap_or(0);
            groups.push((index, blame_group_len(lines, index), commit_time));
        }
    }
    groups.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(&right.0)));
    for (index, group_len, _) in groups {
        let line = &lines[index];
        println!(
            "{} {} {} {group_len}",
            line.commit.to_hex(),
            line.line_no,
            line.line_no
        );
        let commit = commit_cache.read_commit(&line.commit)?;
        print_blame_porcelain_commit(&commit, line.boundary && !root, path)?;
    }
    Ok(())
}

fn blame_starts_group(lines: &[BlameLine], index: usize) -> bool {
    index == 0 || lines[index - 1].commit != lines[index].commit
}

fn blame_group_len(lines: &[BlameLine], index: usize) -> usize {
    if !blame_starts_group(lines, index) {
        return 1;
    }
    let mut len = 1;
    while lines
        .get(index + len)
        .is_some_and(|line| line.commit == lines[index].commit)
    {
        len += 1;
    }
    len
}

fn print_blame_porcelain_commit(
    commit: &zmin_git_core::CommitObject,
    boundary: bool,
    path: &[u8],
) -> Result<()> {
    let (author_time, author_tz) =
        signature_timestamp_timezone(&commit.author).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    let (committer_time, committer_tz) = signature_timestamp_timezone(&commit.committer)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid committer date".into(),
        })?;
    println!("author {}", signature_name(&commit.author));
    println!("author-mail <{}>", signature_email(&commit.author));
    println!("author-time {author_time}");
    println!("author-tz {author_tz}");
    println!("committer {}", signature_name(&commit.committer));
    println!("committer-mail <{}>", signature_email(&commit.committer));
    println!("committer-time {committer_time}");
    println!("committer-tz {committer_tz}");
    println!("summary {}", commit_subject(&commit.message));
    if boundary {
        println!("boundary");
    } else if let Some(parent) = commit.parents.first() {
        println!(
            "previous {} {}",
            parent.to_hex(),
            String::from_utf8_lossy(path)
        );
    }
    println!("filename {}", String::from_utf8_lossy(path));
    Ok(())
}

fn blame_display_id(
    id: &ObjectId,
    boundary: bool,
    long: bool,
    abbrev_width: Option<usize>,
) -> String {
    if long {
        let hex = id.to_hex();
        if boundary {
            format!("^{}", &hex[..hex.len().saturating_sub(1)])
        } else {
            hex
        }
    } else if boundary {
        let width = abbrev_width.unwrap_or(8);
        format!("^{}", short_object_id_len(id, width.saturating_sub(1)))
    } else {
        short_object_id_len(id, abbrev_width.unwrap_or(8))
    }
}

#[derive(Debug, Clone)]
struct ShowBranchHead {
    id: ObjectId,
    display: String,
    current: bool,
    remote: bool,
}

pub(crate) fn show_branch(
    all: bool,
    remotes: bool,
    current: bool,
    sha1_name: bool,
    no_name: bool,
    revs: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let heads = show_branch_heads(&repo, &store, &refs, all, remotes, current, revs)?;
    if heads.is_empty() {
        return Ok(());
    }
    if heads.len() == 1 {
        println!(
            "[{}] {}",
            heads[0].display,
            commit_subject(&commit_cache.read_commit(&heads[0].id)?.message)
        );
        return Ok(());
    }
    for (idx, head) in heads.iter().enumerate() {
        println!(
            "{} [{}] {}",
            show_branch_header_prefix(&heads, idx),
            head.display,
            commit_subject(&commit_cache.read_commit(&head.id)?.message)
        );
    }
    println!("{}", "-".repeat(heads.len()));
    let commits = show_branch_commits(&commit_cache, &heads)?;
    for id in commits {
        let mut prefix = String::new();
        for head in &heads {
            if show_branch_reaches(&commit_cache, &head.id, &id)? {
                prefix.push(if head.current { '*' } else { '+' });
            } else {
                prefix.push(' ');
            }
        }
        let commit = commit_cache.read_commit(&id)?;
        let name = if no_name {
            String::new()
        } else if sha1_name {
            short_object_id(&id)
        } else {
            show_branch_name_for_commit(&commit_cache, &heads, &id)?
        };
        if name.is_empty() {
            println!("{prefix} {}", commit_subject(&commit.message));
        } else {
            println!("{prefix} [{name}] {}", commit_subject(&commit.message));
        }
    }
    Ok(())
}

fn show_branch_heads(
    repo: &GitRepo,
    store: &LooseObjectStore,
    refs: &RefStore,
    all: bool,
    remotes: bool,
    include_current: bool,
    revs: Vec<String>,
) -> Result<Vec<ShowBranchHead>> {
    let current = current_branch_ref(refs)?;
    let mut heads = Vec::new();
    if revs.is_empty() || all || remotes {
        if !remotes {
            refs.for_each_resolved_ref("refs/heads/", |ref_name, id| {
                show_branch_push_ref_head_id(store, current.as_deref(), &mut heads, ref_name, id)
            })?;
        }
        if all || remotes {
            refs.for_each_resolved_ref("refs/remotes/", |ref_name, id| {
                if ref_name.ends_with("/HEAD") {
                    return Ok(());
                }
                show_branch_push_ref_head_id(store, current.as_deref(), &mut heads, ref_name, id)
            })?;
        }
    }
    for rev in revs {
        let id = resolve_commitish(repo, store, &rev)?;
        let ref_name = if rev.starts_with("refs/heads/") {
            rev.clone()
        } else {
            branch_ref_name(&rev).unwrap_or_else(|_| rev.clone())
        };
        heads.push(ShowBranchHead {
            current: current.as_deref() == Some(ref_name.as_str()),
            display: abbrev_ref_name(repo, &rev)?,
            remote: ref_name.starts_with("refs/remotes/"),
            id,
        });
    }
    if include_current
        && let Some(current_ref) = current.as_deref()
        && !heads
            .iter()
            .any(|head| head.current || head.display == show_branch_ref_display(current_ref))
    {
        let id = refs.resolve(current_ref)?;
        show_branch_push_ref_head_id(store, Some(current_ref), &mut heads, current_ref, &id)?;
    }
    Ok(heads)
}

fn show_branch_push_ref_head_id(
    store: &LooseObjectStore,
    current: Option<&str>,
    heads: &mut Vec<ShowBranchHead>,
    ref_name: &str,
    id: &ObjectId,
) -> Result<()> {
    if store.read_object(id)?.kind == GitObjectKind::Commit {
        heads.push(ShowBranchHead {
            current: current == Some(ref_name),
            display: show_branch_ref_display(ref_name),
            remote: ref_name.starts_with("refs/remotes/"),
            id: id.clone(),
        });
    }
    Ok(())
}

fn show_branch_ref_display(ref_name: &str) -> String {
    ref_name
        .strip_prefix("refs/heads/")
        .or_else(|| ref_name.strip_prefix("refs/remotes/"))
        .unwrap_or(ref_name)
        .to_owned()
}

fn show_branch_header_prefix(heads: &[ShowBranchHead], idx: usize) -> String {
    let mut prefix = String::new();
    for _ in 0..idx {
        prefix.push(' ');
    }
    prefix.push(if heads[idx].current { '*' } else { '!' });
    prefix
}

fn show_branch_commits(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    heads: &[ShowBranchHead],
) -> Result<Vec<ObjectId>> {
    let mut pending = heads
        .iter()
        .rev()
        .map(|head| head.id.clone())
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    let mut commits = Vec::new();
    while !pending.is_empty() {
        let id = pending.remove(0);
        if !seen.insert(id.to_hex()) {
            continue;
        }
        let commit = commit_cache.read_commit(&id)?;
        for parent in &commit.parents {
            if !seen.contains(&parent.to_hex())
                && !pending.iter().any(|pending_id| pending_id == parent)
            {
                pending.push(parent.clone());
            }
        }
        commits.push(id);
    }
    Ok(commits)
}

fn show_branch_reaches(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    head: &ObjectId,
    target: &ObjectId,
) -> Result<bool> {
    show_branch_distance(commit_cache, head, target).map(|distance| distance.is_some())
}

fn show_branch_name_for_commit(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    heads: &[ShowBranchHead],
    id: &ObjectId,
) -> Result<String> {
    let mut best = None::<(usize, &ShowBranchHead)>;
    for head in heads {
        if let Some(distance) = show_branch_distance(commit_cache, &head.id, id)? {
            let should_replace = match best.as_ref() {
                None => true,
                Some((best_distance, best_head)) if distance < *best_distance => true,
                Some((best_distance, best_head)) if distance == *best_distance => {
                    (!head.remote && best_head.remote) || (head.remote == best_head.remote)
                }
                Some(_) => false,
            };
            if should_replace {
                best = Some((distance, head));
            }
        }
    }
    Ok(match best {
        Some((0, head)) => head.display.clone(),
        Some((1, head)) => format!("{}^", head.display),
        Some((distance, head)) => format!("{}~{distance}", head.display),
        None => short_object_id(id),
    })
}

fn show_branch_distance(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    head: &ObjectId,
    target: &ObjectId,
) -> Result<Option<usize>> {
    let mut current = head.clone();
    for distance in 0..1024 {
        if &current == target {
            return Ok(Some(distance));
        }
        let commit = commit_cache.read_commit(&current)?;
        let Some(parent) = commit.parents.first() else {
            return Ok(None);
        };
        current = parent.clone();
    }
    Err(CliError::Fatal {
        code: 128,
        message: "show-branch history traversal exceeded 1024 commits".into(),
    })
}

pub(crate) fn cherry(
    verbose: bool,
    abbrev: Option<usize>,
    upstream: Option<&str>,
    head: Option<&str>,
    limit: Option<&str>,
) -> Result<()> {
    let repo = find_repo()?;
    let upstream = match upstream {
        Some(upstream) => upstream.to_owned(),
        None => cherry_default_upstream(&repo)?,
    };
    let head = head.unwrap_or("HEAD");
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let abbrev_len = abbrev.unwrap_or(GitHashAlgorithm::Sha1.digest_len() * 2);
    let upstream_id = resolve_commitish(&repo, &store, &upstream)?;
    let head_id = resolve_commitish(&repo, &store, head)?;
    if upstream_id == head_id {
        return Ok(());
    }

    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let upstream_commits = collect_commits_cached(
        &repo,
        &store,
        &commit_cache,
        std::slice::from_ref(&upstream),
        None,
    )?;
    let mut upstream_patch_ids = HashSet::new();
    for id in upstream_commits {
        if let Some(patch_id) = reference_commands::commit_patch_id_for_cherry_cached(
            &store,
            &commit_cache,
            &tree_cache,
            &id,
        )? {
            upstream_patch_ids.insert(patch_id);
        }
    }

    let mut exclude = vec![upstream];
    if let Some(limit) = limit {
        exclude.push(limit.to_owned());
    }
    let revs = RevListRevs {
        include: vec![head.to_owned()],
        exclude,
        extra_objects: Vec::new(),
    };
    let mut commits =
        collect_commits_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?;
    commits.reverse();
    for id in commits {
        let patch_id = reference_commands::commit_patch_id_for_cherry_cached(
            &store,
            &commit_cache,
            &tree_cache,
            &id,
        )?;
        let sign = if patch_id
            .as_ref()
            .is_some_and(|patch_id| upstream_patch_ids.contains(patch_id))
        {
            '-'
        } else {
            '+'
        };
        let mut line = format!("{sign} {}", short_object_id_len(&id, abbrev_len));
        if verbose {
            let commit = commit_cache.read_commit(&id)?;
            line.push(' ');
            line.push_str(&commit_subject(&commit.message));
        }
        println!("{line}");
    }
    Ok(())
}

fn cherry_default_upstream(repo: &GitRepo) -> Result<String> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let branch = current_branch_ref(&refs)?.ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "HEAD does not point to a branch".into(),
    })?;
    let branch = branch_display_name(&branch);
    let upstream = read_branch_upstream(repo, &branch)?.ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("no upstream configured for branch '{branch}'"),
    })?;
    Ok(upstream.ref_name)
}

pub(crate) struct DescribeOptions {
    pub(crate) all: bool,
    pub(crate) tags: bool,
    pub(crate) long: bool,
    pub(crate) abbrev: Option<usize>,
    pub(crate) exact_match: bool,
    pub(crate) always: bool,
    pub(crate) dirty: Option<String>,
    pub(crate) matches: Vec<String>,
    pub(crate) excludes: Vec<String>,
    pub(crate) commits: Vec<String>,
}

#[derive(Debug, Clone)]
struct DescribeCandidate {
    name: String,
    target: ObjectId,
    annotated: bool,
    ref_priority: u8,
    tagger_timestamp: i64,
}

pub(crate) fn describe(options: DescribeOptions) -> Result<()> {
    if options.long && options.abbrev == Some(0) {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--long' and '--abbrev=0' cannot be used together".into(),
        });
    }
    if options.dirty.is_some() && !options.commits.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "option '--dirty' and commit-ishes cannot be used together".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commits = if options.commits.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        options.commits.clone()
    };
    let abbrev_len = options.abbrev.unwrap_or(default_abbrev_len(&store)?);
    let candidates = describe_candidates(&repo, &store, &options)?;
    let commit_cache = CommitObjectCache::new(&store);
    let dirty_suffix = if let Some(mark) = options.dirty.as_deref() {
        if worktree_clean(&repo, &store)? {
            ""
        } else {
            mark
        }
    } else {
        ""
    };

    for commitish in commits {
        let id = resolve_describe_commitish(&repo, &store, &commitish)?;
        match describe_commit(&commit_cache, &id, &candidates, &options, abbrev_len)? {
            Some(mut description) => {
                description.push_str(dirty_suffix);
                println!("{description}");
            }
            None if options.always => {
                println!(
                    "{}{}",
                    short_object_id_len(&id, abbrev_len.max(1)),
                    dirty_suffix
                );
            }
            None if candidates.is_empty() => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "No names found, cannot describe anything.".into(),
                });
            }
            None => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "No annotated tags can describe '{}'.",
                        short_object_id_len(&id, abbrev_len.max(1))
                    ),
                });
            }
        }
    }
    Ok(())
}

fn describe_candidates(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &DescribeOptions,
) -> Result<Vec<DescribeCandidate>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let prefix = if options.all { "refs/" } else { "refs/tags/" };
    let mut candidates = Vec::new();
    refs.for_each_resolved_ref(prefix, |ref_name, id| {
        if !options.all && !ref_name.starts_with("refs/tags/") {
            return Ok(());
        }
        let display = describe_ref_display_name(ref_name, options.all);
        if !describe_name_matches(&display, &options.matches, &options.excludes) {
            return Ok(());
        }
        let Some((target, annotated, tagger_timestamp)) =
            describe_candidate_target(store, id, options.all || options.tags)?
        else {
            return Ok(());
        };
        if !options.all && !options.tags && !annotated {
            return Ok(());
        }
        candidates.push(DescribeCandidate {
            name: display,
            target,
            annotated,
            ref_priority: describe_ref_priority(ref_name),
            tagger_timestamp,
        });
        Ok::<(), CliError>(())
    })?;
    Ok(candidates)
}

fn describe_ref_priority(ref_name: &str) -> u8 {
    if ref_name.starts_with("refs/tags/") {
        2
    } else if ref_name.starts_with("refs/heads/") {
        1
    } else {
        0
    }
}

fn describe_ref_display_name(ref_name: &str, all: bool) -> String {
    if all {
        ref_name
            .strip_prefix("refs/")
            .unwrap_or(ref_name)
            .to_owned()
    } else {
        tag_display_name(ref_name)
    }
}

fn describe_name_matches(name: &str, matches: &[String], excludes: &[String]) -> bool {
    (matches.is_empty() || matches.iter().any(|pattern| wildcard_match(pattern, name)))
        && !excludes.iter().any(|pattern| wildcard_match(pattern, name))
}

fn describe_candidate_target(
    store: &LooseObjectStore,
    id: &ObjectId,
    allow_commit_ref: bool,
) -> Result<Option<(ObjectId, bool, i64)>> {
    let object = store.read_object(id)?;
    match object.kind {
        GitObjectKind::Tag => {
            let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
            let Some(target) = peel_to_commit(store, tag.target)? else {
                return Ok(None);
            };
            let tagger_timestamp = signature_timestamp(&tag.tagger).unwrap_or(0);
            Ok(Some((target, true, tagger_timestamp)))
        }
        GitObjectKind::Commit if allow_commit_ref => Ok(Some((id.clone(), false, 0))),
        _ => Ok(None),
    }
}

pub(crate) fn peel_to_commit(
    store: &LooseObjectStore,
    mut id: ObjectId,
) -> Result<Option<ObjectId>> {
    for _ in 0..8 {
        let object = store.read_object(&id)?;
        match object.kind {
            GitObjectKind::Commit => return Ok(Some(id)),
            GitObjectKind::Tag => {
                id = decode_tag(GitHashAlgorithm::Sha1, &object.content)?.target;
            }
            _ => return Ok(None),
        }
    }
    Err(CliError::Fatal {
        code: 128,
        message: "tag nesting is too deep".into(),
    })
}

fn resolve_describe_commitish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commitish: &str,
) -> Result<ObjectId> {
    let id = resolve_objectish(repo, commitish)?;
    peel_to_commit(store, id)?.ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("'{commitish}' is not a commit-ish"),
    })
}

fn describe_commit(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
    candidates: &[DescribeCandidate],
    options: &DescribeOptions,
    abbrev_len: usize,
) -> Result<Option<String>> {
    let depths = commit_depths_cached(commit_cache, id)?;
    let mut best = None::<(&DescribeCandidate, usize)>;
    for candidate in candidates {
        let Some(depth) = depths.get(&candidate.target).copied() else {
            continue;
        };
        if options.exact_match && depth != 0 {
            continue;
        }
        let replace = match best {
            None => true,
            Some((best_candidate, best_depth)) => {
                depth < best_depth
                    || (depth == best_depth && describe_candidate_cmp(candidate, best_candidate))
            }
        };
        if replace {
            best = Some((candidate, depth));
        }
    }
    let Some((candidate, depth)) = best else {
        return Ok(None);
    };
    if options.abbrev == Some(0) {
        return Ok(Some(candidate.name.clone()));
    }
    if depth == 0 && !options.long {
        return Ok(Some(candidate.name.clone()));
    }
    Ok(Some(format!(
        "{}-{}-g{}",
        candidate.name,
        depth,
        short_object_id_len(id, abbrev_len)
    )))
}

fn describe_candidate_cmp(candidate: &DescribeCandidate, best: &DescribeCandidate) -> bool {
    candidate.ref_priority > best.ref_priority
        || (candidate.ref_priority == best.ref_priority && candidate.annotated && !best.annotated)
        || (candidate.ref_priority == best.ref_priority
            && candidate.annotated == best.annotated
            && (candidate.tagger_timestamp > best.tagger_timestamp
                || (candidate.tagger_timestamp == best.tagger_timestamp
                    && candidate.name < best.name)))
}

pub(crate) struct NameRevOptions {
    pub(crate) name_only: bool,
    pub(crate) tags: bool,
    pub(crate) refs: Vec<String>,
    pub(crate) excludes: Vec<String>,
    pub(crate) all: bool,
    pub(crate) annotate_stdin: bool,
    pub(crate) always: bool,
    pub(crate) commits: Vec<String>,
}

#[derive(Debug, Clone)]
struct NameRevCandidate {
    name: String,
    depths: HashMap<ObjectId, usize>,
    priority: u8,
}

pub(crate) fn name_rev(options: NameRevOptions) -> Result<()> {
    if options.all && (!options.commits.is_empty() || options.annotate_stdin) {
        return Err(CliError::Fatal {
            code: 129,
            message: "--all cannot be combined with commits or --annotate-stdin".into(),
        });
    }
    if options.annotate_stdin && !options.commits.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "--annotate-stdin cannot be combined with commits".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let candidates = name_rev_candidates(&repo, &store, &commit_cache, &options)?;
    if options.all {
        let mut commits = HashSet::<ObjectId>::new();
        for candidate in &candidates {
            for id in candidate.depths.keys() {
                commits.insert(id.clone());
            }
        }
        let mut ids = commits.into_iter().collect::<Vec<_>>();
        ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        for id in ids {
            print_name_rev(&commit_cache, &id, &candidates, &options)?;
        }
        return Ok(());
    }
    if options.annotate_stdin {
        return annotate_name_rev_stdin(&commit_cache, &candidates, &options);
    }
    if options.commits.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "name-rev requires commits, --all, or --annotate-stdin".into(),
        });
    }
    for commitish in &options.commits {
        let id = resolve_commitish(&repo, &store, commitish)?;
        print_name_rev(&commit_cache, &id, &candidates, &options)?;
    }
    Ok(())
}

fn name_rev_candidates(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    options: &NameRevOptions,
) -> Result<Vec<NameRevCandidate>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut candidates = Vec::new();
    refs.for_each_resolved_ref("refs/", |ref_name, id| {
        if options.tags && !ref_name.starts_with("refs/tags/") {
            return Ok(());
        }
        let display = name_rev_display_name(ref_name);
        if !name_rev_ref_matches(ref_name, &options.refs, &options.excludes) {
            return Ok(());
        }
        let Some(target) = peel_to_commit(store, id.clone())? else {
            return Ok(());
        };
        candidates.push(NameRevCandidate {
            name: display,
            depths: commit_depths_cached(commit_cache, &target)?,
            priority: describe_ref_priority(ref_name),
        });
        Ok::<(), CliError>(())
    })?;
    Ok(candidates)
}

fn name_rev_display_name(ref_name: &str) -> String {
    if let Some(branch) = ref_name.strip_prefix("refs/heads/") {
        branch.to_owned()
    } else {
        ref_name
            .strip_prefix("refs/")
            .unwrap_or(ref_name)
            .to_owned()
    }
}

fn name_rev_ref_matches(ref_name: &str, refs: &[String], excludes: &[String]) -> bool {
    (refs.is_empty() || refs.iter().any(|pattern| wildcard_match(pattern, ref_name)))
        && !excludes
            .iter()
            .any(|pattern| wildcard_match(pattern, ref_name))
}

fn print_name_rev(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
    candidates: &[NameRevCandidate],
    options: &NameRevOptions,
) -> Result<()> {
    let name = best_name_rev(id, candidates)
        .or_else(|| options.always.then(|| short_object_id(id)))
        .unwrap_or_else(|| "undefined".to_owned());
    if options.name_only {
        println!("{name}");
    } else {
        let commit = commit_cache.read_commit(id)?;
        let _ = commit;
        println!("{} {}", id.to_hex(), name);
    }
    Ok(())
}

fn best_name_rev(id: &ObjectId, candidates: &[NameRevCandidate]) -> Option<String> {
    let mut best = None::<(&NameRevCandidate, usize)>;
    for candidate in candidates {
        let Some(depth) = candidate.depths.get(id).copied() else {
            continue;
        };
        let replace = match best {
            None => true,
            Some((best_candidate, best_depth)) => {
                depth < best_depth
                    || (depth == best_depth
                        && (candidate.priority > best_candidate.priority
                            || (candidate.priority == best_candidate.priority
                                && candidate.name < best_candidate.name)))
            }
        };
        if replace {
            best = Some((candidate, depth));
        }
    }
    best.map(|(candidate, depth)| {
        if depth == 0 {
            candidate.name.clone()
        } else {
            format!("{}~{}", candidate.name, depth)
        }
    })
}

fn annotate_name_rev_stdin(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    candidates: &[NameRevCandidate],
    options: &NameRevOptions,
) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let re = regex::Regex::new(r"\b[0-9a-fA-F]{40}\b").map_err(|error| CliError::Fatal {
        code: 128,
        message: format!("failed to build object-id regex: {error}"),
    })?;
    let mut output = String::new();
    let mut last = 0usize;
    for found in re.find_iter(&input) {
        output.push_str(&input[last..found.end()]);
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, found.as_str())?;
        if let Ok(commit) = commit_cache.read_commit(&id) {
            let _ = commit;
            let name = best_name_rev(&id, candidates)
                .or_else(|| options.always.then(|| short_object_id(&id)))
                .unwrap_or_else(|| "undefined".to_owned());
            output.push_str(&format!(" ({name})"));
        }
        last = found.end();
    }
    output.push_str(&input[last..]);
    print!("{output}");
    Ok(())
}

pub(crate) fn range_diff(_no_dual_color: bool, ranges: Vec<String>) -> Result<()> {
    let ranges = match ranges.as_slice() {
        [old, new] => [old.clone(), new.clone()],
        [base, old, new] => [format!("{base}..{old}"), format!("{base}..{new}")],
        _ => {
            return Err(CliError::Fatal {
                code: 129,
                message: "`range-diff` requires two commit ranges or <base> <old> <new>".into(),
            });
        }
    };
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let old = range_diff_commits(&repo, &store, &ranges[0])?;
    let new = range_diff_commits(&repo, &store, &ranges[1])?;
    let abbrev_len = default_abbrev_len(&store)?;
    let mut new_by_patch = HashMap::<String, VecDeque<usize>>::new();
    for (idx, entry) in new.iter().enumerate() {
        if let Some(patch_id) = &entry.patch_id {
            new_by_patch
                .entry(patch_id.clone())
                .or_default()
                .push_back(idx);
        }
    }
    let mut matched_new = HashSet::new();
    for (old_idx, old_entry) in old.iter().enumerate() {
        let matched = old_entry
            .patch_id
            .as_ref()
            .and_then(|patch_id| new_by_patch.get_mut(patch_id))
            .and_then(VecDeque::pop_front);
        if let Some(new_idx) = matched {
            matched_new.insert(new_idx);
            println!(
                "{}:  {} = {}:  {} {}",
                old_idx + 1,
                short_object_id_len(&old_entry.id, abbrev_len),
                new_idx + 1,
                short_object_id_len(&new[new_idx].id, abbrev_len),
                old_entry.subject
            );
        } else {
            println!(
                "{}:  {} < -:  ------- {}",
                old_idx + 1,
                short_object_id_len(&old_entry.id, abbrev_len),
                old_entry.subject
            );
        }
    }
    for (new_idx, new_entry) in new.iter().enumerate() {
        if matched_new.contains(&new_idx) {
            continue;
        }
        println!(
            "-:  ------- > {}:  {} {}",
            new_idx + 1,
            short_object_id_len(&new_entry.id, abbrev_len),
            new_entry.subject
        );
    }
    Ok(())
}

fn range_diff_commits(
    repo: &GitRepo,
    store: &LooseObjectStore,
    range: &str,
) -> Result<Vec<RangeDiffCommit>> {
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let revs = collect_rev_list_revs(repo, store, false, vec![range.to_owned()])?;
    let mut commits =
        collect_commits_with_exclusions_cached(repo, store, &commit_cache, &revs, None)?;
    commits.reverse();
    commits
        .into_iter()
        .map(|id| {
            let commit = commit_cache.read_commit(&id)?;
            Ok(RangeDiffCommit {
                patch_id: reference_commands::commit_patch_id_for_cherry_cached(
                    store,
                    &commit_cache,
                    &tree_cache,
                    &id,
                )?,
                subject: commit_subject(&commit.message),
                id,
            })
        })
        .collect()
}

struct RangeDiffCommit {
    id: ObjectId,
    patch_id: Option<String>,
    subject: String,
}

pub(crate) struct LogOptions<'a> {
    pub(crate) oneline: bool,
    pub(crate) zero: bool,
    pub(crate) all: bool,
    pub(crate) parents: bool,
    pub(crate) first_parent: bool,
    pub(crate) no_diff_merges: bool,
    pub(crate) diff_merges: Option<&'a str>,
    pub(crate) separate_merges: bool,
    pub(crate) dd: bool,
    pub(crate) reverse: bool,
    pub(crate) root: bool,
    pub(crate) patch: bool,
    pub(crate) patch_with_stat: bool,
    pub(crate) combined: bool,
    pub(crate) dense_combined: bool,
    pub(crate) stat: bool,
    pub(crate) numstat: bool,
    pub(crate) shortstat: bool,
    pub(crate) raw: bool,
    pub(crate) summary: bool,
    pub(crate) name_only: bool,
    pub(crate) name_status: bool,
    pub(crate) diff_required: bool,
    pub(crate) decorate: Option<&'a str>,
    pub(crate) clear_decorations: bool,
    pub(crate) pickaxe_string: Option<&'a str>,
    pub(crate) pickaxe_regex: Option<&'a str>,
    pub(crate) pickaxe_regex_mode: bool,
    pub(crate) pickaxe_all: bool,
    pub(crate) ignore_matching_lines: Vec<String>,
    pub(crate) walk_reflogs: bool,
    pub(crate) no_walk: bool,
    pub(crate) format: Option<&'a str>,
    pub(crate) max_count: Option<&'a str>,
    pub(crate) since: Option<&'a str>,
    pub(crate) pretty: Option<&'a str>,
    pub(crate) revs: Vec<String>,
}

impl LogOptions<'_> {
    fn diff_format(&self, patch: bool) -> Option<ShowDiffFormat> {
        if self.patch_with_stat {
            if self.summary {
                Some(ShowDiffFormat::PatchWithStatSummary)
            } else {
                Some(ShowDiffFormat::PatchWithStat)
            }
        } else if patch || self.dd {
            Some(ShowDiffFormat::Patch)
        } else if self.stat {
            Some(ShowDiffFormat::Stat)
        } else if self.numstat {
            Some(ShowDiffFormat::Numstat)
        } else if self.shortstat {
            Some(ShowDiffFormat::Shortstat)
        } else if self.raw {
            Some(ShowDiffFormat::Raw)
        } else if self.summary {
            Some(ShowDiffFormat::Summary)
        } else if self.name_only {
            Some(ShowDiffFormat::NameOnly)
        } else if self.name_status {
            Some(ShowDiffFormat::NameStatus)
        } else {
            None
        }
    }

    fn merge_diff_mode(
        &self,
        repo: &GitRepo,
        diff_format: Option<ShowDiffFormat>,
    ) -> Result<LogMergeDiffMode> {
        let has_diff_format = diff_format.is_some();
        let config_mode = read_config_entry(repo, "log.diffMerges")?
            .map(|entry| parse_log_diff_merges_config(&entry.value))
            .transpose()?;
        let mut mode = if self.dd {
            LogMergeDiffMode::FirstParent
        } else if self.separate_merges && has_diff_format {
            config_mode.unwrap_or(LogMergeDiffMode::Separate)
        } else if self.dense_combined && has_diff_format {
            LogMergeDiffMode::DenseCombined
        } else if self.combined && has_diff_format {
            LogMergeDiffMode::Combined
        } else if self.first_parent && has_diff_format {
            LogMergeDiffMode::FirstParent
        } else {
            LogMergeDiffMode::Off
        };

        if self.no_diff_merges {
            mode = LogMergeDiffMode::Off;
        }
        if let Some(value) = self.diff_merges {
            mode = parse_log_diff_merges_arg(value, config_mode)?;
        }
        Ok(mode)
    }
}

fn parse_log_diff_merges_arg(
    value: &str,
    config_mode: Option<LogMergeDiffMode>,
) -> Result<LogMergeDiffMode> {
    if value == "on" {
        return Ok(config_mode.unwrap_or(LogMergeDiffMode::Separate));
    }
    parse_log_diff_merges_value(value, true).ok_or_else(|| CliError::Fatal {
        code: 129,
        message: format!("unsupported --diff-merges value '{value}'"),
    })
}

fn parse_log_diff_merges_config(value: &str) -> Result<LogMergeDiffMode> {
    parse_log_diff_merges_value(value, true).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "bad config variable 'log.diffMerges'".into(),
    })
}

fn parse_log_diff_merges_value(value: &str, include_on: bool) -> Option<LogMergeDiffMode> {
    match value {
        "off" | "none" => Some(LogMergeDiffMode::Off),
        "first-parent" | "first_parent" | "1" => Some(LogMergeDiffMode::FirstParent),
        "separate" => Some(LogMergeDiffMode::Separate),
        "combined" | "c" => Some(LogMergeDiffMode::Combined),
        "dense-combined" | "dense_combined" | "cc" => Some(LogMergeDiffMode::DenseCombined),
        "on" if include_on => Some(LogMergeDiffMode::Separate),
        _ => None,
    }
}

pub(crate) fn log(options: LogOptions<'_>) -> Result<()> {
    log_with_options(options)
}

fn log_with_options(options: LogOptions<'_>) -> Result<()> {
    let _trace = phase_trace("log.total");
    let (revs, max_count, parsed_zero) =
        split_log_revs_and_count(options.revs.clone(), options.max_count)?;
    let zero = options.zero || parsed_zero;
    let parsed_log_revs = split_log_revs_and_pickaxe(
        revs,
        options.pickaxe_string,
        options.pickaxe_regex,
        options.patch,
        options.pickaxe_regex_mode,
        options.pickaxe_all,
        options.decorate,
        options.clear_decorations,
        options.ignore_matching_lines.clone(),
    )?;
    let selected_formats = [
        options.patch_with_stat,
        options.stat,
        parsed_log_revs.patch,
        options.numstat,
        options.shortstat,
        options.raw,
        options.name_only,
        options.name_status,
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selected_formats > 1 || (options.summary && !options.patch_with_stat && selected_formats > 0)
    {
        return Err(CliError::Fatal {
            code: 129,
            message:
                "log output format must be one of --patch-with-stat, --stat, --numstat, --shortstat, --raw, --summary, --name-only or --name-status"
                    .into(),
        });
    }
    if options.walk_reflogs {
        return log_reflog(&options, parsed_log_revs.revs, max_count);
    }
    let format = LogFormat::parse(
        options.oneline,
        parsed_log_revs.format.as_deref().or(options.format),
        parsed_log_revs.pretty.as_deref().or(options.pretty),
    )?;
    let ignore_matching_lines =
        compile_ignore_matching_lines(&parsed_log_revs.ignore_matching_lines)?;
    let Some(since) = parse_log_since(options.since) else {
        return Ok(());
    };
    let repo = find_repo()?;
    let show_root = options.root || log_showroot_enabled(&repo)?;
    let diff_format = options.diff_format(parsed_log_revs.patch);
    let merge_diff_mode = options.merge_diff_mode(&repo, diff_format)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let (parsed_revs, implicit_pathspecs) =
        split_log_implicit_pathspecs(&repo, parsed_log_revs.revs);
    let mut parsed_pathspecs = parsed_log_revs.pathspecs;
    parsed_pathspecs.extend(implicit_pathspecs);
    let pathspecs = parsed_pathspecs
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, Path::new(path)))
        .collect::<Result<Vec<_>>>()?;
    let revs = if parsed_revs.is_empty() && !options.all {
        vec!["HEAD".to_owned()]
    } else {
        parsed_revs
    };
    let revs = {
        let _trace = phase_trace("log.collect_revs");
        collect_rev_list_revs(&repo, &store, options.all, revs)?
    };
    let commit_cache = CommitObjectCache::new(&store);
    let decoration_mode =
        if format.uses_decoration_placeholder() && parsed_log_revs.decorate.is_none() {
            Some(LogDecorationMode::Short)
        } else {
            parsed_log_revs.decorate
        };
    let decorations = LogDecorations::load(
        &repo,
        &store,
        decoration_mode,
        parsed_log_revs.clear_decorations,
    )?;
    let notes = LogNotes::load(&repo, &store, matches!(format, LogFormat::Default))?;
    let pickaxe_options = PickaxeOptions {
        string: parsed_log_revs.pickaxe_string.as_deref(),
        regex: parsed_log_revs.pickaxe_regex.as_deref(),
        regex_mode: parsed_log_revs.pickaxe_regex_mode,
        all: parsed_log_revs.pickaxe_all,
    };
    let collect_max_count = if pickaxe_options.enabled() {
        None
    } else {
        max_count
    };
    let mut commits = {
        let _trace = phase_trace("log.collect_commits");
        if options.no_walk && !options.all {
            collect_no_walk_commit_objects(
                &repo,
                &store,
                &commit_cache,
                &revs.include,
                collect_max_count,
            )?
        } else if options.first_parent && !options.all && revs.exclude.is_empty() {
            collect_first_parent_commit_objects(
                &repo,
                &store,
                &commit_cache,
                &revs.include,
                collect_max_count,
            )?
        } else {
            collect_commit_objects_with_exclusions_cached(
                &repo,
                &store,
                &commit_cache,
                &revs,
                collect_max_count,
            )?
        }
    };
    if let Some(since) = since {
        commits.retain(|entry| {
            signature_timestamp_timezone(&entry.commit.committer)
                .map(|(timestamp, _)| timestamp)
                .is_some_and(|timestamp| timestamp > since)
        });
    }
    if pickaxe_options.enabled() {
        commits = filter_log_commits_by_pickaxe(
            &repo,
            &store,
            commits,
            merge_diff_mode,
            pickaxe_options,
            options.first_parent,
        )?;
        if let Some(max_count) = max_count {
            commits.truncate(max_count);
        }
    }
    if options.reverse {
        commits.reverse();
    }
    let abbrev_len = 7;
    let terminates_lines = format.terminates_lines();
    let record_terminator = if zero {
        b"\0".as_slice()
    } else {
        b"\n".as_slice()
    };
    let mut out = io::stdout().lock();
    let _render_trace = phase_trace("log.render");
    for (idx, entry) in commits.iter().enumerate() {
        let commit = entry.commit.as_ref();
        if commit.parents.len() > 1 && matches!(merge_diff_mode, LogMergeDiffMode::Separate) {
            let parent_count = if options.first_parent {
                1
            } else {
                commit.parents.len()
            };
            for parent_index in 0..parent_count {
                let from_parent = if options.first_parent {
                    None
                } else {
                    commit.parents.get(parent_index)
                };
                let rendered = format.render_with_from_parent(
                    &entry.id,
                    commit,
                    from_parent,
                    options.parents,
                    abbrev_len,
                    &decorations,
                    &notes,
                )?;
                out.write_all(rendered.as_bytes())?;
                if let Some(diff_format) = diff_format {
                    if matches!(
                        diff_format,
                        ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary
                    ) {
                        out.write_all(b"---\n")?;
                    } else if format.separates_patch() || format.terminates_lines() {
                        out.write_all(b"\n")?;
                    }
                    drop(out);
                    show_commit_diff_against_parent(
                        &repo,
                        &store,
                        commit,
                        diff_format,
                        parent_index,
                        pickaxe_options,
                        &ignore_matching_lines,
                        &pathspecs,
                        zero,
                    )?;
                    out = io::stdout().lock();
                }
                if parent_index + 1 < parent_count || idx + 1 < commits.len() {
                    out.write_all(record_terminator)?;
                }
            }
            continue;
        }
        let combined_merge_diff = commit.parents.len() > 1
            && matches!(
                merge_diff_mode,
                LogMergeDiffMode::Combined | LogMergeDiffMode::DenseCombined
            );
        let commit_diff_format =
            log_commit_diff_format(commit, show_root, diff_format, merge_diff_mode);
        if options.diff_required && commit_diff_format.is_none() {
            continue;
        }
        let next_output = if options.diff_required {
            commits[idx + 1..].iter().any(|next| {
                log_commit_diff_format(
                    next.commit.as_ref(),
                    show_root,
                    diff_format,
                    merge_diff_mode,
                )
                .is_some()
            })
        } else {
            idx + 1 < commits.len()
        };
        let rendered = format.render_with_context(
            &entry.id,
            commit,
            options.parents,
            abbrev_len,
            &decorations,
            &notes,
        )?;
        out.write_all(rendered.as_bytes())?;
        let root_patch_separator =
            options.root && commit.parents.is_empty() && format.separates_patch();
        if terminates_lines
            || next_output
            || (commit_diff_format.is_some() && !root_patch_separator)
        {
            if !(matches!(
                commit_diff_format,
                Some(ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary)
            ) && !combined_merge_diff)
            {
                out.write_all(record_terminator)?;
                if commit_diff_format.is_some() && terminates_lines && format.separates_patch() {
                    out.write_all(b"\n")?;
                }
            }
        }
        if let Some(diff_format) = commit_diff_format {
            if matches!(
                diff_format,
                ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary
            ) && !combined_merge_diff
            {
                out.write_all(b"---\n")?;
            } else if root_patch_separator {
                out.write_all(b"\n")?;
            }
            drop(out);
            show_commit_diff(
                &repo,
                &store,
                commit,
                diff_format,
                merge_diff_mode,
                options.dense_combined
                    || matches!(merge_diff_mode, LogMergeDiffMode::DenseCombined),
                show_root,
                pickaxe_options,
                &ignore_matching_lines,
                &pathspecs,
                zero,
            )?;
            out = io::stdout().lock();
            if next_output {
                out.write_all(b"\n")?;
            }
        }
    }
    Ok(())
}

fn log_merge_diff_enabled(
    commit: &zmin_git_core::CommitObject,
    merge_diff_mode: LogMergeDiffMode,
) -> bool {
    commit.parents.len() > 1 && !matches!(merge_diff_mode, LogMergeDiffMode::Off)
}

fn log_commit_diff_format(
    commit: &zmin_git_core::CommitObject,
    root: bool,
    diff_format: Option<ShowDiffFormat>,
    merge_diff_mode: LogMergeDiffMode,
) -> Option<ShowDiffFormat> {
    let merge_diff_enabled = log_merge_diff_enabled(commit, merge_diff_mode);
    if merge_diff_enabled
        && matches!(merge_diff_mode, LogMergeDiffMode::FirstParent)
        && diff_format.is_none()
    {
        return Some(ShowDiffFormat::Patch);
    }
    diff_format.filter(|_| {
        commit.parents.len() == 1 || (root && commit.parents.is_empty()) || merge_diff_enabled
    })
}

fn split_log_implicit_pathspecs(repo: &GitRepo, revs: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut parsed_revs = Vec::new();
    let mut pathspecs = Vec::new();
    let mut in_pathspecs = false;
    for rev in revs {
        if in_pathspecs {
            pathspecs.push(rev);
            continue;
        }
        if !parsed_revs.is_empty()
            && resolve_objectish(repo, &rev).is_err()
            && repo.root.join(&rev).exists()
        {
            in_pathspecs = true;
            pathspecs.push(rev);
        } else {
            parsed_revs.push(rev);
        }
    }
    (parsed_revs, pathspecs)
}

fn split_log_revs_and_count(
    revs: Vec<String>,
    max_count: Option<&str>,
) -> Result<(Vec<String>, Option<usize>, bool)> {
    let mut parsed_max_count = parse_log_max_count(max_count)?;
    let mut parsed_zero = false;
    let mut parsed_revs = Vec::new();
    let mut iter = revs.into_iter();
    while let Some(rev) = iter.next() {
        if rev == "-z" {
            parsed_zero = true;
        } else if let Some(value) = rev.strip_prefix('-')
            && !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
        {
            parsed_max_count = Some(value.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("'{value}': not an integer"),
            })?);
        } else if let Some(value) = rev.strip_prefix("--max-count=") {
            parsed_max_count = Some(value.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("'{value}': not an integer"),
            })?);
        } else if rev == "--max-count" || rev == "-n" {
            let Some(value) = iter.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("option '{rev}' requires a value"),
                });
            };
            parsed_max_count = Some(value.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("'{value}': not an integer"),
            })?);
        } else if let Some(value) = rev.strip_prefix("-n")
            && !value.is_empty()
        {
            parsed_max_count = Some(value.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("'{value}': not an integer"),
            })?);
        } else {
            parsed_revs.push(rev);
        }
    }
    Ok((parsed_revs, parsed_max_count, parsed_zero))
}

struct LogParsedRevs {
    revs: Vec<String>,
    pickaxe_string: Option<String>,
    pickaxe_regex: Option<String>,
    patch: bool,
    pickaxe_regex_mode: bool,
    pickaxe_all: bool,
    decorate: Option<LogDecorationMode>,
    clear_decorations: bool,
    ignore_matching_lines: Vec<String>,
    pathspecs: Vec<String>,
    format: Option<String>,
    pretty: Option<String>,
}

fn split_log_revs_and_pickaxe(
    revs: Vec<String>,
    pickaxe_string: Option<&str>,
    pickaxe_regex: Option<&str>,
    patch: bool,
    pickaxe_regex_mode: bool,
    pickaxe_all: bool,
    decorate: Option<&str>,
    clear_decorations: bool,
    ignore_matching_lines: Vec<String>,
) -> Result<LogParsedRevs> {
    let mut parsed_revs = Vec::new();
    let mut parsed_pickaxe_string = pickaxe_string.map(str::to_owned);
    let mut parsed_pickaxe_regex = pickaxe_regex.map(str::to_owned);
    let mut parsed_patch = patch;
    let mut parsed_pickaxe_regex_mode = pickaxe_regex_mode;
    let mut parsed_pickaxe_all = pickaxe_all;
    let mut parsed_decorate = parse_log_decoration_mode(decorate)?;
    let mut parsed_clear_decorations = clear_decorations;
    let mut parsed_ignore_matching_lines = ignore_matching_lines;
    let mut parsed_pathspecs = Vec::new();
    let mut parsed_format = None;
    let mut parsed_pretty = None;
    let mut iter = revs.into_iter();
    while let Some(rev) = iter.next() {
        if rev == "-S" {
            let Some(value) = iter.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '-S' requires a value".into(),
                });
            };
            parsed_pickaxe_string = Some(value);
        } else if let Some(value) = rev.strip_prefix("-S") {
            if value.is_empty() {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '-S' requires a value".into(),
                });
            }
            parsed_pickaxe_string = Some(value.to_owned());
        } else if rev == "-G" {
            let Some(value) = iter.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '-G' requires a value".into(),
                });
            };
            parsed_pickaxe_regex = Some(value);
        } else if let Some(value) = rev.strip_prefix("-G") {
            if value.is_empty() {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '-G' requires a value".into(),
                });
            }
            parsed_pickaxe_regex = Some(value.to_owned());
        } else if rev == "-p" || rev == "--patch" {
            parsed_patch = true;
        } else if rev == "--pickaxe-regex" {
            parsed_pickaxe_regex_mode = true;
        } else if rev == "--pickaxe-all" {
            parsed_pickaxe_all = true;
        } else if rev == "-I" || rev == "--ignore-matching-lines" {
            let Some(value) = iter.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("option '{rev}' requires a value"),
                });
            };
            parsed_ignore_matching_lines.push(value);
        } else if let Some(value) = rev.strip_prefix("-I") {
            if value.is_empty() {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '-I' requires a value".into(),
                });
            }
            parsed_ignore_matching_lines.push(value.to_owned());
        } else if let Some(value) = rev.strip_prefix("--ignore-matching-lines=") {
            parsed_ignore_matching_lines.push(value.to_owned());
        } else if rev == "--format" {
            let Some(value) = iter.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '--format' requires a value".into(),
                });
            };
            parsed_format = Some(value);
        } else if let Some(value) = rev.strip_prefix("--format=") {
            parsed_format = Some(value.to_owned());
        } else if rev == "--pretty" {
            let Some(value) = iter.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '--pretty' requires a value".into(),
                });
            };
            parsed_pretty = Some(value);
        } else if let Some(value) = rev.strip_prefix("--pretty=") {
            parsed_pretty = Some(value.to_owned());
        } else if rev == "--decorate" {
            parsed_decorate = Some(LogDecorationMode::Short);
        } else if let Some(value) = rev.strip_prefix("--decorate=") {
            parsed_decorate = parse_log_decoration_mode(Some(value))?;
        } else if rev == "--clear-decorations" {
            parsed_clear_decorations = true;
        } else if rev == "--" {
            parsed_pathspecs.extend(iter);
            break;
        } else {
            parsed_revs.push(rev);
        }
    }
    Ok(LogParsedRevs {
        revs: parsed_revs,
        pickaxe_string: parsed_pickaxe_string,
        pickaxe_regex: parsed_pickaxe_regex,
        patch: parsed_patch,
        pickaxe_regex_mode: parsed_pickaxe_regex_mode,
        pickaxe_all: parsed_pickaxe_all,
        decorate: parsed_decorate,
        clear_decorations: parsed_clear_decorations,
        ignore_matching_lines: parsed_ignore_matching_lines,
        pathspecs: parsed_pathspecs,
        format: parsed_format,
        pretty: parsed_pretty,
    })
}

fn filter_log_commits_by_pickaxe(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commits: Vec<CollectedCommit>,
    merge_diff_mode: LogMergeDiffMode,
    pickaxe_options: PickaxeOptions<'_>,
    first_parent: bool,
) -> Result<Vec<CollectedCommit>> {
    let mut filtered = Vec::new();
    for entry in commits {
        if log_commit_matches_pickaxe(
            repo,
            store,
            entry.commit.as_ref(),
            merge_diff_mode,
            pickaxe_options,
            first_parent,
        )? {
            filtered.push(entry);
        }
    }
    Ok(filtered)
}

fn log_commit_matches_pickaxe(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    merge_diff_mode: LogMergeDiffMode,
    pickaxe_options: PickaxeOptions<'_>,
    first_parent: bool,
) -> Result<bool> {
    if commit.parents.len() > 1 && matches!(merge_diff_mode, LogMergeDiffMode::Separate) {
        let parent_count = if first_parent {
            1
        } else {
            commit.parents.len()
        };
        for parent_index in 0..parent_count {
            if commit_diff_against_parent_matches_pickaxe(
                repo,
                store,
                commit,
                parent_index,
                pickaxe_options,
            )? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    if commit.parents.len() > 1 {
        return commit_diff_against_parent_matches_pickaxe(repo, store, commit, 0, pickaxe_options);
    }
    commit_diff_against_optional_parent_matches_pickaxe(repo, store, commit, None, pickaxe_options)
}

fn commit_diff_against_parent_matches_pickaxe(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    parent_index: usize,
    pickaxe_options: PickaxeOptions<'_>,
) -> Result<bool> {
    let parent_id = commit
        .parents
        .get(parent_index)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "merge parent index out of range".into(),
        })?;
    commit_diff_against_optional_parent_matches_pickaxe(
        repo,
        store,
        commit,
        Some(parent_id),
        pickaxe_options,
    )
}

fn commit_diff_against_optional_parent_matches_pickaxe(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    parent_id: Option<&ObjectId>,
    pickaxe_options: PickaxeOptions<'_>,
) -> Result<bool> {
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let old_index = if let Some(parent_id) = parent_id {
        let parent = commit_cache.read_commit(parent_id)?;
        tree_cache.read_tree_to_index(&parent.tree)?
    } else {
        GitIndex::new()
    };
    let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
    let entries = diff_indexes(&old_index, &new_index)?;
    let context = DiffIndexContext {
        repo,
        store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    Ok(!apply_pickaxe_filter(&context, entries, pickaxe_options)?.is_empty())
}

fn collect_no_walk_commit_objects<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    let roots = if revs.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        revs.to_vec()
    };
    let mut commits = Vec::new();
    for root in roots {
        if max_count.is_some_and(|limit| commits.len() >= limit) {
            break;
        }
        let id = resolve_commitish(repo, store, &root)?;
        let commit = commit_cache.read_commit(&id)?;
        commits.push(CollectedCommit { id, commit });
    }
    Ok(commits)
}

fn collect_first_parent_commit_objects<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &[String],
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    let roots = if revs.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        revs.to_vec()
    };
    let mut commits = Vec::new();
    for root in roots {
        let mut current = resolve_commitish(repo, store, &root)?;
        loop {
            if max_count.is_some_and(|limit| commits.len() >= limit) {
                return Ok(commits);
            }
            let commit = commit_cache.read_commit(&current)?;
            let next = commit.parents.first().cloned();
            commits.push(CollectedCommit {
                id: current,
                commit,
            });
            let Some(parent) = next else {
                break;
            };
            current = parent;
        }
    }
    Ok(commits)
}

fn log_reflog(options: &LogOptions<'_>, revs: Vec<String>, max_count: Option<usize>) -> Result<()> {
    let repo = find_repo()?;
    let format = options
        .format
        .or(options.pretty)
        .or_else(|| log_reflog_embedded_format(&revs))
        .unwrap_or("%gd %H %gs");
    let format = format.strip_prefix("format:").unwrap_or(format);
    if let Some(patterns) = log_reflog_branch_patterns(&revs) {
        return log_reflog_branches(&repo, format, &patterns, max_count);
    }
    let target = revs.first().map(String::as_str).unwrap_or("HEAD");
    log_reflog_target(&repo, format, target, max_count, false)?;
    Ok(())
}

fn log_reflog_target(
    repo: &GitRepo,
    format: &str,
    target: &str,
    max_count: Option<usize>,
    allow_missing: bool,
) -> Result<usize> {
    let path = reflog_path(&repo, target)?;
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && resolve_objectish(&repo, target).is_ok() =>
        {
            return Ok(0);
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut reflog_index = 0usize;
    let mut emitted = 0usize;
    let limit = max_count.unwrap_or(usize::MAX);
    for_each_reflog_line_rev(file, |line| {
        if emitted >= limit {
            return Ok(());
        }
        let Some(entry) = parse_reflog_entry(line) else {
            return Ok(());
        };
        let entry_index = reflog_index;
        reflog_index += 1;
        if entry.new_id == zero_object_id() {
            return Ok(());
        }
        println!(
            "{}",
            render_reflog_log_format(format, target, entry_index, &entry)?
        );
        emitted += 1;
        Ok(())
    })?;
    Ok(emitted)
}

fn log_reflog_branch_patterns(revs: &[String]) -> Option<Vec<String>> {
    let mut patterns = Vec::new();
    for rev in revs {
        if rev == "--branches" || rev == "--heads" {
            patterns.push("*".to_owned());
        } else if let Some(pattern) = rev
            .strip_prefix("--branches=")
            .or_else(|| rev.strip_prefix("--heads="))
        {
            patterns.push(pattern.to_owned());
        }
    }
    (!patterns.is_empty()).then_some(patterns)
}

fn log_reflog_embedded_format(revs: &[String]) -> Option<&str> {
    revs.iter().find_map(|rev| {
        rev.strip_prefix("--format=")
            .or_else(|| rev.strip_prefix("--pretty="))
    })
}

fn log_reflog_branches(
    repo: &GitRepo,
    format: &str,
    patterns: &[String],
    max_count: Option<usize>,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut branches = Vec::new();
    refs.for_each_ref_name("refs/heads/", |ref_name| {
        let short = ref_name
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_name)
            .to_owned();
        if patterns
            .iter()
            .any(|pattern| wildcard_match(pattern, &short) || wildcard_match(pattern, ref_name))
        {
            branches.push(short);
        }
        Ok::<(), CliError>(())
    })?;
    branches.sort();
    let mut emitted = 0usize;
    let limit = max_count.unwrap_or(usize::MAX);
    for branch in branches {
        if emitted >= limit {
            break;
        }
        emitted += log_reflog_target(repo, format, &branch, Some(limit - emitted), true)?;
    }
    Ok(())
}

fn render_reflog_log_format(
    pattern: &str,
    ref_name: &str,
    index: usize,
    entry: &ReflogEntry,
) -> Result<String> {
    let mut out = String::new();
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        let Some(atom) = chars.next() else {
            return Err(CliError::Fatal {
                code: 128,
                message: "unterminated log format placeholder".into(),
            });
        };
        match atom {
            '%' => out.push('%'),
            'H' => out.push_str(&entry.new_id.to_hex()),
            'c' => {
                let Some(next) = chars.next() else {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "unterminated committer log format placeholder".into(),
                    });
                };
                match next {
                    'd' => out.push_str(&reflog_default_date(entry)?),
                    _ => {
                        out.push('%');
                        out.push('c');
                        out.push(next);
                    }
                }
            }
            'g' => {
                let Some(next) = chars.next() else {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "unterminated reflog log format placeholder".into(),
                    });
                };
                match next {
                    'D' | 'd' => out.push_str(&format!("{ref_name}@{{{index}}}")),
                    's' => out.push_str(&entry.message),
                    _ => {
                        out.push('%');
                        out.push('g');
                        out.push(next);
                    }
                }
            }
            _ => {
                out.push('%');
                out.push(atom);
            }
        }
    }
    Ok(out)
}

fn reflog_default_date(entry: &ReflogEntry) -> Result<String> {
    let offset = parse_timezone_offset(&entry.timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "reflog entry has invalid timezone".into(),
    })?;
    let utc =
        chrono::DateTime::from_timestamp(entry.timestamp, 0).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "reflog entry timestamp is out of range".into(),
        })?;
    Ok(utc
        .with_timezone(&offset)
        .format("%a %b %e %H:%M:%S %Y %z")
        .to_string())
}

fn parse_log_max_count(value: Option<&str>) -> Result<Option<usize>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("'{value}': not an integer"),
    })?;
    Ok(Some(parsed))
}

fn parse_log_since(value: Option<&str>) -> Option<Option<i64>> {
    let Some(value) = value else {
        return Some(None);
    };
    if let Some(relative) = parse_relative_log_since(value) {
        return Some(Some(relative));
    }
    if let Ok(timestamp) = value.parse::<i64>() {
        return Some(Some(timestamp));
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(Some(datetime.timestamp()));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|datetime| Some(datetime.and_utc().timestamp()));
    }
    if let Ok(datetime) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Some(Some(datetime.and_utc().timestamp()));
    }
    None
}

fn parse_relative_log_since(value: &str) -> Option<i64> {
    let normalized = value.trim().to_ascii_lowercase();
    let now = current_unix_timestamp().ok()?;
    match normalized.as_str() {
        "yesterday" => return Some(now - 86_400),
        "today" => return Some(now - seconds_since_midnight_utc()?),
        _ => {}
    }
    let parts = normalized.split('.').collect::<Vec<_>>();
    if parts.len() == 3 && parts[2] == "ago" {
        let amount = parts[0].parse::<i64>().ok()?;
        let unit_seconds = match parts[1].trim_end_matches('s') {
            "second" => 1,
            "minute" => 60,
            "hour" => 3_600,
            "day" => 86_400,
            "week" => 604_800,
            "month" => 2_629_746,
            "year" => 31_556_952,
            _ => return None,
        };
        return Some(now - amount.saturating_mul(unit_seconds));
    }
    None
}

fn seconds_since_midnight_utc() -> Option<i64> {
    let now = current_unix_timestamp().ok()?;
    Some(now.rem_euclid(86_400))
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LogFormat<'a> {
    Default,
    ShortOneline,
    FullOneline,
    Custom {
        pattern: &'a str,
        terminates_lines: bool,
    },
}

#[derive(Debug, Clone, Copy)]
enum LogDecorationMode {
    Short,
    Full,
}

fn parse_log_decoration_mode(value: Option<&str>) -> Result<Option<LogDecorationMode>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        "" | "short" | "auto" | "true" => Ok(Some(LogDecorationMode::Short)),
        "full" => Ok(Some(LogDecorationMode::Full)),
        "no" | "false" => Ok(None),
        other => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported --decorate value '{other}'"),
        }),
    }
}

pub(crate) struct LogDecorations {
    entries: HashMap<String, Vec<String>>,
}

impl LogDecorations {
    pub(crate) fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn load(
        repo: &GitRepo,
        _store: &LooseObjectStore,
        mode: Option<LogDecorationMode>,
        clear_decorations: bool,
    ) -> Result<Self> {
        let Some(mode) = mode else {
            return Ok(Self::empty());
        };
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let mut decorations = Self::empty();
        let current_branch = current_branch_ref(&refs)?;
        if let Ok(head_id) = refs.resolve("HEAD") {
            let display = match current_branch.as_deref() {
                Some(branch) => format!("HEAD -> {}", decorate_ref_name(branch, mode, true)),
                None => "HEAD".to_owned(),
            };
            decorations.add(head_id, display);
        }

        let prefix = if clear_decorations { "refs/" } else { "refs/" };
        let mut ref_rows = Vec::<(String, ObjectId)>::new();
        refs.for_each_resolved_ref(prefix, |ref_name, id| {
            if !clear_decorations && !log_decorates_ref_by_default(ref_name) {
                return Ok(());
            }
            if current_branch.as_deref() == Some(ref_name) {
                return Ok(());
            }
            ref_rows.push((ref_name.to_owned(), id.clone()));
            Ok::<(), CliError>(())
        })?;
        ref_rows.sort_by(|left, right| {
            log_decoration_sort_key(&left.0).cmp(&log_decoration_sort_key(&right.0))
        });
        for (ref_name, id) in ref_rows {
            decorations.add(id, decorate_ref_name(&ref_name, mode, false));
        }
        Ok(decorations)
    }

    fn add(&mut self, id: ObjectId, display: String) {
        self.entries.entry(id.to_hex()).or_default().push(display);
    }

    fn get(&self, id: &ObjectId) -> Option<&[String]> {
        self.entries.get(&id.to_hex()).map(Vec::as_slice)
    }
}

fn log_decorates_ref_by_default(ref_name: &str) -> bool {
    ref_name.starts_with("refs/heads/")
        || ref_name.starts_with("refs/remotes/")
        || ref_name.starts_with("refs/tags/")
}

fn log_decoration_sort_key(ref_name: &str) -> (u8, &str, u8, &str) {
    if let Some(short) = ref_name.strip_prefix("refs/tags/") {
        return (0, short, 0, "");
    }
    if let Some(short) = ref_name.strip_prefix("refs/remotes/") {
        if let Some((remote, name)) = short.split_once('/') {
            let remote_head = u8::from(name == "HEAD");
            return (1, remote, remote_head, name);
        }
        return (1, short, 0, "");
    }
    if let Some(short) = ref_name.strip_prefix("refs/heads/") {
        return (2, short, 0, "");
    }
    (3, ref_name, 0, "")
}

fn decorate_ref_name(ref_name: &str, mode: LogDecorationMode, head_target: bool) -> String {
    if matches!(mode, LogDecorationMode::Full) {
        if ref_name.starts_with("refs/tags/") {
            return format!("tag: {ref_name}");
        }
        return ref_name.to_owned();
    }
    if ref_name.starts_with("refs/notes/") {
        return ref_name.to_owned();
    }
    if let Some(short) = ref_name.strip_prefix("refs/heads/") {
        return short.to_owned();
    }
    if let Some(short) = ref_name.strip_prefix("refs/tags/") {
        return format!("tag: {short}");
    }
    if let Some(short) = ref_name.strip_prefix("refs/remotes/") {
        return short.to_owned();
    }
    if head_target {
        return ref_name.to_owned();
    }
    ref_name.to_owned()
}

pub(crate) struct LogNotes {
    entries: HashMap<String, Vec<u8>>,
}

impl LogNotes {
    pub(crate) fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub(crate) fn load(repo: &GitRepo, store: &LooseObjectStore, enabled: bool) -> Result<Self> {
        if !enabled {
            return Ok(Self::empty());
        }
        let runtime = CliPrimitiveRuntime::new_default(repo);
        let object_store = runtime.object_store_adapter();
        let refs = runtime.refs_store_adapter();
        let notes = notes_commands::read_notes_map(&object_store, &refs, "refs/notes/commits")?;
        let mut entries = HashMap::new();
        for (object, note_id) in notes {
            let note = store.read_object(&note_id)?;
            if note.kind == GitObjectKind::Blob {
                entries.insert(object, note.content);
            }
        }
        Ok(Self { entries })
    }

    pub(crate) fn get(&self, id: &ObjectId) -> Option<&[u8]> {
        self.entries.get(&id.to_hex()).map(Vec::as_slice)
    }
}

impl<'a> LogFormat<'a> {
    pub(crate) fn parse(
        oneline: bool,
        format: Option<&'a str>,
        pretty: Option<&'a str>,
    ) -> Result<Self> {
        if oneline && (format.is_some() || pretty.is_some()) {
            return Err(CliError::Fatal {
                code: 128,
                message: "`log --oneline` cannot be combined with --format or --pretty".into(),
            });
        }
        if let Some(raw) = format {
            return match raw {
                "oneline" => Ok(Self::FullOneline),
                pattern => Ok(Self::Custom {
                    pattern: pattern.strip_prefix("format:").unwrap_or(pattern),
                    terminates_lines: !pattern.starts_with("format:"),
                }),
            };
        }
        let Some(raw) = pretty else {
            if oneline {
                return Ok(Self::ShortOneline);
            }
            return Ok(Self::Default);
        };
        match raw {
            "" | "medium" | "default" => Ok(Self::Default),
            "oneline" => Ok(Self::FullOneline),
            pattern => Ok(Self::Custom {
                pattern: pattern.strip_prefix("format:").unwrap_or(pattern),
                terminates_lines: !pattern.starts_with("format:"),
            }),
        }
    }

    pub(crate) fn terminates_lines(&self) -> bool {
        match self {
            Self::ShortOneline | Self::FullOneline => true,
            Self::Default => false,
            Self::Custom {
                terminates_lines, ..
            } => *terminates_lines,
        }
    }

    pub(crate) fn separates_patch(&self) -> bool {
        match self {
            Self::Default => true,
            Self::ShortOneline | Self::FullOneline => false,
            Self::Custom {
                terminates_lines, ..
            } => *terminates_lines,
        }
    }

    fn uses_decoration_placeholder(&self) -> bool {
        match self {
            Self::Custom { pattern, .. } => log_format_uses_placeholder(pattern, 'D'),
            _ => false,
        }
    }

    pub(crate) fn render(
        &self,
        id: &ObjectId,
        commit: &zmin_git_core::CommitObject,
        parents: bool,
        abbrev_len: usize,
    ) -> Result<String> {
        let decorations = LogDecorations::empty();
        let notes = LogNotes::empty();
        self.render_with_context(id, commit, parents, abbrev_len, &decorations, &notes)
    }

    pub(crate) fn render_with_context(
        &self,
        id: &ObjectId,
        commit: &zmin_git_core::CommitObject,
        parents: bool,
        abbrev_len: usize,
        decorations: &LogDecorations,
        notes: &LogNotes,
    ) -> Result<String> {
        match self {
            Self::Default => {
                render_default_log(id, commit, parents, abbrev_len, decorations, notes)
            }
            Self::ShortOneline => Ok(format!(
                "{}{}{} {}",
                short_object_id_len(id, abbrev_len),
                short_parent_suffix(commit, parents, abbrev_len),
                render_oneline_decorations(decorations, id),
                commit_subject(&commit.message)
            )),
            Self::FullOneline => Ok(format!(
                "{}{}{} {}",
                id.to_hex(),
                parent_suffix(commit, parents),
                render_oneline_decorations(decorations, id),
                commit_subject(&commit.message)
            )),
            Self::Custom { pattern, .. } => {
                render_log_format(pattern, id, commit, abbrev_len, decorations, notes)
            }
        }
    }

    fn render_with_from_parent(
        &self,
        id: &ObjectId,
        commit: &zmin_git_core::CommitObject,
        from_parent: Option<&ObjectId>,
        parents: bool,
        abbrev_len: usize,
        decorations: &LogDecorations,
        notes: &LogNotes,
    ) -> Result<String> {
        match (self, from_parent) {
            (Self::Default, Some(parent)) => {
                render_default_log_from_parent(id, commit, parent, parents, abbrev_len)
            }
            _ => self.render_with_context(id, commit, parents, abbrev_len, decorations, notes),
        }
    }
}

fn render_oneline_decorations(decorations: &LogDecorations, id: &ObjectId) -> String {
    let Some(items) = decorations.get(id) else {
        return String::new();
    };
    if items.is_empty() {
        String::new()
    } else {
        format!(" ({})", items.join(", "))
    }
}

fn render_default_log(
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    parents: bool,
    abbrev_len: usize,
    decorations: &LogDecorations,
    notes: &LogNotes,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("commit ");
    out.push_str(&id.to_hex());
    if let Some(items) = decorations.get(id)
        && !items.is_empty()
    {
        out.push_str(" (");
        out.push_str(&items.join(", "));
        out.push(')');
    }
    if parents {
        out.push_str(&parent_suffix(commit, true));
    }
    out.push('\n');
    if commit.parents.len() > 1 {
        out.push_str("Merge:");
        for parent in &commit.parents {
            out.push(' ');
            out.push_str(&short_object_id_len(parent, abbrev_len));
        }
        out.push('\n');
    }
    out.push_str("Author: ");
    out.push_str(&signature_name(&commit.author));
    out.push_str(" <");
    out.push_str(&signature_email(&commit.author));
    out.push_str(">\n");
    out.push_str("Date:   ");
    out.push_str(&signature_log_date(&commit.author)?);
    out.push_str("\n\n");
    for line in split_log_message_lines(&commit.message) {
        out.push_str("    ");
        out.push_str(&String::from_utf8_lossy(line));
        out.push('\n');
    }
    if let Some(note) = notes.get(id) {
        out.push('\n');
        out.push_str("Notes:\n");
        for line in split_log_message_lines(note) {
            out.push_str("    ");
            out.push_str(&String::from_utf8_lossy(line));
            out.push('\n');
        }
    }
    Ok(out)
}

fn render_default_log_from_parent(
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    from_parent: &ObjectId,
    parents: bool,
    abbrev_len: usize,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("commit ");
    out.push_str(&id.to_hex());
    out.push_str(" (from ");
    out.push_str(&from_parent.to_hex());
    out.push(')');
    if parents {
        out.push_str(&parent_suffix(commit, true));
    }
    out.push('\n');
    if commit.parents.len() > 1 {
        out.push_str("Merge:");
        for parent in &commit.parents {
            out.push(' ');
            out.push_str(&short_object_id_len(parent, abbrev_len));
        }
        out.push('\n');
    }
    out.push_str("Author: ");
    out.push_str(&signature_name(&commit.author));
    out.push_str(" <");
    out.push_str(&signature_email(&commit.author));
    out.push_str(">\n");
    out.push_str("Date:   ");
    out.push_str(&signature_log_date(&commit.author)?);
    out.push_str("\n\n");
    for line in split_log_message_lines(&commit.message) {
        out.push_str("    ");
        out.push_str(&String::from_utf8_lossy(line));
        out.push('\n');
    }
    Ok(out)
}

fn log_format_uses_placeholder(pattern: &str, target: char) -> bool {
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        let Some(atom) = chars.next() else {
            break;
        };
        if atom == target {
            return true;
        }
        if atom == 'x' {
            let _ = chars.next();
            let _ = chars.next();
        }
    }
    false
}

fn parent_suffix(commit: &zmin_git_core::CommitObject, parents: bool) -> String {
    if !parents {
        return String::new();
    }
    let mut suffix = String::new();
    for parent in &commit.parents {
        suffix.push(' ');
        suffix.push_str(&parent.to_hex());
    }
    suffix
}

fn short_parent_suffix(
    commit: &zmin_git_core::CommitObject,
    parents: bool,
    abbrev_len: usize,
) -> String {
    if !parents {
        return String::new();
    }
    let mut suffix = String::new();
    for parent in &commit.parents {
        suffix.push(' ');
        suffix.push_str(&short_object_id_len(parent, abbrev_len));
    }
    suffix
}

fn render_log_format(
    pattern: &str,
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    abbrev_len: usize,
    decorations: &LogDecorations,
    notes: &LogNotes,
) -> Result<String> {
    let mut out = String::new();
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        let Some(atom) = chars.next() else {
            return Err(CliError::Fatal {
                code: 128,
                message: "unterminated log format placeholder".into(),
            });
        };
        match atom {
            '%' => out.push('%'),
            'H' => out.push_str(&id.to_hex()),
            'h' => out.push_str(&short_object_id_len(id, abbrev_len)),
            'P' => {
                for (index, parent) in commit.parents.iter().enumerate() {
                    if index > 0 {
                        out.push(' ');
                    }
                    out.push_str(&parent.to_hex());
                }
            }
            'D' => {
                if let Some(items) = decorations.get(id) {
                    out.push_str(&items.join(", "));
                }
            }
            'N' => {
                if let Some(note) = notes.get(id) {
                    out.push_str(&String::from_utf8_lossy(note));
                }
            }
            's' => out.push_str(&commit_subject(&commit.message)),
            'x' => {
                let high = chars.next();
                let low = chars.next();
                match (high, low) {
                    (Some(high), Some(low))
                        if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() =>
                    {
                        let hex = format!("{high}{low}");
                        let byte = u8::from_str_radix(&hex, 16).map_err(|_| CliError::Fatal {
                            code: 128,
                            message: format!("invalid log format escape '%x{hex}'"),
                        })?;
                        out.push(char::from(byte));
                    }
                    _ => {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "unterminated log format hex escape".into(),
                        });
                    }
                }
            }
            'a' => {
                let Some(next) = chars.next() else {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "unterminated author log format placeholder".into(),
                    });
                };
                match next {
                    'n' => out.push_str(&signature_name(&commit.author)),
                    'e' => out.push_str(&signature_email(&commit.author)),
                    'd' => out.push_str(&signature_log_date(&commit.author)?),
                    't' => {
                        out.push_str(&signature_timestamp(&commit.author).unwrap_or(0).to_string())
                    }
                    _ => {
                        out.push('%');
                        out.push('a');
                        out.push(next);
                    }
                }
            }
            'c' => {
                let Some(next) = chars.next() else {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "unterminated committer log format placeholder".into(),
                    });
                };
                match next {
                    'n' => out.push_str(&signature_name(&commit.committer)),
                    'e' => out.push_str(&signature_email(&commit.committer)),
                    'd' => out.push_str(&signature_log_date(&commit.committer)?),
                    't' => out.push_str(
                        &signature_timestamp(&commit.committer)
                            .unwrap_or(0)
                            .to_string(),
                    ),
                    _ => {
                        out.push('%');
                        out.push('c');
                        out.push(next);
                    }
                }
            }
            _ => {
                out.push('%');
                out.push(atom);
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub(crate) struct ShowOptions<'a> {
    pub(crate) no_patch: bool,
    pub(crate) oneline: bool,
    pub(crate) zero: bool,
    pub(crate) stat: bool,
    pub(crate) patch_with_raw: bool,
    pub(crate) patch_with_stat: bool,
    pub(crate) numstat: bool,
    pub(crate) shortstat: bool,
    pub(crate) raw: bool,
    pub(crate) summary: bool,
    pub(crate) name_only: bool,
    pub(crate) name_status: bool,
    pub(crate) root: bool,
    pub(crate) combined: bool,
    pub(crate) separate_merges: bool,
    pub(crate) first_parent: bool,
    pub(crate) format: Option<&'a str>,
    pub(crate) pretty: Option<&'a str>,
    pub(crate) args: Vec<String>,
}

impl ShowOptions<'_> {
    fn diff_format(&self) -> ShowDiffFormat {
        if self.patch_with_raw && self.summary {
            ShowDiffFormat::PatchWithRawSummary
        } else if self.patch_with_raw {
            ShowDiffFormat::PatchWithRaw
        } else if self.patch_with_stat && self.summary {
            ShowDiffFormat::PatchWithStatSummary
        } else if self.patch_with_stat {
            ShowDiffFormat::PatchWithStat
        } else if self.stat && self.summary {
            ShowDiffFormat::StatSummary
        } else if self.stat {
            ShowDiffFormat::Stat
        } else if self.numstat {
            ShowDiffFormat::Numstat
        } else if self.shortstat {
            ShowDiffFormat::Shortstat
        } else if self.raw {
            ShowDiffFormat::Raw
        } else if self.summary {
            ShowDiffFormat::Summary
        } else if self.name_only {
            ShowDiffFormat::NameOnly
        } else if self.name_status {
            ShowDiffFormat::NameStatus
        } else {
            ShowDiffFormat::Patch
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ShowDiffFormat {
    Patch,
    PatchWithRaw,
    PatchWithRawSummary,
    PatchWithStat,
    PatchWithStatSummary,
    Stat,
    StatSummary,
    Numstat,
    Shortstat,
    Raw,
    Summary,
    NameOnly,
    NameStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogMergeDiffMode {
    Off,
    FirstParent,
    Combined,
    DenseCombined,
    Separate,
}

pub(crate) fn show(options: ShowOptions<'_>) -> Result<()> {
    show_with_options(options)
}

fn show_merge_diff_mode(options: &ShowOptions<'_>) -> LogMergeDiffMode {
    if options.first_parent {
        LogMergeDiffMode::FirstParent
    } else if options.separate_merges {
        LogMergeDiffMode::Separate
    } else {
        LogMergeDiffMode::Combined
    }
}

fn show_with_options(options: ShowOptions<'_>) -> Result<()> {
    let selected_formats = [
        options.patch_with_raw,
        options.patch_with_stat,
        options.stat,
        options.numstat,
        options.shortstat,
        options.raw,
        options.summary,
        options.name_only,
        options.name_status,
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    let valid_combined_format = selected_formats == 2
        && options.summary
        && (options.stat || options.patch_with_stat || options.patch_with_raw);
    if selected_formats > 1 && !valid_combined_format {
        return Err(CliError::Fatal {
            code: 129,
            message:
                "show output format must be one of --patch-with-raw, --patch-with-stat, --stat, --numstat, --shortstat, --raw, --summary, --name-only or --name-status"
                    .into(),
        });
    }
    if options.format == Some("raw") && (options.oneline || options.pretty.is_some()) {
        return Err(CliError::Fatal {
            code: 128,
            message: "`show --format=raw` cannot be combined with --oneline or --pretty".into(),
        });
    }
    if show_should_use_log_pipeline(&options) {
        return show_via_log(options);
    }
    let objectish = options
        .args
        .first()
        .cloned()
        .unwrap_or_else(|| "HEAD".to_owned());
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let id =
        resolve_objectish(&repo, &objectish).map_err(|_| ambiguous_revision_error(&objectish))?;
    let object = store.read_object(&id)?;
    let show_root = options.root || log_showroot_enabled(&repo)?;
    show_object(&store, &objectish, &object, options, show_root)
}

fn log_showroot_enabled(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "log.showroot")? else {
        return Ok(true);
    };
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!(
            "bad boolean config value '{}' for 'log.showroot'",
            entry.value
        ),
    })
}

fn show_should_use_log_pipeline(options: &ShowOptions<'_>) -> bool {
    options.args.len() > 1 || options.args.iter().any(|arg| arg == "--")
}

fn show_raw_format_requested(options: &ShowOptions<'_>) -> bool {
    options.format == Some("raw") || options.pretty == Some("raw")
}

fn show_via_log(options: ShowOptions<'_>) -> Result<()> {
    if options.name_only {
        return show_name_only_multi(options);
    }
    log(LogOptions {
        oneline: options.oneline,
        zero: options.zero,
        all: false,
        parents: false,
        first_parent: options.first_parent,
        no_diff_merges: false,
        diff_merges: None,
        separate_merges: options.separate_merges,
        dd: false,
        reverse: false,
        root: options.root,
        patch: !(options.no_patch
            || options.stat
            || options.numstat
            || options.shortstat
            || options.raw
            || options.summary
            || options.name_only
            || options.name_status),
        patch_with_stat: options.patch_with_stat,
        combined: options.combined,
        dense_combined: false,
        stat: options.stat,
        numstat: options.numstat,
        shortstat: options.shortstat,
        raw: options.raw,
        summary: options.summary,
        name_only: options.name_only,
        name_status: options.name_status,
        diff_required: false,
        decorate: None,
        clear_decorations: false,
        pickaxe_string: None,
        pickaxe_regex: None,
        pickaxe_regex_mode: false,
        pickaxe_all: false,
        ignore_matching_lines: Vec::new(),
        walk_reflogs: false,
        no_walk: true,
        format: options.format,
        max_count: None,
        since: None,
        pretty: options.pretty,
        revs: options.args,
    })
}

fn show_name_only_multi(options: ShowOptions<'_>) -> Result<()> {
    let (revs, paths) = split_show_revs_and_paths(options.args);
    let revs = if revs.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        revs
    };
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let format = LogFormat::parse(options.oneline, options.format, options.pretty)?;
    let pathspecs = paths.into_iter().map(PathBuf::from).collect::<Vec<_>>();
    for rev in revs {
        let id = resolve_objectish(&repo, &rev).map_err(|_| ambiguous_revision_error(&rev))?;
        let commit = commit_cache.read_commit(&id)?;
        let rendered = format.render_with_context(
            &id,
            &commit,
            false,
            default_abbrev_len(&store)?,
            &LogDecorations::empty(),
            &LogNotes::empty(),
        )?;
        io::stdout().write_all(rendered.as_bytes())?;
        if format.terminates_lines() {
            io::stdout().write_all(b"\n")?;
        }
        let old_index = if let Some(parent) = commit.parents.first() {
            let parent_commit = commit_cache.read_commit(parent)?;
            tree_cache.read_tree_to_index(&parent_commit.tree)?
        } else if options.root {
            GitIndex::new()
        } else {
            continue;
        };
        let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
        let entries =
            filtered_diff_entries(&repo, &old_index, &new_index, &pathspecs, None, None, false)?;
        if entries.is_empty() {
            continue;
        }
        io::stdout().write_all(b"\n")?;
        print_name_only_entries(&entries, None, false)?;
    }
    Ok(())
}

fn split_show_revs_and_paths(args: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut revs = Vec::new();
    let mut paths = Vec::new();
    let mut in_paths = false;
    for arg in args {
        if arg == "--" {
            in_paths = true;
        } else if in_paths {
            paths.push(arg);
        } else {
            revs.push(arg);
        }
    }
    (revs, paths)
}

fn show_object(
    store: &LooseObjectStore,
    objectish: &str,
    object: &LooseObject,
    options: ShowOptions<'_>,
    show_root: bool,
) -> Result<()> {
    match object.kind {
        GitObjectKind::Blob => {
            io::stdout().write_all(&object.content)?;
            Ok(())
        }
        GitObjectKind::Tree => show_tree_object(store, objectish, &object.id),
        GitObjectKind::Commit => {
            let commit = decode_commit(GitHashAlgorithm::Sha1, &object.content)?;
            if options.no_patch {
                if show_raw_format_requested(&options) {
                    return show_raw_commit(&object.id, &object.content);
                }
                let format = LogFormat::parse(options.oneline, options.format, options.pretty)?;
                let rendered =
                    format.render(&object.id, &commit, false, default_abbrev_len(store)?)?;
                io::stdout().write_all(rendered.as_bytes())?;
                if format.terminates_lines() {
                    io::stdout().write_all(b"\n")?;
                }
                return Ok(());
            }
            if show_raw_format_requested(&options) {
                show_raw_commit(&object.id, &object.content)?;
                if commit.parents.is_empty() && !show_root {
                    return Ok(());
                }
                io::stdout().write_all(b"\n")?;
                let repo = find_repo()?;
                return show_commit_diff(
                    &repo,
                    store,
                    &commit,
                    options.diff_format(),
                    show_merge_diff_mode(&options),
                    !options.combined,
                    show_root,
                    empty_pickaxe_options(),
                    &[],
                    &[],
                    options.zero,
                );
            }
            let format = LogFormat::parse(options.oneline, options.format, options.pretty)?;
            if options.separate_merges && commit.parents.len() > 1 {
                let repo = find_repo()?;
                let decorations = LogDecorations::empty();
                let notes = LogNotes::empty();
                let abbrev_len = default_abbrev_len(store)?;
                let diff_format = options.diff_format();
                let mut out = io::stdout().lock();
                for (idx, parent) in commit.parents.iter().enumerate() {
                    let rendered = format.render_with_from_parent(
                        &object.id,
                        &commit,
                        Some(parent),
                        false,
                        abbrev_len,
                        &decorations,
                        &notes,
                    )?;
                    out.write_all(rendered.as_bytes())?;
                    if format.separates_patch()
                        && matches!(
                            diff_format,
                            ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary
                        )
                    {
                        out.write_all(b"---\n")?;
                    } else if format.terminates_lines() || format.separates_patch() {
                        out.write_all(b"\n")?;
                    }
                    drop(out);
                    show_commit_diff_against_parent(
                        &repo,
                        store,
                        &commit,
                        diff_format,
                        idx,
                        empty_pickaxe_options(),
                        &[],
                        &[],
                        options.zero,
                    )?;
                    out = io::stdout().lock();
                    if idx + 1 < commit.parents.len() {
                        out.write_all(b"\n")?;
                    }
                }
                return Ok(());
            }
            let rendered = format.render(&object.id, &commit, false, default_abbrev_len(store)?)?;
            io::stdout().write_all(rendered.as_bytes())?;
            if format.terminates_lines() {
                io::stdout().write_all(b"\n")?;
            }
            if commit.parents.is_empty() && !show_root {
                return Ok(());
            }
            let diff_format = options.diff_format();
            let has_diff_entries = if commit.parents.len() <= 1 {
                show_commit_diff_has_entries(
                    &find_repo()?,
                    store,
                    &commit,
                    show_root,
                    empty_pickaxe_options(),
                    &[],
                )?
            } else {
                true
            };
            if format.separates_patch()
                && matches!(
                    diff_format,
                    ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary
                )
            {
                if has_diff_entries {
                    io::stdout().write_all(b"---\n")?;
                }
            } else if format.separates_patch() && has_diff_entries {
                io::stdout().write_all(b"\n")?;
            }
            if commit.parents.len() > 1 && format.terminates_lines() && !format.separates_patch() {
                io::stdout().write_all(b"\n")?;
            }
            let repo = find_repo()?;
            show_commit_diff(
                &repo,
                store,
                &commit,
                diff_format,
                show_merge_diff_mode(&options),
                !options.combined,
                show_root,
                empty_pickaxe_options(),
                &[],
                &[],
                options.zero,
            )
        }
        GitObjectKind::Tag => show_tag_object(store, &object.content, options, show_root),
    }
}

fn show_commit_diff_has_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    include_root_diff: bool,
    pickaxe_options: PickaxeOptions<'_>,
    pathspecs: &[Vec<u8>],
) -> Result<bool> {
    if commit.parents.len() > 1 {
        return Ok(true);
    }
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let old_index = if let Some(parent) = commit.parents.first() {
        let parent_commit = commit_cache.read_commit(parent)?;
        tree_cache.read_tree_to_index(&parent_commit.tree)?
    } else if include_root_diff {
        GitIndex::new()
    } else {
        return Ok(false);
    };
    let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
    let entries = diff_indexes(&old_index, &new_index)?
        .into_iter()
        .filter(|entry| diff_entry_matches_pathspec(entry, pathspecs))
        .collect::<Vec<_>>();
    let context = DiffIndexContext {
        repo,
        store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    Ok(!apply_pickaxe_filter(&context, entries, pickaxe_options)?.is_empty())
}

fn show_tree_object(store: &LooseObjectStore, objectish: &str, tree_id: &ObjectId) -> Result<()> {
    println!("tree {objectish}");
    println!();
    let tree_cache = TreeObjectCache::new(store);
    for entry in tree_cache.read_tree(tree_id)?.iter() {
        let suffix = if entry.mode == TreeMode::Tree {
            "/"
        } else {
            ""
        };
        println!("{}{}", String::from_utf8_lossy(&entry.name), suffix);
    }
    Ok(())
}

fn show_raw_commit(id: &ObjectId, content: &[u8]) -> Result<()> {
    let message_start = content
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|idx| idx + 2)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit object missing header end".into(),
        })?;
    let headers = &content[..message_start - 2];
    let message = &content[message_start..];
    let mut out = io::stdout().lock();
    writeln!(out, "commit {}", id.to_hex())?;
    out.write_all(headers)?;
    out.write_all(b"\n\n")?;
    write_indented_message(&mut out, message)
}

fn show_commit_diff(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    format: ShowDiffFormat,
    merge_diff_mode: LogMergeDiffMode,
    dense_combined: bool,
    include_root_diff: bool,
    pickaxe_options: PickaxeOptions<'_>,
    ignore_matching_lines: &[Regex],
    pathspecs: &[Vec<u8>],
    nul_terminated: bool,
) -> Result<()> {
    if commit.parents.len() > 1 && matches!(merge_diff_mode, LogMergeDiffMode::Off) {
        return Ok(());
    }
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    if commit.parents.len() > 1 {
        if matches!(merge_diff_mode, LogMergeDiffMode::FirstParent) {
            return show_commit_diff_against_parent(
                repo,
                store,
                commit,
                format,
                0,
                pickaxe_options,
                ignore_matching_lines,
                pathspecs,
                nul_terminated,
            );
        }
        let parent_indexes =
            diff_commands::combined_diff_tree_parent_indexes(commit, &commit_cache, &tree_cache)?;
        let result_index = tree_cache
            .read_tree_to_index(&commit.tree)
            .map_err(CliError::Io)?;
        match format {
            ShowDiffFormat::Patch => {
                diff_commands::print_combined_diff_tree_patches(
                    store,
                    &parent_indexes,
                    &result_index,
                    pathspecs,
                    diff_commands::CombinedPatchRenderOptions {
                        abbrev_len: None,
                        relative_prefix: None,
                        old_prefix: "a/",
                        new_prefix: "b/",
                        dense_combined,
                        line_prefix: None,
                    },
                )?;
            }
            ShowDiffFormat::PatchWithRaw | ShowDiffFormat::PatchWithRawSummary => {
                diff_commands::print_combined_diff_tree_raw_entries(
                    store,
                    &parent_indexes,
                    &result_index,
                    pathspecs,
                    None,
                    None,
                    nul_terminated,
                )?;
                if matches!(format, ShowDiffFormat::PatchWithRawSummary) {
                    diff_commands::print_combined_diff_tree_summary(
                        &parent_indexes,
                        &result_index,
                        pathspecs,
                        None,
                    )?;
                }
                println!();
                diff_commands::print_combined_diff_tree_patches(
                    store,
                    &parent_indexes,
                    &result_index,
                    pathspecs,
                    diff_commands::CombinedPatchRenderOptions {
                        abbrev_len: None,
                        relative_prefix: None,
                        old_prefix: "a/",
                        new_prefix: "b/",
                        dense_combined,
                        line_prefix: None,
                    },
                )?;
            }
            ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary => {
                diff_commands::print_combined_diff_tree_stat(
                    repo,
                    store,
                    &parent_indexes,
                    &result_index,
                    pathspecs,
                    diff_commands::CombinedStatRenderOptions {
                        relative_prefix: None,
                        whitespace_mode: DiffWhitespaceMode::None,
                        ignore_matching_lines,
                        ignore_blank_lines: false,
                        shortstat: false,
                    },
                )?;
                if matches!(format, ShowDiffFormat::PatchWithStatSummary) {
                    diff_commands::print_combined_diff_tree_summary(
                        &parent_indexes,
                        &result_index,
                        pathspecs,
                        None,
                    )?;
                }
                println!();
                diff_commands::print_combined_diff_tree_patches(
                    store,
                    &parent_indexes,
                    &result_index,
                    pathspecs,
                    diff_commands::CombinedPatchRenderOptions {
                        abbrev_len: None,
                        relative_prefix: None,
                        old_prefix: "a/",
                        new_prefix: "b/",
                        dense_combined,
                        line_prefix: None,
                    },
                )?;
            }
            ShowDiffFormat::Stat | ShowDiffFormat::StatSummary => {
                diff_commands::print_combined_diff_tree_stat(
                    repo,
                    store,
                    &parent_indexes,
                    &result_index,
                    pathspecs,
                    diff_commands::CombinedStatRenderOptions {
                        relative_prefix: None,
                        whitespace_mode: DiffWhitespaceMode::None,
                        ignore_matching_lines,
                        ignore_blank_lines: false,
                        shortstat: false,
                    },
                )?;
                if matches!(format, ShowDiffFormat::StatSummary) {
                    diff_commands::print_combined_diff_tree_summary(
                        &parent_indexes,
                        &result_index,
                        pathspecs,
                        None,
                    )?;
                }
            }
            ShowDiffFormat::Shortstat => {
                diff_commands::print_combined_diff_tree_stat(
                    repo,
                    store,
                    &parent_indexes,
                    &result_index,
                    pathspecs,
                    diff_commands::CombinedStatRenderOptions {
                        relative_prefix: None,
                        whitespace_mode: DiffWhitespaceMode::None,
                        ignore_matching_lines,
                        ignore_blank_lines: false,
                        shortstat: true,
                    },
                )?;
            }
            ShowDiffFormat::Summary => {
                diff_commands::print_combined_diff_tree_summary(
                    &parent_indexes,
                    &result_index,
                    &pathspecs,
                    None,
                )?;
            }
            ShowDiffFormat::Numstat
            | ShowDiffFormat::Raw
            | ShowDiffFormat::NameOnly
            | ShowDiffFormat::NameStatus => {}
        }
        return Ok(());
    }
    let old_index = if let Some(parent) = commit.parents.first() {
        let parent_commit = commit_cache.read_commit(parent)?;
        tree_cache.read_tree_to_index(&parent_commit.tree)?
    } else if include_root_diff {
        GitIndex::new()
    } else {
        return Ok(());
    };
    let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
    show_diff_between_indexes(
        repo,
        store,
        &old_index,
        &new_index,
        format,
        pickaxe_options,
        ignore_matching_lines,
        pathspecs,
        nul_terminated,
    )
}

fn show_commit_diff_against_parent(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    format: ShowDiffFormat,
    parent_index: usize,
    pickaxe_options: PickaxeOptions<'_>,
    ignore_matching_lines: &[Regex],
    pathspecs: &[Vec<u8>],
    nul_terminated: bool,
) -> Result<()> {
    let parent_id = commit
        .parents
        .get(parent_index)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "merge parent index out of range".into(),
        })?;
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let parent = commit_cache.read_commit(parent_id)?;
    let old_index = tree_cache.read_tree_to_index(&parent.tree)?;
    let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
    show_diff_between_indexes(
        repo,
        store,
        &old_index,
        &new_index,
        format,
        pickaxe_options,
        ignore_matching_lines,
        pathspecs,
        nul_terminated,
    )
}

fn show_diff_between_indexes(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    format: ShowDiffFormat,
    pickaxe_options: PickaxeOptions<'_>,
    ignore_matching_lines: &[Regex],
    pathspecs: &[Vec<u8>],
    nul_terminated: bool,
) -> Result<()> {
    let entries = diff_indexes(&old_index, &new_index)?
        .into_iter()
        .filter(|entry| diff_entry_matches_pathspec(entry, pathspecs))
        .collect::<Vec<_>>();
    let context = DiffIndexContext {
        repo,
        store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let entries = apply_pickaxe_filter(&context, entries, pickaxe_options)?;
    let stat_options = DiffStatOptions {
        whitespace_mode: DiffWhitespaceMode::None,
        relative_prefix: None,
        ignore_matching_lines,
        ignore_blank_lines: false,
        compact_summary: false,
    };
    match format {
        ShowDiffFormat::Patch => print_patch_entries(
            repo,
            store,
            &old_index,
            &new_index,
            &entries,
            PatchFormatOptions::cached().with_ignore_matching_lines(ignore_matching_lines.to_vec()),
        ),
        ShowDiffFormat::PatchWithRaw | ShowDiffFormat::PatchWithRawSummary => {
            print_raw_entries(
                &context,
                &entries,
                RawPrintOptions {
                    abbrev_len: None,
                    relative_prefix: None,
                    nul_terminated,
                },
            )?;
            if matches!(format, ShowDiffFormat::PatchWithRawSummary) {
                print_summary_entries(&old_index, &new_index, &entries, None)?;
            }
            if !entries.is_empty() {
                println!();
            }
            print_patch_entries(
                repo,
                store,
                &old_index,
                &new_index,
                &entries,
                PatchFormatOptions::cached()
                    .with_ignore_matching_lines(ignore_matching_lines.to_vec()),
            )
        }
        ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary => {
            print_stat_entries(&context, &entries, stat_options)?;
            if matches!(format, ShowDiffFormat::PatchWithStatSummary) {
                print_summary_entries(&old_index, &new_index, &entries, None)?;
            }
            if !entries.is_empty() {
                println!();
            }
            print_patch_entries(
                repo,
                store,
                &old_index,
                &new_index,
                &entries,
                PatchFormatOptions::cached()
                    .with_ignore_matching_lines(ignore_matching_lines.to_vec()),
            )
        }
        ShowDiffFormat::Stat | ShowDiffFormat::StatSummary => {
            print_stat_entries(&context, &entries, stat_options)?;
            if matches!(format, ShowDiffFormat::StatSummary) {
                print_summary_entries(&old_index, &new_index, &entries, None)?;
            }
            Ok(())
        }
        ShowDiffFormat::Numstat => print_numstat_entries(
            &context,
            &entries,
            NumstatOptions {
                stat: stat_options,
                nul_terminated,
            },
        ),
        ShowDiffFormat::Shortstat => print_shortstat_entries(&context, &entries, stat_options),
        ShowDiffFormat::Raw => print_raw_entries(
            &context,
            &entries,
            RawPrintOptions {
                abbrev_len: None,
                relative_prefix: None,
                nul_terminated,
            },
        ),
        ShowDiffFormat::Summary => print_summary_entries(&old_index, &new_index, &entries, None),
        ShowDiffFormat::NameOnly => print_name_only_entries(&entries, None, nul_terminated),
        ShowDiffFormat::NameStatus => print_name_status_entries(&entries, None, nul_terminated),
    }
}

fn empty_pickaxe_options() -> PickaxeOptions<'static> {
    PickaxeOptions {
        string: None,
        regex: None,
        regex_mode: false,
        all: false,
    }
}

fn show_tag_object(
    store: &LooseObjectStore,
    content: &[u8],
    options: ShowOptions<'_>,
    show_root: bool,
) -> Result<()> {
    let tag = decode_tag(GitHashAlgorithm::Sha1, content)?;
    let mut out = io::stdout().lock();
    out.write_all(b"tag ")?;
    out.write_all(&tag.name)?;
    out.write_all(b"\nTagger: ")?;
    out.write_all(signature_without_timestamp(&tag.tagger))?;
    if options.format != Some("raw") {
        writeln!(out)?;
        writeln!(out, "Date:   {}", signature_log_date(&tag.tagger)?)?;
        out.write_all(b"\n")?;
    } else {
        out.write_all(b"\n\n")?;
    }
    out.write_all(&tag.message)?;
    if !tag.message.ends_with(b"\n\n") {
        out.write_all(b"\n")?;
    }
    drop(out);

    let target = store.read_object(&tag.target)?;
    show_object(store, &tag.target.to_hex(), &target, options, show_root)
}

fn write_indented_message(out: &mut impl Write, message: &[u8]) -> Result<()> {
    if message.is_empty() {
        return Ok(());
    }
    for line in message.split_inclusive(|byte| *byte == b'\n') {
        out.write_all(b"    ")?;
        out.write_all(line)?;
    }
    if !message.ends_with(b"\n") {
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn signature_without_timestamp(signature: &[u8]) -> &[u8] {
    signature
        .iter()
        .rposition(|byte| *byte == b'>')
        .map(|idx| &signature[..=idx])
        .unwrap_or(signature)
}

pub(crate) struct RevListOptions {
    pub(crate) all: bool,
    pub(crate) count: bool,
    pub(crate) objects: bool,
    pub(crate) no_object_names: bool,
    pub(crate) filter: Option<String>,
    pub(crate) filter_provided_objects: bool,
    pub(crate) parents: bool,
    pub(crate) children: bool,
    pub(crate) reverse: bool,
    pub(crate) max_count: Option<usize>,
    pub(crate) revs: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum RevListObjectFilter {
    BlobNone,
    BlobLimit(usize),
    ObjectType(GitObjectKind),
}

pub(crate) fn rev_list(options: RevListOptions) -> Result<()> {
    let RevListOptions {
        all,
        count,
        objects,
        no_object_names,
        filter,
        filter_provided_objects,
        parents,
        children,
        reverse,
        max_count,
        revs,
    } = options;
    let revs = revs
        .into_iter()
        .take_while(|rev| rev != "--")
        .collect::<Vec<_>>();
    let object_filter = filter.as_deref().map(parse_rev_list_filter).transpose()?;
    let _ = filter_provided_objects;
    if revs.is_empty() && !all {
        return Err(CliError::Message("`rev-list` requires a revision".into()));
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let revs = collect_rev_list_revs(&repo, &store, all, revs)?;
    if objects && no_object_names && object_filter.is_some() {
        let filter = object_filter.expect("checked filter");
        let excluded_commits = collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?;
        let mut commit_trees =
            collect_commit_trees_with_exclusions_uncached(&repo, &store, &revs, max_count)?;
        if reverse {
            commit_trees.reverse();
        }
        let extra_object_ids = revs
            .extra_objects
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut out = io::stdout().lock();
        let mut count_value = 0usize;
        for commit in &commit_trees {
            if rev_list_filter_includes(&store, &commit.id, filter)? {
                count_value += 1;
                if !count {
                    writeln!(out, "{}", commit.id)?;
                }
            }
        }
        let visited = for_each_rev_list_filtered_object_id(
            &store,
            &commit_trees,
            &extra_object_ids,
            &excluded_commits,
            filter,
            |id| {
                count_value += 1;
                if !count {
                    writeln!(out, "{id}")?;
                }
                Ok(())
            },
        )?;
        let _ = visited;
        if count {
            println!("{count_value}");
        }
        return Ok(());
    }
    if objects && (no_object_names || count) {
        let excluded_commits = collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?;
        let mut commit_trees =
            collect_commit_trees_with_exclusions_uncached(&repo, &store, &revs, max_count)?;
        if reverse {
            commit_trees.reverse();
        }
        if count {
            let object_count = count_rev_list_objects_uncached(
                &store,
                &commit_trees,
                &revs.extra_objects,
                &excluded_commits,
            )?;
            println!("{}", commit_trees.len() + object_count);
            return Ok(());
        }
        let extra_object_ids = revs
            .extra_objects
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut out = io::stdout().lock();
        for commit in &commit_trees {
            writeln!(out, "{}", commit.id)?;
        }
        write_rev_list_object_ids_uncached(
            &store,
            &commit_trees,
            &extra_object_ids,
            &excluded_commits,
            &mut out,
        )?;
        return Ok(());
    }
    if count && !objects {
        println!(
            "{}",
            count_commits_with_exclusions(&repo, &store, &revs, max_count)?
        );
        return Ok(());
    }

    if objects && !parents && !children {
        let excluded_commits = collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?;
        let mut commit_trees =
            collect_commit_trees_with_exclusions_uncached(&repo, &store, &revs, max_count)?;
        if reverse {
            commit_trees.reverse();
        }
        let mut out = io::stdout().lock();
        for commit in &commit_trees {
            writeln!(out, "{}", commit.id)?;
        }
        for_each_rev_list_object_line_with_trees(
            &store,
            &commit_trees,
            &revs.extra_objects,
            &excluded_commits,
            |id, name| {
                write!(out, "{id}")?;
                if let Some(name) = name {
                    write!(out, " {}", String::from_utf8_lossy(name))?;
                }
                writeln!(out)?;
                Ok(())
            },
        )?;
        return Ok(());
    }

    let mut commit_ids = collect_commits_with_exclusions(&repo, &store, &revs, max_count)?;
    if reverse {
        commit_ids.reverse();
    }
    let excluded_commits = if objects {
        collect_rev_list_excluded_commits(&repo, &store, &revs)?
    } else {
        Vec::new()
    };
    if count {
        let object_count =
            count_rev_list_objects(&store, &commit_ids, &revs.extra_objects, &excluded_commits)?;
        println!("{}", commit_ids.len() + object_count);
        return Ok(());
    }
    let children_by_commit = if children {
        collect_rev_list_children(&store, &commit_ids, reverse)?
    } else {
        HashMap::new()
    };
    let mut out = io::stdout().lock();
    for id in &commit_ids {
        if parents {
            let parents = read_commit_parents_uncached(&store, &id)?;
            write!(out, "{id}")?;
            for parent in parents {
                write!(out, " {parent}")?;
            }
            writeln!(out)?;
        } else if children {
            write!(out, "{id}")?;
            if let Some(children) = children_by_commit.get(id) {
                for child in children {
                    write!(out, " {child}")?;
                }
            }
            writeln!(out)?;
        } else {
            writeln!(out, "{id}")?;
        }
    }
    if objects {
        for_each_rev_list_object_line_with(
            &store,
            &commit_ids,
            &revs.extra_objects,
            &excluded_commits,
            |id, name| {
                write!(out, "{id}")?;
                if let Some(name) = name {
                    write!(out, " {}", String::from_utf8_lossy(name))?;
                }
                writeln!(out)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

fn collect_rev_list_children(
    store: &LooseObjectStore,
    commit_ids: &[ObjectId],
    reverse: bool,
) -> Result<HashMap<ObjectId, Vec<ObjectId>>> {
    let included = commit_ids.iter().cloned().collect::<HashSet<_>>();
    let mut children = HashMap::<ObjectId, Vec<ObjectId>>::new();
    if reverse {
        for child in commit_ids {
            collect_rev_list_child_edges(store, child, &included, &mut children)?;
        }
    } else {
        for child in commit_ids.iter().rev() {
            collect_rev_list_child_edges(store, child, &included, &mut children)?;
        }
    }
    Ok(children)
}

fn collect_rev_list_child_edges(
    store: &LooseObjectStore,
    child: &ObjectId,
    included: &HashSet<ObjectId>,
    children: &mut HashMap<ObjectId, Vec<ObjectId>>,
) -> Result<()> {
    for parent in read_commit_parents_uncached(store, child)? {
        if included.contains(&parent) {
            children.entry(parent).or_default().push(child.clone());
        }
    }
    Ok(())
}

fn parse_rev_list_filter(value: &str) -> Result<RevListObjectFilter> {
    if value == "blob:none" {
        return Ok(RevListObjectFilter::BlobNone);
    }
    if let Some(limit) = value.strip_prefix("blob:limit=") {
        return parse_rev_list_blob_limit_filter(limit);
    }
    if let Some(kind) = value.strip_prefix("object:type=") {
        let kind = match kind {
            "blob" => GitObjectKind::Blob,
            "commit" => GitObjectKind::Commit,
            "tag" => GitObjectKind::Tag,
            "tree" => GitObjectKind::Tree,
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("invalid filter-spec '{value}'"),
                });
            }
        };
        return Ok(RevListObjectFilter::ObjectType(kind));
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("invalid filter-spec '{value}'"),
    })
}

fn parse_rev_list_blob_limit_filter(value: &str) -> Result<RevListObjectFilter> {
    let (number, multiplier) = match value.as_bytes().last().copied() {
        Some(b'k') | Some(b'K') => (&value[..value.len() - 1], 1024usize),
        Some(b'm') | Some(b'M') => (&value[..value.len() - 1], 1024usize * 1024),
        Some(b'g') | Some(b'G') => (&value[..value.len() - 1], 1024usize * 1024 * 1024),
        _ => (value, 1usize),
    };
    let parsed = number.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid filter-spec 'blob:limit={value}'"),
    })?;
    let limit = parsed
        .checked_mul(multiplier)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("invalid filter-spec 'blob:limit={value}'"),
        })?;
    Ok(RevListObjectFilter::BlobLimit(limit))
}

fn rev_list_filter_includes(
    store: &LooseObjectStore,
    id: &ObjectId,
    filter: RevListObjectFilter,
) -> Result<bool> {
    let Some((kind, size)) = store.object_header_hint(id)? else {
        return Ok(false);
    };
    Ok(match filter {
        RevListObjectFilter::BlobNone => kind != GitObjectKind::Blob,
        RevListObjectFilter::BlobLimit(limit) => kind != GitObjectKind::Blob || size <= limit,
        RevListObjectFilter::ObjectType(expected) => kind == expected,
    })
}

fn for_each_rev_list_filtered_object_id<F>(
    store: &LooseObjectStore,
    commits: &[CollectedCommitTree],
    extra_objects: &[ObjectId],
    excluded_commits: &[ObjectId],
    filter: RevListObjectFilter,
    mut visit: F,
) -> Result<usize>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    let mut count = 0usize;
    write_rev_list_object_ids_uncached_filtered(
        store,
        commits,
        extra_objects,
        excluded_commits,
        filter,
        |id| {
            count += 1;
            visit(id)
        },
    )?;
    Ok(count)
}

fn write_rev_list_object_ids_uncached_filtered<F>(
    store: &LooseObjectStore,
    commits: &[CollectedCommitTree],
    extra_objects: &[ObjectId],
    excluded_commits: &[ObjectId],
    filter: RevListObjectFilter,
    mut visit: F,
) -> Result<()>
where
    F: FnMut(&ObjectId) -> Result<()>,
{
    let mut out = Vec::new();
    write_rev_list_object_ids_uncached(store, commits, extra_objects, excluded_commits, &mut out)?;
    for line in out.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(line).map_err(|error| {
            CliError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                error.to_string(),
            ))
        })?;
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, text).map_err(CliError::Io)?;
        if rev_list_filter_includes(store, &id, filter)? {
            visit(&id)?;
        }
    }
    Ok(())
}

pub(crate) fn last_modified(
    recursive: bool,
    show_trees: bool,
    max_depth: Option<i32>,
    nul_terminated: bool,
    args: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let (revs, paths) = split_last_modified_args(&repo, &store, args)?;
    let revs = collect_rev_list_revs(&repo, &store, false, revs)?;
    let commit_cache = CommitObjectCache::new(&store);
    let commits =
        collect_commit_objects_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?;
    let head_rev = revs
        .include
        .first()
        .cloned()
        .unwrap_or_else(|| "HEAD".to_owned());
    let tree_cache = TreeObjectCache::new(&store);
    let head_index = read_treeish_index_cached(&repo, &store, &tree_cache, &head_rev)?;
    let depth = if recursive {
        -1
    } else {
        max_depth.unwrap_or(0)
    };
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let target_paths = last_modified_targets(&head_index, &pathspecs, depth, show_trees);
    let mut owners = BTreeMap::<Vec<u8>, ObjectId>::new();

    for commit_entry in commits {
        let commit = commit_entry.commit.as_ref();
        let current_index = read_commit_tree_index_cached(&tree_cache, commit)?;
        let parent_index = if let Some(parent) = commit.parents.first() {
            let parent = commit_cache.read_commit(parent)?;
            read_commit_tree_index_cached(&tree_cache, &parent)?
        } else {
            GitIndex::new()
        };
        for entry in diff_indexes(&parent_index, &current_index)? {
            for target in last_modified_impacted_targets(&entry, &target_paths) {
                owners
                    .entry(target)
                    .or_insert_with(|| commit_entry.id.clone());
            }
        }
        if owners.len() == target_paths.len() {
            break;
        }
    }

    for path in target_paths {
        let Some(owner) = owners.get(&path) else {
            continue;
        };
        if nul_terminated {
            print!("{}\t{}", owner.to_hex(), String::from_utf8_lossy(&path));
            io::stdout().write_all(&[0])?;
        } else {
            println!("{}\t{}", owner.to_hex(), String::from_utf8_lossy(&path));
        }
    }
    Ok(())
}

fn split_last_modified_args(
    repo: &GitRepo,
    store: &LooseObjectStore,
    args: Vec<String>,
) -> Result<(Vec<String>, Vec<PathBuf>)> {
    let mut before_dashdash = Vec::new();
    let mut paths = Vec::new();
    let mut after_dashdash = false;
    for arg in args {
        if after_dashdash {
            paths.push(PathBuf::from(arg));
        } else if arg == "--" {
            after_dashdash = true;
        } else {
            before_dashdash.push(arg);
        }
    }
    if after_dashdash {
        return Ok((last_modified_revs_or_head(before_dashdash), paths));
    }
    let mut revs = Vec::new();
    let mut split_at = before_dashdash.len();
    for (idx, arg) in before_dashdash.iter().enumerate() {
        if arg.contains("..") || resolve_commitish(repo, store, arg).is_ok() {
            revs.push(arg.clone());
        } else {
            split_at = idx;
            break;
        }
    }
    paths.extend(
        before_dashdash
            .into_iter()
            .skip(split_at)
            .map(PathBuf::from),
    );
    Ok((last_modified_revs_or_head(revs), paths))
}

fn last_modified_revs_or_head(revs: Vec<String>) -> Vec<String> {
    if revs.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        revs
    }
}

fn last_modified_targets(
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
    max_depth: i32,
    show_trees: bool,
) -> Vec<Vec<u8>> {
    let mut targets = BTreeSet::new();
    for entry in index.entries() {
        if !pathspec_matches(&entry.path, pathspecs) {
            continue;
        }
        if max_depth < 0 {
            if show_trees {
                insert_parent_paths(&mut targets, &entry.path);
            }
            targets.insert(entry.path.to_vec());
        } else {
            let limited = path_limited_to_depth(&entry.path, max_depth as usize);
            targets.insert(limited);
        }
    }
    targets.into_iter().collect()
}

fn last_modified_impacted_targets(
    entry: &zmin_git_core::IndexDiffEntry,
    targets: &[Vec<u8>],
) -> Vec<Vec<u8>> {
    targets
        .iter()
        .filter(|target| {
            last_modified_path_impacts(diff_entry_old_path(entry), target)
                || last_modified_path_impacts(&entry.path, target)
        })
        .cloned()
        .collect()
}

fn last_modified_path_impacts(path: &[u8], target: &[u8]) -> bool {
    path == target
        || path
            .strip_prefix(target)
            .is_some_and(|rest| rest.first() == Some(&b'/'))
}

fn path_limited_to_depth(path: &[u8], max_depth: usize) -> Vec<u8> {
    let mut separators = 0;
    for (idx, byte) in path.iter().enumerate() {
        if *byte == b'/' {
            if separators == max_depth {
                return path[..idx].to_vec();
            }
            separators += 1;
        }
    }
    path.to_vec()
}

fn insert_parent_paths(targets: &mut BTreeSet<Vec<u8>>, path: &[u8]) {
    for (idx, byte) in path.iter().enumerate() {
        if *byte == b'/' {
            targets.insert(path[..idx].to_vec());
        }
    }
}

pub(crate) fn merge_base(is_ancestor: bool, octopus: bool, commits: Vec<String>) -> Result<()> {
    if is_ancestor && commits.len() != 2 {
        return Err(CliError::Fatal {
            code: 128,
            message: "--is-ancestor takes exactly two commits".into(),
        });
    }
    if commits.len() < 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "`merge-base` requires at least two commits".into(),
        });
    }

    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let commit_graph = CommitGraphIndex::open(&repo)?;
    let resolved = commits
        .iter()
        .map(|commit| {
            resolve_commitish_for_ancestor_check_with_graph_cached(
                &repo,
                &store,
                &commit_cache,
                commit_graph.as_ref(),
                commit,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let left = &resolved[0];
    let right = &resolved[1];

    if resolved.len() == 2 && left == right {
        if !is_ancestor {
            println!("{}", left.to_hex());
        }
        return Ok(());
    }

    if is_ancestor {
        if let Some(commit_graph) = commit_graph.as_ref() {
            if let Some(result) = commit_graph.is_ancestor(left, right)? {
                return if result {
                    Ok(())
                } else {
                    Err(CliError::Exit(1))
                };
            }
        }
        return if is_ancestor_commit_uncached(&store, left, right)? {
            Ok(())
        } else {
            Err(CliError::Exit(1))
        };
    }

    let base = if octopus {
        best_octopus_merge_base_cached(&commit_cache, &resolved)?
    } else if resolved.len() == 2 {
        best_merge_base_with_commit_graph_cached(commit_graph.as_ref(), &commit_cache, left, right)?
    } else {
        best_multi_merge_base_cached(&commit_cache, &resolved)?
    };
    let Some(base) = base else {
        return Err(CliError::Exit(1));
    };
    println!("{}", base.to_hex());
    Ok(())
}

pub(crate) struct FilterBranchOptions {
    pub(crate) force: bool,
    pub(crate) msg_filter: Option<String>,
    pub(crate) tree_filter: Option<String>,
    pub(crate) index_filter: Option<String>,
    pub(crate) env_filter: Option<String>,
    pub(crate) parent_filter: Option<String>,
    pub(crate) commit_filter: Option<String>,
    pub(crate) tag_name_filter: Option<String>,
    pub(crate) subdirectory_filter: Option<String>,
    pub(crate) original: Option<String>,
    pub(crate) temp_dir: Option<PathBuf>,
    pub(crate) setup: Option<String>,
    pub(crate) state_branch: Option<String>,
    pub(crate) revs: Vec<String>,
}

pub(crate) fn filter_branch(options: FilterBranchOptions) -> Result<()> {
    reject_unsupported_filter_branch_options(&options)?;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 128,
            message: "Cannot rewrite branches: You have unstaged changes.".into(),
        });
    }

    let (all, revs) = filter_branch_revs(options.revs);
    let revs = collect_rev_list_revs(&repo, &store, all, revs)?;
    let mut commits = collect_commits_with_exclusions(&repo, &store, &revs, None)?;
    commits.reverse();
    if commits.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "Found nothing to rewrite".into(),
        });
    }

    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let targets = filter_branch_target_refs(&repo, &refs, all, &commits)?;
    ensure_filter_branch_backups_available(
        &refs,
        &targets,
        options.original.as_deref(),
        options.force,
    )?;
    let temp_root = FilterBranchTempRoot::new(options.temp_dir.as_deref())?;
    let git_shim = if options.tree_filter.is_some()
        || options.index_filter.is_some()
        || options.commit_filter.is_some()
    {
        Some(FilterBranchGitShim::new(temp_root.path())?)
    } else {
        None
    };

    let state_commit = if let Some(state_branch) = options.state_branch.as_deref() {
        filter_branch_load_state(temp_root.path(), &repo, state_branch)?
    } else {
        None
    };
    let mut rewritten = filter_branch_read_map_dir(temp_root.path())?;
    let total = commits.len();
    for (idx, old_id) in commits.iter().enumerate() {
        let old_id_hex = old_id.to_hex();
        if rewritten.contains_key(&old_id_hex) {
            continue;
        }
        let commit = commit_cache.read_commit(old_id)?;
        let mut parents = commit
            .parents
            .iter()
            .flat_map(|parent| filter_branch_mapped_parent_ids(&rewritten, parent))
            .collect::<Vec<_>>();
        if let Some(filter) = options.parent_filter.as_deref() {
            parents = run_filter_branch_parent_filter(
                filter,
                options.setup.as_deref(),
                temp_root.path(),
                &parents,
            )?;
        }
        let mut tree = if let Some(filter) = options.tree_filter.as_deref() {
            checkout_worktree(&repo, &store, old_id)?;
            run_filter_branch_tree_filter(
                &repo,
                git_shim.as_ref(),
                options.setup.as_deref(),
                temp_root.path(),
                filter,
            )?;
            worktree_commands::add(
                true,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                None,
                false,
                None,
                false,
                Vec::new(),
            )?;
            let index = read_repo_index(&repo)?;
            write_tree_from_index(&store, &index)?
        } else {
            commit.tree.clone()
        };
        if let Some(filter) = options.index_filter.as_deref() {
            tree_cache
                .read_tree_to_index(&tree)?
                .write_to_path(&repo.index_path)?;
            run_filter_branch_index_filter(
                &repo,
                git_shim.as_ref(),
                options.setup.as_deref(),
                temp_root.path(),
                filter,
            )?;
            let index = read_repo_index(&repo)?;
            tree = write_tree_from_index(&store, &index)?;
        }
        if let Some(path) = options.subdirectory_filter.as_deref() {
            tree = filter_branch_subdirectory_tree(&store, &tree, path)?;
        }
        let message = if let Some(filter) = options.msg_filter.as_deref() {
            run_filter_branch_msg_filter(
                filter,
                options.setup.as_deref(),
                temp_root.path(),
                &commit.message,
            )?
        } else {
            commit.message.clone()
        };
        let (author, committer) = if let Some(filter) = options.env_filter.as_deref() {
            run_filter_branch_env_filter(
                filter,
                options.setup.as_deref(),
                temp_root.path(),
                &commit,
            )?
        } else {
            (commit.author.clone(), commit.committer.clone())
        };
        let rewritten_value = if let Some(filter) = options.commit_filter.as_deref() {
            run_filter_branch_commit_filter(FilterBranchCommitFilterContext {
                command: filter,
                setup: options.setup.as_deref(),
                temp_root: temp_root.path(),
                git_shim: git_shim.as_ref(),
                repo: &repo,
                commit_id: &old_id_hex,
                tree: &tree,
                parents: &parents,
                author: &author,
                committer: &committer,
                message: &message,
            })?
        } else {
            let encoded = encode_raw_commit(&tree, &parents, &author, &committer, &message)?;
            let new_id = store.write_object(GitObjectKind::Commit, &encoded)?;
            new_id.to_hex()
        };
        println!("Rewrite {} ({}/{})", old_id.to_hex(), idx + 1, total);
        filter_branch_record_map(temp_root.path(), &old_id_hex, &rewritten_value)?;
        rewritten.insert(old_id_hex, rewritten_value);
    }

    for (ref_name, old_id) in targets {
        let Some(rewritten_value) = rewritten.get(&old_id.to_hex()) else {
            continue;
        };
        let backup = filter_branch_backup_ref(options.original.as_deref(), &ref_name);
        refs.write_ref(&backup, &old_id)?;
        let target_ref_name = if let Some(filter) = options.tag_name_filter.as_deref() {
            filter_branch_tag_ref_name(
                filter,
                options.setup.as_deref(),
                temp_root.path(),
                &ref_name,
            )?
        } else {
            ref_name.clone()
        };
        if target_ref_name != ref_name && !ref_name.starts_with("refs/tags/") {
            refs.delete_ref(&ref_name)?;
        }
        let Some(new_id) = filter_branch_single_rewritten_id(rewritten_value, &target_ref_name)?
        else {
            if target_ref_name == "HEAD" {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "filter-branch deleted HEAD".into(),
                });
            }
            refs.delete_ref(&target_ref_name)?;
            println!("Ref '{target_ref_name}' was deleted");
            continue;
        };
        if target_ref_name == "HEAD" {
            refs.write_head_direct(&new_id)?;
        } else {
            refs.write_ref(&target_ref_name, &new_id)?;
        }
        println!("Ref '{target_ref_name}' was rewritten");
    }
    if let Some(state_branch) = options.state_branch.as_deref() {
        filter_branch_save_state(
            temp_root.path(),
            &repo,
            state_branch,
            state_commit.as_ref(),
            &rewritten,
        )?;
    }
    if options.tree_filter.is_some()
        && let Ok(head) = refs.resolve("HEAD")
    {
        checkout_worktree(&repo, &store, &head)?;
    }
    Ok(())
}

fn reject_unsupported_filter_branch_options(_options: &FilterBranchOptions) -> Result<()> {
    Ok(())
}

fn filter_branch_revs(args: Vec<String>) -> (bool, Vec<String>) {
    let mut all = false;
    let mut revs = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--" => {}
            "--all" => all = true,
            _ => revs.push(arg),
        }
    }
    if revs.is_empty() && !all {
        revs.push("HEAD".to_owned());
    }
    (all, revs)
}

fn filter_branch_target_refs(
    repo: &GitRepo,
    refs: &RefStore,
    all: bool,
    commits: &[ObjectId],
) -> Result<Vec<(String, ObjectId)>> {
    let rewritten = commits.iter().map(ObjectId::to_hex).collect::<HashSet<_>>();
    let mut targets = Vec::new();
    if all {
        refs.for_each_resolved_ref("refs/", |ref_name, id| {
            if rewritten.contains(&id.to_hex()) {
                targets.push((ref_name.to_owned(), id.clone()));
            }
            Ok::<(), CliError>(())
        })?;
        return Ok(targets);
    }

    let head = refs.resolve("HEAD")?;
    if !rewritten.contains(&head.to_hex()) {
        return Ok(targets);
    }
    if let Some(branch) = current_branch_ref(refs)? {
        targets.push((branch, head));
    } else {
        let _ = repo;
        targets.push(("HEAD".to_owned(), head));
    }
    Ok(targets)
}

fn ensure_filter_branch_backups_available(
    refs: &RefStore,
    targets: &[(String, ObjectId)],
    original: Option<&str>,
    force: bool,
) -> Result<()> {
    if force {
        return Ok(());
    }
    for (ref_name, _) in targets {
        let backup = filter_branch_backup_ref(original, ref_name);
        if ref_exists(refs, &backup)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "Cannot create a new backup. A previous backup already exists in {backup}"
                ),
            });
        }
    }
    Ok(())
}

fn filter_branch_backup_ref(original: Option<&str>, ref_name: &str) -> String {
    let mut namespace = original.unwrap_or("refs/original/").to_owned();
    if !namespace.ends_with('/') {
        namespace.push('/');
    }
    format!("{namespace}{ref_name}")
}

fn filter_branch_tag_ref_name(
    command: &str,
    setup: Option<&str>,
    temp_root: &Path,
    ref_name: &str,
) -> Result<String> {
    let Some(tag_name) = ref_name.strip_prefix("refs/tags/") else {
        return Ok(ref_name.to_owned());
    };
    let filtered = run_filter_branch_text_filter(command, setup, temp_root, tag_name.as_bytes())?;
    let filtered = String::from_utf8(filtered).map_err(|_| CliError::Fatal {
        code: 128,
        message: "tag-name filter emitted non-UTF-8 output".into(),
    })?;
    let filtered = filtered.trim_end_matches('\n').trim_end_matches('\r');
    if filtered.is_empty() || filtered.contains('/') {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("tag-name filter produced invalid tag name '{filtered}'"),
        });
    }
    Ok(format!("refs/tags/{filtered}"))
}

const FILTER_BRANCH_COMMIT_FUNCTIONS: &str = r#"
EMPTY_TREE=$(git hash-object -t tree /dev/null)

warn () {
    echo "$*" >&2
}

map() {
    if test -r "$workdir/../map/$1"
    then
        cat "$workdir/../map/$1"
    else
        echo "$1"
    fi
}

skip_commit() {
    shift
    while [ -n "$1" ];
    do
        shift
        map "$1"
        shift
    done
}

git_commit_non_empty_tree() {
    if test $# = 3 && test "$1" = $(git rev-parse "$3^{tree}"); then
        map "$3"
    elif test $# = 1 && test "$1" = $EMPTY_TREE; then
        :
    else
        git commit-tree "$@"
    fi
}
"#;

fn filter_branch_shell_script(setup: Option<&str>, command: &str) -> String {
    match setup {
        Some(setup) => format!("{setup}\n{command}"),
        None => command.to_owned(),
    }
}

fn run_filter_branch_text_filter(
    command: &str,
    setup: Option<&str>,
    temp_root: &Path,
    input: &[u8],
) -> Result<Vec<u8>> {
    let mut child = ProcessCommand::new("sh")
        .arg("-c")
        .arg(filter_branch_shell_script(setup, command))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .env("TMPDIR", temp_root)
        .env("TMP", temp_root)
        .env("TEMP", temp_root)
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "failed to open filter stdin".into(),
        })?;
        stdin.write_all(input)?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!("filter failed: {command}"),
        });
    }
    Ok(output.stdout)
}

fn run_filter_branch_msg_filter(
    command: &str,
    setup: Option<&str>,
    temp_root: &Path,
    message: &[u8],
) -> Result<Vec<u8>> {
    run_filter_branch_text_filter(command, setup, temp_root, message)
}

fn run_filter_branch_parent_filter(
    command: &str,
    setup: Option<&str>,
    temp_root: &Path,
    parents: &[ObjectId],
) -> Result<Vec<ObjectId>> {
    let input = parents
        .iter()
        .map(|parent| format!("-p {}", parent.to_hex()))
        .collect::<Vec<_>>()
        .join(" ");
    let mut child = ProcessCommand::new("sh")
        .arg("-c")
        .arg(filter_branch_shell_script(setup, command))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .env("TMPDIR", temp_root)
        .env("TMP", temp_root)
        .env("TEMP", temp_root)
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "failed to open parent-filter stdin".into(),
        })?;
        stdin.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!("parent filter failed: {command}"),
        });
    }
    parse_parent_filter_output(&output.stdout)
}

fn parse_parent_filter_output(output: &[u8]) -> Result<Vec<ObjectId>> {
    let text = std::str::from_utf8(output).map_err(|_| CliError::Fatal {
        code: 128,
        message: "parent filter emitted non-UTF-8 output".into(),
    })?;
    let mut parents = Vec::new();
    let mut parts = text.split_whitespace();
    while let Some(flag) = parts.next() {
        if flag != "-p" {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("parent filter emitted unsupported token '{flag}'"),
            });
        }
        let Some(parent) = parts.next() else {
            return Err(CliError::Fatal {
                code: 128,
                message: "parent filter emitted -p without object id".into(),
            });
        };
        parents.push(ObjectId::from_hex(GitHashAlgorithm::Sha1, parent).map_err(CliError::Io)?);
    }
    Ok(parents)
}

fn filter_branch_subdirectory_tree(
    store: &LooseObjectStore,
    tree: &ObjectId,
    path: &str,
) -> Result<ObjectId> {
    let path = path.trim().trim_matches('/');
    if path.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "subdirectory-filter requires a non-empty path".into(),
        });
    }
    let Some(entry) = find_tree_entry(store, tree, path.as_bytes())? else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("subdirectory-filter path '{path}' does not exist in every commit"),
        });
    };
    if entry.mode != TreeMode::Tree {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("subdirectory-filter path '{path}' is not a tree"),
        });
    }
    Ok(entry.id)
}

fn run_filter_branch_env_filter(
    command: &str,
    setup: Option<&str>,
    temp_root: &Path,
    commit: &zmin_git_core::CommitObject,
) -> Result<(Vec<u8>, Vec<u8>)> {
    const MARKER: &str = "__ZMIN_FILTER_BRANCH_ENV__";
    let author = signature_from_commit_bytes(&commit.author)?;
    let committer = signature_from_commit_bytes(&commit.committer)?;
    let script = format!(
        "{{ {}\n}}\nzmin_filter_status=$?\nprintf '\\n{MARKER}\\n'\nenv\nexit \"$zmin_filter_status\"",
        filter_branch_shell_script(setup, command)
    );
    let output = ProcessCommand::new("sh")
        .arg("-c")
        .arg(script)
        .env("GIT_AUTHOR_NAME", &author.name)
        .env("GIT_AUTHOR_EMAIL", &author.email)
        .env("GIT_AUTHOR_DATE", signature_env_date(&author))
        .env("GIT_COMMITTER_NAME", &committer.name)
        .env("GIT_COMMITTER_EMAIL", &committer.email)
        .env("GIT_COMMITTER_DATE", signature_env_date(&committer))
        .env("TMPDIR", temp_root)
        .env("TMP", temp_root)
        .env("TEMP", temp_root)
        .output()?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!("env filter failed: {command}"),
        });
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_| CliError::Fatal {
        code: 128,
        message: "env filter emitted non-UTF-8 environment".into(),
    })?;
    let Some((_, env_lines)) = stdout.rsplit_once(&format!("\n{MARKER}\n")) else {
        return Err(CliError::Fatal {
            code: 128,
            message: "env filter did not return environment".into(),
        });
    };
    let env = env_lines
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<HashMap<_, _>>();
    let author = signature_from_filter_env(
        &env,
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_DATE",
        &author,
    )?;
    let committer = signature_from_filter_env(
        &env,
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_DATE",
        &committer,
    )?;
    Ok((
        signature_to_commit_bytes(&author),
        signature_to_commit_bytes(&committer),
    ))
}

struct FilterBranchCommitFilterContext<'a> {
    command: &'a str,
    setup: Option<&'a str>,
    temp_root: &'a Path,
    git_shim: Option<&'a FilterBranchGitShim>,
    repo: &'a GitRepo,
    commit_id: &'a str,
    tree: &'a ObjectId,
    parents: &'a [ObjectId],
    author: &'a [u8],
    committer: &'a [u8],
    message: &'a [u8],
}

fn run_filter_branch_commit_filter(context: FilterBranchCommitFilterContext<'_>) -> Result<String> {
    let FilterBranchCommitFilterContext {
        command,
        setup,
        temp_root,
        git_shim,
        repo,
        commit_id,
        tree,
        parents,
        author,
        committer,
        message,
    } = context;
    let author = signature_from_commit_bytes(author)?;
    let committer = signature_from_commit_bytes(committer)?;
    let mut process = ProcessCommand::new("sh");
    process
        .arg("-c")
        .arg(format!(
            "{}\n{}",
            FILTER_BRANCH_COMMIT_FUNCTIONS,
            filter_branch_shell_script(setup, command)
        ))
        .arg("git commit-tree")
        .arg(tree.to_hex())
        .current_dir(temp_root.join("t"))
        .env("GIT_AUTHOR_NAME", &author.name)
        .env("GIT_AUTHOR_EMAIL", &author.email)
        .env(
            "GIT_AUTHOR_DATE",
            format!("@{} {}", author.timestamp, author.timezone),
        )
        .env("GIT_COMMITTER_NAME", &committer.name)
        .env("GIT_COMMITTER_EMAIL", &committer.email)
        .env(
            "GIT_COMMITTER_DATE",
            format!("@{} {}", committer.timestamp, committer.timezone),
        )
        .env("GIT_COMMIT", commit_id)
        .env("GIT_DIR", &repo.git_dir)
        .env("GIT_WORK_TREE", ".")
        .env("GIT_INDEX_FILE", &repo.index_path)
        .env("workdir", temp_root.join("t"))
        .env("TMPDIR", temp_root)
        .env("TMP", temp_root)
        .env("TEMP", temp_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(shim) = git_shim {
        process.env("PATH", shim.path_value());
    }
    for parent in parents {
        process.arg("-p").arg(parent.to_hex());
    }
    let mut child = process.spawn()?;
    {
        let stdin = child.stdin.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "failed to open commit-filter stdin".into(),
        })?;
        stdin.write_all(message)?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: "could not write rewritten commit".into(),
        });
    }
    let rewritten = String::from_utf8(output.stdout).map_err(|_| CliError::Fatal {
        code: 128,
        message: "commit-filter emitted non-UTF-8 output".into(),
    })?;
    Ok(rewritten.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn filter_branch_record_map(temp_root: &Path, commit_id: &str, rewritten: &str) -> Result<()> {
    fs::write(temp_root.join("map").join(commit_id), rewritten).map_err(CliError::Io)
}

fn filter_branch_read_map_dir(temp_root: &Path) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for entry in fs::read_dir(temp_root.join("map"))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let value = fs::read_to_string(&path)?.trim().to_owned();
        map.insert(name, value);
    }
    Ok(map)
}

fn filter_branch_load_state(
    temp_root: &Path,
    repo: &GitRepo,
    state_branch: &str,
) -> Result<Option<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let state_commit = match refs.resolve(state_branch) {
        Ok(id) => {
            eprintln!("Populating map from {state_branch} ({})", id.to_hex());
            id
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!("Branch {state_branch} does not exist. Will create");
            return Ok(None);
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit = decode_commit(
        GitHashAlgorithm::Sha1,
        &store.read_object(&state_commit)?.content,
    )?;
    let entry =
        find_tree_entry(&store, &commit.tree, b"filter.map")?.ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("Unable to load state from {state_branch}:filter.map"),
        })?;
    let blob = store.read_object(&entry.id)?;
    let raw = String::from_utf8(blob.content).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("Unable to load state from {state_branch}:filter.map"),
    })?;
    for line in raw.lines() {
        let Some((from, to)) = line.split_once(':') else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("Unable to load state from {state_branch}:filter.map"),
            });
        };
        filter_branch_record_map(temp_root, to.trim(), from.trim())?;
    }
    Ok(Some(state_commit))
}

fn filter_branch_save_state(
    _temp_root: &Path,
    repo: &GitRepo,
    state_branch: &str,
    state_commit: Option<&ObjectId>,
    rewritten: &HashMap<String, String>,
) -> Result<()> {
    eprintln!("Saving rewrite state to {state_branch}");
    #[cfg(windows)]
    {
        let _ = (repo, state_commit, rewritten);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let mut lines = rewritten
            .iter()
            .map(|(from, to)| format!("{from}:{to}"))
            .collect::<Vec<_>>();
        lines.sort();
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        let state_blob = store.write_object(GitObjectKind::Blob, lines.join("\n").as_bytes())?;
        let tree_content = encode_tree(&[TreeEntry {
            mode: TreeMode::File,
            name: b"filter.map".to_vec(),
            id: state_blob,
        }])?;
        let state_tree = store.write_object(GitObjectKind::Tree, &tree_content)?;
        let author = signature_from_identity(repo, "GIT_AUTHOR")?;
        let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
        let mut builder = CommitBuilder::new(state_tree, author, committer);
        if let Some(parent) = state_commit {
            builder = builder.parent(parent.clone());
        }
        let commit = builder.message(b"Sync\n".to_vec())?.encode()?;
        let state_commit = store.write_object(GitObjectKind::Commit, &commit)?;
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref(state_branch, &state_commit)?;
        Ok(())
    }
}

fn filter_branch_mapped_parent_ids(
    rewritten: &HashMap<String, String>,
    parent: &ObjectId,
) -> Vec<ObjectId> {
    let parent_hex = parent.to_hex();
    rewritten
        .get(&parent_hex)
        .map(String::as_str)
        .unwrap_or(parent_hex.as_str())
        .split_whitespace()
        .filter_map(|value| ObjectId::from_hex(GitHashAlgorithm::Sha1, value).ok())
        .collect()
}

fn filter_branch_single_rewritten_id(rewritten: &str, ref_name: &str) -> Result<Option<ObjectId>> {
    let mut values = rewritten.split_whitespace();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "filter-branch produced multiple rewritten commits for ref '{ref_name}'"
            ),
        });
    }
    ObjectId::from_hex(GitHashAlgorithm::Sha1, first)
        .map(Some)
        .map_err(CliError::Io)
}

fn signature_from_filter_env(
    env: &HashMap<&str, &str>,
    name_key: &str,
    email_key: &str,
    date_key: &str,
    fallback: &Signature,
) -> Result<Signature> {
    let name = env.get(name_key).copied().unwrap_or(&fallback.name);
    let email = env.get(email_key).copied().unwrap_or(&fallback.email);
    let date = env
        .get(date_key)
        .copied()
        .map(str::to_owned)
        .unwrap_or_else(|| signature_env_date(fallback));
    let (timestamp, timezone) = parse_git_date(&date)?;
    Ok(Signature::new(name, email, timestamp, timezone)?)
}

fn signature_env_date(signature: &Signature) -> String {
    format!("{} {}", signature.timestamp, signature.timezone)
}

fn signature_to_commit_bytes(signature: &Signature) -> Vec<u8> {
    format!(
        "{} <{}> {} {}",
        signature.name, signature.email, signature.timestamp, signature.timezone
    )
    .into_bytes()
}

fn run_filter_branch_tree_filter(
    repo: &GitRepo,
    git_shim: Option<&FilterBranchGitShim>,
    setup: Option<&str>,
    temp_root: &Path,
    command: &str,
) -> Result<()> {
    let status = filter_branch_shell(repo, git_shim, setup, temp_root, command).status()?;
    if !status.success() {
        return Err(CliError::Fatal {
            code: status.code().unwrap_or(1),
            message: format!("tree filter failed: {command}"),
        });
    }
    Ok(())
}

fn run_filter_branch_index_filter(
    repo: &GitRepo,
    git_shim: Option<&FilterBranchGitShim>,
    setup: Option<&str>,
    temp_root: &Path,
    command: &str,
) -> Result<()> {
    let status = filter_branch_shell(repo, git_shim, setup, temp_root, command).status()?;
    if !status.success() {
        return Err(CliError::Fatal {
            code: status.code().unwrap_or(1),
            message: format!("index filter failed: {command}"),
        });
    }
    Ok(())
}

fn filter_branch_shell(
    repo: &GitRepo,
    git_shim: Option<&FilterBranchGitShim>,
    setup: Option<&str>,
    temp_root: &Path,
    command: &str,
) -> ProcessCommand {
    let mut process = ProcessCommand::new("sh");
    process
        .arg("-c")
        .arg(filter_branch_shell_script(setup, command))
        .current_dir(&repo.root)
        .env("TMPDIR", temp_root)
        .env("TMP", temp_root)
        .env("TEMP", temp_root);
    if let Some(shim) = git_shim {
        process.env("PATH", shim.path_value());
    }
    process
}

struct FilterBranchTempRoot {
    path: PathBuf,
    cleanup: bool,
}

impl FilterBranchTempRoot {
    fn new(path: Option<&Path>) -> Result<Self> {
        let (path, cleanup) = match path {
            Some(path) => (absolute_path_from_arg(path)?, true),
            None => (
                unique_temp_sibling(&std::env::temp_dir().join("zmin-filter-branch")),
                true,
            ),
        };
        remove_path_if_exists(&path)?;
        fs::create_dir_all(&path)?;
        fs::create_dir_all(path.join("t"))?;
        fs::create_dir_all(path.join("map"))?;
        Ok(Self { path, cleanup })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FilterBranchTempRoot {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct FilterBranchGitShim {
    dir: PathBuf,
    path_value: std::ffi::OsString,
}

impl FilterBranchGitShim {
    fn new(temp_root: &Path) -> Result<Self> {
        let dir = unique_temp_sibling(&temp_root.join("zmin-filter-branch"));
        fs::create_dir(&dir)?;
        let target = if cfg!(windows) {
            dir.join("git.exe")
        } else {
            dir.join("git")
        };
        install_current_exe_alias(&target)?;
        let mut paths = vec![dir.clone()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        let path_value = std::env::join_paths(paths).map_err(|error| {
            CliError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid PATH while preparing filter-branch shim: {error}"),
            ))
        })?;
        Ok(Self { dir, path_value })
    }

    fn path_value(&self) -> &std::ffi::OsStr {
        &self.path_value
    }
}

impl Drop for FilterBranchGitShim {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[cfg(unix)]
fn install_current_exe_alias(target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(std::env::current_exe()?, target)?;
    Ok(())
}

#[cfg(not(unix))]
fn install_current_exe_alias(target: &Path) -> Result<()> {
    fs::copy(std::env::current_exe()?, target)?;
    Ok(())
}

fn encode_raw_commit(
    tree: &ObjectId,
    parents: &[ObjectId],
    author: &[u8],
    committer: &[u8],
    message: &[u8],
) -> Result<Vec<u8>> {
    if author.contains(&0) || committer.contains(&0) || message.contains(&0) {
        return Err(CliError::Fatal {
            code: 128,
            message: "commit data contains NUL".into(),
        });
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"tree ");
    out.extend_from_slice(tree.to_hex().as_bytes());
    out.push(b'\n');
    for parent in parents {
        out.extend_from_slice(b"parent ");
        out.extend_from_slice(parent.to_hex().as_bytes());
        out.push(b'\n');
    }
    out.extend_from_slice(b"author ");
    out.extend_from_slice(author);
    out.push(b'\n');
    out.extend_from_slice(b"committer ");
    out.extend_from_slice(committer);
    out.extend_from_slice(b"\n\n");
    out.extend_from_slice(message);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;
    use zmin_git_core::{CommitBuilder, GitObjectSink, Signature, encode_tree};

    #[test]
    fn reversed_reflog_reader_streams_lines_from_newest_to_oldest() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("HEAD");
        let long_message = "x".repeat(REFLOG_REVERSE_READ_CHUNK_SIZE + 17);
        let lines = [
            "1111111111111111111111111111111111111111 2222222222222222222222222222222222222222 user <u@example.com> 1 +0000\told".to_owned(),
            format!(
                "2222222222222222222222222222222222222222 3333333333333333333333333333333333333333 user <u@example.com> 2 +0000\t{long_message}"
            ),
            "3333333333333333333333333333333333333333 4444444444444444444444444444444444444444 user <u@example.com> 3 +0000\tnew".to_owned(),
        ];
        fs::write(&path, format!("{}\n{}\n{}\n", lines[0], lines[1], lines[2]))
            .expect("write reflog");

        let mut actual = Vec::new();
        let file = fs::File::open(&path).expect("open reflog");
        for_each_reflog_line_rev(file, |line| {
            actual.push(line.to_owned());
            Ok(())
        })
        .expect("read reflog");

        assert_eq!(
            actual,
            vec![lines[2].clone(), lines[1].clone(), lines[0].clone()]
        );
    }

    #[test]
    fn parse_reflog_entry_reads_trailing_timestamp_without_collecting_identity_fields() {
        let entry = parse_reflog_entry(
            "1111111111111111111111111111111111111111 \
             2222222222222222222222222222222222222222 \
             Jane Q Developer <jane@example.com> 123 +0300\tcommit: message",
        )
        .expect("reflog entry");

        assert_eq!(
            entry.new_id.to_hex(),
            "2222222222222222222222222222222222222222"
        );
        assert_eq!(entry.timestamp, 123);
        assert_eq!(entry.timezone, "+0300");
        assert_eq!(entry.message, "commit: message");
    }

    #[test]
    fn show_branch_heads_uses_loose_ref_over_stale_packed_ref() {
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
        let stale = write_history_test_commit(&store, &tree, &[], 1, "stale");
        let live = write_history_test_commit(&store, &tree, &[], 2, "live");
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/main\n", stale.to_hex()),
        )
        .expect("write packed refs");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/main", &live)
            .expect("write loose ref");
        refs.write_head_symbolic("refs/heads/main")
            .expect("write HEAD");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir,
            objects_dir,
            index_path: dir.path().join(".git/index"),
        };

        let heads = show_branch_heads(&repo, &store, &refs, false, false, false, Vec::new())
            .expect("heads");

        assert_eq!(heads.len(), 1);
        assert_eq!(heads[0].id, live);
        assert!(heads[0].current);
    }

    fn write_history_test_commit(
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
