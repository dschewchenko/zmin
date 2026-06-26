use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Replay {
            contained,
            advance,
            onto,
            revision_ranges,
        } => run_replay(contained, advance, onto, revision_ranges),
        runtime::Command::History { command } => run_history(command),
        runtime::Command::RangeDiff {
            no_dual_color,
            no_no_dual_color,
            ranges,
        } => run_range_diff(no_dual_color > 0, no_no_dual_color, ranges),
        runtime::Command::FilterBranch {
            force,
            prune_empty,
            msg_filter,
            tree_filter,
            index_filter,
            env_filter,
            parent_filter,
            commit_filter,
            tag_name_filter,
            subdirectory_filter,
            original,
            temp_dir,
            setup,
            state_branch,
            revs,
        } => super::history_commands::filter_branch(super::history_commands::FilterBranchOptions {
            force,
            prune_empty,
            msg_filter,
            tree_filter,
            index_filter,
            env_filter,
            parent_filter,
            commit_filter,
            tag_name_filter,
            subdirectory_filter,
            original,
            temp_dir,
            setup,
            state_branch,
            revs,
        }),
        runtime::Command::Shortlog {
            committer,
            numbered,
            summary,
            email,
            no_merges,
            format,
            date,
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
            extended_regexp,
            fixed_strings,
            perl_regexp,
            revs,
        } => super::history_commands::shortlog(super::history_commands::ShortlogOptions {
            committer,
            numbered,
            summary,
            email,
            no_merges,
            format: format.last().map(String::as_str),
            date: date.last().map(String::as_str),
            group,
            wrap: wrap.last().map(String::as_str),
            stdin,
            reflog,
            walk_reflogs,
            grep_reflog,
            grep,
            invert_grep,
            all_match,
            regexp_ignore_case,
            extended_regexp,
            fixed_strings,
            perl_regexp,
            revs,
        }),
        runtime::Command::Blame {
            help,
            long,
            root,
            contents,
            encoding,
            first_parent,
            ignore_rev,
            ignore_revs_file,
            reverse,
            revs_file,
            args,
        } => super::history_commands::blame(
            long,
            root,
            false,
            serialize_blame_args(
                help,
                contents,
                encoding,
                first_parent,
                ignore_rev,
                ignore_revs_file,
                reverse,
                revs_file,
                args,
            ),
        ),
        runtime::Command::Annotate {
            help,
            long,
            porcelain,
            incremental,
            line_porcelain,
            contents,
            date,
            encoding,
            first_parent,
            ignore_rev,
            ignore_revs_file,
            progress,
            no_progress,
            color_lines,
            color_by_age,
            reverse,
            root,
            show_stats,
            copies,
            line_ranges,
            moves,
            revs_file,
            blank_boundary,
            raw_timestamp,
            args,
        } => super::history_commands::blame(
            long,
            true,
            true,
            serialize_annotate_args(
                help,
                porcelain,
                incremental,
                line_porcelain,
                contents,
                date,
                encoding,
                first_parent,
                ignore_rev,
                ignore_revs_file,
                progress,
                no_progress,
                color_lines,
                color_by_age,
                reverse,
                root,
                show_stats,
                copies,
                line_ranges,
                moves,
                revs_file,
                blank_boundary,
                raw_timestamp,
                args,
            ),
        ),
        runtime::Command::ShowBranch {
            all,
            remotes,
            current,
            topo_order,
            date_order,
            sparse,
            color,
            no_color,
            more,
            list,
            independent,
            merge_base,
            sha1_name,
            no_name,
            topics,
            reflog,
            revs,
        } => super::history_commands::show_branch(super::history_commands::ShowBranchOptions {
            all,
            remotes,
            current,
            topo_order,
            date_order,
            sparse,
            color,
            no_color,
            more,
            list,
            independent,
            merge_base,
            sha1_name,
            no_name,
            topics,
            reflog,
            revs,
        }),
        runtime::Command::Cherry {
            verbose,
            no_verbose,
            abbrev,
            upstream,
            head,
            limit,
        } => super::history_commands::cherry(
            verbose > 0 && !no_verbose,
            abbrev,
            upstream.as_deref(),
            head.as_deref(),
            limit.as_deref(),
        ),
        runtime::Command::RequestPull {
            patch,
            start,
            url,
            end,
        } => super::history_commands::request_pull(patch > 0, &start, &url, end.as_deref()),
        runtime::Command::Describe {
            all,
            tags,
            contains,
            long,
            abbrev,
            exact_match,
            always,
            dirty,
            broken,
            candidates,
            debug,
            first_parent,
            matches,
            excludes,
            commits,
        } => super::history_commands::describe(super::history_commands::DescribeOptions {
            all,
            tags,
            contains,
            long,
            abbrev,
            exact_match,
            always,
            dirty,
            broken,
            candidates,
            debug,
            first_parent,
            matches,
            excludes,
            commits,
        }),
        runtime::Command::NameRev {
            name_only,
            tags,
            refs,
            excludes,
            all,
            annotate_stdin,
            undefined: _undefined,
            no_undefined,
            always,
            commits,
        } => super::history_commands::name_rev(super::history_commands::NameRevOptions {
            name_only,
            tags,
            refs,
            excludes,
            all,
            annotate_stdin,
            no_undefined,
            always,
            commits,
        }),
        runtime::Command::Reflog { command, args } => {
            super::history_commands::reflog(serialize_reflog_args(command, args))
        }
        runtime::Command::Log {
            oneline,
            zero,
            all,
            author,
            committer,
            count,
            max_parents,
            no_max_parents,
            merges,
            min_parents,
            no_min_parents,
            no_merges,
            parents,
            first_parent,
            no_diff_merges,
            diff_merges,
            separate_merges,
            dd,
            reverse,
            full_history,
            ancestry_path,
            dense,
            sparse,
            show_pulls,
            simplify_merges,
            simplify_by_decoration,
            topo_order,
            date_order,
            author_date_order,
            left_right,
            cherry_pick,
            cherry_mark,
            boundary,
            root,
            patch,
            patch_with_stat,
            combined,
            dense_combined,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            encoding,
            expand_tabs,
            no_expand_tabs,
            notes,
            no_notes,
            decorate,
            clear_decorations,
            abbrev_commit,
            no_abbrev_commit,
            pickaxe_string,
            pickaxe_regex,
            pickaxe_regex_mode,
            pickaxe_all,
            ignore_matching_lines,
            walk_reflogs,
            no_walk,
            grep_reflog,
            grep,
            invert_grep,
            all_match,
            regexp_ignore_case,
            basic_regexp,
            extended_regexp,
            fixed_strings,
            perl_regexp,
            format,
            max_count,
            since,
            until,
            date,
            pretty,
            revs,
        } => super::history_commands::log(super::history_commands::LogOptions {
            oneline,
            zero,
            all,
            author: author.as_deref(),
            committer: committer.as_deref(),
            count,
            max_parents: max_parents.as_deref(),
            no_max_parents,
            merges,
            min_parents: min_parents.as_deref(),
            no_min_parents,
            no_merges,
            parents,
            first_parent,
            no_diff_merges,
            diff_merges: diff_merges.as_deref(),
            separate_merges,
            dd,
            reverse,
            full_history,
            ancestry_path,
            dense,
            sparse,
            show_pulls,
            simplify_merges,
            simplify_by_decoration,
            topo_order,
            date_order,
            author_date_order,
            left_right,
            cherry_pick,
            cherry_mark,
            boundary,
            root,
            patch,
            patch_with_stat,
            combined,
            dense_combined,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            encoding: encoding.as_deref(),
            expand_tabs,
            no_expand_tabs,
            notes,
            no_notes,
            diff_required: false,
            decorate: decorate.as_deref(),
            clear_decorations,
            abbrev_commit,
            no_abbrev_commit,
            pickaxe_string: pickaxe_string.as_deref(),
            pickaxe_regex: pickaxe_regex.as_deref(),
            pickaxe_regex_mode,
            pickaxe_all,
            ignore_matching_lines,
            walk_reflogs,
            no_walk,
            grep_reflog,
            grep,
            invert_grep,
            all_match,
            regexp_ignore_case,
            basic_regexp,
            extended_regexp,
            fixed_strings,
            perl_regexp,
            format: format.as_deref(),
            max_count: max_count.as_deref(),
            since: since.as_deref(),
            until: until.as_deref(),
            date: date.as_deref(),
            pretty: pretty.as_deref(),
            revs,
        }),
        runtime::Command::Whatchanged {
            oneline,
            all,
            parents,
            reverse,
            patch,
            patch_with_stat,
            root,
            combined,
            dense_combined,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            pickaxe_string,
            pickaxe_regex,
            pickaxe_regex_mode,
            pickaxe_all,
            format,
            max_count,
            since,
            date,
            pretty,
            i_still_use_this: _,
            revs,
        } => super::history_commands::log(super::history_commands::LogOptions {
            oneline,
            zero: false,
            all,
            author: None,
            committer: None,
            count: false,
            max_parents: None,
            no_max_parents: false,
            merges: false,
            min_parents: None,
            no_min_parents: false,
            no_merges: false,
            parents,
            first_parent: false,
            no_diff_merges: false,
            diff_merges: None,
            separate_merges: false,
            dd: false,
            reverse,
            full_history: false,
            ancestry_path: false,
            dense: false,
            sparse: false,
            show_pulls: false,
            simplify_merges: false,
            simplify_by_decoration: false,
            topo_order: false,
            date_order: false,
            author_date_order: false,
            left_right: false,
            cherry_pick: false,
            cherry_mark: false,
            boundary: false,
            root,
            patch: patch || combined || dense_combined,
            patch_with_stat,
            combined,
            dense_combined,
            stat,
            numstat,
            shortstat,
            raw: raw
                || !(patch
                    || patch_with_stat
                    || combined
                    || dense_combined
                    || stat
                    || numstat
                    || shortstat
                    || summary
                    || name_only
                    || name_status),
            summary,
            name_only,
            name_status,
            encoding: None,
            expand_tabs: false,
            no_expand_tabs: false,
            diff_required: true,
            pickaxe_string: pickaxe_string.as_deref(),
            pickaxe_regex: pickaxe_regex.as_deref(),
            pickaxe_regex_mode,
            pickaxe_all,
            decorate: None,
            clear_decorations: false,
            ignore_matching_lines: Vec::new(),
            walk_reflogs: false,
            no_walk: false,
            grep_reflog: Vec::new(),
            grep: Vec::new(),
            invert_grep: false,
            all_match: false,
            regexp_ignore_case: false,
            basic_regexp: false,
            extended_regexp: false,
            fixed_strings: false,
            perl_regexp: false,
            notes: false,
            no_notes: false,
            format: format.as_deref(),
            max_count: max_count.as_deref(),
            since: since.as_deref(),
            until: None,
            date: date.as_deref(),
            pretty: pretty.as_deref(),
            abbrev_commit: false,
            no_abbrev_commit: false,
            revs,
        }),
        runtime::Command::Show {
            no_patch,
            oneline,
            zero,
            stat,
            patch_with_raw,
            patch_with_stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
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
            root,
            combined,
            separate_merges,
            first_parent,
            format,
            pretty,
            args,
        } => super::history_commands::show(super::history_commands::ShowOptions {
            no_patch,
            oneline,
            zero,
            stat,
            patch_with_raw,
            patch_with_stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            encoding: encoding.as_deref(),
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
            root,
            combined,
            separate_merges,
            first_parent,
            format: format.as_deref(),
            pretty: pretty.as_deref(),
            args,
        }),
        runtime::Command::RevList {
            oneline,
            all,
            author,
            committer,
            encoding,
            expand_tabs,
            no_expand_tabs,
            notes,
            no_notes,
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
            count,
            max_parents,
            no_max_parents,
            merges,
            min_parents,
            no_min_parents,
            no_merges,
            objects,
            no_object_names,
            filter,
            filter_provided_objects,
            parents,
            first_parent,
            children,
            walk_reflogs,
            grep_reflog,
            reverse,
            full_history,
            ancestry_path,
            dense,
            sparse,
            show_pulls,
            simplify_merges,
            simplify_by_decoration,
            topo_order,
            date_order,
            author_date_order,
            left_right,
            cherry_pick,
            cherry_mark,
            boundary,
            max_count,
            since,
            until,
            date,
            format,
            pretty,
            revs,
        } => super::history_commands::rev_list(super::history_commands::RevListOptions {
            oneline,
            all,
            author: author.as_deref(),
            committer: committer.as_deref(),
            encoding: encoding.as_deref(),
            expand_tabs,
            no_expand_tabs,
            notes,
            no_notes,
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
            count,
            max_parents: max_parents.as_deref(),
            no_max_parents,
            merges,
            min_parents: min_parents.as_deref(),
            no_min_parents,
            no_merges,
            objects,
            no_object_names,
            filter,
            filter_provided_objects,
            parents,
            first_parent,
            children,
            walk_reflogs,
            grep_reflog,
            reverse,
            full_history,
            ancestry_path,
            dense,
            sparse,
            show_pulls,
            simplify_merges,
            simplify_by_decoration,
            topo_order,
            date_order,
            author_date_order,
            left_right,
            cherry_pick,
            cherry_mark,
            boundary,
            max_count,
            since: since.as_deref(),
            until: until.as_deref(),
            date: date.as_deref(),
            format: format.as_deref(),
            pretty: pretty.as_deref(),
            revs,
        }),
        runtime::Command::MergeBase {
            all,
            is_ancestor,
            octopus,
            commits,
        } => super::history_commands::merge_base(all, is_ancestor, octopus, commits),
        runtime::Command::LastModified {
            recursive,
            show_trees,
            max_depth,
            nul_terminated,
            args,
        } => super::history_commands::last_modified(
            recursive,
            show_trees,
            max_depth,
            nul_terminated,
            args,
        ),
        _ => unreachable!("non-history command dispatched to history"),
    }
}

