use super::*;

pub(crate) fn credential(operation: &str) -> Result<()> {
    if !matches!(operation, "fill" | "approve" | "reject") {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential (fill|approve|reject)".into(),
        });
    }
    let entries = read_credential_entries()?;
    match operation {
        "approve" | "reject" => Ok(()),
        "fill" => credential_fill(entries),
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

fn credential_fill(entries: Vec<(String, String)>) -> Result<()> {
    let username = credential_value(&entries, "username");
    let password = credential_value(&entries, "password");
    let protocol = credential_value(&entries, "protocol").unwrap_or("");
    let host = credential_value(&entries, "host").unwrap_or("");
    match (username, password) {
        (Some(_), Some(_)) => {
            for (key, value) in entries {
                println!("{key}={value}");
            }
            Ok(())
        }
        (None, _) => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "could not read Username for '{}': Device not configured",
                credential_url(protocol, None, host)
            ),
        }),
        (Some(username), None) => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "could not read Password for '{}': Device not configured",
                credential_url(protocol, Some(username), host)
            ),
        }),
    }
}

fn credential_value<'a>(entries: &'a [(String, String)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find_map(|(entry_key, value)| (entry_key == key).then_some(value.as_str()))
}

fn credential_url(protocol: &str, username: Option<&str>, host: &str) -> String {
    let mut out = String::new();
    if !protocol.is_empty() {
        out.push_str(protocol);
        out.push_str("://");
    }
    if let Some(username) = username
        && !username.is_empty()
    {
        out.push_str(username);
        out.push('@');
    }
    out.push_str(host);
    out
}

pub(crate) fn credential_store(file: Option<PathBuf>, action: &str) -> Result<()> {
    if !matches!(action, "get" | "store" | "erase") {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential-store [--file <path>] (get|store|erase)".into(),
        });
    }
    let entries = read_credential_entries()?;
    let path = credential_store_path(file)?;
    match action {
        "get" => credential_store_get(&path, &entries),
        "store" => credential_store_store(&path, &entries),
        "erase" => credential_store_erase(&path, &entries),
        _ => Err(CliError::Fatal {
            code: 129,
            message: "usage: git credential-store [--file <path>] (get|store|erase)".into(),
        }),
    }
}

fn credential_store_path(file: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(file) = file {
        return Ok(file);
    }
    let home = std::env::var_os("HOME").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "credential-store requires HOME or --file".into(),
    })?;
    Ok(PathBuf::from(home).join(".git-credentials"))
}

fn credential_store_get(path: &std::path::Path, query: &[(String, String)]) -> Result<()> {
    let rows = read_credential_store_rows(path)?;
    for row in rows.iter().rev() {
        if credential_store_row_matches(row, query) {
            if !row.username.is_empty() {
                println!("username={}", row.username);
            }
            if !row.password.is_empty() {
                println!("password={}", row.password);
            }
            break;
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
        username: credential_value(entries, "username")
            .unwrap_or("")
            .to_owned(),
        password: credential_value(entries, "password")
            .unwrap_or("")
            .to_owned(),
    };
    if row.protocol.is_empty()
        || row.host.is_empty()
        || row.username.is_empty()
        || row.password.is_empty()
    {
        return Ok(());
    }
    let mut rows = read_credential_store_rows(path)?;
    rows.retain(|existing| !credential_store_same_identity(existing, &row));
    rows.push(row);
    write_credential_store_rows(path, &rows)
}

fn credential_store_erase(path: &std::path::Path, query: &[(String, String)]) -> Result<()> {
    let mut rows = read_credential_store_rows(path)?;
    rows.retain(|row| !credential_store_row_matches(row, query));
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
        return Ok(socket);
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

#[cfg(unix)]
fn credential_cache_send(
    socket: &std::path::Path,
    timeout: Option<u64>,
    action: &str,
    entries: &[(String, String)],
) -> Result<()> {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(mut stream) => credential_cache_send_to_stream(&mut stream, action, entries),
        Err(_) => {
            start_credential_cache_daemon(socket, timeout)?;
            let mut stream = connect_credential_cache_daemon(socket)?;
            credential_cache_send_to_stream(&mut stream, action, entries)
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
        match std::os::unix::net::UnixStream::connect(socket) {
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
    let listener = std::os::unix::net::UnixListener::bind(&socket)?;
    let mut rows = Vec::new();
    let timeout = std::time::Duration::from_secs(timeout.unwrap_or(900));
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut request = String::new();
        stream.read_to_string(&mut request)?;
        let (action, entries) = parse_credential_cache_request(&request)?;
        let should_exit = action == "exit";
        let response = credential_cache_apply(&mut rows, &action, &entries, timeout);
        stream.write_all(response.as_bytes())?;
        if should_exit {
            break;
        }
    }
    let _ = fs::remove_file(socket);
    Ok(())
}

#[cfg(not(unix))]
fn credential_cache_daemon(_socket: Option<PathBuf>, _timeout: Option<u64>) -> Result<()> {
    Err(CliError::Fatal {
        code: 128,
        message: "credential-cache requires Unix-domain sockets on this platform".into(),
    })
}

#[cfg(unix)]
fn parse_credential_cache_request(input: &str) -> Result<(String, Vec<(String, String)>)> {
    let Some((action, rest)) = input.split_once('\n') else {
        return Err(CliError::Fatal {
            code: 128,
            message: "credential-cache daemon request is missing action".into(),
        });
    };
    Ok((action.to_owned(), parse_credential_entries(rest)?))
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
            if let Some(username) = credential_value(&row.entries, "username") {
                out.push_str("username=");
                out.push_str(username);
                out.push('\n');
            }
            if let Some(password) = credential_value(&row.entries, "password") {
                out.push_str("password=");
                out.push_str(password);
                out.push('\n');
            }
            out.push('\n');
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
        .filter(|(key, _)| key != "password" && key != "capability[]")
        .all(|(key, value)| credential_value(row, key).is_some_and(|row_value| row_value == value))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CredentialStoreRow {
    protocol: String,
    host: String,
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
    for line in content.lines() {
        if let Some(row) = parse_credential_store_row(line) {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn parse_credential_store_row(line: &str) -> Option<CredentialStoreRow> {
    let (protocol, rest) = line.split_once("://")?;
    let (user_pass, host) = rest.rsplit_once('@')?;
    let (username, password) = user_pass.split_once(':')?;
    Some(CredentialStoreRow {
        protocol: protocol.to_owned(),
        host: host.to_owned(),
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
    Ok(())
}

fn credential_store_row_url(row: &CredentialStoreRow) -> String {
    format!(
        "{}://{}:{}@{}",
        row.protocol, row.username, row.password, row.host
    )
}

fn credential_store_same_identity(left: &CredentialStoreRow, right: &CredentialStoreRow) -> bool {
    left.protocol == right.protocol && left.host == right.host && left.username == right.username
}

fn credential_store_row_matches(row: &CredentialStoreRow, query: &[(String, String)]) -> bool {
    for (key, value) in query {
        match key.as_str() {
            "protocol" if row.protocol != *value => return false,
            "host" if row.host != *value => return false,
            "username" if row.username != *value => return false,
            _ => {}
        }
    }
    true
}
