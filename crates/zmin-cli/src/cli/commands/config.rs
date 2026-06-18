use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Config {
            get,
            get_all,
            list,
            unset,
            unset_all,
            add,
            append,
            bool_value,
            int_value,
            bool_or_int_value,
            bool_or_str_value,
            path_value,
            expiry_date_value,
            value_type,
            default,
            worktree,
            local,
            global,
            file,
            includes,
            show_origin,
            show_scope,
            arg0,
            arg1,
            arg2,
        } => {
            let action = match arg0.as_deref() {
                Some("get") => Some("get"),
                Some("set") => Some("set"),
                Some("unset") => Some("unset"),
                Some("list") => Some("list"),
                _ => None,
            };
            let (name, value) = if action.is_some() {
                (arg1, if action == Some("set") { arg2 } else { None })
            } else {
                (arg0, arg1)
            };
            super::config_commands::config(runtime::ConfigArgs {
                get: get || action == Some("get"),
                get_all,
                list: list || action == Some("list"),
                unset: unset || action == Some("unset"),
                unset_all,
                append: add || append,
                bool_value,
                int_value,
                bool_or_int_value,
                bool_or_str_value,
                path_value,
                expiry_date_value,
                value_type,
                default,
                worktree,
                local,
                global,
                file,
                includes,
                show_origin,
                show_scope,
                name,
                value,
            })
        }
        runtime::Command::Var { list, variable } => {
            super::config_commands::var(list, variable.as_deref())
        }
        runtime::Command::Version { build_options } => {
            super::config_commands::version(build_options)
        }
        command => unreachable!("non-config command routed to config dispatcher: {command:?}"),
    }
}