fn serialize_reflog_args(
    command: Option<runtime::ReflogCommand>,
    mut args: Vec<String>,
) -> Vec<String> {
    let Some(command) = command else {
        return args;
    };
    let mut out = Vec::new();
    match command {
        runtime::ReflogCommand::Expire(expire) => {
            out.push("expire".to_owned());
            if expire.help {
                out.push("--help".to_owned());
            }
            if let Some(value) = expire.expire {
                out.push(format!("--expire={value}"));
            }
            if let Some(value) = expire.expire_unreachable {
                out.push(format!("--expire-unreachable={value}"));
            }
            if expire.rewrite {
                out.push("--rewrite".to_owned());
            }
            if expire.updateref {
                out.push("--updateref".to_owned());
            }
            if expire.stale_fix {
                out.push("--stale-fix".to_owned());
            }
            if expire.dry_run {
                out.push("--dry-run".to_owned());
            }
            if expire.verbose {
                out.push("--verbose".to_owned());
            }
            if expire.all {
                out.push("--all".to_owned());
            }
            if expire.single_worktree {
                out.push("--single-worktree".to_owned());
            }
            out.extend(expire.refs);
        }
        runtime::ReflogCommand::Delete(delete) => {
            out.push("delete".to_owned());
            if delete.rewrite {
                out.push("--rewrite".to_owned());
            }
            if delete.updateref {
                out.push("--updateref".to_owned());
            }
            if delete.dry_run {
                out.push("--dry-run".to_owned());
            }
            if delete.verbose {
                out.push("--verbose".to_owned());
            }
            out.extend(delete.selectors);
        }
        runtime::ReflogCommand::Drop(drop) => {
            out.push("drop".to_owned());
            if drop.all {
                out.push("--all".to_owned());
            }
            if drop.single_worktree {
                out.push("--single-worktree".to_owned());
            }
            out.extend(drop.refs);
        }
    }
    out.append(&mut args);
    out
}

