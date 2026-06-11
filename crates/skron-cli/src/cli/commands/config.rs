use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Config {
            get,
            list,
            unset,
            bool_value,
            int_value,
            bool_or_int_value,
            bool_or_str_value,
            path_value,
            expiry_date_value,
            value_type,
            worktree,
            includes,
            show_origin,
            show_scope,
            name,
            value,
        } => runtime::config_commands::config(runtime::ConfigArgs {
            get,
            list,
            unset,
            bool_value,
            int_value,
            bool_or_int_value,
            bool_or_str_value,
            path_value,
            expiry_date_value,
            value_type,
            worktree,
            includes,
            show_origin,
            show_scope,
            name,
            value,
        }),
        runtime::Command::Var { list, variable } => {
            runtime::config_commands::var(list, variable.as_deref())
        }
        runtime::Command::Version { build_options } => {
            runtime::config_commands::version(build_options)
        }
        command => unreachable!("non-config command routed to config dispatcher: {command:?}"),
    }
}
