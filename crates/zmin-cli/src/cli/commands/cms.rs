use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Save { message } => super::cms_commands::save(&message),
        runtime::Command::Publish => super::cms_commands::publish(),
        runtime::Command::Update => super::cms_commands::update(),
        runtime::Command::Undo => super::cms_commands::undo(),
        runtime::Command::Changes => super::cms_commands::changes(),
        runtime::Command::Timeline => super::cms_commands::timeline(),
        runtime::Command::Recover { paths } => super::cms_commands::recover(&paths),
        command => unreachable!("non-CMS command routed to CMS dispatcher: {command:?}"),
    }
}
