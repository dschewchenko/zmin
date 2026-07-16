use super::*;

pub(crate) fn credential(operation: &str) -> Result<()> {
    if !matches!(operation, "fill" | "approve" | "reject") {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential (fill|approve|reject)".into(),
        });
    }
    let config_entries = credential_config_entries()?;
    let mut entries = normalize_credential_entries(
        read_credential_entries()?,
        credential_protect_protocol_from_entries(&config_entries),
    )?;
    let config = credential_config_for_entries(&entries, &config_entries)?;
    entries = credential_apply_config(entries, &config);
    match operation {
        "approve" => credential_approve_or_reject(entries, &config.helpers, "store"),
        "reject" => credential_approve_or_reject(entries, &config.helpers, "erase"),
        "fill" => credential_fill(entries, &config),
        _ => Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential (fill|approve|reject)".into(),
        }),
    }
}

fn read_credential_entries() -> Result<Vec<(String, String)>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    parse_credential_entries(&input)
}

fn parse_credential_entries(input: &str) -> Result<Vec<(String, String)>> {
    let mut entries = Vec::new();
    for line in input.split_terminator('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unable to parse credential line: {line}"),
            });
        };
        entries.push((key.to_owned(), value.to_owned()));
    }
    Ok(entries)
}

fn credential_fill(mut entries: Vec<(String, String)>, config: &CredentialConfig) -> Result<()> {
    if let Some(username) = config.username.as_deref()
        && credential_value(&entries, "username").is_none()
    {
        set_credential_entry(&mut entries, "username", username);
    }
    let username = credential_value(&entries, "username");
    let password = credential_value(&entries, "password");
    let protocol = credential_value(&entries, "protocol")
        .unwrap_or("")
        .to_owned();
    let host = credential_value(&entries, "host").unwrap_or("").to_owned();
    let path = credential_value(&entries, "path").unwrap_or("").to_owned();
    if protocol.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "refusing to work with credential missing protocol field".into(),
        });
    }
    if host.is_empty() && path.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "refusing to work with credential missing host field".into(),
        });
    }
    match (username, password) {
        (Some(_), Some(_)) => {
            let finalized =
                credential_finalize_fill_entries(&entries, &credential_capabilities(&entries));
            for (key, value) in finalized {
                println!("{key}={value}");
            }
            Ok(())
        }
        _ => {
            match credential_fill_from_helpers(&entries, &config.helpers)? {
                CredentialFillHelperOutcome::Complete(filled)
                | CredentialFillHelperOutcome::Handled(filled) => {
                    for (key, value) in filled {
                        println!("{key}={value}");
                    }
                    return Ok(());
                }
                CredentialFillHelperOutcome::Partial(filled) => {
                    entries = filled;
                }
                CredentialFillHelperOutcome::Unhandled => {}
            }
            let username = credential_value(&entries, "username");
            let password = credential_value(&entries, "password");
            match (username, password) {
                (None, _) => {
                    let value = credential_askpass(
                        &entries,
                        config,
                        &format!(
                            "Username for '{}': ",
                            credential_url(&protocol, None, &host, Some(&path))
                        ),
                    )?;
                    set_credential_entry(&mut entries, "username", &value);
                    credential_fill(entries, config)
                }
                (Some(username), None) => {
                    let value = credential_askpass(
                        &entries,
                        config,
                        &format!(
                            "Password for '{}': ",
                            credential_url_with_options(
                                &protocol,
                                Some(username),
                                &host,
                                Some(&path),
                                true,
                            )
                        ),
                    )?;
                    set_credential_entry(&mut entries, "password", &value);
                    credential_fill(entries, config)
                }
                _ => unreachable!(),
            }
        }
    }
}

fn credential_fill_from_helpers(
    entries: &[(String, String)],
    helpers: &[ConfiguredCredentialHelper],
) -> Result<CredentialFillHelperOutcome> {
    let request_capabilities = credential_capabilities(entries);
    let initial = credential_non_capability_entries(entries);
    let mut current = initial.clone();
    let mut helper_handled_response = false;
    for helper in helpers {
        let helper_request = credential_helper_request_entries(&current, &request_capabilities);
        let response = match helper {
            ConfiguredCredentialHelper::Store { file } => {
                credential_fill_from_store_helper(&helper_request, file.clone())?
            }
            ConfiguredCredentialHelper::Cache { timeout, socket } => {
                credential_fill_from_cache_helper(&helper_request, *timeout, socket.clone())?
            }
            ConfiguredCredentialHelper::External {
                program,
                args,
                shell_snippet,
            } => credential_fill_from_external_helper(
                &helper_request,
                program,
                args,
                *shell_snippet,
            )?,
        };
        if response.quit {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "credential helper '{}' told us to quit",
                    helper.display_name()
                ),
            });
        }
        if !response.entries.is_empty() {
            helper_handled_response = true;
        }
        credential_merge_fill_response(&mut current, &request_capabilities, response)?;
        if credential_response_is_complete(&current, &request_capabilities) {
            return Ok(CredentialFillHelperOutcome::Complete(
                credential_finalize_fill_entries(&current, &request_capabilities),
            ));
        }
    }
    if helper_handled_response && current == initial {
        Ok(CredentialFillHelperOutcome::Handled(
            credential_finalize_fill_entries(&current, &request_capabilities),
        ))
    } else if current != initial {
        Ok(CredentialFillHelperOutcome::Partial(current))
    } else {
        Ok(CredentialFillHelperOutcome::Unhandled)
    }
}

