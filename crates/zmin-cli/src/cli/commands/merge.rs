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
            stat,
            no_stat,
            summary,
            no_summary,
            commit,
            no_commit,
            log,
            no_log,
            verbose: _verbose,
            squash,
            no_squash,
            edit,
            no_edit,
            autostash: _autostash,
            no_autostash: _no_autostash,
            cleanup: _cleanup,
            rerere_autoupdate: _rerere_autoupdate,
            no_rerere_autoupdate: _no_rerere_autoupdate,
            overwrite_ignore: _overwrite_ignore,
            no_overwrite_ignore: _no_overwrite_ignore,
            signoff,
            no_signoff,
            gpg_sign,
            no_gpg_sign,
            verify,
            no_verify,
            quiet,
            progress,
            no_progress,
            allow_unrelated_histories,
            strategies,
            strategy_options,
            commits,
        } => {
            let (no_commit, squash) =
                resolve_merge_commit_mode(raw_args, commit, no_commit, squash, no_squash);
            let show_diffstat =
                resolve_merge_diffstat_mode(raw_args, stat, no_stat, summary, no_summary);
            let log_limit = resolve_merge_log_mode(raw_args, log.as_deref(), no_log)?;
            let _ = resolve_merge_edit_mode(raw_args, edit, no_edit);
            let signoff = resolve_merge_count_mode(
                raw_args,
                "--signoff",
                "--no-signoff",
                signoff,
                no_signoff,
            );
            let gpg_sign = resolve_merge_gpg_sign(raw_args, gpg_sign, no_gpg_sign > 0);
            let _ =
                resolve_merge_count_mode(raw_args, "--verify", "--no-verify", verify, no_verify);
            let quiet = quiet || resolve_merge_short_flag(raw_args, "-q");
            let _ = resolve_merge_count_mode(
                raw_args,
                "--progress",
                "--no-progress",
                progress,
                no_progress,
            );
            super::merge_commands::merge(super::merge_commands::MergeOptions {
                abort,
                continue_,
                ff_only,
                no_ff,
                show_diffstat,
                no_commit,
                log_limit,
                squash,
                cleanup: _cleanup,
                signoff,
                gpg_sign,
                no_gpg_sign: no_gpg_sign > 0,
                quiet,
                allow_unrelated_histories,
                strategies,
                strategy_options,
                commits,
                commit_label: None,
                commit_source: None,
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
            if no_marker_size == 0 {
                marker_size
            } else {
                None
            },
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

fn resolve_merge_diffstat_mode(
    raw_args: &[String],
    stat: bool,
    no_stat: bool,
    summary: bool,
    no_summary: bool,
) -> bool {
    let mut show_diffstat = true;
    if no_stat || no_summary {
        show_diffstat = false;
    } else if stat || summary {
        show_diffstat = true;
    }
    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--stat" | "--summary" => show_diffstat = true,
            "-n" | "--no-stat" | "--no-summary" => show_diffstat = false,
            _ => {}
        }
    }
    show_diffstat
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

pub(crate) fn resolve_merge_edit_mode(raw_args: &[String], edit: u8, no_edit: u8) -> bool {
    let mut enabled = edit > 0 && no_edit == 0;
    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "-e" | "--edit" => enabled = true,
            "--no-edit" => enabled = false,
            _ => {}
        }
    }
    enabled
}

pub(crate) fn resolve_merge_count_mode(
    raw_args: &[String],
    positive: &str,
    negative: &str,
    positive_count: u8,
    negative_count: u8,
) -> bool {
    let mut enabled = positive_count > 0 && negative_count == 0;
    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == positive {
            enabled = true;
        } else if arg == negative {
            enabled = false;
        }
    }
    enabled
}

pub(crate) fn resolve_merge_gpg_sign(
    raw_args: &[String],
    gpg_sign: Option<String>,
    no_gpg_sign: bool,
) -> Option<String> {
    let mut effective = (!no_gpg_sign).then_some(gpg_sign).flatten();
    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--no-gpg-sign" => effective = None,
            "--gpg-sign" | "-S" => effective = Some(String::new()),
            _ if arg.starts_with("--gpg-sign=") => {
                effective = Some(arg.trim_start_matches("--gpg-sign=").to_owned());
            }
            _ if arg.starts_with("-S") && arg.len() > 2 => {
                effective = Some(arg[2..].to_owned());
            }
            _ => {}
        }
    }
    effective
}

pub(crate) fn resolve_merge_short_flag(raw_args: &[String], flag: &str) -> bool {
    raw_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == flag)
}

pub(crate) fn resolve_merge_log_mode(
    raw_args: &[String],
    log: Option<&str>,
    no_log: bool,
) -> std::result::Result<Option<usize>, runtime::CliError> {
    const DEFAULT_MERGE_LOG_LIMIT: usize = 20;

    let mut log_limit = if no_log {
        None
    } else {
        parse_merge_log_limit(log, DEFAULT_MERGE_LOG_LIMIT)?
    };
    for arg in raw_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--log" => log_limit = Some(DEFAULT_MERGE_LOG_LIMIT),
            "--no-log" => log_limit = None,
            _ => {
                if let Some(value) = arg.strip_prefix("--log=") {
                    log_limit = parse_merge_log_limit(Some(value), DEFAULT_MERGE_LOG_LIMIT)?;
                }
            }
        }
    }
    Ok(log_limit)
}

fn parse_merge_log_limit(
    value: Option<&str>,
    default_limit: usize,
) -> std::result::Result<Option<usize>, runtime::CliError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(Some(default_limit));
    }
    let parsed = value
        .parse::<usize>()
        .map_err(|_| runtime::CliError::Fatal {
            code: 129,
            message: format!("option `--log` expects a non-negative integer, got `{value}`"),
        })?;
    if parsed == 0 {
        return Ok(None);
    }
    Ok(Some(parsed))
}
