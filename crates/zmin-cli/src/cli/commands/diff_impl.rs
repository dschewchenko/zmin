use super::*;

pub(crate) fn diff(options: DiffOptions) -> Result<()> {
    if options.no_index {
        return diff_no_index(&options);
    }
    let mut detect_renames = parse_find_renames_option(options.find_renames.as_deref())?;
    let break_rewrites = parse_break_rewrites_option(options.break_rewrites.as_deref())?;
    let mut detect_copies = parse_find_copies_option(options.find_copies.as_deref())?;
    let mut find_copies_harder = options.find_copies_harder;
    if options.no_renames {
        detect_renames = None;
        detect_copies = None;
        find_copies_harder = false;
    }
    let diff_filter = options
        .diff_filter
        .as_deref()
        .map(parse_diff_filter)
        .transpose()?
        .unwrap_or_default();
    let word_diff = parse_word_diff_option(options.word_diff.as_deref())?;
    let ignore_submodules = parse_ignore_submodules_mode(options.ignore_submodules.as_deref())?;
    let abbrev_len = parse_diff_abbrev_len(options.abbrev.as_deref(), options.no_abbrev)?;
    let patch_abbrev_len = if options.full_index && !options.no_full_index {
        Some(GitHashAlgorithm::Sha1.digest_len() * 2)
    } else {
        abbrev_len
    };
    let unified_context = options
        .unified
        .as_deref()
        .map(|value| parse_diff_context_value("--unified", value))
        .transpose()?
        .unwrap_or(3);
    let inter_hunk_context = options
        .inter_hunk_context
        .as_deref()
        .map(|value| parse_diff_context_value("--inter-hunk-context", value))
        .transpose()?
        .unwrap_or(0);
    let submodule_format = parse_submodule_diff_format(options.submodule.as_deref())?;
    validate_diff_algorithm_options(
        options.minimal,
        options.patience,
        options.histogram,
        options.diff_algorithm.as_deref(),
        &options.anchored,
    )?;
    let output_indicator_new = parse_output_indicator(
        "--output-indicator-new",
        options.output_indicator_new.as_deref(),
    )?;
    let output_indicator_old = parse_output_indicator(
        "--output-indicator-old",
        options.output_indicator_old.as_deref(),
    )?;
    let output_indicator_context = parse_output_indicator(
        "--output-indicator-context",
        options.output_indicator_context.as_deref(),
    )?;
    let ignore_matching_lines = compile_ignore_matching_lines(&options.ignore_matching_lines)?;
    let whitespace_mode = diff_whitespace_mode(
        options.ignore_space_at_eol,
        options.ignore_cr_at_eol,
        options.ignore_space_change,
        options.ignore_all_space,
        options.ignore_blank_lines,
    );
    let color_mode = parse_diff_color_option(options.color.as_deref(), options.no_color)?;
    let _accepted_noops = (
        options.no_ext_diff,
        options.no_textconv,
        options.no_color,
        options.no_color_moved,
        options.no_color_moved_ws,
        options.dense_combined,
    );
    let repo = find_repo()?;
    let relative_prefix =
        diff_relative_prefix(&repo, options.relative.as_deref(), options.no_relative)?;
    let (old_prefix, new_prefix) = porcelain_diff_prefixes(
        &repo,
        options.no_prefix,
        options.default_prefix,
        options.src_prefix,
        options.dst_prefix,
    )?;
    let render_options = DiffRenderOptions {
        stat: options.stat,
        patch_with_raw: options.patch_with_raw,
        patch_with_stat: options.patch_with_stat,
        compact_summary: options.compact_summary,
        numstat: options.numstat,
        shortstat: options.shortstat,
        raw: options.raw,
        summary: options.summary,
        name_status: options.name_status,
        name_only: options.name_only,
        nul_terminated: options.nul_terminated,
        patch: !options.no_patch,
        no_patch: options.no_patch,
        binary: options.binary,
        quiet: options.quiet,
        exit_code: options.exit_code,
        raw_abbrev_len: abbrev_len,
        word_diff,
        patch_abbrev_len,
        old_prefix,
        new_prefix,
        unified_context,
        inter_hunk_context,
        output_indicator_new,
        output_indicator_old,
        output_indicator_context,
        ignore_matching_lines,
        ignore_blank_lines: options.ignore_blank_lines,
        whitespace_mode,
        relative_prefix,
        text: options.text,
        irreversible_delete: options.irreversible_delete,
        submodule_format,
        color_mode,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
        line_prefix: options.line_prefix.clone(),
    };
    render_options.validate_format(false)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if !options.cached
        && let Some(combined_input) =
            parse_porcelain_combined_diff_input(&repo, &store, &options.paths)
    {
        return render_porcelain_combined_diff(
            &repo,
            &store,
            &combined_input,
            &render_options,
            options.no_patch,
            options.summary,
            options.shortstat,
            options.line_prefix.as_deref(),
        );
    }
    let index = read_repo_index(&repo)?;
    let diff_input = parse_diff_input(&repo, &store, &index, options.cached, options.paths)?;
    let (old_source, new_source) = diff_side_sources(diff_input.new_side_from_index);
    let pathspecs = diff_input
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let precomputed_entries = diff_input.precomputed_entries;
    let (mut old_index, mut new_index, old_source, new_source, mut render_options) =
        if options.reverse {
            let mut render_options = render_options;
            render_options.old_source = old_source;
            render_options.new_source = new_source;
            render_options.reverse_direction();
            (
                diff_input.new_index,
                diff_input.old_index,
                new_source,
                old_source,
                render_options,
            )
        } else {
            let mut render_options = render_options;
            render_options.old_source = old_source;
            render_options.new_source = new_source;
            (
                diff_input.old_index,
                diff_input.new_index,
                old_source,
                new_source,
                render_options,
            )
        };
    let unmerged_entries = unmerged_diff_entries([&old_index, &new_index]);
    if !unmerged_entries.is_empty() {
        old_index = stage_zero_index(&old_index)?;
        new_index = stage_zero_index(&new_index)?;
    }
    let mut entries = if let Some(mut entries) = precomputed_entries {
        if options.reverse {
            reverse_precomputed_diff_entries(&mut entries);
        }
        entries
    } else {
        diff_entries_for_indexes(
            &old_index,
            &new_index,
            detect_renames,
            detect_copies,
            find_copies_harder,
        )?
    };
    entries.extend(unmerged_entries);
    let diff_context = DiffIndexContext {
        repo: &repo,
        store: &store,
        old_index: &old_index,
        new_index: &new_index,
        old_source,
        new_source,
    };
    let entries = apply_similarity_detection(
        &diff_context,
        entries,
        SimilarityDetectionOptions {
            rename_threshold: detect_renames,
            copy_threshold: detect_copies,
            find_copies_harder,
        },
    )?;
    let entries =
        filter_ignored_submodule_entries(entries, &old_index, &new_index, ignore_submodules);
    let entries = apply_break_rewrites(&diff_context, entries, break_rewrites)?;
    let entries = entries
        .into_iter()
        .filter(|entry| diff_entry_matches_pathspec(entry, &pathspecs))
        .collect::<Vec<_>>();
    let entries = apply_pickaxe_filter(
        &diff_context,
        entries,
        PickaxeOptions {
            string: options.pickaxe_string.as_deref(),
            regex: options.pickaxe_regex.as_deref(),
            regex_mode: options.pickaxe_regex_mode,
            all: options.pickaxe_all,
        },
    )?;
    let entries = apply_diff_filter(entries, diff_filter);
    let entries = apply_diff_order_file(entries, options.order_file.as_deref())?;
    let entries = apply_diff_skip_rotate(
        entries,
        options.skip_to.as_deref(),
        options.rotate_to.as_deref(),
    );
    let entries = filter_diff_relative(entries, render_options.relative_prefix.as_deref());
    if options.dirstat.is_some() || options.dirstat_by_file {
        return print_dirstat_entries(
            &diff_context,
            &entries,
            DiffStatOptions {
                whitespace_mode,
                relative_prefix: render_options.relative_prefix.as_deref(),
                ignore_matching_lines: &render_options.ignore_matching_lines,
                ignore_blank_lines: render_options.ignore_blank_lines,
                compact_summary: false,
            },
            options.dirstat_by_file,
        );
    }
    if options.check {
        return diff_check(
            &repo, &store, &old_index, &new_index, &entries, old_source, new_source,
        );
    }
    render_options.old_source = old_source;
    render_options.new_source = new_source;
    render_diff(
        &repo,
        &store,
        &old_index,
        &new_index,
        &entries,
        render_options,
    )
}

