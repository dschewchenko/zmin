use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Credential { operation } => {
            super::credential_commands::credential(&operation)
        }
        runtime::Command::CredentialStore { file, action } => {
            super::credential_commands::credential_store(file, &action)
        }
        runtime::Command::CredentialCache {
            timeout,
            socket,
            daemon_internal,
            action,
        } => super::credential_commands::credential_cache(timeout, socket, daemon_internal, action),
        command => {
            unreachable!("non-credential command routed to credential dispatcher: {command:?}")
        }
    }
}
