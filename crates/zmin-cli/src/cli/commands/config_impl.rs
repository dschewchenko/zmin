use super::*;

pub(crate) fn config(mut args: ConfigArgs) -> Result<()> {
    if !args.unset && !args.unset_all && args.name.as_deref() == Some("unset") {
        args.unset = true;
        args.name = args.value.take();
    }
    if !args.remove_section && args.name.as_deref() == Some("remove-section") {
        args.remove_section = true;
        args.name = args.value.take();
    }
    if !args.rename_section && args.name.as_deref() == Some("rename-section") {
        args.rename_section = true;
        args.name = args.value.take();
    }
    if let Some(name) = args.name.as_deref()
        && args.value.is_none()
        && (args.modern_set || (name.contains('=') && parse_config_name(name).is_err()))
    {
        return Err(config_missing_set_value_error(name));
    }
    if args.modern_set
        && let Some(name) = args.name.as_deref()
        && args.value.is_some()
        && parse_config_name(name).is_err()
    {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("invalid key: {name}"),
        });
    }
    if args.all && !(args.modern_get || args.value.is_some() || args.unset || args.unset_all) {
        return Err(CliError::Fatal {
            code: 129,
            message: "--all requires `git config get`, `git config set` or `git config unset`"
                .into(),
        });
    }
    if args.regexp && !args.modern_get {
        return Err(CliError::Fatal {
            code: 129,
            message: "--regexp requires `git config get`".into(),
        });
    }
    if args.replace_all && args.value.is_none() {
        return Err(CliError::Fatal {
            code: 129,
            message: "--replace-all requires a value".into(),
        });
    }
    if args.blob.is_some()
        && (args.unset
            || args.unset_all
            || args.remove_section
            || args.value.is_some()
            || args.append)
    {
        return Err(CliError::Fatal {
            code: 129,
            message: "--blob cannot be combined with config writes".into(),
        });
    }
    if args.fixed_value {
        if args.list
            || args.append
            || args.rename_section
            || args.remove_section
            || args.get_urlmatch
            || args.get_color
            || args.get_colorbool
            || args.edit
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "--fixed-value cannot be used with this action".into(),
            });
        }
        let fixed_value_requires_pattern = args.value.is_some()
            || args.replace_all
            || args.get
            || args.get_all
            || args.get_regexp
            || args.unset
            || args.unset_all;
        if fixed_value_requires_pattern && args.value_pattern.is_none() {
            return Err(CliError::Fatal {
                code: 129,
                message: "--fixed-value requires a value pattern".into(),
            });
        }
    }
    if args.default.is_some()
        && (args.list
            || args.unset
            || args.unset_all
            || args.append
            || args.rename_section
            || args.remove_section
            || args.value.is_some())
    {
        return Err(CliError::Fatal {
            code: 129,
            message: "--default is only applicable to config get operations".into(),
        });
    }
    let value_type = config_value_type(&args)?;
    let scoped_file = config_file_scope_path(&args)?;
    if args.edit {
        return config_edit(&args, scoped_file.as_ref());
    }
    if args.get_color {
        return config_get_color(&args, scoped_file.as_ref());
    }
    if args.get_colorbool {
        return config_get_colorbool(&args, scoped_file.as_ref());
    }
    if args.list {
        if args.get
            || args.get_all
            || args.get_regexp
            || args.unset
            || args.unset_all
            || args.append
            || args.name.is_some()
            || args.value.is_some()
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "--list cannot be combined with config get/set arguments".into(),
            });
        }
        let entries = scoped_config_entries(&args, scoped_file.as_ref())?;
        for entry in entries {
            let value = if args.name_only {
                entry.name()
            } else if args.null {
                format_config_null_list_value(&entry)
            } else if let Some(value_type) = value_type {
                match format_config_list_value(&entry, value_type) {
                    Ok(value) => value,
                    Err(_) => continue,
                }
            } else {
                entry.list_line()
            };
            print_config_output_line(&entry, &value, args.show_origin, args.show_scope, args.null)?;
        }
        return Ok(());
    }

    let Some(name) = args.name.clone() else {
        return Err(CliError::Stderr {
            code: 2,
            text: "error: no action specified\n".into(),
        });
    };

    if args.get_urlmatch {
        let Some(url) = args.url.as_deref() else {
            return Err(CliError::Fatal {
                code: 129,
                message: "--get-urlmatch requires a URL".into(),
            });
        };
        return config_get_urlmatch(&args, scoped_file.as_ref(), &name, url);
    }

    if args.rename_section {
        if args.get
            || args.get_all
            || args.get_regexp
            || args.append
            || args.all
            || args.unset
            || args.unset_all
            || args.remove_section
            || args.fixed_value
            || value_type.is_some()
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "--rename-section cannot be combined with config get/set modifiers".into(),
            });
        }
        let Some(new_name) = args.value.as_deref() else {
            return Err(CliError::Fatal {
                code: 128,
                message: "you must provide two arguments".into(),
            });
        };
        let path = config_target_path_for_write(&args, scoped_file.as_ref())?;
        return config_rename_section_in_file(&path, &name, new_name);
    }

    if args.remove_section {
        if args.get
            || args.get_all
            || args.get_regexp
            || args.append
            || args.all
            || args.unset
            || args.unset_all
            || args.fixed_value
            || args.value.is_some()
            || value_type.is_some()
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "--remove-section cannot be combined with config get/set modifiers".into(),
            });
        }
        let path = config_target_path_for_write(&args, scoped_file.as_ref())?;
        return config_remove_section_in_file(&path, &name);
    }

    if args.unset || args.unset_all {
        if args.get
            || args.get_all
            || args.get_regexp
            || args.append
            || args.remove_section
            || args.value.is_some()
            || value_type.is_some()
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "--unset cannot be combined with config get/set modifiers".into(),
            });
        }
        let path = config_target_path_for_write(&args, scoped_file.as_ref())?;
        return config_unset_value_in_file(
            &path,
            &name,
            args.unset_all || args.all,
            args.value_pattern.as_deref(),
            args.fixed_value,
        );
    }

    if args.get_regexp {
        if args.get || args.get_all || args.append {
            return Err(CliError::Fatal {
                code: 129,
                message: "--get-regexp cannot be combined with other config get/set modifiers"
                    .into(),
            });
        }
        let name_regex = Regex::new(&name).map_err(|err| CliError::Fatal {
            code: 1,
            message: err.to_string(),
        })?;
        let value_pattern = args.value_pattern.as_deref();
        let value_regex = if args.fixed_value {
            None
        } else {
            value_pattern
                .map(Regex::new)
                .transpose()
                .map_err(|err| CliError::Fatal {
                    code: 1,
                    message: err.to_string(),
                })?
        };
        let entries = scoped_config_entries(&args, scoped_file.as_ref())?;
        let mut matched = false;
        for entry in entries {
            let entry_name = entry.name();
            if !name_regex.is_match(entry_name.as_bytes()) {
                continue;
            }
            if args.fixed_value {
                if value_pattern.is_some_and(|pattern| entry.value != pattern) {
                    continue;
                }
            } else if value_regex
                .as_ref()
                .is_some_and(|regex| !regex.is_match(entry.value.as_bytes()))
            {
                continue;
            }
            let value = if args.name_only {
                entry_name
            } else {
                let formatted = if let Some(value_type) = value_type {
                    format_config_value(&entry_name, &entry, value_type)?
                } else {
                    entry.value.clone()
                };
                format_config_named_value(
                    &entry_name,
                    &formatted,
                    entry.implicit_bool && value_type.is_none(),
                    args.null,
                )
            };
            print_config_output_line(&entry, &value, args.show_origin, args.show_scope, args.null)?;
            matched = true;
        }
        if !matched {
            return Err(CliError::Exit(1));
        }
        return Ok(());
    }

    if args.modern_get {
        if let Some(url) = args.url.as_deref() {
            return config_get_urlmatch(&args, scoped_file.as_ref(), &name, url);
        }
        if name.is_empty()
            && value_type == Some(ConfigValueType::Color)
            && let Some(default) = args.default.as_deref()
        {
            return write_config_color(&format_config_default_value(
                &name,
                default,
                ConfigValueType::Color,
            )?);
        }
        let mut entries = if args.regexp {
            matching_config_entries_regexp(&args, scoped_file.as_ref(), &name)?
        } else {
            matching_config_entries(&args, scoped_file.as_ref(), &name)?
        };
        entries = filter_entries_by_value_pattern(
            entries,
            args.value_pattern.as_deref(),
            args.fixed_value,
        )?;
        if entries.is_empty() {
            let Some(default) = args.default.as_deref() else {
                return Err(CliError::Exit(1));
            };
            let value = if let Some(value_type) = value_type {
                format_config_default_value(&name, default, value_type)?
            } else {
                default.to_owned()
            };
            if !args.name_only {
                if value_type == Some(ConfigValueType::Color) {
                    return write_config_color(&value);
                }
                print_config_default_output(&args, &name, &value)?;
            }
            return Ok(());
        }
        let selected = if args.all {
            entries
        } else {
            vec![entries.last().expect("non-empty entries").clone()]
        };
        for entry in selected {
            let formatted = if let Some(value_type) = value_type {
                format_config_value(&name, &entry, value_type)?
            } else {
                entry.value.clone()
            };
            let value = if args.name_only {
                entry.name()
            } else if args.show_names {
                format_config_named_value(
                    &entry.name(),
                    &formatted,
                    entry.implicit_bool && value_type.is_none(),
                    args.null,
                )
            } else {
                formatted
            };
            print_config_output_line(&entry, &value, args.show_origin, args.show_scope, args.null)?;
        }
        return Ok(());
    }

    if let Some(value) = args.value.clone() {
        if args.get || args.get_all {
            return Err(CliError::Fatal {
                code: 129,
                message: "--get cannot be combined with setting a config value".into(),
            });
        }
        let stored_value = normalize_config_value(&name, &value, value_type)?;
        let path = config_target_path_for_write(&args, scoped_file.as_ref())?;
        if args.append {
            config_append_value_in_file(&path, &name, &stored_value)?;
        } else {
            config_set_value_in_file(
                &path,
                &name,
                &stored_value,
                args.all || args.replace_all,
                args.value_pattern.as_deref(),
                args.fixed_value,
                args.comment.as_deref(),
            )?;
        }
        return Ok(());
    }

    if args.get_all {
        let entries = filter_entries_by_value_pattern(
            matching_config_entries(&args, scoped_file.as_ref(), &name)?,
            args.value_pattern.as_deref(),
            args.fixed_value,
        )?;
        if entries.is_empty() {
            let Some(default) = args.default.as_deref() else {
                return Err(CliError::Exit(1));
            };
            let value = if let Some(value_type) = value_type {
                format_config_default_value(&name, default, value_type)?
            } else {
                default.to_owned()
            };
            print_config_default_output(&args, &name, &value)?;
            return Ok(());
        }
        for entry in entries {
            let value = if let Some(value_type) = value_type {
                format_config_value(&name, &entry, value_type)?
            } else {
                entry.value.clone()
            };
            print_config_output_line(&entry, &value, args.show_origin, args.show_scope, args.null)?;
        }
        return Ok(());
    }

    let mut entries = filter_entries_by_value_pattern(
        matching_config_entries(&args, scoped_file.as_ref(), &name)?,
        args.value_pattern.as_deref(),
        args.fixed_value,
    )?;
    if value_type == Some(ConfigValueType::Path) {
        entries.retain(|entry| {
            entry
                .value
                .strip_prefix(":(optional)")
                .is_none_or(|path| Path::new(path).exists())
        });
    }
    let entry = entries.into_iter().last();
    match entry {
        Some(entry) => {
            let value = if let Some(value_type) = value_type {
                format_config_value(&name, &entry, value_type)?
            } else {
                entry.value.clone()
            };
            print_config_output_line(&entry, &value, args.show_origin, args.show_scope, args.null)?;
            Ok(())
        }
        None => {
            let Some(default) = args.default.as_deref() else {
                return Err(CliError::Exit(1));
            };
            let value = if let Some(value_type) = value_type {
                format_config_default_value(&name, default, value_type)?
            } else {
                default.to_owned()
            };
            print_config_default_output(&args, &name, &value)?;
            Ok(())
        }
    }
}

