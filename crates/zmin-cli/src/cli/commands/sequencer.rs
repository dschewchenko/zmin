use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::CherryPick {
            abort,
            continue_,
            no_commit,
            mainline,
            commits,
        } => super::sequencer_commands::sequencer_command(
            "cherry-pick",
            abort,
            continue_,
            no_commit,
            mainline,
            commits,
        ),
        runtime::Command::Revert {
            abort,
            continue_,
            no_commit,
            mainline,
            commits,
        } => super::sequencer_commands::sequencer_command(
            "revert", abort, continue_, no_commit, mainline, commits,
        ),
        runtime::Command::Bisect { args } => super::sequencer_commands::bisect(args),
        runtime::Command::Rerere { args } => super::sequencer_commands::rerere(args),
        runtime::Command::Rebase {
            abort,
            continue_,
            interactive,
            onto,
            args,
        } => super::sequencer_commands::rebase(
            abort,
            continue_,
            onto.as_deref(),
            args,
            false,
            interactive,
        ),
        _ => unreachable!("non-sequencer command dispatched to sequencer"),
    }
}