fn credential_fill_from_store_helper(
    entries: &[(String, String)],
    file: Option<PathBuf>,
) -> Result<CredentialHelperResponse> {
    for path in credential_store_read_paths(file)? {
        let rows = read_credential_store_rows_if_accessible(&path)?;
        for row in rows.iter().rev() {
            if credential_store_row_matches(row, entries) {
                let mut response = CredentialHelperResponse::default();
                response
                    .entries
                    .push(("username".to_owned(), row.username.clone()));
                response
                    .entries
                    .push(("password".to_owned(), row.password.clone()));
                return Ok(response);
            }
        }
    }
    Ok(CredentialHelperResponse::default())
}

fn credential_fill_from_cache_helper(
    entries: &[(String, String)],
    timeout: Option<u64>,
    socket: Option<PathBuf>,
) -> Result<CredentialHelperResponse> {
    let socket = credential_cache_socket_path(socket)?;
    #[cfg(unix)]
    {
        let response = credential_cache_request(&socket, timeout, "get", entries)?;
        return parse_credential_helper_response(&response);
    }
    #[cfg(not(unix))]
    {
        let _ = (timeout, socket, entries);
        Ok(CredentialHelperResponse::default())
    }
}

fn credential_fill_from_external_helper(
    entries: &[(String, String)],
    program: &str,
    args: &[String],
    shell_snippet: bool,
) -> Result<CredentialHelperResponse> {
    run_external_credential_helper(program, args, shell_snippet, "get", entries)
}

fn credential_approve_or_reject(
    entries: Vec<(String, String)>,
    helpers: &[ConfiguredCredentialHelper],
    action: &str,
) -> Result<()> {
    if action == "store" && !credential_should_store(&entries) {
        return Ok(());
    }
    for helper in helpers {
        match helper {
            ConfiguredCredentialHelper::Store { file } => match action {
                "store" => {
                    let path = credential_store_write_path(file.clone())?;
                    credential_store_store(&path, &entries)?;
                }
                "erase" => {
                    for path in credential_store_erase_paths(file.clone())? {
                        credential_store_erase(&path, &entries)?;
                    }
                }
                _ => {}
            },
            ConfiguredCredentialHelper::Cache { timeout, socket } => {
                let socket = credential_cache_socket_path(socket.clone())?;
                #[cfg(unix)]
                {
                    let _ = credential_cache_request(&socket, *timeout, action, &entries)?;
                }
                #[cfg(not(unix))]
                {
                    let _ = (timeout, socket);
                }
            }
            ConfiguredCredentialHelper::External {
                program,
                args,
                shell_snippet,
            } => {
                let _ = run_external_credential_helper(
                    program,
                    args,
                    *shell_snippet,
                    action,
                    entries.as_slice(),
                )?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CredentialHelperResponse {
    entries: Vec<(String, String)>,
    capabilities: Vec<String>,
    quit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfiguredCredentialHelper {
    Store {
        file: Option<PathBuf>,
    },
    Cache {
        timeout: Option<u64>,
        socket: Option<PathBuf>,
    },
    External {
        program: String,
        args: Vec<String>,
        shell_snippet: bool,
    },
}

impl ConfiguredCredentialHelper {
    fn display_name(&self) -> &str {
        match self {
            ConfiguredCredentialHelper::Store { .. } => "store",
            ConfiguredCredentialHelper::Cache { .. } => "cache",
            ConfiguredCredentialHelper::External { program, .. } => program
                .strip_prefix("git-credential-")
                .unwrap_or(program.as_str()),
        }
    }
}

enum CredentialFillHelperOutcome {
    Complete(Vec<(String, String)>),
    Partial(Vec<(String, String)>),
    Handled(Vec<(String, String)>),
    Unhandled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CredentialConfig {
    helpers: Vec<ConfiguredCredentialHelper>,
    username: Option<String>,
    use_http_path: bool,
    askpass: Option<String>,
    protect_protocol: bool,
}

fn configured_credential_helpers(entries: &[ConfigEntry]) -> Vec<ConfiguredCredentialHelper> {
    let mut helpers = Vec::new();
    for entry in entries {
        if entry.section != "credential" || !entry.subsection.is_empty() || entry.key != "helper" {
            continue;
        }
        if entry.value.is_empty() {
            helpers.clear();
            continue;
        }
        if let Ok(Some(helper)) = parse_configured_credential_helper(&entry.value) {
            helpers.push(helper);
        }
    }
    helpers
}

fn credential_config_entries() -> Result<Vec<ConfigEntry>> {
    match find_repo() {
        Ok(repo) => read_config_entries(&repo).map_err(CliError::Io),
        Err(_) => read_protected_config_entries(),
    }
}

fn credential_config_for_entries(
    entries: &[(String, String)],
    config_entries: &[ConfigEntry],
) -> Result<CredentialConfig> {
    let protect_protocol = credential_protect_protocol_from_entries(config_entries);
    let normalized_url = credential_match_url(entries);
    let context = normalized_url
        .as_deref()
        .map(|url| parse_credential_context_url(url, protect_protocol))
        .transpose()?;
    let mut helpers = Vec::new();
    let mut username = None;
    let mut use_http_path = false;
    let mut askpass = None;
    let mut protect_protocol = protect_protocol;
    for entry in config_entries {
        if entry.section == "core" && entry.subsection.is_empty() && entry.key == "askpass" {
            askpass = credential_config_string_value(&entry.value);
            continue;
        }
        if entry.section != "credential" {
            continue;
        }
        if !entry.subsection.is_empty() {
            let Some(context) = context.as_ref() else {
                continue;
            };
            let matches = match credential_subsection_matches_context(&entry.subsection, context) {
                Ok(matches) => matches,
                Err(_) => {
                    eprintln!(
                        "warning: skipping credential lookup for key: {}",
                        entry.name()
                    );
                    false
                }
            };
            if !matches {
                continue;
            }
        }
        match entry.key.as_str() {
            "helper" => {
                if entry.value.is_empty() {
                    helpers.clear();
                } else if let Ok(Some(helper)) = parse_configured_credential_helper(&entry.value) {
                    helpers.push(helper);
                }
            }
            "username" => username = credential_config_string_value(&entry.value),
            "usehttppath" => {
                if let Some(value) = entry.bool_value() {
                    use_http_path = value;
                }
            }
            "protectprotocol" if entry.subsection.is_empty() => {
                if let Some(value) = entry.bool_value() {
                    protect_protocol = value;
                }
            }
            _ => {}
        }
    }
    if helpers.is_empty() {
        helpers = configured_credential_helpers(config_entries);
    }
    Ok(CredentialConfig {
        helpers,
        username,
        use_http_path,
        askpass,
        protect_protocol,
    })
}

fn credential_protect_protocol_from_entries(entries: &[ConfigEntry]) -> bool {
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.section == "credential"
                && entry.subsection.is_empty()
                && entry.key == "protectprotocol"
        })
        .and_then(ConfigEntry::bool_value)
        .unwrap_or(true)
}

fn parse_configured_credential_helper(value: &str) -> Result<Option<ConfiguredCredentialHelper>> {
    let trimmed = value.trim();
    if let Some(script) = trimmed.strip_prefix('!') {
        let script = script.trim();
        if script.is_empty() {
            return Ok(None);
        }
        return Ok(Some(ConfiguredCredentialHelper::External {
            program: script.to_owned(),
            args: Vec::new(),
            shell_snippet: true,
        }));
    }
    let words = transport_commands::split_shell_words(value)?;
    let Some(name) = words.first().map(String::as_str) else {
        return Ok(None);
    };
    match name {
        "store" => {
            let mut file = None;
            let mut parts = words.into_iter().skip(1);
            while let Some(part) = parts.next() {
                if part == "--file" {
                    let Some(path) = parts.next() else {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "credential-store helper is missing --file path".into(),
                        });
                    };
                    file = Some(PathBuf::from(path));
                } else if let Some(path) = part.strip_prefix("--file=") {
                    file = Some(PathBuf::from(path));
                }
            }
            Ok(Some(ConfiguredCredentialHelper::Store { file }))
        }
        "cache" => {
            let mut timeout = None;
            let mut socket = None;
            let mut parts = words.into_iter().skip(1);
            while let Some(part) = parts.next() {
                if part == "--timeout" {
                    let Some(value) = parts.next() else {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "credential-cache helper is missing --timeout value".into(),
                        });
                    };
                    timeout = Some(parse_credential_cache_timeout_value(&value)?);
                } else if let Some(value) = part.strip_prefix("--timeout=") {
                    timeout = Some(parse_credential_cache_timeout_value(value)?);
                } else if part == "--socket" {
                    let Some(value) = parts.next() else {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: "credential-cache helper is missing --socket path".into(),
                        });
                    };
                    socket = Some(PathBuf::from(value));
                } else if let Some(value) = part.strip_prefix("--socket=") {
                    socket = Some(PathBuf::from(value));
                }
            }
            Ok(Some(ConfiguredCredentialHelper::Cache { timeout, socket }))
        }
        _ => {
            let mut argv = words;
            let program = argv.remove(0);
            let executable = if program.contains('/') || program.contains('\\') {
                program
            } else {
                format!("git-credential-{program}")
            };
            Ok(Some(ConfiguredCredentialHelper::External {
                program: executable,
                args: argv,
                shell_snippet: false,
            }))
        }
    }
}

