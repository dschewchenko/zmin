use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Grep {
            cached,
            ignore_case,
            invert_match,
            line_number,
            files_with_matches,
            files_without_match,
            count,
            max_count,
            with_filename,
            full_name,
            heading,
            break_,
            fixed_strings,
            pattern,
            args,
        } => super::grep_commands::grep(
            cached,
            ignore_case,
            invert_match,
            line_number,
            files_with_matches,
            files_without_match,
            count,
            max_count,
            with_filename,
            full_name,
            heading,
            break_,
            fixed_strings,
            &pattern,
            args,
        ),
        _ => unreachable!("non-grep command dispatched to grep"),
    }
}