fn config_missing_set_value_error(name: &str) -> CliError {
    if let Some((key, value)) = name.split_once('=')
        && parse_config_name(key).is_ok()
    {
        return CliError::Stderr {
            code: 2,
            text: format!(
                "error: missing value to set to the variable '{name}'\n\
                 hint: did you mean 'git config set {key} {value}'?\n"
            ),
        };
    }
    let description = if parse_config_name(name).is_ok() {
        format!("the variable '{name}'")
    } else {
        format!("a variable with an invalid name '{name}'")
    };
    CliError::Stderr {
        code: 2,
        text: format!("error: missing value to set to {description}\n"),
    }
}

fn print_config_default_output(args: &ConfigArgs, name: &str, value: &str) -> Result<()> {
    let mut entry = parse_config_entry(name, value)?;
    entry.scope = ConfigScope::Command;
    entry.origin = "command line:".into();
    print_config_output_line(&entry, value, args.show_origin, args.show_scope, args.null)
}

fn print_config_output_line(
    entry: &ConfigEntry,
    value: &str,
    show_origin: bool,
    show_scope: bool,
    null: bool,
) -> Result<()> {
    if null {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        if show_scope {
            stdout.write_all(entry.scope.label().as_bytes())?;
            stdout.write_all(b"\0")?;
        }
        if show_origin {
            stdout.write_all(entry.origin.as_bytes())?;
            stdout.write_all(b"\0")?;
        }
        stdout.write_all(value.as_bytes())?;
        stdout.write_all(b"\0")?;
        return Ok(());
    }
    println!(
        "{}",
        format_config_output_line(entry, value, show_origin, show_scope)
    );
    Ok(())
}

fn format_config_null_list_value(entry: &ConfigEntry) -> String {
    if entry.implicit_bool {
        entry.name()
    } else {
        format!("{}\n{}", entry.name(), entry.value)
    }
}

fn format_config_list_value(entry: &ConfigEntry, value_type: ConfigValueType) -> Result<String> {
    if value_type == ConfigValueType::Path {
        let value = format_config_list_path_value(entry)?;
        return Ok(format!("{}={value}", entry.name()));
    }
    let value = format_config_value(&entry.name(), entry, value_type)?;
    Ok(format!("{}={value}", entry.name()))
}

fn format_config_list_path_value(entry: &ConfigEntry) -> Result<String> {
    if let Some(optional) = entry.value.strip_prefix(":(optional)") {
        if Path::new(optional).exists() {
            return Ok(optional.to_owned());
        }
        return Err(CliError::Exit(1));
    }
    format_config_path(&entry.value)
}

fn format_config_named_value(name: &str, value: &str, implicit_bool: bool, null: bool) -> String {
    if null {
        if implicit_bool {
            name.to_owned()
        } else {
            format!("{name}\n{value}")
        }
    } else if implicit_bool {
        name.to_owned()
    } else {
        format!("{name} {value}")
    }
}

fn scoped_config_entries(
    args: &ConfigArgs,
    scoped_file: Option<&PathBuf>,
) -> Result<Vec<ConfigEntry>> {
    if let Some(objectish) = args.blob.as_deref() {
        let repo = config_scope_repo(args)?;
        return parse_config_blob_entries(&repo, objectish, !args.no_includes);
    }
    if let Some(path) = scoped_file {
        if path.as_os_str() == "-" {
            if !args.no_includes {
                let repo = config_scope_repo(args).ok();
                return Ok(read_config_stdin_with_includes(repo.as_ref())?);
            }
            return Ok(read_config_stdin()?);
        }
        if args.includes && !args.no_includes {
            let repo = config_scope_repo(args).ok();
            return Ok(read_config_file_required_with_includes(
                path,
                config_scoped_file_scope(args),
                repo.as_ref(),
            )?);
        }
        if config_scoped_file_required(args) && args.default.is_none() {
            Ok(read_config_file_required(
                path,
                config_scoped_file_scope(args),
            )?)
        } else {
            Ok(read_config_file_scoped(
                path,
                config_scoped_file_scope(args),
            )?)
        }
    } else if args.worktree {
        let repo = config_scope_repo(args)?;
        ensure_worktree_config_scope(&repo)?;
        Ok(read_scoped_worktree_config_entries(&repo)?)
    } else if args.local {
        let repo = config_scope_repo(args)?;
        if args.includes && !args.no_includes {
            Ok(read_local_config_entries_with_includes(&repo)?)
        } else {
            Ok(read_local_config_entries(&repo)?)
        }
    } else {
        match config_scope_repo(args) {
            Ok(repo) if args.no_includes => Ok(read_config_entries_no_includes(&repo)?),
            Ok(repo) => Ok(read_config_entries(&repo)?),
            Err(CliError::Fatal { code: 128, .. }) if !args.local && !args.worktree => {
                Ok(read_config_entries_without_repo(args.no_includes)?)
            }
            Err(error) => Err(error),
        }
    }
}