fn reverse_precomputed_diff_entries(entries: &mut [zmin_git_core::IndexDiffEntry]) {
    for entry in entries {
        entry.status = match entry.status {
            IndexDiffStatus::Added => IndexDiffStatus::Deleted,
            IndexDiffStatus::Deleted => IndexDiffStatus::Added,
            status => status,
        };
        let previous_new_path = entry.path.clone();
        entry.path = entry.old_path.take().unwrap_or(previous_new_path.clone());
        entry.old_path = (previous_new_path != entry.path).then_some(previous_new_path);
    }
}

struct PorcelainCombinedDiffInput {
    result: String,
    parents: Vec<String>,
    paths: Vec<PathBuf>,
}

fn parse_porcelain_combined_diff_input(
    repo: &GitRepo,
    store: &LooseObjectStore,
    args: &[PathBuf],
) -> Option<PorcelainCombinedDiffInput> {
    let mut revs = Vec::new();
    let mut path_start = args.len();
    for (idx, arg) in args.iter().enumerate() {
        let arg = arg.to_string_lossy();
        if arg == "--" {
            path_start = idx + 1;
            break;
        }
        if revs.len() == 3 {
            path_start = idx;
            break;
        }
        if resolve_treeish(repo, store, &arg).is_ok() {
            revs.push(arg.into_owned());
            path_start = idx + 1;
            continue;
        }
        return None;
    }
    if revs.len() != 3 {
        return None;
    }
    Some(PorcelainCombinedDiffInput {
        result: revs.remove(0),
        parents: revs,
        paths: args.iter().skip(path_start).cloned().collect(),
    })
}

fn render_porcelain_combined_diff(
    repo: &GitRepo,
    store: &LooseObjectStore,
    input: &PorcelainCombinedDiffInput,
    render_options: &DiffRenderOptions,
    no_patch: bool,
    summary: bool,
    shortstat: bool,
    line_prefix: Option<&str>,
) -> Result<()> {
    if no_patch {
        return Ok(());
    }
    let tree_cache = TreeObjectCache::new(store);
    let parent_indexes = input
        .parents
        .iter()
        .map(|parent| read_treeish_index_cached(repo, store, &tree_cache, parent))
        .collect::<Result<Vec<_>>>()?;
    let result_index = read_treeish_index_cached(repo, store, &tree_cache, &input.result)?;
    let pathspecs = input
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    if render_options.patch_with_stat {
        print_combined_diff_tree_stat(
            repo,
            store,
            &parent_indexes,
            &result_index,
            &pathspecs,
            CombinedStatRenderOptions {
                relative_prefix: render_options.relative_prefix.as_deref(),
                whitespace_mode: render_options.whitespace_mode,
                ignore_matching_lines: &render_options.ignore_matching_lines,
                ignore_blank_lines: render_options.ignore_blank_lines,
                shortstat: false,
            },
        )?;
        if summary {
            print_combined_diff_tree_summary(
                &parent_indexes,
                &result_index,
                &pathspecs,
                render_options.relative_prefix.as_deref(),
            )?;
        }
        println!();
        return print_combined_diff_tree_patches(
            store,
            &parent_indexes,
            &result_index,
            &pathspecs,
            CombinedPatchRenderOptions {
                abbrev_len: render_options.patch_abbrev_len,
                relative_prefix: render_options.relative_prefix.as_deref(),
                old_prefix: &render_options.old_prefix,
                new_prefix: &render_options.new_prefix,
                dense_combined: true,
                line_prefix,
            },
        );
    }
    if render_options.stat || render_options.shortstat {
        return print_combined_diff_tree_stat(
            repo,
            store,
            &parent_indexes,
            &result_index,
            &pathspecs,
            CombinedStatRenderOptions {
                relative_prefix: render_options.relative_prefix.as_deref(),
                whitespace_mode: render_options.whitespace_mode,
                ignore_matching_lines: &render_options.ignore_matching_lines,
                ignore_blank_lines: render_options.ignore_blank_lines,
                shortstat,
            },
        );
    }
    if render_options.raw {
        return print_combined_diff_tree_raw_entries(
            store,
            &parent_indexes,
            &result_index,
            &pathspecs,
            render_options.raw_abbrev_len,
            render_options.relative_prefix.as_deref(),
            render_options.nul_terminated,
        );
    }
    if summary {
        return print_combined_diff_tree_summary(
            &parent_indexes,
            &result_index,
            &pathspecs,
            render_options.relative_prefix.as_deref(),
        );
    }
    print_combined_diff_tree_patches(
        store,
        &parent_indexes,
        &result_index,
        &pathspecs,
        CombinedPatchRenderOptions {
            abbrev_len: render_options.patch_abbrev_len,
            relative_prefix: render_options.relative_prefix.as_deref(),
            old_prefix: &render_options.old_prefix,
            new_prefix: &render_options.new_prefix,
            dense_combined: true,
            line_prefix,
        },
    )
}

fn porcelain_diff_prefixes(
    repo: &GitRepo,
    no_prefix: bool,
    default_prefix: bool,
    src_prefix: Option<String>,
    dst_prefix: Option<String>,
) -> Result<(String, String)> {
    if no_prefix || default_prefix || src_prefix.is_some() || dst_prefix.is_some() {
        return Ok(diff_prefixes(
            no_prefix,
            default_prefix,
            src_prefix,
            dst_prefix,
        ));
    }
    if read_config_value(repo, "diff.noPrefix")?
        .as_deref()
        .is_some_and(|value| value.is_empty() || parse_git_bool(value) == Some(true))
    {
        return Ok((String::new(), String::new()));
    }
    if read_config_value(repo, "diff.mnemonicPrefix")?
        .as_deref()
        .is_some_and(|value| value.is_empty() || parse_git_bool(value) == Some(true))
    {
        return Ok(("i/".to_owned(), "w/".to_owned()));
    }
    Ok((
        read_config_value(repo, "diff.srcPrefix")?.unwrap_or_else(|| "a/".to_owned()),
        read_config_value(repo, "diff.dstPrefix")?.unwrap_or_else(|| "b/".to_owned()),
    ))
}