fn parse_credential_cache_timeout_value(value: &str) -> Result<u64> {
    value.parse::<u64>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("credential-cache helper has invalid timeout '{value}'"),
    })
}

fn set_credential_entry(entries: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some((_, existing)) = entries.iter_mut().find(|(entry_key, _)| entry_key == key) {
        *existing = value.to_owned();
    } else {
        entries.push((key.to_owned(), value.to_owned()));
    }
}

fn credential_value<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find_map(|(entry_key, value)| (entry_key == key).then_some(value.as_str()))
}

fn credential_has_key(entries: &[(String, String)], key: &str) -> bool {
    entries.iter().any(|(entry_key, _)| entry_key == key)
}

fn credential_values<'a>(entries: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    entries
        .iter()
        .filter_map(|(entry_key, value)| (entry_key == key).then_some(value.as_str()))
        .collect()
}

fn credential_capabilities(entries: &[(String, String)]) -> Vec<String> {
    credential_values(entries, "capability[]")
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn credential_non_capability_entries(entries: &[(String, String)]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter(|(key, _)| key != "capability[]")
        .cloned()
        .collect()
}

fn credential_helper_request_entries(
    entries: &[(String, String)],
    capabilities: &[String],
) -> Vec<(String, String)> {
    capabilities
        .iter()
        .map(|capability| ("capability[]".to_owned(), capability.clone()))
        .chain(entries.iter().cloned())
        .collect()
}

fn parse_credential_helper_response(input: &str) -> Result<CredentialHelperResponse> {
    let entries = parse_credential_entries(input)?;
    let mut response = CredentialHelperResponse::default();
    for (key, value) in entries {
        match key.as_str() {
            "capability[]" => response.capabilities.push(value),
            "quit" if value == "1" => response.quit = true,
            _ => response.entries.push((key, value)),
        }
    }
    Ok(response)
}

fn credential_merge_fill_response(
    current: &mut Vec<(String, String)>,
    request_capabilities: &[String],
    response: CredentialHelperResponse,
) -> Result<()> {
    let helper_supports_authtype = response
        .capabilities
        .iter()
        .any(|capability| capability == "authtype");
    let helper_supports_state = response
        .capabilities
        .iter()
        .any(|capability| capability == "state");
    let caller_supports_authtype = request_capabilities
        .iter()
        .any(|capability| capability == "authtype");
    let caller_supports_state = request_capabilities
        .iter()
        .any(|capability| capability == "state");

    for (key, value) in response.entries {
        match key.as_str() {
            "authtype" | "credential" | "ephemeral" => {
                if helper_supports_authtype && caller_supports_authtype {
                    set_credential_entry(current, &key, &value);
                }
            }
            "state[]" => {
                if helper_supports_state && caller_supports_state {
                    current.push((key, value));
                }
            }
            _ => set_credential_entry(current, &key, &value),
        }
    }

    if credential_password_is_expired(current)? {
        unset_credential_entries(current, "password");
        unset_credential_entries(current, "password_expiry_utc");
    }
    Ok(())
}

fn credential_response_is_complete(
    entries: &[(String, String)],
    request_capabilities: &[String],
) -> bool {
    let has_password = credential_value(entries, "password").is_some();
    if credential_value(entries, "username").is_some() && has_password {
        return true;
    }
    let caller_supports_authtype = request_capabilities
        .iter()
        .any(|capability| capability == "authtype");
    caller_supports_authtype && credential_value(entries, "credential").is_some()
}

fn credential_finalize_fill_entries(
    entries: &[(String, String)],
    request_capabilities: &[String],
) -> Vec<(String, String)> {
    let has_authtype_response = credential_value(entries, "credential").is_some()
        && request_capabilities
            .iter()
            .any(|capability| capability == "authtype");
    let has_state_response = entries.iter().any(|(key, _)| key == "state[]")
        && request_capabilities
            .iter()
            .any(|capability| capability == "state");
    let mut out = Vec::new();
    if has_authtype_response {
        out.push(("capability[]".to_owned(), "authtype".to_owned()));
    }
    if has_state_response {
        out.push(("capability[]".to_owned(), "state".to_owned()));
    }
    if has_authtype_response {
        for key in ["authtype", "credential", "ephemeral"] {
            if let Some(value) = credential_value(entries, key) {
                out.push((key.to_owned(), value.to_owned()));
            }
        }
    }
    for key in [
        "protocol",
        "host",
        "path",
        "username",
        "password",
        "password_expiry_utc",
        "oauth_refresh_token",
    ] {
        if let Some(value) = credential_value(entries, key) {
            out.push((key.to_owned(), value.to_owned()));
        }
    }
    out.extend(entries.iter().filter_map(|(key, value)| {
        (!matches!(
            key.as_str(),
            "capability[]"
                | "protocol"
                | "host"
                | "path"
                | "username"
                | "password"
                | "password_expiry_utc"
                | "oauth_refresh_token"
                | "authtype"
                | "credential"
                | "ephemeral"
                | "state[]"
        ))
        .then_some((key.clone(), value.clone()))
    }));
    if has_state_response {
        out.extend(entries.iter().filter(|(key, _)| key == "state[]").cloned());
    }
    out
}

fn unset_credential_entries(entries: &mut Vec<(String, String)>, key: &str) {
    entries.retain(|(entry_key, _)| entry_key != key);
}

fn credential_should_store(entries: &[(String, String)]) -> bool {
    if credential_value(entries, "ephemeral") == Some("1") {
        return false;
    }
    if !credential_has_key(entries, "password") && !credential_has_key(entries, "credential") {
        return false;
    }
    !credential_password_is_expired(entries).unwrap_or(false)
}

fn credential_password_is_expired(entries: &[(String, String)]) -> Result<bool> {
    let Some(raw) = credential_value(entries, "password_expiry_utc") else {
        return Ok(false);
    };
    let Ok(expiry) = raw.parse::<u64>() else {
        return Ok(false);
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(expiry <= now)
}

fn run_external_credential_helper(
    program: &str,
    args: &[String],
    shell_snippet: bool,
    action: &str,
    entries: &[(String, String)],
) -> Result<CredentialHelperResponse> {
    let helper_action = match action {
        "get" | "store" | "erase" => action,
        other => other,
    };
    let mut input = String::new();
    for (key, value) in entries {
        input.push_str(key);
        input.push('=');
        input.push_str(value);
        input.push('\n');
    }

    let mut command = if shell_snippet {
        let mut command = ProcessCommand::new(crate::runtime::git_shell_command_path());
        command.arg("-c").arg(program).arg("zmin-credential-helper");
        command
    } else {
        let mut command = ProcessCommand::new(program);
        command.args(args);
        command
    };
    command
        .arg(helper_action)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !shell_snippet => {
            writeln!(
                io::stderr(),
                "git: '{}' is not a git command. See 'git --help'.",
                program.strip_prefix("git-").unwrap_or(program)
            )?;
            return Ok(CredentialHelperResponse::default());
        }
        Err(error) => return Err(CliError::Io(error)),
    };
    child
        .stdin
        .as_mut()
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "credential helper stdin is unavailable".into(),
        })?
        .write_all(input.as_bytes())?;
    let output = child.wait_with_output()?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        return Ok(CredentialHelperResponse::default());
    }
    parse_credential_helper_response(&String::from_utf8_lossy(&output.stdout))
}

