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
    let (old_prefix, new_prefix) = diff_prefixes(
        options.no_prefix,
        options.default_prefix,
        options.src_prefix,
        options.dst_prefix,
    );
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
    );
    let repo = find_repo()?;
    let relative_prefix =
        diff_relative_prefix(&repo, options.relative.as_deref(), options.no_relative)?;
    let render_options = DiffRenderOptions {
        stat: options.stat,
        patch_with_raw: options.patch_with_raw,
        patch_with_stat: options.patch_with_stat,
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
        whitespace_mode,
        relative_prefix,
        text: options.text,
        irreversible_delete: options.irreversible_delete,
        submodule_format,
        color_mode,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    render_options.validate_format(false)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let diff_input = parse_diff_input(&repo, &store, &index, options.cached, options.paths)?;
    let (old_source, new_source) = diff_side_sources(diff_input.new_side_from_index);
    let pathspecs = diff_input
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let (old_index, new_index, old_source, new_source, mut render_options) = if options.reverse {
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
    let entries = diff_entries_for_indexes(
        &old_index,
        &new_index,
        detect_renames,
        detect_copies,
        find_copies_harder,
    )?;
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
    let new_index = worktree_index_snapshot(&repo, &index)?;
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
    let entries = filtered_diff_entries(
        &repo,
        &old_index,
        &new_index,
        &options.paths,
        detect_renames,
        detect_copies,
        options.find_copies_harder,
    )?;
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
        worktree_index_snapshot(&repo, &index)?
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
    let entries = filtered_diff_entries(
        &repo,
        &old_index,
        &new_index,
        &options.paths,
        detect_renames,
        detect_copies,
        options.find_copies_harder,
    )?;
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
    let old = options.treeish.as_deref().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "diff-tree requires a tree-ish".into(),
    })?;
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
            if options.nul_terminated {
                print!("{}\0", id.to_hex());
            } else {
                println!("{}", id.to_hex());
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
        if options.nul_terminated {
            print!("{}\0", id.to_hex());
        } else {
            println!("{}", id.to_hex());
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
        relative_prefix: None,
        text: false,
        irreversible_delete: false,
        submodule_format: SubmoduleDiffFormat::Short,
        color_mode: DiffColorMode::Never,
        ignore_matching_lines: Vec::new(),
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
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
                whitespace_mode: render_options.whitespace_mode,
                relative_prefix: render_options.relative_prefix.clone(),
                text: render_options.text,
                irreversible_delete: render_options.irreversible_delete,
                submodule_format: render_options.submodule_format,
                color_mode: render_options.color_mode,
                old_source: render_options.old_source,
                new_source: render_options.new_source,
            },
        )?;
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
    let entries = diff_indexes(&diff_input.old_index, &diff_input.new_index)?
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
    entries: &[skron_git_core::IndexDiffEntry],
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
        "skron-git-difftool-{}-{}",
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