pub(crate) fn diff_files(options: PlumbingDiffOptions) -> Result<()> {
    let detect_renames = parse_find_renames_option(options.find_renames.as_deref())?;
    let break_rewrites = parse_break_rewrites_option(options.break_rewrites.as_deref())?;
    let detect_copies = parse_find_copies_option(options.find_copies.as_deref())?;
    let ignore_submodules = parse_ignore_submodules_mode(options.ignore_submodules.as_deref())?;
    let diff_filter = options
        .diff_filter
        .as_deref()
        .map(parse_diff_filter)
        .transpose()?
        .unwrap_or_default();
    let word_diff = parse_word_diff_option(options.word_diff.as_deref())?;
    let render_options = plumbing_render_options(&options)?;
    let render_options = DiffRenderOptions {
        word_diff,
        ..render_options
    };
    render_options.validate_format(true)?;
    let repo = find_repo()?;
    let relative_prefix =
        diff_relative_prefix(&repo, options.relative.as_deref(), options.no_relative)?;
    let render_options = DiffRenderOptions {
        relative_prefix: relative_prefix.clone(),
        ..render_options
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let index = stage_zero_index(&index)?;
    let new_index = worktree_diff_index_snapshot(&repo, &index)?;
    let (old_index, new_index, render_options) = if options.reverse {
        let mut render_options = render_options;
        render_options.old_source = DiffSideSource::Index;
        render_options.new_source = DiffSideSource::WorktreeOrIndex;
        render_options.reverse_direction();
        (new_index, index, render_options)
    } else {
        let mut render_options = render_options;
        render_options.old_source = DiffSideSource::Index;
        render_options.new_source = DiffSideSource::WorktreeOrIndex;
        (index, new_index, render_options)
    };
    let mut entries = filtered_diff_entries(
        &repo,
        &old_index,
        &new_index,
        &options.paths,
        detect_renames,
        detect_copies,
        options.find_copies_harder,
    )?;
    let index_for_stat_dirty = if options.reverse {
        &new_index
    } else {
        &old_index
    };
    append_worktree_stat_dirty_entries(&repo, index_for_stat_dirty, &options.paths, &mut entries)?;
    let diff_context = DiffIndexContext {
        repo: &repo,
        store: &store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: render_options.old_source,
        new_source: render_options.new_source,
    };
    let entries = apply_similarity_detection(
        &diff_context,
        entries,
        SimilarityDetectionOptions {
            rename_threshold: detect_renames,
            copy_threshold: detect_copies,
            find_copies_harder: options.find_copies_harder,
        },
    )?;
    let entries =
        filter_ignored_submodule_entries(entries, &old_index, &new_index, ignore_submodules);
    let entries = apply_break_rewrites(&diff_context, entries, break_rewrites)?;
    let entries = apply_pickaxe_filter(
        &diff_context,
        entries,
        PickaxeOptions {
            string: options.pickaxe_string.as_deref(),
            regex: options.pickaxe_regex.as_deref(),
            regex_mode: options.pickaxe_regex_mode,
            all: options.pickaxe_all,
        },
    )?;
    let entries = apply_diff_filter(entries, diff_filter);
    let entries = apply_diff_order_file(entries, options.order_file.as_deref())?;
    let entries = apply_diff_skip_rotate(
        entries,
        options.skip_to.as_deref(),
        options.rotate_to.as_deref(),
    );
    let entries = filter_diff_relative(entries, relative_prefix.as_deref());
    render_diff(
        &repo,
        &store,
        &old_index,
        &new_index,
        &entries,
        render_options,
    )
}

fn stage_zero_index(index: &GitIndex) -> Result<GitIndex> {
    Ok(GitIndex::from_entries(
        index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0)
            .cloned()
            .collect(),
    )?)
}

fn unmerged_diff_entries(indexes: [&GitIndex; 2]) -> Vec<zmin_git_core::IndexDiffEntry> {
    let mut paths = BTreeSet::new();
    for index in indexes {
        for entry in index.entries().iter().filter(|entry| entry.stage != 0) {
            paths.insert(entry.path.to_vec());
        }
    }
    paths
        .into_iter()
        .map(|path| zmin_git_core::IndexDiffEntry {
            status: IndexDiffStatus::Added,
            path,
            old_path: None,
            similarity: None,
        })
        .collect()
}

fn append_worktree_stat_dirty_entries(
    repo: &GitRepo,
    index: &GitIndex,
    paths: &[PathBuf],
    entries: &mut Vec<zmin_git_core::IndexDiffEntry>,
) -> Result<()> {
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    let mut existing_paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    for entry in worktree_stat_dirty_diff_entries(repo, index)?
        .into_iter()
        .filter(|entry| diff_entry_matches_pathspec(entry, &pathspecs))
    {
        if existing_paths.insert(entry.path.clone()) {
            entries.push(entry);
        }
    }
    Ok(())
}

pub(crate) fn diff_index(options: PlumbingDiffOptions) -> Result<()> {
    let detect_renames = parse_find_renames_option(options.find_renames.as_deref())?;
    let break_rewrites = parse_break_rewrites_option(options.break_rewrites.as_deref())?;
    let detect_copies = parse_find_copies_option(options.find_copies.as_deref())?;
    let ignore_submodules = parse_ignore_submodules_mode(options.ignore_submodules.as_deref())?;
    let diff_filter = options
        .diff_filter
        .as_deref()
        .map(parse_diff_filter)
        .transpose()?
        .unwrap_or_default();
    let word_diff = parse_word_diff_option(options.word_diff.as_deref())?;
    let render_options = plumbing_render_options(&options)?;
    let render_options = DiffRenderOptions {
        word_diff,
        ..render_options
    };
    render_options.validate_format(true)?;
    let repo = find_repo()?;
    let relative_prefix =
        diff_relative_prefix(&repo, options.relative.as_deref(), options.no_relative)?;
    let render_options = DiffRenderOptions {
        relative_prefix: relative_prefix.clone(),
        ..render_options
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let treeish = options.treeish.as_deref().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "diff-index requires a tree-ish".into(),
    })?;
    let old_index = read_treeish_index(&repo, &store, treeish)?;
    let new_index = if options.cached {
        index.clone()
    } else {
        worktree_diff_index_snapshot_with_missing(&repo, &index, options.merge)?
    };
    let (base_old_source, base_new_source) = diff_side_sources(options.cached);
    let (old_index, new_index, render_options) = if options.reverse {
        let mut render_options = render_options;
        render_options.old_source = base_old_source;
        render_options.new_source = base_new_source;
        render_options.reverse_direction();
        (new_index, old_index, render_options)
    } else {
        let mut render_options = render_options;
        render_options.old_source = base_old_source;
        render_options.new_source = base_new_source;
        (old_index, new_index, render_options)
    };
    let mut entries = filtered_diff_entries(
        &repo,
        &old_index,
        &new_index,
        &options.paths,
        detect_renames,
        detect_copies,
        options.find_copies_harder,
    )?;
    if !options.cached {
        append_worktree_stat_dirty_entries(&repo, &index, &options.paths, &mut entries)?;
    }
    let diff_context = DiffIndexContext {
        repo: &repo,
        store: &store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: render_options.old_source,
        new_source: render_options.new_source,
    };
    let entries = apply_similarity_detection(
        &diff_context,
        entries,
        SimilarityDetectionOptions {
            rename_threshold: detect_renames,
            copy_threshold: detect_copies,
            find_copies_harder: options.find_copies_harder,
        },
    )?;
    let entries =
        filter_ignored_submodule_entries(entries, &old_index, &new_index, ignore_submodules);
    let entries = apply_break_rewrites(&diff_context, entries, break_rewrites)?;
    let entries = apply_pickaxe_filter(
        &diff_context,
        entries,
        PickaxeOptions {
            string: options.pickaxe_string.as_deref(),
            regex: options.pickaxe_regex.as_deref(),
            regex_mode: options.pickaxe_regex_mode,
            all: options.pickaxe_all,
        },
    )?;
    let entries = apply_diff_filter(entries, diff_filter);
    let entries = apply_diff_order_file(entries, options.order_file.as_deref())?;
    let entries = apply_diff_skip_rotate(
        entries,
        options.skip_to.as_deref(),
        options.rotate_to.as_deref(),
    );
    let entries = filter_diff_relative(entries, relative_prefix.as_deref());
    render_diff(
        &repo,
        &store,
        &old_index,
        &new_index,
        &entries,
        render_options,
    )
}