fn config_scoped_file_required(args: &ConfigArgs) -> bool {
    args.file.is_some()
        || std::env::var_os("GIT_CONFIG").is_some()
        || (args.global && std::env::var_os("GIT_CONFIG_GLOBAL").is_some())
        || (args.system && std::env::var_os("GIT_CONFIG_SYSTEM").is_some())
}

fn config_scoped_file_scope(args: &ConfigArgs) -> ConfigScope {
    if args.global {
        ConfigScope::Global
    } else if args.system {
        ConfigScope::System
    } else {
        ConfigScope::Local
    }
}

fn matching_config_entries(
    args: &ConfigArgs,
    scoped_file: Option<&PathBuf>,
    name: &str,
) -> Result<Vec<ConfigEntry>> {
    let (section, subsection, key) = parse_config_name(name).map_err(|_| CliError::Fatal {
        code: 1,
        message: format!("invalid config key: {name}"),
    })?;
    let entries = scoped_config_entries(args, scoped_file)?;
    Ok(entries
        .into_iter()
        .filter(|entry| {
            entry.section == section && entry.subsection == subsection && entry.key == key
        })
        .collect())
}

fn matching_config_entries_regexp(
    args: &ConfigArgs,
    scoped_file: Option<&PathBuf>,
    pattern: &str,
) -> Result<Vec<ConfigEntry>> {
    let name_regex = Regex::new(pattern).map_err(|err| CliError::Fatal {
        code: 1,
        message: err.to_string(),
    })?;
    Ok(scoped_config_entries(args, scoped_file)?
        .into_iter()
        .filter(|entry| name_regex.is_match(entry.name().as_bytes()))
        .collect())
}

fn config_file_scope_path(args: &ConfigArgs) -> Result<Option<PathBuf>> {
    let scope_count = [
        args.blob.is_some(),
        args.file.is_some(),
        args.global,
        args.local,
        args.system,
        args.worktree,
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if scope_count > 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "only one config file scope may be specified".into(),
        });
    }
    if let Some(path) = args.file.as_ref() {
        return Ok(Some(path.clone()));
    }
    if args.local || args.worktree {
        return Ok(None);
    }
    if let Some(path) = std::env::var_os("GIT_CONFIG") {
        return Ok(Some(normalize_windows_input_path(PathBuf::from(path))));
    }
    if args.global {
        return Ok(Some(
            global_config_path_for_write().ok_or(CliError::Exit(1))?,
        ));
    }
    if args.system {
        return Ok(Some(explicit_system_config_path()));
    }
    Ok(None)
}

fn config_get_urlmatch(
    args: &ConfigArgs,
    scoped_file: Option<&PathBuf>,
    name: &str,
    url: &str,
) -> Result<()> {
    if args.all || args.regexp || args.value_pattern.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "--url= cannot be used with --all, --regexp or --value".into(),
        });
    }
    let entries = scoped_config_entries(args, scoped_file)?;
    let parts: Vec<&str> = name.split('.').collect();
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("invalid config key: {name}"),
        });
    }

    if parts.len() == 1 {
        let section = parts[0].to_ascii_lowercase();
        let mut emitted = false;
        let mut keys = std::collections::BTreeSet::new();
        for entry in &entries {
            if entry.section == section {
                keys.insert(entry.key.clone());
            }
        }
        for key in keys {
            if let Some(entry) = best_urlmatch_entry(&entries, &section, &key, url) {
                let display_name = format!("{section}.{}", entry.key);
                let value = if args.name_only {
                    display_name
                } else {
                    let formatted = if let Some(value_type) = config_value_type(args)? {
                        format_config_value(&display_name, entry, value_type)?
                    } else {
                        entry.value.clone()
                    };
                    format_config_named_value(
                        &display_name,
                        &formatted,
                        entry.implicit_bool && config_value_type(args)?.is_none(),
                        args.null,
                    )
                };
                print_config_output_line(
                    entry,
                    &value,
                    args.show_origin,
                    args.show_scope,
                    args.null,
                )?;
                emitted = true;
            }
        }
        if emitted {
            return Ok(());
        }
        return Err(CliError::Exit(1));
    }

    let section = parts[0].to_ascii_lowercase();
    let key = parts[parts.len() - 1].to_ascii_lowercase();
    let Some(entry) = best_urlmatch_entry(&entries, &section, &key, url) else {
        return Err(CliError::Exit(1));
    };
    if args.name_only {
        return Ok(());
    }
    let value = if let Some(value_type) = config_value_type(args)? {
        format_config_value(name, entry, value_type)?
    } else {
        entry.value.clone()
    };
    print_config_output_line(entry, &value, args.show_origin, args.show_scope, args.null)
}

fn best_urlmatch_entry<'a>(
    entries: &'a [ConfigEntry],
    section: &str,
    key: &str,
    url: &str,
) -> Option<&'a ConfigEntry> {
    let mut best: Option<&ConfigEntry> = None;
    let mut best_score = ConfigUrlMatchScore::default();
    for entry in entries {
        if entry.section != section || entry.key != key {
            continue;
        }
        if entry.subsection.is_empty() {
            if best.is_none() {
                best = Some(entry);
            }
            continue;
        }
        if let Some(score) = config_url_match_score(url, &entry.subsection)
            && (best.is_none() || score >= best_score)
        {
            best = Some(entry);
            best_score = score;
        }
    }
    best
}

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ConfigUrlMatchScore {
    exact_host: bool,
    host_literal_length: usize,
    path_length: usize,
    exact_user: bool,
}

struct ConfigUrlParts<'a> {
    scheme: &'a str,
    user: Option<&'a str>,
    host: &'a str,
    path: &'a str,
}

fn config_url_match_score(url: &str, subsection: &str) -> Option<ConfigUrlMatchScore> {
    let target = parse_config_url(url)?;
    let pattern = parse_config_url(subsection)?;
    if !target.scheme.eq_ignore_ascii_case(pattern.scheme) {
        return None;
    }
    let exact_host = !pattern.host.contains('*');
    if !config_url_host_matches(pattern.host, target.host) {
        return None;
    }
    if let Some(user) = pattern.user
        && target.user != Some(user)
    {
        return None;
    }
    let target_path = if target.path.is_empty() {
        "/"
    } else {
        target.path
    };
    let pattern_path = if pattern.path.is_empty() {
        "/"
    } else {
        pattern.path
    };
    if !target_path.starts_with(pattern_path)
        || (!pattern_path.ends_with('/')
            && target_path.len() > pattern_path.len()
            && target_path.as_bytes()[pattern_path.len()] != b'/')
    {
        return None;
    }
    Some(ConfigUrlMatchScore {
        exact_host,
        host_literal_length: pattern.host.bytes().filter(|byte| *byte != b'*').count(),
        path_length: pattern_path.len(),
        exact_user: pattern.user.is_some(),
    })
}

fn parse_config_url(url: &str) -> Option<ConfigUrlParts<'_>> {
    let (scheme, remainder) = url.split_once("://")?;
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    let path = &remainder[authority_end..];
    let (user, host) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(user, host)| (Some(user), host));
    if scheme.is_empty() || host.is_empty() {
        return None;
    }
    Some(ConfigUrlParts {
        scheme,
        user,
        host,
        path,
    })
}

