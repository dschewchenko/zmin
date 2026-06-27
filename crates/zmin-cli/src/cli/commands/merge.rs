use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Merge {
            abort,
            continue_,
            ff_only,
            no_ff,
            commit,
            no_commit,
            squash,
            no_squash,
            strategies,
            commits,
        } => {
            let (no_commit, squash) =
                resolve_merge_commit_mode(raw_args, commit, no_commit, squash, no_squash);
            super::merge_commands::merge(super::merge_commands::MergeOptions {
                abort,
                continue_,
                ff_only,
                no_ff,
                no_commit,
                squash,
                strategies,
                commits,
                commit_label: None,
            })
        }
        runtime::Command::Mergetool {
            tool,
            tool_help,
            no_prompt,
            prompt,
            gui: _,
            no_gui: _,
            orderfile,
            paths,
        } => super::merge_commands::mergetool(
            tool.as_deref(),
            tool_help,
            no_prompt,
            prompt,
            orderfile,
            paths,
        ),
        runtime::Command::MergeTree {
            write_tree,
            trivial_merge,
            messages,
            no_messages,
            quiet,
            nul_terminated,
            name_only,
            allow_unrelated_histories,
            stdin,
            merge_base,
            strategy_options,
            args,
        } => super::merge_commands::merge_tree_command(runtime::MergeTreeOptions {
            write_tree,
            trivial_merge,
            messages,
            no_messages,
            quiet,
            nul_terminated,
            name_only,
            allow_unrelated_histories,
            stdin,
            merge_base,
            strategy_options,
            args,
        }),
        runtime::Command::MergeFile {
            stdout,
            quiet,
            no_quiet,
            ours,
            no_ours,
            theirs,
            no_theirs,
            union,
            no_union,
            diff3,
            no_diff3,
            zdiff3,
            marker_size,
            no_marker_size,
            diff_algorithm,
            object_id,
            no_object_id,
            labels,
            current,
            base,
            other,
        } => super::merge_commands::merge_file_command(
            stdout,
            quiet > 0 && no_quiet == 0,
            super::merge_commands::MergeFileConflictStyle::from_flags(
                ours > 0 && no_ours == 0,
                theirs > 0 && no_theirs == 0,
                union > 0 && no_union == 0,
            ),
            (diff3 > 0 || zdiff3 > 0) && no_diff3 == 0,
            if no_marker_size == 0 { marker_size } else { None },
            diff_algorithm,
            object_id > 0 && no_object_id == 0,
            labels,
            current,
            base,
            other,
        ),
        runtime::Command::MergeOneFile {
            orig_blob,
            our_blob,
            their_blob,
            path,
            orig_mode,
            our_mode,
            their_mode,
        } => super::merge_commands::merge_one_file(
            &orig_blob,
            &our_blob,
            &their_blob,
            &path,
            &orig_mode,
            &our_mode,
            &their_mode,
        ),
        runtime::Command::MergeIndex {
            one_shot,
            quiet,
            merge_program,
            all,
            paths,
        } => super::merge_commands::merge_index(one_shot, quiet, &merge_program, all, paths),
        command => unreachable!("non-merge command routed to merge dispatcher: {command:?}"),
    }
}

fn resolve_merge_commit_mode(
    raw_args: &[String],
    commit: bool,
    no_commit: bool,
    squash: bool,
    no_squash: bool,
) -> (bool, bool) {
    let mut effective_no_commit = no_commit;
    if commit {
        effective_no_commit = false;
    }
    let mut effective_squash = squash;
    if no_squash {
        effective_squash = false;
    }
    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--commit" => effective_no_commit = false,
            "--no-commit" => effective_no_commit = true,
            "--squash" => effective_squash = true,
            "--no-squash" => effective_squash = false,
            _ => {}
        }
    }
    (effective_no_commit, effective_squash)
}