pub(crate) fn diff_tree(options: PlumbingDiffOptions) -> Result<()> {
    let detect_renames = parse_find_renames_option(options.find_renames.as_deref())?;
    let break_rewrites = parse_break_rewrites_option(options.break_rewrites.as_deref())?;
    let detect_copies = parse_find_copies_option(options.find_copies.as_deref())?;
    let ignore_submodules = parse_ignore_submodules_mode(options.ignore_submodules.as_deref())?;
    let diff_filter = options
        .diff_filter
        .as_deref()
        .map(parse_diff_filter)
        .transpose()?
        .unwrap_or_default();
    let word_diff = parse_word_diff_option(options.word_diff.as_deref())?;
    let render_options = plumbing_render_options(&options)?;
    let render_options = DiffRenderOptions {
        word_diff,
        ..render_options
    };
    render_options.validate_format(true)?;
    let repo = find_repo()?;
    let relative_prefix =
        diff_relative_prefix(&repo, options.relative.as_deref(), options.no_relative)?;
    let render_options = DiffRenderOptions {
        relative_prefix: relative_prefix.clone(),
        ..render_options
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    if options.stdin {
        return diff_tree_stdin(
            &repo,
            &store,
            &commit_cache,
            &tree_cache,
            &options,
            &render_options,
            DiffTreeStdinOptions {
                detect_renames,
                detect_copies,
                break_rewrites,
                ignore_submodules,
                diff_filter,
                relative_prefix: relative_prefix.as_deref(),
            },
        );
    }
    let old = options.treeish.as_deref().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "diff-tree requires a tree-ish".into(),
    })?;
    if (options.combined || options.dense_combined)
        && options.new_treeish.is_none()
        && combined_diff_tree_patch_with_stat_mode(&options)
    {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        if commit.parents.len() > 1 {
            print_diff_tree_commit_id(&id, options.nul_terminated);
            let parent_indexes =
                combined_diff_tree_parent_indexes(&commit, &commit_cache, &tree_cache)?;
            let result_index = tree_cache
                .read_tree_to_index(&commit.tree)
                .map_err(CliError::Io)?;
            let pathspecs = options
                .paths
                .iter()
                .map(|path| path_arg_to_repo_relative(&repo, path))
                .collect::<Result<Vec<_>>>()?;
            print_combined_diff_tree_stat(
                &repo,
                &store,
                &parent_indexes,
                &result_index,
                &pathspecs,
                CombinedStatRenderOptions {
                    relative_prefix: relative_prefix.as_deref(),
                    whitespace_mode: render_options.whitespace_mode,
                    ignore_matching_lines: &render_options.ignore_matching_lines,
                    ignore_blank_lines: render_options.ignore_blank_lines,
                    shortstat: false,
                },
            )?;
            if options.summary {
                print_combined_diff_tree_summary(
                    &parent_indexes,
                    &result_index,
                    &pathspecs,
                    relative_prefix.as_deref(),
                )?;
            }
            println!();
            return print_combined_diff_tree_patches(
                &store,
                &parent_indexes,
                &result_index,
                &pathspecs,
                CombinedPatchRenderOptions {
                    abbrev_len: render_options.patch_abbrev_len,
                    relative_prefix: relative_prefix.as_deref(),
                    old_prefix: &render_options.old_prefix,
                    new_prefix: &render_options.new_prefix,
                    dense_combined: options.dense_combined,
                    line_prefix: None,
                },
            );
        }
    }
    if (options.combined || options.dense_combined)
        && options.new_treeish.is_none()
        && combined_diff_tree_stat_mode(&options)
    {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        if commit.parents.len() > 1 {
            print_diff_tree_commit_id(&id, options.nul_terminated);
            let parent_indexes =
                combined_diff_tree_parent_indexes(&commit, &commit_cache, &tree_cache)?;
            let result_index = tree_cache
                .read_tree_to_index(&commit.tree)
                .map_err(CliError::Io)?;
            let pathspecs = options
                .paths
                .iter()
                .map(|path| path_arg_to_repo_relative(&repo, path))
                .collect::<Result<Vec<_>>>()?;
            return print_combined_diff_tree_stat(
                &repo,
                &store,
                &parent_indexes,
                &result_index,
                &pathspecs,
                CombinedStatRenderOptions {
                    relative_prefix: relative_prefix.as_deref(),
                    whitespace_mode: render_options.whitespace_mode,
                    ignore_matching_lines: &render_options.ignore_matching_lines,
                    ignore_blank_lines: render_options.ignore_blank_lines,
                    shortstat: options.shortstat,
                },
            );
        }
    }
    if (options.combined || options.dense_combined)
        && options.new_treeish.is_none()
        && combined_diff_tree_summary_mode(&options)
    {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        if commit.parents.len() > 1 {
            print_diff_tree_commit_id(&id, options.nul_terminated);
            let parent_indexes =
                combined_diff_tree_parent_indexes(&commit, &commit_cache, &tree_cache)?;
            let result_index = tree_cache
                .read_tree_to_index(&commit.tree)
                .map_err(CliError::Io)?;
            let pathspecs = options
                .paths
                .iter()
                .map(|path| path_arg_to_repo_relative(&repo, path))
                .collect::<Result<Vec<_>>>()?;
            return print_combined_diff_tree_summary(
                &parent_indexes,
                &result_index,
                &pathspecs,
                relative_prefix.as_deref(),
            );
        }
    }
    if (options.combined || options.dense_combined)
        && options.new_treeish.is_none()
        && combined_diff_tree_patch_mode(&options)
    {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        if commit.parents.len() > 1 {
            print_diff_tree_commit_id(&id, options.nul_terminated);
            if options.no_patch {
                return Ok(());
            }
            let parent_indexes =
                combined_diff_tree_parent_indexes(&commit, &commit_cache, &tree_cache)?;
            let result_index = tree_cache
                .read_tree_to_index(&commit.tree)
                .map_err(CliError::Io)?;
            let pathspecs = options
                .paths
                .iter()
                .map(|path| path_arg_to_repo_relative(&repo, path))
                .collect::<Result<Vec<_>>>()?;
            return print_combined_diff_tree_patches(
                &store,
                &parent_indexes,
                &result_index,
                &pathspecs,
                CombinedPatchRenderOptions {
                    abbrev_len: render_options.patch_abbrev_len,
                    relative_prefix: relative_prefix.as_deref(),
                    old_prefix: &render_options.old_prefix,
                    new_prefix: &render_options.new_prefix,
                    dense_combined: options.dense_combined,
                    line_prefix: None,
                },
            );
        }
    }
    if (options.combined || options.dense_combined)
        && options.new_treeish.is_none()
        && combined_diff_tree_raw_mode(&options)
    {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        if commit.parents.len() > 1 {
            print_diff_tree_commit_id(&id, options.nul_terminated);
            if options.no_patch {
                return Ok(());
            }
            let parent_indexes =
                combined_diff_tree_parent_indexes(&commit, &commit_cache, &tree_cache)?;
            let result_index = tree_cache
                .read_tree_to_index(&commit.tree)
                .map_err(CliError::Io)?;
            let pathspecs = options
                .paths
                .iter()
                .map(|path| path_arg_to_repo_relative(&repo, path))
                .collect::<Result<Vec<_>>>()?;
            return print_combined_diff_tree_raw_entries(
                &store,
                &parent_indexes,
                &result_index,
                &pathspecs,
                render_options.raw_abbrev_len,
                relative_prefix.as_deref(),
                options.nul_terminated,
            );
        }
    }
    if options.merge && options.new_treeish.is_none() {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        if commit.parents.len() > 1 {
            for parent in &commit.parents {
                print_diff_tree_commit_id(&id, options.nul_terminated);
                let mut parent_options = options.clone();
                parent_options.merge = false;
                parent_options.treeish = Some(parent.to_hex());
                parent_options.new_treeish = Some(id.to_hex());
                diff_tree(parent_options)?;
            }
            return Ok(());
        }
    }
    if options.new_treeish.is_none() {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        if commit_cache.read_commit(&id)?.parents.len() > 1 {
            return Ok(());
        }
    }
    let log_format = if options.pretty.is_some() || options.format.is_some() {
        Some(history_commands::LogFormat::parse(
            false,
            options.format.as_deref(),
            options.pretty.as_deref(),
        )?)
    } else {
        None
    };
    let log_notes = history_commands::LogNotes::load(
        &repo,
        &store,
        options.notes
            || options
                .format
                .as_deref()
                .is_some_and(|format| format.contains("%N")),
    )?;
    let recursive_tree = diff_tree_needs_recursive_entries(&options);
    let root_tree_diff = (!recursive_tree)
        .then(|| {
            diff_tree_root_entries(
                &repo,
                &store,
                &commit_cache,
                &tree_cache,
                old,
                options.new_treeish.as_deref(),
                options.reverse,
            )
        })
        .transpose()?;
    if let Some(entries) = root_tree_diff {
        if options.new_treeish.is_none() {
            let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
            let commit = commit_cache.read_commit(&id)?;
            if !options.root && commit.parents.is_empty() {
                return Ok(());
            }
            if let Some(format) = log_format.as_ref() {
                print_diff_tree_log_format(
                    format,
                    &store,
                    &id,
                    &commit,
                    &log_notes,
                    !options.no_patch,
                    options.patch_with_stat,
                )?;
            } else {
                print_diff_tree_commit_id(&id, options.nul_terminated);
            }
        }
        if options.no_patch {
            return Ok(());
        }
        return render_diff_tree_root_entries(
            &store,
            entries,
            RootTreeRenderOptions {
                diff_filter,
                order_file: options.order_file.as_deref(),
                skip_to: options.skip_to.as_deref(),
                rotate_to: options.rotate_to.as_deref(),
                relative_prefix: relative_prefix.as_deref(),
                options: &render_options,
            },
        );
    }
    let (old_index, new_index) = if let Some(new) = options.new_treeish.as_deref() {
        (
            read_treeish_index(&repo, &store, old)?,
            read_treeish_index(&repo, &store, new)?,
        )
    } else {
        let id = resolve_objectish(&repo, old).map_err(|_| ambiguous_revision_error(old))?;
        let commit = commit_cache.read_commit(&id)?;
        if !options.root && commit.parents.is_empty() {
            return Ok(());
        }
        if let Some(format) = log_format.as_ref() {
            print_diff_tree_log_format(
                format,
                &store,
                &id,
                &commit,
                &log_notes,
                !options.no_patch,
                options.patch_with_stat,
            )?;
        } else {
            print_diff_tree_commit_id(&id, options.nul_terminated);
        }
        let old_index = if let Some(parent) = commit.parents.first() {
            let parent_commit = commit_cache.read_commit(parent)?;
            tree_cache.read_tree_to_index(&parent_commit.tree)?
        } else {
            GitIndex::new()
        };
        let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
        (old_index, new_index)
    };
    let entries = filtered_diff_entries(
        &repo,
        if options.reverse {
            &new_index
        } else {
            &old_index
        },
        if options.reverse {
            &old_index
        } else {
            &new_index
        },
        &options.paths,
        detect_renames,
        detect_copies,
        options.find_copies_harder,
    )?;
    let compare_old_index = if options.reverse {
        &new_index
    } else {
        &old_index
    };
    let compare_new_index = if options.reverse {
        &old_index
    } else {
        &new_index
    };
    let diff_context = DiffIndexContext {
        repo: &repo,
        store: &store,
        old_index: compare_old_index,
        new_index: compare_new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let entries = apply_similarity_detection(
        &diff_context,
        entries,
        SimilarityDetectionOptions {
            rename_threshold: detect_renames,
            copy_threshold: detect_copies,
            find_copies_harder: options.find_copies_harder,
        },
    )?;
    let entries = filter_ignored_submodule_entries(
        entries,
        compare_old_index,
        compare_new_index,
        ignore_submodules,
    );
    let entries = apply_break_rewrites(&diff_context, entries, break_rewrites)?;
    let entries = apply_pickaxe_filter(
        &diff_context,
        entries,
        PickaxeOptions {
            string: options.pickaxe_string.as_deref(),
            regex: options.pickaxe_regex.as_deref(),
            regex_mode: options.pickaxe_regex_mode,
            all: options.pickaxe_all,
        },
    )?;
    let entries = apply_diff_filter(entries, diff_filter);
    let entries = apply_diff_order_file(entries, options.order_file.as_deref())?;
    let entries = apply_diff_skip_rotate(
        entries,
        options.skip_to.as_deref(),
        options.rotate_to.as_deref(),
    );
    let entries = filter_diff_relative(entries, relative_prefix.as_deref());
    let (old_index, new_index, render_options) = if options.reverse {
        let mut render_options = render_options;
        render_options.old_source = DiffSideSource::Index;
        render_options.new_source = DiffSideSource::Index;
        render_options.reverse_direction();
        (new_index, old_index, render_options)
    } else {
        let mut render_options = render_options;
        render_options.old_source = DiffSideSource::Index;
        render_options.new_source = DiffSideSource::Index;
        (old_index, new_index, render_options)
    };
    render_diff(
        &repo,
        &store,
        &old_index,
        &new_index,
        &entries,
        render_options,
    )
}