fn credential_url(
    protocol: &str,
    username: Option<&str>,
    host: &str,
    path: Option<&str>,
) -> String {
    credential_url_with_options(protocol, username, host, path, false)
}

fn credential_url_with_options(
    protocol: &str,
    username: Option<&str>,
    host: &str,
    path: Option<&str>,
    encode_username: bool,
) -> String {
    let mut out = String::new();
    if !protocol.is_empty() {
        out.push_str(protocol);
        out.push_str("://");
    }
    if let Some(username) = username
        && !username.is_empty()
    {
        if encode_username {
            out.push_str(&percent_encode_credential_component(username));
        } else {
            out.push_str(username);
        }
        out.push('@');
    }
    out.push_str(host);
    if let Some(path) = path
        && !path.is_empty()
    {
        out.push('/');
        out.push_str(path);
    }
    out
}

fn normalize_credential_entries(
    entries: Vec<(String, String)>,
    protect_protocol: bool,
) -> Result<Vec<(String, String)>> {
    let mut normalized = entries;
    if let Some(url) = credential_value(&normalized, "url").map(str::to_owned) {
        let parsed = parse_credential_url(&url, protect_protocol)?;
        set_credential_entry(&mut normalized, "protocol", &parsed.protocol);
        set_credential_entry(&mut normalized, "host", &parsed.host);
        if let Some(username) = parsed.username.as_deref()
            && credential_value(&normalized, "username").is_none()
        {
            set_credential_entry(&mut normalized, "username", username);
        }
        if let Some(path) = parsed.path.as_deref() {
            set_credential_entry(&mut normalized, "path", path);
        }
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCredentialUrl {
    protocol: String,
    username: Option<String>,
    host: String,
    path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CredentialContextUrl {
    scheme: Option<String>,
    username: Option<String>,
    host: Option<String>,
    path: Option<String>,
}

fn parse_credential_url(value: &str, protect_protocol: bool) -> Result<ParsedCredentialUrl> {
    let context = parse_credential_context_url(value, protect_protocol)?;
    let protocol = context.scheme.unwrap_or_default();
    let username = context.username;
    let host = context.host.unwrap_or_default();
    let path = context.path;
    if path.as_deref().is_some_and(|path| path.contains('\n')) {
        eprintln!("warning: url contains a newline in its path component: {value}");
        return Err(CliError::Fatal {
            code: 128,
            message: format!("credential url cannot be parsed: {value}"),
        });
    }
    Ok(ParsedCredentialUrl {
        protocol,
        username,
        host,
        path,
    })
}

fn parse_credential_context_url(
    value: &str,
    protect_protocol: bool,
) -> Result<CredentialContextUrl> {
    if let Some(rest) = value.strip_prefix('/') {
        return Ok(CredentialContextUrl {
            scheme: None,
            username: None,
            host: None,
            path: Some(percent_decode_credential_url_component(rest)?),
        });
    }
    let (scheme, rest) = if let Some((protocol, rest)) = value.split_once("://") {
        (Some(protocol.to_owned()), rest)
    } else {
        (None, value)
    };
    let delimiter = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..delimiter];
    let tail = &rest[delimiter..];
    let (username, host) = if let Some((userinfo, host)) = authority.rsplit_once('@') {
        let username = userinfo
            .split_once(':')
            .map(|(user, _)| user)
            .unwrap_or(userinfo);
        (
            Some(percent_decode_credential_url_component(username)?),
            percent_decode_credential_url_component(host)?,
        )
    } else {
        (None, percent_decode_credential_url_component(authority)?)
    };
    let path = if let Some(stripped) = tail.strip_prefix('/') {
        (!stripped.is_empty()).then_some(percent_decode_credential_url_component(stripped)?)
    } else if !tail.is_empty() {
        Some(percent_decode_credential_url_component(tail)?)
    } else {
        None
    };
    if protect_protocol && host.contains('\r') {
        return Err(CliError::Fatal {
            code: 128,
            message: "credential value for host contains carriage return\nIf this is intended, set `credential.protectProtocol=false`".into(),
        });
    }
    if host.contains('\n') {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("credential url cannot be parsed: {value}"),
        });
    }
    Ok(CredentialContextUrl {
        scheme,
        username,
        host: (!host.is_empty()).then_some(host),
        path,
    })
}