fn config_url_host_matches(pattern: &str, host: &str) -> bool {
    let pattern_labels = pattern.split('.').collect::<Vec<_>>();
    let host_labels = host.split('.').collect::<Vec<_>>();
    pattern_labels.len() == host_labels.len()
        && pattern_labels
            .iter()
            .zip(host_labels)
            .all(|(pattern, host)| *pattern == "*" || pattern.eq_ignore_ascii_case(host))
}

fn config_target_path_for_write(
    args: &ConfigArgs,
    scoped_file: Option<&PathBuf>,
) -> Result<PathBuf> {
    if let Some(path) = scoped_file {
        if path.as_os_str() == "-" {
            return Err(CliError::Fatal {
                code: 128,
                message: "writing to stdin is not supported".into(),
            });
        }
        return Ok(path.clone());
    }
    let repo = config_scope_repo(args)?;
    if args.worktree {
        ensure_worktree_config_scope(&repo)?;
        return Ok(worktree_config_path_for_scope(&repo)?);
    }
    Ok(local_config_path(&repo)?)
}

fn config_scope_repo(args: &ConfigArgs) -> Result<GitRepo> {
    let repo = find_repo_or_bare().map_err(|error| match error {
        CliError::Fatal { code: 128, message } if message == "not a git repository" => {
            if args.local {
                CliError::Fatal {
                    code: 128,
                    message: "--local can only be used inside a git repository".into(),
                }
            } else if args.worktree {
                CliError::Fatal {
                    code: 128,
                    message: "--worktree can only be used inside a git repository".into(),
                }
            } else {
                CliError::Fatal {
                    code: 128,
                    message: "not in a git directory".into(),
                }
            }
        }
        other => other,
    })?;
    validate_repository_format(&repo).map_err(|error| match error {
        CliError::Fatal { message, .. } => CliError::Stderr {
            code: 1,
            text: format!("warning: {message}\n"),
        },
        other => other,
    })?;
    Ok(repo)
}

fn config_get_colorbool(args: &ConfigArgs, scoped_file: Option<&PathBuf>) -> Result<()> {
    let Some(name) = args.name.as_deref() else {
        return Err(CliError::Exit(1));
    };
    let tty = args
        .value
        .as_deref()
        .and_then(parse_git_bool)
        .unwrap_or(false);
    let entry = matching_config_entries(args, scoped_file, name)?
        .into_iter()
        .last();
    let Some(entry) = entry else {
        if tty {
            println!("false");
            return Ok(());
        }
        return Err(CliError::Exit(1));
    };
    let mode = entry.value.to_ascii_lowercase();
    let enabled = match mode.as_str() {
        "always" => true,
        "never" => false,
        "auto" => tty,
        _ => entry.bool_value().unwrap_or(false) && tty,
    };
    if tty {
        println!("{}", if enabled { "true" } else { "false" });
        return Ok(());
    }
    if enabled {
        Ok(())
    } else {
        Err(CliError::Exit(1))
    }
}

fn config_get_color(args: &ConfigArgs, scoped_file: Option<&PathBuf>) -> Result<()> {
    let name = args.name.as_deref().unwrap_or("");
    let value = if name.is_empty() {
        args.value.clone()
    } else {
        matching_config_entries(args, scoped_file, name)?
            .into_iter()
            .last()
            .map(|entry| entry.value)
            .or_else(|| args.value.clone())
    }
    .ok_or(CliError::Exit(1))?;
    write_config_color(&format_config_color(&value)?)
}

fn write_config_color(value: &str) -> Result<()> {
    std::io::stdout().lock().write_all(value.as_bytes())?;
    Ok(())
}

fn config_edit(args: &ConfigArgs, scoped_file: Option<&PathBuf>) -> Result<()> {
    if args.blob.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "editing a blob is not supported".into(),
        });
    }
    let path = config_target_path_for_write(args, scoped_file)?;
    let editor = std::env::var("GIT_EDITOR")
        .ok()
        .or_else(|| {
            find_repo_or_bare()
                .ok()
                .and_then(|repo| git_editor(&repo).ok().flatten())
        })
        .or_else(|| std::env::var("VISUAL").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vi".to_owned());
    let status = run_editor_command_with_path(&editor, &path)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: status.code().unwrap_or(1),
            message: "editor failed".into(),
        })
    }
}

fn filter_entries_by_value_pattern(
    entries: Vec<ConfigEntry>,
    value_pattern: Option<&str>,
    fixed_value: bool,
) -> Result<Vec<ConfigEntry>> {
    let Some(pattern) = value_pattern else {
        return Ok(entries);
    };
    if fixed_value {
        return Ok(entries
            .into_iter()
            .filter(|entry| entry.value == pattern)
            .collect());
    }
    let (invert, pattern) = pattern
        .strip_prefix('!')
        .map_or((false, pattern), |pattern| (true, pattern));
    let regex = Regex::new(pattern).map_err(|err| CliError::Fatal {
        code: 6,
        message: err.to_string(),
    })?;
    Ok(entries
        .into_iter()
        .filter(|entry| regex.is_match(entry.value.as_bytes()) != invert)
        .collect())
}

struct ConfigEditState {
    entries: Vec<ConfigEntry>,
    lines: Vec<String>,
}

struct ConfigLineEdit {
    start: usize,
    end: usize,
    replacement: Vec<String>,
}

impl ConfigEditState {
    fn read(path: &Path) -> Result<Self> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(CliError::Io(error)),
        };
        let lines = if content.is_empty() {
            Vec::new()
        } else {
            content.split_inclusive('\n').map(str::to_owned).collect()
        };
        Ok(Self {
            entries: read_config_file(path)?,
            lines,
        })
    }

    fn entry_span(&self, index: usize) -> std::ops::Range<usize> {
        let start = self.entries[index].line.unwrap_or(1).saturating_sub(1);
        let mut end = (start + 1).min(self.lines.len());
        while end > 0 && end < self.lines.len() && config_line_continues(&self.lines[end - 1]) {
            end += 1;
        }
        start..end
    }

    fn delete_edit(&self, index: usize) -> ConfigLineEdit {
        let span = self.entry_span(index);
        let replacement = self.inline_section_header(span.start);
        ConfigLineEdit {
            start: span.start,
            end: span.end,
            replacement,
        }
    }

    fn replace_edit(&self, index: usize, replacement: &ConfigEntry) -> ConfigLineEdit {
        let span = self.entry_span(index);
        let replacement = replacement.clone();
        let mut rendered = self.inline_section_header(span.start);
        rendered.push(render_config_entry_line(&replacement));
        ConfigLineEdit {
            start: span.start,
            end: span.end,
            replacement: rendered,
        }
    }

    fn inline_section_header(&self, line: usize) -> Vec<String> {
        let Some(raw) = self.lines.get(line) else {
            return Vec::new();
        };
        let trimmed = raw.trim_start();
        if !trimmed.starts_with('[') {
            return Vec::new();
        }
        let Some((header, rest)) = trimmed.split_once(']') else {
            return Vec::new();
        };
        if rest.trim().is_empty() || header.is_empty() {
            return Vec::new();
        }
        vec![format!("{header}]\n")]
    }

    fn insert(mut self, path: &Path, entry: &ConfigEntry) -> Result<()> {
        if let Some(index) = self.entries.iter().rposition(|candidate| {
            candidate.section == entry.section && candidate.subsection == entry.subsection
        }) {
            let line = self.entry_span(index).end;
            self.ensure_insertion_newline(line);
            return self.write(
                path,
                vec![ConfigLineEdit {
                    start: line,
                    end: line,
                    replacement: vec![render_config_entry_line(entry)],
                }],
            );
        }
        if let Some(line) = config_section_header_line(&self.lines, entry)? {
            self.ensure_insertion_newline(line);
            return self.write(
                path,
                vec![ConfigLineEdit {
                    start: line,
                    end: line,
                    replacement: vec![render_config_entry_line(entry)],
                }],
            );
        }
        if self.lines.last().is_some_and(|line| !line.ends_with('\n')) {
            self.lines.last_mut().expect("last line exists").push('\n');
        }
        let start = self.lines.len();
        let header = format_config_section_header(&entry.raw_section, &entry.subsection);
        self.write(
            path,
            vec![ConfigLineEdit {
                start,
                end: start,
                replacement: vec![format!("{header}\n"), render_config_entry_line(entry)],
            }],
        )
    }

    fn ensure_insertion_newline(&mut self, line: usize) {
        if let Some(previous) = line
            .checked_sub(1)
            .and_then(|index| self.lines.get_mut(index))
            && !previous.ends_with('\n')
        {
            previous.push('\n');
        }
    }

    fn write(mut self, path: &Path, mut edits: Vec<ConfigLineEdit>) -> Result<()> {
        reject_locked_config_for_edit(path).map_err(CliError::Io)?;
        self.apply_edits(&mut edits);
        fs::write(path, self.lines.concat()).map_err(CliError::Io)
    }

    fn write_pruning_empty_section(
        mut self,
        path: &Path,
        mut edits: Vec<ConfigLineEdit>,
        section: &str,
        subsection: &str,
    ) -> Result<()> {
        reject_locked_config_for_edit(path).map_err(CliError::Io)?;
        self.apply_edits(&mut edits);
        let mut removals = Vec::new();
        let mut index = 0usize;
        while index < self.lines.len() {
            if !config_header_matches(&self.lines[index], section, subsection)? {
                index += 1;
                continue;
            }
            let start = index;
            index += 1;
            let mut has_content = start.checked_sub(1).is_some_and(|previous| {
                self.lines[previous].trim_start().starts_with(['#', ';'])
                    && !previous
                        .checked_sub(1)
                        .is_some_and(|before| config_line_continues(&self.lines[before]))
            });
            while index < self.lines.len() && !config_line_starts_section(&self.lines[index]) {
                let trimmed = self.lines[index].trim();
                if !trimmed.is_empty() {
                    has_content = true;
                }
                index += 1;
            }
            if !has_content {
                removals.push(start..index);
            }
        }
        for removal in removals.into_iter().rev() {
            self.lines.drain(removal);
        }
        fs::write(path, self.lines.concat()).map_err(CliError::Io)
    }

    fn apply_edits(&mut self, edits: &mut [ConfigLineEdit]) {
        edits.sort_by(|left, right| right.start.cmp(&left.start));
        for edit in edits {
            self.lines
                .splice(edit.start..edit.end, edit.replacement.drain(..));
        }
    }
}

