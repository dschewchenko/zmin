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
            ranges,
        } => run_range_diff(no_dual_color, ranges),
        runtime::Command::FilterBranch {
            force,
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
        } => runtime::history_commands::filter_branch(
            runtime::history_commands::FilterBranchOptions {
                force,
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
            },
        ),
        runtime::Command::Shortlog {
            committer,
            numbered,
            summary,
            email,
            no_merges,
            revs,
        } => runtime::history_commands::shortlog(
            committer, numbered, summary, email, no_merges, revs,
        ),
        runtime::Command::Blame { long, root, args } => {
            runtime::history_commands::blame(long, root, false, args)
        }
        runtime::Command::Annotate { args } => {
            runtime::history_commands::blame(false, true, true, args)
        }
        runtime::Command::ShowBranch {
            all,
            remotes,
            current,
            sha1_name,
            no_name,
            revs,
        } => {
            runtime::history_commands::show_branch(all, remotes, current, sha1_name, no_name, revs)
        }
        runtime::Command::Cherry {
            verbose,
            abbrev,
            upstream,
            head,
            limit,
        } => runtime::history_commands::cherry(
            verbose,
            abbrev,
            upstream.as_deref(),
            head.as_deref(),
            limit.as_deref(),
        ),
        runtime::Command::RequestPull { start, url, end } => {
            runtime::history_commands::request_pull(&start, &url, end.as_deref())
        }
        runtime::Command::Describe {
            all,
            tags,
            long,
            abbrev,
            exact_match,
            always,
            dirty,
            matches,
            excludes,
            commits,
        } => runtime::history_commands::describe(runtime::history_commands::DescribeOptions {
            all,
            tags,
            long,
            abbrev,
            exact_match,
            always,
            dirty,
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
            always,
            commits,
        } => runtime::history_commands::name_rev(runtime::history_commands::NameRevOptions {
            name_only,
            tags,
            refs,
            excludes,
            all,
            annotate_stdin,
            always,
            commits,
        }),
        runtime::Command::Reflog { args } => runtime::history_commands::reflog(args),
        runtime::Command::Log {
            oneline,
            all,
            parents,
            reverse,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            format,
            max_count,
            since,
            pretty,
            revs,
        } => runtime::history_commands::log(runtime::history_commands::LogOptions {
            oneline,
            all,
            parents,
            reverse,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            format: format.as_deref(),
            max_count: max_count.as_deref(),
            since: since.as_deref(),
            pretty: pretty.as_deref(),
            revs,
        }),
        runtime::Command::Whatchanged {
            oneline,
            all,
            parents,
            reverse,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            format,
            max_count,
            since,
            pretty,
            revs,
        } => runtime::history_commands::log(runtime::history_commands::LogOptions {
            oneline,
            all,
            parents,
            reverse,
            stat,
            numstat,
            shortstat,
            raw: raw || !(stat || numstat || shortstat || summary || name_only || name_status),
            summary,
            name_only,
            name_status,
            format: format.as_deref(),
            max_count: max_count.as_deref(),
            since: since.as_deref(),
            pretty: pretty.as_deref(),
            revs,
        }),
        runtime::Command::Show {
            no_patch,
            oneline,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            format,
            pretty,
            object,
        } => runtime::history_commands::show(runtime::history_commands::ShowOptions {
            no_patch,
            oneline,
            stat,
            numstat,
            shortstat,
            raw,
            summary,
            name_only,
            name_status,
            format: format.as_deref(),
            pretty: pretty.as_deref(),
            object: object.as_deref(),
        }),
        runtime::Command::RevList {
            all,
            count,
            objects,
            no_object_names,
            parents,
            reverse,
            max_count,
            revs,
        } => runtime::history_commands::rev_list(runtime::history_commands::RevListOptions {
            all,
            count,
            objects,
            no_object_names,
            parents,
            reverse,
            max_count,
            revs,
        }),
        runtime::Command::MergeBase {
            is_ancestor,
            octopus,
            commits,
        } => runtime::history_commands::merge_base(is_ancestor, octopus, commits),
        runtime::Command::LastModified {
            recursive,
            show_trees,
            max_depth,
            nul_terminated,
            args,
        } => runtime::history_commands::last_modified(
            recursive,
            show_trees,
            max_depth,
            nul_terminated,
            args,
        ),
        _ => unreachable!("non-history command dispatched to history"),
    }
}

pub(crate) fn run_replay(
    contained: bool,
    advance: Option<String>,
    onto: Option<String>,
    revision_ranges: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::history_commands::run_replay(contained, advance, onto, revision_ranges)
}

pub(crate) fn run_history(
    command: runtime::HistoryCommand,
) -> std::result::Result<(), runtime::CliError> {
    runtime::history_commands::run_history(command)
}

pub(crate) fn run_range_diff(
    no_dual_color: bool,
    ranges: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::history_commands::range_diff(no_dual_color, ranges)
}