pub(crate) fn diff_pairs(options: DiffPairsOptions) -> Result<()> {
    let word_diff = parse_word_diff_option(options.word_diff.as_deref())?;
    let patch = !options.no_patch
        && (options.patch
            || ![
                options.stat,
                options.numstat,
                options.shortstat,
                options.raw,
                options.summary,
                options.name_status,
                options.name_only,
            ]
            .into_iter()
            .any(|selected| selected));
    let render_options = DiffRenderOptions {
        stat: options.stat,
        patch_with_raw: false,
        patch_with_stat: false,
        compact_summary: false,
        numstat: options.numstat,
        shortstat: options.shortstat,
        raw: options.raw,
        summary: options.summary,
        name_status: options.name_status,
        name_only: options.name_only,
        nul_terminated: options.nul_terminated,
        patch,
        no_patch: options.no_patch,
        binary: false,
        quiet: options.quiet,
        exit_code: false,
        raw_abbrev_len: Some(GitHashAlgorithm::Sha1.digest_len() * 2),
        word_diff,
        patch_abbrev_len: None,
        old_prefix: "a/".to_owned(),
        new_prefix: "b/".to_owned(),
        unified_context: 3,
        inter_hunk_context: 0,
        output_indicator_new: Some(b'+'),
        output_indicator_old: Some(b'-'),
        output_indicator_context: Some(b' '),
        whitespace_mode: DiffWhitespaceMode::None,
        ignore_blank_lines: false,
        relative_prefix: None,
        text: false,
        irreversible_delete: false,
        submodule_format: SubmoduleDiffFormat::Short,
        color_mode: DiffColorMode::Never,
        ignore_matching_lines: Vec::new(),
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
        line_prefix: None,
    };
    render_options.validate_format(true)?;

    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    for (batch_index, batch) in parse_diff_pairs_batches(&input, options.nul_terminated)?
        .into_iter()
        .enumerate()
    {
        if batch_index > 0 && !options.quiet && !options.no_patch {
            if options.nul_terminated {
                io::stdout().write_all(&[0])?;
            } else {
                io::stdout().write_all(b"\n")?;
            }
        }
        render_diff(
            &repo,
            &store,
            &batch.old_index,
            &batch.new_index,
            &batch.entries,
            DiffRenderOptions {
                stat: render_options.stat,
                patch_with_raw: render_options.patch_with_raw,
                patch_with_stat: render_options.patch_with_stat,
                compact_summary: render_options.compact_summary,
                numstat: render_options.numstat,
                shortstat: render_options.shortstat,
                raw: render_options.raw,
                summary: render_options.summary,
                name_status: render_options.name_status,
                name_only: render_options.name_only,
                nul_terminated: render_options.nul_terminated,
                patch: render_options.patch,
                no_patch: render_options.no_patch,
                binary: render_options.binary,
                quiet: render_options.quiet,
                exit_code: render_options.exit_code,
                raw_abbrev_len: render_options.raw_abbrev_len,
                word_diff: render_options.word_diff,
                patch_abbrev_len: render_options.patch_abbrev_len,
                old_prefix: render_options.old_prefix.clone(),
                new_prefix: render_options.new_prefix.clone(),
                unified_context: render_options.unified_context,
                inter_hunk_context: render_options.inter_hunk_context,
                output_indicator_new: render_options.output_indicator_new,
                output_indicator_old: render_options.output_indicator_old,
                output_indicator_context: render_options.output_indicator_context,
                ignore_matching_lines: render_options.ignore_matching_lines.clone(),
                ignore_blank_lines: render_options.ignore_blank_lines,
                whitespace_mode: render_options.whitespace_mode,
                relative_prefix: render_options.relative_prefix.clone(),
                text: render_options.text,
                irreversible_delete: render_options.irreversible_delete,
                submodule_format: render_options.submodule_format,
                color_mode: render_options.color_mode,
                old_source: render_options.old_source,
                new_source: render_options.new_source,
                line_prefix: render_options.line_prefix.clone(),
            },
        )?;
    }
    Ok(())
}

fn combined_diff_tree_raw_mode(options: &PlumbingDiffOptions) -> bool {
    (options.raw || !options.dense_combined)
        && !options.patch
        && !options.patch_with_raw
        && !options.patch_with_stat
        && !options.binary
        && !options.stat
        && !options.numstat
        && !options.shortstat
        && !options.summary
        && !options.name_status
        && !options.name_only
}

fn combined_diff_tree_patch_mode(options: &PlumbingDiffOptions) -> bool {
    (options.dense_combined
        || options.patch
        || options.binary
        || options.unified.is_some()
        || options.inter_hunk_context.is_some())
        && !options.raw
        && !options.patch_with_raw
        && !options.patch_with_stat
        && !options.stat
        && !options.numstat
        && !options.shortstat
        && !options.summary
        && !options.name_status
        && !options.name_only
}

