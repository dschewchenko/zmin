use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Apply {
            check,
            cached,
            index,
            reverse,
            patches,
        } => run_apply(check, cached, index, reverse, patches),
        _ => unreachable!("non-patch command dispatched to patch"),
    }
}

pub(crate) fn run_apply(
    check: bool,
    cached: bool,
    index: bool,
    reverse: bool,
    patches: Vec<std::path::PathBuf>,
) -> std::result::Result<(), runtime::CliError> {
    super::patch_commands::run_apply(check, cached, index, reverse, patches)
}
