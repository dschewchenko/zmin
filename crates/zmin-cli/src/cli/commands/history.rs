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
            revs,
        } => {
            super::history_commands::shortlog(committer, numbered, summary, email, no_merges, revs)
        }
        runtime::Command::Blame { long, root, args } => {
            super::history_commands::blame(long, root, false, args)
        }
        runtime::Command::Annotate { args } => {
            super::history_commands::blame(false, true, true, args)
        }
        runtime::Command::ShowBranch {
            all,
            remotes,
            current,
            sha1_name,
            no_name,
            revs,
        } => super::history_commands::show_branch(all, remotes, current, sha1_name, no_name, revs),
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
        } => {
            super::history_commands::request_pull(patch > 0, &start, &url, end.as_deref())
        }
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
            parents,
            first_parent,
            no_diff_merges,
            diff_merges,
            separate_merges,
            dd,
            reverse,
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
            decorate,
            clear_decorations,
            pickaxe_string,
            pickaxe_regex,
            pickaxe_regex_mode,
            pickaxe_all,
            ignore_matching_lines,
            walk_reflogs,
            no_walk,
            format,
            max_count,
            since,
            date,
            pretty,
            revs,
        } => super::history_commands::log(super::history_commands::LogOptions {
            oneline,
            zero,
            all,
            parents,
            first_parent,
            no_diff_merges,
            diff_merges: diff_merges.as_deref(),
            separate_merges,
            dd,
            reverse,
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
            diff_required: false,
            decorate: decorate.as_deref(),
            clear_decorations,
            pickaxe_string: pickaxe_string.as_deref(),
            pickaxe_regex: pickaxe_regex.as_deref(),
            pickaxe_regex_mode,
            pickaxe_all,
            ignore_matching_lines,
            walk_reflogs,
            no_walk,
            format: format.as_deref(),
            max_count: max_count.as_deref(),
            since: since.as_deref(),
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
            parents,
            first_parent: false,
            no_diff_merges: false,
            diff_merges: None,
            separate_merges: false,
            dd: false,
            reverse,
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
            format: format.as_deref(),
            max_count: max_count.as_deref(),
            since: since.as_deref(),
            date: date.as_deref(),
            pretty: pretty.as_deref(),
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
            root,
            combined,
            separate_merges,
            first_parent,
            format: format.as_deref(),
            pretty: pretty.as_deref(),
            args,
        }),
        runtime::Command::RevList {
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
        } => super::history_commands::rev_list(super::history_commands::RevListOptions {
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

fn serialize_reflog_args(command: Option<runtime::ReflogCommand>, mut args: Vec<String>) -> Vec<String> {
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