fn combined_diff_tree_patch_with_stat_mode(options: &PlumbingDiffOptions) -> bool {
    options.patch_with_stat
        && !options.raw
        && !options.patch_with_raw
        && !options.numstat
        && !options.shortstat
        && !options.name_status
        && !options.name_only
}

fn combined_diff_tree_stat_mode(options: &PlumbingDiffOptions) -> bool {
    (options.stat || options.shortstat)
        && !options.raw
        && !options.patch
        && !options.patch_with_raw
        && !options.patch_with_stat
        && !options.numstat
        && !options.name_status
        && !options.name_only
}

fn combined_diff_tree_summary_mode(options: &PlumbingDiffOptions) -> bool {
    options.summary
        && !options.stat
        && !options.shortstat
        && !options.raw
        && !options.patch
        && !options.patch_with_raw
        && !options.patch_with_stat
        && !options.numstat
        && !options.name_status
        && !options.name_only
}

fn print_diff_tree_commit_id(id: &ObjectId, nul_terminated: bool) {
    if nul_terminated {
        print!("{}\0", id.to_hex());
    } else {
        println!("{}", id.to_hex());
    }
}

pub(crate) fn combined_diff_tree_parent_indexes(
    commit: &CommitObject,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
) -> Result<Vec<GitIndex>> {
    commit
        .parents
        .iter()
        .map(|parent| {
            let parent_commit = commit_cache.read_commit(parent)?;
            tree_cache
                .read_tree_to_index(&parent_commit.tree)
                .map_err(CliError::Io)
        })
        .collect()
}

pub(crate) fn print_combined_diff_tree_raw_entries(
    store: &LooseObjectStore,
    parent_indexes: &[GitIndex],
    result_index: &GitIndex,
    pathspecs: &[Vec<u8>],
    abbrev_len: Option<usize>,
    relative_prefix: Option<&[u8]>,
    nul_terminated: bool,
) -> Result<()> {
    let abbrev_len = abbrev_len.unwrap_or(default_abbrev_len(store)?);
    for path in combined_diff_tree_paths(parent_indexes, result_index, pathspecs) {
        let result_entry = find_index_entry(result_index, &path);
        let parent_entries = parent_indexes
            .iter()
            .map(|index| find_index_entry(index, &path))
            .collect::<Vec<_>>();
        let status = parent_entries
            .iter()
            .map(|entry| combined_diff_tree_status(*entry, result_entry))
            .collect::<String>();
        let parent_modes = parent_entries
            .iter()
            .map(|entry| {
                entry
                    .map(|entry| index_mode_octal(entry.mode))
                    .unwrap_or("000000")
            })
            .collect::<Vec<_>>()
            .join(" ");
        let result_mode = result_entry
            .map(|entry| index_mode_octal(entry.mode))
            .unwrap_or("000000");
        let parent_ids = parent_entries
            .iter()
            .map(|entry| {
                entry
                    .map(|entry| diff_raw_object_id_len(&entry.id, abbrev_len))
                    .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len))
            })
            .collect::<Vec<_>>()
            .join(" ");
        let result_id = result_entry
            .map(|entry| diff_raw_object_id_len(&entry.id, abbrev_len))
            .unwrap_or_else(|| diff_raw_zero_object_id_len(abbrev_len));
        let path = diff_display_path(&path, relative_prefix);
        if nul_terminated {
            print!(
                "{}{} {result_mode} {parent_ids} {result_id} {status}\0{path}\0",
                ":".repeat(parent_indexes.len()),
                parent_modes
            );
        } else {
            println!(
                "{}{} {result_mode} {parent_ids} {result_id} {status}\t{path}",
                ":".repeat(parent_indexes.len()),
                parent_modes
            );
        }
    }
    Ok(())
}

pub(crate) fn print_combined_diff_tree_summary(
    parent_indexes: &[GitIndex],
    result_index: &GitIndex,
    pathspecs: &[Vec<u8>],
    relative_prefix: Option<&[u8]>,
) -> Result<()> {
    let Some(first_parent_index) = parent_indexes.first() else {
        return Ok(());
    };
    let entries = diff_indexes(first_parent_index, result_index)?
        .into_iter()
        .filter(|entry| diff_entry_matches_pathspec(entry, pathspecs))
        .collect::<Vec<_>>();
    print_summary_entries(first_parent_index, result_index, &entries, relative_prefix)
}

pub(crate) struct CombinedStatRenderOptions<'a> {
    pub(crate) relative_prefix: Option<&'a [u8]>,
    pub(crate) whitespace_mode: DiffWhitespaceMode,
    pub(crate) ignore_matching_lines: &'a [Regex],
    pub(crate) ignore_blank_lines: bool,
    pub(crate) shortstat: bool,
}

pub(crate) fn print_combined_diff_tree_stat(
    repo: &GitRepo,
    store: &LooseObjectStore,
    parent_indexes: &[GitIndex],
    result_index: &GitIndex,
    pathspecs: &[Vec<u8>],
    options: CombinedStatRenderOptions<'_>,
) -> Result<()> {
    let Some(first_parent_index) = parent_indexes.first() else {
        return Ok(());
    };
    let combined_paths = combined_diff_tree_paths(parent_indexes, result_index, pathspecs)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let entries = diff_indexes(first_parent_index, result_index)?
        .into_iter()
        .filter(|entry| combined_paths.contains(&entry.path))
        .collect::<Vec<_>>();
    let context = DiffIndexContext {
        repo,
        store,
        old_index: first_parent_index,
        new_index: result_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let stat_options = DiffStatOptions {
        whitespace_mode: options.whitespace_mode,
        relative_prefix: options.relative_prefix,
        ignore_matching_lines: options.ignore_matching_lines,
        ignore_blank_lines: options.ignore_blank_lines,
        compact_summary: false,
    };
    if options.shortstat {
        print_shortstat_entries(&context, &entries, stat_options)
    } else {
        print_stat_entries(&context, &entries, stat_options)
    }
}

pub(crate) struct CombinedPatchRenderOptions<'a> {
    pub(crate) abbrev_len: Option<usize>,
    pub(crate) relative_prefix: Option<&'a [u8]>,
    pub(crate) old_prefix: &'a str,
    pub(crate) new_prefix: &'a str,
    pub(crate) dense_combined: bool,
    pub(crate) line_prefix: Option<&'a str>,
}

pub(crate) fn print_combined_diff_tree_patches(
    store: &LooseObjectStore,
    parent_indexes: &[GitIndex],
    result_index: &GitIndex,
    pathspecs: &[Vec<u8>],
    options: CombinedPatchRenderOptions<'_>,
) -> Result<()> {
    let abbrev_len = options.abbrev_len.unwrap_or(default_abbrev_len(store)?);
    for path in combined_diff_tree_paths(parent_indexes, result_index, pathspecs) {
        let Some(result_entry) = find_index_entry(result_index, &path) else {
            continue;
        };
        if result_entry.mode == IndexMode::Gitlink {
            continue;
        }
        let parent_entries = parent_indexes
            .iter()
            .map(|index| find_index_entry(index, &path))
            .collect::<Vec<_>>();
        if parent_entries
            .iter()
            .any(|entry| entry.is_none_or(|entry| entry.mode == IndexMode::Gitlink))
        {
            continue;
        }
        let parent_entries = parent_entries
            .into_iter()
            .map(|entry| entry.expect("gitlink/missing parent entries skipped"))
            .collect::<Vec<_>>();
        let result_content = read_index_entry_content(store, result_entry)?;
        let parent_contents = parent_entries
            .iter()
            .map(|entry| read_index_entry_content(store, entry))
            .collect::<Result<Vec<_>>>()?;
        let result_lines = split_combined_patch_lines(&result_content);
        let parent_lines = parent_contents
            .iter()
            .map(|content| split_combined_patch_lines(content))
            .collect::<Vec<_>>();
        let display_path = diff_display_path(&path, options.relative_prefix);
        let old_path = format!("{}{}", options.old_prefix, display_path);
        let new_path = format!("{}{}", options.new_prefix, display_path);
        let line_prefix = options.line_prefix.unwrap_or("");
        if options.dense_combined {
            println!("{line_prefix}diff --cc {display_path}");
        } else {
            println!("{line_prefix}diff --combined {display_path}");
        }
        let parent_ids = parent_entries
            .iter()
            .map(|entry| short_object_id_len(&entry.id, abbrev_len))
            .collect::<Vec<_>>()
            .join(",");
        let result_id = short_object_id_len(&result_entry.id, abbrev_len);
        println!("{line_prefix}index {parent_ids}..{result_id}");
        println!("{line_prefix}--- {old_path}");
        println!("{line_prefix}+++ {new_path}");
        let parent_ranges = parent_lines
            .iter()
            .map(|lines| format!("-1,{}", lines.len()))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "{line_prefix}@@@ {parent_ranges} +1,{} @@@",
            result_lines.len()
        );
        print_combined_patch_hunk(&parent_lines, &result_lines, line_prefix);
    }
    Ok(())
}