fn percent_decode_credential_url_component(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("credential url percent escape is invalid: {value}"),
            })?;
            let low = *bytes.get(index + 2).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("credential url percent escape is invalid: {value}"),
            })?;
            out.push(
                decode_percent_hex_byte(high, low).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("credential url percent escape is invalid: {value}"),
                })?,
            );
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn decode_percent_hex_byte(high: u8, low: u8) -> Option<u8> {
    fn decode_nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    Some((decode_nibble(high)? << 4) | decode_nibble(low)?)
}

fn percent_encode_credential_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            out.push(ch);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

fn credential_match_url(entries: &[(String, String)]) -> Option<String> {
    if let Some(url) = credential_value(entries, "url") {
        return Some(url.to_owned());
    }
    let protocol = credential_value(entries, "protocol")?;
    let host = credential_value(entries, "host").unwrap_or("");
    let username = credential_value(entries, "username");
    let path = credential_value(entries, "path");
    Some(credential_url(protocol, username, host, path))
}

fn credential_subsection_matches_context(
    subsection: &str,
    context: &CredentialContextUrl,
) -> Result<bool> {
    let candidate = parse_credential_context_url(subsection, true)?;
    if let Some(scheme) = candidate.scheme.as_deref()
        && context.scheme.as_deref() != Some(scheme)
    {
        return Ok(false);
    }
    if let Some(username) = candidate.username.as_deref()
        && context.username.as_deref() != Some(username)
    {
        return Ok(false);
    }
    if let Some(host) = candidate.host.as_deref() {
        let Some(context_host) = context.host.as_deref() else {
            return Ok(false);
        };
        if !credential_host_matches(host, context_host) {
            return Ok(false);
        }
    }
    if let Some(path) = candidate.path.as_deref() {
        let context_path = context.path.as_deref().unwrap_or("");
        if !credential_path_prefix_matches(path, context_path) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn credential_host_matches(pattern: &str, value: &str) -> bool {
    pattern == value || wildcard_match_pathspec(pattern, value, false, true)
}

fn credential_path_prefix_matches(pattern: &str, value: &str) -> bool {
    value == pattern
        || value
            .strip_prefix(pattern)
            .is_some_and(|tail| tail.is_empty() || pattern.ends_with('/') || tail.starts_with('/'))
}

fn credential_config_string_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn credential_apply_config(
    mut entries: Vec<(String, String)>,
    config: &CredentialConfig,
) -> Vec<(String, String)> {
    unset_credential_entries(&mut entries, "url");
    let protocol = credential_value(&entries, "protocol").unwrap_or("");
    if matches!(protocol, "http" | "https") && !config.use_http_path {
        unset_credential_entries(&mut entries, "path");
    }
    entries
}

fn credential_askpass(
    entries: &[(String, String)],
    config: &CredentialConfig,
    prompt: &str,
) -> Result<String> {
    let Some(command) = std::env::var("GIT_ASKPASS")
        .ok()
        .or_else(|| config.askpass.clone())
    else {
        let protocol = credential_value(entries, "protocol").unwrap_or("");
        let host = credential_value(entries, "host").unwrap_or("");
        let username = credential_value(entries, "username");
        return Err(CliError::Fatal {
            code: 128,
            message: if username.is_some() {
                format!(
                    "could not read Password for '{}': Device not configured",
                    credential_url(protocol, username, host, credential_value(entries, "path"))
                )
            } else {
                format!(
                    "could not read Username for '{}': Device not configured",
                    credential_url(protocol, None, host, credential_value(entries, "path"))
                )
            },
        });
    };
    let output = std::process::Command::new(command)
        .arg(prompt)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(CliError::Io)?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: 128,
            message: "credential askpass helper failed".into(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_owned())
}

pub(crate) fn credential_store(file: Option<PathBuf>, action: &str) -> Result<()> {
    if !matches!(action, "get" | "store" | "erase") {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential-store [--file <path>] (get|store|erase)".into(),
        });
    }
    let entries = read_credential_entries()?;
    match action {
        "get" => credential_store_get(&credential_store_read_paths(file.clone())?, &entries),
        "store" => credential_store_store(&credential_store_write_path(file)?, &entries),
        "erase" => {
            for path in credential_store_erase_paths(file)? {
                credential_store_erase(&path, &entries)?;
            }
            Ok(())
        }
        _ => Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential-store [--file <path>] (get|store|erase)".into(),
        }),
    }
}