fn config_line_continues(line: &str) -> bool {
    let body = line.trim_end_matches(['\n', '\r']);
    body.as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn render_config_entry_line(entry: &ConfigEntry) -> String {
    let mut line = if entry.implicit_bool {
        format!("\t{}", entry.raw_key)
    } else {
        format!(
            "\t{} = {}",
            entry.raw_key,
            encode_config_value(&entry.value)
        )
    };
    if let Some(comment) = entry.comment.as_deref() {
        line.push_str(comment);
    }
    line.push('\n');
    line
}

fn config_section_header_line(lines: &[String], target: &ConfigEntry) -> Result<Option<usize>> {
    let mut insertion = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some(header) = trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        else {
            continue;
        };
        let (section, subsection) = parse_config_header_name(header)?;
        let old_style = !header.contains([' ', '"']) && header.contains('.');
        let subsection_matches = if old_style {
            subsection.eq_ignore_ascii_case(&target.subsection)
        } else {
            subsection == target.subsection
        };
        if section.eq_ignore_ascii_case(&target.section) && subsection_matches {
            insertion = Some(
                lines[index + 1..]
                    .iter()
                    .position(|line| config_line_starts_section(line))
                    .map_or(lines.len(), |offset| index + 1 + offset),
            );
        }
    }
    Ok(insertion)
}

fn config_append_value_in_file(path: &Path, name: &str, value: &str) -> Result<()> {
    let entry = parse_config_entry(name, value)?;
    ConfigEditState::read(path)?.insert(path, &entry)
}

fn config_set_value_in_file(
    path: &Path,
    name: &str,
    value: &str,
    replace_all: bool,
    value_pattern: Option<&str>,
    fixed_value: bool,
    comment: Option<&str>,
) -> Result<()> {
    let mut new_entry = parse_config_entry(name, value)?;
    new_entry.comment = comment.map(format_config_comment).transpose()?;
    let state = ConfigEditState::read(path)?;
    let key_indices = matching_key_indices(&state.entries, &new_entry);
    let matched_indices =
        matching_value_indices(&state.entries, &key_indices, value_pattern, fixed_value)?;

    if replace_all {
        if let Some((&last, rest)) = matched_indices.split_last() {
            let mut edits = rest
                .iter()
                .map(|index| state.delete_edit(*index))
                .collect::<Vec<_>>();
            edits.push(state.replace_edit(last, &new_entry));
            return state.write(path, edits);
        } else {
            return state.insert(path, &new_entry);
        }
    }

    if value_pattern.is_none() && key_indices.len() > 1 {
        return Err(config_set_multiple_values_error(name));
    }

    match matched_indices.as_slice() {
        [] => {
            if value_pattern.is_some() {
                return state.insert(path, &new_entry);
            } else if let [index] = key_indices.as_slice() {
                let edit = state.replace_edit(*index, &new_entry);
                return state.write(path, vec![edit]);
            } else {
                return state.insert(path, &new_entry);
            }
        }
        [index] => {
            let edit = state.replace_edit(*index, &new_entry);
            return state.write(path, vec![edit]);
        }
        _ => return Err(config_multi_value_warning(name)),
    }
}

fn config_unset_value_in_file(
    path: &Path,
    name: &str,
    remove_all: bool,
    value_pattern: Option<&str>,
    fixed_value: bool,
) -> Result<()> {
    let target = parse_config_entry(name, "")?;
    let state = ConfigEditState::read(path)?;
    let key_indices = matching_key_indices(&state.entries, &target);
    let matched_indices =
        matching_value_indices(&state.entries, &key_indices, value_pattern, fixed_value)?;

    if remove_all {
        if matched_indices.is_empty() {
            return Err(CliError::Exit(5));
        }
        let edits = matched_indices
            .into_iter()
            .map(|index| state.delete_edit(index))
            .collect();
        return state.write_pruning_empty_section(path, edits, &target.section, &target.subsection);
    }

    if value_pattern.is_none() && key_indices.len() > 1 {
        return Err(config_multi_value_warning(name));
    }
    if matched_indices.is_empty() {
        return Err(CliError::Exit(5));
    }
    let edit = state.delete_edit(matched_indices[0]);
    state.write_pruning_empty_section(path, vec![edit], &target.section, &target.subsection)
}

fn config_remove_section_in_file(path: &Path, name: &str) -> Result<()> {
    let (section, subsection) = parse_config_section_name(name)?;
    reject_locked_config_for_edit(path).map_err(CliError::Io)?;
    let content = fs::read_to_string(path).map_err(CliError::Io)?;
    let lines = content.split_inclusive('\n').collect::<Vec<_>>();
    let mut output = String::with_capacity(content.len());
    let mut index = 0usize;
    let mut removed = false;
    while index < lines.len() {
        if config_header_matches(lines[index], &section, &subsection)? {
            removed = true;
            index += 1;
            while index < lines.len() && !config_line_starts_section(lines[index]) {
                index += 1;
            }
            continue;
        }
        output.push_str(lines[index]);
        index += 1;
    }
    if !removed {
        return Err(CliError::Exit(128));
    }
    fs::write(path, output).map_err(CliError::Io)
}

