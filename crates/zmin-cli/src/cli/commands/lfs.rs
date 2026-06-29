use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Lfs { args } => super::lfs_commands::lfs_command(args),
        command => unreachable!("non-lfs command routed to lfs dispatcher: {command:?}"),
    }
}
