use super::*;

pub(crate) fn config(args: ConfigArgs) -> Result<()> {
    let _includes = args.includes;
    let value_type = config_value_type(&args)?;
    if args.list {
        if args.get || args.unset || args.name.is_some() || args.value.is_some() {
            return Err(CliError::Fatal {
                code: 129,
                message: "--list cannot be combined with config get/set arguments".into(),
            });
        }
        let repo = find_repo()?;
        let entries = if args.worktree {
            ensure_worktree_config_scope(&repo)?;
            read_scoped_worktree_config_entries(&repo)?
        } else {
            read_config_entries(&repo)?
        };
        for entry in entries {
            println!(
                "{}",
                format_config_output_line(
                    &entry,
                    &entry.list_line(),
                    args.show_origin,
                    args.show_scope,
                )
            );
        }
        return Ok(());
    }

    let Some(name) = args.name else {
        return Err(CliError::Fatal {
            code: 129,
            message: "config key is required".into(),
        });
    };

    if args.unset {
        if args.get || args.value.is_some() || value_type.is_some() {
            return Err(CliError::Fatal {
                code: 129,
                message: "--unset cannot be combined with config get/set modifiers".into(),
            });
        }
        let repo = find_repo()?;
        if args.worktree {
            ensure_worktree_config_scope(&repo)?;
            return unset_worktree_config_value(&repo, &name);
        }
        return unset_config_value(&repo, &name);
    }

    if let Some(value) = args.value {
        if args.get {
            return Err(CliError::Fatal {
                code: 129,
                message: "--get cannot be combined with setting a config value".into(),
            });
        }
        let stored_value = normalize_config_value(&name, &value, value_type)?;
        let repo = find_repo()?;
        if args.worktree {
            ensure_worktree_config_scope(&repo)?;
            set_worktree_config_value(&repo, &name, &stored_value)?;
        } else {
            set_config_value(&repo, &name, &stored_value)?;
        }
        return Ok(());
    }

    let repo = find_repo()?;
    let entry = if args.worktree {
        ensure_worktree_config_scope(&repo)?;
        read_worktree_config_entry(&repo, &name)?
    } else {
        read_config_entry(&repo, &name)?
    };
    match entry {
        Some(entry) => {
            let value = if let Some(value_type) = value_type {
                format_config_value(&name, &entry, value_type)?
            } else {
                entry.value.clone()
            };
            println!(
                "{}",
                format_config_output_line(&entry, &value, args.show_origin, args.show_scope)
            );
            Ok(())
        }
        None => Err(CliError::Exit(1)),
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
    let shorthand_types = [
        args.bool_value,
        args.int_value,
        args.bool_or_int_value,
        args.bool_or_str_value,
        args.path_value,
        args.expiry_date_value,
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    if shorthand_types > 1 || (shorthand_types > 0 && args.value_type.is_some()) {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: only one type at a time\n".into(),
        });
    }
    let parsed = match args.value_type.as_deref() {
        Some("bool") => Some(ConfigValueType::Bool),
        Some("int") => Some(ConfigValueType::Int),
        Some("bool-or-int") => Some(ConfigValueType::BoolOrInt),
        Some("bool-or-str") => Some(ConfigValueType::BoolOrStr),
        Some("path") => Some(ConfigValueType::Path),
        Some("expiry-date") => Some(ConfigValueType::ExpiryDate),
        Some("color") => Some(ConfigValueType::Color),
        Some(value_type) => {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("unsupported config type '{value_type}'"),
            });
        }
        None if args.bool_value => Some(ConfigValueType::Bool),
        None if args.int_value => Some(ConfigValueType::Int),
        None if args.bool_or_int_value => Some(ConfigValueType::BoolOrInt),
        None if args.bool_or_str_value => Some(ConfigValueType::BoolOrStr),
        None if args.path_value => Some(ConfigValueType::Path),
        None if args.expiry_date_value => Some(ConfigValueType::ExpiryDate),
        None => None,
    };
    Ok(parsed)
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
            if parse_git_bool(value).is_some() {
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
            if entry.bool_value().is_some() {
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
        ConfigValueType::Path => format_config_path(&entry.value),
        ConfigValueType::ExpiryDate => format_config_expiry_date(name, &entry.value),
        ConfigValueType::Color => format_config_color(&entry.value),
    }
}

fn format_config_bool(name: &str, entry: &ConfigEntry) -> Result<String> {
    let Some(parsed) = entry.bool_value() else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{}' for '{}'", entry.value, name),
        });
    };
    Ok(if parsed { "true" } else { "false" }.to_owned())
}

fn normalize_config_bool(name: &str, value: &str) -> Result<String> {
    let Some(parsed) = parse_git_bool(value) else {
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
    None
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

fn parse_config_color(value: &str) -> Option<String> {
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
        println!("GIT_ATTR_SYSTEM={}", git_attr_system_path());
        println!("GIT_ATTR_GLOBAL={}", git_attr_global_path()?);
        for path in git_config_global_paths()? {
            println!("GIT_CONFIG_GLOBAL={}", git_var_path_output(&path));
        }
        return Ok(());
    }

    match variable {
        Some("GIT_AUTHOR_IDENT") => {
            println!(
                "{}",
                signature_line(&signature_from_identity(&repo, "GIT_AUTHOR")?)
            );
            Ok(())
        }
        Some("GIT_COMMITTER_IDENT") => {
            println!(
                "{}",
                signature_line(&signature_from_identity(&repo, "GIT_COMMITTER")?)
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
        Some("GIT_ATTR_SYSTEM") => {
            println!("{}", git_attr_system_path());
            Ok(())
        }
        Some("GIT_ATTR_GLOBAL") => {
            println!("{}", git_attr_global_path()?);
            Ok(())
        }
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
    println!("git version {}.skron", env!("CARGO_PKG_VERSION"));
    if build_options {
        println!("cpu: {}", std::env::consts::ARCH);
        println!("no commit associated with this build");
        println!(
            "sizeof-long: {}",
            std::mem::size_of::<std::os::raw::c_long>()
        );
        println!("sizeof-size_t: {}", std::mem::size_of::<usize>());
        println!("shell-path: {}", git_shell_path());
        println!("zlib: miniz_oxide");
        println!("SHA-1: skron-git-core");
        println!("SHA-256: skron-git-core");
    }
    Ok(())
}

fn print_optional_var(value: Option<String>) -> Result<()> {
    let Some(value) = value else {
        return Err(CliError::Exit(1));
    };
    println!("{value}");
    Ok(())
}