fn config_line_starts_section(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

fn config_header_matches(line: &str, section: &str, subsection: &str) -> Result<bool> {
    let trimmed = line.trim_start();
    let Some(after_open) = trimmed.strip_prefix('[') else {
        return Ok(false);
    };
    let Some((header, _)) = after_open.split_once(']') else {
        return Ok(false);
    };
    let (candidate_section, candidate_subsection) = parse_config_header_name(header)?;
    Ok(candidate_section.eq_ignore_ascii_case(section) && candidate_subsection == subsection)
}

fn config_rename_section_in_file(path: &Path, old_name: &str, new_name: &str) -> Result<()> {
    let (old_section, old_subsection) = parse_config_section_name(old_name)?;
    let (new_section, new_subsection) = parse_config_section_name(new_name)?;
    reject_locked_config_for_edit(path).map_err(CliError::Io)?;
    let content = fs::read_to_string(path).map_err(CliError::Io)?;
    let mut out = String::with_capacity(content.len());
    let mut renamed = false;
    for (index, line) in content.split_inclusive('\n').enumerate() {
        let line_no = index + 1;
        let had_newline = line.ends_with('\n');
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body.len() > 524_288 {
            return Err(CliError::Stderr {
                code: 128,
                text: format!(
                    "error: refusing to work with overly long line in '{}' on line {line_no}\n",
                    display_config_path_for_error_for_edit(path)
                ),
            });
        }

        let trimmed = body.trim_start();
        if let Some(after_open) = trimmed.strip_prefix('[')
            && let Some((section_raw, rest)) = after_open.split_once(']')
        {
            let (section, subsection) = parse_config_header_name(section_raw)?;
            if section.eq_ignore_ascii_case(&old_section) && subsection == old_subsection {
                renamed = true;
                out.push_str(&format_config_section_header(&new_section, &new_subsection));
                let rest = rest.trim();
                if !rest.is_empty() {
                    out.push('\n');
                    out.push('\t');
                    out.push_str(rest);
                }
                if had_newline {
                    out.push('\n');
                }
                continue;
            }
        }

        out.push_str(body);
        if had_newline {
            out.push('\n');
        }
    }

    if !renamed {
        return Err(CliError::Exit(128));
    }
    fs::write(path, out).map_err(CliError::Io)?;
    Ok(())
}

fn format_config_section_header(section: &str, subsection: &str) -> String {
    if subsection.is_empty() {
        format!("[{section}]")
    } else {
        format!("[{section} \"{subsection}\"]")
    }
}

fn parse_config_header_name(raw: &str) -> Result<(String, String)> {
    let parsed = parse_config_section(raw);
    if !parsed.1.is_empty() || !parsed.0.contains('.') {
        return Ok(parsed);
    }
    let (section, subsection) = parse_config_section_name(&parsed.0)?;
    Ok((section, subsection))
}

fn reject_locked_config_for_edit(path: &Path) -> io::Result<()> {
    let lock_path = path.with_extension("lock");
    if lock_path.exists() {
        return Err(io::Error::other(format!(
            "could not lock config file {}",
            display_config_path_for_error_for_edit(path)
        )));
    }
    Ok(())
}

fn display_config_path_for_error_for_edit(path: &Path) -> String {
    if path.file_name().and_then(|name| name.to_str()) == Some("config")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            == Some(".git")
    {
        ".git/config".to_owned()
    } else {
        path.display().to_string()
    }
}

fn matching_key_indices(entries: &[ConfigEntry], target: &ConfigEntry) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| config_entry_key_matches(entry, target).then_some(index))
        .collect()
}

fn matching_value_indices(
    entries: &[ConfigEntry],
    indices: &[usize],
    value_pattern: Option<&str>,
    fixed_value: bool,
) -> Result<Vec<usize>> {
    let Some(pattern) = value_pattern else {
        return Ok(indices.to_vec());
    };
    if fixed_value {
        return Ok(indices
            .iter()
            .copied()
            .filter(|index| entries[*index].value == pattern)
            .collect());
    }
    let (invert, pattern) = pattern
        .strip_prefix('!')
        .map_or((false, pattern), |pattern| (true, pattern));
    let regex = Regex::new(pattern).map_err(|err| CliError::Fatal {
        code: 6,
        message: err.to_string(),
    })?;
    Ok(indices
        .iter()
        .copied()
        .filter(|index| regex.is_match(entries[*index].value.as_bytes()) != invert)
        .collect())
}

fn format_config_comment(message: &str) -> Result<String> {
    if message.contains(['\n', '\r']) {
        return Err(CliError::Fatal {
            code: 129,
            message: "comment must not contain linefeeds".into(),
        });
    }
    if message.is_empty() {
        return Ok(" # ".to_owned());
    }
    if message.starts_with('#') {
        return Ok(format!(" {message}"));
    }
    let trimmed = message.trim_start_matches([' ', '\t']);
    if trimmed.starts_with('#') {
        return Ok(message.to_owned());
    }
    Ok(format!(" # {message}"))
}

fn config_multi_value_warning(name: &str) -> CliError {
    CliError::Stderr {
        code: 5,
        text: format!("warning: {name} has multiple values\n"),
    }
}

fn config_set_multiple_values_error(name: &str) -> CliError {
    CliError::Stderr {
        code: 5,
        text: format!(
            "warning: {name} has multiple values\nerror: cannot overwrite multiple values with a single value\n       Use a regexp, --add or --replace-all to change {name}.\n"
        ),
    }
}

fn format_config_output_line(
    entry: &ConfigEntry,
    value: &str,
    show_origin: bool,
    show_scope: bool,
) -> String {
    let mut out = String::new();
    if show_scope {
        out.push_str(entry.scope.label());
        out.push('\t');
    }
    if show_origin {
        out.push_str(&entry.origin);
        out.push('\t');
    }
    out.push_str(value);
    out
}

fn config_value_type(args: &ConfigArgs) -> Result<Option<ConfigValueType>> {
    let mut current = None;
    for specifier in &args.type_specifiers {
        if specifier == "none" {
            current = None;
            continue;
        }
        let parsed = parse_config_value_type(specifier)?;
        match current {
            Some(existing) if existing != parsed => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: only one type at a time\n".into(),
                });
            }
            _ => current = Some(parsed),
        }
    }
    Ok(current)
}

fn parse_config_value_type(value_type: &str) -> Result<ConfigValueType> {
    match value_type {
        "bool" => Ok(ConfigValueType::Bool),
        "int" => Ok(ConfigValueType::Int),
        "bool-or-int" => Ok(ConfigValueType::BoolOrInt),
        "bool-or-str" => Ok(ConfigValueType::BoolOrStr),
        "path" => Ok(ConfigValueType::Path),
        "expiry-date" => Ok(ConfigValueType::ExpiryDate),
        "color" => Ok(ConfigValueType::Color),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unrecognized --type argument, {value_type}"),
        }),
    }
}

fn normalize_config_value(
    name: &str,
    value: &str,
    value_type: Option<ConfigValueType>,
) -> Result<String> {
    match value_type {
        Some(ConfigValueType::Bool) => normalize_config_bool(name, value),
        Some(ConfigValueType::Int) => normalize_config_int(name, value),
        Some(ConfigValueType::BoolOrInt) => {
            if config_bool_or_int_is_bool(value, false) {
                normalize_config_bool(name, value)
            } else {
                normalize_config_int(name, value)
            }
        }
        Some(ConfigValueType::BoolOrStr) => {
            if parse_git_bool(value).is_some() {
                normalize_config_bool(name, value)
            } else {
                Ok(value.to_owned())
            }
        }
        Some(ConfigValueType::Path) | Some(ConfigValueType::ExpiryDate) => Ok(value.to_owned()),
        Some(ConfigValueType::Color) => {
            validate_config_color(value)?;
            Ok(value.to_owned())
        }
        None => Ok(value.to_owned()),
    }
}

fn format_config_value(
    name: &str,
    entry: &ConfigEntry,
    value_type: ConfigValueType,
) -> Result<String> {
    match value_type {
        ConfigValueType::Bool => format_config_bool(name, entry),
        ConfigValueType::Int => normalize_config_int_read(name, &entry.value),
        ConfigValueType::BoolOrInt => {
            if config_bool_or_int_is_bool(&entry.value, entry.implicit_bool) {
                format_config_bool(name, entry)
            } else {
                normalize_config_int_read(name, &entry.value)
            }
        }
        ConfigValueType::BoolOrStr => {
            if entry.bool_value().is_some() {
                format_config_bool(name, entry)
            } else {
                Ok(entry.value.clone())
            }
        }
        ConfigValueType::Path => {
            if entry.implicit_bool {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "bad config value for '{}' in {} line {}",
                        name,
                        entry.origin,
                        entry.line.unwrap_or(0)
                    ),
                });
            }
            format_config_path(&entry.value)
        }
        ConfigValueType::ExpiryDate => format_config_expiry_date(name, &entry.value),
        ConfigValueType::Color => format_config_color(&entry.value),
    }
}