fn split_combined_patch_lines(content: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(content)
        .split_terminator('\n')
        .map(str::to_owned)
        .collect()
}

fn print_combined_patch_hunk(
    parent_lines: &[Vec<String>],
    result_lines: &[String],
    line_prefix: &str,
) {
    let mut parent_positions = vec![0usize; parent_lines.len()];
    for (result_index, result_line) in result_lines.iter().enumerate() {
        for parent_index in 0..parent_lines.len() {
            while parent_positions[parent_index] < parent_lines[parent_index].len()
                && parent_lines[parent_index][parent_positions[parent_index]] != *result_line
                && !result_lines[result_index..]
                    .iter()
                    .any(|line| *line == parent_lines[parent_index][parent_positions[parent_index]])
            {
                print_combined_patch_deleted_line(
                    parent_lines,
                    parent_index,
                    parent_positions[parent_index],
                    line_prefix,
                );
                parent_positions[parent_index] += 1;
            }
        }
        let mut prefix = String::with_capacity(parent_lines.len());
        for parent_index in 0..parent_lines.len() {
            if parent_positions[parent_index] < parent_lines[parent_index].len()
                && parent_lines[parent_index][parent_positions[parent_index]] == *result_line
            {
                prefix.push(' ');
                parent_positions[parent_index] += 1;
            } else {
                prefix.push('+');
            }
        }
        println!("{line_prefix}{prefix}{result_line}");
    }
    for parent_index in 0..parent_lines.len() {
        while parent_positions[parent_index] < parent_lines[parent_index].len() {
            print_combined_patch_deleted_line(
                parent_lines,
                parent_index,
                parent_positions[parent_index],
                line_prefix,
            );
            parent_positions[parent_index] += 1;
        }
    }
}

fn print_combined_patch_deleted_line(
    parent_lines: &[Vec<String>],
    parent_index: usize,
    line_index: usize,
    line_prefix: &str,
) {
    let mut prefix = String::with_capacity(parent_lines.len());
    for index in 0..parent_lines.len() {
        if index == parent_index {
            prefix.push('-');
        } else {
            prefix.push(' ');
        }
    }
    println!(
        "{line_prefix}{prefix}{}",
        parent_lines[parent_index][line_index]
    );
}

fn combined_diff_tree_paths(
    parent_indexes: &[GitIndex],
    result_index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Vec<Vec<u8>> {
    let mut paths = BTreeSet::new();
    for entry in result_index.entries() {
        if entry.stage == 0 {
            paths.insert(entry.path.clone());
        }
    }
    for index in parent_indexes {
        for entry in index.entries() {
            if entry.stage == 0 {
                paths.insert(entry.path.clone());
            }
        }
    }
    paths
        .into_iter()
        .filter(|path| pathspec_matches(path, pathspecs))
        .filter(|path| {
            let result_entry = find_index_entry(result_index, path);
            parent_indexes.iter().all(|parent_index| {
                !combined_diff_tree_entries_equal(
                    find_index_entry(parent_index, path),
                    result_entry,
                )
            })
        })
        .collect()
}

fn combined_diff_tree_entries_equal(
    parent_entry: Option<&IndexEntry>,
    result_entry: Option<&IndexEntry>,
) -> bool {
    match (parent_entry, result_entry) {
        (Some(parent), Some(result)) => parent.mode == result.mode && parent.id == result.id,
        (None, None) => true,
        _ => false,
    }
}

struct DiffTreeStdinOptions<'a> {
    detect_renames: Option<u8>,
    detect_copies: Option<u8>,
    break_rewrites: Option<u8>,
    ignore_submodules: IgnoreSubmodulesMode,
    diff_filter: DiffFilter,
    relative_prefix: Option<&'a [u8]>,
}

fn diff_tree_stdin(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    options: &PlumbingDiffOptions,
    render_options: &DiffRenderOptions,
    stdin_options: DiffTreeStdinOptions<'_>,
) -> Result<()> {
    let log_format = if options.pretty.is_some() || options.format.is_some() {
        Some(history_commands::LogFormat::parse(
            false,
            options.format.as_deref(),
            options.pretty.as_deref(),
        )?)
    } else {
        None
    };
    let log_notes = history_commands::LogNotes::load(
        repo,
        store,
        options.notes
            || options
                .format
                .as_deref()
                .is_some_and(|format| format.contains("%N")),
    )?;
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    for token in input.split_whitespace() {
        let id = resolve_objectish(repo, token).map_err(|_| ambiguous_revision_error(token))?;
        let commit = commit_cache.read_commit(&id)?;
        if !options.root && commit.parents.is_empty() {
            continue;
        }
        if commit.parents.len() > 1
            && !(options.merge || options.combined || options.dense_combined)
        {
            continue;
        }
        if let Some(format) = log_format.as_ref() {
            print_diff_tree_log_format(
                format,
                store,
                &id,
                &commit,
                &log_notes,
                !options.no_patch,
                false,
            )?;
        } else {
            print_diff_tree_commit_id(&id, options.nul_terminated);
        }
        if options.no_patch {
            continue;
        }
        let Some(parent) = commit.parents.first() else {
            continue;
        };
        let parent_commit = commit_cache.read_commit(parent)?;
        let old_index = tree_cache.read_tree_to_index(&parent_commit.tree)?;
        let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
        let entries = filtered_diff_entries(
            repo,
            if options.reverse {
                &new_index
            } else {
                &old_index
            },
            if options.reverse {
                &old_index
            } else {
                &new_index
            },
            &options.paths,
            stdin_options.detect_renames,
            stdin_options.detect_copies,
            options.find_copies_harder,
        )?;
        let compare_old_index = if options.reverse {
            &new_index
        } else {
            &old_index
        };
        let compare_new_index = if options.reverse {
            &old_index
        } else {
            &new_index
        };
        let diff_context = DiffIndexContext {
            repo,
            store,
            old_index: compare_old_index,
            new_index: compare_new_index,
            old_source: DiffSideSource::Index,
            new_source: DiffSideSource::Index,
        };
        let entries = apply_similarity_detection(
            &diff_context,
            entries,
            SimilarityDetectionOptions {
                rename_threshold: stdin_options.detect_renames,
                copy_threshold: stdin_options.detect_copies,
                find_copies_harder: options.find_copies_harder,
            },
        )?;
        let entries = filter_ignored_submodule_entries(
            entries,
            compare_old_index,
            compare_new_index,
            stdin_options.ignore_submodules,
        );
        let entries = apply_break_rewrites(&diff_context, entries, stdin_options.break_rewrites)?;
        let entries = apply_pickaxe_filter(
            &diff_context,
            entries,
            PickaxeOptions {
                string: options.pickaxe_string.as_deref(),
                regex: options.pickaxe_regex.as_deref(),
                regex_mode: options.pickaxe_regex_mode,
                all: options.pickaxe_all,
            },
        )?;
        let entries = apply_diff_filter(entries, stdin_options.diff_filter);
        let entries = apply_diff_order_file(entries, options.order_file.as_deref())?;
        let entries = apply_diff_skip_rotate(
            entries,
            options.skip_to.as_deref(),
            options.rotate_to.as_deref(),
        );
        let entries = filter_diff_relative(entries, stdin_options.relative_prefix);
        let (old_index, new_index, mut render_options) = if options.reverse {
            let mut render_options = render_options.clone();
            render_options.old_source = DiffSideSource::Index;
            render_options.new_source = DiffSideSource::Index;
            render_options.reverse_direction();
            (new_index, old_index, render_options)
        } else {
            let mut render_options = render_options.clone();
            render_options.old_source = DiffSideSource::Index;
            render_options.new_source = DiffSideSource::Index;
            (old_index, new_index, render_options)
        };
        render_options.relative_prefix =
            stdin_options.relative_prefix.map(|prefix| prefix.to_vec());
        render_diff(
            repo,
            store,
            &old_index,
            &new_index,
            &entries,
            render_options,
        )?;
    }
    Ok(())
}