fn credential_store_home_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "credential-store requires HOME or --file".into(),
    })?;
    Ok(PathBuf::from(home).join(".git-credentials"))
}

fn credential_store_xdg_path() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(home).join("git").join("credentials"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "credential-store requires HOME or --file".into(),
    })?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("git")
        .join("credentials"))
}

fn credential_store_read_paths(file: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    if let Some(file) = file {
        return Ok(vec![file]);
    }
    Ok(vec![
        credential_store_home_path()?,
        credential_store_xdg_path()?,
    ])
}

fn credential_store_write_path(file: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(file) = file {
        return Ok(file);
    }
    let home = credential_store_home_path()?;
    let xdg = credential_store_xdg_path()?;
    if home.exists() || !xdg.exists() {
        Ok(home)
    } else {
        Ok(xdg)
    }
}

fn credential_store_erase_paths(file: Option<PathBuf>) -> Result<Vec<PathBuf>> {
    if let Some(file) = file {
        return Ok(vec![file]);
    }
    Ok(vec![
        credential_store_home_path()?,
        credential_store_xdg_path()?,
    ])
}

fn credential_store_get(paths: &[PathBuf], query: &[(String, String)]) -> Result<()> {
    for path in paths {
        let rows = read_credential_store_rows_if_accessible(path)?;
        for row in rows.iter().rev() {
            if credential_store_row_matches(row, query) {
                println!("username={}", row.username);
                println!("password={}", row.password);
                return Ok(());
            }
        }
    }
    Ok(())
}

fn credential_store_store(path: &std::path::Path, entries: &[(String, String)]) -> Result<()> {
    let row = CredentialStoreRow {
        protocol: credential_value(entries, "protocol")
            .unwrap_or("")
            .to_owned(),
        host: credential_value(entries, "host").unwrap_or("").to_owned(),
        path: credential_value(entries, "path").map(str::to_owned),
        username: credential_value(entries, "username")
            .unwrap_or("")
            .to_owned(),
        password: credential_value(entries, "password")
            .unwrap_or("")
            .to_owned(),
    };
    if row.protocol.is_empty() || row.host.is_empty() {
        return Ok(());
    }
    let mut rows = read_credential_store_rows_if_accessible(path)?;
    rows.retain(|existing| !credential_store_same_identity(existing, &row));
    rows.push(row);
    write_credential_store_rows(path, &rows)
}

fn credential_store_erase(path: &std::path::Path, query: &[(String, String)]) -> Result<()> {
    let existed = path.exists();
    let mut rows = read_credential_store_rows_if_accessible(path)?;
    rows.retain(|row| !credential_store_row_matches(row, query));
    if rows.is_empty() && !existed {
        return Ok(());
    }
    write_credential_store_rows(path, &rows)
}

pub(crate) fn credential_cache(
    timeout: Option<u64>,
    socket: Option<PathBuf>,
    daemon_internal: bool,
    action: Option<String>,
) -> Result<()> {
    if daemon_internal {
        return credential_cache_daemon(socket, timeout);
    }
    let action = action.ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "usage: git credential-cache [--timeout <n>] [--socket <path>] <action>".into(),
    })?;
    if !matches!(action.as_str(), "get" | "store" | "erase" | "exit") {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential-cache [--timeout <n>] [--socket <path>] <action>"
                .into(),
        });
    }
    let entries = if action == "exit" {
        Vec::new()
    } else {
        read_credential_entries()?
    };
    let socket = credential_cache_socket_path(socket)?;
    #[cfg(unix)]
    {
        credential_cache_send(&socket, timeout, &action, &entries)
    }
    #[cfg(not(unix))]
    {
        let _ = (timeout, socket, entries);
        Err(CliError::Fatal {
            code: 128,
            message: "credential-cache requires Unix-domain sockets on this platform".into(),
        })
    }
}