fn format_config_default_value(
    name: &str,
    value: &str,
    value_type: ConfigValueType,
) -> Result<String> {
    let result = match value_type {
        ConfigValueType::Bool => normalize_config_bool(name, value),
        ConfigValueType::Int => normalize_config_int_read(name, value),
        ConfigValueType::BoolOrInt => {
            if config_bool_or_int_is_bool(value, false) {
                normalize_config_bool(name, value)
            } else {
                normalize_config_int_read(name, value)
            }
        }
        ConfigValueType::BoolOrStr => {
            if parse_git_bool(value).is_some() {
                normalize_config_bool(name, value)
            } else {
                Ok(value.to_owned())
            }
        }
        ConfigValueType::Path => format_config_path(value),
        ConfigValueType::ExpiryDate => format_config_expiry_date(name, value),
        ConfigValueType::Color => format_config_color(value),
    };
    result.map_err(|error| {
        let detail = match error {
            CliError::Fatal { message, .. } => message,
            CliError::Stderr { text, .. } => text.trim().to_owned(),
            CliError::Message(message) => message,
            CliError::Io(error) => error.to_string(),
            CliError::Exit(_) => "invalid value".into(),
        };
        CliError::Fatal {
            code: 128,
            message: format!("failed to format default config value '{value}': {detail}"),
        }
    })
}

fn config_bool_or_int_is_bool(value: &str, implicit: bool) -> bool {
    implicit
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "" | "true" | "yes" | "on" | "false" | "no" | "off"
        )
}

fn format_config_bool(name: &str, entry: &ConfigEntry) -> Result<String> {
    let Some(parsed) = parse_config_bool_like_git(&entry.value, entry.implicit_bool) else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{}' for '{}'", entry.value, name),
        });
    };
    Ok(if parsed { "true" } else { "false" }.to_owned())
}

fn normalize_config_bool(name: &str, value: &str) -> Result<String> {
    let Some(parsed) = parse_config_bool_like_git(value, false) else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{value}' for '{name}'"),
        });
    };
    Ok(if parsed { "true" } else { "false" }.to_owned())
}

fn normalize_config_int(name: &str, value: &str) -> Result<String> {
    parse_config_int(value)
        .map(|value| value.to_string())
        .map_err(|reason| CliError::Fatal {
            code: 128,
            message: format!("bad numeric config value '{value}' for '{name}': {reason}"),
        })
}

fn normalize_config_int_read(name: &str, value: &str) -> Result<String> {
    parse_config_int(value)
        .map(|value| value.to_string())
        .map_err(|reason| CliError::Fatal {
            code: 128,
            message: format!(
                "bad numeric config value '{value}' for '{name}' in file .git/config: {reason}"
            ),
        })
}

fn parse_config_int(value: &str) -> std::result::Result<i64, &'static str> {
    if value.is_empty() {
        return Err("invalid unit");
    }
    let mut chars = value.chars();
    let Some(suffix) = chars.next_back() else {
        return Err("invalid unit");
    };
    let (number, multiplier) = match suffix {
        'k' | 'K' => (&value[..value.len() - suffix.len_utf8()], 1024_i64),
        'm' | 'M' => (&value[..value.len() - suffix.len_utf8()], 1024_i64 * 1024),
        'g' | 'G' => (
            &value[..value.len() - suffix.len_utf8()],
            1024_i64 * 1024 * 1024,
        ),
        ch if ch.is_ascii_digit() => (value, 1),
        _ => return Err("invalid unit"),
    };
    if number.is_empty() || number == "-" || number == "+" {
        return Err("invalid unit");
    }
    let parsed = number.parse::<i64>().map_err(|_| "invalid unit")?;
    parsed.checked_mul(multiplier).ok_or("out of range")
}

fn format_config_path(value: &str) -> Result<String> {
    if let Some(optional) = value.strip_prefix(":(optional)") {
        if Path::new(optional).exists() {
            return Ok(optional.to_owned());
        }
        return Err(CliError::Exit(1));
    }
    let Some(rest) = value.strip_prefix("~/") else {
        return Ok(value.to_owned());
    };
    let home = config_home_dir().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "failed to expand user dir in: '~/': $HOME is unset".into(),
    })?;
    Ok(format_config_path_output(
        Path::new(&home).join(rest).display().to_string(),
    ))
}

fn format_config_path_output(value: String) -> String {
    #[cfg(windows)]
    {
        return value.replace('\\', "/");
    }
    #[cfg(not(windows))]
    {
        value
    }
}

fn config_home_dir() -> Option<String> {
    if let Ok(home) = std::env::var("HOME") {
        return Some(home);
    }
    #[cfg(windows)]
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        return Some(user_profile);
    }
    None
}

fn parse_config_bool_like_git(value: &str, implicit_bool: bool) -> Option<bool> {
    if implicit_bool {
        return Some(true);
    }
    if let Some(parsed) = parse_git_bool(value) {
        return Some(parsed);
    }
    if let Ok(parsed) = parse_config_int(value) {
        return Some(parsed != 0);
    }
    value.trim().parse::<i64>().ok().map(|parsed| parsed != 0)
}

fn format_config_expiry_date(name: &str, value: &str) -> Result<String> {
    let timestamp = parse_config_expiry_date(value).ok_or_else(|| CliError::Stderr {
        code: 128,
        text: format!(
            "error: '{value}' for '{name}' is not a valid timestamp\nfatal: bad config line in file .git/config\n"
        ),
    })?;
    Ok(timestamp.to_string())
}

fn parse_config_expiry_date(value: &str) -> Option<u64> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "never" => return Some(0),
        "now" => return Some(u64::MAX),
        _ => {}
    }
    if let Ok(timestamp) = normalized.parse::<u64>() {
        return Some(timestamp);
    }
    if let Some(timestamp) = parse_relative_config_expiry_date(&normalized) {
        return Some(timestamp);
    }
    if let Some(timestamp) = parse_compact_relative_config_expiry_date(&normalized) {
        return Some(timestamp);
    }
    if let Some(timestamp) = parse_dotted_relative_config_expiry_date(&normalized) {
        return Some(timestamp);
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(value.trim()) {
        return u64::try_from(datetime.timestamp()).ok();
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .and_then(|datetime| u64::try_from(datetime.and_utc().timestamp()).ok());
    }
    if let Ok(datetime) = chrono::NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S") {
        return u64::try_from(datetime.and_utc().timestamp()).ok();
    }
    if let Ok(datetime) =
        chrono::NaiveDateTime::parse_from_str(value.trim(), "%a %b %e %H:%M:%S %Y")
    {
        return u64::try_from(datetime.and_utc().timestamp()).ok();
    }
    for format in ["%Y/%m/%d %I:%M:%S%p", "%Y/%m/%d %I:%M:%S %p"] {
        if let Ok(datetime) = chrono::NaiveDateTime::parse_from_str(value.trim(), format) {
            return u64::try_from(datetime.and_utc().timestamp()).ok();
        }
    }
    None
}

fn parse_dotted_relative_config_expiry_date(normalized: &str) -> Option<u64> {
    let (relative, clock) = normalized.rsplit_once(' ')?;
    let time = chrono::NaiveTime::parse_from_str(clock, "%H:%M").ok()?;
    let mut tokens = relative.split('.').filter(|token| !token.is_empty());
    let mut days = 0_i64;
    while let Some(amount) = tokens.next() {
        let amount = amount.parse::<i64>().ok()?;
        let unit = tokens.next()?.trim_end_matches('s');
        days = days.checked_add(match unit {
            "week" => amount.checked_mul(7)?,
            "day" => amount,
            _ => return None,
        })?;
    }
    let date = crate::runtime::local_now()
        .date_naive()
        .checked_sub_signed(chrono::Duration::days(days))?;
    let local = date.and_time(time);
    let timestamp = crate::runtime::local_naive_datetime(local)?.timestamp();
    u64::try_from(timestamp).ok()
}

fn parse_relative_config_expiry_date(normalized: &str) -> Option<u64> {
    let suffix = " ago";
    let value = normalized.strip_suffix(suffix)?;
    let mut parts = value.split_whitespace();
    let amount = parts.next()?.parse::<u64>().ok()?;
    let unit = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let seconds = match unit.trim_end_matches('s') {
        "second" => 1,
        "minute" => 60,
        "hour" => 3_600,
        "day" => 86_400,
        "week" => 604_800,
        _ => return None,
    };
    let now = u64::try_from(current_unix_timestamp().ok()?).ok()?;
    Some(now.saturating_sub(amount.saturating_mul(seconds)))
}