fn combined_diff_tree_status(
    parent_entry: Option<&IndexEntry>,
    result_entry: Option<&IndexEntry>,
) -> char {
    match (parent_entry, result_entry) {
        (None, Some(_)) => 'A',
        (Some(_), None) => 'D',
        (Some(parent), Some(result)) if parent.mode != result.mode => 'T',
        (Some(_), Some(_)) => 'M',
        (None, None) => ' ',
    }
}

fn print_diff_tree_log_format(
    format: &history_commands::LogFormat<'_>,
    store: &LooseObjectStore,
    id: &ObjectId,
    commit: &zmin_git_core::CommitObject,
    notes: &history_commands::LogNotes,
    emit_patch_separator: bool,
    stat_patch_separator: bool,
) -> Result<()> {
    let rendered = format.render_with_context(
        id,
        commit,
        false,
        default_abbrev_len(store)?,
        &history_commands::LogDecorations::empty(),
        notes,
    )?;
    print!("{rendered}");
    if format.terminates_lines() {
        println!();
    }
    if emit_patch_separator && format.separates_patch() {
        if stat_patch_separator {
            println!("---");
        } else {
            println!();
        }
    }
    Ok(())
}

pub(crate) fn difftool(
    cached: bool,
    tool: Option<&str>,
    extcmd: Option<&str>,
    paths: Vec<PathBuf>,
) -> Result<()> {
    let repo = find_repo()?;
    let command = resolve_difftool_command(&repo, tool, extcmd)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let diff_input = parse_diff_input(&repo, &store, &index, cached, paths)?;
    let pathspecs = diff_input
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let entries = diff_input
        .precomputed_entries
        .unwrap_or(diff_indexes(&diff_input.old_index, &diff_input.new_index)?)
        .into_iter()
        .filter(|entry| pathspec_matches(&entry.path, &pathspecs))
        .collect::<Vec<_>>();
    let temp_root = create_difftool_temp_root()?;
    let context = DifftoolRunContext {
        repo: &repo,
        store: &store,
        old_index: &diff_input.old_index,
        new_index: &diff_input.new_index,
        new_side_from_index: diff_input.new_side_from_index,
        temp_root: &temp_root,
        command: &command,
    };
    let result = run_difftool_entries(&context, &entries);
    let cleanup = fs::remove_dir_all(&temp_root);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) if error.kind() != io::ErrorKind::NotFound => Err(CliError::Io(error)),
        (Ok(()), _) => Ok(()),
    }
}

struct DifftoolRunContext<'a> {
    repo: &'a GitRepo,
    store: &'a LooseObjectStore,
    old_index: &'a GitIndex,
    new_index: &'a GitIndex,
    new_side_from_index: bool,
    temp_root: &'a std::path::Path,
    command: &'a DifftoolCommand,
}

#[derive(Debug, Clone)]
struct DifftoolCommand {
    command: String,
    append_paths: bool,
}

fn resolve_difftool_command(
    repo: &GitRepo,
    tool: Option<&str>,
    extcmd: Option<&str>,
) -> Result<DifftoolCommand> {
    if let Some(extcmd) = extcmd {
        return Ok(DifftoolCommand {
            command: extcmd.to_owned(),
            append_paths: true,
        });
    }
    let tool = match tool {
        Some(tool) => tool.to_owned(),
        None => read_config_value(repo, "diff.tool")?.ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "no difftool configured; set diff.tool or pass --tool/--extcmd".into(),
        })?,
    };
    let command = read_config_value(repo, &format!("difftool.{tool}.cmd"))?.ok_or_else(|| {
        CliError::Fatal {
            code: 1,
            message: format!("diff tool '{tool}' is not configured"),
        }
    })?;
    Ok(DifftoolCommand {
        command,
        append_paths: false,
    })
}

fn run_difftool_entries(
    context: &DifftoolRunContext<'_>,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<()> {
    for entry in entries {
        let old_entry = find_index_entry(context.old_index, &entry.path);
        let new_entry = find_index_entry(context.new_index, &entry.path);
        let local = match old_entry {
            Some(entry) => write_difftool_temp_file(
                context.temp_root,
                "old",
                &entry.path,
                &read_index_entry_content(context.store, entry)?,
            )?,
            None => null_device_path(),
        };
        let remote = match new_entry {
            Some(entry) if context.new_side_from_index => write_difftool_temp_file(
                context.temp_root,
                "new",
                &entry.path,
                &read_index_entry_content(context.store, entry)?,
            )?,
            Some(entry) => {
                let relative = String::from_utf8_lossy(&entry.path);
                let worktree_path = context.repo.root.join(relative.as_ref());
                if path_exists(&worktree_path) {
                    PathBuf::from(relative.as_ref())
                } else {
                    write_difftool_temp_file(
                        context.temp_root,
                        "new",
                        &entry.path,
                        &read_index_entry_content(context.store, entry)?,
                    )?
                }
            }
            None => null_device_path(),
        };
        run_difftool_command(context.repo, context.command, &local, &remote)?;
    }
    Ok(())
}

pub(crate) fn create_difftool_temp_root() -> Result<PathBuf> {
    let unique = format!(
        "zmin-difftool-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub(crate) fn write_difftool_temp_file(
    temp_root: &std::path::Path,
    side: &str,
    relative: &[u8],
    content: &[u8],
) -> Result<PathBuf> {
    let path = temp_root
        .join(side)
        .join(String::from_utf8_lossy(relative).as_ref());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    Ok(path)
}

fn run_difftool_command(
    repo: &GitRepo,
    command: &DifftoolCommand,
    local: &std::path::Path,
    remote: &std::path::Path,
) -> Result<()> {
    let mut process = difftool_shell(command, local, remote);
    let status = process
        .current_dir(&repo.root)
        .status()
        .map_err(CliError::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Exit(status.code().unwrap_or(1)))
    }
}

#[cfg(not(windows))]
fn difftool_shell(
    command: &DifftoolCommand,
    local: &std::path::Path,
    remote: &std::path::Path,
) -> ProcessCommand {
    let command_line = if command.append_paths {
        format!(
            "{} {} {}",
            command.command,
            shell_quote_path(local),
            shell_quote_path(remote)
        )
    } else {
        command.command.clone()
    };
    let mut process = ProcessCommand::new("sh");
    process
        .arg("-c")
        .arg(command_line)
        .env("LOCAL", local)
        .env("REMOTE", remote);
    process
}

#[cfg(windows)]
fn difftool_shell(
    command: &DifftoolCommand,
    local: &std::path::Path,
    remote: &std::path::Path,
) -> ProcessCommand {
    let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| std::ffi::OsString::from("cmd.exe"));
    let command_line = if command.append_paths {
        format!(
            "{} {} {}",
            command.command,
            cmd_quote_path(local),
            cmd_quote_path(remote)
        )
    } else {
        command.command.clone()
    };
    let mut process = ProcessCommand::new(shell);
    process
        .arg("/C")
        .arg(command_line)
        .env("LOCAL", local)
        .env("REMOTE", remote);
    process
}

#[cfg(not(windows))]
pub(crate) fn shell_quote_path(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

#[cfg(windows)]
pub(crate) fn cmd_quote_path(path: &std::path::Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('"', "\"\""))
}

#[cfg(not(windows))]
pub(crate) fn null_device_path() -> PathBuf {
    PathBuf::from("/dev/null")
}

#[cfg(windows)]
pub(crate) fn null_device_path() -> PathBuf {
    PathBuf::from("NUL")
}
