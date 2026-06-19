use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Grep {
            cached,
            line_number,
            files_with_matches,
            fixed_strings,
            pattern,
            args,
        } => super::grep_commands::grep(
            cached,
            line_number,
            files_with_matches,
            fixed_strings,
            &pattern,
            args,
        ),
        _ => unreachable!("non-grep command dispatched to grep"),
    }
}