fn parse_compact_relative_config_expiry_date(normalized: &str) -> Option<u64> {
    let split_at = normalized
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(normalized.len());
    if split_at == 0 || split_at == normalized.len() {
        return None;
    }
    let amount = normalized[..split_at].parse::<u64>().ok()?;
    let unit = &normalized[split_at..];
    let seconds = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return None,
    };
    let now = u64::try_from(current_unix_timestamp().ok()?).ok()?;
    Some(now.saturating_sub(amount.saturating_mul(seconds)))
}

fn validate_config_color(value: &str) -> Result<()> {
    if parse_config_color(value).is_some() {
        Ok(())
    } else {
        Err(CliError::Stderr {
            code: 128,
            text: format!(
                "error: invalid color value: {value}\nfatal: cannot parse color '{value}'\n"
            ),
        })
    }
}

fn format_config_color(value: &str) -> Result<String> {
    let sequence = parse_config_color(value).ok_or_else(|| CliError::Stderr {
        code: 128,
        text: format!("error: invalid color value: {value}\nfatal: cannot parse color '{value}'\n"),
    })?;
    Ok(sequence)
}

pub(crate) fn parse_config_color(value: &str) -> Option<String> {
    let mut reset_codes = Vec::new();
    let mut attribute_codes = Vec::new();
    let mut foreground_code = None::<String>;
    let mut background_code = None::<String>;
    let mut color_slots = 0_u8;
    for token in value.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "normal" => {}
            "reset" => reset_codes.push(String::new()),
            "bold" => attribute_codes.push("1".to_owned()),
            "dim" => attribute_codes.push("2".to_owned()),
            "italic" => attribute_codes.push("3".to_owned()),
            "ul" | "underline" => attribute_codes.push("4".to_owned()),
            "blink" => attribute_codes.push("5".to_owned()),
            "reverse" => attribute_codes.push("7".to_owned()),
            "strike" => attribute_codes.push("9".to_owned()),
            "nobold" | "no-bold" | "nodim" | "no-dim" => attribute_codes.push("22".to_owned()),
            "noitalic" | "no-italic" => attribute_codes.push("23".to_owned()),
            "noul" | "no-ul" | "nounderline" | "no-underline" => {
                attribute_codes.push("24".to_owned());
            }
            "noblink" | "no-blink" => attribute_codes.push("25".to_owned()),
            "noreverse" | "no-reverse" => attribute_codes.push("27".to_owned()),
            "nostrike" | "no-strike" => attribute_codes.push("29".to_owned()),
            color => {
                let color_code = parse_config_color_code(color, color_slots)?;
                if color_slots == 0 {
                    foreground_code = Some(color_code);
                } else {
                    background_code = Some(color_code);
                }
                color_slots = color_slots.saturating_add(1);
            }
        }
    }
    let mut codes = reset_codes;
    codes.extend(attribute_codes);
    if let Some(code) = foreground_code {
        codes.push(code);
    }
    if let Some(code) = background_code {
        codes.push(code);
    }
    if codes.is_empty() {
        return Some(String::new());
    }
    Some(format!("\x1b[{}m", codes.join(";")))
}

fn parse_config_color_code(token: &str, color_slots: u8) -> Option<String> {
    if color_slots >= 2 {
        return None;
    }
    if token.as_bytes().iter().all(|byte| byte.is_ascii_digit()) {
        return Some(token.to_owned());
    }
    let prefix = if color_slots == 0 { 30 } else { 40 };
    if let Some(index) = named_config_color_index(token) {
        return Some((prefix + index).to_string());
    }
    if let Some(index) = token
        .strip_prefix("bright")
        .and_then(named_config_color_index)
    {
        return Some((prefix + 60 + index).to_string());
    }
    if let Some(hex) = token.strip_prefix('#') {
        return parse_config_hex_color(hex, color_slots == 1);
    }
    None
}

fn named_config_color_index(token: &str) -> Option<u8> {
    match token {
        "black" => Some(0),
        "red" => Some(1),
        "green" => Some(2),
        "yellow" => Some(3),
        "blue" => Some(4),
        "magenta" => Some(5),
        "cyan" => Some(6),
        "white" => Some(7),
        _ => None,
    }
}

fn parse_config_hex_color(hex: &str, background: bool) -> Option<String> {
    if hex.len() != 6 || !hex.as_bytes().iter().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
    let slot = if background { 48 } else { 38 };
    Some(format!("{slot};2;{red};{green};{blue}"))
}

pub(crate) fn var(list: bool, variable: Option<&str>) -> Result<()> {
    let repo = find_repo()?;
    if list {
        if variable.is_some() {
            return Err(CliError::Stderr {
                code: 129,
                text: "usage: git var (-l | <variable>)\n".into(),
            });
        }
        for entry in read_config_entries(&repo)? {
            println!("{}", entry.list_line());
        }
        if let Ok(signature) = signature_from_identity(&repo, "GIT_COMMITTER") {
            println!("GIT_COMMITTER_IDENT={}", signature_line(&signature));
        }
        if let Ok(signature) = signature_from_identity(&repo, "GIT_AUTHOR") {
            println!("GIT_AUTHOR_IDENT={}", signature_line(&signature));
        }
        if let Some(editor) = git_editor(&repo)? {
            println!("GIT_EDITOR={editor}");
        }
        if let Some(editor) = git_sequence_editor(&repo)? {
            println!("GIT_SEQUENCE_EDITOR={editor}");
        }
        println!("GIT_PAGER={}", git_pager(&repo)?);
        println!("GIT_DEFAULT_BRANCH={}", default_branch_name(&repo)?);
        println!("GIT_SHELL_PATH={}", git_shell_path());
        if let Some(path) = git_attr_system_path() {
            println!("GIT_ATTR_SYSTEM={path}");
        }
        if let Some(path) = git_attr_global_path()? {
            println!("GIT_ATTR_GLOBAL={path}");
        }
        if let Some(path) = git_config_system_path() {
            println!("GIT_CONFIG_SYSTEM={path}");
        }
        for path in git_config_global_paths()? {
            println!("GIT_CONFIG_GLOBAL={}", git_var_path_output(&path));
        }
        return Ok(());
    }

    match variable {
        Some("GIT_AUTHOR_IDENT") => {
            println!(
                "{}",
                signature_line(&signature_from_strict_identity(&repo, "GIT_AUTHOR")?)
            );
            Ok(())
        }
        Some("GIT_COMMITTER_IDENT") => {
            println!(
                "{}",
                signature_line(&signature_from_strict_identity(&repo, "GIT_COMMITTER")?)
            );
            Ok(())
        }
        Some("GIT_DEFAULT_BRANCH") => {
            println!("{}", default_branch_name(&repo)?);
            Ok(())
        }
        Some("GIT_EDITOR") => print_optional_var(git_editor(&repo)?),
        Some("GIT_SEQUENCE_EDITOR") => print_optional_var(git_sequence_editor(&repo)?),
        Some("GIT_PAGER") => {
            println!("{}", git_pager(&repo)?);
            Ok(())
        }
        Some("GIT_SHELL_PATH") => {
            println!("{}", git_shell_path());
            Ok(())
        }
        Some("GIT_ATTR_SYSTEM") => print_optional_var(git_attr_system_path()),
        Some("GIT_ATTR_GLOBAL") => print_optional_var(git_attr_global_path()?),
        Some("GIT_CONFIG_SYSTEM") => print_optional_var(git_config_system_path()),
        Some("GIT_CONFIG_GLOBAL") => {
            for path in git_config_global_paths()? {
                println!("{}", git_var_path_output(&path));
            }
            Ok(())
        }
        _ => Err(CliError::Stderr {
            code: 129,
            text: "usage: git var (-l | <variable>)\n".into(),
        }),
    }
}

pub(crate) fn version(build_options: bool) -> Result<()> {
    write_git_compatible_version(std::io::stdout().lock(), build_options).map_err(CliError::Io)
}

fn print_optional_var(value: Option<String>) -> Result<()> {
    let Some(value) = value else {
        return Err(CliError::Exit(1));
    };
    println!("{value}");
    Ok(())
}
