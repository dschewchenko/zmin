use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Config {
            options:
                runtime::ConfigCommandArgs {
                    null,
                    all,
                    blob,
                    comment,
                    fixed_value,
                    get,
                    get_all,
                    get_color,
                    get_colorbool,
                    get_regexp,
                    get_urlmatch,
                    edit,
                    list,
                    name_only,
                    show_names,
                    no_includes,
                    no_type,
                    regexp,
                    replace_all,
                    rename_section,
                    remove_section,
                    system,
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
                    url,
                    value_pattern,
                    arg0,
                    arg1,
                    arg2,
                },
        } => {
            let type_specifiers = ordered_type_specifiers(raw_args);
            let action = match arg0.as_deref() {
                Some("get") => Some("get"),
                Some("set") => Some("set"),
                Some("unset") => Some("unset"),
                Some("list") => Some("list"),
                Some("edit") => Some("edit"),
                Some("rename-section") => Some("rename-section"),
                Some("remove-section") => Some("remove-section"),
                _ => None,
            };
            let legacy_url = get_urlmatch.then(|| arg1.clone()).flatten();
            let (name, value, positional_value_pattern) = if action.is_some() {
                let pattern = if matches!(action, Some("get" | "unset")) {
                    arg2.clone()
                } else {
                    None
                };
                (
                    arg1,
                    if matches!(action, Some("set" | "rename-section")) {
                        arg2
                    } else {
                        None
                    },
                    pattern,
                )
            } else {
                let query_like = get || get_all || get_regexp || get_urlmatch || unset || unset_all;
                (
                    arg0,
                    if query_like { None } else { arg1.clone() },
                    if get_urlmatch {
                        None
                    } else if query_like {
                        arg1
                    } else {
                        arg2
                    },
                )
            };
            let resolved_url = legacy_url.or(url);
            super::config_commands::config(runtime::ConfigArgs {
                all,
                blob,
                comment,
                fixed_value,
                get: get || action == Some("get"),
                get_all: get_all || (action == Some("get") && all),
                get_color,
                get_colorbool,
                get_regexp,
                get_urlmatch,
                edit: edit || action == Some("edit"),
                list: list || action == Some("list"),
                name_only,
                show_names,
                no_includes,
                no_type,
                regexp,
                replace_all,
                rename_section: rename_section || action == Some("rename-section"),
                remove_section: remove_section || action == Some("remove-section"),
                system,
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
                type_specifiers,
                default,
                worktree,
                local,
                global,
                file,
                includes,
                modern_get: action == Some("get"),
                modern_set: action == Some("set"),
                show_origin,
                show_scope,
                url: resolved_url,
                value_pattern: value_pattern.or(positional_value_pattern),
                null,
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

fn ordered_type_specifiers(raw_args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut index = 1usize;
    while index < raw_args.len() {
        let arg = &raw_args[index];
        if arg == "--" {
            break;
        }
        match arg.as_str() {
            "--no-type" => out.push("none".to_owned()),
            "--bool" => out.push("bool".to_owned()),
            "--int" => out.push("int".to_owned()),
            "--bool-or-int" => out.push("bool-or-int".to_owned()),
            "--bool-or-str" => out.push("bool-or-str".to_owned()),
            "--path" => out.push("path".to_owned()),
            "--expiry-date" => out.push("expiry-date".to_owned()),
            "-t" | "--type" => {
                if let Some(value) = raw_args.get(index + 1) {
                    out.push(value.clone());
                    index += 1;
                }
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--type=") {
                    out.push(value.to_owned());
                }
            }
        }
        index += 1;
    }
    out
}