fn serialize_blame_args(
    help: bool,
    contents: Option<std::path::PathBuf>,
    encoding: Option<String>,
    first_parent: bool,
    ignore_rev: Vec<String>,
    ignore_revs_file: Vec<std::path::PathBuf>,
    reverse: Option<String>,
    revs_file: Option<std::path::PathBuf>,
    mut args: Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    if help {
        out.push("--help".to_owned());
    }
    if let Some(path) = contents {
        out.push("--contents".to_owned());
        out.push(path.to_string_lossy().into_owned());
    }
    if let Some(value) = encoding {
        out.push(format!("--encoding={value}"));
    }
    if first_parent {
        out.push("--first-parent".to_owned());
    }
    for value in ignore_rev {
        out.push("--ignore-rev".to_owned());
        out.push(value);
    }
    for path in ignore_revs_file {
        out.push("--ignore-revs-file".to_owned());
        out.push(path.to_string_lossy().into_owned());
    }
    if let Some(value) = reverse {
        out.push("--reverse".to_owned());
        out.push(value);
    }
    if let Some(path) = revs_file {
        out.push("-S".to_owned());
        out.push(path.to_string_lossy().into_owned());
    }
    out.append(&mut args);
    out
}

#[allow(clippy::too_many_arguments)]
fn serialize_annotate_args(
    help: bool,
    porcelain: bool,
    incremental: bool,
    line_porcelain: bool,
    contents: Option<std::path::PathBuf>,
    date: Option<String>,
    encoding: Option<String>,
    first_parent: bool,
    ignore_rev: Vec<String>,
    ignore_revs_file: Vec<std::path::PathBuf>,
    progress: bool,
    no_progress: bool,
    color_lines: bool,
    color_by_age: bool,
    reverse: Option<String>,
    root: bool,
    show_stats: bool,
    copies: u8,
    line_ranges: Vec<String>,
    moves: u8,
    revs_file: Option<std::path::PathBuf>,
    blank_boundary: bool,
    raw_timestamp: bool,
    mut args: Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    if help {
        out.push("-h".to_owned());
    }
    if porcelain {
        out.push("--porcelain".to_owned());
    }
    if incremental {
        out.push("--incremental".to_owned());
    }
    if line_porcelain {
        out.push("--line-porcelain".to_owned());
    }
    if let Some(value) = date {
        out.push(format!("--date={value}"));
    }
    if progress {
        out.push("--progress".to_owned());
    }
    if no_progress {
        out.push("--no-progress".to_owned());
    }
    if color_lines {
        out.push("--color-lines".to_owned());
    }
    if color_by_age {
        out.push("--color-by-age".to_owned());
    }
    if root {
        out.push("--root".to_owned());
    }
    if show_stats {
        out.push("--show-stats".to_owned());
    }
    for _ in 0..copies {
        out.push("-C".to_owned());
    }
    for value in line_ranges {
        out.push("-L".to_owned());
        out.push(value);
    }
    for _ in 0..moves {
        out.push("-M".to_owned());
    }
    if blank_boundary {
        out.push("-b".to_owned());
    }
    if raw_timestamp {
        out.push("-t".to_owned());
    }
    out.extend(serialize_blame_args(
        false,
        contents,
        encoding,
        first_parent,
        ignore_rev,
        ignore_revs_file,
        reverse,
        revs_file,
        Vec::new(),
    ));
    out.append(&mut args);
    out
}

pub(crate) fn run_replay(
    contained: bool,
    advance: Option<String>,
    onto: Option<String>,
    revision_ranges: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::history_commands::run_replay(contained, advance, onto, revision_ranges)
}

pub(crate) fn run_history(
    command: runtime::HistoryCommand,
) -> std::result::Result<(), runtime::CliError> {
    super::history_commands::run_history(command)
}

pub(crate) fn run_range_diff(
    no_dual_color: bool,
    no_no_dual_color: bool,
    ranges: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::history_commands::range_diff(no_dual_color, no_no_dual_color, ranges)
}