fn credential_cache_socket_path(socket: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(socket) = socket {
        return Ok(expand_credential_helper_path(&socket));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let legacy_dir = PathBuf::from(&home).join(".git-credential-cache");
        if fs::metadata(&legacy_dir)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            return Ok(legacy_dir.join("socket"));
        }
    }
    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(cache_home).join("git/credential/socket"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "credential-cache requires HOME, XDG_CACHE_HOME, or --socket".into(),
    })?;
    Ok(PathBuf::from(home).join(".cache/git/credential/socket"))
}

fn expand_credential_helper_path(path: &std::path::Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if raw == "~" {
            return home;
        }
        if let Some(rest) = raw.strip_prefix("~/") {
            return home.join(rest);
        }
        if let Some(rest) = raw.strip_prefix("$HOME/") {
            return home.join(rest);
        }
        if let Some(rest) = raw.strip_prefix("${HOME}/") {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

#[cfg(unix)]
fn credential_cache_send(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    match unix_stream_connect_with_long_path_support(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_request(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<String> {
    match unix_stream_connect_with_long_path_support(socket) {
        Ok(mut stream) => credential_cache_request_to_stream(&mut stream, action, timeout, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_request_to_stream(&mut stream, action, timeout, entries)
        }
    }
}

#[cfg(unix)]
fn credential_cache_send_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    print!("{response}");
    Ok(())
}

#[cfg(unix)]
fn credential_cache_request_to_stream(
    stream: &mut std::os::unix::net::UnixStream,
    action: &str,
    timeout: Option<u64>,
    entries: &[(String, String)],
) -> Result<String> {
    let mut request = String::new();
    request.push_str(action);
    request.push('\n');
    if let Some(timeout) = timeout {
        request.push_str("__zmin_timeout=");
        request.push_str(&timeout.to_string());
        request.push('\n');
    }
    for (key, value) in entries {
        request.push_str(key);
        request.push('=');
        request.push_str(value);
        request.push('\n');
    }
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(CliError::Io)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

#[cfg(unix)]
fn start_credential_cache_daemon(socket: &std::path::Path, timeout: Option<u64>) -> Result<()> {
    if let Some(parent) = socket.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        let _ = fs::remove_file(socket);
    }
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("credential-cache")
        .arg("--daemon-internal")
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(timeout) = timeout {
        command.arg("--timeout").arg(timeout.to_string());
    }
    command.spawn().map_err(CliError::Io)?;
    Ok(())
}

#[cfg(unix)]
fn connect_credential_cache_daemon(
    socket: &std::path::Path,
) -> Result<std::os::unix::net::UnixStream> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match unix_stream_connect_with_long_path_support(socket) {
            Ok(stream) => return Ok(stream),
            Err(err) if std::time::Instant::now() < deadline => {
                let _ = err;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(err) => return Err(CliError::Io(err)),
        }
    }
}

#[cfg(unix)]
fn credential_cache_daemon(socket: Option<PathBuf>, timeout: Option<u64>) -> Result<()> {
    let socket = credential_cache_socket_path(socket)?;
    if let Some(parent) = socket.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        fs::remove_file(&socket)?;
    }
    let listener = unix_listener_bind_with_long_path_support(&socket)?;
    let mut rows = Vec::new();
    let timeout = std::time::Duration::from_secs(timeout.unwrap_or(900));
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut request = String::new();
        stream.read_to_string(&mut request)?;
        let (action, entries, request_timeout) = parse_credential_cache_request(&request)?;
        let should_exit = action == "exit";
        let response = credential_cache_apply(
            &mut rows,
            &action,
            &entries,
            request_timeout
                .map(std::time::Duration::from_secs)
                .unwrap_or(timeout),
        );
        stream.write_all(response.as_bytes())?;
        if should_exit {
            break;
        }
    }
    let _ = fs::remove_file(socket);
    Ok(())
}

#[cfg(unix)]
fn unix_socket_path_limit() -> usize {
    let sample = libc::sockaddr_un {
        sun_len: 0,
        sun_family: 0,
        sun_path: [0; 104],
    };
    sample.sun_path.len()
}

#[cfg(unix)]
fn with_unix_socket_path_for_stdlib<T>(
    path: &std::path::Path,
    op: impl FnOnce(&std::path::Path) -> io::Result<T>,
) -> io::Result<T> {
    use std::os::unix::ffi::OsStrExt;

    if path.as_os_str().as_bytes().len() < unix_socket_path_limit() {
        return op(path);
    }

    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must be shorter than SUN_LEN",
        ));
    };
    let Some(file_name) = path.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must be shorter than SUN_LEN",
        ));
    };
    if file_name.as_bytes().len() >= unix_socket_path_limit() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must be shorter than SUN_LEN",
        ));
    }

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(parent)?;
    let result = op(std::path::Path::new(file_name));
    let restore_result = std::env::set_current_dir(original_dir);
    match (result, restore_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), Ok(())) => Err(err),
        (_, Err(err)) => Err(err),
    }
}

#[cfg(unix)]
fn unix_stream_connect_with_long_path_support(
    path: &std::path::Path,
) -> io::Result<std::os::unix::net::UnixStream> {
    with_unix_socket_path_for_stdlib(path, |socket_path| {
        std::os::unix::net::UnixStream::connect(socket_path)
    })
}

#[cfg(unix)]
fn unix_listener_bind_with_long_path_support(
    path: &std::path::Path,
) -> io::Result<std::os::unix::net::UnixListener> {
    with_unix_socket_path_for_stdlib(path, |socket_path| {
        std::os::unix::net::UnixListener::bind(socket_path)
    })
}

#[cfg(not(unix))]
fn credential_cache_daemon(_socket: Option<PathBuf>, _timeout: Option<u64>) -> Result<()> {
    Err(CliError::Fatal {
        code: 128,
        message: "credential-cache requires Unix-domain sockets on this platform".into(),
    })
}

