use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Notes { args } => super::notes_commands::notes(args),
        command => unreachable!("non-notes command routed to notes dispatcher: {command:?}"),
    }
}
