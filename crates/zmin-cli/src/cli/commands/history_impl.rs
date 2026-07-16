use super::patch_commands::{
    ApplyFilePatch, PatchAnswers, apply_hunks_to_content, parse_apply_patches, select_patch_hunks,
};
use super::sequencer_commands::apply_tree_delta;
use super::*;
use crate::runtime::{DiffColorMode, parse_diff_color_option};
use chrono::Datelike;
use similar::{Algorithm, DiffTag, capture_diff_slices};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::sync::Arc;

const REFLOG_REVERSE_READ_CHUNK_SIZE: usize = 16 * 1024;
const COMMIT_AUTHOR_METADATA_PREFIX_BYTES: usize = 1024;
const COMMIT_AUTHOR_HINT_PREFIX_BYTES: usize = 256;
const COMMIT_AUTHOR_HEADER_PREFIX_BYTES: usize = 128;
const PARALLEL_LOG_RENDER_METADATA_MIN_COMMITS: usize = 1024;
const PARALLEL_LOG_AUTHOR_METADATA_MIN_COMMITS: usize = 1024;
const PARALLEL_LOG_METADATA_MAX_WORKERS: usize = 8;
const PARALLEL_LOG_AUTHOR_METADATA_MAX_WORKERS: usize = 3;
const PARALLEL_LOG_METADATA_STACK_BYTES: usize = 128 * 1024;
const REV_LIST_OUTPUT_BUFFER_BYTES: usize = 64 * 1024;
const REV_LIST_PACK_OBJECT_CACHE_BYTES: usize = 128 * 1024;
const LOG_PACK_OBJECT_CACHE_BYTES: usize = 4 * 1024;
const LOG_PACK_FILE_READER_BUFFER_BYTES: usize = 512;

const BLAME_USAGE: &str = r#"usage: git blame [<options>] [<rev-opts>] [<rev>] [--] <file>

    <rev-opts> are documented in git-rev-list(1)

    --[no-]incremental    show blame entries as we find them, incrementally
    -b                    do not show object names of boundary commits (Default: off)
    --[no-]root           do not treat root commits as boundaries (Default: off)
    --[no-]show-stats     show work cost statistics
    --[no-]progress       force progress reporting
    --[no-]score-debug    show output score for blame entries
    -f, --[no-]show-name  show original filename (Default: auto)
    -n, --[no-]show-number
                          show original linenumber (Default: off)
    -p, --[no-]porcelain  show in a format designed for machine consumption
    --[no-]line-porcelain show porcelain format with per-line commit information
    -c                    use the same output mode as git-annotate (Default: off)
    -t                    show raw timestamp (Default: off)
    -l                    show long commit SHA1 (Default: off)
    -s                    suppress author name and timestamp (Default: off)
    -e, --[no-]show-email show author email instead of name (Default: off)
    -w                    ignore whitespace differences
    --[no-]ignore-rev <rev>
                          ignore <rev> when blaming
    --[no-]ignore-revs-file <file>
                          ignore revisions from <file>
    --[no-]color-lines    color redundant metadata from previous line differently
    --[no-]color-by-age   color lines by age
    --[no-]minimal        spend extra cycles to find better match
    -S <file>             use revisions from <file> instead of calling git-rev-list
    --[no-]contents <file>
                          use <file>'s contents as the final image
    -C[<score>]           find line copies within and across files
    -M[<score>]           find line movements within and across files
    -L <range>            process only line range <start>,<end> or function :<funcname>
    --[no-]abbrev[=<n>]   use <n> digits to display object names
"#;
const BLAME_USAGE_LINE: &str = "usage: git blame [<options>] [<rev-opts>] [<rev>] [--] <file>";

fn blame_unknown_option(option: &str) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!("error: unknown option `{option}'\n{BLAME_USAGE}"),
    }
}

fn blame_usage_error() -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!("{BLAME_USAGE_LINE}\n"),
    }
}

pub(crate) fn run_replay(options: super::history::ReplayOptions) -> Result<()> {
    let super::history::ReplayOptions {
        all,
        branches,
        tags,
        remotes,
        not,
        stdin,
        count,
        exclude,
        topo_order,
        date_order,
        author_date_order,
        reverse,
        abbrev_commit,
        no_abbrev_commit,
        author,
        committer,
        encoding,
        alternate_refs,
        glob,
        ignore_missing,
        indexed_objects,
        remove_empty,
        single_worktree,
        unpacked,
        bisect_all,
        bisect_vars,
        boundary,
        first_parent,
        right_only,
        left_only,
        left_right,
        cherry,
        cherry_pick,
        cherry_mark,
        parents,
        objects,
        objects_edge,
        objects_edge_aggressive,
        show_signature,
        walk_reflogs,
        no_walk,
        do_walk,
        no_merges,
        merge,
        dense,
        sparse,
        full_history,
        children,
        ancestry_path,
        simplify_merges,
        simplify_by_decoration,
        in_commit_order,
        commit_header,
        no_commit_header,
        disk_usage,
        expand_tabs,
        no_expand_tabs,
        show_linear_break,
        header,
        notes,
        no_notes,
        show_notes,
        show_notes_by_default,
        standard_notes,
        no_standard_notes,
        reflog,
        bisect,
        grep_reflog,
        grep,
        invert_grep,
        all_match,
        regexp_ignore_case,
        basic_regexp,
        extended_regexp,
        fixed_strings,
        perl_regexp,
        pretty,
        oneline,
        format,
        date,
        relative_date,
        graph,
        object_names,
        no_object_names,
        exclude_promisor_objects,
        filter,
        filter_print_omitted,
        filter_provided_objects,
        progress,
        missing,
        use_bitmap_index,
        quiet,
        show_pulls,
        timestamp,
        no_filter,
        max_count,
        max_age,
        min_age,
        skip,
        since,
        since_as_filter,
        until,
        max_parents,
        no_max_parents,
        min_parents,
        no_min_parents,
        exclude_first_parent_only,
        merges,
        exclude_hidden,
        contained,
        advance,
        onto,
        revision_ranges,
    } = options;
    if contained && advance.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--advance' and '--contained' cannot be used together".into(),
        });
    }
    let repo = find_repo_with_parent_dir_error()?;
    if advance.is_none() && onto.is_none() {
        return Err(CliError::Stderr {
            code: 129,
            text: replay_usage_error("exactly one of --onto, --advance, or --revert is required"),
        });
    }
    let parsed_max_count = parse_log_max_count(max_count.as_deref())?;
    if revision_ranges.is_empty() && !stdin {
        return Err(CliError::Stderr {
            code: 129,
            text: replay_usage_error("need a revision range"),
        });
    }

    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1)
        .with_stable_pack_snapshot();
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    if topo_order || count {
        let _ = ();
    }
    if date_order || author_date_order {
        eprintln!(
            "warning: some rev walking options will be overridden as 'sort_order' bit in 'struct rev_info' will be forced"
        );
    }
    if reverse {
        eprintln!(
            "warning: some rev walking options will be overridden as 'reverse' bit in 'struct rev_info' will be forced"
        );
    }
    let empty_selection_lane =
        replay_has_empty_selection_lane(left_only, merges, min_age.as_deref());
    let glob_multiple_sources = advance.is_some() && glob.is_some();
    let rev_args = collect_replay_rev_args(
        branches,
        tags,
        remotes,
        not,
        stdin,
        count,
        exclude,
        alternate_refs,
        glob,
        abbrev_commit,
        no_abbrev_commit,
        author,
        committer,
        encoding,
        ignore_missing,
        indexed_objects,
        remove_empty,
        single_worktree,
        unpacked,
        bisect_all,
        bisect_vars,
        boundary,
        first_parent,
        right_only,
        left_only,
        left_right,
        cherry,
        cherry_pick,
        cherry_mark,
        parents,
        objects,
        objects_edge,
        objects_edge_aggressive,
        show_signature,
        walk_reflogs,
        no_walk,
        do_walk,
        no_merges,
        merge,
        dense,
        sparse,
        full_history,
        children,
        ancestry_path,
        simplify_merges,
        simplify_by_decoration,
        in_commit_order,
        commit_header,
        no_commit_header,
        disk_usage,
        expand_tabs,
        no_expand_tabs,
        show_linear_break,
        header,
        notes,
        no_notes,
        show_notes,
        show_notes_by_default,
        standard_notes,
        no_standard_notes,
        reflog,
        bisect,
        grep_reflog.clone(),
        grep,
        invert_grep,
        all_match,
        regexp_ignore_case,
        basic_regexp,
        extended_regexp,
        fixed_strings,
        perl_regexp,
        pretty,
        oneline,
        format,
        date,
        relative_date,
        graph,
        object_names,
        no_object_names,
        exclude_promisor_objects,
        filter.clone(),
        filter_print_omitted,
        filter_provided_objects,
        progress.clone(),
        missing.clone(),
        use_bitmap_index,
        quiet,
        show_pulls,
        timestamp,
        no_filter,
        parsed_max_count,
        max_age,
        min_age,
        skip,
        since,
        since_as_filter,
        until,
        max_parents,
        no_max_parents,
        min_parents,
        no_min_parents,
        merges,
        exclude_first_parent_only,
        exclude_hidden,
        revision_ranges,
    )?;
    if bisect_all {
        return Err(replay_unrecognized_argument("--bisect-all"));
    }
    if bisect_vars {
        return Err(replay_unrecognized_argument("--bisect-vars"));
    }
    if boundary {
        return Err(CliError::Fatal {
            code: 128,
            message: "replaying down from root commit is not supported yet!".into(),
        });
    }
    if commit_header {
        return Err(replay_unrecognized_argument("--commit-header"));
    }
    if no_commit_header {
        return Err(replay_unrecognized_argument("--no-commit-header"));
    }
    if disk_usage {
        return Err(replay_unrecognized_argument("--disk-usage"));
    }
    if exclude_promisor_objects {
        return Err(replay_unrecognized_argument("--exclude-promisor-objects"));
    }
    if filter_print_omitted {
        return Err(replay_unrecognized_argument("--filter-print-omitted"));
    }
    if filter_provided_objects {
        return Err(replay_unrecognized_argument("--filter-provided-objects"));
    }
    if header {
        return Err(replay_unrecognized_argument("--header"));
    }
    if missing.is_some() {
        return Err(replay_unrecognized_argument("--missing=print"));
    }
    if no_object_names {
        return Err(replay_unrecognized_argument("--no-object-names"));
    }
    if object_names {
        return Err(replay_unrecognized_argument("--object-names"));
    }
    if progress.is_some() {
        return Err(replay_unrecognized_argument("--progress=1"));
    }
    if timestamp {
        return Err(replay_unrecognized_argument("--timestamp"));
    }
    if use_bitmap_index {
        return Err(replay_unrecognized_argument("--use-bitmap-index"));
    }
    if graph {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--graph' and '--reverse' cannot be used together".into(),
        });
    }
    if !grep_reflog.is_empty() && !walk_reflogs {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--grep-reflog' requires '--walk-reflogs'".into(),
        });
    }
    if filter.is_some() && !objects {
        return Err(CliError::Fatal {
            code: 128,
            message: "object filtering requires --objects".into(),
        });
    }
    if merge {
        return Err(CliError::Fatal {
            code: 128,
            message:
                "--merge requires one of the pseudorefs MERGE_HEAD, CHERRY_PICK_HEAD, REVERT_HEAD or REBASE_HEAD"
                    .into(),
        });
    }
    if walk_reflogs {
        let reflog_revs = rev_list_reflog_targets(&rev_args);
        if let Some(target) = rev_list_walk_reflogs_exclusion_target(&reflog_revs) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("cannot walk reflogs for {target}"),
            });
        }
    }
    if glob_multiple_sources {
        return Err(CliError::Fatal {
            code: 128,
            message:
                "cannot advance target with multiple sources because ordering would be ill-defined"
                    .into(),
        });
    }
    if empty_selection_lane {
        return Err(CliError::Stderr {
            code: 1,
            text: String::new(),
        });
    }
    let revs = collect_rev_list_revs(&repo, &store, all, rev_args).map_err(|error| {
        if replay_needs_commits_error(&error) {
            CliError::Fatal {
                code: 128,
                message: "need some commits to replay".into(),
            }
        } else {
            error
        }
    })?;
    if advance.is_some()
        && (all || branches || tags || remotes)
        && replay_has_multiple_sources(&revs)
    {
        return Err(CliError::Fatal {
            code: 128,
            message:
                "cannot advance target with multiple sources because ordering would be ill-defined"
                    .into(),
        });
    }
    let mut commits =
        collect_commits_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?;
    commits.reverse();
    let Some(first) = commits.first() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "need some commits to replay".into(),
        });
    };
    let first_commit = commit_cache.read_commit(first)?;
    let base = first_commit
        .parents
        .first()
        .cloned()
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: if boundary {
                "replaying down from root commit is not supported yet!".into()
            } else {
                "cannot replay a root commit yet".into()
            },
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

fn collect_replay_rev_args(
    branches: bool,
    tags: bool,
    remotes: bool,
    not: bool,
    stdin: bool,
    count: bool,
    exclude: Vec<String>,
    alternate_refs: bool,
    glob: Option<String>,
    abbrev_commit: bool,
    no_abbrev_commit: bool,
    author: Option<String>,
    committer: Option<String>,
    encoding: Option<String>,
    ignore_missing: bool,
    indexed_objects: bool,
    remove_empty: bool,
    single_worktree: bool,
    unpacked: bool,
    bisect_all: bool,
    bisect_vars: bool,
    boundary: bool,
    first_parent: bool,
    right_only: bool,
    left_only: bool,
    left_right: bool,
    cherry: bool,
    cherry_pick: bool,
    cherry_mark: bool,
    parents: bool,
    objects: bool,
    objects_edge: bool,
    objects_edge_aggressive: bool,
    show_signature: bool,
    walk_reflogs: bool,
    no_walk: bool,
    do_walk: bool,
    no_merges: bool,
    merge: bool,
    dense: bool,
    sparse: bool,
    full_history: bool,
    children: bool,
    ancestry_path: bool,
    simplify_merges: bool,
    simplify_by_decoration: bool,
    in_commit_order: bool,
    commit_header: bool,
    no_commit_header: bool,
    disk_usage: bool,
    expand_tabs: bool,
    no_expand_tabs: bool,
    show_linear_break: Option<String>,
    header: bool,
    notes: bool,
    no_notes: bool,
    show_notes: bool,
    show_notes_by_default: bool,
    standard_notes: bool,
    no_standard_notes: bool,
    reflog: bool,
    bisect: bool,
    grep_reflog: Vec<String>,
    grep: Vec<String>,
    invert_grep: bool,
    all_match: bool,
    regexp_ignore_case: bool,
    basic_regexp: bool,
    extended_regexp: bool,
    fixed_strings: bool,
    perl_regexp: bool,
    pretty: Option<String>,
    oneline: bool,
    format: Option<String>,
    date: Option<String>,
    relative_date: bool,
    graph: bool,
    object_names: bool,
    no_object_names: bool,
    exclude_promisor_objects: bool,
    filter: Option<String>,
    filter_print_omitted: bool,
    filter_provided_objects: bool,
    progress: Option<String>,
    missing: Option<String>,
    use_bitmap_index: bool,
    quiet: bool,
    show_pulls: bool,
    timestamp: bool,
    no_filter: bool,
    max_count: Option<usize>,
    max_age: Option<String>,
    min_age: Option<String>,
    skip: Option<usize>,
    since: Option<String>,
    since_as_filter: Option<String>,
    until: Option<String>,
    max_parents: Option<String>,
    no_max_parents: bool,
    min_parents: Option<String>,
    no_min_parents: bool,
    merges: bool,
    exclude_first_parent_only: bool,
    exclude_hidden: Option<String>,
    mut revision_ranges: Vec<String>,
) -> Result<Vec<String>> {
    let mut rev_args = Vec::new();
    if count
        || !exclude.is_empty()
        || alternate_refs
        || glob.is_some()
        || abbrev_commit
        || no_abbrev_commit
        || author.is_some()
        || committer.is_some()
        || encoding.is_some()
        || ignore_missing
        || indexed_objects
        || remove_empty
        || single_worktree
        || unpacked
        || bisect_all
        || bisect_vars
        || boundary
        || first_parent
        || right_only
        || left_only
        || left_right
        || cherry
        || cherry_pick
        || cherry_mark
        || parents
        || objects
        || objects_edge
        || objects_edge_aggressive
        || show_signature
        || walk_reflogs
        || no_walk
        || do_walk
        || no_merges
        || merge
        || dense
        || sparse
        || full_history
        || children
        || ancestry_path
        || simplify_merges
        || simplify_by_decoration
        || in_commit_order
        || commit_header
        || no_commit_header
        || disk_usage
        || expand_tabs
        || no_expand_tabs
        || show_linear_break.is_some()
        || header
        || notes
        || no_notes
        || show_notes
        || show_notes_by_default
        || standard_notes
        || no_standard_notes
        || reflog
        || bisect
        || !grep_reflog.is_empty()
        || !grep.is_empty()
        || invert_grep
        || all_match
        || regexp_ignore_case
        || basic_regexp
        || extended_regexp
        || fixed_strings
        || perl_regexp
        || pretty.is_some()
        || oneline
        || format.is_some()
        || date.is_some()
        || relative_date
        || graph
        || object_names
        || no_object_names
        || exclude_promisor_objects
        || filter.is_some()
        || filter_print_omitted
        || filter_provided_objects
        || progress.is_some()
        || missing.is_some()
        || use_bitmap_index
        || show_pulls
        || timestamp
        || no_filter
        || quiet
        || max_count.is_some()
        || max_age.is_some()
        || min_age.is_some()
        || skip.is_some()
        || since.is_some()
        || since_as_filter.is_some()
        || until.is_some()
        || max_parents.is_some()
        || no_max_parents
        || min_parents.is_some()
        || no_min_parents
        || merges
        || exclude_first_parent_only
        || exclude_hidden.is_some()
    {
        let _ = ();
    }
    if branches {
        rev_args.push("--branches".to_owned());
    }
    if tags {
        rev_args.push("--tags".to_owned());
    }
    if remotes {
        rev_args.push("--remotes".to_owned());
    }
    if not {
        rev_args.push("--not".to_owned());
    }
    if stdin {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        revision_ranges.extend(
            input
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
        );
    }
    rev_args.extend(revision_ranges);
    Ok(rev_args)
}

fn replay_needs_commits_error(error: &CliError) -> bool {
    matches!(error, CliError::Message(message) if message == "`rev-list` requires at least one positive revision")
}

fn replay_has_multiple_sources(revs: &RevListRevs) -> bool {
    revs.include.len() > 1 || !revs.extra_objects.is_empty() || revs.symmetric_diff.is_some()
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

fn replay_has_empty_selection_lane(left_only: bool, merges: bool, min_age: Option<&str>) -> bool {
    left_only || merges || min_age.is_some()
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

pub(crate) fn run_history(command: HistoryCommand, _raw_args: &[String]) -> Result<()> {
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
    let repo = find_repo_or_bare()?;
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
    let repo = find_repo_or_bare()?;
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
    let repo = find_repo_or_bare()?;
    let mut args = args.into_iter().peekable();
    let command = args
        .next_if(|arg| {
            matches!(
                arg.as_str(),
                "show" | "list" | "exists" | "expire" | "delete" | "drop" | "write"
            )
        })
        .map(|arg| match arg.as_str() {
            "show" => ReflogCommand::Show,
            "list" => ReflogCommand::List,
            "exists" => ReflogCommand::Exists,
            "expire" => ReflogCommand::Expire,
            "delete" => ReflogCommand::Delete,
            "drop" => ReflogCommand::Drop,
            "write" => ReflogCommand::Write,
            _ => unreachable!("reflog command parser only selects known subcommands"),
        })
        .unwrap_or(ReflogCommand::Show);
    match command {
        ReflogCommand::Drop => reflog_drop(&repo, args.collect()),
        ReflogCommand::Write => reflog_write(&repo, args.collect()),
        ReflogCommand::Delete => reflog_delete(&repo, args.collect()),
        ReflogCommand::Expire => {
            let args = args.collect::<Vec<_>>();
            if args.iter().any(|arg| arg == "-h" || arg == "--help") {
                print!("{}", reflog_expire_usage());
                return Err(CliError::Exit(129));
            }
            reflog_expire(args)
        }
        ReflogCommand::Show => {
            let mut date_mode = ReflogDateMode::Index;
            let mut no_abbrev_commit = false;
            let mut format = None;
            let mut max_count = None;
            let mut grep_reflog = Vec::new();
            let mut ref_name = None;
            let mut all = false;
            let mut pathspecs = Vec::new();
            let mut in_pathspecs = false;
            while let Some(arg) = args.next() {
                if in_pathspecs {
                    pathspecs.push(arg);
                } else if arg == "--" {
                    in_pathspecs = true;
                } else if arg == "-h" || arg == "--help" {
                    print!("{}", reflog_show_usage());
                    return Err(CliError::Exit(129));
                } else if let Some(value) = arg.strip_prefix("--date=") {
                    date_mode = parse_reflog_date_mode(value)?;
                } else if arg == "--no-abbrev-commit" {
                    no_abbrev_commit = true;
                } else if arg == "--all" {
                    all = true;
                } else if arg == "--date" {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "reflog --date requires --date=<format>".into(),
                    });
                } else if let Some(value) = arg.strip_prefix("--format=") {
                    format = Some(value.to_owned());
                } else if arg == "--max-count" {
                    let value = args.next().ok_or_else(|| CliError::Fatal {
                        code: 129,
                        message: "reflog --max-count requires a value".into(),
                    })?;
                    max_count = parse_log_max_count(Some(&value))?;
                } else if let Some(value) = arg.strip_prefix("--max-count=") {
                    max_count = parse_log_max_count(Some(value))?;
                } else if let Some(value) = arg.strip_prefix('-')
                    && !value.is_empty()
                    && value.bytes().all(|byte| byte.is_ascii_digit())
                {
                    max_count = parse_log_max_count(Some(value))?;
                } else if arg == "-n" {
                    let value = args.next().ok_or_else(|| CliError::Fatal {
                        code: 129,
                        message: "reflog -n requires a value".into(),
                    })?;
                    max_count = parse_log_max_count(Some(&value))?;
                } else if let Some(value) = arg.strip_prefix("-n") {
                    if value.is_empty() {
                        ref_name = Some(arg);
                    } else {
                        max_count = parse_log_max_count(Some(value))?;
                    }
                } else if arg == "--grep-reflog" {
                    grep_reflog.push(args.next().ok_or_else(|| CliError::Fatal {
                        code: 129,
                        message: "reflog --grep-reflog requires a value".into(),
                    })?);
                } else if let Some(value) = arg.strip_prefix("--grep-reflog=") {
                    grep_reflog.push(value.to_owned());
                } else {
                    ref_name = Some(arg);
                }
            }
            let options = ReflogShowOptions {
                ref_name: ref_name.as_deref().unwrap_or("HEAD"),
                date_mode,
                no_abbrev_commit,
                format: format.as_deref(),
                max_count,
                grep_reflog: &grep_reflog,
                pathspecs: &pathspecs,
            };
            if all {
                for name in reflog_names(&repo)? {
                    reflog_show(
                        &repo,
                        ReflogShowOptions {
                            ref_name: &name,
                            ..options
                        },
                    )?;
                }
                Ok(())
            } else {
                reflog_show(&repo, options)
            }
        }
        ReflogCommand::List => {
            if let Some(arg) = args.next() {
                return Err(CliError::Stderr {
                    code: 1,
                    text: format!("error: list does not accept arguments: '{arg}'\n"),
                });
            }
            reflog_list(&repo)
        }
        ReflogCommand::Exists => {
            let mut args = args.collect::<Vec<_>>();
            if matches!(
                args.first().map(String::as_str),
                Some("--" | "--end-of-options")
            ) {
                args.remove(0);
            }
            if args.len() != 1 || args[0].starts_with('-') {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "usage: git reflog exists <ref>\n".into(),
                });
            }
            let ref_name = args.remove(0);
            if reflog_exists(&repo, &ref_name)? {
                Ok(())
            } else {
                Err(CliError::Exit(1))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReflogCommand {
    Show,
    List,
    Exists,
    Expire,
    Delete,
    Drop,
    Write,
}

fn reflog_write(repo: &GitRepo, args: Vec<String>) -> Result<()> {
    if args.len() != 4 {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git reflog write <ref> <old-oid> <new-oid> <message>\n".into(),
        });
    }
    let ref_name = &args[0];
    if !(ref_name.starts_with("refs/") && check_ref_format(ref_name, false)
        || is_valid_pseudoref_name(ref_name))
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid reference name: {ref_name}"),
        });
    }
    let old_id = reflog_write_object_id(&args[1], "old")?;
    let new_id = reflog_write_object_id(&args[2], "new")?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    for (label, id) in [("old", &old_id), ("new", &new_id)] {
        if *id != zero_object_id() && !store.contains_object(id)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("{label} object {} does not exist", id.to_hex()),
            });
        }
    }
    let message = args[3].split_whitespace().collect::<Vec<_>>().join(" ");
    append_reflog(repo, ref_name, &old_id, &new_id, &message)
}

fn reflog_write_object_id(value: &str, label: &str) -> Result<ObjectId> {
    if value.len() != GitHashAlgorithm::Sha1.digest_len() * 2 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid {label} object ID: {value}"),
        });
    }
    ObjectId::from_hex(GitHashAlgorithm::Sha1, value).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid {label} object ID: {value}"),
    })
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
    let update_ref = args.iter().any(|arg| arg == "--updateref");
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
        if update_ref && ref_name != "HEAD" {
            reflog_update_ref_to_log_tip(repo, &ref_name)?;
        }
    }
    Ok(())
}

fn reflog_update_ref_to_log_tip(repo: &GitRepo, ref_name: &str) -> Result<()> {
    if let Some(reftable) = reftable_reflog(repo, ref_name)? {
        let Some(record) = reftable
            .store
            .reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == reftable.ref_name)
            .max_by_key(|record| record.update_index)
        else {
            return Ok(());
        };
        return reftable
            .store
            .write_ref(&reftable.ref_name, &record.new_id)
            .map_err(CliError::Io);
    }
    let path = reflog_path(repo, ref_name)?;
    let content = fs::read_to_string(path)?;
    let Some(entry) = content.lines().next_back().and_then(parse_reflog_entry) else {
        return Ok(());
    };
    let canonical = reflog_expire_canonical_ref_name(repo, ref_name)?;
    let common_dir = read_common_git_dir(&repo.git_dir)?;
    RefStore::new(common_dir, GitHashAlgorithm::Sha1)
        .write_ref(&canonical, &entry.new_id)
        .map_err(CliError::Io)
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
    if let Some(reftable) = reftable_reflog(repo, ref_name)? {
        let mut records = reftable
            .store
            .reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == reftable.ref_name)
            .collect::<Vec<_>>();
        records.sort_by_key(|record| record.update_index);
        let remove_index = match selector {
            ReflogDeleteSelector::Index(index) => records.len().checked_sub(index + 1),
            ReflogDeleteSelector::Date(timestamp) => records.iter().rposition(|record| {
                i64::try_from(record.timestamp).unwrap_or(i64::MAX) <= timestamp
            }),
        };
        let Some(remove_index) = remove_index else {
            return Ok(());
        };
        return reftable
            .store
            .delete_reftable_log_entries(
                &reftable.ref_name,
                &[records[remove_index].update_index],
                true,
            )
            .map_err(CliError::Io);
    }
    let path = reflog_path(repo, ref_name)?;
    let content = fs::read_to_string(&path)?;
    let mut lines = content.lines().map(str::to_owned).collect::<Vec<_>>();
    let Some(remove_index) = reflog_delete_line_index(&lines, selector) else {
        return Ok(());
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
    let repo = find_repo_or_bare()?;
    validate_explicit_reflog_expire_refs(&repo, &args)?;
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
    let explicit_policy = reflog_expire_explicit_policy(&args)?;
    if let Some(ref policy) = explicit_policy {
        if policy.expire == Some(ReflogExpireAge::All) {
            return reflog_expire_by_timestamp(&repo, &args, i64::MAX);
        }
        if policy.has_unreachable_pruning() {
            return reflog_expire_with_unreachable_policy(&repo, &args, policy);
        }
        if let Some(expire_age) = policy.expire {
            return match expire_age {
                ReflogExpireAge::All => reflog_expire_by_timestamp(&repo, &args, i64::MAX),
                ReflogExpireAge::Never => reflog_expire_noop(&repo, &args),
                ReflogExpireAge::Timestamp(timestamp) => {
                    reflog_expire_by_timestamp(&repo, &args, timestamp)
                }
            };
        }
    }
    if reflog_expire_refs(&repo, &args)?.is_empty() {
        return Ok(());
    }
    let Some(timestamp) = reflog_expire_default_timestamp(&repo)? else {
        return reflog_expire_noop(&repo, &args);
    };
    reflog_expire_default_by_timestamp(&repo, &args, timestamp)
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
            reflog_expire_path_by_timestamp(repo, &path, i64::MAX, verbose)?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReflogExpireAge {
    All,
    Never,
    Timestamp(i64),
}

#[derive(Debug, Clone, Copy, Default)]
struct ReflogExpirePolicy {
    expire: Option<ReflogExpireAge>,
    expire_unreachable: Option<ReflogExpireAge>,
}

impl ReflogExpirePolicy {
    fn has_unreachable_pruning(self) -> bool {
        self.expire_unreachable.is_some()
    }
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

fn reflog_expire_explicit_policy(args: &[String]) -> Result<Option<ReflogExpirePolicy>> {
    let mut policy = ReflogExpirePolicy::default();
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--expire" => {
                if let Some(value) = iter.next() {
                    policy.expire = Some(parse_reflog_expire_age(value)?);
                }
            }
            "--expire-unreachable" => {
                if let Some(value) = iter.next() {
                    policy.expire_unreachable = Some(parse_reflog_expire_age(value)?);
                }
            }
            value if value.starts_with("--expire=") => {
                policy.expire = Some(parse_reflog_expire_age(&value["--expire=".len()..])?);
            }
            value if value.starts_with("--expire-unreachable=") => {
                policy.expire_unreachable = Some(parse_reflog_expire_age(
                    &value["--expire-unreachable=".len()..],
                )?);
            }
            _ => {}
        }
    }
    if policy.expire.is_none() && policy.expire_unreachable.is_none() {
        return Ok(None);
    }
    Ok(Some(policy))
}

fn parse_reflog_expire_age(value: &str) -> Result<ReflogExpireAge> {
    if reflog_expire_policy_value_is_now(value) {
        return Ok(ReflogExpireAge::All);
    }
    if reflog_expire_policy_value_is_never(value) {
        return Ok(ReflogExpireAge::Never);
    }
    let (timestamp, _) = parse_git_date(value)?;
    Ok(ReflogExpireAge::Timestamp(timestamp))
}

fn reflog_expire_default_timestamp(repo: &GitRepo) -> Result<Option<i64>> {
    let value = read_config_value(repo, "gc.reflogexpire")?.unwrap_or_default();
    if value.is_empty() {
        return Ok(Some(current_unix_timestamp()? - 90 * 24 * 60 * 60));
    }
    if reflog_expire_policy_value_is_never(&value) {
        return Ok(None);
    }
    if reflog_expire_policy_value_is_now(&value) {
        return Ok(Some(i64::MAX));
    }
    if let Some(timestamp) = parse_reflog_expire_relative_ago(&value)? {
        return Ok(Some(timestamp));
    }
    let (timestamp, _) = parse_git_date(&value)?;
    Ok(Some(timestamp))
}

fn parse_reflog_expire_relative_ago(value: &str) -> Result<Option<i64>> {
    let normalized = value.trim().to_ascii_lowercase().replace('.', " ");
    let parts = normalized.split_whitespace().collect::<Vec<_>>();
    let [amount, unit, "ago"] = parts.as_slice() else {
        return Ok(None);
    };
    let amount = amount.parse::<i64>().map_err(|error| CliError::Fatal {
        code: 128,
        message: format!("invalid reflog expiry value '{value}': {error}"),
    })?;
    let seconds = match *unit {
        "second" | "seconds" => amount,
        "minute" | "minutes" => amount * 60,
        "hour" | "hours" => amount * 60 * 60,
        "day" | "days" => amount * 24 * 60 * 60,
        "week" | "weeks" => amount * 7 * 24 * 60 * 60,
        "month" | "months" => amount * 30 * 24 * 60 * 60,
        "year" | "years" => amount * 365 * 24 * 60 * 60,
        _ => return Ok(None),
    };
    Ok(Some(current_unix_timestamp()? - seconds))
}

fn reflog_expire_by_timestamp(repo: &GitRepo, args: &[String], timestamp: i64) -> Result<()> {
    let verbose = args.iter().any(|arg| arg == "--verbose");
    if args.iter().any(|arg| arg == "--all") {
        for path in reflog_expire_paths(repo, args)? {
            reflog_expire_path_by_timestamp(repo, &path, timestamp, verbose)?;
        }
        return Ok(());
    }
    for ref_name in reflog_expire_refs(repo, args)? {
        reflog_expire_ref_by_timestamp(repo, &ref_name, timestamp, verbose)?;
    }
    Ok(())
}

fn reflog_expire_with_unreachable_policy(
    repo: &GitRepo,
    args: &[String],
    policy: &ReflogExpirePolicy,
) -> Result<()> {
    let verbose = args.iter().any(|arg| arg == "--verbose");
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    for ref_name in reflog_expire_refs(repo, args)? {
        reflog_expire_ref_with_policy(repo, &store, &ref_name, policy, verbose)?;
    }
    Ok(())
}

fn reflog_expire_default_by_timestamp(
    repo: &GitRepo,
    args: &[String],
    timestamp: i64,
) -> Result<()> {
    let verbose = args.iter().any(|arg| arg == "--verbose");
    for ref_name in reflog_expire_refs(repo, args)? {
        if ref_name == "HEAD" {
            reflog_expire_keep_ref(repo, &ref_name, false)?;
            continue;
        }
        reflog_expire_ref_by_timestamp(repo, &ref_name, timestamp, verbose)?;
    }
    Ok(())
}

fn reflog_expire_ref_by_timestamp(
    repo: &GitRepo,
    ref_name: &str,
    timestamp: i64,
    verbose: bool,
) -> Result<()> {
    if let Some(reftable) = reftable_reflog(repo, ref_name)? {
        let records = reftable
            .store
            .reftable_logs()?
            .into_iter()
            .filter(|record| {
                record.ref_name == reftable.ref_name
                    && i64::try_from(record.timestamp).unwrap_or(i64::MAX) <= timestamp
            })
            .collect::<Vec<_>>();
        if verbose {
            for record in &records {
                println!("prune {}", record.message.trim_end_matches('\n'));
            }
        }
        let indices = records
            .iter()
            .map(|record| record.update_index)
            .collect::<Vec<_>>();
        return reftable
            .store
            .delete_reftable_log_entries(&reftable.ref_name, &indices, true)
            .map_err(CliError::Io);
    }
    let path = reflog_path(repo, ref_name)?;
    reflog_expire_path_by_timestamp(repo, &path, timestamp, verbose)?;
    apply_reflog_shared_repository_permissions(repo, &path)
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
    let worktrees = read_common_git_dir(&repo.git_dir)?.join("worktrees");
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

fn reflog_expire_path_by_timestamp(
    repo: &GitRepo,
    path: &Path,
    timestamp: i64,
    verbose: bool,
) -> Result<()> {
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
                println!("prune {}", reflog_expire_verbose_message(line));
            }
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    fs::write(path, kept).map_err(CliError::Io)?;
    apply_reflog_shared_repository_permissions(repo, path)
}

fn reflog_expire_ref_with_policy(
    repo: &GitRepo,
    store: &LooseObjectStore,
    ref_name: &str,
    policy: &ReflogExpirePolicy,
    verbose: bool,
) -> Result<()> {
    let path = reflog_path(repo, ref_name)?;
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let reachable_tip = reflog_expire_reachable_tip(repo, store, ref_name);
    let mut seen = HashMap::<ObjectId, bool>::new();
    let mut kept = String::new();
    for line in content.lines() {
        let expire = parse_reflog_entry(line)
            .map(|entry| {
                reflog_entry_matches_expire_policy(
                    store,
                    reachable_tip.as_ref(),
                    &mut seen,
                    &entry,
                    policy,
                )
            })
            .unwrap_or(false);
        if expire {
            if verbose {
                println!("prune {}", reflog_expire_verbose_message(line));
            }
        } else {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    fs::write(&path, kept).map_err(CliError::Io)?;
    apply_reflog_shared_repository_permissions(repo, &path)
}

fn apply_reflog_shared_repository_permissions(repo: &GitRepo, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let Some(value) = read_config_value(&repo, "core.sharedRepository")? else {
            return Ok(());
        };
        let mode = match value.as_str() {
            "group" | "true" | "1" | "2" | "0664" | "664" => 0o664,
            "all" | "world" | "everybody" | "0666" | "666" => 0o666,
            "0660" | "660" => 0o660,
            _ => return Ok(()),
        };
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(CliError::Io)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = repo;
        let _ = path;
        Ok(())
    }
}

fn reflog_expire_reachable_tip(
    repo: &GitRepo,
    store: &LooseObjectStore,
    ref_name: &str,
) -> Option<ObjectId> {
    resolve_commitish(repo, store, ref_name).ok()
}

fn reflog_entry_matches_expire_policy(
    store: &LooseObjectStore,
    reachable_tip: Option<&ObjectId>,
    seen: &mut HashMap<ObjectId, bool>,
    entry: &ReflogEntry,
    policy: &ReflogExpirePolicy,
) -> bool {
    match policy.expire {
        Some(ReflogExpireAge::All) => return true,
        Some(ReflogExpireAge::Timestamp(timestamp)) if entry.timestamp <= timestamp => return true,
        _ => {}
    }
    let Some(unreachable_policy) = policy.expire_unreachable else {
        return false;
    };
    let old_reachable = entry.old_id == zero_object_id()
        || reflog_entry_is_reachable(store, reachable_tip, seen, &entry.old_id);
    let new_reachable = entry.new_id == zero_object_id()
        || reflog_entry_is_reachable(store, reachable_tip, seen, &entry.new_id);
    if old_reachable && new_reachable {
        return false;
    }
    match unreachable_policy {
        ReflogExpireAge::All => true,
        ReflogExpireAge::Never => false,
        ReflogExpireAge::Timestamp(timestamp) => entry.timestamp <= timestamp,
    }
}

fn reflog_entry_is_reachable(
    store: &LooseObjectStore,
    reachable_tip: Option<&ObjectId>,
    seen: &mut HashMap<ObjectId, bool>,
    target: &ObjectId,
) -> bool {
    let Some(tip) = reachable_tip else {
        return false;
    };
    reflog_object_reachable_from(store, tip, target, seen)
}

fn reflog_object_reachable_from(
    store: &LooseObjectStore,
    start: &ObjectId,
    target: &ObjectId,
    seen: &mut HashMap<ObjectId, bool>,
) -> bool {
    if start == target {
        return true;
    }
    if *target == zero_object_id() {
        return false;
    }
    if let Some(reachable) = seen.get(target) {
        return *reachable;
    }
    let mut pending = vec![start.clone()];
    let mut visited = HashSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        if current == *target {
            seen.insert(target.clone(), true);
            return true;
        }
        let object = match store.packed_first().read_object(&current) {
            Ok(object) => object,
            Err(_) => continue,
        };
        if object.kind != GitObjectKind::Commit {
            continue;
        }
        let commit = match decode_commit(current.algorithm(), &object.content) {
            Ok(commit) => commit,
            Err(_) => continue,
        };
        pending.extend(commit.parents);
    }
    seen.insert(target.clone(), false);
    false
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
    if let Some(reftable) = reftable_reflog(repo, ref_name)? {
        if !reftable.store.reftable_log_exists(&reftable.ref_name)? {
            return Err(reflog_not_found_error(ref_name));
        }
        if verbose {
            for record in reftable
                .store
                .reftable_logs()?
                .into_iter()
                .filter(|record| record.ref_name == reftable.ref_name)
            {
                println!("keep {}", record.message.trim_end_matches('\n'));
            }
        }
        return Ok(());
    }
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
            println!("keep {}", reflog_expire_verbose_message(line));
        }
    }
    Ok(())
}

fn reflog_expire_verbose_message(line: &str) -> &str {
    line.split_once('\t')
        .map(|(_, message)| message)
        .unwrap_or(line)
}

fn reflog_not_found_error(ref_name: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("error: reflog could not be found: '{ref_name}'"),
    }
}

fn reflog_expire_refs(repo: &GitRepo, args: &[String]) -> Result<Vec<String>> {
    if args.iter().any(|arg| arg == "--all") {
        return Ok(reflog_names(repo)?.into_iter().collect());
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

fn validate_explicit_reflog_expire_refs(repo: &GitRepo, args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "--all") {
        return Ok(());
    }
    let mut errors = String::new();
    for ref_name in reflog_expire_refs(repo, args)? {
        let exists = if ref_name.contains("@{") {
            false
        } else if let Some(reftable) = reftable_reflog(repo, &ref_name)? {
            reftable.store.reftable_log_exists(&reftable.ref_name)?
        } else {
            reflog_path(repo, &ref_name)?.is_file()
        };
        if !exists {
            errors.push_str(&format!("error: {ref_name} points nowhere!\n"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CliError::Stderr {
            code: 1,
            text: errors,
        })
    }
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
    Default,
    Local,
    Iso,
    IsoStrict,
    Rfc2822,
    Short,
    Unix,
    Raw,
    Relative,
    Human,
}

fn parse_reflog_date_mode(value: &str) -> Result<ReflogDateMode> {
    match value {
        "default" => Ok(ReflogDateMode::Default),
        "local" => Ok(ReflogDateMode::Local),
        "iso" => Ok(ReflogDateMode::Iso),
        "iso-strict" => Ok(ReflogDateMode::IsoStrict),
        "rfc" | "rfc2822" => Ok(ReflogDateMode::Rfc2822),
        "short" => Ok(ReflogDateMode::Short),
        "unix" => Ok(ReflogDateMode::Unix),
        "raw" => Ok(ReflogDateMode::Raw),
        "relative" => Ok(ReflogDateMode::Relative),
        "human" => Ok(ReflogDateMode::Human),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown date format {value}"),
        }),
    }
}

#[derive(Debug, Clone, Copy)]
struct ReflogShowOptions<'a> {
    ref_name: &'a str,
    date_mode: ReflogDateMode,
    no_abbrev_commit: bool,
    format: Option<&'a str>,
    max_count: Option<usize>,
    grep_reflog: &'a [String],
    pathspecs: &'a [String],
}

fn reflog_show(repo: &GitRepo, options: ReflogShowOptions<'_>) -> Result<()> {
    let ref_name = options.ref_name;
    if !reflog_show_pathspecs_match(repo, options.pathspecs)? {
        return Ok(());
    }
    let display = reflog_display_name(ref_name);
    let object_len = if options.no_abbrev_commit {
        GitHashAlgorithm::Sha1.digest_len() * 2
    } else {
        7
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let mut index = 0usize;
    let mut emitted = 0usize;
    let limit = options.max_count.unwrap_or(usize::MAX);
    let mut render_entry = |entry: ReflogEntry| -> Result<()> {
        if emitted >= limit {
            return Ok(());
        }
        let selector_index = index;
        index += 1;
        if !options.grep_reflog.is_empty()
            && !shortlog_commit_matches_grep(
                entry.message.as_bytes(),
                options.grep_reflog,
                false,
                false,
                false,
                ShortlogPatternMode::Basic,
            )?
        {
            return Ok(());
        }
        if let Some(format) = options.format
            && format.starts_with('%')
        {
            println!(
                "{}",
                render_reflog_log_format(format, &display, selector_index, &entry)?
            );
            emitted += 1;
            return Ok(());
        }
        if let Some(format) = options.format {
            let log_format = LogFormat::parse(false, Some(format), None)?;
            let commit = commit_cache.read_commit(&entry.new_id)?;
            let rendered = log_format.render_with_context_default_date(
                &entry.new_id,
                commit.as_ref(),
                false,
                object_len,
                None,
                true,
                true,
                &LogDecorations::empty(),
                &LogNotes::empty(),
            )?;
            let selector = reflog_selector(selector_index, &entry, options.date_mode)?;
            print!(
                "{}",
                insert_reflog_headers(rendered, &format!("{}@{{{selector}}}", display), &entry,)
            );
            emitted += 1;
            return Ok(());
        }
        let selector = reflog_selector(selector_index, &entry, options.date_mode)?;
        println!(
            "{} {}@{{{}}}: {}",
            short_object_id_len(&entry.new_id, object_len),
            display,
            selector,
            entry.message
        );
        emitted += 1;
        Ok(())
    };
    if let Some(entries) = reftable_reflog_entries(repo, ref_name)? {
        if entries.is_empty() && ref_name != "HEAD" && !reflog_exists(repo, ref_name)? {
            return Err(ambiguous_revision_error(ref_name));
        }
        for entry in entries.into_iter().rev() {
            render_entry(entry)?;
        }
    } else {
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
        for_each_reflog_line_rev(file, |line| {
            if let Some(entry) = parse_reflog_entry(line) {
                render_entry(entry)?;
            }
            Ok(())
        })?;
    }
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
        ReflogDateMode::Default => reflog_default_date(entry),
        ReflogDateMode::Local => reflog_local_date(entry),
        ReflogDateMode::Iso => reflog_iso_selector(entry.timestamp, &entry.timezone),
        ReflogDateMode::IsoStrict => reflog_strict_iso_selector(entry.timestamp, &entry.timezone),
        ReflogDateMode::Rfc2822 => reflog_mail_date(entry),
        ReflogDateMode::Short => reflog_short_date(entry),
        ReflogDateMode::Unix => Ok(entry.timestamp.to_string()),
        ReflogDateMode::Raw => Ok(format!("{} {}", entry.timestamp, entry.timezone)),
        ReflogDateMode::Relative => reflog_relative_date(entry.timestamp),
        ReflogDateMode::Human => reflog_human_date(entry),
    }
}

fn reflog_list(repo: &GitRepo) -> Result<()> {
    for name in reflog_names(repo)? {
        println!("{name}");
    }
    Ok(())
}

fn reflog_names(repo: &GitRepo) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let logs_dir = repo.git_dir.join("logs");
    let mut local_names = Vec::new();
    collect_reflog_names(&logs_dir, &logs_dir, &mut local_names)?;
    names.extend(local_names);
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(&common_git_dir, GitHashAlgorithm::Sha1);
    if refs.storage_kind()? == zmin_git_core::refs::RefStorageKind::Reftable {
        names.extend(
            refs.reftable_logs()?
                .into_iter()
                .map(|record| record.ref_name),
        );
    }
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
    Ok(names)
}

fn log_reflog_object_ids(repo: &GitRepo) -> Result<Vec<String>> {
    let zero = zero_object_id();
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for name in reflog_names(repo)? {
        if let Some(entries) = reftable_reflog_entries(repo, &name)? {
            for entry in entries {
                insert_log_reflog_object_id(&entry.old_id, &zero, &mut seen, &mut ids);
                insert_log_reflog_object_id(&entry.new_id, &zero, &mut seen, &mut ids);
            }
            continue;
        }
        let file = match fs::File::open(reflog_path(repo, &name)?) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        for_each_reflog_line_rev(file, |line| {
            if let Some(entry) = parse_reflog_entry(line) {
                insert_log_reflog_object_id(&entry.old_id, &zero, &mut seen, &mut ids);
                insert_log_reflog_object_id(&entry.new_id, &zero, &mut seen, &mut ids);
            }
            Ok(())
        })?;
    }
    Ok(ids)
}

fn insert_log_reflog_object_id(
    id: &ObjectId,
    zero: &ObjectId,
    seen: &mut HashSet<ObjectId>,
    ids: &mut Vec<String>,
) {
    if id != zero && seen.insert(id.clone()) {
        ids.push(id.to_string());
    }
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

#[derive(Debug, Clone)]
pub(crate) struct ReflogEntry {
    pub(crate) old_id: ObjectId,
    pub(crate) new_id: ObjectId,
    pub(crate) identity: String,
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
    let identity = fields.collect::<Vec<_>>().join(" ");
    if identity.is_empty() {
        return None;
    }
    Some(ReflogEntry {
        old_id,
        new_id,
        identity,
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

fn reflog_strict_iso_selector(timestamp: i64, timezone: &str) -> Result<String> {
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "reflog entry has invalid timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "reflog entry timestamp is out of range".into(),
    })?;
    let formatted = utc
        .with_timezone(&offset)
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string();
    if let Some(prefix) = formatted.strip_suffix("+00:00") {
        Ok(format!("{prefix}Z"))
    } else {
        Ok(formatted)
    }
}

fn reflog_mail_date(entry: &ReflogEntry) -> Result<String> {
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
        .format("%a, %d %b %Y %H:%M:%S %z")
        .to_string())
}

fn reflog_short_date(entry: &ReflogEntry) -> Result<String> {
    let offset = parse_timezone_offset(&entry.timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "reflog entry has invalid timezone".into(),
    })?;
    let utc =
        chrono::DateTime::from_timestamp(entry.timestamp, 0).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "reflog entry timestamp is out of range".into(),
        })?;
    Ok(utc.with_timezone(&offset).format("%Y-%m-%d").to_string())
}

fn reflog_local_date(entry: &ReflogEntry) -> Result<String> {
    Ok(crate::runtime::local_datetime(entry.timestamp)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "reflog entry timestamp is out of range".into(),
        })?
        .format("%a %b %-d %H:%M:%S %Y")
        .to_string())
}

fn reflog_relative_date(timestamp: i64) -> Result<String> {
    let now = git_test_date_now().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
            .unwrap_or(0)
    });
    Ok(relative_date_from_timestamps(timestamp, now))
}

pub(crate) fn relative_date_from_timestamps(timestamp: i64, now: i64) -> String {
    if now < timestamp {
        return "in the future".to_owned();
    }
    let mut diff = now - timestamp;
    if diff < 90 {
        return plural_blame_date(diff, "second");
    }
    diff = (diff + 30) / 60;
    if diff < 90 {
        return plural_blame_date(diff, "minute");
    }
    diff = (diff + 30) / 60;
    if diff < 36 {
        return plural_blame_date(diff, "hour");
    }
    diff = (diff + 12) / 24;
    if diff < 14 {
        return plural_blame_date(diff, "day");
    }
    if diff < 70 {
        return plural_blame_date((diff + 3) / 7, "week");
    }
    if diff < 365 {
        return plural_blame_date((diff + 15) / 30, "month");
    }
    if diff < 1825 {
        let total_months = (diff * 12 * 2 + 365) / (365 * 2);
        let years = total_months / 12;
        let months = total_months % 12;
        if months == 0 {
            return plural_blame_date(years, "year");
        }
        let year_unit = if years == 1 { "year" } else { "years" };
        let month_unit = if months == 1 { "month" } else { "months" };
        return format!("{years} {year_unit}, {months} {month_unit} ago");
    }
    plural_blame_date((diff + 183) / 365, "year")
}

fn reflog_human_date(entry: &ReflogEntry) -> Result<String> {
    let offset = parse_timezone_offset(&entry.timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "reflog entry has invalid timezone".into(),
    })?;
    let utc =
        chrono::DateTime::from_timestamp(entry.timestamp, 0).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "reflog entry timestamp is out of range".into(),
        })?;
    let entry_time = utc.with_timezone(&offset);
    let now = git_test_date_now()
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .map(|timestamp| timestamp.with_timezone(&offset))
        .unwrap_or_else(|| crate::runtime::local_now().with_timezone(&offset));
    if entry_time.year() == now.year() && entry_time.month() == now.month() {
        if entry_time.day() + 5 > now.day() {
            return Ok(entry_time.format("%a %H:%M %z").to_string());
        }
        return Ok(entry_time.format("%b %-d %H:%M").to_string());
    }
    if entry_time.year() == now.year() {
        return Ok(entry_time.format("%b %-d %H:%M").to_string());
    }
    Ok(entry_time.format("%b %-d %Y").to_string())
}

fn reflog_path(repo: &GitRepo, ref_name: &str) -> Result<PathBuf> {
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    if ref_name == "main-worktree/HEAD" {
        return Ok(common_git_dir.join("logs/HEAD"));
    }
    if let Some(rest) = ref_name.strip_prefix("worktrees/")
        && let Some((worktree, "HEAD")) = rest.split_once('/')
    {
        return Ok(common_git_dir
            .join("worktrees")
            .join(worktree)
            .join("logs/HEAD"));
    }
    let normalized = normalized_reflog_name(repo, ref_name)?;
    let git_dir = if is_per_worktree_ref(&normalized) {
        repo.git_dir.clone()
    } else {
        common_git_dir
    };
    Ok(git_dir.join("logs").join(normalized))
}

fn normalized_reflog_name(repo: &GitRepo, ref_name: &str) -> Result<String> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if ref_name == "HEAD" || ref_name.starts_with("refs/") {
        Ok(ref_name.to_owned())
    } else if ref_name == "stash" {
        Ok("refs/stash".to_owned())
    } else if let Some(ref_name) = branch_checkout_ref(&refs, ref_name)? {
        Ok(ref_name)
    } else {
        Ok(ref_name.to_owned())
    }
}

struct ReftableReflog {
    ref_name: String,
    store: RefStore,
}

fn reftable_reflog(repo: &GitRepo, ref_name: &str) -> Result<Option<ReftableReflog>> {
    if ref_name == "main-worktree/HEAD" || ref_name.starts_with("worktrees/") {
        return Ok(None);
    }
    let normalized = normalized_reflog_name(repo, ref_name)?;
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let git_dir = if is_per_worktree_ref(&normalized) {
        repo.git_dir.clone()
    } else {
        common_git_dir
    };
    let store = RefStore::new(git_dir, repo_hash_algorithm_from_config(repo)?);
    if store.storage_kind()? != zmin_git_core::refs::RefStorageKind::Reftable {
        return Ok(None);
    }
    Ok(Some(ReftableReflog {
        ref_name: normalized,
        store,
    }))
}

fn reftable_reflog_entries(repo: &GitRepo, ref_name: &str) -> Result<Option<Vec<ReflogEntry>>> {
    let Some(reftable) = reftable_reflog(repo, ref_name)? else {
        return Ok(None);
    };
    let mut records = reftable
        .store
        .reftable_logs()?
        .into_iter()
        .filter(|record| record.ref_name == reftable.ref_name)
        .collect::<Vec<_>>();
    records.sort_by_key(|record| record.update_index);
    Ok(Some(
        records
            .into_iter()
            .map(|record| ReflogEntry {
                old_id: record.old_id,
                new_id: record.new_id,
                identity: format!("{} <{}>", record.name, record.email),
                timestamp: i64::try_from(record.timestamp).unwrap_or(i64::MAX),
                timezone: format!("{:+05}", record.timezone_offset),
                message: record.message.trim_end_matches('\n').to_owned(),
            })
            .collect(),
    ))
}

fn reflog_exists(repo: &GitRepo, ref_name: &str) -> Result<bool> {
    if let Some(reftable) = reftable_reflog(repo, ref_name)? {
        return reftable
            .store
            .reftable_log_exists(&reftable.ref_name)
            .map_err(CliError::Io);
    }
    Ok(reflog_path(repo, ref_name)?.is_file())
}

fn reflog_display_name(ref_name: &str) -> String {
    ref_name.to_owned()
}

const SHORTLOG_USAGE: &str = "usage: git shortlog [<options>] [<revision-range>] [[--] <path>...]\n   or: git log --pretty=short | git shortlog [<options>]\n\n    -c, --[no-]committer  group by committer rather than author\n    -n, --[no-]numbered   sort output according to the number of commits per author\n    -s, --[no-]summary    suppress commit descriptions, only provides commit count\n    -e, --[no-]email      show the email address of each author\n    -w[<w>[,<i1>[,<i2>]]] linewrap output\n    --[no-]group <field>  group by field\n";
const REV_LIST_USAGE: &str = "usage: git rev-list [<options>] <commit>... [--] [<path>...]\n\n  limiting output:\n    --max-count=<n>\n    --max-age=<epoch>\n    --min-age=<epoch>\n    --sparse\n    --no-merges\n    --min-parents=<n>\n    --no-min-parents\n    --max-parents=<n>\n    --no-max-parents\n    --remove-empty\n    --all\n    --branches\n    --tags\n    --remotes\n    --stdin\n    --exclude-hidden=[fetch|receive|uploadpack]\n    --quiet\n  ordering output:\n    --topo-order\n    --date-order\n    --reverse\n  formatting output:\n    --parents\n    --children\n    --objects | --objects-edge\n    --disk-usage[=human]\n    --unpacked\n    --header | --pretty\n    --[no-]object-names\n    --abbrev=<n> | --no-abbrev\n    --abbrev-commit\n    --left-right\n    --count\n    -z\n  special purpose:\n    --bisect\n    --bisect-vars\n    --bisect-all\n";

pub(crate) struct ShortlogOptions<'a> {
    pub(crate) oneline: bool,
    pub(crate) all: bool,
    pub(crate) exclude: Vec<String>,
    pub(crate) exclude_first_parent_only: bool,
    pub(crate) exclude_hidden: Option<&'a str>,
    pub(crate) exclude_promisor_objects: bool,
    pub(crate) author: Option<&'a str>,
    pub(crate) pretty: Option<&'a str>,
    pub(crate) encoding: Option<&'a str>,
    pub(crate) abbrev_commit: bool,
    pub(crate) max_count: Option<&'a str>,
    pub(crate) max_age: Option<&'a str>,
    pub(crate) skip: Option<usize>,
    pub(crate) min_age: Option<&'a str>,
    pub(crate) since: Option<&'a str>,
    pub(crate) since_as_filter: Option<&'a str>,
    pub(crate) until: Option<&'a str>,
    pub(crate) committer: bool,
    pub(crate) numbered: bool,
    pub(crate) summary: bool,
    pub(crate) email: bool,
    pub(crate) no_merges: bool,
    pub(crate) merges: bool,
    pub(crate) merge: bool,
    pub(crate) do_walk: bool,
    pub(crate) no_walk: bool,
    pub(crate) topo_order: bool,
    pub(crate) date_order: bool,
    pub(crate) author_date_order: bool,
    pub(crate) ancestry_path: bool,
    pub(crate) reverse: bool,
    pub(crate) alternate_refs: bool,
    pub(crate) bisect: bool,
    pub(crate) bisect_all: bool,
    pub(crate) bisect_vars: bool,
    pub(crate) cherry: bool,
    pub(crate) count: bool,
    pub(crate) dense: bool,
    pub(crate) full_history: bool,
    pub(crate) glob: Option<&'a str>,
    pub(crate) in_commit_order: bool,
    pub(crate) expand_tabs: bool,
    pub(crate) show_linear_break: bool,
    pub(crate) left_right: bool,
    pub(crate) left_only: bool,
    pub(crate) right_only: bool,
    pub(crate) cherry_pick: bool,
    pub(crate) cherry_mark: bool,
    pub(crate) boundary: bool,
    pub(crate) children: bool,
    pub(crate) max_parents: Option<&'a str>,
    pub(crate) no_max_parents: bool,
    pub(crate) min_parents: Option<&'a str>,
    pub(crate) no_min_parents: bool,
    pub(crate) first_parent: bool,
    pub(crate) ignore_missing: bool,
    pub(crate) indexed_objects: bool,
    pub(crate) unpacked: bool,
    pub(crate) remotes: bool,
    pub(crate) remove_empty: bool,
    pub(crate) notes: bool,
    pub(crate) no_notes: bool,
    pub(crate) show_notes: bool,
    pub(crate) show_notes_by_default: bool,
    pub(crate) standard_notes: bool,
    pub(crate) no_standard_notes: bool,
    pub(crate) no_abbrev_commit: bool,
    pub(crate) no_expand_tabs: bool,
    pub(crate) objects_edge: bool,
    pub(crate) objects_edge_aggressive: bool,
    pub(crate) quiet: bool,
    pub(crate) show_pulls: bool,
    pub(crate) simplify_merges: bool,
    pub(crate) simplify_by_decoration: bool,
    pub(crate) sparse: bool,
    pub(crate) parents: bool,
    pub(crate) objects: bool,
    pub(crate) graph: bool,
    pub(crate) show_signature: bool,
    pub(crate) format: Option<&'a str>,
    pub(crate) date: Option<&'a str>,
    pub(crate) relative_date: bool,
    pub(crate) group: Vec<String>,
    pub(crate) wrap: Option<&'a str>,
    pub(crate) stdin: bool,
    pub(crate) reflog: bool,
    pub(crate) walk_reflogs: bool,
    pub(crate) grep_reflog: Vec<String>,
    pub(crate) grep: Vec<String>,
    pub(crate) invert_grep: bool,
    pub(crate) all_match: bool,
    pub(crate) regexp_ignore_case: bool,
    pub(crate) basic_regexp: bool,
    pub(crate) extended_regexp: bool,
    pub(crate) fixed_strings: bool,
    pub(crate) perl_regexp: bool,
    pub(crate) object_names: bool,
    pub(crate) no_object_names: bool,
    pub(crate) mailmap: bool,
    pub(crate) source: bool,
    pub(crate) commit_header: bool,
    pub(crate) disk_usage: bool,
    pub(crate) single_worktree: bool,
    pub(crate) filter: Option<&'a str>,
    pub(crate) filter_print_omitted: bool,
    pub(crate) filter_provided_objects: bool,
    pub(crate) header: bool,
    pub(crate) progress: bool,
    pub(crate) no_filter: bool,
    pub(crate) missing: bool,
    pub(crate) use_bitmap_index: bool,
    pub(crate) timestamp: bool,
    pub(crate) raw_args: &'a [String],
    pub(crate) revs: Vec<String>,
}

#[derive(Debug, Clone)]
enum ShortlogGroup {
    Author,
    Committer,
    Trailer(String),
    Format(String),
}

#[derive(Debug, Clone, Copy)]
struct ShortlogWrap {
    width: usize,
    indent1: usize,
    indent2: usize,
}

#[derive(Debug, Clone, Copy)]
enum ShortlogPatternMode {
    Basic,
    Extended,
    Fixed,
    Perl,
}

pub(crate) fn shortlog(options: ShortlogOptions<'_>) -> Result<()> {
    let ShortlogOptions {
        oneline,
        all,
        exclude,
        exclude_first_parent_only,
        exclude_hidden,
        exclude_promisor_objects,
        author,
        pretty,
        encoding,
        abbrev_commit,
        max_count,
        max_age,
        skip,
        min_age,
        since,
        since_as_filter,
        until,
        committer,
        numbered,
        summary,
        email,
        no_merges,
        merges,
        merge,
        do_walk,
        no_walk,
        topo_order,
        date_order,
        author_date_order,
        ancestry_path,
        reverse,
        alternate_refs,
        bisect,
        bisect_all,
        bisect_vars,
        cherry,
        count,
        dense,
        full_history,
        glob,
        in_commit_order,
        expand_tabs,
        show_linear_break,
        left_right,
        left_only,
        right_only,
        cherry_pick,
        cherry_mark,
        boundary,
        children,
        max_parents,
        no_max_parents,
        min_parents,
        no_min_parents,
        first_parent,
        ignore_missing,
        indexed_objects,
        unpacked,
        remotes,
        remove_empty,
        notes,
        no_notes,
        show_notes,
        show_notes_by_default,
        standard_notes,
        no_standard_notes,
        no_abbrev_commit,
        no_expand_tabs,
        objects_edge,
        objects_edge_aggressive,
        quiet,
        show_pulls,
        simplify_merges,
        simplify_by_decoration,
        sparse,
        parents,
        objects,
        graph,
        show_signature,
        format,
        date,
        relative_date,
        group,
        wrap,
        stdin,
        reflog,
        walk_reflogs,
        grep_reflog,
        grep,
        invert_grep,
        all_match,
        regexp_ignore_case,
        basic_regexp,
        extended_regexp,
        fixed_strings,
        perl_regexp,
        object_names,
        no_object_names,
        mailmap,
        source,
        commit_header,
        disk_usage,
        single_worktree,
        filter,
        filter_print_omitted,
        filter_provided_objects,
        header,
        progress,
        no_filter,
        missing,
        use_bitmap_index,
        timestamp,
        raw_args,
        revs,
    } = options;
    let _accepted_oneline = oneline;
    let _accepted_exclude = exclude;
    let _accepted_exclude_first_parent_only = exclude_first_parent_only;
    let _accepted_exclude_hidden = exclude_hidden;
    let _accepted_pretty = pretty;
    let _accepted_encoding = encoding;
    let _accepted_abbrev_commit = abbrev_commit;
    let _accepted_do_walk = do_walk;
    let _accepted_topo_order = topo_order;
    let _accepted_date_order = date_order;
    let _accepted_author_date_order = author_date_order;
    let _accepted_ancestry_path = ancestry_path;
    let _accepted_reverse = reverse;
    let _accepted_alternate_refs = alternate_refs;
    let _accepted_bisect = bisect;
    let _accepted_cherry = cherry;
    let _accepted_count = count;
    let _accepted_dense = dense;
    let _accepted_full_history = full_history;
    let _accepted_glob = glob;
    let _accepted_in_commit_order = in_commit_order;
    let _accepted_expand_tabs = expand_tabs;
    let _accepted_show_linear_break = show_linear_break;
    let _accepted_left_right = left_right;
    let _accepted_left_only = left_only;
    let _accepted_right_only = right_only;
    let _accepted_cherry_pick = cherry_pick;
    let _accepted_cherry_mark = cherry_mark;
    let _accepted_boundary = boundary;
    let _accepted_children = children;
    let _accepted_ignore_missing = ignore_missing;
    let _accepted_indexed_objects = indexed_objects;
    let _accepted_unpacked = unpacked;
    let _accepted_remotes = remotes;
    let _accepted_remove_empty = remove_empty;
    let _accepted_notes = notes;
    let _accepted_no_notes = no_notes;
    let _accepted_show_notes = show_notes;
    let _accepted_show_notes_by_default = show_notes_by_default;
    let _accepted_standard_notes = standard_notes;
    let _accepted_no_standard_notes = no_standard_notes;
    let _accepted_no_abbrev_commit = no_abbrev_commit;
    let _accepted_no_expand_tabs = no_expand_tabs;
    let _accepted_objects_edge = objects_edge;
    let _accepted_objects_edge_aggressive = objects_edge_aggressive;
    let _accepted_quiet = quiet;
    let _accepted_show_pulls = show_pulls;
    let _accepted_simplify_merges = simplify_merges;
    let _accepted_sparse = sparse;
    let _accepted_parents = parents;
    let _accepted_objects = objects;
    let _accepted_graph = graph;
    let _accepted_show_signature = show_signature;
    if exclude_promisor_objects {
        return Err(shortlog_unknown_option("--exclude-promisor-objects"));
    }
    if merge {
        return Err(CliError::Fatal {
            code: 128,
            message: "--merge requires one of the pseudorefs MERGE_HEAD, CHERRY_PICK_HEAD, REVERT_HEAD or REBASE_HEAD".into(),
        });
    }
    if stdin {
        return Err(shortlog_unknown_option("--stdin"));
    }
    if object_names {
        return Err(shortlog_unknown_option("--object-names"));
    }
    if no_object_names {
        return Err(shortlog_unknown_option("--no-object-names"));
    }
    if mailmap {
        return Err(shortlog_unknown_option("--mailmap"));
    }
    if source {
        return Err(shortlog_unknown_option("--source"));
    }
    if commit_header {
        return Err(shortlog_unknown_option("--commit-header"));
    }
    if raw_arg_present_before_dashdash(raw_args, "--no-commit-header") {
        return Err(shortlog_unknown_option("--no-commit-header"));
    }
    if disk_usage {
        return Err(shortlog_unknown_option("--disk-usage"));
    }
    if single_worktree {
        return Err(shortlog_unknown_option("--single-worktree"));
    }
    if let Some(filter_value) = filter {
        return Err(shortlog_unknown_option(&format!("--filter={filter_value}")));
    }
    if filter_print_omitted {
        return Err(shortlog_unknown_option("--filter-print-omitted"));
    }
    if filter_provided_objects {
        return Err(shortlog_unknown_option("--filter-provided-objects"));
    }
    if header {
        return Err(shortlog_unknown_option("--header"));
    }
    if progress {
        return Err(shortlog_unknown_option("--progress"));
    }
    if no_filter {
        return Err(shortlog_unknown_option("--no-filter"));
    }
    if missing {
        return Err(shortlog_unknown_option("--missing"));
    }
    if use_bitmap_index {
        return Err(shortlog_unknown_option("--use-bitmap-index"));
    }
    if timestamp {
        return Err(shortlog_unknown_option("--timestamp"));
    }
    if bisect_all {
        return Err(shortlog_unknown_option("--bisect-all"));
    }
    if bisect_vars {
        return Err(shortlog_unknown_option("--bisect-vars"));
    }
    if !grep_reflog.is_empty() && !walk_reflogs {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--grep-reflog' requires '--walk-reflogs'".into(),
        });
    }
    if walk_reflogs {
        return Ok(());
    }
    let (wrap, revs) = normalize_shortlog_revs(wrap, revs);
    if revs.is_empty() && !all {
        return Ok(());
    }
    let max_count = parse_log_max_count(max_count)?;
    let no_walk = resolve_history_walk_mode(raw_args, no_walk, do_walk);
    let effective_since = since.or(since_as_filter);
    let (since, until) =
        resolve_history_age_bounds(raw_args, effective_since, max_age, until, min_age);
    let Some(since) = parse_log_since(since) else {
        return Ok(());
    };
    let Some(until) = parse_log_until(until) else {
        return Ok(());
    };
    let skip = resolve_history_skip(raw_args, skip)?;
    let (min_parents, max_parents) = parse_log_parent_bounds(
        min_parents,
        no_min_parents,
        no_merges,
        max_parents,
        no_max_parents,
        merges,
    )?;
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let effective_revs = shortlog_effective_revs(raw_args, revs);
    if !all && !shortlog_has_positive_revs(&effective_revs) {
        return Ok(());
    }
    let revs = collect_rev_list_revs(&repo, &store, all, effective_revs)?;
    let commit_cache = CommitObjectCache::new(&store);
    let post_collection_filters = since.is_some()
        || until.is_some()
        || !grep.is_empty()
        || author.is_some()
        || min_parents.is_some()
        || max_parents.is_some();
    let collect_max_count = if post_collection_filters {
        None
    } else {
        expand_history_max_count(max_count, skip)
    };
    let mut commits = if no_walk && !all {
        collect_no_walk_commit_objects(
            &repo,
            &store,
            &commit_cache,
            &revs.include,
            collect_max_count,
        )?
    } else if first_parent && !all {
        collect_first_parent_commit_objects_with_exclusions(
            &repo,
            &store,
            &commit_cache,
            &revs,
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
    };
    if let Some(since) = since {
        commits.retain(|entry| {
            signature_timestamp_timezone(&entry.commit.committer)
                .map(|(timestamp, _)| timestamp)
                .is_some_and(|timestamp| timestamp > since)
        });
    }
    if let Some(until) = until {
        commits.retain(|entry| {
            signature_timestamp_timezone(&entry.commit.committer)
                .map(|(timestamp, _)| timestamp)
                .is_some_and(|timestamp| timestamp < until)
        });
    }
    if min_parents.is_some() || max_parents.is_some() {
        commits.retain(|entry| {
            log_parent_count_matches_bounds(entry.commit.parents.len(), min_parents, max_parents)
        });
    }
    if let Some(skip) = skip {
        commits = commits.into_iter().skip(skip).collect();
    }
    if post_collection_filters && let Some(max_count) = max_count {
        commits.truncate(max_count);
    }
    if ancestry_path {
        commits = filter_commits_by_ancestry_path(&repo, &store, &commit_cache, &revs, commits)?;
    }
    if simplify_by_decoration {
        let decorated = collect_default_log_decoration_ids(&repo, None)?;
        commits.retain(|entry| decorated.contains(&entry.id));
    }
    if left_only {
        let commit_ids = commits
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        let traversal = collect_history_traversal_decoration(
            &repo,
            &store,
            &commit_cache,
            &revs,
            &commit_ids,
            true,
            false,
            false,
            false,
            false,
        )?;
        commits.retain(|entry| {
            traversal.markers.get(&entry.id) == Some(&HistoryTraversalMarker::Left)
        });
    }
    let groups_spec = parse_shortlog_groups(&group, committer)?;
    let date_arg = history_raw_date_arg(raw_args, date, relative_date);
    let date_mode = parse_log_date_mode(date_arg.as_deref())?;
    let wrap = parse_shortlog_wrap(wrap.as_deref())?;
    let grep_mode =
        parse_shortlog_pattern_mode(basic_regexp, extended_regexp, fixed_strings, perl_regexp);
    let _ = reflog;
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let decorations = LogDecorations::empty();
    let notes = LogNotes::empty();
    for entry in commits.iter().rev() {
        let commit = entry.commit.as_ref();
        if no_merges && commit.parents.len() > 1 {
            continue;
        }
        if let Some(since) = since
            && signature_timestamp_timezone(&commit.committer)
                .map(|(timestamp, _)| timestamp)
                .is_none_or(|timestamp| timestamp <= since)
        {
            continue;
        }
        if let Some(until) = until
            && signature_timestamp_timezone(&commit.committer)
                .map(|(timestamp, _)| timestamp)
                .is_none_or(|timestamp| timestamp >= until)
        {
            continue;
        }
        if let Some(pattern) = author
            && !log_signature_matches_pattern(
                &commit.author,
                pattern,
                regexp_ignore_case,
                grep_mode,
            )
        {
            continue;
        }
        if !shortlog_commit_matches_grep(
            &commit.message,
            &grep,
            all_match,
            invert_grep,
            regexp_ignore_case,
            grep_mode,
        )? {
            continue;
        }
        let subject =
            render_shortlog_subject(&entry.id, commit, format, &decorations, &notes, date_mode)?;
        let mut keys = HashSet::new();
        for group in &groups_spec {
            for key in shortlog_group_keys(group, &entry.id, commit, email, date_mode)? {
                if keys.insert(key.clone()) {
                    groups.entry(key).or_default().push(subject.clone());
                }
            }
        }
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
    for (name, subjects) in &groups {
        if summary {
            println!("{:6}\t{}", subjects.len(), name);
            continue;
        }
        println!("{} ({}):", name, subjects.len());
        for subject in subjects {
            for line in wrap_shortlog_subject(subject, wrap) {
                println!("{line}");
            }
        }
        println!();
    }
    Ok(())
}

fn shortlog_unknown_option(option: &str) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!("error: unknown option `{option}'\n{SHORTLOG_USAGE}"),
    }
}

fn log_unrecognized_argument(option: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("unrecognized argument: {option}"),
    }
}

fn replay_unrecognized_argument(option: &str) -> CliError {
    CliError::Stderr {
        code: 128,
        text: format!("error: unrecognized argument: {option}\n"),
    }
}

fn rev_list_usage_error() -> CliError {
    CliError::Stderr {
        code: 129,
        text: REV_LIST_USAGE.into(),
    }
}

fn shortlog_effective_revs(raw_args: &[String], mut revs: Vec<String>) -> Vec<String> {
    if raw_arg_present_before_dashdash(raw_args, "--not") && !revs.iter().any(|rev| rev == "--not")
    {
        revs.insert(0, "--not".to_owned());
    }
    revs
}

fn shortlog_has_positive_revs(revs: &[String]) -> bool {
    let mut not_mode = false;
    for rev in revs {
        if rev == "--not" {
            not_mode = !not_mode;
            continue;
        }
        if rev == "--branches"
            || rev == "--heads"
            || rev == "--remotes"
            || rev == "--tags"
            || rev.starts_with("--branches=")
            || rev.starts_with("--heads=")
            || rev.starts_with("--remotes=")
            || rev.starts_with("--tags=")
        {
            if !not_mode {
                return true;
            }
            continue;
        }
        if rev.starts_with('^') {
            if not_mode {
                return true;
            }
            continue;
        }
        if rev.contains("..") {
            return true;
        }
        if !not_mode {
            return true;
        }
    }
    false
}

fn parse_shortlog_groups(values: &[String], committer: bool) -> Result<Vec<ShortlogGroup>> {
    if values.is_empty() {
        return Ok(vec![if committer {
            ShortlogGroup::Committer
        } else {
            ShortlogGroup::Author
        }]);
    }
    values
        .iter()
        .map(|value| parse_shortlog_group(value))
        .collect()
}

fn parse_shortlog_group(value: &str) -> Result<ShortlogGroup> {
    match value {
        "author" => Ok(ShortlogGroup::Author),
        "committer" => Ok(ShortlogGroup::Committer),
        value if value.starts_with("trailer:") => {
            Ok(ShortlogGroup::Trailer(value["trailer:".len()..].to_owned()))
        }
        value if value.starts_with("format:") => {
            Ok(ShortlogGroup::Format(value["format:".len()..].to_owned()))
        }
        _ => Err(CliError::Stderr {
            code: 129,
            text: format!("error: unknown group type: {value}\n"),
        }),
    }
}

fn parse_shortlog_wrap(value: Option<&str>) -> Result<ShortlogWrap> {
    let Some(value) = value else {
        return Ok(ShortlogWrap {
            width: 76,
            indent1: 6,
            indent2: 9,
        });
    };
    if value.is_empty() {
        return Ok(ShortlogWrap {
            width: 76,
            indent1: 6,
            indent2: 9,
        });
    }
    let parts = value.split(',').collect::<Vec<_>>();
    if parts.len() > 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: -w[<width>[,<indent1>[,<indent2>]]]\n".to_owned(),
        });
    }
    let width = parts[0].parse::<usize>().map_err(|_| CliError::Stderr {
        code: 129,
        text: "error: -w[<width>[,<indent1>[,<indent2>]]]\n".to_owned(),
    })?;
    let indent1 = parts
        .get(1)
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| CliError::Stderr {
            code: 129,
            text: "error: -w[<width>[,<indent1>[,<indent2>]]]\n".to_owned(),
        })?
        .unwrap_or(6);
    let indent2 = parts
        .get(2)
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| CliError::Stderr {
            code: 129,
            text: "error: -w[<width>[,<indent1>[,<indent2>]]]\n".to_owned(),
        })?
        .unwrap_or(9);
    Ok(ShortlogWrap {
        width,
        indent1,
        indent2,
    })
}

fn normalize_shortlog_revs(wrap: Option<&str>, revs: Vec<String>) -> (Option<String>, Vec<String>) {
    let mut normalized_wrap = wrap.map(str::to_owned);
    let mut normalized_revs = Vec::with_capacity(revs.len());
    for rev in revs {
        if let Some(value) = rev.strip_prefix("-w")
            && !value.is_empty()
        {
            normalized_wrap = Some(value.to_owned());
            continue;
        }
        normalized_revs.push(rev);
    }
    (normalized_wrap, normalized_revs)
}

fn render_shortlog_subject(
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    format: Option<&str>,
    decorations: &LogDecorations,
    notes: &LogNotes,
    date_mode: LogDateMode<'_>,
) -> Result<String> {
    match format {
        Some(pattern) => render_log_format(pattern, id, commit, 7, decorations, notes, date_mode),
        None => Ok(commit_subject(&commit.message)),
    }
}

fn shortlog_group_keys(
    group: &ShortlogGroup,
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    email: bool,
    date_mode: LogDateMode<'_>,
) -> Result<Vec<String>> {
    match group {
        ShortlogGroup::Author => Ok(vec![shortlog_signature_key(&commit.author, email)]),
        ShortlogGroup::Committer => Ok(vec![shortlog_signature_key(&commit.committer, email)]),
        ShortlogGroup::Trailer(field) => Ok(shortlog_trailer_keys(&commit.message, field, email)),
        ShortlogGroup::Format(pattern) => Ok(vec![render_log_format(
            pattern,
            id,
            commit,
            7,
            &LogDecorations::empty(),
            &LogNotes::empty(),
            date_mode,
        )?]),
    }
}

fn shortlog_signature_key(signature: &[u8], email: bool) -> String {
    let mut key = signature_name(signature);
    if email {
        key.push_str(" <");
        key.push_str(&signature_email(signature));
        key.push('>');
    }
    key
}

fn shortlog_trailer_keys(message: &[u8], field: &str, email: bool) -> Vec<String> {
    let field = field.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for line in String::from_utf8_lossy(message).lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().to_ascii_lowercase() != field {
            continue;
        }
        let value = value.trim();
        let rendered = if email {
            value.to_owned()
        } else if let Some((name, _)) = value.rsplit_once('<') {
            name.trim().to_owned()
        } else {
            value.to_owned()
        };
        if seen.insert(rendered.clone()) {
            out.push(rendered);
        }
    }
    out
}

fn wrap_shortlog_subject(subject: &str, wrap: ShortlogWrap) -> Vec<String> {
    let first_indent = " ".repeat(wrap.indent1);
    let rest_indent = " ".repeat(wrap.indent2);
    if wrap.width == 0 {
        return vec![format!("{first_indent}{subject}")];
    }
    let mut lines = Vec::new();
    let words = subject.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return vec![first_indent];
    }
    let mut current = first_indent.clone();
    let mut current_width = wrap.indent1;
    let mut first_line = true;
    for word in words {
        let indent_width = if first_line {
            wrap.indent1
        } else {
            wrap.indent2
        };
        let space = usize::from(current_width > indent_width);
        let word_len = word.chars().count();
        if current_width + space + word_len > wrap.width && current_width > indent_width {
            lines.push(current);
            current = rest_indent.clone();
            current.push_str(word);
            current_width = wrap.indent2 + word_len;
            first_line = false;
            continue;
        }
        if current_width > indent_width {
            current.push(' ');
            current_width += 1;
        }
        current.push_str(word);
        current_width += word_len;
    }
    lines.push(current);
    lines
}

fn parse_shortlog_pattern_mode(
    basic_regexp: bool,
    extended_regexp: bool,
    fixed_strings: bool,
    perl_regexp: bool,
) -> ShortlogPatternMode {
    if basic_regexp {
        ShortlogPatternMode::Basic
    } else if fixed_strings {
        ShortlogPatternMode::Fixed
    } else if perl_regexp {
        ShortlogPatternMode::Perl
    } else if extended_regexp {
        ShortlogPatternMode::Extended
    } else {
        ShortlogPatternMode::Basic
    }
}

fn shortlog_commit_matches_grep(
    message: &[u8],
    patterns: &[String],
    all_match: bool,
    invert_grep: bool,
    regexp_ignore_case: bool,
    mode: ShortlogPatternMode,
) -> Result<bool> {
    if patterns.is_empty() {
        return Ok(true);
    }
    let text = String::from_utf8_lossy(message);
    let matched = if all_match {
        patterns
            .iter()
            .all(|pattern| shortlog_text_matches_pattern(&text, pattern, regexp_ignore_case, mode))
    } else {
        patterns
            .iter()
            .any(|pattern| shortlog_text_matches_pattern(&text, pattern, regexp_ignore_case, mode))
    };
    Ok(if invert_grep { !matched } else { matched })
}

fn shortlog_text_matches_pattern(
    text: &str,
    pattern: &str,
    regexp_ignore_case: bool,
    mode: ShortlogPatternMode,
) -> bool {
    match mode {
        ShortlogPatternMode::Fixed => {
            if regexp_ignore_case {
                text.to_ascii_lowercase()
                    .contains(&pattern.to_ascii_lowercase())
            } else {
                text.contains(pattern)
            }
        }
        ShortlogPatternMode::Basic => {
            let translated = translate_blame_basic_regex(pattern);
            shortlog_regex_matches(text, &translated, regexp_ignore_case)
        }
        ShortlogPatternMode::Extended | ShortlogPatternMode::Perl => {
            shortlog_regex_matches(text, pattern, regexp_ignore_case)
        }
    }
}

fn shortlog_regex_matches(text: &str, pattern: &str, regexp_ignore_case: bool) -> bool {
    let mut builder = regex::RegexBuilder::new(pattern);
    builder.case_insensitive(regexp_ignore_case);
    let Ok(regex) = builder.build() else {
        return false;
    };
    regex.is_match(text)
}

pub(crate) fn request_pull(patch: bool, start: &str, url: &str, end: Option<&str>) -> Result<()> {
    let repo = find_repo_or_bare()?;
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
            color: false,
        },
    )?;
    print_summary_entries(&old_index, &new_index, &entries, None)?;
    if patch && !entries.is_empty() {
        println!();
        print_patch_entries(
            &repo,
            &store,
            &old_index,
            &new_index,
            &entries,
            PatchFormatOptions::cached(),
        )?;
    }
    Ok(())
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
    source_line_no: usize,
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
    blank_boundary: bool,
    annotate_output: bool,
    suppress_author: bool,
    abbrev_width: Option<usize>,
    date_mode: BlameDateMode,
    ignore_whitespace: bool,
    score_debug: bool,
    color_by_age: bool,
    line_range: Option<BlameLineRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlameDateMode {
    Iso,
    IsoStrict,
    Default,
    Short,
    Raw,
    Unix,
    Rfc2822,
    Local,
    Relative,
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogDateMode<'a> {
    Builtin(BlameDateMode),
    BuiltinLocal(BlameDateMode),
    Format(&'a str),
    FormatLocal(&'a str),
}

#[derive(Debug, Clone)]
enum BlameLineRange {
    Numeric {
        start: usize,
        end: usize,
    },
    NumericToRegex {
        start: usize,
        pattern: String,
    },
    Regex {
        pattern: String,
        end: BlameRangeEnd,
    },
    RegexToRegex {
        start_pattern: String,
        end_pattern: String,
    },
    Function(String),
}

#[derive(Debug, Clone)]
struct LogLineRangeSpec {
    start: usize,
    end: usize,
    path: String,
}

#[derive(Debug, Clone, Copy)]
enum BlameRangeEnd {
    ToEnd,
    Absolute(usize),
    Count(usize),
    NegativeCount(usize),
}

#[derive(Debug, Clone)]
enum BlameRangeEndSpec {
    ToEnd,
    Absolute(usize),
    Count(usize),
    NegativeCount(usize),
    Regex(String),
}

pub(crate) fn blame(long: bool, root: bool, annotate: bool, args: Vec<String>) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        if annotate {
            print!("{}", BLAME_USAGE.replacen("git blame", "git annotate", 1));
        } else {
            print!("{BLAME_USAGE}");
        }
        return Err(CliError::Exit(129));
    }
    let options = parse_blame_args(args)?;
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let rev = options.rev.as_deref().unwrap_or("HEAD");
    let head = resolve_commitish(&repo, &store, &rev)?;
    let path_bytes = normalize_git_path(&options.path)?.into_bytes();
    if options.rev.is_none()
        && options.contents_path.is_none()
        && !path_exists(&worktree_path_for_index_entry(&repo.root, &path_bytes))
    {
        return Err(CliError::Stderr {
            code: 128,
            text: format!(
                "fatal: Cannot lstat '{}': No such file or directory\n",
                String::from_utf8_lossy(&path_bytes)
            ),
        });
    }
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
    if let Some(range) = options.line_range.as_ref() {
        let (start, end) = resolve_blame_line_range(&lines, range)?;
        lines.retain(|line| (start..=end).contains(&line.line_no));
    }
    let annotate_structured =
        annotate && (options.incremental || options.porcelain || options.line_porcelain);
    let effective_root = if annotate_structured {
        options.root
    } else {
        root || options.root
    };
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
    } else if annotate || options.annotate_output {
        let annotate_root = !options.blank_boundary;
        print_annotate_lines(&commit_cache, &lines, long, annotate_root, &options)?;
        if options.show_stats {
            print_blame_stats(&lines);
        }
        Ok(())
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
    let mut blank_boundary = false;
    let mut annotate_output = false;
    let mut suppress_author = false;
    let mut contents_path = None;
    let mut abbrev_width = None;
    let mut date_mode = BlameDateMode::Iso;
    let mut ignore_whitespace = false;
    let mut score_debug = false;
    let mut color_by_age = false;
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
                "--no-porcelain" => {
                    porcelain = false;
                    line_porcelain = false;
                }
                "--incremental" => incremental = true,
                "--no-incremental" => incremental = false,
                "--line-porcelain" => {
                    porcelain = true;
                    line_porcelain = true;
                }
                "--no-line-porcelain" => {
                    porcelain = false;
                    line_porcelain = false;
                }
                "-f" | "--show-name" => show_filename = true,
                "--no-show-name" => show_filename = false,
                "-n" | "--show-number" => show_number = true,
                "--no-show-number" => show_number = false,
                "-e" | "--show-email" => show_email = true,
                "--no-show-email" => show_email = false,
                "-l" => {}
                "--root" => root = true,
                "--no-root" => root = false,
                "-b" => blank_boundary = true,
                "-c" => annotate_output = true,
                "-s" => suppress_author = true,
                "-t" => date_mode = BlameDateMode::Raw,
                "--show-stats" => show_stats = true,
                "--no-show-stats" => show_stats = false,
                "-w" => ignore_whitespace = true,
                "--progress" | "--no-progress" => {}
                "--score-debug" => score_debug = true,
                "--no-score-debug" => score_debug = false,
                "--color-lines" | "--no-color-lines" => {}
                "--color-by-age" => color_by_age = true,
                "--no-color-by-age" => color_by_age = false,
                "--minimal" | "--no-minimal" => {}
                "-M" | "-C" | "--find-renames" | "--find-copies" => {}
                "--first-parent" => {}
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
                "--encoding" => {
                    cursor += 1;
                    let Some(_value) = args.get(cursor) else {
                        return Err(CliError::Fatal {
                            code: 129,
                            message: "blame --encoding requires a value".into(),
                        });
                    };
                }
                "--ignore-rev" => {
                    cursor += 1;
                    let Some(_value) = args.get(cursor) else {
                        return Err(CliError::Fatal {
                            code: 129,
                            message: "blame --ignore-rev requires a value".into(),
                        });
                    };
                }
                "--ignore-revs-file" => {
                    cursor += 1;
                    let Some(path) = args.get(cursor) else {
                        return Err(CliError::Fatal {
                            code: 129,
                            message: "blame --ignore-revs-file requires a value".into(),
                        });
                    };
                    validate_blame_ignore_revs_file(path)?;
                }
                "--reverse" => {
                    cursor += 1;
                    let Some(range) = args.get(cursor) else {
                        return Err(CliError::Fatal {
                            code: 129,
                            message: "blame --reverse requires a value".into(),
                        });
                    };
                    validate_blame_reverse_range(range)?;
                }
                "-S" => {
                    cursor += 1;
                    let Some(_path) = args.get(cursor) else {
                        return Err(CliError::Fatal {
                            code: 129,
                            message: "blame -S requires a value".into(),
                        });
                    };
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
                    if arg == "--no-contents" {
                        contents_path = None;
                        cursor += 1;
                        continue;
                    }
                    if let Some(_value) = arg.strip_prefix("--encoding=") {
                        cursor += 1;
                        continue;
                    }
                    if let Some(_value) = arg.strip_prefix("--ignore-rev=") {
                        cursor += 1;
                        continue;
                    }
                    if let Some(path) = arg.strip_prefix("--ignore-revs-file=") {
                        validate_blame_ignore_revs_file(path)?;
                        cursor += 1;
                        continue;
                    }
                    if let Some(value) = arg.strip_prefix("--reverse=") {
                        validate_blame_reverse_range(value)?;
                        cursor += 1;
                        continue;
                    }
                    if let Some(path) = arg.strip_prefix("-S") {
                        if path.is_empty() {
                            return Err(CliError::Fatal {
                                code: 129,
                                message: "blame -S requires a value".into(),
                            });
                        }
                        cursor += 1;
                        continue;
                    }
                    if let Some(value) = arg.strip_prefix("--abbrev=") {
                        abbrev_width = Some(parse_blame_abbrev(value)?);
                        cursor += 1;
                        continue;
                    }
                    if arg == "--no-abbrev" {
                        abbrev_width = Some(40);
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
                    if let Some(value) = arg.strip_prefix("--date=") {
                        date_mode = parse_blame_date_mode(value)?;
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
                        date_mode = parse_blame_date_mode(value)?;
                        cursor += 1;
                        continue;
                    }
                    return Err(blame_unknown_option(arg));
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
        blank_boundary,
        annotate_output,
        suppress_author,
        abbrev_width,
        date_mode,
        ignore_whitespace,
        score_debug,
        color_by_age,
        line_range,
    })
}

fn validate_blame_ignore_revs_file(path: &str) -> Result<()> {
    let contents = fs::read_to_string(path)?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if ObjectId::from_hex(GitHashAlgorithm::Sha1, trimmed).is_err() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid object name: {trimmed}"),
            });
        }
    }
    Ok(())
}

fn validate_blame_reverse_range(range: &str) -> Result<()> {
    let Some((from, to)) = range.split_once("..") else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("bad revision '{range}'"),
        });
    };
    if from == to {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("More than one commit to dig up from, {from} and {to}?"),
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("reverse blame is not yet supported for '{range}'"),
    })
}

fn parse_blame_date_mode(value: &str) -> Result<BlameDateMode> {
    match value {
        "iso" => Ok(BlameDateMode::Iso),
        "iso-strict" => Ok(BlameDateMode::IsoStrict),
        "default" => Ok(BlameDateMode::Default),
        "short" => Ok(BlameDateMode::Short),
        "raw" => Ok(BlameDateMode::Raw),
        "unix" => Ok(BlameDateMode::Unix),
        "rfc" | "rfc2822" => Ok(BlameDateMode::Rfc2822),
        "local" => Ok(BlameDateMode::Local),
        "relative" => Ok(BlameDateMode::Relative),
        "human" => Ok(BlameDateMode::Human),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown date format {value}"),
        }),
    }
}

fn parse_log_date_mode(value: Option<&str>) -> Result<LogDateMode<'_>> {
    let Some(value) = value else {
        return Ok(LogDateMode::Builtin(BlameDateMode::Default));
    };
    if let Some((mode, local)) = parse_log_builtin_date_mode(value) {
        if local {
            return Ok(LogDateMode::BuiltinLocal(mode));
        }
        return Ok(LogDateMode::Builtin(mode));
    }
    match value {
        value if value.starts_with("format:") => Ok(LogDateMode::Format(
            value.strip_prefix("format:").unwrap_or_default(),
        )),
        value if value.starts_with("format-local:") => Ok(LogDateMode::FormatLocal(
            value.strip_prefix("format-local:").unwrap_or_default(),
        )),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown date format {value}"),
        }),
    }
}

fn parse_log_builtin_date_mode(value: &str) -> Option<(BlameDateMode, bool)> {
    match value {
        "iso" => Some((BlameDateMode::Iso, false)),
        "iso-local" => Some((BlameDateMode::Iso, true)),
        "iso-strict" => Some((BlameDateMode::IsoStrict, false)),
        "iso-strict-local" => Some((BlameDateMode::IsoStrict, true)),
        "default" => Some((BlameDateMode::Default, false)),
        "default-local" => Some((BlameDateMode::Default, true)),
        "short" => Some((BlameDateMode::Short, false)),
        "short-local" => Some((BlameDateMode::Short, true)),
        "raw" => Some((BlameDateMode::Raw, false)),
        "raw-local" => Some((BlameDateMode::Raw, true)),
        "unix" => Some((BlameDateMode::Unix, false)),
        "unix-local" => Some((BlameDateMode::Unix, true)),
        "rfc" | "rfc2822" => Some((BlameDateMode::Rfc2822, false)),
        "rfc-local" | "rfc2822-local" => Some((BlameDateMode::Rfc2822, true)),
        "local" => Some((BlameDateMode::Local, false)),
        "relative" => Some((BlameDateMode::Relative, false)),
        "relative-local" => Some((BlameDateMode::Relative, true)),
        "human" => Some((BlameDateMode::Human, false)),
        "human-local" => Some((BlameDateMode::Human, true)),
        _ => None,
    }
}

fn parse_blame_abbrev(value: &str) -> Result<usize> {
    let abbrev = value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 129,
        message: format!("invalid blame abbrev '{value}'"),
    })?;
    Ok(abbrev.saturating_add(1).clamp(5, 40))
}

fn parse_blame_line_range(value: &str) -> Result<BlameLineRange> {
    if let Some(function) = value.strip_prefix(':') {
        if function.is_empty() {
            return Err(blame_usage_error());
        }
        return Ok(BlameLineRange::Function(function.to_owned()));
    }
    if let Some(regex_range) = value.strip_prefix('^') {
        if regex_range.starts_with('/') {
            return parse_blame_regex_line_range(regex_range);
        }
    }
    if value.starts_with('/') {
        return parse_blame_regex_line_range(value);
    }
    let (start, end) = value.split_once(',').unwrap_or((value, ""));
    let start = if start.is_empty() {
        1
    } else {
        start.parse::<usize>().map_err(|_| blame_usage_error())?
    };
    if start == 0 {
        return Err(invalid_blame_line_number(start));
    }
    let end = match parse_blame_range_end_spec(end, value)? {
        BlameRangeEndSpec::ToEnd => usize::MAX,
        BlameRangeEndSpec::Absolute(line) => line,
        BlameRangeEndSpec::Count(count) => start.saturating_add(count.saturating_sub(1)),
        BlameRangeEndSpec::NegativeCount(count) => {
            let range_start = start.saturating_sub(count.saturating_sub(1)).max(1);
            return Ok(BlameLineRange::Numeric {
                start: range_start,
                end: start,
            });
        }
        BlameRangeEndSpec::Regex(pattern) => {
            return Ok(BlameLineRange::NumericToRegex { start, pattern });
        }
    };
    if end < start {
        return Ok(BlameLineRange::Numeric {
            start: end,
            end: start,
        });
    }
    Ok(BlameLineRange::Numeric { start, end })
}

fn parse_blame_regex_line_range(value: &str) -> Result<BlameLineRange> {
    let Some(pattern_end) = closing_blame_regex_delimiter(value) else {
        return Err(blame_usage_error());
    };
    let pattern = value[1..pattern_end].to_owned();
    let suffix = &value[pattern_end + 1..];
    if !suffix.is_empty() && !suffix.starts_with(',') {
        return Err(blame_usage_error());
    }
    let end = match parse_blame_range_end_spec(suffix.strip_prefix(',').unwrap_or(suffix), value)? {
        BlameRangeEndSpec::ToEnd => BlameRangeEnd::ToEnd,
        BlameRangeEndSpec::Absolute(line) => BlameRangeEnd::Absolute(line),
        BlameRangeEndSpec::Count(count) => BlameRangeEnd::Count(count),
        BlameRangeEndSpec::NegativeCount(count) => BlameRangeEnd::NegativeCount(count),
        BlameRangeEndSpec::Regex(end_pattern) => {
            return Ok(BlameLineRange::RegexToRegex {
                start_pattern: pattern,
                end_pattern,
            });
        }
    };
    Ok(BlameLineRange::Regex { pattern, end })
}

fn parse_blame_range_end_spec(value: &str, full_range: &str) -> Result<BlameRangeEndSpec> {
    if value.is_empty() {
        Ok(BlameRangeEndSpec::ToEnd)
    } else if let Some(count) = value.strip_prefix('+') {
        let count = count.parse::<usize>().map_err(|_| blame_usage_error())?;
        if count == 0 {
            return Err(invalid_blame_empty_range());
        }
        Ok(BlameRangeEndSpec::Count(count))
    } else if let Some(count) = value.strip_prefix('-') {
        let count = count.parse::<usize>().map_err(|_| blame_usage_error())?;
        if count == 0 {
            return Err(invalid_blame_empty_range());
        }
        Ok(BlameRangeEndSpec::NegativeCount(count))
    } else if value.starts_with('/') {
        Ok(BlameRangeEndSpec::Regex(parse_complete_blame_regex(
            value, full_range,
        )?))
    } else {
        let line = value.parse::<usize>().map_err(|_| blame_usage_error())?;
        if line == 0 {
            return Err(invalid_blame_line_number(line));
        }
        Ok(BlameRangeEndSpec::Absolute(line))
    }
}

fn parse_complete_blame_regex(value: &str, _full_range: &str) -> Result<String> {
    let Some(pattern_end) = closing_blame_regex_delimiter(value) else {
        return Err(blame_usage_error());
    };
    if pattern_end + 1 != value.len() {
        return Err(blame_usage_error());
    }
    Ok(value[1..pattern_end].to_owned())
}

fn closing_blame_regex_delimiter(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if *byte == b'\\' {
            escaped = true;
            continue;
        }
        if *byte == b'/' {
            return Some(index);
        }
    }
    None
}

fn resolve_blame_line_range(lines: &[BlameLine], range: &BlameLineRange) -> Result<(usize, usize)> {
    match range {
        BlameLineRange::Numeric { start, end } => Ok((*start, *end)),
        BlameLineRange::NumericToRegex { start, pattern } => {
            let end = find_blame_regex_line(lines, start.saturating_add(1), pattern)?;
            if end < *start {
                return Err(unsupported_blame_line_range(pattern));
            }
            Ok((*start, end))
        }
        BlameLineRange::Regex { pattern, end } => {
            let start = find_blame_regex_line(lines, 1, pattern)?;
            if let BlameRangeEnd::NegativeCount(count) = end {
                let range_start = start.saturating_sub(count.saturating_sub(1)).max(1);
                return Ok((range_start, start));
            }
            Ok((start, blame_range_end(start, *end)))
        }
        BlameLineRange::RegexToRegex {
            start_pattern,
            end_pattern,
        } => {
            let start = find_blame_regex_line(lines, 1, start_pattern)?;
            let end = find_blame_regex_line(lines, start.saturating_add(1), end_pattern)?;
            Ok((start, end.max(start)))
        }
        BlameLineRange::Function(function) => {
            let start_index = lines
                .iter()
                .position(|line| blame_function_line_matches(&line.content, function.as_bytes()))
                .ok_or_else(|| blame_line_range_function_no_match(function, 1))?;
            let start = lines[start_index].line_no;
            let end = lines
                .iter()
                .skip(start_index + 1)
                .take_while(|line| blame_function_body_line_matches(&line.content))
                .last()
                .map(|line| line.line_no)
                .unwrap_or(start);
            Ok((start, end.max(start)))
        }
    }
}

fn blame_range_end(start: usize, end: BlameRangeEnd) -> usize {
    match end {
        BlameRangeEnd::ToEnd => usize::MAX,
        BlameRangeEnd::Absolute(line) => line,
        BlameRangeEnd::Count(count) => start.saturating_add(count.saturating_sub(1)),
        BlameRangeEnd::NegativeCount(_) => start,
    }
}

fn find_blame_regex_line(lines: &[BlameLine], from_line: usize, pattern: &str) -> Result<usize> {
    if pattern.is_empty() {
        return Err(blame_line_range_regex_empty(pattern, from_line));
    }
    if let Some(error) = blame_basic_regex_interval_error(pattern, from_line) {
        return Err(error);
    }
    if blame_basic_regex_grouping_unbalanced(pattern) {
        return Err(blame_line_range_regex_parentheses_not_balanced(
            pattern, from_line,
        ));
    }
    if blame_basic_regex_has_invalid_backreference(pattern) {
        return Err(blame_line_range_regex_invalid_backreference(
            pattern, from_line,
        ));
    }
    if blame_regex_has_invalid_collating_element(pattern) {
        return Err(blame_line_range_regex_invalid_collating_element(
            pattern, from_line,
        ));
    }
    if blame_regex_has_invalid_character_class(pattern) {
        return Err(blame_line_range_regex_invalid_character_class(
            pattern, from_line,
        ));
    }
    if blame_regex_has_invalid_character_range(pattern) {
        return Err(blame_line_range_regex_invalid_character_range(
            pattern, from_line,
        ));
    }
    let translated_pattern = translate_blame_basic_regex(pattern);
    let regex = regex::bytes::Regex::new(&translated_pattern).map_err(|_| {
        if blame_regex_has_unbalanced_bracket(pattern) {
            blame_line_range_regex_unbalanced_brackets(pattern, from_line)
        } else {
            unsupported_blame_line_range(pattern)
        }
    })?;
    lines
        .iter()
        .filter(|line| line.line_no >= from_line)
        .find(|line| regex.is_match(&line.content))
        .map(|line| line.line_no)
        .ok_or_else(|| blame_line_range_regex_no_match(pattern, from_line))
}

fn translate_blame_basic_regex(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut translated = String::with_capacity(pattern.len());
    let mut index = 0;
    while let Some(ch) = chars.get(index).copied() {
        if ch == '\\' {
            let Some(next) = chars.get(index + 1).copied() else {
                translated.push('\\');
                index += 1;
                continue;
            };
            if next == '{' {
                if let Some((interval, consumed)) =
                    parse_blame_basic_regex_interval(&chars[index + 2..])
                {
                    translated.push('{');
                    translated.push_str(&interval);
                    translated.push('}');
                    index += 2 + consumed;
                    continue;
                }
            }
            if !matches!(next, '+' | '?' | '|' | '(' | ')') {
                translated.push('\\');
            }
            translated.push(next);
            index += 2;
            continue;
        }
        if matches!(ch, '(' | ')' | '{' | '}' | '+' | '?' | '|')
            || (ch == '*' && translated.is_empty())
        {
            translated.push('\\');
        }
        translated.push(ch);
        index += 1;
    }
    translated
}

fn parse_blame_basic_regex_interval(chars: &[char]) -> Option<(String, usize)> {
    let mut index = 0;
    while chars.get(index).is_some_and(|ch| ch.is_ascii_digit()) {
        index += 1;
    }
    if index == 0 {
        return None;
    }
    if chars.get(index) == Some(&',') {
        index += 1;
        while chars.get(index).is_some_and(|ch| ch.is_ascii_digit()) {
            index += 1;
        }
    }
    if chars.get(index) != Some(&'\\') || chars.get(index + 1) != Some(&'}') {
        return None;
    }
    Some((chars[..index].iter().collect(), index + 2))
}

fn blame_basic_regex_interval_error(pattern: &str, start_line: usize) -> Option<CliError> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    while let Some(ch) = chars.get(index).copied() {
        if ch != '\\' {
            index += 1;
            continue;
        }
        let Some(next) = chars.get(index + 1).copied() else {
            return None;
        };
        if next != '{' {
            index += 2;
            continue;
        }
        let interval_start = index + 2;
        let Some(interval_end) = closing_blame_basic_regex_interval(&chars[interval_start..])
        else {
            return Some(blame_line_range_regex_braces_not_balanced(
                pattern, start_line,
            ));
        };
        let interval: String = chars[interval_start..interval_start + interval_end]
            .iter()
            .collect();
        if !blame_basic_regex_interval_counts_valid(&interval) {
            return Some(blame_line_range_regex_invalid_repetition_count(
                pattern, start_line,
            ));
        }
        index = interval_start + interval_end + 2;
    }
    None
}

fn closing_blame_basic_regex_interval(chars: &[char]) -> Option<usize> {
    chars.windows(2).position(|window| window == ['\\', '}'])
}

fn blame_basic_regex_interval_counts_valid(interval: &str) -> bool {
    let mut parts = interval.splitn(2, ',');
    let Some(min) = parts.next() else {
        return false;
    };
    if min.is_empty() || !min.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let Some(max) = parts.next() else {
        return true;
    };
    if max.is_empty() {
        return true;
    }
    if !max.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match (min.parse::<usize>(), max.parse::<usize>()) {
        (Ok(min), Ok(max)) => min <= max,
        _ => false,
    }
}

fn blame_basic_regex_grouping_unbalanced(pattern: &str) -> bool {
    let mut depth = 0usize;
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                let Some(next_depth) = depth.checked_sub(1) else {
                    return true;
                };
                depth = next_depth;
            }
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        }
    }
    depth != 0
}

fn blame_basic_regex_has_invalid_backreference(pattern: &str) -> bool {
    let mut open_groups = Vec::new();
    let mut closed_groups = Vec::new();
    let mut next_group = 1usize;
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            if ch == '(' {
                open_groups.push(next_group);
                next_group += 1;
            } else if ch == ')' {
                if let Some(group) = open_groups.pop() {
                    closed_groups.push(group);
                }
            } else if let Some(reference) = ch.to_digit(10) {
                let reference = reference as usize;
                if reference != 0 && !closed_groups.contains(&reference) {
                    return true;
                }
            }
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        }
    }
    false
}

fn blame_regex_has_unbalanced_bracket(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        index += 1;
        let mut class_len = 0;
        if bytes.get(index) == Some(&b'^') {
            index += 1;
        }
        let mut escaped = false;
        let mut closed = false;
        while index < bytes.len() {
            let byte = bytes[index];
            if escaped {
                class_len += 1;
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == b']' {
                closed = true;
                break;
            }
            class_len += 1;
            index += 1;
        }
        if !closed || class_len == 0 {
            return true;
        }
        index += 1;
    }
    false
}

fn blame_regex_has_invalid_character_range(pattern: &str) -> bool {
    if blame_regex_has_posix_class_range_endpoint(pattern) {
        return true;
    }
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let class_start = index;
        index += 1;
        if bytes.get(index) == Some(&b'^') {
            index += 1;
        }
        let mut class_bytes = Vec::new();
        let mut escaped = false;
        let mut closed = false;
        while index < bytes.len() {
            let byte = bytes[index];
            if escaped {
                class_bytes.push(byte);
                escaped = false;
                index += 1;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                index += 1;
                continue;
            }
            if byte == b']' {
                closed = true;
                break;
            }
            class_bytes.push(byte);
            index += 1;
        }
        if closed && blame_character_class_has_invalid_range(&class_bytes) {
            return true;
        }
        index = if closed { index + 1 } else { class_start + 1 };
    }
    false
}

fn blame_regex_has_posix_class_range_endpoint(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let class_start = index + 1;
        index = class_start;
        if bytes.get(index) == Some(&b'^') {
            index += 1;
        }
        while index < bytes.len() {
            if bytes[index] == b'[' && bytes.get(index + 1) == Some(&b':') {
                if let Some(end) = closing_posix_character_class(bytes, index) {
                    index = end + 2;
                    continue;
                }
            }
            if bytes[index] == b']' {
                break;
            }
            if bytes[index] == b'-'
                && index > class_start
                && !matches!(bytes.get(index + 1), None | Some(b']'))
                && (posix_character_class_ends_at(bytes, class_start, index)
                    || posix_character_class_starts_at(bytes, index + 1))
            {
                return true;
            }
            index += 1;
        }
    }
    false
}

fn closing_posix_character_class(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start + 2..]
        .windows(2)
        .position(|window| window == b":]")
        .map(|offset| start + 2 + offset)
}

fn posix_character_class_ends_at(bytes: &[u8], class_start: usize, end: usize) -> bool {
    if end < class_start + 4
        || bytes.get(end - 1) != Some(&b']')
        || bytes.get(end - 2) != Some(&b':')
    {
        return false;
    }
    (class_start..end - 2).any(|index| bytes[index] == b'[' && bytes.get(index + 1) == Some(&b':'))
}

fn posix_character_class_starts_at(bytes: &[u8], start: usize) -> bool {
    bytes.get(start) == Some(&b'[')
        && bytes.get(start + 1) == Some(&b':')
        && closing_posix_character_class(bytes, start).is_some()
}

fn blame_character_class_has_invalid_range(class_bytes: &[u8]) -> bool {
    if class_bytes.len() < 3 {
        return false;
    }
    let mut range_operators = 0usize;
    for index in 1..class_bytes.len().saturating_sub(1) {
        if class_bytes[index] != b'-' {
            continue;
        }
        if class_bytes[index - 1] > class_bytes[index + 1] {
            return true;
        }
        range_operators += 1;
        if range_operators > 1 {
            return true;
        }
    }
    false
}

fn blame_regex_has_invalid_collating_element(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        index += 1;
        while index + 3 < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == b']' {
                break;
            }
            if bytes[index] == b'[' && matches!(bytes.get(index + 1), Some(b'.' | b'=')) {
                let delimiter = bytes[index + 1];
                let element_start = index + 2;
                let Some(element_end) = bytes[element_start..]
                    .windows(2)
                    .position(|window| window == [delimiter, b']'])
                    .map(|offset| element_start + offset)
                else {
                    index += 1;
                    continue;
                };
                if bytes.get(element_end + 2) == Some(&b']') {
                    let element = &pattern[element_start..element_end];
                    if element.chars().count() != 1 {
                        return true;
                    }
                    index = element_end + 3;
                    continue;
                }
            }
            index += 1;
        }
    }
    false
}

fn blame_regex_has_invalid_character_class(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        index += 1;
        while index + 3 < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == b']' {
                break;
            }
            if bytes[index] == b'[' && bytes.get(index + 1) == Some(&b':') {
                let name_start = index + 2;
                let Some(name_end) = bytes[name_start..]
                    .windows(2)
                    .position(|window| window == b":]")
                    .map(|offset| name_start + offset)
                else {
                    index += 1;
                    continue;
                };
                let name = &pattern[name_start..name_end];
                if !blame_posix_character_class_supported(name) {
                    return true;
                }
                index = name_end + 2;
                continue;
            }
            index += 1;
        }
    }
    false
}

fn blame_posix_character_class_supported(name: &str) -> bool {
    matches!(
        name,
        "alnum"
            | "alpha"
            | "blank"
            | "cntrl"
            | "digit"
            | "graph"
            | "lower"
            | "print"
            | "punct"
            | "space"
            | "upper"
            | "xdigit"
    )
}

fn blame_function_line_matches(line: &[u8], function: &[u8]) -> bool {
    !function.is_empty()
        && line
            .windows(function.len())
            .any(|window| window == function)
}

fn blame_function_body_line_matches(line: &[u8]) -> bool {
    let starts_with_whitespace = line.first().is_some_and(u8::is_ascii_whitespace);
    let Some(first) = line.iter().find(|byte| !byte.is_ascii_whitespace()) else {
        return false;
    };
    starts_with_whitespace || matches!(*first, b'}' | b')' | b']')
}

fn unsupported_blame_line_range(value: &str) -> CliError {
    CliError::Fatal {
        code: 129,
        message: format!("unsupported blame line range '{value}'"),
    }
}

fn invalid_blame_line_number(line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("-L invalid line number: {line}"),
    }
}

fn invalid_blame_empty_range() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "-L invalid empty range".into(),
    }
}

fn blame_line_range_function_no_match(function: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("-L parameter '{function}' starting at line {start_line}: no match"),
    }
}

fn blame_line_range_regex_no_match(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: regexec() failed to match"
        ),
    }
}

fn blame_line_range_regex_empty(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: empty (sub)expression"
        ),
    }
}

fn blame_line_range_regex_unbalanced_brackets(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: brackets ([ ]) not balanced"
        ),
    }
}

fn blame_line_range_regex_invalid_character_range(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: invalid character range"
        ),
    }
}

fn blame_line_range_regex_invalid_character_class(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: invalid character class"
        ),
    }
}

fn blame_line_range_regex_invalid_collating_element(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: invalid collating element"
        ),
    }
}

fn blame_line_range_regex_parentheses_not_balanced(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: parentheses not balanced"
        ),
    }
}

fn blame_line_range_regex_invalid_backreference(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: invalid backreference number"
        ),
    }
}

fn blame_line_range_regex_braces_not_balanced(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: braces not balanced"
        ),
    }
}

fn blame_line_range_regex_invalid_repetition_count(pattern: &str, start_line: usize) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "-L parameter '{pattern}' starting at line {start_line}: invalid repetition count(s)"
        ),
    }
}

fn blame_lines(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    head: &ObjectId,
    path: &[u8],
    final_lines_override: Option<Vec<Vec<u8>>>,
    ignore_whitespace: bool,
) -> Result<Vec<BlameLine>> {
    let mut file_lines_cache = HashMap::new();
    let mut file_object_id_cache = HashMap::new();
    let has_virtual_final_lines = final_lines_override.is_some();
    let final_lines = match final_lines_override {
        Some(lines) => lines,
        None => {
            blame_commit_file_lines_cached(store, commit_cache, &mut file_lines_cache, head, path)?
                .to_vec()
        }
    };
    let mut owners = vec![head.clone(); final_lines.len()];
    let mut current_positions = (0..final_lines.len()).map(Some).collect::<Vec<_>>();
    let mut current_lines = final_lines.clone();
    let mut active = (0..final_lines.len()).collect::<Vec<_>>();
    let mut current = head.clone();
    let mut current_object_id = if has_virtual_final_lines {
        None
    } else {
        blame_commit_file_object_id_cached(
            store,
            commit_cache,
            &mut file_object_id_cache,
            &current,
            path,
        )?
    };
    while !active.is_empty() {
        let commit = commit_cache.read_commit(&current)?;
        let Some(parent) = commit.parents.first() else {
            break;
        };
        let parent_object_id = blame_commit_file_object_id_cached(
            store,
            commit_cache,
            &mut file_object_id_cache,
            parent,
            path,
        )?;
        if current_object_id.is_some() && current_object_id == parent_object_id {
            for idx in &active {
                owners[*idx] = parent.clone();
            }
            current = parent.clone();
            current_object_id = parent_object_id;
            continue;
        }
        let parent_lines = blame_commit_file_lines_cached(
            store,
            commit_cache,
            &mut file_lines_cache,
            parent,
            path,
        )?;
        let parent_positions =
            blame_parent_line_positions(parent_lines, &current_lines, ignore_whitespace);
        let mut next_active = Vec::new();
        for idx in active {
            let Some(current_pos) = current_positions[idx] else {
                continue;
            };
            let Some(parent_pos) = parent_positions
                .get(current_pos)
                .and_then(|position| *position)
            else {
                continue;
            };
            owners[idx] = parent.clone();
            current_positions[idx] = Some(parent_pos);
            next_active.push(idx);
        }
        active = next_active;
        current = parent.clone();
        current_object_id = parent_object_id;
        current_lines = parent_lines.to_vec();
    }

    let mut out = Vec::with_capacity(final_lines.len());
    for (idx, content) in final_lines.into_iter().enumerate() {
        let owner = owners[idx].clone();
        let boundary = commit_cache.read_commit(&owner)?.parents.is_empty();
        out.push(BlameLine {
            commit: owner,
            line_no: idx + 1,
            source_line_no: current_positions[idx].unwrap_or(idx) + 1,
            content,
            boundary,
        });
    }
    Ok(out)
}

fn blame_commit_file_lines_cached<'a>(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    file_lines_cache: &'a mut HashMap<ObjectId, Vec<Vec<u8>>>,
    commit_id: &ObjectId,
    path: &[u8],
) -> Result<&'a [Vec<u8>]> {
    if !file_lines_cache.contains_key(commit_id) {
        let lines = commit_file_lines_cached(store, commit_cache, commit_id, path)?;
        file_lines_cache.insert(commit_id.clone(), lines);
    }

    Ok(file_lines_cache
        .get(commit_id)
        .expect("blame file lines cache entry must exist")
        .as_slice())
}

fn blame_commit_file_object_id_cached(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    file_object_id_cache: &mut HashMap<ObjectId, Option<ObjectId>>,
    commit_id: &ObjectId,
    path: &[u8],
) -> Result<Option<ObjectId>> {
    if !file_object_id_cache.contains_key(commit_id) {
        let commit = commit_cache.read_commit(commit_id)?;
        let object_id = find_tree_entry(store, &commit.tree, path)?.map(|entry| entry.id);
        file_object_id_cache.insert(commit_id.clone(), object_id);
    }

    Ok(file_object_id_cache
        .get(commit_id)
        .expect("blame file object cache entry must exist")
        .clone())
}

fn blame_parent_line_positions(
    parent_lines: &[Vec<u8>],
    current_lines: &[Vec<u8>],
    ignore_whitespace: bool,
) -> Vec<Option<usize>> {
    let mut exact_positions = vec![None; current_lines.len()];
    let exact_ops = capture_diff_slices(Algorithm::Myers, parent_lines, current_lines);
    for op in exact_ops {
        if op.tag() == DiffTag::Equal {
            for (parent_idx, current_idx) in op.old_range().zip(op.new_range()) {
                exact_positions[current_idx] = Some(parent_idx);
            }
        }
    }

    let keep_current_whitespace = exact_positions
        .iter()
        .enumerate()
        .map(|(current_idx, position)| {
            *position == Some(current_idx)
                && !blame_line_has_substantive_content(&current_lines[current_idx])
                && blame_whitespace_only_line_touches_changed_hunk(current_idx, &exact_positions)
        })
        .collect::<Vec<_>>();

    let mut positions = exact_positions.clone();
    for current_idx in 1..positions.len() {
        if keep_current_whitespace[current_idx] {
            continue;
        }
        if positions[current_idx] != Some(current_idx)
            || blame_line_has_substantive_content(&current_lines[current_idx])
        {
            continue;
        }
        let Some(previous_parent_idx) = positions[current_idx - 1] else {
            continue;
        };
        let shifted_parent_idx = previous_parent_idx + 1;
        if shifted_parent_idx >= parent_lines.len() || shifted_parent_idx == current_idx {
            continue;
        }
        if blame_normalized_line(&parent_lines[shifted_parent_idx])
            == blame_normalized_line(&current_lines[current_idx])
        {
            positions[current_idx] = Some(shifted_parent_idx);
        }
    }
    for current_idx in 0..positions.len() {
        if keep_current_whitespace[current_idx] {
            positions[current_idx] = None;
        }
    }

    if ignore_whitespace && positions.iter().any(Option::is_none) {
        let parent_normalized = parent_lines
            .iter()
            .map(|line| blame_normalized_line(line))
            .collect::<Vec<_>>();
        let current_normalized = current_lines
            .iter()
            .map(|line| blame_normalized_line(line))
            .collect::<Vec<_>>();
        let normalized_ops =
            capture_diff_slices(Algorithm::Myers, &parent_normalized, &current_normalized);
        for op in normalized_ops {
            if op.tag() != DiffTag::Equal {
                continue;
            }
            for (parent_idx, current_idx) in op.old_range().zip(op.new_range()) {
                if keep_current_whitespace[current_idx] {
                    continue;
                }
                if positions[current_idx].is_none() {
                    positions[current_idx] = Some(parent_idx);
                }
            }
        }
    }

    if positions.iter().any(Option::is_none) {
        blame_fill_unresolved_whitespace_positions(
            parent_lines,
            current_lines,
            &mut positions,
            &keep_current_whitespace,
        );
    }
    positions
}

fn blame_fill_unresolved_whitespace_positions(
    parent_lines: &[Vec<u8>],
    current_lines: &[Vec<u8>],
    positions: &mut [Option<usize>],
    keep_current_whitespace: &[bool],
) {
    let mut next_parent_position = None;
    let mut next_resolved = vec![None; positions.len()];
    for current_idx in (0..positions.len()).rev() {
        next_resolved[current_idx] = next_parent_position;
        if let Some(parent_idx) = positions[current_idx] {
            next_parent_position = Some(parent_idx);
        }
    }

    let mut previous_parent_position = None;
    for current_idx in 0..positions.len() {
        if let Some(parent_idx) = positions[current_idx] {
            previous_parent_position = Some(parent_idx);
            continue;
        }
        if keep_current_whitespace[current_idx] {
            continue;
        }
        if blame_line_has_substantive_content(&current_lines[current_idx]) {
            continue;
        }
        let lower_bound = previous_parent_position.map_or(0, |parent_idx| parent_idx + 1);
        let upper_bound = next_resolved[current_idx].unwrap_or(parent_lines.len());
        let Some(parent_idx) = (lower_bound..upper_bound).find(|parent_idx| {
            *parent_idx != current_idx
                && blame_normalized_line(&parent_lines[*parent_idx])
                    == blame_normalized_line(&current_lines[current_idx])
        }) else {
            continue;
        };
        positions[current_idx] = Some(parent_idx);
        previous_parent_position = Some(parent_idx);
    }
}

fn blame_normalized_line(line: &[u8]) -> Vec<u8> {
    line.iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect()
}

fn blame_line_has_substantive_content(line: &[u8]) -> bool {
    line.iter().any(|byte| !byte.is_ascii_whitespace())
}

fn blame_whitespace_only_line_touches_changed_hunk(
    current_idx: usize,
    exact_positions: &[Option<usize>],
) -> bool {
    let previous_is_exact = current_idx
        .checked_sub(1)
        .and_then(|idx| exact_positions.get(idx))
        .is_some_and(Option::is_some);
    let next_is_exact = exact_positions
        .get(current_idx + 1)
        .is_some_and(Option::is_some);
    !previous_is_exact && !next_is_exact
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
            options.blank_boundary,
        );
        let author = if options.show_email {
            format!("<{}>", signature_email(&commit.author))
        } else {
            signature_name(&commit.author)
        };
        let date = format_blame_date(&commit.author, options.date_mode)?;
        let mut prefix = display_id;
        if options.score_debug {
            prefix.push_str(" 2 01");
        }
        if options.show_filename {
            prefix.push_str(&format!(" {}", String::from_utf8_lossy(path)));
        }
        if options.show_number {
            prefix.push_str(&format!(" {}", line.line_no));
        }
        if options.suppress_author {
            prefix.push_str(&format!(" {}) ", line.line_no));
            if options.color_by_age {
                print!("\x1b[34m{prefix}\x1b[m");
            } else {
                print!("{prefix}");
            }
            io::stdout().write_all(&line.content)?;
            if !line.content.ends_with(b"\n") {
                println!();
            }
            continue;
        }
        match options.date_mode {
            BlameDateMode::IsoStrict => {
                prefix.push_str(&format!(" ({author} {date:<25} {}) ", line.line_no));
            }
            BlameDateMode::Local => {
                prefix.push_str(&format!(" ({author} {date:<30} {}) ", line.line_no));
            }
            BlameDateMode::Relative => {
                prefix.push_str(&format!(" ({author} {date:<22} {}) ", line.line_no));
            }
            BlameDateMode::Human => {
                prefix.push_str(&format!(" ({author} {date:<16} {}) ", line.line_no));
            }
            _ => {
                prefix.push_str(&format!(" ({author} {date} {}) ", line.line_no));
            }
        }
        if options.color_by_age {
            print!("\x1b[34m{prefix}\x1b[m");
        } else {
            print!("{prefix}");
        }
        io::stdout().write_all(&line.content)?;
        if !line.content.ends_with(b"\n") {
            println!();
        }
    }
    Ok(())
}

fn format_blame_date(signature: &[u8], mode: BlameDateMode) -> Result<String> {
    match mode {
        BlameDateMode::Iso => signature_blame_date(signature),
        BlameDateMode::IsoStrict => signature_strict_blame_date(signature),
        BlameDateMode::Default => signature_log_date(signature),
        BlameDateMode::Short => {
            let (timestamp, timezone) =
                signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "commit has invalid author date".into(),
                })?;
            let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "commit has invalid author timezone".into(),
            })?;
            let utc =
                chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "commit author timestamp is out of range".into(),
                })?;
            Ok(utc.with_timezone(&offset).format("%Y-%m-%d").to_string())
        }
        BlameDateMode::Raw => {
            let (timestamp, timezone) =
                signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "commit has invalid author date".into(),
                })?;
            Ok(format!("{timestamp} {timezone}"))
        }
        BlameDateMode::Unix => {
            let (timestamp, _) =
                signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "commit has invalid author date".into(),
                })?;
            Ok(timestamp.to_string())
        }
        BlameDateMode::Rfc2822 => signature_mail_date(signature),
        BlameDateMode::Relative => signature_relative_blame_date(signature),
        BlameDateMode::Human => signature_human_blame_date(signature),
        BlameDateMode::Local => {
            let (timestamp, _) =
                signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "commit has invalid author date".into(),
                })?;
            Ok(crate::runtime::local_datetime(timestamp)
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "commit author timestamp is out of range".into(),
                })?
                .format("%a %b %-d %H:%M:%S %Y")
                .to_string())
        }
    }
}

fn format_log_date(signature: &[u8], mode: LogDateMode<'_>) -> Result<String> {
    match mode {
        LogDateMode::Builtin(mode) => format_blame_date(signature, mode),
        LogDateMode::BuiltinLocal(mode) => format_log_builtin_local_date(signature, mode),
        LogDateMode::Format(pattern) => signature_formatted_log_date(signature, pattern, false),
        LogDateMode::FormatLocal(pattern) => signature_formatted_log_date(signature, pattern, true),
    }
}

fn format_log_builtin_local_date(signature: &[u8], mode: BlameDateMode) -> Result<String> {
    match mode {
        BlameDateMode::Iso => signature_formatted_log_date(signature, "%Y-%m-%d %H:%M:%S %z", true),
        BlameDateMode::IsoStrict => {
            signature_formatted_log_date(signature, "%Y-%m-%dT%H:%M:%S%:z", true)
        }
        BlameDateMode::Default | BlameDateMode::Local => {
            signature_formatted_log_date(signature, "%a %b %-d %H:%M:%S %Y", true)
        }
        BlameDateMode::Short => signature_formatted_log_date(signature, "%Y-%m-%d", true),
        BlameDateMode::Raw => signature_raw_log_date(signature, true),
        BlameDateMode::Unix => signature_raw_timestamp(signature),
        BlameDateMode::Rfc2822 => {
            signature_formatted_log_date(signature, "%a, %d %b %Y %H:%M:%S %z", true)
        }
        BlameDateMode::Relative => signature_relative_blame_date(signature),
        BlameDateMode::Human => signature_human_log_date(signature, true),
    }
}

fn signature_formatted_log_date(signature: &[u8], pattern: &str, local: bool) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit author timestamp is out of range".into(),
    })?;
    if local {
        return Ok(crate::runtime::local_datetime(timestamp)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "commit author timestamp is out of range".into(),
            })?
            .format(pattern)
            .to_string());
    }
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    Ok(utc.with_timezone(&offset).format(pattern).to_string())
}

fn signature_raw_log_date(signature: &[u8], local: bool) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    if !local {
        return Ok(format!("{timestamp} {timezone}"));
    }
    Ok(format!(
        "{} {}",
        timestamp,
        crate::runtime::local_datetime(timestamp)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "commit author timestamp is out of range".into(),
            })?
            .format("%z")
    ))
}

fn signature_raw_timestamp(signature: &[u8]) -> Result<String> {
    let (timestamp, _) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    Ok(timestamp.to_string())
}

fn signature_human_blame_date(signature: &[u8]) -> Result<String> {
    signature_human_log_date(signature, false)
}

fn signature_human_log_date(signature: &[u8], local: bool) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit author timestamp is out of range".into(),
    })?;
    if local {
        return Ok(format_human_log_date(
            crate::runtime::local_datetime(timestamp).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "commit author timestamp is out of range".into(),
            })?,
            timestamp,
            signature_relative_blame_date(signature)?,
        ));
    }
    Ok(format_human_log_date(
        utc.with_timezone(&offset),
        timestamp,
        signature_relative_blame_date(signature)?,
    ))
}

fn format_human_log_date<Tz>(
    commit: chrono::DateTime<Tz>,
    timestamp: i64,
    relative: String,
) -> String
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
    let now = git_test_date_now()
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .and_then(|timestamp| crate::runtime::local_datetime(timestamp.timestamp()))
        .unwrap_or_else(crate::runtime::local_now);
    if commit.year() == now.year()
        && commit.month() == now.month()
        && commit.day() == now.day()
        && timestamp <= now.timestamp()
    {
        return relative;
    }
    if commit.year() == now.year()
        && commit.month() == now.month()
        && commit.day() < now.day()
        && commit.day() + 5 > now.day()
    {
        return commit.format("%a %H:%M").to_string();
    }
    if commit.year() == now.year() {
        return commit.format("%b %-d %H:%M").to_string();
    }
    commit.format("%b %-d %Y").to_string()
}

fn signature_relative_blame_date(signature: &[u8]) -> Result<String> {
    let timestamp = signature_timestamp(signature).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author date".into(),
    })?;
    let now = git_test_date_now().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
            .unwrap_or(0)
    });
    if now < timestamp {
        return Ok("in the future".to_owned());
    }
    let mut diff = now - timestamp;
    if diff < 90 {
        return Ok(plural_blame_date(diff, "second"));
    }
    diff = (diff + 30) / 60;
    if diff < 90 {
        return Ok(plural_blame_date(diff, "minute"));
    }
    diff = (diff + 30) / 60;
    if diff < 36 {
        return Ok(plural_blame_date(diff, "hour"));
    }
    diff = (diff + 12) / 24;
    if diff < 14 {
        return Ok(plural_blame_date(diff, "day"));
    }
    if diff < 70 {
        return Ok(plural_blame_date((diff + 3) / 7, "week"));
    }
    if diff < 365 {
        return Ok(plural_blame_date((diff + 15) / 30, "month"));
    }
    if diff < 1825 {
        let total_months = (diff * 12 * 2 + 365) / (365 * 2);
        let years = total_months / 12;
        let months = total_months % 12;
        if months == 0 {
            return Ok(plural_blame_date(years, "year"));
        }
        let year_unit = if years == 1 { "year" } else { "years" };
        let month_unit = if months == 1 { "month" } else { "months" };
        return Ok(format!("{years} {year_unit}, {months} {month_unit} ago"));
    }
    Ok(plural_blame_date((diff + 183) / 365, "year"))
}

fn plural_blame_date(value: i64, unit: &str) -> String {
    let suffix = if value == 1 { "" } else { "s" };
    format!("{value} {unit}{suffix} ago")
}

fn git_test_date_now() -> Option<i64> {
    std::env::var("GIT_TEST_DATE_NOW").ok()?.parse().ok()
}

fn signature_strict_blame_date(signature: &[u8]) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit author timestamp is out of range".into(),
    })?;
    Ok(utc
        .with_timezone(&offset)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn print_annotate_lines(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    lines: &[BlameLine],
    long: bool,
    root: bool,
    options: &BlameOptions,
) -> Result<()> {
    for line in lines {
        let commit = commit_cache.read_commit(&line.commit)?;
        let author = signature_name(&commit.author);
        let date = if matches!(options.date_mode, BlameDateMode::Raw) {
            format_blame_date(&commit.author, options.date_mode)?
        } else {
            signature_blame_date(&commit.author)?
        };
        let display_id = blame_display_id(
            &line.commit,
            line.boundary && (!root || options.blank_boundary),
            long,
            options.abbrev_width,
            options.blank_boundary,
        );
        print!("{}\t({author:>10}\t{date}\t{})", display_id, line.line_no);
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
                line.source_line_no,
                line.line_no
            );
        } else {
            println!(
                "{} {} {}",
                line.commit.to_hex(),
                line.source_line_no,
                line.line_no
            );
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
    let mut described = HashSet::new();
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
            line.source_line_no,
            line.line_no
        );
        let commit = commit_cache.read_commit(&line.commit)?;
        print_blame_incremental_commit(
            &commit,
            line.boundary && !root,
            path,
            described.insert(line.commit.clone()),
        )?;
    }
    Ok(())
}

fn blame_starts_group(lines: &[BlameLine], index: usize) -> bool {
    index == 0
        || lines[index - 1].commit != lines[index].commit
        || lines[index - 1].source_line_no + 1 != lines[index].source_line_no
}

fn blame_group_len(lines: &[BlameLine], index: usize) -> usize {
    if !blame_starts_group(lines, index) {
        return 1;
    }
    let mut len = 1;
    while lines.get(index + len).is_some_and(|line| {
        line.commit == lines[index].commit
            && line.source_line_no == lines[index + len - 1].source_line_no + 1
    }) {
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

fn print_blame_incremental_commit(
    commit: &zmin_git_core::CommitObject,
    boundary: bool,
    path: &[u8],
    describe_commit: bool,
) -> Result<()> {
    if describe_commit {
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
    }
    if boundary {
        if describe_commit {
            println!("boundary");
        }
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
    blank_boundary: bool,
) -> String {
    if long {
        let hex = id.to_hex();
        if boundary && blank_boundary {
            return " ".repeat(hex.len());
        }
        if boundary {
            format!("^{}", &hex[..hex.len().saturating_sub(1)])
        } else {
            hex
        }
    } else if boundary {
        let width = abbrev_width.unwrap_or(8);
        if blank_boundary {
            return " ".repeat(width);
        }
        format!("^{}", short_object_id_len(id, width.saturating_sub(1)))
    } else {
        short_object_id_len(id, abbrev_width.unwrap_or(8))
    }
}

#[derive(Debug, Clone)]
struct ShowBranchHead {
    id: ObjectId,
    display: String,
    header_subject: Option<String>,
    current: bool,
    remote: bool,
}

pub(crate) struct ShowBranchOptions {
    pub(crate) all: bool,
    pub(crate) remotes: bool,
    pub(crate) current: bool,
    pub(crate) topo_order: bool,
    pub(crate) date_order: bool,
    pub(crate) sparse: bool,
    pub(crate) color: Option<String>,
    pub(crate) no_color: bool,
    pub(crate) more: Option<isize>,
    pub(crate) list: bool,
    pub(crate) independent: bool,
    pub(crate) merge_base: bool,
    pub(crate) sha1_name: bool,
    pub(crate) no_name: bool,
    pub(crate) topics: bool,
    pub(crate) reflog: Option<String>,
    pub(crate) revs: Vec<String>,
}

pub(crate) fn show_branch(options: ShowBranchOptions) -> Result<()> {
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let _accepted_sparse = options.sparse;
    let color_mode = parse_diff_color_option(options.color.as_deref(), options.no_color)?;
    if options.independent {
        return show_branch_independent(&repo, &store, &commit_cache, options.revs);
    }
    if options.merge_base {
        return show_branch_merge_base(&repo, &store, &commit_cache, options.revs);
    }
    let reflog_mode = options.reflog.is_some();
    let heads = if reflog_mode {
        show_branch_reflog_heads(&repo, &refs, options.reflog.as_deref(), &options.revs)?
    } else {
        show_branch_heads(
            &repo,
            &store,
            &refs,
            options.all,
            options.remotes,
            options.current,
            options.revs,
        )?
    };
    if heads.is_empty() {
        return Ok(());
    }
    let more = if options.list { Some(-1) } else { options.more };
    if more.is_some_and(|value| value < 0) {
        return show_branch_list(&commit_cache, &heads);
    }
    if heads.len() == 1 {
        println!(
            "[{}] {}",
            heads[0].display,
            show_branch_header_subject(&commit_cache, &heads[0])?
        );
        return Ok(());
    }
    for (idx, head) in heads.iter().enumerate() {
        println!(
            "{} [{}] {}",
            show_branch_header_prefix(&heads, idx, color_mode),
            head.display,
            show_branch_header_subject(&commit_cache, head)?
        );
    }
    println!("{}", "-".repeat(heads.len()));
    let commits = show_branch_commits(
        &commit_cache,
        &heads,
        options.topo_order,
        options.date_order,
        !reflog_mode,
    )?;
    for id in commits {
        if options.topics && show_branch_commit_is_first_branch_only(&commit_cache, &heads, &id)? {
            continue;
        }
        let mut prefix = String::new();
        for (idx, head) in heads.iter().enumerate() {
            if show_branch_reaches(&commit_cache, &head.id, &id)? {
                prefix.push_str(&show_branch_prefix_marker(
                    idx,
                    if head.current { '*' } else { '+' },
                    color_mode,
                ));
            } else {
                prefix.push(' ');
            }
        }
        let commit = commit_cache.read_commit(&id)?;
        let name = if options.no_name {
            String::new()
        } else if options.sha1_name {
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

fn show_branch_list(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    heads: &[ShowBranchHead],
) -> Result<()> {
    for head in heads {
        let prefix = if head.current { "* " } else { "  " };
        println!(
            "{prefix}[{}] {}",
            head.display,
            show_branch_header_subject(commit_cache, head)?
        );
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
            header_subject: None,
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
            header_subject: None,
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

fn show_branch_header_prefix(
    heads: &[ShowBranchHead],
    idx: usize,
    color_mode: DiffColorMode,
) -> String {
    let mut prefix = String::new();
    for _ in 0..idx {
        prefix.push(' ');
    }
    prefix.push_str(&show_branch_prefix_marker(
        idx,
        if heads[idx].current { '*' } else { '!' },
        color_mode,
    ));
    prefix
}

fn show_branch_commits(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    heads: &[ShowBranchHead],
    _topo_order: bool,
    _date_order: bool,
    reverse_heads: bool,
) -> Result<Vec<ObjectId>> {
    let mut pending = if reverse_heads {
        heads
            .iter()
            .rev()
            .map(|head| head.id.clone())
            .collect::<Vec<_>>()
    } else {
        heads.iter().map(|head| head.id.clone()).collect::<Vec<_>>()
    };
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

fn show_branch_header_subject(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    head: &ShowBranchHead,
) -> Result<String> {
    match &head.header_subject {
        Some(subject) => Ok(subject.clone()),
        None => Ok(commit_subject(&commit_cache.read_commit(&head.id)?.message)),
    }
}

fn show_branch_prefix_marker(idx: usize, marker: char, color_mode: DiffColorMode) -> String {
    if marker == ' ' || !show_branch_color_enabled(color_mode) {
        return marker.to_string();
    }
    let color = match idx % 6 {
        0 => 31,
        1 => 32,
        2 => 33,
        3 => 34,
        4 => 35,
        _ => 36,
    };
    format!("\u{1b}[{color}m{marker}\u{1b}[m")
}

fn show_branch_color_enabled(color_mode: DiffColorMode) -> bool {
    match color_mode {
        DiffColorMode::Never => false,
        DiffColorMode::Always => true,
        DiffColorMode::Auto => io::stdout().is_terminal(),
    }
}

fn show_branch_commit_is_first_branch_only(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    heads: &[ShowBranchHead],
    id: &ObjectId,
) -> Result<bool> {
    let Some((first, rest)) = heads.split_first() else {
        return Ok(false);
    };
    if !show_branch_reaches(commit_cache, &first.id, id)? {
        return Ok(false);
    }
    for head in rest {
        if show_branch_reaches(commit_cache, &head.id, id)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn show_branch_independent(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    revs: Vec<String>,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let heads = show_branch_heads(repo, store, &refs, false, false, false, revs)?;
    for (idx, head) in heads.iter().enumerate() {
        let mut reachable = false;
        for (other_idx, other) in heads.iter().enumerate() {
            if idx == other_idx {
                continue;
            }
            if show_branch_reaches(commit_cache, &other.id, &head.id)? {
                reachable = true;
                break;
            }
        }
        if !reachable {
            println!("{}", head.id.to_hex());
        }
    }
    Ok(())
}

fn show_branch_merge_base(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    revs: Vec<String>,
) -> Result<()> {
    if revs.len() < 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "`show-branch --merge-base` requires at least two revs".into(),
        });
    }
    let resolved = revs
        .iter()
        .map(|rev| resolve_commitish(repo, store, rev))
        .collect::<Result<Vec<_>>>()?;
    for base in merge_bases_all_cached(commit_cache, &resolved[0], &resolved[1])? {
        println!("{}", base.to_hex());
    }
    Ok(())
}

fn show_branch_reflog_heads(
    repo: &GitRepo,
    refs: &RefStore,
    raw: Option<&str>,
    revs: &[String],
) -> Result<Vec<ShowBranchHead>> {
    let selector = parse_show_branch_reflog_selector(raw)?;
    let ref_name = if let Some(value) = revs.first() {
        if value == "HEAD" {
            "HEAD".to_owned()
        } else {
            branch_ref_name(value).unwrap_or_else(|_| value.clone())
        }
    } else if let Some(current) = current_branch_ref(refs)? {
        current
    } else {
        "HEAD".to_owned()
    };
    let path = reflog_path(repo, &ref_name)?;
    let file = fs::File::open(&path).map_err(CliError::Io)?;
    let mut entries = Vec::new();
    let mut index = 0usize;
    let now = git_test_date_now().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
            .unwrap_or(0)
    });
    for_each_reflog_line_rev(file, |line| {
        let Some(entry) = parse_reflog_entry(line) else {
            return Ok(());
        };
        if index >= selector.base && entries.len() < selector.limit {
            let date = relative_date_from_timestamps(entry.timestamp, now);
            entries.push(ShowBranchHead {
                id: entry.new_id.clone(),
                display: format!("{ref_name}@{{{index}}}"),
                header_subject: Some(format!("({date}) {}", entry.message)),
                current: false,
                remote: false,
            });
        }
        index += 1;
        Ok(())
    })?;
    if entries.is_empty() {
        println!("No revs to be shown.");
    }
    Ok(entries)
}

struct ShowBranchReflogSelector {
    limit: usize,
    base: usize,
}

fn parse_show_branch_reflog_selector(raw: Option<&str>) -> Result<ShowBranchReflogSelector> {
    let Some(raw) = raw else {
        return Ok(ShowBranchReflogSelector { limit: 10, base: 0 });
    };
    if raw.is_empty() {
        return Ok(ShowBranchReflogSelector { limit: 10, base: 0 });
    }
    if raw.starts_with('=') {
        return Err(CliError::Stderr {
            code: 129,
            text: format!("error: unrecognized reflog param '{raw}'\n"),
        });
    }
    let (limit, base) = match raw.split_once(',') {
        Some((limit, base)) => (limit, Some(base)),
        None => (raw, None),
    };
    let limit = limit.parse::<usize>().map_err(|_| CliError::Stderr {
        code: 129,
        text: format!("error: unrecognized reflog param '{raw}'\n"),
    })?;
    let base = match base {
        Some(base) => base.parse::<usize>().map_err(|_| CliError::Stderr {
            code: 129,
            text: format!("error: unrecognized reflog param '{raw}'\n"),
        })?,
        None => 0,
    };
    Ok(ShowBranchReflogSelector { limit, base })
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
    let repo = find_repo_or_bare()?;
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
        symmetric_diff: None,
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
    pub(crate) contains: bool,
    pub(crate) long: bool,
    pub(crate) abbrev: Option<usize>,
    pub(crate) exact_match: bool,
    pub(crate) always: bool,
    pub(crate) dirty: Option<String>,
    pub(crate) broken: Option<String>,
    pub(crate) candidates: Option<usize>,
    pub(crate) debug: bool,
    pub(crate) first_parent: bool,
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
    if options.broken.is_some() && !options.commits.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "option '--broken' and commit-ishes cannot be used together".into(),
        });
    }
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1)
        .with_stable_pack_snapshot()
        .with_packed_objects_first();
    let commits = if options.commits.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        options.commits.clone()
    };
    let abbrev_len = options.abbrev.unwrap_or(default_abbrev_len(&store)?);
    let candidates = describe_candidates(&repo, &store, &options)?;
    let commit_cache = CommitObjectCache::new(&store);
    let dirty_suffix = describe_dirty_suffix(&repo, &store, &options)?;

    for commitish in commits {
        let id = resolve_describe_commitish(&repo, &store, &commitish)?;
        if options.debug {
            eprintln!("describe {commitish}");
        }
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
                let message = if options.candidates == Some(0) {
                    format!("no tag exactly matches '{id}'")
                } else {
                    format!(
                        "No tags can describe '{}'.\nTry --always, or create some tags.",
                        id
                    )
                };
                return Err(CliError::Fatal { code: 128, message });
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
            describe_candidate_target(store, id, options.all || options.tags || options.contains)?
        else {
            return Ok(());
        };
        if !options.all && !options.tags && !options.contains && !annotated {
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
    candidates.sort_by(|left, right| {
        right
            .tagger_timestamp
            .cmp(&left.tagger_timestamp)
            .then_with(|| describe_candidate_cmp(left, right).cmp(&false))
    });
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
    let depths = if options.contains {
        HashMap::new()
    } else if options.first_parent {
        commit_depths_first_parent(commit_cache, id)?
    } else {
        describe_traversal_depths(commit_cache, id)?
    };
    let mut best = None::<(&DescribeCandidate, usize)>;
    let candidate_limit = options.candidates.unwrap_or(10);
    for candidate in candidates.iter().take(candidate_limit.max(1)) {
        let Some(depth) = describe_candidate_depth(commit_cache, id, candidate, options, &depths)?
        else {
            continue;
        };
        if (options.exact_match || options.candidates == Some(0)) && depth != 0 {
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
        if options.debug {
            eprintln!("No exact match on refs or tags, searching to describe");
            eprintln!("traversed {} commits", depths.len().max(1));
        }
        return Ok(None);
    };
    if options.debug {
        eprintln!("No exact match on refs or tags, searching to describe");
        eprintln!("finished search at {}", candidate.target);
        for debug_candidate in candidates.iter().take(candidate_limit.max(1)) {
            if let Some(debug_depth) =
                describe_candidate_depth(commit_cache, id, debug_candidate, options, &depths)?
            {
                let kind = if debug_candidate.annotated {
                    "annotated"
                } else {
                    "lightweight"
                };
                eprintln!("{kind:>10} {:>10} {}", debug_depth, debug_candidate.name);
            }
        }
        eprintln!("traversed {} commits", depths.len().max(1));
    }
    if options.abbrev == Some(0) {
        return Ok(Some(candidate.name.clone()));
    }
    if options.contains {
        return Ok(Some(if depth == 0 {
            candidate.name.clone()
        } else {
            format!("{}~{}", candidate.name, depth)
        }));
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

fn describe_dirty_suffix<'a>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &'a DescribeOptions,
) -> Result<&'a str> {
    if let Some(mark) = options.dirty.as_deref() {
        return if worktree_clean(repo, store)? {
            Ok("")
        } else {
            Ok(mark)
        };
    }
    if let Some(mark) = options.broken.as_deref() {
        match worktree_clean(repo, store) {
            Ok(true) => Ok(""),
            Ok(false) => Ok(mark),
            Err(error) => {
                let mut message = match error {
                    CliError::Fatal { message, .. } => message,
                    CliError::Stderr { text, .. } => text,
                    CliError::Message(message) => message,
                    CliError::Io(io_error) => io_error.to_string(),
                    CliError::Exit(code) => format!("exited with status {code}"),
                };
                if message == "git index is too short" {
                    message = ".git/index: index file smaller than expected".to_owned();
                }
                eprintln!("fatal: {message}");
                eprintln!("fatal: {message}");
                Ok(mark)
            }
        }
    } else {
        Ok("")
    }
}

fn commit_depths_first_parent(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    start: &ObjectId,
) -> Result<HashMap<ObjectId, usize>> {
    let mut depths = HashMap::with_capacity(1024);
    depths.insert(start.clone(), 0usize);
    let mut pending = VecDeque::from([start.clone()]);
    while let Some(id) = pending.pop_front() {
        let depth = depths[&id];
        let links = commit_cache.read_commit_links(&id)?;
        let Some(parent) = links.parents.first() else {
            continue;
        };
        let parent_depth = depth.checked_add(1).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit depth overflow".into(),
        })?;
        if let Entry::Vacant(entry) = depths.entry(parent.clone()) {
            pending.push_back(entry.key().clone());
            entry.insert(parent_depth);
        }
    }
    Ok(depths)
}

fn describe_traversal_depths(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    start: &ObjectId,
) -> Result<HashMap<ObjectId, usize>> {
    let mut depths = HashMap::with_capacity(1024);
    let mut queued = HashSet::with_capacity(1024);
    let mut pending = VecDeque::from([start.clone()]);
    queued.insert(start.clone());
    let mut depth = 0usize;
    while let Some(id) = pending.pop_front() {
        depths.insert(id.clone(), depth);
        depth = depth.checked_add(1).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit depth overflow".into(),
        })?;
        let links = commit_cache.read_commit_links(&id)?;
        for parent in &links.parents {
            if queued.insert(parent.clone()) {
                pending.push_back(parent.clone());
            }
        }
    }
    Ok(depths)
}

fn describe_candidate_depth(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
    candidate: &DescribeCandidate,
    options: &DescribeOptions,
    depths: &HashMap<ObjectId, usize>,
) -> Result<Option<usize>> {
    if options.contains {
        let contains_depths = if options.first_parent {
            commit_depths_first_parent(commit_cache, &candidate.target)?
        } else {
            commit_depths_cached(commit_cache, &candidate.target)?
        };
        Ok(contains_depths.get(id).copied())
    } else {
        Ok(depths.get(&candidate.target).copied())
    }
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

pub(crate) fn validate_for_each_ref_describe_atom(atom: &str) -> Result<()> {
    parse_for_each_ref_describe_options(atom).map(|_| ())
}

pub(crate) fn describe_for_each_ref(
    repo: &GitRepo,
    object_id: &ObjectId,
    atom: &str,
) -> Result<String> {
    let options = parse_for_each_ref_describe_options(atom)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let Some(commit_id) = peel_to_commit(&store, object_id.clone())? else {
        return Ok(String::new());
    };
    let abbrev_len = options.abbrev.unwrap_or(default_abbrev_len(&store)?);
    let candidates = describe_candidates(repo, &store, &options)?;
    let commit_cache = CommitObjectCache::new(&store);
    Ok(
        describe_commit(&commit_cache, &commit_id, &candidates, &options, abbrev_len)?
            .unwrap_or_default(),
    )
}

fn parse_for_each_ref_describe_options(atom: &str) -> Result<DescribeOptions> {
    let atom = atom.strip_prefix('*').unwrap_or(atom);
    let arguments = atom.strip_prefix("describe:");
    if atom != "describe" && arguments.is_none() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("unknown field name: {atom}"),
        });
    }
    let mut options = DescribeOptions {
        all: false,
        tags: false,
        contains: false,
        long: false,
        abbrev: None,
        exact_match: false,
        always: false,
        dirty: None,
        broken: None,
        candidates: None,
        debug: false,
        first_parent: false,
        matches: Vec::new(),
        excludes: Vec::new(),
        commits: Vec::new(),
    };
    let mut remaining = arguments.unwrap_or_default();
    while !remaining.is_empty() {
        let (argument, rest) = remaining
            .split_once(',')
            .map(|(argument, rest)| (argument, Some(rest)))
            .unwrap_or((remaining, None));
        if argument == "tags" {
            options.tags = true;
        } else if let Some(value) = argument.strip_prefix("abbrev=") {
            options.abbrev = Some(value.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("unrecognized %(describe) argument: {remaining}"),
            })?);
        } else if let Some(value) = argument.strip_prefix("match=") {
            options.matches.push(value.to_owned());
        } else if let Some(value) = argument.strip_prefix("exclude=") {
            options.excludes.push(value.to_owned());
        } else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unrecognized %(describe) argument: {remaining}"),
            });
        }
        remaining = rest.unwrap_or_default();
    }
    Ok(options)
}

pub(crate) struct NameRevOptions {
    pub(crate) name_only: bool,
    pub(crate) tags: bool,
    pub(crate) refs: Vec<String>,
    pub(crate) excludes: Vec<String>,
    pub(crate) all: bool,
    pub(crate) annotate_stdin: bool,
    pub(crate) no_undefined: bool,
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
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let candidates = name_rev_candidates(&repo, &store, &commit_cache, &options)?;
    if options.all {
        let mut roots = Vec::<ObjectId>::new();
        let mut seen_roots = HashSet::<ObjectId>::new();
        for candidate in &candidates {
            for (id, depth) in &candidate.depths {
                if *depth == 0 && seen_roots.insert(id.clone()) {
                    roots.push(id.clone());
                }
            }
        }
        for id in collect_commits_from_ids_cached(&repo, &commit_cache, &roots, None)? {
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
    let name = best_name_rev(id, candidates);
    if options.no_undefined && name.is_none() {
        print!("{} ", id.to_hex());
        return Err(CliError::Stderr {
            code: 128,
            text: format!("fatal: cannot describe '{}'\n", id.to_hex()),
        });
    }
    let name = name.unwrap_or_else(|| "undefined".to_owned());
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

pub(crate) fn range_diff(
    _no_dual_color: bool,
    no_no_dual_color: bool,
    creation_factor: Option<String>,
    left_only: bool,
    right_only: bool,
    notes: bool,
    no_notes: bool,
    ranges: Vec<String>,
) -> Result<()> {
    let ranges = parse_range_diff_ranges(&ranges)?;
    let options = RangeDiffOptions {
        color: no_no_dual_color,
        creation_factor,
        left_only,
        right_only,
        notes,
        no_notes,
    };
    print!("{}", render_range_diff_output(&ranges, &options)?);
    Ok(())
}

pub(crate) struct RangeDiffOptions {
    pub(crate) color: bool,
    pub(crate) creation_factor: Option<String>,
    pub(crate) left_only: bool,
    pub(crate) right_only: bool,
    pub(crate) notes: bool,
    pub(crate) no_notes: bool,
}

pub(crate) fn render_range_diff_output(
    ranges: &[String; 2],
    options: &RangeDiffOptions,
) -> Result<String> {
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let old = range_diff_commits(&repo, &store, &ranges[0])?;
    let new = range_diff_commits(&repo, &store, &ranges[1])?;
    let abbrev_len = default_abbrev_len(&store)?;
    let color = options.color;
    let _accepted_creation_factor = options.creation_factor.as_deref();
    let _accepted_notes = options.notes;
    let _accepted_no_notes = options.no_notes;
    let mut output = String::new();
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
            write_range_diff_line(
                &mut output,
                color,
                "33",
                &format!(
                    "{}:  {} = {}:  {} {}",
                    old_idx + 1,
                    short_object_id_len(&old_entry.id, abbrev_len),
                    new_idx + 1,
                    short_object_id_len(&new[new_idx].id, abbrev_len),
                    old_entry.subject
                ),
            )?;
        } else {
            if options.right_only {
                continue;
            }
            write_range_diff_line(
                &mut output,
                color,
                "31",
                &format!(
                    "{}:  {} < -:  ------- {}",
                    old_idx + 1,
                    short_object_id_len(&old_entry.id, abbrev_len),
                    old_entry.subject
                ),
            )?;
        }
    }
    for (new_idx, new_entry) in new.iter().enumerate() {
        if matched_new.contains(&new_idx) {
            continue;
        }
        if options.left_only {
            continue;
        }
        write_range_diff_line(
            &mut output,
            color,
            "32",
            &format!(
                "-:  ------- > {}:  {} {}",
                new_idx + 1,
                short_object_id_len(&new_entry.id, abbrev_len),
                new_entry.subject
            ),
        )?;
    }
    Ok(output)
}

fn parse_range_diff_ranges(ranges: &[String]) -> Result<[String; 2]> {
    match ranges {
        [old, new] => Ok([old.clone(), new.clone()]),
        [base, old, new] => Ok([format!("{base}..{old}"), format!("{base}..{new}")]),
        _ => Err(CliError::Fatal {
            code: 129,
            message: "`range-diff` requires two commit ranges or <base> <old> <new>".into(),
        }),
    }
}

fn write_range_diff_line(out: &mut String, color: bool, code: &str, line: &str) -> Result<()> {
    if color {
        out.push_str(&format!("\x1b[{code}m{line}\x1b[m\n"));
    } else {
        out.push_str(line);
        out.push('\n');
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
    pub(crate) exclude: Vec<String>,
    pub(crate) exclude_first_parent_only: bool,
    pub(crate) exclude_hidden: Option<&'a str>,
    pub(crate) exclude_promisor_objects: bool,
    pub(crate) author: Option<&'a str>,
    pub(crate) committer: Option<&'a str>,
    pub(crate) alternate_refs: bool,
    pub(crate) bisect: bool,
    pub(crate) bisect_all: bool,
    pub(crate) bisect_vars: bool,
    pub(crate) cherry: bool,
    pub(crate) count: bool,
    pub(crate) glob: Option<&'a str>,
    pub(crate) skip: Option<usize>,
    pub(crate) max_parents: Option<&'a str>,
    pub(crate) no_max_parents: bool,
    pub(crate) merges: bool,
    pub(crate) merge: bool,
    pub(crate) max_age: Option<&'a str>,
    pub(crate) min_parents: Option<&'a str>,
    pub(crate) min_age: Option<&'a str>,
    pub(crate) no_min_parents: bool,
    pub(crate) no_merges: bool,
    pub(crate) parents: bool,
    pub(crate) first_parent: bool,
    pub(crate) follow: bool,
    pub(crate) no_diff_merges: bool,
    pub(crate) diff_merges: Option<&'a str>,
    pub(crate) separate_merges: bool,
    pub(crate) dd: bool,
    pub(crate) reverse: bool,
    pub(crate) full_history: bool,
    pub(crate) ancestry_path: bool,
    pub(crate) in_commit_order: bool,
    pub(crate) dense: bool,
    pub(crate) sparse: bool,
    pub(crate) show_pulls: bool,
    pub(crate) show_linear_break: bool,
    pub(crate) simplify_merges: bool,
    pub(crate) simplify_by_decoration: bool,
    pub(crate) topo_order: bool,
    pub(crate) date_order: bool,
    pub(crate) author_date_order: bool,
    pub(crate) left_right: bool,
    pub(crate) left_only: bool,
    pub(crate) right_only: bool,
    pub(crate) cherry_pick: bool,
    pub(crate) cherry_mark: bool,
    pub(crate) boundary: bool,
    pub(crate) children: bool,
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
    pub(crate) encoding: Option<&'a str>,
    pub(crate) expand_tabs: bool,
    pub(crate) no_expand_tabs: bool,
    pub(crate) notes: bool,
    pub(crate) no_notes: bool,
    pub(crate) show_notes: bool,
    pub(crate) show_notes_by_default: bool,
    pub(crate) standard_notes: bool,
    pub(crate) no_standard_notes: bool,
    pub(crate) diff_required: bool,
    pub(crate) decorate: Option<&'a str>,
    pub(crate) decorate_refs: Option<&'a str>,
    pub(crate) decorate_refs_exclude: Option<&'a str>,
    pub(crate) no_decorate: bool,
    pub(crate) clear_decorations: bool,
    pub(crate) abbrev_commit: bool,
    pub(crate) no_abbrev_commit: bool,
    pub(crate) graph: bool,
    pub(crate) objects: bool,
    pub(crate) objects_edge: bool,
    pub(crate) objects_edge_aggressive: bool,
    pub(crate) no_object_names: bool,
    pub(crate) indexed_objects: bool,
    pub(crate) unpacked: bool,
    pub(crate) remove_empty: bool,
    pub(crate) ignore_missing: bool,
    pub(crate) filter: Option<String>,
    pub(crate) full_diff: bool,
    pub(crate) filter_print_omitted: bool,
    pub(crate) filter_provided_objects: bool,
    pub(crate) pickaxe_string: Option<&'a str>,
    pub(crate) pickaxe_regex: Option<&'a str>,
    pub(crate) pickaxe_regex_mode: bool,
    pub(crate) pickaxe_all: bool,
    pub(crate) ignore_matching_lines: Vec<String>,
    pub(crate) walk_reflogs: bool,
    pub(crate) reflog: bool,
    pub(crate) do_walk: bool,
    pub(crate) no_walk: bool,
    pub(crate) stdin: bool,
    pub(crate) grep_reflog: Vec<String>,
    pub(crate) grep: Vec<String>,
    pub(crate) invert_grep: bool,
    pub(crate) all_match: bool,
    pub(crate) regexp_ignore_case: bool,
    pub(crate) basic_regexp: bool,
    pub(crate) extended_regexp: bool,
    pub(crate) fixed_strings: bool,
    pub(crate) perl_regexp: bool,
    pub(crate) format: Option<&'a str>,
    pub(crate) show_signature: bool,
    pub(crate) log_size: bool,
    pub(crate) line_ranges: Vec<String>,
    pub(crate) mailmap: bool,
    pub(crate) no_mailmap: bool,
    pub(crate) use_mailmap: bool,
    pub(crate) no_use_mailmap: bool,
    pub(crate) source: bool,
    pub(crate) max_count: Option<&'a str>,
    pub(crate) since: Option<&'a str>,
    pub(crate) since_as_filter: Option<&'a str>,
    pub(crate) until: Option<&'a str>,
    pub(crate) date: Option<&'a str>,
    pub(crate) relative_date: bool,
    pub(crate) pretty: Option<&'a str>,
    pub(crate) single_worktree: bool,
    pub(crate) commit_header: bool,
    pub(crate) no_commit_header: bool,
    pub(crate) disk_usage: bool,
    pub(crate) header: bool,
    pub(crate) progress: bool,
    pub(crate) no_filter: bool,
    pub(crate) missing: bool,
    pub(crate) use_bitmap_index: bool,
    pub(crate) quiet: bool,
    pub(crate) raw_args: &'a [String],
    pub(crate) revs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RevListMissingMode {
    AllowAny,
    AllowPromisor,
    Print,
}

impl LogOptions<'_> {
    fn history_order(&self) -> Option<HistoryCommitOrder> {
        if self.author_date_order {
            Some(HistoryCommitOrder::AuthorDate)
        } else if self.date_order {
            Some(HistoryCommitOrder::Date)
        } else if self.topo_order {
            Some(HistoryCommitOrder::Topo)
        } else {
            None
        }
    }

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HistoryCommitOrder {
    Topo,
    Date,
    AuthorDate,
}

#[derive(Debug, Clone)]
struct HistoryOrderMetadata {
    id: ObjectId,
    parents: Vec<ObjectId>,
    timestamp: i64,
    original_index: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct HistoryOrderReady {
    timestamp: i64,
    original_index: usize,
    id: ObjectId,
}

impl Ord for HistoryOrderReady {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.timestamp
            .cmp(&other.timestamp)
            .then_with(|| other.original_index.cmp(&self.original_index))
            .then_with(|| self.id.to_hex().cmp(&other.id.to_hex()))
    }
}

impl PartialOrd for HistoryOrderReady {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn history_order_timestamp(commit: &CommitObject, order: HistoryCommitOrder) -> Result<i64> {
    let (signature, label) = match order {
        HistoryCommitOrder::AuthorDate => (&commit.author, "author"),
        HistoryCommitOrder::Topo | HistoryCommitOrder::Date => (&commit.committer, "committer"),
    };
    signature_timestamp(signature).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("commit has invalid {label} timestamp"),
    })
}

fn reorder_history_from_metadata(
    commits: Vec<HistoryOrderMetadata>,
    order: HistoryCommitOrder,
) -> Result<Vec<ObjectId>> {
    let included = commits
        .iter()
        .map(|commit| commit.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut remaining_children = HashMap::<ObjectId, usize>::new();
    let mut metadata_by_id = HashMap::<ObjectId, HistoryOrderMetadata>::new();

    for commit in commits {
        remaining_children.entry(commit.id.clone()).or_insert(0);
        for parent in &commit.parents {
            if included.contains(parent) {
                *remaining_children.entry(parent.clone()).or_insert(0) += 1;
            }
        }
        metadata_by_id.insert(commit.id.clone(), commit);
    }

    let mut ordered = Vec::with_capacity(metadata_by_id.len());
    if matches!(order, HistoryCommitOrder::Topo) {
        let mut ready = metadata_by_id
            .values()
            .filter(|commit| remaining_children.get(&commit.id).copied().unwrap_or(0) == 0)
            .map(|commit| (commit.original_index, commit.id.clone()))
            .collect::<Vec<_>>();
        ready.sort_by_key(|(original_index, _)| *original_index);
        while let Some((_, next_id)) = ready.pop() {
            let Some(commit) = metadata_by_id.get(&next_id) else {
                continue;
            };
            ordered.push(next_id.clone());
            for parent in &commit.parents {
                if !included.contains(parent) {
                    continue;
                }
                let Some(remaining) = remaining_children.get_mut(parent) else {
                    continue;
                };
                *remaining -= 1;
                if *remaining == 0 {
                    let parent_commit =
                        metadata_by_id.get(parent).ok_or_else(|| CliError::Fatal {
                            code: 128,
                            message: "history order parent metadata missing".into(),
                        })?;
                    ready.push((parent_commit.original_index, parent.clone()));
                }
            }
        }
    } else {
        let mut ready = std::collections::BinaryHeap::new();
        for commit in metadata_by_id.values() {
            if remaining_children.get(&commit.id).copied().unwrap_or(0) == 0 {
                ready.push(HistoryOrderReady {
                    timestamp: commit.timestamp,
                    original_index: commit.original_index,
                    id: commit.id.clone(),
                });
            }
        }
        while let Some(next) = ready.pop() {
            let Some(commit) = metadata_by_id.get(&next.id) else {
                continue;
            };
            ordered.push(next.id.clone());
            for parent in &commit.parents {
                if !included.contains(parent) {
                    continue;
                }
                let Some(remaining) = remaining_children.get_mut(parent) else {
                    continue;
                };
                *remaining -= 1;
                if *remaining == 0 {
                    let parent_commit =
                        metadata_by_id.get(parent).ok_or_else(|| CliError::Fatal {
                            code: 128,
                            message: "history order parent metadata missing".into(),
                        })?;
                    ready.push(HistoryOrderReady {
                        timestamp: parent_commit.timestamp,
                        original_index: parent_commit.original_index,
                        id: parent.clone(),
                    });
                }
            }
        }
    }

    if ordered.len() != metadata_by_id.len() {
        return Err(CliError::Fatal {
            code: 128,
            message: "history ordering failed to visit every collected commit".into(),
        });
    }
    Ok(ordered)
}

fn reorder_collected_commits(
    commits: Vec<CollectedCommit>,
    order: HistoryCommitOrder,
) -> Result<Vec<CollectedCommit>> {
    let metadata = commits
        .iter()
        .enumerate()
        .map(|(original_index, entry)| {
            Ok(HistoryOrderMetadata {
                id: entry.id.clone(),
                parents: entry.commit.parents.clone(),
                timestamp: history_order_timestamp(entry.commit.as_ref(), order)?,
                original_index,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let ordered_ids = reorder_history_from_metadata(metadata, order)?;
    let mut commits_by_id = commits
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect::<HashMap<_, _>>();
    ordered_ids
        .into_iter()
        .map(|id| {
            commits_by_id.remove(&id).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "history ordered commit missing from collected set".into(),
            })
        })
        .collect()
}

fn reorder_collected_commit_metadata(
    commits: Vec<CollectedCommitMetadata>,
    order: HistoryCommitOrder,
) -> Result<Vec<CollectedCommitMetadata>> {
    let metadata = commits
        .iter()
        .enumerate()
        .map(|(original_index, entry)| {
            let timestamp = match order {
                HistoryCommitOrder::AuthorDate => signature_timestamp(&entry.author),
                HistoryCommitOrder::Topo | HistoryCommitOrder::Date => {
                    signature_timestamp(&entry.committer)
                }
            }
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "history ordering encountered an invalid commit timestamp".into(),
            })?;
            Ok(HistoryOrderMetadata {
                id: entry.id.clone(),
                parents: entry.parents.clone(),
                timestamp,
                original_index,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let ordered_ids = reorder_history_from_metadata(metadata, order)?;
    let mut commits_by_id = commits
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect::<HashMap<_, _>>();
    ordered_ids
        .into_iter()
        .map(|id| {
            commits_by_id.remove(&id).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "history ordered commit metadata missing from collected set".into(),
            })
        })
        .collect()
}

fn reorder_commit_ids<S>(
    commit_cache: &CommitObjectCache<'_, S>,
    commit_ids: Vec<ObjectId>,
    order: HistoryCommitOrder,
) -> Result<Vec<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    let metadata = commit_ids
        .iter()
        .enumerate()
        .map(|(original_index, id)| {
            let commit = commit_cache.read_commit(id)?;
            Ok(HistoryOrderMetadata {
                id: id.clone(),
                parents: commit.parents.clone(),
                timestamp: history_order_timestamp(commit.as_ref(), order)?,
                original_index,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    reorder_history_from_metadata(metadata, order)
}

fn filter_commits_by_ancestry_path(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    revs: &RevListRevs,
    commits: Vec<CollectedCommit>,
) -> Result<Vec<CollectedCommit>> {
    let bounds = resolve_ancestry_path_bounds(repo, store, revs)?;
    if bounds.is_empty() {
        return Ok(commits);
    }
    let mut filtered = Vec::with_capacity(commits.len());
    for entry in commits {
        if bounds
            .iter()
            .any(|bound| is_ancestor_commit_cached(commit_cache, bound, &entry.id).unwrap_or(false))
        {
            filtered.push(entry);
        }
    }
    Ok(filtered)
}

fn filter_commit_ids_by_ancestry_path(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    revs: &RevListRevs,
    commit_ids: Vec<ObjectId>,
) -> Result<Vec<ObjectId>> {
    let bounds = resolve_ancestry_path_bounds(repo, store, revs)?;
    if bounds.is_empty() {
        return Ok(commit_ids);
    }
    let mut filtered = Vec::with_capacity(commit_ids.len());
    for id in commit_ids {
        if bounds
            .iter()
            .any(|bound| is_ancestor_commit_cached(commit_cache, bound, &id).unwrap_or(false))
        {
            filtered.push(id);
        }
    }
    Ok(filtered)
}

fn resolve_ancestry_path_bounds(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
) -> Result<Vec<ObjectId>> {
    revs.exclude
        .iter()
        .map(|rev| resolve_commitish(repo, store, rev))
        .collect()
}

fn collect_default_log_decoration_ids(
    repo: &GitRepo,
    ref_snapshot: Option<&RevListRefSnapshot>,
) -> Result<HashSet<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut decorated = HashSet::new();
    if let Some(head_id) = ref_snapshot.and_then(|snapshot| snapshot.head_id.clone()) {
        decorated.insert(head_id);
    } else if let Ok(head_id) = refs.resolve("HEAD") {
        decorated.insert(head_id);
    }
    if let Some(snapshot) = ref_snapshot {
        for row in &snapshot.refs {
            if log_decorates_ref_by_default(&row.ref_name) {
                decorated.insert(row.id.clone());
            }
        }
    } else {
        refs.for_each_resolved_ref("refs/", |ref_name, id| {
            if log_decorates_ref_by_default(ref_name) {
                decorated.insert(id.clone());
            }
            Ok::<(), CliError>(())
        })?;
    }
    Ok(decorated)
}

fn parse_log_diff_merges_arg(
    value: &str,
    config_mode: Option<LogMergeDiffMode>,
) -> Result<LogMergeDiffMode> {
    if value == "on" {
        return Ok(config_mode.unwrap_or(LogMergeDiffMode::Separate));
    }
    parse_log_diff_merges_value(value, true).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("invalid value for '--diff-merges': '{value}'"),
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
        "separate" | "m" => Some(LogMergeDiffMode::Separate),
        "combined" | "c" => Some(LogMergeDiffMode::Combined),
        "dense-combined" | "dense_combined" | "cc" => Some(LogMergeDiffMode::DenseCombined),
        "on" if include_on => Some(LogMergeDiffMode::Separate),
        _ => None,
    }
}

pub(crate) fn log(options: LogOptions<'_>) -> Result<()> {
    match log_with_options(options) {
        Err(CliError::Io(error)) if error.to_string().contains("reference broken") => {
            Err(CliError::Fatal {
                code: 128,
                message: "your current branch appears to be broken".into(),
            })
        }
        Err(CliError::Fatal { code, message }) if message == "reference broken" => {
            Err(CliError::Fatal {
                code,
                message: "your current branch appears to be broken".into(),
            })
        }
        other => other,
    }
}

fn log_with_options(options: LogOptions<'_>) -> Result<()> {
    let _trace = phase_trace("log.total");
    let _ = options.count;
    let _accepted_quiet = options.quiet;
    for unsupported in ["--object-names", "--timestamp"] {
        if raw_arg_present_before_dashdash(options.raw_args, unsupported) {
            return Err(log_unrecognized_argument(unsupported));
        }
    }
    let _accepted_exclude = &options.exclude;
    let _accepted_exclude_first_parent_only = options.exclude_first_parent_only;
    let _accepted_exclude_hidden = options.exclude_hidden;
    let _accepted_alternate_refs = options.alternate_refs;
    let _accepted_bisect = options.bisect;
    let _accepted_cherry = options.cherry;
    let _accepted_glob = options.glob;
    let _accepted_in_commit_order = options.in_commit_order;
    let _accepted_show_linear_break = options.show_linear_break;
    let _accepted_indexed_objects = options.indexed_objects;
    let _accepted_unpacked = options.unpacked;
    let _accepted_remove_empty = options.remove_empty;
    let _accepted_ignore_missing = options.ignore_missing;
    let _accepted_single_worktree = options.single_worktree;
    let _accepted_no_filter = options.no_filter;
    let _accepted_follow = options.follow;
    let _accepted_decorate_refs = options.decorate_refs;
    let _accepted_decorate_refs_exclude = options.decorate_refs_exclude;
    let _accepted_no_decorate = options.no_decorate;
    let _accepted_objects_edge = options.objects_edge;
    let _accepted_objects_edge_aggressive = options.objects_edge_aggressive;
    let _accepted_show_signature = options.show_signature;
    let _accepted_mailmap = options.mailmap;
    let _accepted_no_mailmap = options.no_mailmap;
    let _accepted_use_mailmap = options.use_mailmap;
    let _accepted_no_use_mailmap = options.no_use_mailmap;
    let _accepted_source = options.source;
    let _accepted_full_diff = options.full_diff;
    if options.exclude_promisor_objects {
        return Err(log_unrecognized_argument("--exclude-promisor-objects"));
    }
    if options.bisect_all {
        return Err(log_unrecognized_argument("--bisect-all"));
    }
    if options.bisect_vars {
        return Err(log_unrecognized_argument("--bisect-vars"));
    }
    if options.commit_header {
        return Err(log_unrecognized_argument("--commit-header"));
    }
    if options.no_commit_header {
        return Err(log_unrecognized_argument("--no-commit-header"));
    }
    if options.disk_usage {
        return Err(log_unrecognized_argument("--disk-usage"));
    }
    if options.filter_print_omitted {
        return Err(log_unrecognized_argument("--filter-print-omitted"));
    }
    if options.header {
        return Err(log_unrecognized_argument("--header"));
    }
    if options.progress {
        return Err(log_unrecognized_argument("--progress"));
    }
    if options.missing {
        return Err(log_unrecognized_argument("--missing"));
    }
    if options.use_bitmap_index {
        return Err(log_unrecognized_argument("--use-bitmap-index"));
    }
    if options.merge {
        return Err(CliError::Fatal {
            code: 128,
            message:
                "--merge requires one of the pseudorefs MERGE_HEAD, CHERRY_PICK_HEAD, REVERT_HEAD or REBASE_HEAD"
                    .into(),
        });
    }
    let revs_with_stdin = history_revs_with_stdin(&options.revs, options.stdin)?;
    let late_options = split_log_revs_and_count(
        revs_with_stdin,
        options.max_count,
        options.regexp_ignore_case,
        options.basic_regexp,
        options.extended_regexp,
        options.fixed_strings,
        options.perl_regexp,
    )?;
    let revs = late_options.revs;
    let max_count = late_options.max_count;
    let parsed_zero = late_options.zero;
    let regexp_ignore_case = late_options.regexp_ignore_case;
    let basic_regexp = late_options.basic_regexp;
    let extended_regexp = late_options.extended_regexp;
    let fixed_strings = late_options.fixed_strings;
    let perl_regexp = late_options.perl_regexp;
    let topo_order = options.topo_order || late_options.topo_order;
    let date_order = options.date_order || late_options.date_order;
    let author_date_order = options.author_date_order || late_options.author_date_order;
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
        parsed_log_revs.patch && !options.patch_with_stat,
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
    let walk_reflogs = options.walk_reflogs;
    let no_walk = resolve_history_walk_mode(options.raw_args, options.no_walk, options.do_walk);
    if !options.grep_reflog.is_empty() && !walk_reflogs {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--grep-reflog' requires '--walk-reflogs'".into(),
        });
    }
    if options.filter.is_some() && !options.objects {
        return Err(CliError::Fatal {
            code: 128,
            message: "object filtering requires --objects".into(),
        });
    }
    if options.filter_provided_objects {
        return Err(CliError::Fatal {
            code: 128,
            message: "unrecognized argument: --filter-provided-objects".into(),
        });
    }
    if options.no_object_names {
        return Err(CliError::Fatal {
            code: 128,
            message: "unrecognized argument: --no-object-names".into(),
        });
    }
    if walk_reflogs {
        return log_reflog(&options, &parsed_log_revs, max_count);
    }
    let format = LogFormat::parse(
        options.oneline,
        parsed_log_revs.format.as_deref().or(options.format),
        parsed_log_revs.pretty.as_deref().or(options.pretty),
    )?;
    let ignore_matching_lines =
        compile_ignore_matching_lines(&parsed_log_revs.ignore_matching_lines)?;
    let grep_mode =
        parse_shortlog_pattern_mode(basic_regexp, extended_regexp, fixed_strings, perl_regexp);
    let effective_since = options.since.or(options.since_as_filter);
    let (since, until) = resolve_history_age_bounds(
        options.raw_args,
        effective_since,
        options.max_age,
        options.until,
        options.min_age,
    );
    let Some(since) = parse_log_since(since) else {
        return Ok(());
    };
    let Some(until) = parse_log_until(until) else {
        return Ok(());
    };
    let skip = resolve_history_skip(options.raw_args, options.skip)?;
    let author_pattern = options.author;
    let committer_pattern = options.committer;
    let (min_parents, max_parents) = parse_log_parent_bounds(
        options.min_parents,
        options.no_min_parents,
        options.no_merges,
        options.max_parents,
        options.no_max_parents,
        options.merges,
    )?;
    let date_arg = history_raw_date_arg(options.raw_args, options.date, options.relative_date);
    let date_mode = parse_log_date_mode(date_arg.as_deref())?;
    let encoding = parsed_log_revs.encoding.as_deref().or(options.encoding);
    let expand_tabs =
        log_expand_tabs_enabled(encoding, options.expand_tabs, options.no_expand_tabs);
    let repo = find_repo_or_bare()?;
    let show_root = options.root || log_showroot_enabled(&repo)?;
    let diff_format = options.diff_format(parsed_log_revs.patch);
    let merge_diff_mode = options.merge_diff_mode(&repo, diff_format)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1)
        .with_transient_packed_object_reads()
        .with_trusted_packed_object_reads()
        .with_buffered_pack_reads()
        .with_packed_file_reader_buffer_capacity(LOG_PACK_FILE_READER_BUFFER_BYTES)
        .with_packed_object_read_cache_byte_limit(LOG_PACK_OBJECT_CACHE_BYTES);
    let (mut parsed_revs, implicit_pathspecs) =
        split_log_implicit_pathspecs(&repo, parsed_log_revs.revs.clone());
    let mut parsed_pathspecs = parsed_log_revs.pathspecs.clone();
    parsed_pathspecs.extend(implicit_pathspecs);
    if options.follow && parsed_pathspecs.is_empty() && parsed_revs.len() == 1 {
        let candidate = parsed_revs[0].clone();
        if resolve_objectish(&repo, &candidate).is_err() {
            parsed_revs.clear();
            parsed_pathspecs.push(candidate);
        }
    }
    let pathspecs = parsed_pathspecs
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, Path::new(path)))
        .collect::<Result<Vec<_>>>()?;
    let mut requested_revs = if parsed_revs.is_empty() && !options.all && !options.reflog {
        vec!["HEAD".to_owned()]
    } else {
        parsed_revs
    };
    if options.reflog {
        requested_revs.extend(log_reflog_object_ids(&repo)?);
    }
    let configured_decoration_mode = if parsed_log_revs.decorate.is_none() {
        log_configured_decoration_mode(&repo)?
    } else {
        None
    };
    let decoration_mode = parsed_log_revs
        .decorate
        .or(configured_decoration_mode)
        .or_else(|| {
            format
                .uses_decoration_placeholder()
                .then_some(LogDecorationMode::Short)
        });
    let ref_snapshot = if options.all
        || requested_revs.iter().any(|rev| {
            matches!(
                rev.as_str(),
                "--branches" | "--heads" | "--remotes" | "--tags"
            ) || rev.starts_with("--branches=")
                || rev.starts_with("--heads=")
                || rev.starts_with("--remotes=")
                || rev.starts_with("--tags=")
        })
        || decoration_mode.is_some()
        || options.simplify_by_decoration
    {
        Some(collect_rev_list_ref_snapshot(&repo)?)
    } else {
        None
    };
    let revs = {
        let _trace = phase_trace("log.collect_revs");
        collect_rev_list_revs_with_snapshot(
            &repo,
            &store,
            options.all,
            requested_revs.clone(),
            ref_snapshot.as_ref(),
        )?
    };
    let commit_cache = CommitObjectCache::new(&store);
    let decorations = LogDecorations::load(
        &repo,
        &store,
        decoration_mode,
        parsed_log_revs.clear_decorations,
        ref_snapshot.as_ref(),
    )?;
    let notes_enabled = log_notes_enabled(
        &format,
        options.notes,
        options.no_notes,
        options.show_notes,
        options.show_notes_by_default,
        options.standard_notes,
        options.no_standard_notes,
    );
    let notes = LogNotes::load(&repo, &store, notes_enabled)?;
    let line_range_specs = options
        .line_ranges
        .iter()
        .map(|value| parse_log_line_range_spec(value))
        .collect::<Result<Vec<_>>>()?;
    if !line_range_specs.is_empty() {
        return log_with_line_ranges(
            &repo,
            &store,
            &commit_cache,
            &options,
            &format,
            &decorations,
            &notes,
            date_mode,
            expand_tabs,
            &parsed_log_revs,
            &pathspecs,
            &revs,
            line_range_specs,
        );
    }
    let pickaxe_options = PickaxeOptions {
        string: parsed_log_revs.pickaxe_string.as_deref(),
        regex: parsed_log_revs.pickaxe_regex.as_deref(),
        regex_mode: parsed_log_revs.pickaxe_regex_mode,
        all: parsed_log_revs.pickaxe_all,
    };
    let _accepted_full_history = options.full_history;
    let _accepted_dense = options.dense;
    let _accepted_sparse = options.sparse;
    let _accepted_show_pulls = options.show_pulls;
    let _accepted_simplify_merges = options.simplify_merges;
    let _accepted_children = options.children;
    let _accepted_graph = options.graph;
    let _accepted_objects = options.objects;
    let _accepted_filter = options.filter.as_deref();
    let history_order = if author_date_order {
        Some(HistoryCommitOrder::AuthorDate)
    } else if date_order {
        Some(HistoryCommitOrder::Date)
    } else if topo_order {
        Some(HistoryCommitOrder::Topo)
    } else {
        None
    };
    let simplify_history_topo = options.simplify_merges || options.simplify_by_decoration;
    let history_order_requires_full_collection = matches!(
        history_order,
        Some(HistoryCommitOrder::Topo | HistoryCommitOrder::AuthorDate)
    );
    let post_collection_filters = since.is_some()
        || until.is_some()
        || !options.grep.is_empty()
        || author_pattern.is_some()
        || committer_pattern.is_some()
        || min_parents.is_some()
        || max_parents.is_some()
        || options.ancestry_path
        || options.simplify_by_decoration
        || simplify_history_topo
        || history_order_requires_full_collection;
    let collect_max_count = if pickaxe_options.enabled() || post_collection_filters {
        None
    } else {
        expand_history_max_count(max_count, skip)
    };
    if let LogFormat::Custom { pattern, .. } = &format
        && log_format_supports_metadata_only(pattern)
        && diff_format.is_none()
        && !notes_enabled
        && pathspecs.is_empty()
        && !pickaxe_options.enabled()
        && since.is_none()
        && until.is_none()
        && options.grep.is_empty()
        && author_pattern.is_none()
        && committer_pattern.is_none()
        && min_parents.is_none()
        && max_parents.is_none()
        && !options.ancestry_path
        && !options.simplify_by_decoration
        && !simplify_history_topo
        && !no_walk
        && !options.first_parent
        && !options.left_right
        && !options.left_only
        && !options.right_only
        && !options.cherry
        && !options.cherry_pick
        && !options.cherry_mark
        && !options.boundary
    {
        let render_plan = LogMetadataRenderPlan::from_pattern(pattern, history_order);
        let uses_abbreviated_ids = format.uses_abbreviated_object_id() || options.abbrev_commit;
        let record_terminator = if zero {
            b"\0".as_slice()
        } else {
            b"\n".as_slice()
        };
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        if render_plan.supports_lightweight(history_order) {
            let exact_observed_roots = observed_log_root_ids_exact_family(
                &repo,
                &store,
                &requested_revs,
                ref_snapshot.as_ref(),
            )?;
            if render_plan.supports_author_only_lightweight(history_order) {
                if let Some(roots) = exact_observed_roots.as_ref()
                    && skip.is_none()
                    && max_count.is_none()
                    && !options.reverse
                    && !uses_abbreviated_ids
                    && let Some(commit_hints) =
                        collect_commit_render_hints_from_ids_with_exclusions_commit_graph(
                            &repo, roots, None,
                        )?
                {
                    let _trace = phase_trace("log.render_author_metadata_streaming");
                    render_commit_author_metadata_from_hints_streaming(
                        &store,
                        &commit_hints,
                        render_plan,
                        pattern,
                        GitHashAlgorithm::Sha1.digest_len() * 2,
                        &decorations,
                        record_terminator,
                        zero || format.terminates_lines(),
                        &mut out,
                    )?;
                    return Ok(());
                }
                let mut commits = {
                    let _trace = phase_trace("log.collect_commits");
                    if let Some(roots) = exact_observed_roots.as_ref() {
                        if let Some(commit_hints) =
                            collect_commit_render_hints_from_ids_with_exclusions_commit_graph(
                                &repo,
                                roots,
                                collect_max_count,
                            )?
                        {
                            collect_commit_author_metadata_from_hints(
                                &store,
                                &commit_hints,
                                render_plan,
                            )?
                        } else if let Some(commit_hints) =
                            collect_commit_render_hints_with_exclusions_commit_graph(
                                &repo,
                                &store,
                                &revs,
                                collect_max_count,
                            )?
                        {
                            collect_commit_author_metadata_from_hints(
                                &store,
                                &commit_hints,
                                render_plan,
                            )?
                        } else {
                            collect_commit_author_metadata_with_exclusions(
                                &repo,
                                &store,
                                &revs,
                                collect_max_count,
                                render_plan,
                            )?
                        }
                    } else if let Some(commit_hints) =
                        collect_commit_render_hints_with_exclusions_commit_graph(
                            &repo,
                            &store,
                            &revs,
                            collect_max_count,
                        )?
                    {
                        collect_commit_author_metadata_from_hints(
                            &store,
                            &commit_hints,
                            render_plan,
                        )?
                    } else {
                        collect_commit_author_metadata_with_exclusions(
                            &repo,
                            &store,
                            &revs,
                            collect_max_count,
                            render_plan,
                        )?
                    }
                };
                if let Some(skip) = skip {
                    commits = commits.into_iter().skip(skip).collect();
                }
                if let Some(max_count) = max_count {
                    commits.truncate(max_count);
                }
                if options.reverse {
                    commits.reverse();
                }
                let abbrev_len = log_output_abbrev_len(
                    &repo,
                    &store,
                    options.no_abbrev_commit,
                    uses_abbreviated_ids,
                    commits.iter().flat_map(|commit| {
                        std::iter::once(&commit.id).chain(commit.parents.iter())
                    }),
                )?;
                let _render_trace = phase_trace("log.render");
                for (index, commit) in commits.iter().enumerate() {
                    let rendered = render_log_format_author_metadata(
                        pattern,
                        &commit.id,
                        commit,
                        abbrev_len,
                        &decorations,
                    )?;
                    out.write_all(rendered.as_bytes())?;
                    if zero || format.terminates_lines() || index + 1 < commits.len() {
                        out.write_all(record_terminator)?;
                    }
                }
            } else {
                let mut commits = {
                    let _trace = phase_trace("log.collect_commits");
                    if let Some(roots) = exact_observed_roots.as_ref() {
                        if let Some(commit_hints) =
                            collect_commit_render_hints_from_ids_with_exclusions_commit_graph(
                                &repo,
                                roots,
                                collect_max_count,
                            )?
                        {
                            collect_commit_render_metadata_from_hints(
                                &store,
                                &commit_hints,
                                render_plan,
                            )?
                        } else if let Some(commit_hints) =
                            collect_commit_render_hints_with_exclusions_commit_graph(
                                &repo,
                                &store,
                                &revs,
                                collect_max_count,
                            )?
                        {
                            collect_commit_render_metadata_from_hints(
                                &store,
                                &commit_hints,
                                render_plan,
                            )?
                        } else {
                            collect_commit_render_metadata_with_exclusions(
                                &repo,
                                &store,
                                &revs,
                                collect_max_count,
                                render_plan,
                            )?
                        }
                    } else if let Some(commit_hints) =
                        collect_commit_render_hints_with_exclusions_commit_graph(
                            &repo,
                            &store,
                            &revs,
                            collect_max_count,
                        )?
                    {
                        collect_commit_render_metadata_from_hints(
                            &store,
                            &commit_hints,
                            render_plan,
                        )?
                    } else {
                        collect_commit_render_metadata_with_exclusions(
                            &repo,
                            &store,
                            &revs,
                            collect_max_count,
                            render_plan,
                        )?
                    }
                };
                if let Some(skip) = skip {
                    commits = commits.into_iter().skip(skip).collect();
                }
                if let Some(max_count) = max_count {
                    commits.truncate(max_count);
                }
                if options.reverse {
                    commits.reverse();
                }
                let abbrev_len = log_output_abbrev_len(
                    &repo,
                    &store,
                    options.no_abbrev_commit,
                    uses_abbreviated_ids,
                    commits.iter().flat_map(|commit| {
                        std::iter::once(&commit.id).chain(commit.parents.iter())
                    }),
                )?;
                let _render_trace = phase_trace("log.render");
                for (index, commit) in commits.iter().enumerate() {
                    let rendered = render_log_format_render_metadata(
                        pattern,
                        &commit.id,
                        commit,
                        abbrev_len,
                        &decorations,
                        date_mode,
                    )?;
                    out.write_all(rendered.as_bytes())?;
                    if zero || format.terminates_lines() || index + 1 < commits.len() {
                        out.write_all(record_terminator)?;
                    }
                }
            }
        } else {
            let mut commits = {
                let _trace = phase_trace("log.collect_commits");
                if let Some(commit_ids) = collect_commits_with_exclusions_commit_graph(
                    &repo,
                    &store,
                    &revs,
                    collect_max_count,
                )? {
                    let packed_first_store = store.packed_first();
                    commit_ids
                        .into_iter()
                        .map(|id| read_commit_metadata_from_store(&packed_first_store, &id))
                        .collect::<Result<Vec<_>>>()?
                } else {
                    collect_commit_metadata_with_exclusions(
                        &repo,
                        &store,
                        &revs,
                        collect_max_count,
                    )?
                }
            };
            if let Some(order) = history_order
                && order != HistoryCommitOrder::Date
            {
                commits = reorder_collected_commit_metadata(commits, order)?;
            }
            if let Some(skip) = skip {
                commits = commits.into_iter().skip(skip).collect();
            }
            if let Some(max_count) = max_count {
                commits.truncate(max_count);
            }
            if options.reverse {
                commits.reverse();
            }
            let abbrev_len = log_output_abbrev_len(
                &repo,
                &store,
                options.no_abbrev_commit,
                uses_abbreviated_ids,
                commits
                    .iter()
                    .flat_map(|commit| std::iter::once(&commit.id).chain(commit.parents.iter())),
            )?;
            let _render_trace = phase_trace("log.render");
            for (index, commit) in commits.iter().enumerate() {
                let rendered = render_log_format_metadata(
                    pattern,
                    &commit.id,
                    commit,
                    abbrev_len,
                    &decorations,
                    date_mode,
                )?;
                out.write_all(rendered.as_bytes())?;
                if zero || format.terminates_lines() || index + 1 < commits.len() {
                    out.write_all(record_terminator)?;
                }
            }
        }
        return Ok(());
    }
    if matches!(format, LogFormat::ShortOneline | LogFormat::FullOneline)
        && diff_format.is_none()
        && !notes_enabled
        && pathspecs.is_empty()
        && !pickaxe_options.enabled()
        && since.is_none()
        && until.is_none()
        && options.grep.is_empty()
        && author_pattern.is_none()
        && committer_pattern.is_none()
        && min_parents.is_none()
        && max_parents.is_none()
        && !options.ancestry_path
        && !options.simplify_by_decoration
        && !simplify_history_topo
        && history_order.is_none()
        && !no_walk
        && !options.first_parent
        && !options.left_right
        && !options.left_only
        && !options.right_only
        && !options.cherry
        && !options.cherry_pick
        && !options.cherry_mark
        && !options.boundary
        && !options.follow
        && !options.graph
        && !options.log_size
        && !options.diff_required
    {
        let mut commits = {
            let _trace = phase_trace("log.collect_commits.compact_oneline");
            collect_commit_oneline_with_exclusions(&repo, &store, &revs, collect_max_count)?
        };
        if let Some(skip) = skip {
            commits = commits.into_iter().skip(skip).collect();
        }
        if let Some(max_count) = max_count {
            commits.truncate(max_count);
        }
        if options.reverse {
            commits.reverse();
        }
        let abbrev_len = log_output_abbrev_len(
            &repo,
            &store,
            options.no_abbrev_commit,
            matches!(format, LogFormat::ShortOneline),
            commits
                .iter()
                .flat_map(|commit| std::iter::once(&commit.id).chain(commit.parents.iter())),
        )?;
        let record_terminator = if zero {
            b"\0".as_slice()
        } else {
            b"\n".as_slice()
        };
        let mut out = io::stdout().lock();
        let _render_trace = phase_trace("log.render.compact_oneline");
        for commit in &commits {
            match format {
                LogFormat::ShortOneline => {
                    out.write_all(short_object_id_len(&commit.id, abbrev_len).as_bytes())?;
                    if options.parents {
                        for parent in &commit.parents {
                            out.write_all(b" ")?;
                            out.write_all(short_object_id_len(parent, abbrev_len).as_bytes())?;
                        }
                    }
                }
                LogFormat::FullOneline => {
                    out.write_all(commit.id.to_hex().as_bytes())?;
                    if options.parents {
                        for parent in &commit.parents {
                            out.write_all(b" ")?;
                            out.write_all(parent.to_hex().as_bytes())?;
                        }
                    }
                }
                _ => unreachable!("compact oneline format guard"),
            }
            if let Some(wrapped) = decorations.wrapped(&commit.id) {
                out.write_all(wrapped.as_bytes())?;
            }
            out.write_all(b" ")?;
            out.write_all(commit.subject.as_bytes())?;
            out.write_all(record_terminator)?;
        }
        return Ok(());
    }
    let mut commits = {
        let _trace = phase_trace("log.collect_commits");
        if no_walk && !options.all {
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
    if let Some(until) = until {
        commits.retain(|entry| {
            signature_timestamp_timezone(&entry.commit.committer)
                .map(|(timestamp, _)| timestamp)
                .is_some_and(|timestamp| timestamp < until)
        });
    }
    if !options.grep.is_empty() {
        commits.retain(|entry| {
            shortlog_commit_matches_grep(
                &entry.commit.message,
                &options.grep,
                options.all_match,
                options.invert_grep,
                regexp_ignore_case,
                grep_mode,
            )
            .unwrap_or(false)
        });
    }
    if let Some(pattern) = author_pattern {
        commits.retain(|entry| {
            log_signature_matches_pattern(
                &entry.commit.author,
                pattern,
                regexp_ignore_case,
                grep_mode,
            )
        });
    }
    if let Some(pattern) = committer_pattern {
        commits.retain(|entry| {
            log_signature_matches_pattern(
                &entry.commit.committer,
                pattern,
                regexp_ignore_case,
                grep_mode,
            )
        });
    }
    if min_parents.is_some() || max_parents.is_some() {
        commits.retain(|entry| {
            log_parent_count_matches_bounds(entry.commit.parents.len(), min_parents, max_parents)
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
    }
    if options.ancestry_path {
        commits = filter_commits_by_ancestry_path(&repo, &store, &commit_cache, &revs, commits)?;
    }
    if options.simplify_by_decoration {
        let decorated = collect_default_log_decoration_ids(&repo, ref_snapshot.as_ref())?;
        commits.retain(|entry| decorated.contains(&entry.id));
    }
    let mut traversal_markers = HashMap::new();
    if options.left_right
        || options.left_only
        || options.right_only
        || options.cherry
        || options.cherry_pick
        || options.cherry_mark
        || options.boundary
    {
        let commit_ids = commits
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        let traversal = collect_history_traversal_decoration(
            &repo,
            &store,
            &commit_cache,
            &revs,
            &commit_ids,
            options.left_right || options.left_only || options.right_only,
            options.cherry,
            options.cherry_pick,
            options.cherry_mark,
            options.boundary,
        )?;
        if options.cherry {
            commits.retain(|entry| traversal.markers.contains_key(&entry.id));
        }
        if options.cherry_pick && !traversal.equivalent_ids.is_empty() {
            commits.retain(|entry| !traversal.equivalent_ids.contains(&entry.id));
        }
        if options.boundary {
            for id in &traversal.boundary_ids {
                commits.push(CollectedCommit {
                    id: id.clone(),
                    commit: commit_cache.read_commit(id)?,
                });
            }
        }
        traversal_markers = traversal.markers;
    }
    if options.left_only {
        commits.retain(|entry| {
            traversal_markers.get(&entry.id) == Some(&HistoryTraversalMarker::Left)
        });
    }
    if options.right_only {
        commits.retain(|entry| {
            traversal_markers.get(&entry.id) == Some(&HistoryTraversalMarker::Right)
        });
    }
    if let Some(order) =
        history_order.or_else(|| simplify_history_topo.then_some(HistoryCommitOrder::Topo))
    {
        commits = reorder_collected_commits(commits, order)?;
    }
    if let Some(skip) = skip {
        commits = commits.into_iter().skip(skip).collect();
    }
    if let Some(max_count) = max_count {
        commits.truncate(max_count);
    }
    if options.reverse {
        commits.reverse();
    }
    let default_commit_abbrev = options.abbrev_commit && !options.no_abbrev_commit;
    let abbrev_len = log_output_abbrev_len(
        &repo,
        &store,
        options.no_abbrev_commit,
        format.uses_abbreviated_object_id() || default_commit_abbrev,
        commits
            .iter()
            .flat_map(|entry| std::iter::once(&entry.id).chain(entry.commit.parents.iter())),
    )?;
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
            let mut visible_parent_indexes = Vec::new();
            for parent_index in 0..parent_count {
                if diff_format.is_some()
                    && !commit_diff_against_parent_has_entries(
                        &repo,
                        &store,
                        commit,
                        parent_index,
                        pickaxe_options,
                        &pathspecs,
                    )?
                {
                    continue;
                }
                visible_parent_indexes.push(parent_index);
            }
            if visible_parent_indexes.is_empty() {
                continue;
            }
            for (visible_index, parent_index) in visible_parent_indexes.iter().copied().enumerate()
            {
                let from_parent = if options.first_parent {
                    None
                } else {
                    commit.parents.get(parent_index)
                };
                let rendered = render_log_with_note_mode(
                    &format,
                    &entry.id,
                    commit,
                    from_parent,
                    options.parents,
                    abbrev_len,
                    traversal_markers.get(&entry.id).copied(),
                    default_commit_abbrev,
                    expand_tabs,
                    &decorations,
                    &notes,
                    date_mode,
                    options.standard_notes && !options.show_notes,
                )?;
                out.write_all(rendered.as_bytes())?;
                if let Some(diff_format) = diff_format {
                    if matches!(
                        diff_format,
                        ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary
                    ) {
                        out.write_all(b"---\n")?;
                    } else if rendered.ends_with('\n') {
                        out.write_all(b"\n")?;
                    } else {
                        out.write_all(b"\n\n")?;
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
                        None,
                        None,
                        zero,
                        None,
                        None,
                        false,
                    )?;
                    out = io::stdout().lock();
                }
                if zero
                    || visible_index + 1 < visible_parent_indexes.len()
                    || idx + 1 < commits.len()
                {
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
        let rendered = render_log_with_note_mode(
            &format,
            &entry.id,
            commit,
            None,
            options.parents,
            abbrev_len,
            traversal_markers.get(&entry.id).copied(),
            default_commit_abbrev,
            expand_tabs,
            &decorations,
            &notes,
            date_mode,
            options.standard_notes && !options.show_notes,
        )?;
        if options.log_size {
            writeln!(
                out,
                "log size {}",
                log_message_size(commit.message.as_slice())
            )?;
        }
        if options.graph {
            out.write_all(b"* ")?;
        }
        out.write_all(rendered.as_bytes())?;
        let root_patch_separator =
            options.root && commit.parents.is_empty() && format.separates_patch();
        if zero
            || terminates_lines
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
                None,
                None,
                zero,
                None,
                None,
                false,
            )?;
            out = io::stdout().lock();
            if next_output
                && !(terminates_lines
                    && matches!(
                        diff_format,
                        ShowDiffFormat::Raw
                            | ShowDiffFormat::Stat
                            | ShowDiffFormat::StatSummary
                            | ShowDiffFormat::Numstat
                            | ShowDiffFormat::Shortstat
                            | ShowDiffFormat::Summary
                            | ShowDiffFormat::NameOnly
                            | ShowDiffFormat::NameStatus
                    ))
            {
                out.write_all(b"\n")?;
            }
        }
    }
    Ok(())
}

fn parse_log_line_range_spec(value: &str) -> Result<LogLineRangeSpec> {
    let Some((range, path)) = value.rsplit_once(':') else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("unsupported log line range '{value}'"),
        });
    };
    if path.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("unsupported log line range '{value}'"),
        });
    }
    match parse_blame_line_range(range)? {
        BlameLineRange::Numeric { start, end } if start == 1 && end >= start => {
            Ok(LogLineRangeSpec {
                start,
                end,
                path: path.to_owned(),
            })
        }
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unsupported log line range '{value}'"),
        }),
    }
}

fn log_with_line_ranges(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    options: &LogOptions<'_>,
    format: &LogFormat<'_>,
    decorations: &LogDecorations,
    notes: &LogNotes,
    date_mode: LogDateMode<'_>,
    expand_tabs: bool,
    parsed_log_revs: &LogParsedRevs,
    pathspecs: &[Vec<u8>],
    revs: &RevListRevs,
    line_range_specs: Vec<LogLineRangeSpec>,
) -> Result<()> {
    if !pathspecs.is_empty()
        || parsed_log_revs.pickaxe_string.is_some()
        || parsed_log_revs.pickaxe_regex.is_some()
        || parsed_log_revs.patch
        || parsed_log_revs.pickaxe_regex_mode
        || parsed_log_revs.pickaxe_all
        || parsed_log_revs.decorate.is_some()
        || parsed_log_revs.clear_decorations
        || !parsed_log_revs.ignore_matching_lines.is_empty()
        || parsed_log_revs.format.is_some()
        || parsed_log_revs.pretty.is_some()
        || options.walk_reflogs
        || options.reflog
        || options.no_walk
        || options.do_walk
        || options.first_parent
        || options.reverse
        || options.zero
        || options.parents
        || options.graph
        || options.log_size
        || options.oneline
        || options.all
        || options.max_count.is_some()
        || options.skip.is_some()
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "unsupported log -L option combination".into(),
        });
    }

    let commits =
        collect_commit_objects_with_exclusions_cached(repo, store, commit_cache, revs, None)?;
    let abbrev_len = if options.no_abbrev_commit {
        GitHashAlgorithm::Sha1.digest_len() * 2
    } else {
        configured_default_abbrev_len(repo, store)?
    };
    let default_commit_abbrev = options.abbrev_commit && !options.no_abbrev_commit;
    let mut out = io::stdout().lock();

    for (spec_idx, spec) in line_range_specs.iter().enumerate() {
        let path_bytes = normalize_git_path(&spec.path)?.into_bytes();
        let mut wrote_any = false;
        for entry in &commits {
            let commit = entry.commit.as_ref();
            if commit.parents.len() > 1 {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "unsupported log line range '{}'",
                        options.line_ranges[spec_idx]
                    ),
                });
            }
            let new_lines = commit_file_lines_cached(store, commit_cache, &entry.id, &path_bytes)?;
            let old_lines = if let Some(parent) = commit.parents.first() {
                commit_file_lines_cached(store, commit_cache, parent, &path_bytes)?
            } else {
                Vec::new()
            };
            let selected_new = select_log_line_range_lines(&new_lines, spec.start, spec.end);
            let selected_old = select_log_line_range_lines(&old_lines, spec.start, spec.end);
            if selected_old == selected_new {
                continue;
            }
            if wrote_any {
                out.write_all(b"\n")?;
            }
            let rendered = render_log_with_note_mode(
                format,
                &entry.id,
                commit,
                None,
                false,
                abbrev_len,
                None,
                default_commit_abbrev,
                expand_tabs,
                decorations,
                notes,
                date_mode,
                options.standard_notes && !options.show_notes,
            )?;
            out.write_all(rendered.as_bytes())?;
            out.write_all(b"\n")?;
            write_log_line_range_patch(
                &mut out,
                &spec.path,
                !commit.parents.is_empty(),
                !new_lines.is_empty(),
                &join_log_line_range_lines(&selected_old),
                &join_log_line_range_lines(&selected_new),
            )?;
            wrote_any = true;
        }
    }
    Ok(())
}

fn select_log_line_range_lines(lines: &[Vec<u8>], start: usize, end: usize) -> Vec<Vec<u8>> {
    lines
        .iter()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start).saturating_add(1))
        .cloned()
        .collect()
}

fn join_log_line_range_lines(lines: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(lines.iter().map(Vec::len).sum());
    for line in lines {
        out.extend_from_slice(line);
    }
    out
}

fn write_log_line_range_patch(
    out: &mut impl Write,
    path: &str,
    parent_exists: bool,
    new_exists: bool,
    old: &[u8],
    new: &[u8],
) -> Result<()> {
    writeln!(out, "diff --git a/{path} b/{path}")?;
    let old_label = if parent_exists {
        format!("a/{path}")
    } else {
        "/dev/null".to_owned()
    };
    let new_label = if new_exists {
        format!("b/{path}")
    } else {
        "/dev/null".to_owned()
    };
    writeln!(out, "--- {old_label}")?;
    writeln!(out, "+++ {new_label}")?;
    let mut hunk = Vec::new();
    write_unified_full_file_hunk(&mut hunk, old, new, path, HunkFormatOptions::default())?;
    out.write_all(normalize_log_line_range_hunk_header(&hunk).as_bytes())?;
    Ok(())
}

fn normalize_log_line_range_hunk_header(hunk: &[u8]) -> String {
    let text = String::from_utf8_lossy(hunk);
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some((ranges, suffix)) = rest.split_once(" @@") {
                let mut parts = ranges.split_whitespace();
                if let (Some(old_range), Some(new_range), None) =
                    (parts.next(), parts.next(), parts.next())
                {
                    out.push_str("@@ ");
                    out.push_str(&normalize_log_line_range_token(old_range));
                    out.push(' ');
                    out.push_str(&normalize_log_line_range_token(new_range));
                    out.push_str(" @@");
                    out.push_str(suffix);
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    out
}

fn normalize_log_line_range_token(token: &str) -> String {
    if token.contains(',') || token.len() < 2 {
        return token.to_owned();
    }
    let (prefix, value) = token.split_at(1);
    if value == "0" {
        token.to_owned()
    } else {
        format!("{prefix}{value},1")
    }
}

fn log_message_size(message: &[u8]) -> usize {
    let mut len = message.len();
    while len > 0 && message[len - 1] == b'\n' {
        len -= 1;
    }
    len
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
        if resolve_objectish(repo, &rev).is_err() && repo.root.join(&rev).exists() {
            in_pathspecs = true;
            pathspecs.push(rev);
        } else {
            parsed_revs.push(rev);
        }
    }
    (parsed_revs, pathspecs)
}

struct LateLogRevOptions {
    revs: Vec<String>,
    max_count: Option<usize>,
    zero: bool,
    regexp_ignore_case: bool,
    basic_regexp: bool,
    extended_regexp: bool,
    fixed_strings: bool,
    perl_regexp: bool,
    topo_order: bool,
    date_order: bool,
    author_date_order: bool,
}

fn split_log_revs_and_count(
    revs: Vec<String>,
    max_count: Option<&str>,
    regexp_ignore_case: bool,
    basic_regexp: bool,
    extended_regexp: bool,
    fixed_strings: bool,
    perl_regexp: bool,
) -> Result<LateLogRevOptions> {
    let mut parsed_max_count = parse_log_max_count(max_count)?;
    let mut parsed_zero = false;
    let mut parsed_revs = Vec::new();
    let mut parsed_regexp_ignore_case = regexp_ignore_case;
    let mut parsed_basic_regexp = basic_regexp;
    let mut parsed_extended_regexp = extended_regexp;
    let mut parsed_fixed_strings = fixed_strings;
    let mut parsed_perl_regexp = perl_regexp;
    let mut parsed_topo_order = false;
    let mut parsed_date_order = false;
    let mut parsed_author_date_order = false;
    let mut iter = revs.into_iter();
    while let Some(rev) = iter.next() {
        if rev == "-z" {
            parsed_zero = true;
        } else if rev == "--regexp-ignore-case" {
            parsed_regexp_ignore_case = true;
        } else if rev == "--basic-regexp" {
            parsed_basic_regexp = true;
        } else if rev == "--extended-regexp" {
            parsed_extended_regexp = true;
        } else if rev == "--fixed-strings" {
            parsed_fixed_strings = true;
        } else if rev == "--perl-regexp" {
            parsed_perl_regexp = true;
        } else if rev == "--topo-order" {
            parsed_topo_order = true;
            parsed_date_order = false;
            parsed_author_date_order = false;
        } else if rev == "--date-order" {
            parsed_topo_order = false;
            parsed_date_order = true;
            parsed_author_date_order = false;
        } else if rev == "--author-date-order" {
            parsed_topo_order = false;
            parsed_date_order = false;
            parsed_author_date_order = true;
        } else if rev == "--find-copies-harder" {
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
        } else if let Some(value) = rev.strip_prefix("--find-renames=")
            && value.chars().all(|ch| ch.is_ascii_digit() || ch == '%')
        {
        } else if let Some(value) = rev.strip_prefix("--find-copies=")
            && value.chars().all(|ch| ch.is_ascii_digit() || ch == '%')
        {
        } else if let Some(value) = rev.strip_prefix("-M")
            && value.chars().all(|ch| ch.is_ascii_digit() || ch == '%')
        {
        } else if let Some(value) = rev.strip_prefix("-C")
            && value.chars().all(|ch| ch.is_ascii_digit() || ch == '%')
        {
        } else if rev == "-M" || rev == "-C" || rev == "--find-renames" || rev == "--find-copies" {
        } else {
            parsed_revs.push(rev);
        }
    }
    Ok(LateLogRevOptions {
        revs: parsed_revs,
        max_count: parsed_max_count,
        zero: parsed_zero,
        regexp_ignore_case: parsed_regexp_ignore_case,
        basic_regexp: parsed_basic_regexp,
        extended_regexp: parsed_extended_regexp,
        fixed_strings: parsed_fixed_strings,
        perl_regexp: parsed_perl_regexp,
        topo_order: parsed_topo_order,
        date_order: parsed_date_order,
        author_date_order: parsed_author_date_order,
    })
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
    encoding: Option<String>,
}

fn history_revs_with_stdin(revs: &[String], stdin: bool) -> Result<Vec<String>> {
    let mut resolved = revs.to_vec();
    if !stdin {
        return Ok(resolved);
    }
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(CliError::Io)?;
    resolved.extend(
        input
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned),
    );
    Ok(resolved)
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
    let mut parsed_encoding = None;
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
        } else if rev == "--encoding" {
            let Some(value) = iter.next() else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '--encoding' requires a value".into(),
                });
            };
            parsed_encoding = Some(value);
        } else if let Some(value) = rev.strip_prefix("--encoding=") {
            parsed_encoding = Some(value.to_owned());
        } else if rev == "--date" {
            if iter.next().is_none() {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '--date' requires a value".into(),
                });
            }
        } else if rev.starts_with("--date=") || rev == "--relative-date" {
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
        encoding: parsed_encoding,
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
    if !commit.parents.is_empty() {
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

fn commit_diff_against_parent_has_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    parent_index: usize,
    pickaxe_options: PickaxeOptions<'_>,
    pathspecs: &[Vec<u8>],
) -> Result<bool> {
    let parent_id = commit
        .parents
        .get(parent_index)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "merge parent index out of range".into(),
        })?;
    commit_diff_against_optional_parent_has_entries(
        repo,
        store,
        commit,
        Some(parent_id),
        pickaxe_options,
        pathspecs,
    )
}

fn commit_diff_against_optional_parent_has_entries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    parent_id: Option<&ObjectId>,
    pickaxe_options: PickaxeOptions<'_>,
    pathspecs: &[Vec<u8>],
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
    let mut seen = HashSet::new();
    for root in roots {
        if max_count.is_some_and(|limit| commits.len() >= limit) {
            break;
        }
        let id = resolve_commitish(repo, store, &root)?;
        if !seen.insert(id.clone()) {
            continue;
        }
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

fn log_reflog(
    options: &LogOptions<'_>,
    parsed_revs: &LogParsedRevs,
    max_count: Option<usize>,
) -> Result<()> {
    let repo = find_repo()?;
    let explicit_format = parsed_revs
        .format
        .as_deref()
        .or(options.format)
        .or(parsed_revs.pretty.as_deref())
        .or(options.pretty);
    let embedded_format = log_reflog_embedded_format(&parsed_revs.revs);
    let format = explicit_format.or(embedded_format).unwrap_or("%gd %H %gs");
    let format = format.strip_prefix("format:").unwrap_or(format);
    let date_arg = history_raw_date_arg(options.raw_args, options.date, options.relative_date)
        .or_else(|| log_reflog_embedded_date(&parsed_revs.revs).map(str::to_owned));
    let date_mode = parse_log_date_mode(date_arg.as_deref())?;
    let custom_format = explicit_format
        .or(embedded_format)
        .map(|value| value.strip_prefix("format:").unwrap_or(value));
    if let Some(patterns) = log_reflog_branch_patterns(&parsed_revs.revs) {
        return log_reflog_branches(
            &repo,
            format,
            custom_format,
            date_mode,
            &patterns,
            max_count,
            true,
            &options.grep_reflog,
            options.regexp_ignore_case,
        );
    }
    log_reflog_targets(
        &repo,
        options,
        parsed_revs,
        max_count,
        custom_format,
        date_mode,
        date_arg.as_deref(),
    )
}

#[derive(Clone, Copy)]
enum LogReflogTargetSelector {
    None,
    Index(usize),
    Date,
}

struct LogReflogRecord {
    display: String,
    index: usize,
    target_order: usize,
    selector: LogReflogTargetSelector,
    entry: ReflogEntry,
}

fn log_reflog_targets(
    repo: &GitRepo,
    options: &LogOptions<'_>,
    parsed_revs: &LogParsedRevs,
    max_count: Option<usize>,
    custom_format: Option<&str>,
    date_mode: LogDateMode<'_>,
    date_arg: Option<&str>,
) -> Result<()> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let (reflog_revs, implicit_pathspecs) =
        split_log_implicit_pathspecs(repo, parsed_revs.revs.clone());
    let mut targets = reflog_revs
        .iter()
        .filter(|rev| !rev.starts_with('-'))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        targets.push("HEAD");
    }
    let mut records = Vec::new();
    for (target_order, target) in targets.into_iter().enumerate() {
        collect_log_reflog_records(repo, target, target_order, &mut records)?;
    }
    records.sort_by(|left, right| {
        right
            .entry
            .timestamp
            .cmp(&left.entry.timestamp)
            .then_with(|| left.target_order.cmp(&right.target_order))
            .then_with(|| left.index.cmp(&right.index))
    });

    let (since, until) = resolve_history_age_bounds(
        options.raw_args,
        options.since.or(options.since_as_filter),
        options.max_age,
        options.until,
        options.min_age,
    );
    let since = parse_log_since(since).flatten();
    let until = parse_log_until(until).flatten();
    let (_, max_parents) = parse_log_parent_bounds(
        options.min_parents,
        options.no_min_parents,
        options.no_merges,
        options.max_parents,
        options.no_max_parents,
        options.merges,
    )?;
    let mut raw_pathspecs = parsed_revs.pathspecs.clone();
    raw_pathspecs.extend(implicit_pathspecs);
    let pathspecs = raw_pathspecs
        .iter()
        .map(|path| path_arg_to_repo_relative_lexical(repo, Path::new(path)))
        .collect::<Result<Vec<_>>>()?;
    let explicit_selector_date = history_has_explicit_date_arg(options.raw_args)
        || log_reflog_embedded_date(&parsed_revs.revs).is_some();
    let selector_date_mode = if explicit_selector_date {
        log_reflog_date_mode(date_arg.unwrap_or("default"))?
    } else {
        ReflogDateMode::Index
    };
    let format = log_reflog_format(options, parsed_revs)?;
    let decorations = LogDecorations::empty();
    let notes = LogNotes::empty();
    let abbrev_len = if options.no_abbrev_commit {
        GitHashAlgorithm::Sha1.digest_len() * 2
    } else {
        configured_default_abbrev_len(repo, &store)?
    };
    let default_commit_abbrev = options.reflog || options.abbrev_commit;
    let diff_format = options.diff_format(parsed_revs.patch);
    let merge_diff_mode = options.merge_diff_mode(repo, diff_format)?;
    let ignore_matching_lines = compile_ignore_matching_lines(&parsed_revs.ignore_matching_lines)?;
    let show_root = options.root || log_showroot_enabled(repo)?;
    let mut emitted = 0usize;
    let limit = max_count.unwrap_or(usize::MAX);
    let mut out = io::stdout().lock();

    for record in records {
        if emitted >= limit {
            break;
        }
        if since.is_some_and(|value| record.entry.timestamp < value)
            || until.is_some_and(|value| record.entry.timestamp > value)
            || record.entry.new_id == zero_object_id()
        {
            continue;
        }
        if !options.grep_reflog.is_empty()
            && !shortlog_commit_matches_grep(
                record.entry.message.as_bytes(),
                &options.grep_reflog,
                false,
                false,
                options.regexp_ignore_case,
                ShortlogPatternMode::Basic,
            )?
        {
            continue;
        }
        let commit = commit_cache.read_commit(&record.entry.new_id)?;
        let matches_pathspec = if pathspecs.is_empty() {
            true
        } else if commit.parents.len() > 1 {
            let mut matches_all_parents = true;
            for parent_index in 0..commit.parents.len() {
                if !commit_diff_against_parent_has_entries(
                    repo,
                    &store,
                    commit.as_ref(),
                    parent_index,
                    empty_pickaxe_options(),
                    &pathspecs,
                )? {
                    matches_all_parents = false;
                    break;
                }
            }
            matches_all_parents
        } else {
            show_commit_matches_pathspec(
                repo,
                &store,
                commit.as_ref(),
                merge_diff_mode,
                true,
                empty_pickaxe_options(),
                &pathspecs,
                None,
            )?
        };
        if max_parents.is_some_and(|limit| commit.parents.len() > limit) || !matches_pathspec {
            continue;
        }
        let record_date_mode = match record.selector {
            LogReflogTargetSelector::Date if !explicit_selector_date => ReflogDateMode::Default,
            LogReflogTargetSelector::Index(_) => ReflogDateMode::Index,
            _ => selector_date_mode,
        };
        let selector = reflog_selector(record.index, &record.entry, record_date_mode)?;
        let selector = format!("{}@{{{selector}}}", record.display);
        let rendered = render_log_reflog_record(
            &format,
            custom_format,
            &record,
            commit.as_ref(),
            &selector,
            abbrev_len,
            default_commit_abbrev,
            &decorations,
            &notes,
            date_mode,
        )?;
        if emitted > 0 && log_reflog_format_is_multiline(&format) {
            out.write_all(b"\n")?;
        }
        out.write_all(rendered.as_bytes())?;
        if let Some(diff_format) = diff_format {
            if !rendered.ends_with('\n') {
                out.write_all(b"\n")?;
            }
            out.write_all(b"\n")?;
            drop(out);
            show_commit_diff(
                repo,
                &store,
                commit.as_ref(),
                diff_format,
                merge_diff_mode,
                options.dense_combined,
                show_root,
                empty_pickaxe_options(),
                &ignore_matching_lines,
                &pathspecs,
                None,
                None,
                false,
                None,
                None,
                false,
            )?;
            out = io::stdout().lock();
        }
        emitted += 1;
    }
    Ok(())
}

fn collect_log_reflog_records(
    repo: &GitRepo,
    target: &str,
    target_order: usize,
    records: &mut Vec<LogReflogRecord>,
) -> Result<()> {
    let resolved_target = resolve_previous_checkout_expression(repo, target)?;
    let target = resolved_target.as_deref().unwrap_or(target);
    let (base, selector) = parse_log_reflog_target(target)?;
    let path = reflog_path(repo, &base)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && resolve_objectish(repo, &base).is_ok() =>
        {
            return Ok(());
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    let display = normalize_reflog_display_target(repo, &base)?;
    let mut index = 0usize;
    for_each_reflog_line_rev(file, |line| {
        let Some(entry) = parse_reflog_entry(line) else {
            return Ok(());
        };
        let include = match selector {
            LogReflogTargetSelector::None => true,
            LogReflogTargetSelector::Index(selected) => index >= selected,
            LogReflogTargetSelector::Date => true,
        };
        if include {
            records.push(LogReflogRecord {
                display: display.clone(),
                index,
                target_order,
                selector,
                entry,
            });
        }
        index += 1;
        Ok(())
    })?;
    if let Some(timestamp) = log_reflog_target_date(target)? {
        records.retain(|record| {
            record.target_order != target_order || record.entry.timestamp <= timestamp
        });
    }
    Ok(())
}

fn parse_log_reflog_target(target: &str) -> Result<(String, LogReflogTargetSelector)> {
    let Some(prefix) = target.strip_suffix('}') else {
        return Ok((target.to_owned(), LogReflogTargetSelector::None));
    };
    let Some((base, raw)) = prefix.rsplit_once("@{") else {
        return Ok((target.to_owned(), LogReflogTargetSelector::None));
    };
    let base = if base.is_empty() { "HEAD" } else { base }.to_owned();
    if let Ok(index) = raw.parse::<usize>() {
        return Ok((base, LogReflogTargetSelector::Index(index)));
    }
    parse_reflog_selector_timestamp(raw).map_err(CliError::Io)?;
    Ok((base, LogReflogTargetSelector::Date))
}

fn log_reflog_target_date(target: &str) -> Result<Option<i64>> {
    let Some(prefix) = target.strip_suffix('}') else {
        return Ok(None);
    };
    let Some((_, raw)) = prefix.rsplit_once("@{") else {
        return Ok(None);
    };
    if raw.parse::<usize>().is_ok() {
        return Ok(None);
    }
    parse_reflog_selector_timestamp(raw)
        .map(Some)
        .map_err(CliError::Io)
}

fn history_has_explicit_date_arg(raw_args: &[String]) -> bool {
    raw_args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--date" || arg == "--relative-date" || arg.starts_with("--date="))
}

fn log_reflog_date_mode(value: &str) -> Result<ReflogDateMode> {
    let value = value
        .strip_suffix("-local")
        .unwrap_or(value)
        .strip_prefix("format:")
        .unwrap_or(value);
    parse_reflog_date_mode(value).or(Ok(ReflogDateMode::Default))
}

fn log_reflog_format<'a>(
    options: &LogOptions<'a>,
    parsed_revs: &'a LogParsedRevs,
) -> Result<LogFormat<'a>> {
    let embedded = log_reflog_embedded_format(&parsed_revs.revs);
    let format = parsed_revs
        .format
        .as_deref()
        .or(options.format)
        .or(embedded);
    let pretty = parsed_revs.pretty.as_deref().or(options.pretty);
    if format.is_none() && pretty.is_none() && options.reflog && !options.oneline {
        return Ok(LogFormat::ShortOneline);
    }
    LogFormat::parse(options.oneline, format, pretty)
}

fn log_reflog_format_is_multiline(format: &LogFormat<'_>) -> bool {
    matches!(
        format,
        LogFormat::Default | LogFormat::Short | LogFormat::Full | LogFormat::Fuller
    )
}

fn render_log_reflog_record(
    format: &LogFormat<'_>,
    _custom_format: Option<&str>,
    record: &LogReflogRecord,
    commit: &zmin_git_core::CommitObject,
    selector: &str,
    abbrev_len: usize,
    default_commit_abbrev: bool,
    decorations: &LogDecorations,
    notes: &LogNotes,
    date_mode: LogDateMode<'_>,
) -> Result<String> {
    let mut rendered = match format {
        LogFormat::ShortOneline => format!(
            "{} {selector}: {}",
            short_object_id_len(&record.entry.new_id, abbrev_len),
            record.entry.message
        ),
        LogFormat::FullOneline => format!(
            "{} {selector}: {}",
            record.entry.new_id.to_hex(),
            record.entry.message
        ),
        LogFormat::Custom { pattern, .. } => render_log_reflog_custom_format(
            pattern,
            record,
            commit,
            selector,
            abbrev_len,
            decorations,
            notes,
            date_mode,
        )?,
        _ => {
            let rendered = format.render_with_context(
                &record.entry.new_id,
                commit,
                false,
                abbrev_len,
                None,
                default_commit_abbrev,
                true,
                decorations,
                notes,
                date_mode,
            )?;
            insert_reflog_headers(rendered, selector, &record.entry)
        }
    };
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn insert_reflog_headers(mut rendered: String, selector: &str, entry: &ReflogEntry) -> String {
    let first_end = rendered.find('\n').map(|index| index + 1).unwrap_or(0);
    let insert_at = if rendered[first_end..].starts_with("Merge:") {
        first_end
            + rendered[first_end..]
                .find('\n')
                .map(|index| index + 1)
                .unwrap_or(0)
    } else {
        first_end
    };
    let headers = format!(
        "Reflog: {selector} ({})\nReflog message: {}\n",
        entry.identity, entry.message
    );
    rendered.insert_str(insert_at, &headers);
    rendered
}

fn render_log_reflog_custom_format(
    pattern: &str,
    record: &LogReflogRecord,
    commit: &zmin_git_core::CommitObject,
    selector: &str,
    abbrev_len: usize,
    decorations: &LogDecorations,
    notes: &LogNotes,
    date_mode: LogDateMode<'_>,
) -> Result<String> {
    const SELECTOR_TOKEN: &str = "\u{1f}zmin-reflog-selector\u{1f}";
    const SUBJECT_TOKEN: &str = "\u{1f}zmin-reflog-subject\u{1f}";
    let mut rewritten = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' && chars.peek() == Some(&'g') {
            chars.next();
            match chars.next() {
                Some('d' | 'D') => rewritten.push_str(SELECTOR_TOKEN),
                Some('s') => rewritten.push_str(SUBJECT_TOKEN),
                Some(atom) => {
                    rewritten.push('%');
                    rewritten.push('g');
                    rewritten.push(atom);
                }
                None => rewritten.push_str("%g"),
            }
        } else {
            rewritten.push(ch);
        }
    }
    Ok(render_log_format(
        &rewritten,
        &record.entry.new_id,
        commit,
        abbrev_len,
        decorations,
        notes,
        date_mode,
    )?
    .replace(SELECTOR_TOKEN, selector)
    .replace(SUBJECT_TOKEN, &record.entry.message))
}

fn log_reflog_target(
    repo: &GitRepo,
    format: &str,
    custom_format: Option<&str>,
    date_mode: LogDateMode<'_>,
    target: &str,
    max_count: Option<usize>,
    allow_missing: bool,
    render_reflog_placeholders: bool,
    grep_reflog: &[String],
    regexp_ignore_case: bool,
) -> Result<usize> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let display_target = normalize_reflog_display_target(repo, target)?;
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
        if !grep_reflog.is_empty()
            && !shortlog_commit_matches_grep(
                entry.message.as_bytes(),
                grep_reflog,
                false,
                false,
                regexp_ignore_case,
                ShortlogPatternMode::Basic,
            )?
        {
            return Ok(());
        }
        let rendered = if let Some(pattern) = custom_format {
            if render_reflog_placeholders && log_format_uses_placeholder(pattern, 'g') {
                render_reflog_log_format(pattern, &display_target, entry_index, &entry)?
            } else {
                let commit = commit_cache.read_commit(&entry.new_id)?;
                render_log_format(
                    pattern,
                    &entry.new_id,
                    &commit,
                    7,
                    &LogDecorations::empty(),
                    &LogNotes::empty(),
                    date_mode,
                )?
            }
        } else {
            render_reflog_log_format(format, &display_target, entry_index, &entry)?
        };
        println!("{rendered}");
        emitted += 1;
        Ok(())
    })?;
    Ok(emitted)
}

fn normalize_reflog_display_target(repo: &GitRepo, target: &str) -> Result<String> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let resolved_previous = resolve_previous_checkout_expression(repo, target)
        .ok()
        .flatten();
    let target = resolved_previous.as_deref().unwrap_or(target);
    if target == "HEAD" || target.starts_with("refs/") {
        return Ok(target.to_owned());
    }
    if target == "stash" {
        return Ok("stash".to_owned());
    }
    if let Some(ref_name) = branch_checkout_ref(&refs, target)? {
        return Ok(branch_display_name(&ref_name));
    }
    Ok(target.to_owned())
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

fn log_reflog_embedded_date(revs: &[String]) -> Option<&str> {
    revs.iter().find_map(|rev| rev.strip_prefix("--date="))
}

fn log_reflog_branches(
    repo: &GitRepo,
    format: &str,
    custom_format: Option<&str>,
    date_mode: LogDateMode<'_>,
    patterns: &[String],
    max_count: Option<usize>,
    render_reflog_placeholders: bool,
    grep_reflog: &[String],
    regexp_ignore_case: bool,
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
        emitted += log_reflog_target(
            repo,
            format,
            custom_format,
            date_mode,
            &branch,
            Some(limit - emitted),
            true,
            render_reflog_placeholders,
            grep_reflog,
            regexp_ignore_case,
        )?;
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
        .format("%a %b %-d %H:%M:%S %Y %z")
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

fn raw_arg_last_value<'a>(raw_args: &'a [String], names: &[&str]) -> Option<&'a str> {
    let mut last = None;
    let mut idx = 0usize;
    while idx < raw_args.len() {
        let arg = raw_args[idx].as_str();
        for name in names {
            if arg == *name {
                if let Some(value) = raw_args.get(idx + 1) {
                    last = Some(value.as_str());
                }
                idx += 1;
                break;
            }
            if let Some(value) = arg.strip_prefix(name)
                && let Some(value) = value.strip_prefix('=')
            {
                last = Some(value);
                break;
            }
        }
        idx += 1;
    }
    last
}

fn raw_arg_last_toggle(
    raw_args: &[String],
    enabled_name: &str,
    disabled_name: &str,
) -> Option<bool> {
    let mut last = None;
    for arg in raw_args {
        match arg.as_str() {
            value if value == enabled_name => last = Some(true),
            value if value == disabled_name => last = Some(false),
            value if value.starts_with(&(enabled_name.to_owned() + "=")) => last = Some(true),
            value if value.starts_with(&(disabled_name.to_owned() + "=")) => last = Some(false),
            _ => {}
        }
    }
    last
}

fn raw_arg_present_before_dashdash(raw_args: &[String], name: &str) -> bool {
    raw_args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == name)
}

fn resolve_history_walk_mode(raw_args: &[String], no_walk: bool, do_walk: bool) -> bool {
    raw_arg_last_toggle(raw_args, "--no-walk", "--do-walk").unwrap_or(no_walk && !do_walk)
}

fn resolve_history_age_bounds<'a>(
    raw_args: &'a [String],
    since: Option<&'a str>,
    max_age: Option<&'a str>,
    until: Option<&'a str>,
    min_age: Option<&'a str>,
) -> (Option<&'a str>, Option<&'a str>) {
    let resolved_since = raw_arg_last_value(raw_args, &["--since", "--max-age"])
        .or(max_age)
        .or(since);
    let resolved_until = raw_arg_last_value(raw_args, &["--until", "--min-age"])
        .or(min_age)
        .or(until);
    (resolved_since, resolved_until)
}

fn parse_history_skip_value(value: &str) -> Result<usize> {
    value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("'{value}': not an integer"),
    })
}

fn resolve_history_skip(raw_args: &[String], skip: Option<usize>) -> Result<Option<usize>> {
    let mut last = None;
    let mut idx = 0usize;
    while idx < raw_args.len() {
        let arg = raw_args[idx].as_str();
        if arg == "--skip" {
            let Some(value) = raw_args.get(idx + 1) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option '--skip' requires a value".into(),
                });
            };
            last = Some(parse_history_skip_value(value)?);
            idx += 1;
        } else if let Some(value) = arg.strip_prefix("--skip=") {
            last = Some(parse_history_skip_value(value)?);
        }
        idx += 1;
    }
    Ok(last.or(skip))
}

fn expand_history_max_count(max_count: Option<usize>, skip: Option<usize>) -> Option<usize> {
    match (max_count, skip) {
        (Some(max_count), Some(skip)) => Some(max_count.saturating_add(skip)),
        (Some(max_count), None) => Some(max_count),
        (None, _) => None,
    }
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

fn parse_log_until(value: Option<&str>) -> Option<Option<i64>> {
    parse_log_since(value)
}

fn parse_log_parent_count_option(name: &str, value: Option<&str>) -> Result<Option<usize>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid value for '{name}': '{value}'"),
    })?;
    Ok(Some(parsed))
}

fn parse_log_parent_bounds(
    min_parents: Option<&str>,
    no_min_parents: bool,
    no_merges: bool,
    max_parents: Option<&str>,
    no_max_parents: bool,
    merges: bool,
) -> Result<(Option<usize>, Option<usize>)> {
    let mut min = parse_log_parent_count_option("--min-parents", min_parents)?;
    let mut max = parse_log_parent_count_option("--max-parents", max_parents)?;
    if no_min_parents {
        min = None;
    }
    if no_max_parents {
        max = None;
    }
    if merges {
        min = Some(2);
    }
    if no_merges {
        max = Some(1);
    }
    Ok((min, max))
}

fn log_signature_matches_pattern(
    signature: &[u8],
    pattern: &str,
    regexp_ignore_case: bool,
    mode: ShortlogPatternMode,
) -> bool {
    let rendered = String::from_utf8_lossy(signature);
    shortlog_text_matches_pattern(&rendered, pattern, regexp_ignore_case, mode)
}

fn log_parent_count_matches_bounds(
    parent_count: usize,
    min_parents: Option<usize>,
    max_parents: Option<usize>,
) -> bool {
    min_parents.is_none_or(|min| parent_count >= min)
        && max_parents.is_none_or(|max| parent_count <= max)
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
    Short,
    Full,
    Fuller,
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
        "" | "short" | "auto" | "true" | "yes" | "on" | "1" => Ok(Some(LogDecorationMode::Short)),
        "full" => Ok(Some(LogDecorationMode::Full)),
        "no" | "false" | "off" | "0" => Ok(None),
        other => Err(CliError::Fatal {
            code: 128,
            message: format!("invalid --decorate option: {other}"),
        }),
    }
}

fn log_configured_decoration_mode(repo: &GitRepo) -> Result<Option<LogDecorationMode>> {
    let Some(entry) = read_config_entry(repo, "log.decorate")? else {
        return Ok(None);
    };
    parse_log_decoration_mode(Some(if entry.implicit_bool {
        ""
    } else {
        &entry.value
    }))
    .map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("bad config value '{}' for 'log.decorate'", entry.value),
    })
}

pub(crate) struct LogDecorations {
    entries: HashMap<ObjectId, LogDecorationEntry>,
}

struct LogDecorationEntry {
    items: Vec<String>,
    joined: String,
    wrapped: String,
}

struct LogDecorationRefRow {
    ref_name: String,
    target: ObjectId,
    annotated_tag: bool,
}

impl LogDecorations {
    pub(crate) fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn load(
        repo: &GitRepo,
        store: &LooseObjectStore,
        mode: Option<LogDecorationMode>,
        clear_decorations: bool,
        ref_snapshot: Option<&RevListRefSnapshot>,
    ) -> Result<Self> {
        let Some(mode) = mode else {
            return Ok(Self::empty());
        };
        let head_refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let refs = RefStore::new(log_repo_common_dir(repo), GitHashAlgorithm::Sha1);
        let mut decorations = Self::empty();
        let current_branch = current_branch_ref(&head_refs).map_err(|error| match error {
            CliError::Io(inner)
                if matches!(
                    inner.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData
                ) =>
            {
                CliError::Fatal {
                    code: 128,
                    message: "your current branch appears to be broken".into(),
                }
            }
            other => other,
        })?;
        if let Some(head_id) = ref_snapshot
            .and_then(|snapshot| snapshot.head_id.clone())
            .or_else(|| {
                current_branch
                    .as_deref()
                    .and_then(|branch| refs.resolve(branch).ok())
            })
            .or_else(|| head_refs.resolve("HEAD").ok())
            && let Some(head_target) = peel_to_commit(store, head_id)?
        {
            let display = match current_branch.as_deref() {
                Some(branch) => format!("HEAD -> {}", decorate_ref_name(branch, mode, true)),
                None => "HEAD".to_owned(),
            };
            decorations.add(head_target, display);
        }

        let prefix = if clear_decorations { "refs/" } else { "refs/" };
        let mut ref_rows = Vec::<LogDecorationRefRow>::new();
        if let Some(snapshot) = ref_snapshot {
            for row in &snapshot.refs {
                if !row.ref_name.starts_with(prefix) {
                    continue;
                }
                if !clear_decorations && !log_decorates_ref_by_default(&row.ref_name) {
                    continue;
                }
                if current_branch.as_deref() == Some(row.ref_name.as_str()) {
                    continue;
                }
                let annotated_tag = if row.ref_name.starts_with("refs/tags/") {
                    matches!(store.read_object(&row.id)?.kind, GitObjectKind::Tag)
                } else {
                    false
                };
                let Some(target) = peel_to_commit(store, row.id.clone())? else {
                    continue;
                };
                ref_rows.push(LogDecorationRefRow {
                    ref_name: row.ref_name.clone(),
                    target,
                    annotated_tag,
                });
            }
        } else {
            refs.for_each_resolved_ref(prefix, |ref_name, id| {
                if !clear_decorations && !log_decorates_ref_by_default(ref_name) {
                    return Ok(());
                }
                if current_branch.as_deref() == Some(ref_name) {
                    return Ok(());
                }
                let annotated_tag = if ref_name.starts_with("refs/tags/") {
                    matches!(store.read_object(id)?.kind, GitObjectKind::Tag)
                } else {
                    false
                };
                let Some(target) = peel_to_commit(store, id.clone())? else {
                    return Ok(());
                };
                ref_rows.push(LogDecorationRefRow {
                    ref_name: ref_name.to_owned(),
                    target,
                    annotated_tag,
                });
                Ok::<(), CliError>(())
            })?;
        }
        ref_rows.sort_by(|left, right| {
            if left.ref_name.starts_with("refs/heads/") && right.ref_name.starts_with("refs/heads/")
            {
                return right.ref_name.cmp(&left.ref_name);
            }
            let left_key = log_decoration_sort_key(&left.ref_name, left.annotated_tag);
            let right_key = log_decoration_sort_key(&right.ref_name, right.annotated_tag);
            left_key.cmp(&right_key)
        });
        for row in ref_rows {
            decorations.add(row.target, decorate_ref_name(&row.ref_name, mode, false));
        }
        Ok(decorations)
    }

    fn add(&mut self, id: ObjectId, display: String) {
        let entry = self
            .entries
            .entry(id)
            .or_insert_with(|| LogDecorationEntry {
                items: Vec::new(),
                joined: String::new(),
                wrapped: String::new(),
            });
        if !entry.joined.is_empty() {
            entry.joined.push_str(", ");
        }
        entry.joined.push_str(&display);
        entry.items.push(display);
        entry.wrapped.clear();
        entry.wrapped.push_str(" (");
        entry.wrapped.push_str(&entry.joined);
        entry.wrapped.push(')');
    }

    fn get(&self, id: &ObjectId) -> Option<&[String]> {
        self.entries.get(id).map(|entry| entry.items.as_slice())
    }

    fn joined(&self, id: &ObjectId) -> Option<&str> {
        self.entries
            .get(id)
            .filter(|entry| !entry.joined.is_empty())
            .map(|entry| entry.joined.as_str())
    }

    fn wrapped(&self, id: &ObjectId) -> Option<&str> {
        self.entries
            .get(id)
            .filter(|entry| !entry.items.is_empty())
            .map(|entry| entry.wrapped.as_str())
    }
}

fn log_repo_common_dir(repo: &GitRepo) -> &Path {
    repo.objects_dir.parent().unwrap_or(&repo.git_dir)
}

fn log_decorates_ref_by_default(ref_name: &str) -> bool {
    ref_name.starts_with("refs/heads/")
        || ref_name.starts_with("refs/remotes/")
        || ref_name.starts_with("refs/tags/")
}

fn log_decoration_sort_key(ref_name: &str, annotated_tag: bool) -> (u8, u8, &str, u8, &str) {
    if let Some(short) = ref_name.strip_prefix("refs/tags/") {
        return (0, u8::from(!annotated_tag), short, 0, "");
    }
    if let Some(short) = ref_name.strip_prefix("refs/remotes/") {
        if let Some((remote, name)) = short.split_once('/') {
            let remote_head = u8::from(name == "HEAD");
            return (1, 0, remote, remote_head, name);
        }
        return (1, 0, short, 0, "");
    }
    if let Some(short) = ref_name.strip_prefix("refs/heads/") {
        return (2, 0, short, 0, "");
    }
    (3, 0, ref_name, 0, "")
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
    entries: HashMap<ObjectId, Vec<u8>>,
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
                let object_id =
                    ObjectId::from_hex(GitHashAlgorithm::Sha1, &object).map_err(CliError::Io)?;
                entries.insert(object_id, note.content);
            }
        }
        Ok(Self { entries })
    }

    pub(crate) fn get(&self, id: &ObjectId) -> Option<&[u8]> {
        self.entries.get(id).map(Vec::as_slice)
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
                "short" => Ok(Self::Short),
                "medium" | "default" => Ok(Self::Default),
                "full" => Ok(Self::Full),
                "fuller" => Ok(Self::Fuller),
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
            "short" => Ok(Self::Short),
            "full" => Ok(Self::Full),
            "fuller" => Ok(Self::Fuller),
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
            Self::Default | Self::Short | Self::Full | Self::Fuller => false,
            Self::Custom {
                terminates_lines, ..
            } => *terminates_lines,
        }
    }

    pub(crate) fn separates_patch(&self) -> bool {
        match self {
            Self::Default | Self::Short | Self::Full | Self::Fuller => true,
            Self::ShortOneline | Self::FullOneline => false,
            Self::Custom {
                terminates_lines, ..
            } => *terminates_lines,
        }
    }

    fn uses_decoration_placeholder(&self) -> bool {
        match self {
            Self::Custom { pattern, .. } => {
                log_format_uses_placeholder(pattern, 'd')
                    || log_format_uses_placeholder(pattern, 'D')
            }
            _ => false,
        }
    }

    fn uses_notes_display(&self) -> bool {
        match self {
            Self::Default | Self::Short | Self::Full | Self::Fuller => true,
            Self::Custom { pattern, .. } => log_format_uses_placeholder(pattern, 'N'),
            Self::ShortOneline | Self::FullOneline => false,
        }
    }

    fn uses_abbreviated_object_id(&self) -> bool {
        match self {
            Self::ShortOneline => true,
            Self::Custom { pattern, .. } => log_format_uses_placeholder(pattern, 'h'),
            Self::Default | Self::Short | Self::Full | Self::Fuller | Self::FullOneline => false,
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
        self.render_with_context_default_date(
            id,
            commit,
            parents,
            abbrev_len,
            None,
            false,
            true,
            &decorations,
            &notes,
        )
    }

    pub(crate) fn render_with_context_default_date(
        &self,
        id: &ObjectId,
        commit: &zmin_git_core::CommitObject,
        parents: bool,
        abbrev_len: usize,
        marker: Option<HistoryTraversalMarker>,
        default_commit_abbrev: bool,
        expand_tabs: bool,
        decorations: &LogDecorations,
        notes: &LogNotes,
    ) -> Result<String> {
        self.render_with_context(
            id,
            commit,
            parents,
            abbrev_len,
            marker,
            default_commit_abbrev,
            expand_tabs,
            decorations,
            notes,
            LogDateMode::Builtin(BlameDateMode::Default),
        )
    }

    fn render_with_context(
        &self,
        id: &ObjectId,
        commit: &zmin_git_core::CommitObject,
        parents: bool,
        abbrev_len: usize,
        marker: Option<HistoryTraversalMarker>,
        default_commit_abbrev: bool,
        expand_tabs: bool,
        decorations: &LogDecorations,
        notes: &LogNotes,
        date_mode: LogDateMode<'_>,
    ) -> Result<String> {
        match self {
            Self::Default => render_default_log(
                id,
                commit,
                parents,
                abbrev_len,
                marker,
                default_commit_abbrev,
                expand_tabs,
                decorations,
                notes,
                date_mode,
            ),
            Self::Short => render_short_log(
                id,
                commit,
                parents,
                abbrev_len,
                marker,
                default_commit_abbrev,
                expand_tabs,
                notes,
            ),
            Self::Full => render_full_log(
                id,
                commit,
                parents,
                abbrev_len,
                marker,
                default_commit_abbrev,
                expand_tabs,
                decorations,
                notes,
                false,
            ),
            Self::Fuller => render_full_log(
                id,
                commit,
                parents,
                abbrev_len,
                marker,
                default_commit_abbrev,
                expand_tabs,
                decorations,
                notes,
                true,
            ),
            Self::ShortOneline => Ok(format!(
                "{}{}{}{} {}",
                marker
                    .map(HistoryTraversalMarker::log_prefix)
                    .unwrap_or_default(),
                short_object_id_len(id, abbrev_len),
                short_parent_suffix(commit, parents, abbrev_len),
                render_oneline_decorations(decorations, id),
                commit_subject(&commit.message)
            )),
            Self::FullOneline => Ok(format!(
                "{}{}{}{} {}",
                marker
                    .map(HistoryTraversalMarker::log_prefix)
                    .unwrap_or_default(),
                id.to_hex(),
                parent_suffix(commit, parents),
                render_oneline_decorations(decorations, id),
                commit_subject(&commit.message)
            )),
            Self::Custom { pattern, .. } => render_log_format(
                pattern,
                id,
                commit,
                abbrev_len,
                decorations,
                notes,
                date_mode,
            ),
        }
    }

    fn render_with_from_parent(
        &self,
        id: &ObjectId,
        commit: &zmin_git_core::CommitObject,
        from_parent: Option<&ObjectId>,
        parents: bool,
        abbrev_len: usize,
        marker: Option<HistoryTraversalMarker>,
        default_commit_abbrev: bool,
        expand_tabs: bool,
        decorations: &LogDecorations,
        notes: &LogNotes,
        date_mode: LogDateMode<'_>,
    ) -> Result<String> {
        match (self, from_parent) {
            (Self::Default, Some(parent)) => render_default_log_from_parent(
                id,
                commit,
                parent,
                parents,
                abbrev_len,
                marker,
                default_commit_abbrev,
                expand_tabs,
                date_mode,
            ),
            _ => self.render_with_context(
                id,
                commit,
                parents,
                abbrev_len,
                marker,
                default_commit_abbrev,
                expand_tabs,
                decorations,
                notes,
                date_mode,
            ),
        }
    }
}

fn log_output_abbrev_len<'a, I>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    no_abbrev_commit: bool,
    uses_abbreviated_ids: bool,
    ids: I,
) -> Result<usize>
where
    I: IntoIterator<Item = &'a ObjectId>,
{
    let _trace = phase_trace("log.abbrev");
    let full_len = GitHashAlgorithm::Sha1.digest_len() * 2;
    if no_abbrev_commit || !uses_abbreviated_ids {
        return Ok(full_len);
    }
    let ids = ids.into_iter().cloned().collect::<Vec<_>>();
    configured_default_abbrev_len_for_ids(repo, store, &ids)
}

fn render_oneline_decorations(decorations: &LogDecorations, id: &ObjectId) -> String {
    decorations.wrapped(id).unwrap_or_default().to_owned()
}

fn render_log_decorations<'a>(
    decorations: &'a LogDecorations,
    id: &ObjectId,
    wrapped: bool,
) -> &'a str {
    if wrapped {
        decorations.wrapped(id).unwrap_or_default()
    } else {
        decorations.joined(id).unwrap_or_default()
    }
}

fn render_default_log(
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    parents: bool,
    abbrev_len: usize,
    marker: Option<HistoryTraversalMarker>,
    default_commit_abbrev: bool,
    expand_tabs: bool,
    decorations: &LogDecorations,
    notes: &LogNotes,
    date_mode: LogDateMode<'_>,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("commit ");
    if let Some(marker) = marker {
        out.push(marker.rev_list_prefix());
        out.push(' ');
    }
    if default_commit_abbrev {
        out.push_str(&short_object_id_len(id, abbrev_len));
    } else {
        out.push_str(&id.to_hex());
    }
    if parents {
        out.push_str(&parent_suffix(commit, true));
    }
    if let Some(wrapped) = decorations.wrapped(id) {
        out.push_str(wrapped);
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
    out.push_str(&format_log_date(&commit.author, date_mode)?);
    out.push_str("\n\n");
    for line in split_log_message_lines(&commit.message) {
        out.push_str("    ");
        append_indented_log_line(&mut out, line, expand_tabs);
        out.push('\n');
    }
    if let Some(note) = notes.get(id) {
        out.push('\n');
        out.push_str("Notes:\n");
        for line in split_log_message_lines(note) {
            out.push_str("    ");
            append_indented_log_line(&mut out, line, expand_tabs);
            out.push('\n');
        }
    }
    Ok(out)
}

fn render_short_log(
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    parents: bool,
    abbrev_len: usize,
    marker: Option<HistoryTraversalMarker>,
    default_commit_abbrev: bool,
    expand_tabs: bool,
    notes: &LogNotes,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("commit ");
    if let Some(marker) = marker {
        out.push(marker.rev_list_prefix());
        out.push(' ');
    }
    if default_commit_abbrev {
        out.push_str(&short_object_id_len(id, abbrev_len));
    } else {
        out.push_str(&id.to_hex());
    }
    if parents {
        out.push_str(&parent_suffix(commit, true));
    }
    out.push('\n');
    out.push_str("Author: ");
    out.push_str(&signature_name(&commit.author));
    out.push_str(" <");
    out.push_str(&signature_email(&commit.author));
    out.push_str(">\n\n");
    for line in split_log_message_lines(&commit.message) {
        out.push_str("    ");
        append_indented_log_line(&mut out, line, expand_tabs);
        out.push('\n');
    }
    if let Some(note) = notes.get(id) {
        out.push('\n');
        out.push_str("Notes:\n");
        for line in split_log_message_lines(note) {
            out.push_str("    ");
            append_indented_log_line(&mut out, line, expand_tabs);
            out.push('\n');
        }
    }
    Ok(out)
}

fn render_full_log(
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    parents: bool,
    abbrev_len: usize,
    marker: Option<HistoryTraversalMarker>,
    default_commit_abbrev: bool,
    expand_tabs: bool,
    decorations: &LogDecorations,
    notes: &LogNotes,
    fuller: bool,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("commit ");
    if let Some(marker) = marker {
        out.push(marker.rev_list_prefix());
        out.push(' ');
    }
    if default_commit_abbrev {
        out.push_str(&short_object_id_len(id, abbrev_len));
    } else {
        out.push_str(&id.to_hex());
    }
    if parents {
        out.push_str(&parent_suffix(commit, true));
    }
    if let Some(wrapped) = decorations.wrapped(id) {
        out.push_str(wrapped);
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
    if fuller {
        let author_date = signature_log_date(&commit.author)?;
        let committer_date = signature_log_date(&commit.committer)?;
        out.push_str("Author:     ");
        out.push_str(&signature_name(&commit.author));
        out.push_str(" <");
        out.push_str(&signature_email(&commit.author));
        out.push_str(">\n");
        out.push_str("AuthorDate: ");
        out.push_str(&author_date);
        out.push('\n');
        out.push_str("Commit:     ");
        out.push_str(&signature_name(&commit.committer));
        out.push_str(" <");
        out.push_str(&signature_email(&commit.committer));
        out.push_str(">\n");
        out.push_str("CommitDate: ");
        out.push_str(&committer_date);
        out.push_str("\n\n");
    } else {
        out.push_str("Author: ");
        out.push_str(&signature_name(&commit.author));
        out.push_str(" <");
        out.push_str(&signature_email(&commit.author));
        out.push_str(">\n");
        out.push_str("Commit: ");
        out.push_str(&signature_name(&commit.committer));
        out.push_str(" <");
        out.push_str(&signature_email(&commit.committer));
        out.push_str(">\n\n");
    }
    for line in split_log_message_lines(&commit.message) {
        out.push_str("    ");
        append_indented_log_line(&mut out, line, expand_tabs);
        out.push('\n');
    }
    if let Some(note) = notes.get(id) {
        out.push('\n');
        out.push_str("Notes:\n");
        for line in split_log_message_lines(note) {
            out.push_str("    ");
            append_indented_log_line(&mut out, line, expand_tabs);
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
    marker: Option<HistoryTraversalMarker>,
    default_commit_abbrev: bool,
    expand_tabs: bool,
    date_mode: LogDateMode<'_>,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("commit ");
    if let Some(marker) = marker {
        out.push(marker.rev_list_prefix());
        out.push(' ');
    }
    if default_commit_abbrev {
        out.push_str(&short_object_id_len(id, abbrev_len));
    } else {
        out.push_str(&id.to_hex());
    }
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
    out.push_str(&format_log_date(&commit.author, date_mode)?);
    out.push_str("\n\n");
    for line in split_log_message_lines(&commit.message) {
        out.push_str("    ");
        append_indented_log_line(&mut out, line, expand_tabs);
        out.push('\n');
    }
    Ok(out)
}

fn append_indented_log_line(out: &mut String, line: &[u8], expand_tabs: bool) {
    let text = String::from_utf8_lossy(line);
    if expand_tabs {
        let mut column = 0usize;
        for ch in text.chars() {
            if ch == '\t' {
                let spaces = 8 - (column % 8);
                for _ in 0..spaces {
                    out.push(' ');
                }
                column += spaces;
            } else {
                out.push(ch);
                column += 1;
            }
        }
    } else {
        out.push_str(&text);
    }
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

#[derive(Clone, Copy)]
struct LogMetadataRenderPlan {
    needs_parents: bool,
    needs_author_name: bool,
    needs_author_email: bool,
    needs_author_signature: bool,
    needs_author_timestamp: bool,
    needs_committer_name: bool,
    needs_committer_email: bool,
    needs_committer_signature: bool,
    needs_committer_timestamp: bool,
}

impl LogMetadataRenderPlan {
    fn from_pattern(pattern: &str, history_order: Option<HistoryCommitOrder>) -> Self {
        let mut plan = Self {
            needs_parents: false,
            needs_author_name: false,
            needs_author_email: false,
            needs_author_signature: false,
            needs_author_timestamp: matches!(history_order, Some(HistoryCommitOrder::AuthorDate)),
            needs_committer_name: false,
            needs_committer_email: false,
            needs_committer_signature: false,
            needs_committer_timestamp: matches!(
                history_order,
                Some(HistoryCommitOrder::Date | HistoryCommitOrder::Topo)
            ),
        };
        let mut chars = pattern.chars();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                continue;
            }
            let Some(atom) = chars.next() else {
                break;
            };
            match atom {
                'P' => plan.needs_parents = true,
                'x' => {
                    let _ = chars.next();
                    let _ = chars.next();
                }
                'a' => match chars.next() {
                    Some('n') => plan.needs_author_name = true,
                    Some('e') => plan.needs_author_email = true,
                    Some('d') => {
                        plan.needs_author_signature = true;
                        plan.needs_author_timestamp = true;
                    }
                    Some('t') => plan.needs_author_timestamp = true,
                    _ => {}
                },
                'c' => match chars.next() {
                    Some('n') => plan.needs_committer_name = true,
                    Some('e') => plan.needs_committer_email = true,
                    Some('d') => {
                        plan.needs_committer_signature = true;
                        plan.needs_committer_timestamp = true;
                    }
                    Some('t') => plan.needs_committer_timestamp = true,
                    _ => {}
                },
                _ => {}
            }
        }
        plan
    }

    fn supports_lightweight(self, history_order: Option<HistoryCommitOrder>) -> bool {
        !matches!(
            history_order,
            Some(HistoryCommitOrder::Topo | HistoryCommitOrder::AuthorDate)
        )
    }

    fn supports_author_only_lightweight(self, history_order: Option<HistoryCommitOrder>) -> bool {
        self.supports_lightweight(history_order)
            && !self.needs_author_signature
            && !self.needs_committer_name
            && !self.needs_committer_email
            && !self.needs_committer_signature
    }

    fn author_hint_prefix_bytes(self, hints: &CommitRenderMetadataHints) -> usize {
        if hints.parents.is_some() && hints.committer_timestamp.is_some() {
            return COMMIT_AUTHOR_HEADER_PREFIX_BYTES;
        }
        if hints.parents.is_some() || hints.committer_timestamp.is_some() {
            return COMMIT_AUTHOR_HINT_PREFIX_BYTES;
        }
        COMMIT_AUTHOR_METADATA_PREFIX_BYTES
    }
}

#[derive(Clone)]
struct CollectedCommitAuthorMetadata {
    id: ObjectId,
    parents: Arc<[ObjectId]>,
    author_name: Option<Arc<str>>,
    author_email: Option<Arc<str>>,
    author_timestamp: Option<i64>,
    committer_timestamp: i64,
}

type AuthorIdentityPool = HashMap<Arc<str>, Arc<str>>;

fn intern_author_identity(pool: &mut AuthorIdentityPool, value: &mut Option<Arc<str>>) {
    let Some(identity) = value.as_mut() else {
        return;
    };
    if let Some(interned) = pool.get(identity.as_ref()) {
        *identity = Arc::clone(interned);
    } else {
        pool.insert(Arc::clone(identity), Arc::clone(identity));
    }
}

fn intern_commit_author_metadata(
    pool: &mut AuthorIdentityPool,
    metadata: &mut CollectedCommitAuthorMetadata,
) {
    intern_author_identity(pool, &mut metadata.author_name);
    intern_author_identity(pool, &mut metadata.author_email);
}

struct CollectedCommitRenderMetadata {
    id: ObjectId,
    parents: Arc<[ObjectId]>,
    author_name: Option<String>,
    author_email: Option<String>,
    author_signature: Option<Vec<u8>>,
    author_timestamp: Option<i64>,
    committer_name: Option<String>,
    committer_email: Option<String>,
    committer_signature: Option<Vec<u8>>,
    committer_timestamp: Option<i64>,
}

#[derive(Clone, Default)]
struct CommitRenderMetadataHints {
    parents: Option<Arc<[ObjectId]>>,
    committer_timestamp: Option<i64>,
}

struct PendingCommitRenderMetadata {
    metadata: CollectedCommitRenderMetadata,
    timestamp: i64,
}

struct HeapPendingCommitRenderMetadata {
    pending: PendingCommitRenderMetadata,
    sequence: u64,
}

impl HeapPendingCommitRenderMetadata {
    fn new(pending: PendingCommitRenderMetadata, sequence: u64) -> Self {
        Self { pending, sequence }
    }
}

impl PartialEq for HeapPendingCommitRenderMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.pending.metadata.id == other.pending.metadata.id
            && self.pending.timestamp == other.pending.timestamp
            && self.sequence == other.sequence
    }
}

impl Eq for HeapPendingCommitRenderMetadata {}

impl PartialOrd for HeapPendingCommitRenderMetadata {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPendingCommitRenderMetadata {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.pending
            .timestamp
            .cmp(&other.pending.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
            .then_with(|| {
                other
                    .pending
                    .metadata
                    .id
                    .as_bytes()
                    .cmp(self.pending.metadata.id.as_bytes())
            })
    }
}

struct PendingCommitAuthorMetadata {
    metadata: CollectedCommitAuthorMetadata,
    timestamp: i64,
}

struct HeapPendingCommitAuthorMetadata {
    pending: PendingCommitAuthorMetadata,
    sequence: u64,
}

impl HeapPendingCommitAuthorMetadata {
    fn new(pending: PendingCommitAuthorMetadata, sequence: u64) -> Self {
        Self { pending, sequence }
    }
}

impl PartialEq for HeapPendingCommitAuthorMetadata {
    fn eq(&self, other: &Self) -> bool {
        self.pending.metadata.id == other.pending.metadata.id
            && self.pending.timestamp == other.pending.timestamp
            && self.sequence == other.sequence
    }
}

impl Eq for HeapPendingCommitAuthorMetadata {}

impl PartialOrd for HeapPendingCommitAuthorMetadata {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapPendingCommitAuthorMetadata {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.pending
            .timestamp
            .cmp(&other.pending.timestamp)
            .then_with(|| other.sequence.cmp(&self.sequence))
            .then_with(|| {
                other
                    .pending
                    .metadata
                    .id
                    .as_bytes()
                    .cmp(self.pending.metadata.id.as_bytes())
            })
    }
}

fn commit_render_metadata_complete(
    plan: LogMetadataRenderPlan,
    parents_ready: bool,
    author_name: &Option<String>,
    author_email: &Option<String>,
    author_signature: &Option<Vec<u8>>,
    author_timestamp: &Option<i64>,
    committer_name: &Option<String>,
    committer_email: &Option<String>,
    committer_signature: &Option<Vec<u8>>,
    committer_timestamp: &Option<i64>,
) -> bool {
    (!plan.needs_parents || parents_ready)
        && (!plan.needs_author_name || author_name.is_some())
        && (!plan.needs_author_email || author_email.is_some())
        && (!plan.needs_author_signature || author_signature.is_some())
        && (!plan.needs_author_timestamp || author_timestamp.is_some())
        && (!plan.needs_committer_name || committer_name.is_some())
        && (!plan.needs_committer_email || committer_email.is_some())
        && (!plan.needs_committer_signature || committer_signature.is_some())
        && (!plan.needs_committer_timestamp || committer_timestamp.is_some())
}

fn find_commit_header_value<'a>(bytes: &'a [u8], header: &[u8]) -> Option<&'a [u8]> {
    let mut start = 0;
    while start < bytes.len() {
        let end = bytes[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| start + offset)
            .unwrap_or(bytes.len());
        let line = &bytes[start..end];
        if line.is_empty() {
            return None;
        }
        if !line.starts_with(b" ")
            && let Some(value) = line.strip_prefix(header)
        {
            return Some(value);
        }
        start = end.saturating_add(1);
    }
    None
}

fn commit_header_line_complete(bytes: &[u8], header: &[u8]) -> bool {
    let mut start = 0;
    while start < bytes.len() {
        let Some(offset) = bytes[start..].iter().position(|byte| *byte == b'\n') else {
            return false;
        };
        let end = start + offset;
        let line = &bytes[start..end];
        if line.is_empty() {
            return false;
        }
        if !line.starts_with(b" ") && line.starts_with(header) {
            return true;
        }
        start = end.saturating_add(1);
    }
    false
}

fn parse_commit_render_metadata(
    algorithm: GitHashAlgorithm,
    id: &ObjectId,
    bytes: &[u8],
    plan: LogMetadataRenderPlan,
) -> Result<CollectedCommitRenderMetadata> {
    parse_commit_render_metadata_with_hints(
        algorithm,
        id,
        bytes,
        plan,
        CommitRenderMetadataHints::default(),
    )
}

fn parse_commit_author_metadata_with_hints(
    algorithm: GitHashAlgorithm,
    id: &ObjectId,
    bytes: &[u8],
    plan: LogMetadataRenderPlan,
    hints: CommitRenderMetadataHints,
) -> Result<CollectedCommitAuthorMetadata> {
    let hinted_parents = hints.parents;
    if let (Some(parents), Some(committer_timestamp)) =
        (hinted_parents.as_ref(), hints.committer_timestamp)
        && let Some(author) = find_commit_header_value(bytes, b"author ")
    {
        let (author_name, author_email) = if plan.needs_author_name && plan.needs_author_email {
            let (name, email) = signature_name_email(author);
            (Some(Arc::from(name)), Some(Arc::from(email)))
        } else {
            (
                plan.needs_author_name
                    .then(|| Arc::from(signature_name(author))),
                plan.needs_author_email
                    .then(|| Arc::from(signature_email(author))),
            )
        };
        let author_timestamp = if plan.needs_author_timestamp {
            Some(signature_timestamp(author).ok_or_else(|| {
                CliError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "commit has invalid author timestamp",
                ))
            })?)
        } else {
            None
        };
        return Ok(CollectedCommitAuthorMetadata {
            id: id.clone(),
            parents: Arc::clone(parents),
            author_name,
            author_email,
            author_timestamp,
            committer_timestamp,
        });
    }

    let mut parsed_parents = Vec::new();
    let mut saw_author_header = false;
    let mut author_name = None;
    let mut author_email = None;
    let mut author_timestamp = None;
    let mut committer_timestamp = hints.committer_timestamp;
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            break;
        }
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"parent ") {
            if plan.needs_parents && hinted_parents.is_none() {
                let hex =
                    std::str::from_utf8(value.split(|byte| *byte == b' ').next().unwrap_or(value))
                        .map_err(|_| {
                            CliError::Io(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "commit has invalid parent header",
                            ))
                        })?;
                parsed_parents.push(ObjectId::from_hex(algorithm, hex).map_err(|_| {
                    CliError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "commit has invalid parent header",
                    ))
                })?);
            }
        } else if let Some(value) = line.strip_prefix(b"author ") {
            saw_author_header = true;
            if plan.needs_author_name && plan.needs_author_email {
                let (name, email) = signature_name_email(value);
                author_name = Some(Arc::from(name));
                author_email = Some(Arc::from(email));
            } else {
                if plan.needs_author_name {
                    author_name = Some(Arc::from(signature_name(value)));
                }
                if plan.needs_author_email {
                    author_email = Some(Arc::from(signature_email(value)));
                }
            }
            if plan.needs_author_timestamp {
                author_timestamp = signature_timestamp(value);
            }
        } else if let Some(value) = line.strip_prefix(b"committer ")
            && committer_timestamp.is_none()
        {
            committer_timestamp = signature_timestamp(value);
        }
        if (!plan.needs_author_name || author_name.is_some())
            && (!plan.needs_author_email || author_email.is_some())
            && (!plan.needs_author_timestamp || author_timestamp.is_some())
            && committer_timestamp.is_some()
        {
            break;
        }
    }
    if plan.needs_author_name && author_name.is_none() {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit missing author header",
        )));
    }
    if plan.needs_author_email && author_email.is_none() {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit missing author header",
        )));
    }
    if plan.needs_author_timestamp && author_timestamp.is_none() {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid author timestamp",
        )));
    }
    if !saw_author_header {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit missing author header",
        )));
    }
    let committer_timestamp = committer_timestamp.ok_or_else(|| {
        CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid committer timestamp",
        ))
    })?;
    let parents = hinted_parents.unwrap_or_else(|| Arc::from(parsed_parents));
    Ok(CollectedCommitAuthorMetadata {
        id: id.clone(),
        parents,
        author_name,
        author_email,
        author_timestamp,
        committer_timestamp,
    })
}

fn read_pending_commit_author_metadata(
    store: &LooseObjectStore,
    id: ObjectId,
    plan: LogMetadataRenderPlan,
) -> Result<PendingCommitAuthorMetadata> {
    let metadata = read_commit_author_metadata_with_hints(
        store,
        &id,
        plan,
        CommitRenderMetadataHints::default(),
    )?;
    let timestamp = metadata.committer_timestamp;
    Ok(PendingCommitAuthorMetadata {
        metadata,
        timestamp,
    })
}

fn collect_commit_author_metadata_with_exclusions(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
    plan: LogMetadataRenderPlan,
) -> Result<Vec<CollectedCommitAuthorMetadata>> {
    let excluded = if revs.exclude.is_empty() {
        std::collections::HashSet::new()
    } else {
        collect_rev_list_excluded_commits(repo, store, revs)?
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
    };
    let traversal_plan = LogMetadataRenderPlan {
        needs_parents: true,
        needs_committer_timestamp: true,
        ..plan
    };
    let mut pending = std::collections::BinaryHeap::new();
    let mut scheduled = std::collections::HashSet::new();
    let mut sequence = 0_u64;
    for rev in &revs.include {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitAuthorMetadata::new(
                read_pending_commit_author_metadata(store, id, traversal_plan)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::new();
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let metadata = pending_commit.metadata;
        if excluded.contains(&metadata.id) {
            continue;
        }
        let is_shallow = shallow_commits.contains(&metadata.id);
        if !is_shallow {
            for parent in metadata.parents.iter() {
                if scheduled.insert(parent.clone()) {
                    pending.push(HeapPendingCommitAuthorMetadata::new(
                        read_pending_commit_author_metadata(store, parent.clone(), traversal_plan)?,
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

fn parse_commit_render_metadata_with_hints(
    algorithm: GitHashAlgorithm,
    id: &ObjectId,
    bytes: &[u8],
    plan: LogMetadataRenderPlan,
    hints: CommitRenderMetadataHints,
) -> Result<CollectedCommitRenderMetadata> {
    let hinted_parents = hints.parents;
    let mut parsed_parents = Vec::new();
    let mut parents_ready = !plan.needs_parents || hinted_parents.is_some();
    let mut author_name = None;
    let mut author_email = None;
    let mut author_signature = None;
    let mut author_timestamp = None;
    let mut committer_name = None;
    let mut committer_email = None;
    let mut committer_signature = None;
    let mut committer_timestamp = if plan.needs_committer_timestamp {
        hints.committer_timestamp
    } else {
        None
    };
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            break;
        }
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"parent ") {
            if plan.needs_parents && !parents_ready {
                let hex =
                    std::str::from_utf8(value.split(|byte| *byte == b' ').next().unwrap_or(value))
                        .map_err(|_| {
                            CliError::Io(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "commit has invalid parent header",
                            ))
                        })?;
                let parent = ObjectId::from_hex(algorithm, hex).map_err(|_| {
                    CliError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "commit has invalid parent header",
                    ))
                })?;
                parsed_parents.push(parent);
            }
        } else if let Some(value) = line.strip_prefix(b"author ") {
            parents_ready = true;
            if plan.needs_author_name && plan.needs_author_email {
                let (name, email) = signature_name_email(value);
                author_name = Some(name);
                author_email = Some(email);
            } else {
                if plan.needs_author_name {
                    author_name = Some(signature_name(value));
                }
                if plan.needs_author_email {
                    author_email = Some(signature_email(value));
                }
            }
            if plan.needs_author_signature {
                author_signature = Some(value.to_vec());
            }
            if plan.needs_author_timestamp {
                author_timestamp = signature_timestamp(value);
            }
        } else if let Some(value) = line.strip_prefix(b"committer ") {
            parents_ready = true;
            if plan.needs_committer_name {
                committer_name = Some(signature_name(value));
            }
            if plan.needs_committer_email {
                committer_email = Some(signature_email(value));
            }
            if plan.needs_committer_signature {
                committer_signature = Some(value.to_vec());
            }
            if plan.needs_committer_timestamp {
                committer_timestamp = committer_timestamp.or_else(|| signature_timestamp(value));
            }
        }
        if commit_render_metadata_complete(
            plan,
            parents_ready,
            &author_name,
            &author_email,
            &author_signature,
            &author_timestamp,
            &committer_name,
            &committer_email,
            &committer_signature,
            &committer_timestamp,
        ) {
            break;
        }
    }
    if plan.needs_parents && !parents_ready {
        parents_ready = true;
    }
    if plan.needs_author_timestamp && author_timestamp.is_none() {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid author timestamp",
        )));
    }
    if plan.needs_committer_timestamp && committer_timestamp.is_none() {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid committer timestamp",
        )));
    }
    if plan.needs_parents && !parents_ready {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit parent headers could not be resolved",
        )));
    }
    let parents = hinted_parents.unwrap_or_else(|| Arc::from(parsed_parents));
    Ok(CollectedCommitRenderMetadata {
        id: id.clone(),
        parents,
        author_name,
        author_email,
        author_signature,
        author_timestamp,
        committer_name,
        committer_email,
        committer_signature,
        committer_timestamp,
    })
}

fn read_pending_commit_render_metadata(
    store: &LooseObjectStore,
    id: ObjectId,
    plan: LogMetadataRenderPlan,
) -> Result<PendingCommitRenderMetadata> {
    let metadata = read_commit_render_metadata_with_hints(
        store,
        &id,
        plan,
        CommitRenderMetadataHints::default(),
    )?;
    let timestamp = metadata.committer_timestamp.ok_or_else(|| {
        CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "commit has invalid committer timestamp",
        ))
    })?;
    Ok(PendingCommitRenderMetadata {
        metadata,
        timestamp,
    })
}

fn collect_commit_render_metadata_with_exclusions(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    max_count: Option<usize>,
    plan: LogMetadataRenderPlan,
) -> Result<Vec<CollectedCommitRenderMetadata>> {
    let excluded = if revs.exclude.is_empty() {
        std::collections::HashSet::new()
    } else {
        collect_rev_list_excluded_commits(repo, store, revs)?
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
    };
    let traversal_plan = LogMetadataRenderPlan {
        needs_parents: true,
        needs_committer_timestamp: true,
        ..plan
    };
    let mut pending = std::collections::BinaryHeap::new();
    let mut scheduled = std::collections::HashSet::new();
    let mut sequence = 0_u64;
    for rev in &revs.include {
        let id = resolve_commitish(repo, store, rev)?;
        if scheduled.insert(id.clone()) {
            pending.push(HeapPendingCommitRenderMetadata::new(
                read_pending_commit_render_metadata(store, id, traversal_plan)?,
                sequence,
            ));
            sequence += 1;
        }
    }
    let shallow_commits = read_shallow_commits(repo)?;
    let mut out = Vec::new();
    while let Some(heap_entry) = pending.pop() {
        let pending_commit = heap_entry.pending;
        let metadata = pending_commit.metadata;
        if excluded.contains(&metadata.id) {
            continue;
        }
        let is_shallow = shallow_commits.contains(&metadata.id);
        if !is_shallow {
            for parent in metadata.parents.iter() {
                if scheduled.insert(parent.clone()) {
                    pending.push(HeapPendingCommitRenderMetadata::new(
                        read_pending_commit_render_metadata(store, parent.clone(), traversal_plan)?,
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

fn read_commit_render_metadata(
    store: &LooseObjectStore,
    id: &ObjectId,
    plan: LogMetadataRenderPlan,
) -> Result<CollectedCommitRenderMetadata> {
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    parse_commit_render_metadata(id.algorithm(), id, &object.content, plan)
}

fn read_commit_render_metadata_with_hints<S>(
    store: &S,
    id: &ObjectId,
    plan: LogMetadataRenderPlan,
    hints: CommitRenderMetadataHints,
) -> Result<CollectedCommitRenderMetadata>
where
    S: GitObjectStore + ?Sized,
{
    let prefix_or_full =
        store.read_object_prefix_or_full(id, COMMIT_AUTHOR_METADATA_PREFIX_BYTES)?;
    if prefix_or_full.object.kind == GitObjectKind::Commit {
        let parsed = parse_commit_render_metadata_with_hints(
            id.algorithm(),
            id,
            &prefix_or_full.object.content,
            plan,
            hints.clone(),
        );
        if let Ok(metadata) = parsed {
            return Ok(metadata);
        }
        if prefix_or_full.is_complete {
            return parsed;
        }
    } else if prefix_or_full.is_complete {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    parse_commit_render_metadata_with_hints(id.algorithm(), id, &object.content, plan, hints)
}

fn read_commit_author_metadata_with_hints<S>(
    store: &S,
    id: &ObjectId,
    plan: LogMetadataRenderPlan,
    hints: CommitRenderMetadataHints,
) -> Result<CollectedCommitAuthorMetadata>
where
    S: GitObjectStore + ?Sized,
{
    let prefix_len = plan.author_hint_prefix_bytes(&hints);
    let prefix_or_full = store.read_object_prefix_or_full(id, prefix_len)?;
    if prefix_or_full.object.kind == GitObjectKind::Commit {
        let parsed = parse_commit_author_metadata_with_hints(
            id.algorithm(),
            id,
            &prefix_or_full.object.content,
            plan,
            hints.clone(),
        );
        if let Ok(metadata) = parsed.as_ref() {
            if !prefix_or_full.is_complete
                && prefix_len == COMMIT_AUTHOR_HEADER_PREFIX_BYTES
                && !commit_header_line_complete(&prefix_or_full.object.content, b"author ")
            {
                // The short author-only prefix can end in the middle of the
                // author header. Fall back to the full object so output stays
                // byte-identical to stock Git.
            } else {
                return Ok(metadata.clone());
            }
        }
        if prefix_or_full.is_complete {
            return parsed;
        }
    } else if prefix_or_full.is_complete {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "object is not a commit",
        )));
    }
    parse_commit_author_metadata_with_hints(id.algorithm(), id, &object.content, plan, hints)
}

fn collect_commit_render_metadata_from_hints(
    store: &LooseObjectStore,
    hints: &[CommitGraphRenderHint],
    plan: LogMetadataRenderPlan,
) -> Result<Vec<CollectedCommitRenderMetadata>> {
    let _trace = phase_trace("log.collect_commits.render_metadata_from_hints");
    let workers = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(hints.len())
        .min(PARALLEL_LOG_METADATA_MAX_WORKERS);
    if workers <= 1 || hints.len() < PARALLEL_LOG_RENDER_METADATA_MIN_COMMITS {
        let packed_first_store = store.packed_first();
        return hints
            .iter()
            .map(|hint| {
                read_commit_render_metadata_with_hints(
                    &packed_first_store,
                    &hint.id,
                    plan,
                    CommitRenderMetadataHints {
                        parents: Some(hint.parents.clone()),
                        committer_timestamp: Some(hint.committer_timestamp),
                    },
                )
            })
            .collect();
    }

    let chunk_len = hints.len().div_ceil(workers).max(1);
    let chunked = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in hints.chunks(chunk_len) {
            let worker_store = store.fork_for_parallel_reads();
            handles.push(
                std::thread::Builder::new()
                    .stack_size(PARALLEL_LOG_METADATA_STACK_BYTES)
                    .spawn_scoped(
                        scope,
                        move || -> Result<Vec<CollectedCommitRenderMetadata>> {
                            let packed_first_store = worker_store.packed_first();
                            chunk
                                .iter()
                                .map(|hint| {
                                    read_commit_render_metadata_with_hints(
                                        &packed_first_store,
                                        &hint.id,
                                        plan,
                                        CommitRenderMetadataHints {
                                            parents: Some(hint.parents.clone()),
                                            committer_timestamp: Some(hint.committer_timestamp),
                                        },
                                    )
                                })
                                .collect()
                        },
                    )?,
            );
        }
        let mut chunked = Vec::with_capacity(handles.len());
        for handle in handles {
            chunked.push(handle.join().map_err(|_| {
                CliError::Message("parallel log metadata worker panicked".into())
            })??);
        }
        Ok::<_, CliError>(chunked)
    })?;

    let mut out = Vec::with_capacity(hints.len());
    for mut chunk in chunked {
        out.append(&mut chunk);
    }
    Ok(out)
}

fn collect_commit_author_metadata_from_hints(
    store: &LooseObjectStore,
    hints: &[CommitGraphRenderHint],
    plan: LogMetadataRenderPlan,
) -> Result<Vec<CollectedCommitAuthorMetadata>> {
    let _trace = phase_trace("log.collect_commits.author_metadata_from_hints");
    let workers = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(hints.len())
        .min(PARALLEL_LOG_AUTHOR_METADATA_MAX_WORKERS);
    if workers <= 1 || hints.len() < PARALLEL_LOG_AUTHOR_METADATA_MIN_COMMITS {
        let packed_first_store = store.packed_first();
        return hints
            .iter()
            .map(|hint| {
                read_commit_author_metadata_with_hints(
                    &packed_first_store,
                    &hint.id,
                    plan,
                    CommitRenderMetadataHints {
                        parents: Some(hint.parents.clone()),
                        committer_timestamp: Some(hint.committer_timestamp),
                    },
                )
            })
            .collect();
    }

    let chunk_len = hints.len().div_ceil(workers).max(1);
    let chunked = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for chunk in hints.chunks(chunk_len) {
            let worker_store = store.fork_for_parallel_reads();
            handles.push(
                std::thread::Builder::new()
                    .stack_size(PARALLEL_LOG_METADATA_STACK_BYTES)
                    .spawn_scoped(
                        scope,
                        move || -> Result<Vec<CollectedCommitAuthorMetadata>> {
                            let packed_first_store = worker_store.packed_first();
                            let mut author_identity_pool = AuthorIdentityPool::new();
                            chunk
                                .iter()
                                .map(|hint| {
                                    let mut metadata = read_commit_author_metadata_with_hints(
                                        &packed_first_store,
                                        &hint.id,
                                        plan,
                                        CommitRenderMetadataHints {
                                            parents: Some(hint.parents.clone()),
                                            committer_timestamp: Some(hint.committer_timestamp),
                                        },
                                    )?;
                                    intern_commit_author_metadata(
                                        &mut author_identity_pool,
                                        &mut metadata,
                                    );
                                    Ok(metadata)
                                })
                                .collect()
                        },
                    )?,
            );
        }
        let mut chunked = Vec::with_capacity(handles.len());
        for handle in handles {
            chunked.push(handle.join().map_err(|_| {
                CliError::Message("parallel log author metadata worker panicked".into())
            })??);
        }
        Ok::<_, CliError>(chunked)
    })?;

    let mut out = Vec::with_capacity(hints.len());
    for mut chunk in chunked {
        out.append(&mut chunk);
    }
    Ok(out)
}

fn render_commit_author_metadata_from_hints_streaming<W: Write>(
    store: &LooseObjectStore,
    hints: &[CommitGraphRenderHint],
    plan: LogMetadataRenderPlan,
    pattern: &str,
    abbrev_len: usize,
    decorations: &LogDecorations,
    record_terminator: &[u8],
    terminate_last: bool,
    out: &mut W,
) -> Result<()> {
    let workers = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(hints.len())
        .min(PARALLEL_LOG_AUTHOR_METADATA_MAX_WORKERS);
    if workers <= 1 || hints.len() < PARALLEL_LOG_AUTHOR_METADATA_MIN_COMMITS {
        let packed_first_store = store.packed_first();
        let mut author_identity_pool = AuthorIdentityPool::new();
        for (index, hint) in hints.iter().enumerate() {
            let mut metadata = read_commit_author_metadata_with_hints(
                &packed_first_store,
                &hint.id,
                plan,
                CommitRenderMetadataHints {
                    parents: Some(hint.parents.clone()),
                    committer_timestamp: Some(hint.committer_timestamp),
                },
            )?;
            intern_commit_author_metadata(&mut author_identity_pool, &mut metadata);
            let rendered = render_log_format_author_metadata(
                pattern,
                &metadata.id,
                &metadata,
                abbrev_len,
                decorations,
            )?;
            out.write_all(rendered.as_bytes())?;
            if terminate_last || index + 1 < hints.len() {
                out.write_all(record_terminator)?;
            }
        }
        return Ok(());
    }

    std::thread::scope(|scope| {
        let mut receivers = Vec::with_capacity(workers);
        let mut handles = Vec::with_capacity(workers);
        for worker_index in 0..workers {
            let worker_store = store.fork_for_parallel_reads();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            receivers.push(receiver);
            handles.push(
                std::thread::Builder::new()
                    .stack_size(PARALLEL_LOG_METADATA_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        let packed_first_store = worker_store.packed_first();
                        let mut author_identity_pool = AuthorIdentityPool::new();
                        for hint in hints.iter().skip(worker_index).step_by(workers) {
                            let metadata = read_commit_author_metadata_with_hints(
                                &packed_first_store,
                                &hint.id,
                                plan,
                                CommitRenderMetadataHints {
                                    parents: Some(hint.parents.clone()),
                                    committer_timestamp: Some(hint.committer_timestamp),
                                },
                            )
                            .map(|mut metadata| {
                                intern_commit_author_metadata(
                                    &mut author_identity_pool,
                                    &mut metadata,
                                );
                                metadata
                            });
                            if sender.send(metadata).is_err() {
                                break;
                            }
                        }
                    })?,
            );
        }

        for index in 0..hints.len() {
            let metadata = receivers[index % workers].recv().map_err(|_| {
                CliError::Message("parallel log author metadata worker stopped early".into())
            })??;
            let rendered = render_log_format_author_metadata(
                pattern,
                &metadata.id,
                &metadata,
                abbrev_len,
                decorations,
            )?;
            out.write_all(rendered.as_bytes())?;
            if terminate_last || index + 1 < hints.len() {
                out.write_all(record_terminator)?;
            }
        }
        for handle in handles {
            handle.join().map_err(|_| {
                CliError::Message("parallel log author metadata worker panicked".into())
            })?;
        }
        Ok(())
    })
}

fn log_format_supports_metadata_only(pattern: &str) -> bool {
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        let Some(atom) = chars.next() else {
            return false;
        };
        match atom {
            '%' | 'H' | 'h' | 'P' | 'd' | 'D' => {}
            'x' => {
                let Some(high) = chars.next() else {
                    return false;
                };
                let Some(low) = chars.next() else {
                    return false;
                };
                if !high.is_ascii_hexdigit() || !low.is_ascii_hexdigit() {
                    return false;
                }
            }
            'a' | 'c' => match chars.next() {
                Some('n' | 'e' | 'd' | 't') => {}
                _ => return false,
            },
            _ => return false,
        }
    }
    true
}

fn render_log_format_metadata(
    pattern: &str,
    id: &ObjectId,
    commit: &CollectedCommitMetadata,
    abbrev_len: usize,
    decorations: &LogDecorations,
    date_mode: LogDateMode<'_>,
) -> Result<String> {
    let mut out = String::new();
    let mut chars = pattern.chars();
    let mut author_name = None;
    let mut author_email = None;
    let mut author_timestamp = None;
    let mut author_formatted_date = None;
    let mut committer_name = None;
    let mut committer_email = None;
    let mut committer_timestamp = None;
    let mut committer_formatted_date = None;
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
            'd' => out.push_str(&render_log_decorations(decorations, id, true)),
            'D' => out.push_str(&render_log_decorations(decorations, id, false)),
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
                    'n' => out.push_str(
                        author_name.get_or_insert_with(|| signature_name(&commit.author)),
                    ),
                    'e' => out.push_str(
                        author_email.get_or_insert_with(|| signature_email(&commit.author)),
                    ),
                    'd' => out.push_str(
                        author_formatted_date
                            .get_or_insert(format_log_date(&commit.author, date_mode)?),
                    ),
                    't' => out.push_str(
                        &author_timestamp
                            .get_or_insert_with(|| signature_timestamp(&commit.author).unwrap_or(0))
                            .to_string(),
                    ),
                    _ => unreachable!("metadata-only log format validation mismatch"),
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
                    'n' => out.push_str(
                        committer_name.get_or_insert_with(|| signature_name(&commit.committer)),
                    ),
                    'e' => out.push_str(
                        committer_email.get_or_insert_with(|| signature_email(&commit.committer)),
                    ),
                    'd' => out.push_str(
                        committer_formatted_date
                            .get_or_insert(format_log_date(&commit.committer, date_mode)?),
                    ),
                    't' => out.push_str(
                        &committer_timestamp
                            .get_or_insert_with(|| {
                                signature_timestamp(&commit.committer).unwrap_or(0)
                            })
                            .to_string(),
                    ),
                    _ => unreachable!("metadata-only log format validation mismatch"),
                }
            }
            _ => unreachable!("metadata-only log format validation mismatch"),
        }
    }
    Ok(out)
}

fn render_log_format_author_metadata(
    pattern: &str,
    id: &ObjectId,
    commit: &CollectedCommitAuthorMetadata,
    abbrev_len: usize,
    decorations: &LogDecorations,
) -> Result<String> {
    let mut out = String::with_capacity(
        pattern.len()
            + commit.author_name.as_ref().map_or(0, |value| value.len())
            + commit.author_email.as_ref().map_or(0, |value| value.len())
            + commit.parents.len().saturating_mul(41)
            + decorations.wrapped(id).map_or(0, str::len),
    );
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
            'd' => out.push_str(&render_log_decorations(decorations, id, true)),
            'D' => out.push_str(&render_log_decorations(decorations, id, false)),
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
                    'n' => out.push_str(commit.author_name.as_deref().unwrap_or_default()),
                    'e' => out.push_str(commit.author_email.as_deref().unwrap_or_default()),
                    't' => out.push_str(&commit.author_timestamp.unwrap_or_default().to_string()),
                    _ => unreachable!("author-only log format validation mismatch"),
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
                    't' => out.push_str(&commit.committer_timestamp.to_string()),
                    _ => unreachable!("author-only log format validation mismatch"),
                }
            }
            _ => unreachable!("author-only log format validation mismatch"),
        }
    }
    Ok(out)
}

fn render_log_format_render_metadata(
    pattern: &str,
    id: &ObjectId,
    commit: &CollectedCommitRenderMetadata,
    abbrev_len: usize,
    decorations: &LogDecorations,
    date_mode: LogDateMode<'_>,
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
            'd' => out.push_str(&render_log_decorations(decorations, id, true)),
            'D' => out.push_str(&render_log_decorations(decorations, id, false)),
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
                    'n' => out.push_str(commit.author_name.as_deref().unwrap_or("")),
                    'e' => out.push_str(commit.author_email.as_deref().unwrap_or("")),
                    'd' => out.push_str(&format_log_date(
                        commit.author_signature.as_deref().unwrap_or_default(),
                        date_mode,
                    )?),
                    't' => out.push_str(&commit.author_timestamp.unwrap_or_default().to_string()),
                    _ => unreachable!("metadata-only log format validation mismatch"),
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
                    'n' => out.push_str(commit.committer_name.as_deref().unwrap_or("")),
                    'e' => out.push_str(commit.committer_email.as_deref().unwrap_or("")),
                    'd' => out.push_str(&format_log_date(
                        commit.committer_signature.as_deref().unwrap_or_default(),
                        date_mode,
                    )?),
                    't' => {
                        out.push_str(&commit.committer_timestamp.unwrap_or_default().to_string())
                    }
                    _ => unreachable!("metadata-only log format validation mismatch"),
                }
            }
            _ => unreachable!("metadata-only log format validation mismatch"),
        }
    }
    Ok(out)
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
    date_mode: LogDateMode<'_>,
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
            'd' => out.push_str(&render_log_decorations(decorations, id, true)),
            'D' => out.push_str(&render_log_decorations(decorations, id, false)),
            'N' => {
                if let Some(note) = notes.get(id) {
                    out.push_str(&String::from_utf8_lossy(note));
                }
            }
            'b' => out.push_str(&commit_body(&commit.message)),
            'B' => out.push_str(&String::from_utf8_lossy(&commit.message)),
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
                    'd' => out.push_str(&format_log_date(&commit.author, date_mode)?),
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
                    'd' => out.push_str(&format_log_date(&commit.committer, date_mode)?),
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
            'g' => {
                let Some(next) = chars.next() else {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "unterminated reflog log format placeholder".into(),
                    });
                };
                match next {
                    'D' | 'd' | 'n' | 'N' | 'e' | 'E' | 's' => {}
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

fn commit_body(message: &[u8]) -> String {
    let Some(newline) = message.iter().position(|byte| *byte == b'\n') else {
        return String::new();
    };
    let body = &message[newline + 1..];
    let body = body
        .strip_prefix(b"\r\n")
        .or_else(|| body.strip_prefix(b"\n"))
        .unwrap_or(body);
    String::from_utf8_lossy(body).into_owned()
}

fn log_notes_enabled(
    format: &LogFormat<'_>,
    notes: bool,
    no_notes: bool,
    show_notes: bool,
    show_notes_by_default: bool,
    standard_notes: bool,
    no_standard_notes: bool,
) -> bool {
    if no_notes {
        return false;
    }
    if standard_notes {
        if show_notes {
            return !no_standard_notes;
        }
        return false;
    }
    if no_standard_notes {
        if show_notes_by_default {
            return true;
        }
        return false;
    }
    if show_notes || show_notes_by_default {
        return true;
    }
    notes || matches!(format, LogFormat::Default)
}

fn history_raw_date_arg(
    raw_args: &[String],
    default_date: Option<&str>,
    relative_date: bool,
) -> Option<String> {
    let fallback = if relative_date && default_date.is_none() {
        Some("relative".to_owned())
    } else {
        default_date.map(str::to_owned)
    };
    if raw_args.is_empty() {
        return fallback;
    }
    let args = raw_args
        .first()
        .filter(|arg| !arg.starts_with('-'))
        .map(|_| &raw_args[1..])
        .unwrap_or(raw_args);
    let mut selected = None::<String>;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if arg == "--relative-date" {
            selected = Some("relative".to_owned());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--date=") {
            selected = Some(value.to_owned());
            continue;
        }
        if arg == "--date"
            && let Some(value) = iter.peek()
        {
            selected = Some((**value).clone());
            iter.next();
        }
    }
    selected.or(fallback)
}

fn observed_log_root_ids_exact_family(
    _repo: &GitRepo,
    store: &LooseObjectStore,
    requested_revs: &[String],
    ref_snapshot: Option<&RevListRefSnapshot>,
) -> Result<Option<Vec<ObjectId>>> {
    let Some(snapshot) = ref_snapshot else {
        return Ok(None);
    };
    if requested_revs.is_empty() {
        return Ok(None);
    }
    let mut include_head = false;
    let mut include_branches = false;
    let mut include_remotes = false;
    let mut include_tags = false;
    for rev in requested_revs {
        match rev.as_str() {
            "HEAD" => include_head = true,
            "--branches" | "--heads" => include_branches = true,
            "--remotes" => include_remotes = true,
            "--tags" => include_tags = true,
            _ => return Ok(None),
        }
    }
    if !include_head || !include_branches || !include_remotes || !include_tags {
        return Ok(None);
    }

    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    if let Some(head_id) = &snapshot.head_id
        && seen.insert(head_id.clone())
    {
        roots.push(head_id.clone());
    }
    for row in &snapshot.refs {
        let include = row.ref_name.starts_with("refs/heads/")
            || row.ref_name.starts_with("refs/remotes/")
            || row.ref_name.starts_with("refs/tags/");
        if !include {
            continue;
        }
        let id = if row.ref_name.starts_with("refs/tags/") {
            peel_to_commit(store, row.id.clone())?.ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("revision '{}' is not a commit", row.ref_name),
            })?
        } else {
            row.id.clone()
        };
        if seen.insert(id.clone()) {
            roots.push(id);
        }
    }
    Ok(Some(roots))
}

fn rev_list_supports_notes_display(
    notes: bool,
    show_notes: bool,
    show_notes_by_default: bool,
    standard_notes: bool,
    _no_standard_notes: bool,
) -> bool {
    if notes || show_notes {
        return true;
    }
    show_notes_by_default && !standard_notes
}

fn log_expand_tabs_enabled(
    _encoding: Option<&str>,
    expand_tabs: bool,
    no_expand_tabs: bool,
) -> bool {
    if no_expand_tabs {
        return false;
    }
    let _ = expand_tabs;
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryTraversalMarker {
    Boundary,
    Equivalent,
    Left,
    Right,
    PlainCherry,
}

impl HistoryTraversalMarker {
    fn rev_list_prefix(self) -> char {
        match self {
            Self::Boundary => '-',
            Self::Equivalent => '=',
            Self::Left => '<',
            Self::Right => '>',
            Self::PlainCherry => '+',
        }
    }

    fn log_prefix(self) -> String {
        format!("{} ", self.rev_list_prefix())
    }
}

#[derive(Debug, Default)]
struct HistoryTraversalDecoration {
    markers: HashMap<ObjectId, HistoryTraversalMarker>,
    boundary_ids: Vec<ObjectId>,
    equivalent_ids: HashSet<ObjectId>,
}

fn collect_history_traversal_decoration(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    revs: &RevListRevs,
    commit_ids: &[ObjectId],
    left_right: bool,
    cherry: bool,
    cherry_pick: bool,
    cherry_mark: bool,
    boundary: bool,
) -> Result<HistoryTraversalDecoration> {
    if !left_right && !cherry && !cherry_pick && !cherry_mark && !boundary {
        return Ok(HistoryTraversalDecoration::default());
    }

    let included_ids = commit_ids.iter().cloned().collect::<HashSet<_>>();
    let (left_ids, right_ids) =
        collect_history_traversal_side_sets(repo, store, commit_cache, revs, &included_ids)?;
    let equivalent_ids = if cherry || cherry_pick || cherry_mark {
        collect_history_patch_equivalent_ids(store, commit_cache, &left_ids, &right_ids)?
    } else {
        HashSet::new()
    };
    let boundary_ids = if boundary {
        collect_history_boundary_ids(store, commit_ids, &included_ids)?
    } else {
        Vec::new()
    };

    let mut markers = HashMap::new();
    for id in commit_ids {
        let marker = if left_right {
            if cherry_mark && equivalent_ids.contains(id) {
                Some(HistoryTraversalMarker::Equivalent)
            } else if left_ids.contains(id) {
                Some(HistoryTraversalMarker::Left)
            } else if right_ids.contains(id) {
                Some(HistoryTraversalMarker::Right)
            } else {
                None
            }
        } else if (cherry || cherry_mark) && equivalent_ids.contains(id) {
            right_ids
                .contains(id)
                .then_some(HistoryTraversalMarker::Equivalent)
        } else if cherry || cherry_mark {
            if left_ids.contains(id) {
                None
            } else {
                right_ids
                    .contains(id)
                    .then_some(HistoryTraversalMarker::PlainCherry)
            }
        } else {
            None
        };
        if let Some(marker) = marker {
            markers.insert(id.clone(), marker);
        }
    }
    for id in &boundary_ids {
        markers.insert(id.clone(), HistoryTraversalMarker::Boundary);
    }

    Ok(HistoryTraversalDecoration {
        markers,
        boundary_ids,
        equivalent_ids,
    })
}

fn collect_history_traversal_side_sets(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    revs: &RevListRevs,
    included_ids: &HashSet<ObjectId>,
) -> Result<(HashSet<ObjectId>, HashSet<ObjectId>)> {
    let Some(symmetric_diff) = revs.symmetric_diff.as_ref() else {
        return Ok((HashSet::new(), included_ids.clone()));
    };
    let excluded_ids = symmetric_diff
        .merge_bases
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let left_root = resolve_commitish(repo, store, &symmetric_diff.left)?;
    let right_root = resolve_commitish(repo, store, &symmetric_diff.right)?;
    let left_ids = collect_commits_from_ids_cached_with_excluded(
        repo,
        commit_cache,
        std::slice::from_ref(&left_root),
        None,
        &excluded_ids,
    )?
    .into_iter()
    .filter(|id| included_ids.contains(id))
    .collect::<HashSet<_>>();
    let right_ids = collect_commits_from_ids_cached_with_excluded(
        repo,
        commit_cache,
        std::slice::from_ref(&right_root),
        None,
        &excluded_ids,
    )?
    .into_iter()
    .filter(|id| included_ids.contains(id))
    .collect::<HashSet<_>>();
    Ok((left_ids, right_ids))
}

fn collect_history_patch_equivalent_ids(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    left_ids: &HashSet<ObjectId>,
    right_ids: &HashSet<ObjectId>,
) -> Result<HashSet<ObjectId>> {
    let tree_cache = TreeObjectCache::new(store);
    let mut left_patch_ids = HashSet::new();
    for id in left_ids {
        if let Some(patch_id) = reference_commands::commit_patch_id_for_cherry_cached(
            store,
            commit_cache,
            &tree_cache,
            id,
        )? {
            left_patch_ids.insert(patch_id);
        }
    }
    let mut right_patch_ids = HashSet::new();
    for id in right_ids {
        if let Some(patch_id) = reference_commands::commit_patch_id_for_cherry_cached(
            store,
            commit_cache,
            &tree_cache,
            id,
        )? {
            right_patch_ids.insert(patch_id);
        }
    }
    let shared_patch_ids = left_patch_ids
        .intersection(&right_patch_ids)
        .cloned()
        .collect::<HashSet<_>>();
    if shared_patch_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let mut equivalent_ids = HashSet::new();
    for id in left_ids.iter().chain(right_ids.iter()) {
        if reference_commands::commit_patch_id_for_cherry_cached(
            store,
            commit_cache,
            &tree_cache,
            id,
        )?
        .as_ref()
        .is_some_and(|patch_id| shared_patch_ids.contains(patch_id))
        {
            equivalent_ids.insert(id.clone());
        }
    }
    Ok(equivalent_ids)
}

fn collect_history_boundary_ids(
    store: &LooseObjectStore,
    commit_ids: &[ObjectId],
    included_ids: &HashSet<ObjectId>,
) -> Result<Vec<ObjectId>> {
    let mut boundary_ids = Vec::new();
    let mut seen = HashSet::new();
    for id in commit_ids {
        for parent in read_commit_parents_uncached(store, id)? {
            if !included_ids.contains(&parent) && seen.insert(parent.clone()) {
                boundary_ids.push(parent);
            }
        }
    }
    Ok(boundary_ids)
}

#[derive(Debug, Clone)]
pub(crate) struct ShowOptions<'a> {
    pub(crate) no_patch: bool,
    pub(crate) patch: bool,
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
    pub(crate) find_renames: Option<&'a str>,
    pub(crate) find_copies: Option<&'a str>,
    pub(crate) find_copies_harder: bool,
    pub(crate) encoding: Option<&'a str>,
    pub(crate) expand_tabs: bool,
    pub(crate) no_expand_tabs: bool,
    pub(crate) notes: bool,
    pub(crate) no_notes: bool,
    pub(crate) show_notes: bool,
    pub(crate) show_notes_by_default: bool,
    pub(crate) standard_notes: bool,
    pub(crate) no_standard_notes: bool,
    pub(crate) show_signature: bool,
    pub(crate) abbrev: Option<&'a str>,
    pub(crate) no_abbrev: bool,
    pub(crate) abbrev_commit: bool,
    pub(crate) no_abbrev_commit: bool,
    pub(crate) parents: bool,
    pub(crate) root: bool,
    pub(crate) combined: bool,
    pub(crate) separate_merges: bool,
    pub(crate) first_parent: bool,
    pub(crate) diff_merges: Option<&'a str>,
    pub(crate) no_diff_merges: bool,
    pub(crate) do_walk: bool,
    pub(crate) no_walk: Option<&'a str>,
    pub(crate) format: Option<&'a str>,
    pub(crate) max_count: Option<&'a str>,
    pub(crate) pretty: Option<&'a str>,
    pub(crate) stdin: bool,
    pub(crate) args: Vec<String>,
    pub(crate) raw_args: &'a [String],
}

impl ShowOptions<'_> {
    fn diff_format(&self) -> ShowDiffFormat {
        if (self.patch_with_raw || (self.patch && self.raw)) && self.summary {
            ShowDiffFormat::PatchWithRawSummary
        } else if self.patch_with_raw || (self.patch && self.raw) {
            ShowDiffFormat::PatchWithRaw
        } else if (self.patch_with_stat || (self.patch && self.stat)) && self.summary {
            ShowDiffFormat::PatchWithStatSummary
        } else if self.patch_with_stat || (self.patch && self.stat) {
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

fn show_additive_diff_formats(options: &ShowOptions<'_>) -> Vec<ShowDiffFormat> {
    let mut formats = Vec::new();
    if options.raw {
        formats.push(ShowDiffFormat::Raw);
    }
    if options.numstat {
        formats.push(ShowDiffFormat::Numstat);
    }
    if options.shortstat {
        formats.push(ShowDiffFormat::Shortstat);
    }
    formats
}

fn show_supports_additive_diff_formats(options: &ShowOptions<'_>) -> bool {
    let formats = show_additive_diff_formats(options);
    formats.len() > 1
        && !options.patch
        && !options.patch_with_raw
        && !options.patch_with_stat
        && !options.stat
        && !options.summary
        && !options.name_only
        && !options.name_status
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

fn show_merge_diff_mode(options: &ShowOptions<'_>) -> Result<LogMergeDiffMode> {
    if options.no_diff_merges {
        return Ok(LogMergeDiffMode::Off);
    }
    if let Some(value) = options.diff_merges {
        return parse_log_diff_merges_arg(value, None);
    }
    if options.first_parent {
        Ok(LogMergeDiffMode::FirstParent)
    } else if options.separate_merges {
        Ok(LogMergeDiffMode::Separate)
    } else {
        Ok(LogMergeDiffMode::Combined)
    }
}

fn show_effective_merge_diff_mode(options: &ShowOptions<'_>) -> Result<LogMergeDiffMode> {
    if raw_arg_present_before_dashdash(options.raw_args, "--no-diff-merges") {
        return Ok(LogMergeDiffMode::Off);
    }
    if let Some(value) = raw_arg_last_value(options.raw_args, &["--diff-merges"]) {
        return parse_log_diff_merges_arg(value, None);
    }
    show_merge_diff_mode(options)
}

fn show_effective_parents(options: &ShowOptions<'_>) -> bool {
    options.parents || raw_arg_present_before_dashdash(options.raw_args, "--parents")
}

fn show_effective_decoration_mode(options: &ShowOptions<'_>) -> Result<Option<LogDecorationMode>> {
    if raw_arg_last_toggle(options.raw_args, "--decorate", "--no-decorate") == Some(false) {
        return Ok(None);
    }
    if let Some(value) = raw_arg_last_value(options.raw_args, &["--decorate"]) {
        return parse_log_decoration_mode(Some(value));
    }
    if raw_arg_present_before_dashdash(options.raw_args, "--decorate") {
        return parse_log_decoration_mode(Some(""));
    }
    Ok(None)
}

fn show_default_abbrev_len(repo: &GitRepo, store: &LooseObjectStore) -> Result<usize> {
    configured_default_abbrev_len(repo, store)
}

fn show_with_options(options: ShowOptions<'_>) -> Result<()> {
    let _accepted_show_signature = options.show_signature;
    let detect_renames = parse_find_renames_option(options.find_renames)?;
    let detect_copies = parse_find_copies_option(options.find_copies)?;
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
    if selected_formats > 1
        && !valid_combined_format
        && !show_supports_additive_diff_formats(&options)
    {
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
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let (positional_revs, paths) = split_show_revs_and_paths(&repo, &store, options.args.clone());
    let resolved_revs = history_revs_with_stdin(&positional_revs, options.stdin)?;
    if let Some(argument) = resolved_revs
        .iter()
        .find(|argument| argument.starts_with('-') && !show_revision_selector_is_known(argument))
    {
        return Err(log_unrecognized_argument(argument));
    }
    if (options.name_only || options.name_status)
        && show_should_use_log_pipeline(&repo, &resolved_revs, options.stdin, &options)
    {
        return show_via_log(options, resolved_revs, detect_renames, detect_copies);
    }
    if show_should_use_multi_object_renderer(&repo, &resolved_revs, options.stdin, &options) {
        return show_multi_objects(options, resolved_revs, detect_renames, detect_copies);
    }
    if show_raw_format_requested(&options)
        && show_should_use_log_pipeline(&repo, &resolved_revs, options.stdin, &options)
    {
        return show_raw_multi(options, resolved_revs, detect_renames, detect_copies);
    }
    if show_should_use_log_pipeline(&repo, &resolved_revs, options.stdin, &options) {
        return show_via_log(options, resolved_revs, detect_renames, detect_copies);
    }
    let objectish = resolved_revs
        .first()
        .cloned()
        .unwrap_or_else(|| "HEAD".to_owned());
    let id = resolve_objectish(&repo, &objectish).map_err(|_| show_revision_error(&objectish))?;
    let object = store.read_object(&id)?;
    let show_root = options.root || log_showroot_enabled(&repo)?;
    let mut object_options = options;
    object_options.stdin = false;
    object_options.args = vec![objectish.clone()];
    if !paths.is_empty() {
        object_options.args.push("--".to_owned());
        object_options.args.extend(paths);
    }
    show_object(
        &store,
        &objectish,
        &object,
        object_options,
        show_root,
        None,
        detect_renames,
        detect_copies,
    )
}

fn show_revision_selector_is_known(argument: &str) -> bool {
    matches!(
        argument,
        "--all"
            | "--alternate-refs"
            | "--bisect"
            | "--do-walk"
            | "--ignore-missing"
            | "--indexed-objects"
            | "--not"
            | "--objects"
            | "--objects-edge"
            | "--objects-edge-aggressive"
            | "--reflog"
            | "--single-worktree"
            | "--unpacked"
    ) || [
        "--branches=",
        "--exclude=",
        "--exclude-hidden=",
        "--glob=",
        "--remotes=",
        "--tags=",
    ]
    .iter()
    .any(|prefix| argument.starts_with(prefix))
        || matches!(argument, "--branches" | "--remotes" | "--tags")
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

fn show_should_use_log_pipeline(
    repo: &GitRepo,
    revs: &[String],
    stdin: bool,
    options: &ShowOptions<'_>,
) -> bool {
    stdin
        || options.max_count.is_some_and(|value| value != "1")
        || revs.len() > 1
        || revs.iter().any(|rev| resolve_objectish(repo, rev).is_err())
        || options.diff_merges.is_some()
        || options.no_diff_merges
        || options.do_walk
        || options.no_walk.is_some()
}

fn show_should_use_multi_object_renderer(
    repo: &GitRepo,
    revs: &[String],
    stdin: bool,
    options: &ShowOptions<'_>,
) -> bool {
    if options.do_walk || revs.is_empty() {
        return false;
    }
    if !stdin
        && revs.len() <= 1
        && options.no_walk.is_none()
        && options.diff_merges.is_none()
        && !options.no_diff_merges
    {
        return false;
    }
    revs.iter().all(|rev| resolve_objectish(repo, rev).is_ok())
}

fn show_raw_format_requested(options: &ShowOptions<'_>) -> bool {
    options.format == Some("raw") || options.pretty == Some("raw")
}

fn show_diff_format_needs_raw_abbrev_len(format: ShowDiffFormat) -> bool {
    matches!(
        format,
        ShowDiffFormat::Raw | ShowDiffFormat::PatchWithRaw | ShowDiffFormat::PatchWithRawSummary
    )
}

fn show_format_needs_commit_abbrev_len(
    format: &LogFormat<'_>,
    default_commit_abbrev: bool,
    parents: usize,
) -> bool {
    format.uses_abbreviated_object_id()
        || default_commit_abbrev
        || (matches!(
            format,
            LogFormat::Default | LogFormat::Full | LogFormat::Fuller
        ) && parents > 1)
}

fn show_multi_objects(
    options: ShowOptions<'_>,
    mut revs: Vec<String>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
) -> Result<()> {
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let (positional_revs, paths) = split_show_revs_and_paths(&repo, &store, options.args.clone());
    if revs.is_empty() {
        revs = positional_revs;
    }
    if revs.is_empty() {
        revs.push("HEAD".to_owned());
    }
    let show_root = options.root || log_showroot_enabled(&repo)?;
    let prepared = prepare_show_context(&repo, &store, &options)?;
    let separate_objects = show_raw_format_requested(&options)
        || options.patch
        || options.patch_with_raw
        || options.patch_with_stat
        || options.stat
        || options.shortstat
        || options.raw;
    for (idx, objectish) in revs.iter().enumerate() {
        if idx > 0 && separate_objects {
            io::stdout().write_all(b"\n")?;
        }
        let id = resolve_objectish(&repo, objectish).map_err(|_| show_revision_error(objectish))?;
        let object = store.read_object(&id)?;
        let mut object_options = options.clone();
        object_options.stdin = false;
        object_options.args = vec![objectish.clone()];
        if !paths.is_empty() {
            object_options.args.push("--".to_owned());
            object_options.args.extend(paths.iter().cloned());
        }
        show_object(
            &store,
            objectish,
            &object,
            object_options,
            show_root,
            Some(&prepared),
            detect_renames,
            detect_copies,
        )?;
    }
    Ok(())
}

fn show_raw_multi(
    options: ShowOptions<'_>,
    mut revs: Vec<String>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
) -> Result<()> {
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let (positional_revs, paths) = split_show_revs_and_paths(&repo, &store, options.args.clone());
    if revs.is_empty() {
        revs = positional_revs;
    }
    if revs.is_empty() {
        revs.push("HEAD".to_owned());
    }
    let show_root = options.root || log_showroot_enabled(&repo)?;
    let prepared = prepare_show_context(&repo, &store, &options)?;
    for (idx, objectish) in revs.iter().enumerate() {
        if idx > 0 {
            io::stdout().write_all(b"\n")?;
        }
        let id = resolve_objectish(&repo, objectish).map_err(|_| show_revision_error(objectish))?;
        let object = store.read_object(&id)?;
        let mut object_options = options.clone();
        object_options.stdin = false;
        object_options.args = vec![objectish.clone()];
        if !paths.is_empty() {
            object_options.args.push("--".to_owned());
            object_options.args.extend(paths.iter().cloned());
        }
        show_object(
            &store,
            objectish,
            &object,
            object_options,
            show_root,
            Some(&prepared),
            detect_renames,
            detect_copies,
        )?;
    }
    Ok(())
}

fn show_revision_error(objectish: &str) -> CliError {
    if objectish.starts_with('-') {
        log_unrecognized_argument(objectish)
    } else {
        ambiguous_revision_error(objectish)
    }
}

fn show_via_log(
    options: ShowOptions<'_>,
    resolved_revs: Vec<String>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
) -> Result<()> {
    if options.name_only {
        return show_name_only_multi(options, resolved_revs, detect_renames, detect_copies);
    }
    if options.name_status {
        return show_name_status_multi(options, resolved_revs, detect_renames, detect_copies);
    }
    let patch_with_raw = options.patch_with_raw || (options.patch && options.raw);
    let patch_with_stat = options.patch_with_stat || (options.patch && options.stat);
    let patch = options.patch
        && !patch_with_raw
        && !patch_with_stat
        && !options.numstat
        && !options.shortstat
        && !options.summary
        && !options.name_only
        && !options.name_status;
    log(LogOptions {
        oneline: options.oneline,
        zero: options.zero,
        all: false,
        exclude: Vec::new(),
        exclude_first_parent_only: false,
        exclude_hidden: None,
        exclude_promisor_objects: false,
        author: None,
        committer: None,
        alternate_refs: false,
        bisect: false,
        bisect_all: false,
        bisect_vars: false,
        cherry: false,
        count: false,
        glob: None,
        skip: None,
        max_parents: None,
        no_max_parents: false,
        merges: false,
        merge: false,
        max_age: None,
        min_parents: None,
        min_age: None,
        no_min_parents: false,
        no_merges: false,
        parents: options.parents,
        first_parent: options.first_parent,
        follow: false,
        no_diff_merges: options.no_diff_merges,
        diff_merges: options.diff_merges,
        separate_merges: options.separate_merges,
        dd: false,
        reverse: false,
        full_history: false,
        ancestry_path: false,
        in_commit_order: false,
        dense: false,
        sparse: false,
        show_pulls: false,
        show_linear_break: false,
        simplify_merges: false,
        simplify_by_decoration: false,
        topo_order: false,
        date_order: false,
        author_date_order: false,
        left_right: false,
        left_only: false,
        right_only: false,
        cherry_pick: false,
        cherry_mark: false,
        boundary: false,
        children: false,
        root: options.root,
        patch: !(options.no_patch
            || options.stat
            || options.numstat
            || options.shortstat
            || options.raw
            || options.summary
            || options.name_only
            || options.name_status)
            || patch,
        patch_with_stat,
        combined: options.combined,
        dense_combined: false,
        stat: options.stat && !patch_with_stat,
        numstat: options.numstat,
        shortstat: options.shortstat,
        raw: options.raw && !patch_with_raw,
        summary: options.summary,
        name_only: options.name_only,
        name_status: options.name_status,
        encoding: options.encoding,
        expand_tabs: options.expand_tabs,
        no_expand_tabs: options.no_expand_tabs,
        notes: options.notes || options.show_notes || options.show_notes_by_default,
        no_notes: options.no_notes || options.standard_notes || options.no_standard_notes,
        show_notes: false,
        show_notes_by_default: false,
        standard_notes: false,
        no_standard_notes: false,
        diff_required: false,
        decorate: None,
        decorate_refs: None,
        decorate_refs_exclude: None,
        no_decorate: false,
        clear_decorations: false,
        abbrev_commit: options.abbrev_commit,
        no_abbrev_commit: options.no_abbrev_commit,
        graph: false,
        objects: false,
        objects_edge: false,
        objects_edge_aggressive: false,
        no_object_names: false,
        indexed_objects: false,
        unpacked: false,
        remove_empty: false,
        ignore_missing: false,
        filter: None,
        full_diff: false,
        filter_print_omitted: false,
        filter_provided_objects: false,
        pickaxe_string: None,
        pickaxe_regex: None,
        pickaxe_regex_mode: false,
        pickaxe_all: false,
        stdin: false,
        ignore_matching_lines: Vec::new(),
        walk_reflogs: false,
        reflog: false,
        do_walk: options.do_walk,
        no_walk: options.no_walk.is_some(),
        grep_reflog: Vec::new(),
        grep: Vec::new(),
        invert_grep: false,
        all_match: false,
        regexp_ignore_case: false,
        basic_regexp: false,
        extended_regexp: false,
        fixed_strings: false,
        perl_regexp: false,
        format: options.format,
        show_signature: options.show_signature,
        log_size: false,
        line_ranges: Vec::new(),
        mailmap: false,
        no_mailmap: false,
        use_mailmap: false,
        no_use_mailmap: false,
        source: false,
        max_count: options.max_count,
        since: None,
        since_as_filter: None,
        until: None,
        date: None,
        relative_date: false,
        pretty: options.pretty,
        single_worktree: false,
        commit_header: false,
        no_commit_header: false,
        disk_usage: false,
        header: false,
        progress: false,
        no_filter: false,
        missing: false,
        use_bitmap_index: false,
        quiet: false,
        raw_args: options.raw_args,
        revs: late_show_revs(
            if resolved_revs.is_empty() {
                options.args
            } else {
                resolved_revs
            },
            options.max_count,
            detect_renames,
            detect_copies,
            options.find_copies_harder,
        ),
    })
}

fn late_show_revs(
    mut revs: Vec<String>,
    max_count: Option<&str>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Vec<String> {
    if let Some(value) = max_count {
        revs.push(format!("--max-count={value}"));
    }
    if let Some(value) = detect_renames {
        revs.push(format!("--find-renames={value}%"));
    }
    if let Some(value) = detect_copies {
        revs.push(format!("--find-copies={value}%"));
    }
    if find_copies_harder {
        revs.push("--find-copies-harder".to_owned());
    }
    revs
}

fn show_name_only_multi(
    options: ShowOptions<'_>,
    resolved_revs: Vec<String>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
) -> Result<()> {
    show_named_diff_multi(
        options,
        resolved_revs,
        detect_renames,
        detect_copies,
        NamedShowDiffFormat::NameOnly,
    )
}

fn show_name_status_multi(
    options: ShowOptions<'_>,
    resolved_revs: Vec<String>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
) -> Result<()> {
    show_named_diff_multi(
        options,
        resolved_revs,
        detect_renames,
        detect_copies,
        NamedShowDiffFormat::NameStatus,
    )
}

#[derive(Clone, Copy)]
enum NamedShowDiffFormat {
    NameOnly,
    NameStatus,
}

fn write_named_tree_diff<W: Write>(
    out: &mut W,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
    named_format: NamedShowDiffFormat,
) -> Result<()> {
    let mut started = false;
    zmin_git_core::for_each_tree_diff(tree_cache, old_tree, new_tree, |entry| {
        if !started {
            out.write_all(b"\n")?;
            started = true;
        }
        match named_format {
            NamedShowDiffFormat::NameOnly => {
                writeln!(out, "{}", diff_display_path(&entry.path, None))?
            }
            NamedShowDiffFormat::NameStatus => writeln!(
                out,
                "{}\t{}",
                entry.status.name_status(),
                diff_display_path(&entry.path, None)
            )?,
        }
        Ok(())
    })?;
    Ok(())
}

fn write_object_parents_header<W: Write>(
    out: &mut W,
    id: &ObjectId,
    parents: &[ObjectId],
) -> io::Result<()> {
    id.write_hex_io(&mut *out)?;
    out.write_all(b" ")?;
    for (index, parent) in parents.iter().enumerate() {
        if index > 0 {
            out.write_all(b" ")?;
        }
        parent.write_hex_io(&mut *out)?;
    }
    out.write_all(b"\n")
}

fn show_named_diff_multi(
    options: ShowOptions<'_>,
    resolved_revs: Vec<String>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    named_format: NamedShowDiffFormat,
) -> Result<()> {
    let _trace = phase_trace("show.named_multi");
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1)
        .with_transient_packed_object_reads()
        .with_packed_file_reader_buffer_capacity(LOG_PACK_FILE_READER_BUFFER_BYTES);
    let show_root = options.root || log_showroot_enabled(&repo)?;
    let merge_diff_mode = show_effective_merge_diff_mode(&options)?;
    let (positional_revs, paths) = split_show_revs_and_paths(&repo, &store, options.args);
    let revs = if !resolved_revs.is_empty() {
        resolved_revs
    } else if positional_revs.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        positional_revs
    };
    let commit_cache = CommitObjectCache::new(&store);
    // The materialized tree indexes below already retain everything needed
    // across requested commits. Keeping every decoded tree at the same time
    // duplicates paths and object ids, which is especially costly for GUI
    // clients that feed several adjacent commits through `show --stdin`.
    let tree_cache = TreeObjectCache::transient(&store);
    let direct_object_parents_header = options.format == Some("%H %P")
        && options.pretty.is_none()
        && !options.oneline
        && !options.parents
        && !options.notes
        && !options.show_notes
        && !options.show_notes_by_default;
    let format = if direct_object_parents_header {
        None
    } else {
        Some(LogFormat::parse(
            options.oneline,
            options.format,
            options.pretty,
        )?)
    };
    let abbrev_len = if direct_object_parents_header {
        GitHashAlgorithm::Sha1.digest_len() * 2
    } else {
        default_abbrev_len(&store)?
    };
    let pathspecs = paths
        .iter()
        .map(|path| normalize_git_path(path).map_err(CliError::Io))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(String::into_bytes)
        .collect::<Vec<_>>();
    let stream_tree_diff = pathspecs.is_empty()
        && detect_renames.is_none()
        && detect_copies.is_none()
        && !options.find_copies_harder;
    let tree_indexes = ShowTreeIndexCache::new();
    let decorations = LogDecorations::empty();
    let notes = LogNotes::empty();
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut out = io::BufWriter::new(stdout);
    for rev in revs {
        let id = resolve_objectish(&repo, &rev).map_err(|_| ambiguous_revision_error(&rev))?;
        if direct_object_parents_header && stream_tree_diff {
            let commit = commit_cache.read_commit_links(&id)?;
            write_object_parents_header(&mut out, &id, &commit.parents)?;
            if commit.parents.len() > 1 && !matches!(merge_diff_mode, LogMergeDiffMode::FirstParent)
            {
                continue;
            }
            let old_tree = if let Some(parent) = commit.parents.first() {
                Some(commit_cache.read_commit_links(parent)?.tree.clone())
            } else if show_root {
                None
            } else {
                continue;
            };
            write_named_tree_diff(
                &mut out,
                &tree_cache,
                old_tree.as_ref(),
                &commit.tree,
                named_format,
            )?;
            continue;
        }
        let commit = commit_cache.read_commit(&id)?;
        if let Some(format) = format.as_ref() {
            let rendered = format.render_with_context_default_date(
                &id,
                &commit,
                options.parents,
                abbrev_len,
                None,
                false,
                true,
                &decorations,
                &notes,
            )?;
            out.write_all(rendered.as_bytes())?;
            if format.terminates_lines() {
                out.write_all(b"\n")?;
            }
        } else {
            write_object_parents_header(&mut out, &id, &commit.parents)?;
        }
        if commit.parents.len() > 1 && !matches!(merge_diff_mode, LogMergeDiffMode::FirstParent) {
            continue;
        }
        if stream_tree_diff {
            let old_tree = if let Some(parent) = commit.parents.first() {
                Some(commit_cache.read_commit(parent)?.tree.clone())
            } else if show_root {
                None
            } else {
                continue;
            };
            write_named_tree_diff(
                &mut out,
                &tree_cache,
                old_tree.as_ref(),
                &commit.tree,
                named_format,
            )?;
            continue;
        }
        let old_index = if let Some(parent) = commit.parents.first() {
            let parent_commit = commit_cache.read_commit(parent)?;
            read_show_tree_index(&tree_cache, Some(&tree_indexes), &parent_commit.tree)?
        } else if show_root {
            Arc::new(GitIndex::new())
        } else {
            continue;
        };
        let new_index = read_show_tree_index(&tree_cache, Some(&tree_indexes), &commit.tree)?;
        let entries = filtered_diff_entries_pathspecs(
            old_index.as_ref(),
            new_index.as_ref(),
            &pathspecs,
            detect_renames,
            detect_copies,
            options.find_copies_harder,
        )?;
        let context = DiffIndexContext {
            repo: &repo,
            store: &store,
            old_index: old_index.as_ref(),
            new_index: new_index.as_ref(),
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::Index,
        };
        let entries = apply_similarity_detection(
            &context,
            entries,
            SimilarityDetectionOptions {
                rename_threshold: detect_renames,
                copy_threshold: detect_copies,
                find_copies_harder: options.find_copies_harder,
            },
        )?;
        if entries.is_empty() {
            continue;
        }
        out.write_all(b"\n")?;
        match named_format {
            NamedShowDiffFormat::NameOnly => {
                write_name_only_entries_buffered(&mut out, &entries, None, false)?
            }
            NamedShowDiffFormat::NameStatus => {
                write_name_status_entries_buffered(&mut out, &entries, None, false)?
            }
        }
    }
    Ok(())
}

fn split_show_revs_and_paths(
    repo: &GitRepo,
    _store: &LooseObjectStore,
    args: Vec<String>,
) -> (Vec<String>, Vec<String>) {
    fn is_late_show_option(arg: &str) -> bool {
        matches!(arg, "--parents" | "--date" | "--decorate" | "-c" | "-r")
            || arg.starts_with("--date=")
            || arg.starts_with("--decorate=")
            || arg.starts_with("--diff-merges=")
    }

    let mut revs = Vec::new();
    let mut paths = Vec::new();
    let mut in_paths = false;
    for arg in args {
        if arg == "--" {
            in_paths = true;
        } else if in_paths {
            paths.push(arg);
        } else if is_late_show_option(&arg) {
            continue;
        } else if resolve_objectish(repo, &arg).is_ok() {
            revs.push(arg);
        } else if !revs.is_empty() && repo.root.join(&arg).exists() {
            in_paths = true;
            paths.push(arg);
        } else {
            revs.push(arg);
        }
    }
    (revs, paths)
}

struct PreparedShowContext {
    repo: GitRepo,
    decorations: LogDecorations,
    date_arg: Option<String>,
    pathspecs: Vec<Vec<u8>>,
    tree_indexes: ShowTreeIndexCache,
}

struct ShowTreeIndexCache {
    indexes: RefCell<HashMap<ObjectId, Arc<GitIndex>>>,
}

impl ShowTreeIndexCache {
    fn new() -> Self {
        Self {
            indexes: RefCell::new(HashMap::new()),
        }
    }
}

fn prepare_show_context(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &ShowOptions<'_>,
) -> Result<PreparedShowContext> {
    let decoration_mode = show_effective_decoration_mode(options)?;
    let decorations = LogDecorations::load(repo, store, decoration_mode, false, None)?;
    let date_arg = history_raw_date_arg(options.raw_args, None, false);
    let (_, paths) = split_show_revs_and_paths(repo, store, options.args.clone());
    let pathspecs = paths
        .iter()
        .map(|path| normalize_git_path(path).map_err(CliError::Io))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(String::into_bytes)
        .collect::<Vec<_>>();
    Ok(PreparedShowContext {
        repo: repo.clone(),
        decorations,
        date_arg,
        pathspecs,
        tree_indexes: ShowTreeIndexCache::new(),
    })
}

fn read_show_tree_index(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_indexes: Option<&ShowTreeIndexCache>,
    tree_id: &ObjectId,
) -> Result<Arc<GitIndex>> {
    if let Some(tree_indexes) = tree_indexes {
        if let Some(index) = tree_indexes.indexes.borrow().get(tree_id) {
            return Ok(index.clone());
        }
        let index = Arc::new(tree_cache.read_tree_to_index(tree_id)?);
        tree_indexes
            .indexes
            .borrow_mut()
            .insert(tree_id.clone(), index.clone());
        return Ok(index);
    }
    Ok(Arc::new(tree_cache.read_tree_to_index(tree_id)?))
}

fn show_object(
    store: &LooseObjectStore,
    objectish: &str,
    object: &LooseObject,
    options: ShowOptions<'_>,
    show_root: bool,
    prepared: Option<&PreparedShowContext>,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
) -> Result<()> {
    let fallback_repo;
    let repo = if let Some(prepared) = prepared {
        &prepared.repo
    } else {
        fallback_repo = find_repo_or_bare()?;
        &fallback_repo
    };
    let effective_parents = show_effective_parents(&options);
    let fallback_decorations;
    let decorations = if let Some(prepared) = prepared {
        &prepared.decorations
    } else {
        let decoration_mode = show_effective_decoration_mode(&options)?;
        fallback_decorations = LogDecorations::load(repo, store, decoration_mode, false, None)?;
        &fallback_decorations
    };
    let fallback_date_arg;
    let date_arg = if let Some(prepared) = prepared {
        prepared.date_arg.as_deref()
    } else {
        fallback_date_arg = history_raw_date_arg(options.raw_args, None, false);
        fallback_date_arg.as_deref()
    };
    let date_mode = parse_log_date_mode(date_arg)?;
    let fallback_pathspecs;
    let pathspecs = if let Some(prepared) = prepared {
        prepared.pathspecs.as_slice()
    } else {
        let (_, paths) = split_show_revs_and_paths(repo, store, options.args.clone());
        fallback_pathspecs = paths
            .iter()
            .map(|path| normalize_git_path(path).map_err(CliError::Io))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(String::into_bytes)
            .collect::<Vec<_>>();
        fallback_pathspecs.as_slice()
    };
    let tree_indexes = prepared.map(|prepared| &prepared.tree_indexes);
    match object.kind {
        GitObjectKind::Blob => {
            io::stdout().write_all(&object.content)?;
            Ok(())
        }
        GitObjectKind::Tree => show_tree_object(store, objectish, &object.id),
        GitObjectKind::Commit => {
            let commit = {
                let _trace = phase_trace("show.decode_commit");
                decode_commit(GitHashAlgorithm::Sha1, &object.content)?
            };
            let format = {
                let _trace = phase_trace("show.format.parse");
                LogFormat::parse(options.oneline, options.format, options.pretty)?
            };
            let diff_format = options.diff_format();
            let additive_diff_formats = show_additive_diff_formats(&options);
            let merge_diff_mode = show_effective_merge_diff_mode(&options)?;
            let notes = {
                let _trace = phase_trace("show.notes.load");
                LogNotes::load(
                    &repo,
                    store,
                    format.uses_notes_display()
                        && show_notes_enabled(
                            options.notes,
                            options.no_notes,
                            options.show_notes,
                            options.show_notes_by_default,
                            options.standard_notes,
                            options.no_standard_notes,
                        ),
                )?
            };
            let default_commit_abbrev = options.abbrev_commit && !options.no_abbrev_commit;
            let raw_abbrev_len = {
                let _trace = phase_trace("show.abbrev.raw");
                if show_diff_format_needs_raw_abbrev_len(diff_format) {
                    parse_diff_abbrev_len(options.abbrev, options.no_abbrev)?
                        .or(Some(show_default_abbrev_len(&repo, store)?))
                } else {
                    None
                }
            };
            let abbrev_len = {
                let _trace = phase_trace("show.abbrev.commit");
                if options.no_abbrev_commit {
                    GitHashAlgorithm::Sha1.digest_len() * 2
                } else if show_format_needs_commit_abbrev_len(
                    &format,
                    default_commit_abbrev,
                    commit.parents.len(),
                ) {
                    raw_abbrev_len.unwrap_or(show_default_abbrev_len(&repo, store)?)
                } else {
                    GitHashAlgorithm::Sha1.digest_len() * 2
                }
            };
            let has_diff_entries = if pathspecs.is_empty() {
                true
            } else {
                let _trace = phase_trace("show.pathspec.matches");
                show_commit_matches_pathspec(
                    repo,
                    store,
                    &commit,
                    merge_diff_mode,
                    show_root,
                    empty_pickaxe_options(),
                    pathspecs,
                    tree_indexes,
                )?
            };
            if !has_diff_entries {
                return Ok(());
            }
            let visible_parent_ids = {
                let _trace = phase_trace("show.visible_parents");
                visible_show_parent_ids_for_pathspec(
                    repo,
                    store,
                    &commit,
                    show_root,
                    pathspecs,
                    tree_indexes,
                )?
            };
            if options.no_patch {
                if show_raw_format_requested(&options) {
                    return show_raw_commit(
                        &object.id,
                        &object.content,
                        effective_parents,
                        &visible_parent_ids,
                    );
                }
                let rendered = {
                    let _trace = phase_trace("show.render_header");
                    format.render_with_context(
                        &object.id,
                        &commit,
                        effective_parents,
                        abbrev_len,
                        None,
                        default_commit_abbrev,
                        log_expand_tabs_enabled(
                            options.encoding,
                            options.expand_tabs,
                            options.no_expand_tabs,
                        ),
                        &decorations,
                        &notes,
                        date_mode,
                    )?
                };
                io::stdout().write_all(rendered.as_bytes())?;
                if format.terminates_lines() {
                    io::stdout().write_all(b"\n")?;
                }
                return Ok(());
            }
            if show_raw_format_requested(&options) {
                show_raw_commit(
                    &object.id,
                    &object.content,
                    effective_parents,
                    &visible_parent_ids,
                )?;
                if visible_parent_ids.is_empty() && !show_root {
                    return Ok(());
                }
                if matches!(
                    options.diff_format(),
                    ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary
                ) {
                    io::stdout().write_all(b"---\n")?;
                } else {
                    io::stdout().write_all(b"\n")?;
                }
                if commit.parents.len() <= 1 {
                    return show_commit_diff_against_optional_parent(
                        repo,
                        store,
                        &commit,
                        visible_parent_ids.first(),
                        options.diff_format(),
                        empty_pickaxe_options(),
                        &[],
                        pathspecs,
                        tree_indexes,
                        raw_abbrev_len,
                        options.zero,
                        detect_renames,
                        detect_copies,
                        options.find_copies_harder,
                    );
                }
                return show_commit_diff(
                    repo,
                    store,
                    &commit,
                    options.diff_format(),
                    merge_diff_mode,
                    !options.combined,
                    show_root,
                    empty_pickaxe_options(),
                    &[],
                    pathspecs,
                    tree_indexes,
                    raw_abbrev_len,
                    options.zero,
                    detect_renames,
                    detect_copies,
                    options.find_copies_harder,
                );
            }
            if options.separate_merges && commit.parents.len() > 1 {
                let mut out = io::stdout().lock();
                for (idx, parent) in commit.parents.iter().enumerate() {
                    let rendered = format.render_with_from_parent(
                        &object.id,
                        &commit,
                        Some(parent),
                        effective_parents,
                        abbrev_len,
                        None,
                        default_commit_abbrev,
                        log_expand_tabs_enabled(
                            options.encoding,
                            options.expand_tabs,
                            options.no_expand_tabs,
                        ),
                        &decorations,
                        &notes,
                        date_mode,
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
                        repo,
                        store,
                        &commit,
                        diff_format,
                        idx,
                        empty_pickaxe_options(),
                        &[],
                        pathspecs,
                        tree_indexes,
                        raw_abbrev_len,
                        options.zero,
                        detect_renames,
                        detect_copies,
                        options.find_copies_harder,
                    )?;
                    out = io::stdout().lock();
                    if idx + 1 < commit.parents.len() {
                        out.write_all(b"\n")?;
                    }
                }
                return Ok(());
            }
            let rendered = {
                let _trace = phase_trace("show.render_header");
                format.render_with_context(
                    &object.id,
                    &commit,
                    effective_parents,
                    abbrev_len,
                    None,
                    default_commit_abbrev,
                    log_expand_tabs_enabled(
                        options.encoding,
                        options.expand_tabs,
                        options.no_expand_tabs,
                    ),
                    &decorations,
                    &notes,
                    date_mode,
                )?
            };
            let zero_diff_header = options.zero
                && matches!(
                    diff_format,
                    ShowDiffFormat::NameStatus
                        | ShowDiffFormat::NameOnly
                        | ShowDiffFormat::Numstat
                        | ShowDiffFormat::Raw
                );
            io::stdout().write_all(rendered.as_bytes())?;
            if zero_diff_header {
                io::stdout().write_all(b"\0")?;
            }
            if format.terminates_lines() {
                io::stdout().write_all(b"\n")?;
            }
            if commit.parents.is_empty() && !show_root {
                return Ok(());
            }
            if format.separates_patch()
                && matches!(
                    diff_format,
                    ShowDiffFormat::PatchWithStat | ShowDiffFormat::PatchWithStatSummary
                )
            {
                if has_diff_entries {
                    io::stdout().write_all(b"---\n")?;
                }
            } else if format.separates_patch() && has_diff_entries && !zero_diff_header {
                io::stdout().write_all(b"\n")?;
            }
            if commit.parents.len() > 1 && format.terminates_lines() && !format.separates_patch() {
                io::stdout().write_all(b"\n")?;
            }
            if additive_diff_formats.len() > 1 {
                return show_commit_diff_sequence(
                    repo,
                    store,
                    &commit,
                    &additive_diff_formats,
                    &visible_parent_ids,
                    merge_diff_mode,
                    show_root,
                    pathspecs,
                    tree_indexes,
                    raw_abbrev_len,
                    options.zero,
                    detect_renames,
                    detect_copies,
                    options.find_copies_harder,
                );
            }
            show_commit_diff(
                repo,
                store,
                &commit,
                diff_format,
                merge_diff_mode,
                !options.combined,
                show_root,
                empty_pickaxe_options(),
                &[],
                pathspecs,
                tree_indexes,
                raw_abbrev_len,
                options.zero,
                detect_renames,
                detect_copies,
                options.find_copies_harder,
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
    tree_indexes: Option<&ShowTreeIndexCache>,
) -> Result<bool> {
    if commit.parents.len() > 1 {
        return Ok(true);
    }
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let old_index = if let Some(parent) = commit.parents.first() {
        let parent_commit = commit_cache.read_commit(parent)?;
        read_show_tree_index(&tree_cache, tree_indexes, &parent_commit.tree)?
    } else if include_root_diff {
        Arc::new(GitIndex::new())
    } else {
        return Ok(false);
    };
    let new_index = read_show_tree_index(&tree_cache, tree_indexes, &commit.tree)?;
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

fn show_commit_matches_pathspec(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    merge_diff_mode: LogMergeDiffMode,
    include_root_diff: bool,
    pickaxe_options: PickaxeOptions<'_>,
    pathspecs: &[Vec<u8>],
    tree_indexes: Option<&ShowTreeIndexCache>,
) -> Result<bool> {
    if pathspecs.is_empty() {
        return Ok(true);
    }
    let literal_paths = (!pickaxe_options.enabled())
        .then(|| visible_show_literal_paths(pathspecs))
        .flatten();
    if commit.parents.len() <= 1 {
        if let Some(paths) = literal_paths.as_deref() {
            let commit_cache = CommitObjectCache::new(store);
            return commit_touches_literal_paths(
                store,
                &commit_cache,
                commit,
                include_root_diff,
                paths,
            );
        }
        return show_commit_diff_has_entries(
            repo,
            store,
            commit,
            include_root_diff,
            pickaxe_options,
            pathspecs,
            tree_indexes,
        );
    }
    if matches!(merge_diff_mode, LogMergeDiffMode::Off) {
        return Ok(false);
    }
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let mut literal_path_cache = LiteralTreePathIdentityCache::default();
    let new_index = read_show_tree_index(&tree_cache, tree_indexes, &commit.tree)?;

    for parent_id in &commit.parents {
        let parent = commit_cache.read_commit(parent_id)?;
        if let Some(paths) = literal_paths.as_deref() {
            if literal_tree_entry_identities_cached(
                &tree_cache,
                &mut literal_path_cache,
                &commit.tree,
                paths,
            )? != literal_tree_entry_identities_cached(
                &tree_cache,
                &mut literal_path_cache,
                &parent.tree,
                paths,
            )? {
                return Ok(true);
            }
            if matches!(merge_diff_mode, LogMergeDiffMode::FirstParent) {
                break;
            }
            continue;
        }
        let old_index = read_show_tree_index(&tree_cache, tree_indexes, &parent.tree)?;
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
        if !apply_pickaxe_filter(&context, entries, pickaxe_options)?.is_empty() {
            return Ok(true);
        }
        if matches!(merge_diff_mode, LogMergeDiffMode::FirstParent) {
            break;
        }
    }
    Ok(false)
}

fn show_notes_enabled(
    _notes: bool,
    no_notes: bool,
    _show_notes: bool,
    _show_notes_by_default: bool,
    standard_notes: bool,
    no_standard_notes: bool,
) -> bool {
    if no_notes || standard_notes || no_standard_notes {
        return false;
    }
    true
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

fn show_raw_commit(
    id: &ObjectId,
    content: &[u8],
    parents: bool,
    visible_parents: &[ObjectId],
) -> Result<()> {
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
    write!(out, "commit {}", id.to_hex())?;
    if parents {
        for parent in visible_parents {
            write!(out, " {}", parent.to_hex())?;
        }
    }
    writeln!(out)?;
    out.write_all(headers)?;
    out.write_all(b"\n\n")?;
    write_indented_message(&mut out, message)
}

fn write_rev_list_header_record(
    out: &mut dyn io::Write,
    id: &ObjectId,
    content: &[u8],
) -> Result<()> {
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
    writeln!(out, "{}", id.to_hex())?;
    out.write_all(headers)?;
    out.write_all(b"\n\n")?;
    write_indented_message(out, message)?;
    out.write_all(b"\0")?;
    Ok(())
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
    tree_indexes: Option<&ShowTreeIndexCache>,
    raw_abbrev_len: Option<usize>,
    nul_terminated: bool,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<()> {
    let _trace = phase_trace("show.commit_diff");
    if commit.parents.len() > 1 && matches!(merge_diff_mode, LogMergeDiffMode::Off) {
        return Ok(());
    }
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    if matches!(format, ShowDiffFormat::Raw)
        && commit.parents.len() <= 1
        && !pickaxe_options.enabled()
        && detect_renames.is_none()
        && detect_copies.is_none()
        && !find_copies_harder
    {
        let _trace = phase_trace("show.commit_diff.raw");
        let old_tree = if let Some(parent) = commit.parents.first() {
            let _trace = phase_trace("show.commit_diff.raw.read_parent_commit");
            Some(commit_cache.read_commit(parent)?.tree.clone())
        } else if include_root_diff {
            None
        } else {
            return Ok(());
        };
        return print_tree_raw_entries(
            store,
            old_tree.as_ref(),
            &commit.tree,
            pathspecs,
            raw_abbrev_len,
            None,
            nul_terminated,
        );
    }
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
                tree_indexes,
                raw_abbrev_len,
                nul_terminated,
                detect_renames,
                detect_copies,
                find_copies_harder,
            );
        }
        let parent_indexes =
            diff_commands::combined_diff_tree_parent_indexes(commit, &commit_cache, &tree_cache)?;
        let result_index = read_show_tree_index(&tree_cache, tree_indexes, &commit.tree)?;
        match format {
            ShowDiffFormat::Patch => {
                diff_commands::print_combined_diff_tree_patches(
                    store,
                    &parent_indexes,
                    result_index.as_ref(),
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
                    result_index.as_ref(),
                    pathspecs,
                    raw_abbrev_len,
                    None,
                    nul_terminated,
                )?;
                if matches!(format, ShowDiffFormat::PatchWithRawSummary) {
                    diff_commands::print_combined_diff_tree_summary(
                        &parent_indexes,
                        result_index.as_ref(),
                        pathspecs,
                        None,
                    )?;
                }
                println!();
                diff_commands::print_combined_diff_tree_patches(
                    store,
                    &parent_indexes,
                    result_index.as_ref(),
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
                    result_index.as_ref(),
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
                        result_index.as_ref(),
                        pathspecs,
                        None,
                    )?;
                }
                println!();
                diff_commands::print_combined_diff_tree_patches(
                    store,
                    &parent_indexes,
                    result_index.as_ref(),
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
                    result_index.as_ref(),
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
                        result_index.as_ref(),
                        pathspecs,
                        None,
                    )?;
                }
            }
            ShowDiffFormat::Shortstat => {
                return show_commit_diff_against_parent(
                    repo,
                    store,
                    commit,
                    format,
                    0,
                    pickaxe_options,
                    ignore_matching_lines,
                    pathspecs,
                    tree_indexes,
                    raw_abbrev_len,
                    nul_terminated,
                    detect_renames,
                    detect_copies,
                    find_copies_harder,
                );
            }
            ShowDiffFormat::Summary => {
                diff_commands::print_combined_diff_tree_summary(
                    &parent_indexes,
                    result_index.as_ref(),
                    &pathspecs,
                    None,
                )?;
            }
            ShowDiffFormat::Raw | ShowDiffFormat::NameOnly | ShowDiffFormat::NameStatus => {}
            ShowDiffFormat::Numstat => {
                return show_commit_diff_against_parent(
                    repo,
                    store,
                    commit,
                    format,
                    0,
                    pickaxe_options,
                    ignore_matching_lines,
                    pathspecs,
                    tree_indexes,
                    raw_abbrev_len,
                    nul_terminated,
                    detect_renames,
                    detect_copies,
                    find_copies_harder,
                );
            }
        }
        return Ok(());
    }
    let old_index = if let Some(parent) = commit.parents.first() {
        let parent_commit = commit_cache.read_commit(parent)?;
        read_show_tree_index(&tree_cache, tree_indexes, &parent_commit.tree)?
    } else if include_root_diff {
        Arc::new(GitIndex::new())
    } else {
        return Ok(());
    };
    let new_index = read_show_tree_index(&tree_cache, tree_indexes, &commit.tree)?;
    show_diff_between_indexes(
        repo,
        store,
        &old_index,
        &new_index,
        format,
        pickaxe_options,
        ignore_matching_lines,
        pathspecs,
        raw_abbrev_len,
        nul_terminated,
        detect_renames,
        detect_copies,
        find_copies_harder,
    )
}

fn show_commit_diff_against_optional_parent(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    parent_id: Option<&ObjectId>,
    format: ShowDiffFormat,
    pickaxe_options: PickaxeOptions<'_>,
    ignore_matching_lines: &[Regex],
    pathspecs: &[Vec<u8>],
    tree_indexes: Option<&ShowTreeIndexCache>,
    raw_abbrev_len: Option<usize>,
    nul_terminated: bool,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<()> {
    if matches!(format, ShowDiffFormat::Raw)
        && !pickaxe_options.enabled()
        && detect_renames.is_none()
        && detect_copies.is_none()
        && !find_copies_harder
    {
        let old_tree = if let Some(parent_id) = parent_id {
            let commit_cache = CommitObjectCache::new(store);
            Some(commit_cache.read_commit(parent_id)?.tree.clone())
        } else {
            None
        };
        return print_tree_raw_entries(
            store,
            old_tree.as_ref(),
            &commit.tree,
            pathspecs,
            raw_abbrev_len,
            None,
            nul_terminated,
        );
    }
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let old_index = if let Some(parent_id) = parent_id {
        let parent = commit_cache.read_commit(parent_id)?;
        read_show_tree_index(&tree_cache, tree_indexes, &parent.tree)?
    } else {
        Arc::new(GitIndex::new())
    };
    let new_index = read_show_tree_index(&tree_cache, tree_indexes, &commit.tree)?;
    show_diff_between_indexes(
        repo,
        store,
        &old_index,
        &new_index,
        format,
        pickaxe_options,
        ignore_matching_lines,
        pathspecs,
        raw_abbrev_len,
        nul_terminated,
        detect_renames,
        detect_copies,
        find_copies_harder,
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
    tree_indexes: Option<&ShowTreeIndexCache>,
    raw_abbrev_len: Option<usize>,
    nul_terminated: bool,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<()> {
    let _trace = phase_trace("show.commit_diff_against_parent");
    let parent_id = commit
        .parents
        .get(parent_index)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "merge parent index out of range".into(),
        })?;
    show_commit_diff_against_optional_parent(
        repo,
        store,
        commit,
        Some(parent_id),
        format,
        pickaxe_options,
        ignore_matching_lines,
        pathspecs,
        tree_indexes,
        raw_abbrev_len,
        nul_terminated,
        detect_renames,
        detect_copies,
        find_copies_harder,
    )
}

fn show_commit_diff_sequence(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    formats: &[ShowDiffFormat],
    visible_parent_ids: &[ObjectId],
    merge_diff_mode: LogMergeDiffMode,
    show_root: bool,
    pathspecs: &[Vec<u8>],
    tree_indexes: Option<&ShowTreeIndexCache>,
    raw_abbrev_len: Option<usize>,
    nul_terminated: bool,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<()> {
    for format in formats {
        if commit.parents.len() <= 1 {
            show_commit_diff_against_optional_parent(
                repo,
                store,
                commit,
                visible_parent_ids.first(),
                *format,
                empty_pickaxe_options(),
                &[],
                pathspecs,
                tree_indexes,
                raw_abbrev_len,
                nul_terminated,
                detect_renames,
                detect_copies,
                find_copies_harder,
            )?;
        } else {
            show_commit_diff(
                repo,
                store,
                commit,
                *format,
                merge_diff_mode,
                true,
                show_root,
                empty_pickaxe_options(),
                &[],
                pathspecs,
                tree_indexes,
                raw_abbrev_len,
                nul_terminated,
                detect_renames,
                detect_copies,
                find_copies_harder,
            )?;
        }
    }
    Ok(())
}

fn visible_show_parent_ids_for_pathspec(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    include_root_diff: bool,
    pathspecs: &[Vec<u8>],
    tree_indexes: Option<&ShowTreeIndexCache>,
) -> Result<Vec<ObjectId>> {
    if pathspecs.is_empty() || commit.parents.len() != 1 {
        return Ok(commit.parents.clone());
    }
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let literal_paths = visible_show_literal_paths(pathspecs);
    if let Some(paths) = literal_paths.as_deref() {
        let mut literal_path_cache = LiteralTreePathIdentityCache::default();
        let mut current_parent_id = commit.parents[0].clone();
        let mut current_parent = commit_cache.read_commit(&current_parent_id)?;
        let mut current_entries = literal_tree_entry_identities_cached(
            &tree_cache,
            &mut literal_path_cache,
            &current_parent.tree,
            paths,
        )?;
        loop {
            let Some(next_parent_id) = current_parent.parents.first().cloned() else {
                if include_root_diff && current_entries.iter().any(Option::is_some) {
                    return Ok(vec![current_parent_id]);
                }
                return Ok(Vec::new());
            };
            let next_parent = commit_cache.read_commit(&next_parent_id)?;
            let next_entries = literal_tree_entry_identities_cached(
                &tree_cache,
                &mut literal_path_cache,
                &next_parent.tree,
                paths,
            )?;
            if current_entries != next_entries {
                return Ok(vec![current_parent_id]);
            }
            current_parent_id = next_parent_id;
            current_parent = next_parent;
            current_entries = next_entries;
        }
    }
    let mut current_parent = commit.parents.first().cloned();
    while let Some(parent_id) = current_parent {
        let parent_commit = commit_cache.read_commit(&parent_id)?;
        let has_entries = show_commit_diff_has_entries(
            repo,
            store,
            parent_commit.as_ref(),
            include_root_diff,
            empty_pickaxe_options(),
            pathspecs,
            tree_indexes,
        )?;
        if has_entries {
            return Ok(vec![parent_id]);
        }
        current_parent = parent_commit.parents.first().cloned();
    }
    Ok(Vec::new())
}

fn visible_show_literal_paths(pathspecs: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    let mut paths = Vec::with_capacity(pathspecs.len());
    for raw in pathspecs {
        let rule = parse_pathspec_rule(raw);
        if rule.exclude || rule.options.icase || rule.pattern.is_empty() {
            return None;
        }
        if rule
            .pattern
            .iter()
            .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
        {
            return None;
        }
        paths.push(rule.pattern.to_vec());
    }
    Some(paths)
}

fn commit_touches_literal_paths(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commit: &zmin_git_core::CommitObject,
    include_root_diff: bool,
    paths: &[Vec<u8>],
) -> Result<bool> {
    let tree_cache = TreeObjectCache::new(store);
    let mut literal_path_cache = LiteralTreePathIdentityCache::default();
    commit_touches_literal_paths_with_tree_cache(
        &tree_cache,
        &mut literal_path_cache,
        commit_cache,
        commit,
        include_root_diff,
        paths,
    )
}

fn commit_touches_literal_paths_with_tree_cache(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    literal_path_cache: &mut LiteralTreePathIdentityCache,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commit: &zmin_git_core::CommitObject,
    include_root_diff: bool,
    paths: &[Vec<u8>],
) -> Result<bool> {
    let parent_commit = commit
        .parents
        .first()
        .map(|parent_id| commit_cache.read_commit(parent_id))
        .transpose()?;
    if parent_commit.is_none() && !include_root_diff {
        return Ok(false);
    }
    let current =
        literal_tree_entry_identities_cached(tree_cache, literal_path_cache, &commit.tree, paths)?;
    let parent = parent_commit
        .as_ref()
        .map(|parent| {
            literal_tree_entry_identities_cached(
                tree_cache,
                literal_path_cache,
                &parent.tree,
                paths,
            )
        })
        .transpose()?;
    Ok(current != parent.unwrap_or_else(|| vec![None; paths.len()]))
}

fn literal_tree_entry_identities_cached(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    literal_path_cache: &mut LiteralTreePathIdentityCache,
    tree_id: &ObjectId,
    paths: &[Vec<u8>],
) -> Result<Vec<Option<(TreeMode, ObjectId)>>> {
    paths
        .iter()
        .map(|path| {
            literal_tree_entry_identity_cached(tree_cache, literal_path_cache, tree_id, path)
        })
        .collect()
}

#[derive(Default)]
struct LiteralTreePathIdentityCache {
    entries: HashMap<(ObjectId, Vec<u8>), Option<(TreeMode, ObjectId)>>,
}

fn literal_tree_entry_identity_cached(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    literal_path_cache: &mut LiteralTreePathIdentityCache,
    tree_id: &ObjectId,
    path: &[u8],
) -> Result<Option<(TreeMode, ObjectId)>> {
    if path.is_empty() {
        return Ok(None);
    }
    let key = (tree_id.clone(), path.to_vec());
    if let Some(cached) = literal_path_cache.entries.get(&key) {
        return Ok(cached.clone());
    }

    let result =
        literal_tree_entry_identity_uncached(tree_cache, literal_path_cache, tree_id, path)?;
    literal_path_cache.entries.insert(key, result.clone());
    Ok(result)
}

fn literal_tree_entry_identity_uncached(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    literal_path_cache: &mut LiteralTreePathIdentityCache,
    tree_id: &ObjectId,
    path: &[u8],
) -> Result<Option<(TreeMode, ObjectId)>> {
    let mut components = path.splitn(2, |byte| *byte == b'/');
    let component = components.next().unwrap_or_default();
    if component.is_empty() {
        return Ok(None);
    }
    let remainder = components.next();
    let entry = tree_cache
        .read_tree(tree_id)?
        .iter()
        .find(|entry| entry.name == component)
        .cloned();
    let Some(entry) = entry else {
        return Ok(None);
    };
    let Some(remainder) = remainder else {
        return Ok(Some((entry.mode, entry.id)));
    };
    if entry.mode != TreeMode::Tree {
        return Ok(None);
    }
    literal_tree_entry_identity_cached(tree_cache, literal_path_cache, &entry.id, remainder)
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
    raw_abbrev_len: Option<usize>,
    nul_terminated: bool,
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    find_copies_harder: bool,
) -> Result<()> {
    let entries = diff_entries_for_indexes(
        old_index,
        new_index,
        detect_renames,
        detect_copies,
        find_copies_harder,
    )?
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
    let entries = apply_similarity_detection(
        &context,
        entries,
        SimilarityDetectionOptions {
            rename_threshold: detect_renames,
            copy_threshold: detect_copies,
            find_copies_harder,
        },
    )?;
    let entries = apply_pickaxe_filter(&context, entries, pickaxe_options)?;
    let stat_options = DiffStatOptions {
        whitespace_mode: DiffWhitespaceMode::None,
        relative_prefix: None,
        ignore_matching_lines,
        ignore_blank_lines: false,
        compact_summary: false,
        color: false,
    };
    match format {
        ShowDiffFormat::Patch => print_patch_entries(
            repo,
            store,
            &old_index,
            &new_index,
            &entries,
            PatchFormatOptions::cached()
                .with_abbrev_len(raw_abbrev_len)
                .with_ignore_matching_lines(ignore_matching_lines.to_vec()),
        ),
        ShowDiffFormat::PatchWithRaw | ShowDiffFormat::PatchWithRawSummary => {
            print_raw_entries(
                &context,
                &entries,
                RawPrintOptions {
                    abbrev_len: raw_abbrev_len,
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
                    .with_abbrev_len(raw_abbrev_len)
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
                    .with_abbrev_len(raw_abbrev_len)
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
                abbrev_len: raw_abbrev_len,
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
    show_object(
        store,
        &tag.target.to_hex(),
        &target,
        options,
        show_root,
        None,
        None,
        None,
    )
}

fn write_indented_message(out: &mut (impl Write + ?Sized), message: &[u8]) -> Result<()> {
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

pub(crate) struct RevListOptions<'a> {
    pub(crate) oneline: bool,
    pub(crate) header: bool,
    pub(crate) graph: bool,
    pub(crate) all: bool,
    pub(crate) exclude: Vec<String>,
    pub(crate) exclude_first_parent_only: bool,
    pub(crate) exclude_hidden: Option<&'a str>,
    pub(crate) exclude_promisor_objects: bool,
    pub(crate) author: Option<&'a str>,
    pub(crate) committer: Option<&'a str>,
    pub(crate) alternate_refs: bool,
    pub(crate) encoding: Option<&'a str>,
    pub(crate) expand_tabs: bool,
    pub(crate) no_expand_tabs: bool,
    pub(crate) notes: bool,
    pub(crate) no_notes: bool,
    pub(crate) show_notes: bool,
    pub(crate) show_notes_by_default: bool,
    pub(crate) standard_notes: bool,
    pub(crate) no_standard_notes: bool,
    pub(crate) abbrev_commit: bool,
    pub(crate) no_abbrev_commit: bool,
    pub(crate) grep: Vec<String>,
    pub(crate) invert_grep: bool,
    pub(crate) all_match: bool,
    pub(crate) regexp_ignore_case: bool,
    pub(crate) basic_regexp: bool,
    pub(crate) extended_regexp: bool,
    pub(crate) fixed_strings: bool,
    pub(crate) perl_regexp: bool,
    pub(crate) bisect: bool,
    pub(crate) bisect_all: bool,
    pub(crate) bisect_vars: bool,
    pub(crate) cherry: bool,
    pub(crate) count: bool,
    pub(crate) glob: Option<&'a str>,
    pub(crate) skip: Option<usize>,
    pub(crate) max_parents: Option<&'a str>,
    pub(crate) max_age: Option<&'a str>,
    pub(crate) no_max_parents: bool,
    pub(crate) merges: bool,
    pub(crate) merge: bool,
    pub(crate) min_parents: Option<&'a str>,
    pub(crate) min_age: Option<&'a str>,
    pub(crate) no_min_parents: bool,
    pub(crate) no_merges: bool,
    pub(crate) objects: bool,
    pub(crate) objects_edge: bool,
    pub(crate) objects_edge_aggressive: bool,
    pub(crate) indexed_objects: bool,
    pub(crate) unpacked: bool,
    pub(crate) remove_empty: bool,
    pub(crate) ignore_missing: bool,
    pub(crate) object_names: bool,
    pub(crate) no_object_names: bool,
    pub(crate) filter: Option<String>,
    pub(crate) filter_print_omitted: bool,
    pub(crate) filter_provided_objects: bool,
    pub(crate) parents: bool,
    pub(crate) first_parent: bool,
    pub(crate) children: bool,
    pub(crate) walk_reflogs: bool,
    pub(crate) reflog: bool,
    pub(crate) do_walk: bool,
    pub(crate) no_walk: bool,
    pub(crate) stdin: bool,
    pub(crate) grep_reflog: Vec<String>,
    pub(crate) reverse: bool,
    pub(crate) full_history: bool,
    pub(crate) in_commit_order: bool,
    pub(crate) ancestry_path: bool,
    pub(crate) dense: bool,
    pub(crate) sparse: bool,
    pub(crate) show_pulls: bool,
    pub(crate) show_linear_break: bool,
    pub(crate) simplify_merges: bool,
    pub(crate) simplify_by_decoration: bool,
    pub(crate) topo_order: bool,
    pub(crate) date_order: bool,
    pub(crate) author_date_order: bool,
    pub(crate) left_right: bool,
    pub(crate) left_only: bool,
    pub(crate) right_only: bool,
    pub(crate) cherry_pick: bool,
    pub(crate) cherry_mark: bool,
    pub(crate) boundary: bool,
    pub(crate) max_count: Option<usize>,
    pub(crate) since: Option<&'a str>,
    pub(crate) since_as_filter: Option<&'a str>,
    pub(crate) until: Option<&'a str>,
    pub(crate) relative_date: bool,
    pub(crate) timestamp: bool,
    pub(crate) date: Option<&'a str>,
    pub(crate) show_signature: bool,
    pub(crate) single_worktree: bool,
    pub(crate) commit_header: bool,
    pub(crate) no_commit_header: bool,
    pub(crate) disk_usage: bool,
    pub(crate) progress: bool,
    pub(crate) no_filter: bool,
    pub(crate) missing: Option<&'a str>,
    pub(crate) use_bitmap_index: bool,
    pub(crate) quiet: bool,
    pub(crate) format: Option<&'a str>,
    pub(crate) pretty: Option<&'a str>,
    pub(crate) raw_args: &'a [String],
    pub(crate) revs: Vec<String>,
}

fn rev_list_history_order(options: &RevListOptions<'_>) -> Option<HistoryCommitOrder> {
    if options.author_date_order {
        Some(HistoryCommitOrder::AuthorDate)
    } else if options.date_order {
        Some(HistoryCommitOrder::Date)
    } else if options.topo_order {
        Some(HistoryCommitOrder::Topo)
    } else {
        None
    }
}

fn rev_list_walk_reflogs_exclusion_target(revs: &[String]) -> Option<String> {
    let mut not_mode = false;
    for rev in revs {
        if rev == "--not" {
            not_mode = !not_mode;
            continue;
        }
        if let Some(stripped) = rev.strip_prefix('^') {
            return Some(stripped.to_owned());
        }
        if let Some((left, _)) = rev.split_once("...") {
            return Some(if left.is_empty() { "HEAD" } else { left }.to_owned());
        }
        if let Some((left, _)) = rev.split_once("..") {
            return Some(if left.is_empty() { "HEAD" } else { left }.to_owned());
        }
        if not_mode {
            return Some(rev.clone());
        }
    }
    None
}

fn rev_list_reflog_embedded_format<'a>(revs: &'a [String]) -> Option<&'a str> {
    revs.iter().find_map(|rev| {
        rev.strip_prefix("--format=")
            .or_else(|| rev.strip_prefix("--pretty="))
    })
}

fn rev_list_reflog_embedded_date<'a>(revs: &'a [String]) -> Option<&'a str> {
    revs.iter().find_map(|rev| rev.strip_prefix("--date="))
}

fn rev_list_reflog_targets(revs: &[String]) -> Vec<String> {
    revs.iter()
        .filter(|rev| {
            rev.as_str() != "--"
                && !rev.starts_with("--format=")
                && !rev.starts_with("--pretty=")
                && !rev.starts_with("--date=")
                && rev.as_str() != "--relative-date"
        })
        .cloned()
        .collect()
}

fn render_rev_list_format(
    format: &LogFormat<'_>,
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    parents: bool,
    abbrev_len: usize,
    marker: Option<HistoryTraversalMarker>,
    default_commit_abbrev: bool,
    expand_tabs: bool,
    decorations: &LogDecorations,
    notes: &LogNotes,
    date_mode: LogDateMode<'_>,
) -> Result<String> {
    match format {
        LogFormat::Custom { pattern, .. } if log_format_uses_placeholder(pattern, 'N') => {
            let literal_pattern = pattern.replace("%N", "%%N");
            render_log_format(
                &literal_pattern,
                id,
                commit,
                abbrev_len,
                decorations,
                notes,
                date_mode,
            )
        }
        _ => format.render_with_context(
            id,
            commit,
            parents,
            abbrev_len,
            marker,
            default_commit_abbrev,
            expand_tabs,
            decorations,
            notes,
            date_mode,
        ),
    }
}

fn render_log_with_note_mode(
    format: &LogFormat<'_>,
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    from_parent: Option<&ObjectId>,
    parents: bool,
    abbrev_len: usize,
    marker: Option<HistoryTraversalMarker>,
    default_commit_abbrev: bool,
    expand_tabs: bool,
    decorations: &LogDecorations,
    notes: &LogNotes,
    date_mode: LogDateMode<'_>,
    literal_standard_notes: bool,
) -> Result<String> {
    if literal_standard_notes
        && let LogFormat::Custom { pattern, .. } = format
        && log_format_uses_placeholder(pattern, 'N')
    {
        let literal_pattern = pattern.replace("%N", "%%N");
        return render_log_format(
            &literal_pattern,
            id,
            commit,
            abbrev_len,
            decorations,
            notes,
            date_mode,
        );
    }
    if from_parent.is_some() {
        return format.render_with_from_parent(
            id,
            commit,
            from_parent,
            parents,
            abbrev_len,
            marker,
            default_commit_abbrev,
            expand_tabs,
            decorations,
            notes,
            date_mode,
        );
    }
    format.render_with_context(
        id,
        commit,
        parents,
        abbrev_len,
        marker,
        default_commit_abbrev,
        expand_tabs,
        decorations,
        notes,
        date_mode,
    )
}

#[derive(Debug, Clone, Copy)]
enum RevListObjectFilter {
    BlobNone,
    BlobLimit(usize),
    ObjectType(GitObjectKind),
}

pub(crate) fn rev_list(options: RevListOptions<'_>) -> Result<()> {
    let _trace = phase_trace("rev_list.total");
    let history_order = rev_list_history_order(&options);
    let RevListOptions {
        oneline,
        header,
        graph,
        all,
        exclude,
        exclude_first_parent_only,
        exclude_hidden,
        exclude_promisor_objects,
        author,
        committer,
        alternate_refs,
        encoding,
        expand_tabs,
        no_expand_tabs,
        notes,
        no_notes,
        show_notes,
        show_notes_by_default,
        standard_notes,
        no_standard_notes,
        abbrev_commit,
        no_abbrev_commit,
        grep,
        invert_grep,
        all_match,
        regexp_ignore_case,
        basic_regexp,
        extended_regexp,
        fixed_strings,
        perl_regexp,
        bisect,
        bisect_all,
        bisect_vars,
        cherry,
        count,
        glob,
        skip,
        max_parents,
        max_age,
        no_max_parents,
        merges,
        merge,
        min_parents,
        min_age,
        no_min_parents,
        no_merges,
        objects,
        objects_edge,
        objects_edge_aggressive,
        indexed_objects,
        unpacked,
        remove_empty,
        ignore_missing,
        object_names,
        no_object_names,
        filter,
        filter_print_omitted,
        filter_provided_objects,
        parents,
        first_parent,
        children,
        walk_reflogs,
        reflog,
        do_walk,
        no_walk,
        stdin,
        grep_reflog,
        reverse,
        full_history,
        in_commit_order,
        ancestry_path,
        dense,
        sparse,
        show_pulls,
        show_linear_break,
        simplify_merges,
        simplify_by_decoration,
        topo_order: _,
        date_order: _,
        author_date_order: _,
        left_right,
        left_only,
        right_only,
        cherry_pick,
        cherry_mark,
        boundary,
        max_count,
        since,
        since_as_filter,
        until,
        relative_date,
        timestamp,
        date,
        show_signature,
        single_worktree,
        commit_header,
        no_commit_header,
        disk_usage,
        progress,
        no_filter,
        missing,
        use_bitmap_index,
        quiet,
        format,
        pretty,
        raw_args,
        revs,
    } = options;
    let objects = objects || objects_edge || objects_edge_aggressive;
    let _accepted_exclude = exclude;
    let _accepted_exclude_first_parent_only = exclude_first_parent_only;
    let _accepted_exclude_hidden = exclude_hidden;
    let _accepted_alternate_refs = alternate_refs;
    let _accepted_full_history = full_history;
    let _accepted_dense = dense;
    let _accepted_sparse = sparse;
    let _accepted_show_pulls = show_pulls;
    let _accepted_simplify_merges = simplify_merges;
    let _accepted_cherry = cherry;
    let _accepted_glob = glob;
    let _accepted_in_commit_order = in_commit_order;
    let _accepted_show_linear_break = show_linear_break;
    let _accepted_no_notes = no_notes;
    let _accepted_standard_notes = standard_notes;
    let _accepted_no_standard_notes = no_standard_notes;
    let _accepted_indexed_objects = indexed_objects;
    let _accepted_unpacked = unpacked;
    let _accepted_remove_empty = remove_empty;
    let _accepted_ignore_missing = ignore_missing;
    let _accepted_object_names = object_names;
    let _accepted_do_walk = do_walk;
    let _accepted_stdin = stdin;
    let _accepted_show_signature = show_signature;
    let _accepted_single_worktree = single_worktree;
    let _accepted_no_filter = no_filter;
    let _accepted_commit_header = commit_header;
    let _accepted_no_commit_header = no_commit_header;
    let _accepted_exclude_promisor_objects = exclude_promisor_objects;
    let _accepted_filter_print_omitted = filter_print_omitted;
    let _accepted_use_bitmap_index = use_bitmap_index;
    let missing_mode = parse_rev_list_missing_mode(missing)?;
    if progress {
        return Err(rev_list_usage_error());
    }
    if merge {
        return Err(CliError::Fatal {
            code: 128,
            message:
                "--merge requires one of the pseudorefs MERGE_HEAD, CHERRY_PICK_HEAD, REVERT_HEAD or REBASE_HEAD"
                    .into(),
        });
    }
    let walk_reflogs = walk_reflogs || reflog;
    let no_walk = resolve_history_walk_mode(raw_args, no_walk, do_walk);
    if !grep_reflog.is_empty() && !walk_reflogs {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--grep-reflog' requires '--walk-reflogs'".into(),
        });
    }
    if walk_reflogs && reverse {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--reverse' and '--walk-reflogs' cannot be used together".into(),
        });
    }
    let simplify_history_topo = simplify_merges || simplify_by_decoration;
    let effective_since = since.or(since_as_filter);
    let (since, until) =
        resolve_history_age_bounds(raw_args, effective_since, max_age, until, min_age);
    let Some(since) = parse_log_since(since) else {
        return Ok(());
    };
    let Some(until) = parse_log_until(until) else {
        return Ok(());
    };
    let skip = resolve_history_skip(raw_args, skip)?;
    if rev_list_supports_notes_display(
        notes,
        show_notes,
        show_notes_by_default,
        standard_notes,
        no_standard_notes,
    ) {
        return Err(CliError::Fatal {
            code: 128,
            message: "rev-list does not support display of notes".into(),
        });
    }
    let grep_mode =
        parse_shortlog_pattern_mode(basic_regexp, extended_regexp, fixed_strings, perl_regexp);
    let date = history_raw_date_arg(raw_args, date, relative_date);
    let embedded_date = if walk_reflogs {
        rev_list_reflog_embedded_date(&revs).map(str::to_owned)
    } else {
        None
    };
    let embedded_format = if walk_reflogs {
        rev_list_reflog_embedded_format(&revs).map(str::to_owned)
    } else {
        None
    };
    let date = date.or(embedded_date).unwrap_or_default();
    let date = (!date.is_empty()).then_some(date);
    let format = format
        .map(str::to_owned)
        .or(embedded_format)
        .unwrap_or_default();
    let format = (!format.is_empty()).then_some(format);
    let rendered_format = if oneline
        || format.is_some()
        || pretty.is_some()
        || date.is_some()
        || encoding.is_some()
        || expand_tabs
        || no_expand_tabs
    {
        Some(LogFormat::parse(oneline, format.as_deref(), pretty)?)
    } else {
        None
    };
    let date_mode = parse_log_date_mode(date.as_deref())?;
    let expand_tabs = false;
    let default_commit_abbrev = abbrev_commit && !no_abbrev_commit;
    let (min_parents, max_parents) = parse_log_parent_bounds(
        min_parents,
        no_min_parents,
        no_merges,
        max_parents,
        no_max_parents,
        merges,
    )?;
    let revs = revs
        .into_iter()
        .take_while(|rev| rev != "--")
        .collect::<Vec<_>>();
    let post_collection_filters = since.is_some()
        || until.is_some()
        || author.is_some()
        || committer.is_some()
        || !grep.is_empty()
        || min_parents.is_some()
        || max_parents.is_some()
        || ancestry_path
        || simplify_by_decoration
        || simplify_history_topo
        || history_order.is_some();
    let object_filter = filter.as_deref().map(parse_rev_list_filter).transpose()?;
    let _ = filter_provided_objects;
    if revs.is_empty() && !all {
        return Err(CliError::Message("`rev-list` requires a revision".into()));
    }
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1)
        .with_stable_pack_snapshot()
        .with_packed_objects_first()
        .with_transient_packed_object_reads()
        .with_trusted_packed_object_reads()
        .with_buffered_pack_reads()
        .with_packed_object_read_cache_byte_limit(REV_LIST_PACK_OBJECT_CACHE_BYTES);
    let abbrev_len = if no_abbrev_commit {
        GitHashAlgorithm::Sha1.digest_len() * 2
    } else if default_commit_abbrev
        || rendered_format
            .as_ref()
            .is_some_and(LogFormat::uses_abbreviated_object_id)
    {
        configured_default_abbrev_len(&repo, &store)?
    } else {
        GitHashAlgorithm::Sha1.digest_len() * 2
    };
    let commit_cache = CommitObjectCache::new(&store);
    if walk_reflogs {
        if quiet {
            return Ok(());
        }
        let reflog_revs = rev_list_reflog_targets(&revs);
        if let Some(target) = rev_list_walk_reflogs_exclusion_target(&reflog_revs) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("cannot walk reflogs for {target}"),
            });
        }
        return rev_list_walk_reflogs(
            &repo,
            &store,
            &commit_cache,
            RevListReflogRenderOptions {
                rendered_format: rendered_format.as_ref(),
                date_mode,
                default_commit_abbrev,
                abbrev_len,
                parents,
                regexp_ignore_case,
                count,
                max_count,
            },
            &reflog_revs,
            &grep_reflog,
        );
    }
    let mut revs = collect_rev_list_revs(&repo, &store, all, revs)?;
    let promisor_objects = if exclude_promisor_objects {
        collect_promisor_object_ids(&repo, &store)?
    } else {
        HashSet::new()
    };
    let promised_missing = if matches!(missing_mode, Some(RevListMissingMode::AllowPromisor))
        || matches!(missing_mode, Some(RevListMissingMode::Print))
    {
        collect_promised_missing_object_ids(&repo, &store)?
    } else {
        HashSet::new()
    };
    let mut policy_promised_objects = HashSet::with_capacity(
        promisor_objects
            .len()
            .saturating_add(promised_missing.len()),
    );
    policy_promised_objects.extend(promisor_objects.iter().cloned());
    policy_promised_objects.extend(promised_missing.iter().cloned());
    sanitize_rev_list_promised_inputs(
        &repo,
        &store,
        &mut revs,
        &policy_promised_objects,
        exclude_promisor_objects,
        ignore_missing,
    )?;
    let tree_missing_policy = RevListTreeMissingPolicy {
        allow_any: matches!(missing_mode, Some(RevListMissingMode::AllowAny)),
        exclude_promisor_objects,
        emit_missing_objects: matches!(missing_mode, Some(RevListMissingMode::Print)),
        promised_objects: (!policy_promised_objects.is_empty()).then_some(&policy_promised_objects),
    };
    let mut promised_commit_excludes = Vec::new();
    let mut promised_excluded_commits = Vec::new();
    for id in &policy_promised_objects {
        match store.object_header_hint(id)? {
            Some((GitObjectKind::Commit, _)) => {
                promised_commit_excludes.push(id.to_hex());
                promised_excluded_commits.push(id.clone());
            }
            Some(_) => {}
            None => {
                promised_excluded_commits.push(id.clone());
            }
        }
    }
    let revs_with_promisor_commit_excludes = (!promised_commit_excludes.is_empty()).then(|| {
        let mut exclude = revs.exclude.clone();
        exclude.extend(promised_commit_excludes);
        RevListRevs {
            include: revs.include.clone(),
            exclude,
            extra_objects: revs.extra_objects.clone(),
            symmetric_diff: revs.symmetric_diff.clone(),
        }
    });
    if quiet
        && !objects
        && !count
        && !parents
        && !children
        && !left_right
        && !cherry_pick
        && !cherry_mark
        && !boundary
    {
        return Ok(());
    }
    if objects && no_object_names && object_filter.is_some() {
        let filter = object_filter.expect("checked filter");
        let mut excluded_commits =
            collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?;
        excluded_commits.extend(promised_excluded_commits.iter().cloned());
        let mut commit_trees = if policy_promised_objects.is_empty() {
            let traversal_revs = revs_with_promisor_commit_excludes.as_ref().unwrap_or(&revs);
            collect_commit_trees_with_exclusions_uncached(
                &repo,
                &store,
                traversal_revs,
                expand_history_max_count(max_count, skip),
            )?
        } else {
            let mut roots = Vec::with_capacity(revs.include.len());
            let mut seen = HashSet::with_capacity(revs.include.len());
            for rev in &revs.include {
                let id = resolve_commitish(&repo, &store, rev)?;
                if seen.insert(id.clone()) {
                    roots.push(id);
                }
            }
            let excluded = excluded_commits.iter().cloned().collect::<HashSet<_>>();
            collect_commit_trees_from_ids_uncached_with_excluded(
                &repo,
                &store,
                &roots,
                expand_history_max_count(max_count, skip),
                &excluded,
            )?
        };
        if let Some(skip) = skip {
            commit_trees = commit_trees.into_iter().skip(skip).collect();
        }
        if reverse {
            commit_trees.reverse();
        }
        let extra_object_ids = revs
            .extra_objects
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut out =
            io::BufWriter::with_capacity(REV_LIST_OUTPUT_BUFFER_BYTES, io::stdout().lock());
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
    if objects && count {
        let mut excluded_commits =
            collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?;
        excluded_commits.extend(promised_excluded_commits.iter().cloned());
        let mut commit_trees = if policy_promised_objects.is_empty() {
            let traversal_revs = revs_with_promisor_commit_excludes.as_ref().unwrap_or(&revs);
            collect_commit_trees_with_exclusions_uncached(
                &repo,
                &store,
                traversal_revs,
                expand_history_max_count(max_count, skip),
            )?
        } else {
            let mut roots = Vec::with_capacity(revs.include.len());
            let mut seen = HashSet::with_capacity(revs.include.len());
            for rev in &revs.include {
                let id = resolve_commitish(&repo, &store, rev)?;
                if seen.insert(id.clone()) {
                    roots.push(id);
                }
            }
            let excluded = excluded_commits.iter().cloned().collect::<HashSet<_>>();
            collect_commit_trees_from_ids_uncached_with_excluded(
                &repo,
                &store,
                &roots,
                expand_history_max_count(max_count, skip),
                &excluded,
            )?
        };
        if let Some(skip) = skip {
            commit_trees = commit_trees.into_iter().skip(skip).collect();
        }
        if reverse {
            commit_trees.reverse();
        }
        if count {
            let object_count = count_rev_list_objects_uncached(
                &store,
                &commit_trees,
                &revs.extra_objects,
                &excluded_commits,
                tree_missing_policy,
            )?;
            println!("{}", commit_trees.len() + object_count);
            return Ok(());
        }
        let extra_object_ids = revs
            .extra_objects
            .iter()
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let mut out =
            io::BufWriter::with_capacity(REV_LIST_OUTPUT_BUFFER_BYTES, io::stdout().lock());
        for commit in &commit_trees {
            writeln!(out, "{}", commit.id)?;
        }
        write_rev_list_object_ids_uncached(
            &store,
            &commit_trees,
            &extra_object_ids,
            &excluded_commits,
            tree_missing_policy,
            &mut out,
        )?;
        return Ok(());
    }
    if count && !objects && !post_collection_filters {
        let count_value = if policy_promised_objects.is_empty() {
            count_commits_with_exclusions(
                &repo,
                &store,
                &revs,
                expand_history_max_count(max_count, skip),
            )?
        } else {
            let mut roots = Vec::with_capacity(revs.include.len());
            let mut seen = HashSet::with_capacity(revs.include.len());
            for rev in &revs.include {
                let id = resolve_commitish(&repo, &store, rev)?;
                if seen.insert(id.clone()) {
                    roots.push(id);
                }
            }
            let mut excluded = collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?
                .into_iter()
                .collect::<HashSet<_>>();
            excluded.extend(policy_promised_objects.iter().cloned());
            collect_commits_from_ids_cached_with_excluded(
                &repo,
                &commit_cache,
                &roots,
                expand_history_max_count(max_count, skip),
                &excluded,
            )?
            .len()
        };
        println!("{}", count_value.saturating_sub(skip.unwrap_or(0)));
        return Ok(());
    }

    if objects && !parents && !children {
        let _branch_trace = phase_trace("rev_list.objects");
        let mut traversal_excluded_commits = {
            let _trace = phase_trace("rev_list.objects.collect_excluded");
            collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?
        };
        traversal_excluded_commits.extend(promised_excluded_commits.iter().cloned());
        let excluded = traversal_excluded_commits
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let object_walk_excluded_commits = if exclude_promisor_objects {
            collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?
        } else {
            traversal_excluded_commits.clone()
        };
        let traversal_revs = if policy_promised_objects.is_empty() {
            revs_with_promisor_commit_excludes.as_ref().unwrap_or(&revs)
        } else {
            &revs
        };
        let mut commit_trees = {
            let _trace = phase_trace("rev_list.objects.collect_commits");
            if policy_promised_objects.is_empty() {
                collect_commit_trees_with_exclusions_uncached(
                    &repo,
                    &store,
                    traversal_revs,
                    expand_history_max_count(max_count, skip),
                )?
            } else {
                let mut roots = Vec::with_capacity(revs.include.len());
                let mut seen = HashSet::with_capacity(revs.include.len());
                for rev in &revs.include {
                    let id = resolve_commitish(&repo, &store, rev)?;
                    if seen.insert(id.clone()) {
                        roots.push(id);
                    }
                }
                collect_commits_from_ids_cached_with_excluded(
                    &repo,
                    &commit_cache,
                    &roots,
                    expand_history_max_count(max_count, skip),
                    &excluded,
                )?
                .into_iter()
                .map(|id| {
                    Ok(CollectedCommitTree {
                        tree: read_commit_tree_uncached(&store, &id)?,
                        id,
                    })
                })
                .collect::<Result<Vec<_>>>()?
            }
        };
        if let Some(skip) = skip {
            commit_trees = commit_trees.into_iter().skip(skip).collect();
        }
        if reverse {
            commit_trees.reverse();
        }
        let mut out =
            io::BufWriter::with_capacity(REV_LIST_OUTPUT_BUFFER_BYTES, io::stdout().lock());
        let mut deferred_missing_ids = Vec::new();
        {
            let _trace = phase_trace("rev_list.objects.write_commits");
            for commit in &commit_trees {
                writeln!(out, "{}", commit.id)?;
            }
        }
        {
            let _trace = phase_trace("rev_list.objects.walk_trees");
            for_each_rev_list_object_line_with_trees(
                &store,
                &commit_trees,
                &revs.extra_objects,
                &object_walk_excluded_commits,
                tree_missing_policy,
                |id, kind, name| {
                    if matches!(missing_mode, Some(RevListMissingMode::Print))
                        && !store.contains_object(id).map_err(CliError::Io)?
                    {
                        deferred_missing_ids.push(id.clone());
                        let _ = name;
                        return Ok(());
                    }
                    if let Some(filter) = object_filter {
                        if !rev_list_filter_includes_known_kind(&store, id, kind, filter)? {
                            return Ok(());
                        }
                    }
                    let name = if no_object_names { None } else { name };
                    write_rev_list_object_output(&store, &mut out, id, name, missing_mode)?;
                    Ok(())
                },
            )?;
        }
        if phase_trace_enabled() {
            let (entries, bytes) = store.packed_object_read_cache_usage()?;
            phase_trace_emit(
                "rev_list.objects.pack_cache",
                0.0,
                &[
                    ("entries", entries.to_string()),
                    ("bytes", bytes.to_string()),
                ],
            );
        }
        for id in deferred_missing_ids {
            write_rev_list_object_output(&store, &mut out, &id, None, missing_mode)?;
        }
        out.flush()?;
        return Ok(());
    }

    let collect_max_count = if count || objects {
        expand_history_max_count(max_count, skip)
    } else {
        max_count
    };
    let collect_max_count = if post_collection_filters && (count || objects) {
        None
    } else {
        collect_max_count
    };

    let mut commit_ids = if post_collection_filters {
        let commit_cache = CommitObjectCache::new(&store);
        let mut commits = if no_walk && !all {
            collect_no_walk_commit_objects(
                &repo,
                &store,
                &commit_cache,
                &revs.include,
                collect_max_count,
            )?
        } else if first_parent && !all {
            collect_first_parent_commit_objects_with_exclusions(
                &repo,
                &store,
                &commit_cache,
                &revs,
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
        };
        if let Some(since) = since {
            commits.retain(|entry| {
                signature_timestamp_timezone(&entry.commit.committer)
                    .map(|(timestamp, _)| timestamp)
                    .is_some_and(|timestamp| timestamp > since)
            });
        }
        if let Some(until) = until {
            commits.retain(|entry| {
                signature_timestamp_timezone(&entry.commit.committer)
                    .map(|(timestamp, _)| timestamp)
                    .is_some_and(|timestamp| timestamp < until)
            });
        }
        if let Some(pattern) = author {
            commits.retain(|entry| {
                log_signature_matches_pattern(
                    &entry.commit.author,
                    pattern,
                    regexp_ignore_case,
                    grep_mode,
                )
            });
        }
        if let Some(pattern) = committer {
            commits.retain(|entry| {
                log_signature_matches_pattern(
                    &entry.commit.committer,
                    pattern,
                    regexp_ignore_case,
                    grep_mode,
                )
            });
        }
        if !grep.is_empty() {
            commits.retain(|entry| {
                shortlog_commit_matches_grep(
                    &entry.commit.message,
                    &grep,
                    all_match,
                    invert_grep,
                    regexp_ignore_case,
                    grep_mode,
                )
                .unwrap_or(false)
            });
        }
        if min_parents.is_some() || max_parents.is_some() {
            commits.retain(|entry| {
                log_parent_count_matches_bounds(
                    entry.commit.parents.len(),
                    min_parents,
                    max_parents,
                )
            });
        }
        commits.into_iter().map(|entry| entry.id).collect()
    } else {
        if no_walk && !all {
            collect_no_walk_commit_objects(
                &repo,
                &store,
                &commit_cache,
                &revs.include,
                collect_max_count,
            )?
            .into_iter()
            .map(|entry| entry.id)
            .collect()
        } else if first_parent && !all {
            collect_first_parent_commit_objects_with_exclusions(
                &repo,
                &store,
                &commit_cache,
                &revs,
                collect_max_count,
            )?
            .into_iter()
            .map(|entry| entry.id)
            .collect()
        } else {
            if policy_promised_objects.is_empty() {
                collect_commits_with_exclusions(&repo, &store, &revs, collect_max_count)?
            } else {
                let mut roots = Vec::with_capacity(revs.include.len());
                let mut seen = HashSet::with_capacity(revs.include.len());
                for rev in &revs.include {
                    let id = resolve_commitish(&repo, &store, rev)?;
                    if seen.insert(id.clone()) {
                        roots.push(id);
                    }
                }
                let mut excluded =
                    collect_rev_list_excluded_commits_uncached(&repo, &store, &revs)?
                        .into_iter()
                        .collect::<HashSet<_>>();
                excluded.extend(policy_promised_objects.iter().cloned());
                collect_commits_from_ids_cached_with_excluded(
                    &repo,
                    &commit_cache,
                    &roots,
                    collect_max_count,
                    &excluded,
                )?
            }
        }
    };
    if ancestry_path {
        commit_ids =
            filter_commit_ids_by_ancestry_path(&repo, &store, &commit_cache, &revs, commit_ids)?;
    }
    if simplify_by_decoration {
        let decorated = collect_default_log_decoration_ids(&repo, None)?;
        commit_ids.retain(|id| decorated.contains(id));
    }
    let traversal = collect_history_traversal_decoration(
        &repo,
        &store,
        &commit_cache,
        &revs,
        &commit_ids,
        left_right || left_only || right_only,
        cherry,
        cherry_pick,
        cherry_mark,
        boundary,
    )?;
    if cherry {
        commit_ids.retain(|id| traversal.markers.contains_key(id));
    }
    if cherry_pick && !traversal.equivalent_ids.is_empty() {
        commit_ids.retain(|id| !traversal.equivalent_ids.contains(id));
    }
    if boundary {
        commit_ids.extend(traversal.boundary_ids.iter().cloned());
    }
    if left_only {
        commit_ids.retain(|id| traversal.markers.get(id) == Some(&HistoryTraversalMarker::Left));
    }
    if right_only {
        commit_ids.retain(|id| traversal.markers.get(id) == Some(&HistoryTraversalMarker::Right));
    }
    if let Some(order) =
        history_order.or_else(|| simplify_history_topo.then_some(HistoryCommitOrder::Topo))
    {
        commit_ids = reorder_commit_ids(&commit_cache, commit_ids, order)?;
    }
    if let Some(skip) = skip {
        commit_ids = commit_ids.into_iter().skip(skip).collect();
    }
    if reverse {
        commit_ids.reverse();
    }
    let excluded_commits = if objects {
        let mut excluded_commits = collect_rev_list_excluded_commits(&repo, &store, &revs)?;
        if !exclude_promisor_objects {
            excluded_commits.extend(promised_excluded_commits.iter().cloned());
        }
        excluded_commits
    } else {
        Vec::new()
    };
    if disk_usage {
        println!(
            "{}",
            rev_list_disk_usage_bytes(&repo, &store, &commit_ids, no_walk, all)?
        );
        return Ok(());
    }
    let show_traversal_markers = left_right || cherry || cherry_mark || boundary;
    if count {
        let object_count =
            count_rev_list_objects(&store, &commit_ids, &revs.extra_objects, &excluded_commits)?;
        println!("{}", commit_ids.len() + object_count);
        return Ok(());
    }
    if bisect {
        if let Some(id) = rev_list_bisect_choice(&commit_ids) {
            println!("{id}");
        }
        return Ok(());
    }
    if bisect_all {
        let decorations =
            LogDecorations::load(&repo, &store, Some(LogDecorationMode::Short), false, None)?;
        let last_distance = commit_ids.len().saturating_sub(1);
        for (index, id) in commit_ids.iter().rev().enumerate() {
            let distance = last_distance.saturating_sub(index);
            if let Some(items) = decorations.get(id) {
                let mut parts = items.to_vec();
                parts.push(format!("dist={distance}"));
                println!("{id} ({})", parts.join(", "));
            } else {
                println!("{id} (dist={distance})");
            }
        }
        return Ok(());
    }
    if bisect_vars {
        if let Some(id) = rev_list_bisect_choice(&commit_ids) {
            println!("bisect_rev='{id}'");
            println!("bisect_nr=0");
            println!("bisect_good=0");
            println!("bisect_bad=0");
            println!("bisect_all={}", commit_ids.len());
            println!("bisect_steps=0");
        }
        return Ok(());
    }
    if header {
        let mut out = io::stdout().lock();
        for id in &commit_ids {
            let content = store.read_object(id)?;
            write_rev_list_header_record(&mut out, id, &content.content)?;
        }
        return Ok(());
    }
    let children_by_commit = if children {
        collect_rev_list_children(&store, &commit_ids, reverse)?
    } else {
        HashMap::new()
    };
    let decorations = LogDecorations::empty();
    let notes = LogNotes::empty();
    let mut out = io::stdout().lock();
    for id in &commit_ids {
        let marker = show_traversal_markers
            .then(|| traversal.markers.get(id).copied())
            .flatten();
        if let Some(format) = rendered_format.as_ref() {
            let commit = commit_cache.read_commit(id)?;
            let rendered = render_rev_list_format(
                format,
                id,
                commit.as_ref(),
                parents,
                abbrev_len,
                marker,
                default_commit_abbrev,
                expand_tabs,
                &decorations,
                &notes,
                date_mode,
            )?;
            if matches!(format, LogFormat::Custom { .. }) {
                writeln!(out, "commit {}", id.to_hex())?;
            }
            out.write_all(rendered.as_bytes())?;
            if format.terminates_lines() {
                out.write_all(b"\n")?;
            }
        } else if parents {
            if timestamp {
                let commit = commit_cache.read_commit(id)?;
                let timestamp = signature_timestamp_timezone(&commit.committer)
                    .map(|(timestamp, _)| timestamp)
                    .unwrap_or_default();
                write!(out, "{timestamp} ")?;
                write!(out, "{id}")?;
                for parent in &commit.parents {
                    write!(out, " {parent}")?;
                }
                writeln!(out)?;
                continue;
            }
            let parents = read_commit_parents_uncached(&store, id)?;
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
            if timestamp {
                let commit = commit_cache.read_commit(id)?;
                let timestamp = signature_timestamp_timezone(&commit.committer)
                    .map(|(timestamp, _)| timestamp)
                    .unwrap_or_default();
                write!(out, "{timestamp} ")?;
            }
            if let Some(marker) = marker {
                write!(out, "{}", marker.rev_list_prefix())?;
            }
            if default_commit_abbrev {
                writeln!(out, "{}", short_object_id_len(id, abbrev_len))?;
            } else if graph {
                writeln!(out, "* {id}")?;
            } else {
                writeln!(out, "{id}")?;
            }
        }
    }
    if objects {
        let mut deferred_missing_ids = Vec::new();
        for_each_rev_list_object_line_with(
            &store,
            &commit_ids,
            &revs.extra_objects,
            &excluded_commits,
            tree_missing_policy,
            |id, _, name| {
                if matches!(missing_mode, Some(RevListMissingMode::Print))
                    && !store.contains_object(id).map_err(CliError::Io)?
                {
                    deferred_missing_ids.push(id.clone());
                    let _ = name;
                    return Ok(());
                }
                write_rev_list_object_output(&store, &mut out, id, name, missing_mode)?;
                Ok(())
            },
        )?;
        for id in deferred_missing_ids {
            write_rev_list_object_output(&store, &mut out, &id, None, missing_mode)?;
        }
    }
    Ok(())
}

fn rev_list_bisect_choice(commit_ids: &[ObjectId]) -> Option<&ObjectId> {
    if commit_ids.is_empty() {
        None
    } else {
        commit_ids.get(commit_ids.len() / 2)
    }
}

fn rev_list_disk_usage_bytes(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_ids: &[ObjectId],
    no_walk: bool,
    all: bool,
) -> Result<u64> {
    if no_walk || all {
        return Ok(0);
    }
    let mut total = 0_u64;
    for id in commit_ids {
        let Some((kind, _)) = store.object_header_hint(id)? else {
            continue;
        };
        if kind != GitObjectKind::Commit {
            continue;
        }
        let path = repo
            .objects_dir
            .join(id.to_hex().get(..2).unwrap_or_default())
            .join(id.to_hex().get(2..).unwrap_or_default());
        if let Ok(metadata) = fs::metadata(path) {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

struct RevListReflogRenderOptions<'a> {
    rendered_format: Option<&'a LogFormat<'a>>,
    date_mode: LogDateMode<'a>,
    default_commit_abbrev: bool,
    abbrev_len: usize,
    parents: bool,
    regexp_ignore_case: bool,
    count: bool,
    max_count: Option<usize>,
}

fn rev_list_walk_reflogs(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    options: RevListReflogRenderOptions<'_>,
    revs: &[String],
    grep_reflog: &[String],
) -> Result<()> {
    let revs = revs
        .iter()
        .take_while(|rev| rev.as_str() != "--")
        .cloned()
        .collect::<Vec<_>>();
    if revs.is_empty() {
        return Err(CliError::Message("`rev-list` requires a revision".into()));
    }
    let commit_ids = collect_rev_list_reflog_commit_ids(
        repo,
        store,
        &revs,
        grep_reflog,
        options.regexp_ignore_case,
        options.max_count,
    )?;
    if options.count {
        println!("{}", commit_ids.len());
        return Ok(());
    }
    let decorations = LogDecorations::empty();
    let notes = LogNotes::empty();
    let mut out = io::stdout().lock();
    for id in &commit_ids {
        if let Some(format) = options.rendered_format {
            let commit = commit_cache.read_commit(id)?;
            let rendered = render_rev_list_format(
                format,
                id,
                commit.as_ref(),
                options.parents,
                options.abbrev_len,
                None,
                options.default_commit_abbrev,
                false,
                &decorations,
                &notes,
                options.date_mode,
            )?;
            if matches!(format, LogFormat::Custom { .. }) {
                writeln!(out, "commit {}", id.to_hex())?;
            }
            out.write_all(rendered.as_bytes())?;
            if format.terminates_lines() {
                out.write_all(b"\n")?;
            }
        } else if options.parents {
            let parents = read_commit_parents_uncached(store, id)?;
            write!(out, "{id}")?;
            for parent in parents {
                write!(out, " {parent}")?;
            }
            writeln!(out)?;
        } else if options.default_commit_abbrev {
            writeln!(out, "{}", short_object_id_len(id, options.abbrev_len))?;
        } else {
            writeln!(out, "{id}")?;
        }
    }
    Ok(())
}

fn collect_first_parent_commit_objects_with_exclusions<S>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    revs: &RevListRevs,
    max_count: Option<usize>,
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    let roots = if revs.include.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        revs.include.clone()
    };
    let excluded = collect_rev_list_excluded_commits(repo, store, revs)?
        .into_iter()
        .collect::<HashSet<_>>();
    let mut commits = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let mut current = resolve_commitish(repo, store, &root)?;
        loop {
            if !seen.insert(current.clone()) {
                break;
            }
            if excluded.contains(&current) {
                break;
            }
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

fn collect_rev_list_reflog_commit_ids(
    repo: &GitRepo,
    _store: &LooseObjectStore,
    revs: &[String],
    grep_reflog: &[String],
    regexp_ignore_case: bool,
    max_count: Option<usize>,
) -> Result<Vec<ObjectId>> {
    let mut commit_ids = Vec::new();
    let limit = max_count.unwrap_or(usize::MAX);
    for target in revs {
        if commit_ids.len() >= limit {
            break;
        }
        let path = reflog_path(repo, target)?;
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && resolve_objectish(repo, target).is_ok() =>
            {
                continue;
            }
            Err(error) => return Err(CliError::Io(error)),
        };
        for_each_reflog_line_rev(file, |line| {
            if commit_ids.len() >= limit {
                return Ok(());
            }
            let Some(entry) = parse_reflog_entry(line) else {
                return Ok(());
            };
            if entry.new_id == zero_object_id() {
                return Ok(());
            }
            if !grep_reflog.is_empty()
                && !shortlog_commit_matches_grep(
                    entry.message.as_bytes(),
                    grep_reflog,
                    false,
                    false,
                    regexp_ignore_case,
                    ShortlogPatternMode::Basic,
                )?
            {
                return Ok(());
            }
            commit_ids.push(entry.new_id);
            Ok(())
        })?;
    }
    Ok(commit_ids)
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

fn parse_rev_list_missing_mode(value: Option<&str>) -> Result<Option<RevListMissingMode>> {
    match value {
        None => Ok(None),
        Some("") => Err(rev_list_usage_error()),
        Some("allow-any") => Ok(Some(RevListMissingMode::AllowAny)),
        Some("allow-promisor") => Ok(Some(RevListMissingMode::AllowPromisor)),
        Some("print") => Ok(Some(RevListMissingMode::Print)),
        Some(_) => Err(rev_list_usage_error()),
    }
}

fn sanitize_rev_list_promised_inputs(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &mut RevListRevs,
    promised_missing: &HashSet<ObjectId>,
    exclude_promisor_objects: bool,
    ignore_missing: bool,
) -> Result<()> {
    if !exclude_promisor_objects && !ignore_missing {
        return Ok(());
    }

    let mut include = Vec::with_capacity(revs.include.len());
    for rev in std::mem::take(&mut revs.include) {
        let Ok(id) = resolve_objectish(repo, &rev) else {
            include.push(rev);
            continue;
        };
        let missing_locally = !store.contains_object(&id).map_err(CliError::Io)?;
        if missing_locally
            && (ignore_missing || promised_missing.contains(&id) || exclude_promisor_objects)
        {
            if ignore_missing {
                continue;
            }
            if exclude_promisor_objects {
                return Err(ambiguous_revision_error(&rev));
            }
        }
        include.push(rev);
    }
    revs.include = include;

    let mut extra_objects = Vec::with_capacity(revs.extra_objects.len());
    for (id, name) in std::mem::take(&mut revs.extra_objects) {
        let missing_locally = !store.contains_object(&id).map_err(CliError::Io)?;
        if missing_locally
            && (ignore_missing || promised_missing.contains(&id) || exclude_promisor_objects)
        {
            if ignore_missing {
                continue;
            }
            if exclude_promisor_objects {
                let display = if name.is_empty() { id.to_hex() } else { name };
                return Err(ambiguous_revision_error(&display));
            }
        }
        extra_objects.push((id, name));
    }
    revs.extra_objects = extra_objects;

    Ok(())
}

fn write_rev_list_object_output<W: std::io::Write>(
    store: &LooseObjectStore,
    out: &mut W,
    id: &ObjectId,
    name: Option<&[u8]>,
    missing_mode: Option<RevListMissingMode>,
) -> Result<()> {
    if matches!(missing_mode, Some(RevListMissingMode::Print)) {
        let present = store.contains_object(id).map_err(CliError::Io)?;
        if !present {
            out.write_all(b"?")?;
            id.write_hex_io(out)?;
            out.write_all(b"\n")?;
            return Ok(());
        }
    }
    id.write_hex_io(out)?;
    if let Some(name) = name {
        out.write_all(b" ")?;
        out.write_all(name)?;
    }
    out.write_all(b"\n")?;
    Ok(())
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

fn rev_list_filter_includes_known_kind(
    store: &LooseObjectStore,
    id: &ObjectId,
    kind: Option<GitObjectKind>,
    filter: RevListObjectFilter,
) -> Result<bool> {
    let Some(kind) = kind else {
        return rev_list_filter_includes(store, id, filter);
    };
    Ok(match filter {
        RevListObjectFilter::BlobNone => kind != GitObjectKind::Blob,
        RevListObjectFilter::BlobLimit(limit) if kind == GitObjectKind::Blob => store
            .object_header_hint(id)?
            .is_some_and(|(_, size)| size <= limit),
        RevListObjectFilter::BlobLimit(_) => true,
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
    write_rev_list_object_ids_uncached(
        store,
        commits,
        extra_objects,
        excluded_commits,
        RevListTreeMissingPolicy::default(),
        &mut out,
    )?;
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

pub(crate) fn merge_base(
    all: bool,
    is_ancestor: bool,
    octopus: bool,
    commits: Vec<String>,
) -> Result<()> {
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

    if all {
        for base in merge_bases_all_cached(&commit_cache, left, right)? {
            println!("{}", base.to_hex());
        }
        return Ok(());
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
    pub(crate) prune_empty: bool,
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
    let empty_tree = if options.prune_empty {
        Some(store.write_object(
            GitObjectKind::Tree,
            &encode_tree(&[]).map_err(CliError::Io)?,
        )?)
    } else {
        None
    };
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
        print!("\r");
        println!(
            "Rewrite {} ({}/{}) (0 seconds passed, remaining 0 predicted)    ",
            old_id.to_hex(),
            idx + 1,
            total
        );
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
        } else if options.prune_empty
            && filter_branch_should_prune_commit(
                &commit_cache,
                empty_tree.as_ref().expect("prune-empty tree"),
                &tree,
                &parents,
            )?
        {
            parents.first().map(ObjectId::to_hex).unwrap_or_default()
        } else {
            let encoded = encode_raw_commit(&tree, &parents, &author, &committer, &message)?;
            let new_id = store.write_object(GitObjectKind::Commit, &encoded)?;
            new_id.to_hex()
        };
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
    if _options.prune_empty && _options.commit_filter.is_some() {
        return Err(CliError::Stderr {
            code: 1,
            text: "Cannot set --prune-empty and --commit-filter at the same time\n".into(),
        });
    }
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

fn filter_branch_should_prune_commit(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    empty_tree: &ObjectId,
    tree: &ObjectId,
    parents: &[ObjectId],
) -> Result<bool> {
    if parents.len() > 1 {
        return Ok(false);
    }
    if let Some(parent) = parents.first() {
        return Ok(commit_cache.read_commit(parent)?.tree == *tree);
    }
    Ok(tree == empty_tree)
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
            return Err(filter_branch_commit_tree_failure());
        }
        let Some(parent) = parts.next() else {
            return Err(filter_branch_commit_tree_failure());
        };
        parents.push(ObjectId::from_hex(GitHashAlgorithm::Sha1, parent).map_err(CliError::Io)?);
    }
    Ok(parents)
}

fn filter_branch_commit_tree_failure() -> CliError {
    CliError::Stderr {
        code: 1,
        text: "fatal: must give exactly one tree\ncould not write rewritten commit\n".into(),
    }
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

    use std::cell::Cell;
    use tempfile::TempDir;
    use zmin_git_core::object_store::PrefixOrFullObject;
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

    #[test]
    fn author_metadata_with_full_hints_uses_prefix_object_read_path() {
        struct CountingStore {
            object: LooseObject,
            full_reads: Cell<usize>,
            prefix_reads: Cell<usize>,
        }

        impl GitObjectStore for CountingStore {
            fn read_object(&self, _id: &ObjectId) -> io::Result<LooseObject> {
                self.full_reads.set(self.full_reads.get() + 1);
                Ok(self.object.clone())
            }

            fn read_object_prefix_or_full(
                &self,
                _id: &ObjectId,
                _max_bytes: usize,
            ) -> io::Result<PrefixOrFullObject> {
                self.prefix_reads.set(self.prefix_reads.get() + 1);
                Ok(PrefixOrFullObject {
                    object: self.object.clone(),
                    is_complete: true,
                })
            }
        }

        let author = Signature::new("Example User", "user@example.test", 1716200000, "+0300")
            .expect("author");
        let committer = Signature::new("Commit User", "commit@example.test", 1716200100, "+0300")
            .expect("committer");
        let tree = ObjectId::from_hex(GitHashAlgorithm::Sha1, &"1".repeat(40)).expect("tree id");
        let commit_bytes = CommitBuilder::new(tree, author, committer)
            .message("subject\n".to_owned())
            .expect("message")
            .encode()
            .expect("encode commit");
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, &"2".repeat(40)).expect("object id");
        let parent =
            ObjectId::from_hex(GitHashAlgorithm::Sha1, &"3".repeat(40)).expect("parent id");
        let store = CountingStore {
            object: LooseObject {
                id: id.clone(),
                kind: GitObjectKind::Commit,
                content: commit_bytes,
            },
            full_reads: Cell::new(0),
            prefix_reads: Cell::new(0),
        };

        let metadata = read_commit_author_metadata_with_hints(
            &store,
            &id,
            LogMetadataRenderPlan {
                needs_parents: false,
                needs_author_name: true,
                needs_author_email: true,
                needs_author_signature: false,
                needs_author_timestamp: false,
                needs_committer_name: false,
                needs_committer_email: false,
                needs_committer_signature: false,
                needs_committer_timestamp: false,
            },
            CommitRenderMetadataHints {
                parents: Some(Arc::from([parent.clone()])),
                committer_timestamp: Some(1716200100),
            },
        )
        .expect("metadata");

        assert_eq!(metadata.author_name.as_deref(), Some("Example User"));
        assert_eq!(metadata.author_email.as_deref(), Some("user@example.test"));
        assert_eq!(metadata.parents.as_ref(), [parent].as_slice());
        assert_eq!(store.full_reads.get(), 0);
        assert_eq!(store.prefix_reads.get(), 1);
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
use std::collections::hash_map::Entry;