#[cfg(unix)]
fn parse_credential_cache_request(
    input: &str,
) -> Result<(String, Vec<(String, String)>, Option<u64>)> {
    let Some((action, rest)) = input.split_once('\n') else {
        return Err(CliError::Fatal {
            code: 128,
            message: "credential-cache daemon request is missing action".into(),
        });
    };
    let mut timeout = None;
    let mut entries = Vec::new();
    for (key, value) in parse_credential_entries(rest)? {
        if key == "__zmin_timeout" {
            timeout = value.parse::<u64>().ok();
        } else {
            entries.push((key, value));
        }
    }
    Ok((action.to_owned(), entries, timeout))
}

#[derive(Debug, Clone)]
#[cfg(unix)]
struct CredentialCacheRow {
    entries: Vec<(String, String)>,
    expires_at: std::time::Instant,
}

#[cfg(unix)]
fn credential_cache_apply(
    rows: &mut Vec<CredentialCacheRow>,
    action: &str,
    entries: &[(String, String)],
    timeout: std::time::Duration,
) -> String {
    let now = std::time::Instant::now();
    rows.retain(|row| row.expires_at > now);
    match action {
        "get" => credential_cache_get(rows, entries),
        "store" => {
            rows.retain(|row| !credential_cache_same_identity(&row.entries, entries));
            rows.push(CredentialCacheRow {
                entries: entries.to_vec(),
                expires_at: now + timeout,
            });
            String::new()
        }
        "erase" => {
            rows.retain(|row| !credential_cache_matches(&row.entries, entries));
            String::new()
        }
        "exit" => String::new(),
        _ => String::new(),
    }
}

#[cfg(unix)]
fn credential_cache_get(rows: &[CredentialCacheRow], query: &[(String, String)]) -> String {
    for row in rows.iter().rev() {
        if credential_cache_matches(&row.entries, query) {
            let mut out = String::new();
            out.push_str("capability[]=authtype\n");
            let caller_supports_authtype = query
                .iter()
                .any(|(key, value)| key == "capability[]" && value == "authtype");
            for key in [
                "username",
                "password",
                "password_expiry_utc",
                "oauth_refresh_token",
            ] {
                if let Some(value) = credential_value(&row.entries, key) {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(value);
                    out.push('\n');
                }
            }
            if caller_supports_authtype {
                for key in ["authtype", "credential", "ephemeral"] {
                    if let Some(value) = credential_value(&row.entries, key) {
                        out.push_str(key);
                        out.push('=');
                        out.push_str(value);
                        out.push('\n');
                    }
                }
            }
            return out;
        }
    }
    String::new()
}

#[cfg(unix)]
fn credential_cache_same_identity(left: &[(String, String)], right: &[(String, String)]) -> bool {
    ["protocol", "host", "path", "username"]
        .iter()
        .all(|key| credential_value(left, key) == credential_value(right, key))
}

#[cfg(unix)]
fn credential_cache_matches(row: &[(String, String)], query: &[(String, String)]) -> bool {
    query
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "capability[]" | "wwwauth[]"))
        .all(|(key, value)| credential_value(row, key).is_some_and(|row_value| row_value == value))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CredentialStoreRow {
    protocol: String,
    host: String,
    path: Option<String>,
    username: String,
    password: String,
}

fn read_credential_store_rows(path: &std::path::Path) -> Result<Vec<CredentialStoreRow>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut rows = Vec::new();
    for line in content.split_terminator('\n') {
        if let Some(row) = parse_credential_store_row(line) {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn read_credential_store_rows_if_accessible(
    path: &std::path::Path,
) -> Result<Vec<CredentialStoreRow>> {
    match read_credential_store_rows(path) {
        Ok(rows) => Ok(rows),
        Err(CliError::Io(error))
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
            ) =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

fn parse_credential_store_row(line: &str) -> Option<CredentialStoreRow> {
    let (protocol, rest) = line.split_once("://")?;
    if protocol.is_empty() {
        return None;
    }
    let delimiter = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..delimiter];
    let tail = &rest[delimiter..];
    let (user_pass, host) = authority.rsplit_once('@')?;
    if host.is_empty() || host.contains('\r') {
        return None;
    }
    let (username, password) = user_pass.split_once(':')?;
    let path = if let Some(stripped) = tail.strip_prefix('/') {
        Some(stripped.to_owned())
    } else if tail.is_empty() {
        None
    } else {
        Some(tail.to_owned())
    };
    Some(CredentialStoreRow {
        protocol: protocol.to_owned(),
        host: host.to_owned(),
        path,
        username: username.to_owned(),
        password: password.to_owned(),
    })
}

fn write_credential_store_rows(path: &std::path::Path, rows: &[CredentialStoreRow]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = String::new();
    for row in rows {
        out.push_str(&credential_store_row_url(row));
        out.push('\n');
    }
    fs::write(path, out)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn credential_store_row_url(row: &CredentialStoreRow) -> String {
    let mut url = format!(
        "{}://{}:{}@{}",
        row.protocol, row.username, row.password, row.host
    );
    if let Some(path) = row.path.as_deref() {
        url.push('/');
        url.push_str(path);
    }
    url
}

fn credential_store_same_identity(left: &CredentialStoreRow, right: &CredentialStoreRow) -> bool {
    left.protocol == right.protocol
        && left.host == right.host
        && left.path == right.path
        && left.username == right.username
}

fn credential_store_row_matches(row: &CredentialStoreRow, query: &[(String, String)]) -> bool {
    for (key, value) in query {
        match key.as_str() {
            "protocol" if row.protocol != *value => return false,
            "host" if row.host != *value => return false,
            "path" if row.path.as_deref() != Some(value.as_str()) => return false,
            "username" if row.username != *value => return false,
            "password" if row.password != *value => return false,
            _ => {}
        }
    }
    true
}
