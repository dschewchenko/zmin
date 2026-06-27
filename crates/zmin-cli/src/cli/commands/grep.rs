use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Grep {
            cached,
            untracked,
            exclude_standard,
            no_exclude_standard,
            no_index,
            recurse_submodules,
            recursive,
            no_recursive,
            max_depth,
            threads,
            quiet,
            ignore_case,
            invert_match,
            line_number,
            files_with_matches,
            name_only,
            files_without_match,
            count,
            all_match,
            max_count,
            after_context,
            before_context,
            context,
            and,
            or,
            not,
            patterns,
            pattern_files,
            with_filename,
            no_filename,
            ignore_binary,
            open_files_in_pager,
            null_terminated,
            full_name,
            heading,
            break_,
            show_function,
            function_context,
            basic_regexp,
            extended_regexp,
            fixed_strings,
            perl_regexp,
            text,
            textconv,
            no_textconv,
            color,
            no_color,
            word_regexp,
            column,
            only_matching,
            pattern,
            args,
        } => {
            let (pattern, open_files_in_pager, args) =
                normalize_open_files_in_pager_pattern(pattern, args, open_files_in_pager, raw_args);
            super::grep_commands::grep(
                cached,
                untracked,
                exclude_standard,
                no_exclude_standard,
                no_index,
                recurse_submodules,
                recursive,
                no_recursive,
                max_depth,
                threads,
                quiet,
                ignore_case,
                invert_match,
                line_number,
                files_with_matches,
                name_only,
                files_without_match,
                count,
                all_match,
                max_count,
                after_context,
                before_context,
                context,
                and,
                or,
                not,
                patterns,
                pattern_files,
                with_filename,
                no_filename,
                ignore_binary,
                open_files_in_pager,
                null_terminated,
                full_name,
                heading,
                break_,
                show_function,
                function_context,
                basic_regexp,
                extended_regexp,
                fixed_strings,
                perl_regexp,
                text,
                textconv,
                no_textconv,
                color,
                no_color,
                word_regexp,
                column,
                only_matching,
                pattern,
                args,
            )
        }
        _ => unreachable!("non-grep command dispatched to grep"),
    }
}

fn normalize_open_files_in_pager_pattern(
    pattern: Option<String>,
    args: Vec<String>,
    open_files_in_pager: Option<String>,
    raw_args: &[String],
) -> (Option<String>, Option<String>, Vec<String>) {
    if pattern.is_some() || open_files_in_pager.is_none() {
        return (pattern, open_files_in_pager, args);
    }
    let mut pager_value = open_files_in_pager;
    let mut recovered_pattern = None;
    let mut recovered_args = Vec::new();
    let mut saw_pager = false;

    for arg in raw_args.iter().skip(1) {
        if !saw_pager {
            if arg == "-O" || arg == "--open-files-in-pager" {
                pager_value = Some(String::new());
                saw_pager = true;
                continue;
            }
            if let Some(value) = arg.strip_prefix("-O")
                && !value.is_empty()
            {
                pager_value = Some(value.to_owned());
                saw_pager = true;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--open-files-in-pager=") {
                pager_value = Some(value.to_owned());
                saw_pager = true;
                continue;
            }
            continue;
        }
        if recovered_pattern.is_none() {
            recovered_pattern = Some(arg.clone());
        } else {
            recovered_args.push(arg.clone());
        }
    }

    (recovered_pattern, pager_value, recovered_args)
}
