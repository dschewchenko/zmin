use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::CherryPick {
            abort,
            continue_,
            no_commit,
            mainline,
            commits,
        } => runtime::sequencer_commands::sequencer_command(
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
        } => runtime::sequencer_commands::sequencer_command(
            "revert", abort, continue_, no_commit, mainline, commits,
        ),
        runtime::Command::Bisect { args } => runtime::sequencer_commands::bisect(args),
        runtime::Command::Rerere { args } => runtime::sequencer_commands::rerere(args),
        runtime::Command::Rebase {
            abort,
            continue_,
            onto,
            args,
        } => runtime::sequencer_commands::rebase(
            abort,
            continue_,
            onto.as_deref(),
            args,
            false,
            false,
        ),
        _ => unreachable!("non-sequencer command dispatched to sequencer"),
    }
}
