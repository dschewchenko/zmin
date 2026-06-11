use super::*;

pub(crate) fn am(patches: Vec<PathBuf>) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot apply patches with a dirty worktree".into(),
        });
    }
    let mails = read_am_mails(&patches)?;
    if mails.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "No mail patches supplied".into(),
        });
    }
    for mail in mails {
        apply_mail_patch(&repo, &store, &mail)?;
    }
    Ok(())
}

fn read_am_mails(paths: &[PathBuf]) -> Result<Vec<String>> {
    if paths.is_empty() {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        return Ok(split_am_mailbox(&input));
    }
    let mut mails = Vec::new();
    for path in paths {
        let input = fs::read_to_string(path)?;
        mails.extend(split_am_mailbox(&input));
    }
    Ok(mails)
}

fn split_am_mailbox(input: &str) -> Vec<String> {
    let mut mails = Vec::new();
    let mut current = Vec::new();
    for line in input.lines() {
        if line.starts_with("From ") && !current.is_empty() {
            mails.push(lines_with_final_newline(&current));
            current.clear();
            continue;
        }
        current.push(line.to_owned());
    }
    if !current.is_empty() {
        mails.push(lines_with_final_newline(&current));
    }
    mails
}

fn apply_mail_patch(repo: &GitRepo, store: &LooseObjectStore, mail: &str) -> Result<()> {
    let (headers, body) = split_mail_headers(mail);
    let header_map = parse_mail_headers(headers);
    let from = header_map.get("from").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "mail patch is missing From header".into(),
    })?;
    let subject = header_map
        .get("subject")
        .map(|value| clean_mail_subject(value, false, false))
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "mail patch is missing Subject header".into(),
        })?;
    let date = header_map.get("date").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "mail patch is missing Date header".into(),
    })?;
    let (author_name, author_email) = parse_mail_author(from);
    let (timestamp, timezone) = parse_mail_date(date)?;
    let author = Signature::new(author_name, author_email, timestamp, timezone)?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let (message_body, patch_text) = split_mail_body_patch(body);
    if patch_text.trim().is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "mail patch does not contain a diff".into(),
        });
    }
    let mut index = read_repo_index(repo)?;
    let options = patch_commands::ApplyOptions {
        check: false,
        cached: false,
        index: true,
        reverse: false,
        patches: Vec::new(),
    };
    for patch in patch_commands::parse_apply_patches(patch_text.as_bytes())? {
        let update = patch_commands::apply_file_patch(repo, store, &index, &patch, &options)?;
        patch_commands::write_apply_update(repo, store, &mut index, update, &options)?;
    }
    index.write_to_path(&repo.index_path)?;
    let tree = write_tree_from_index(store, &index)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let message = mail_commit_message(&subject, &message_body);
    let commit = CommitBuilder::new(tree, author, committer)
        .parent(head_id)
        .message(message.as_bytes().to_vec())?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    println!("Applying: {subject}");
    Ok(())
}

fn parse_mail_date(value: &str) -> Result<(i64, String)> {
    if let Ok((timestamp, timezone)) = parse_git_date(value) {
        return Ok((timestamp, timezone));
    }
    let parsed = chrono::DateTime::parse_from_rfc2822(value).map_err(|err| CliError::Fatal {
        code: 128,
        message: format!("mail patch has invalid Date header: {err}"),
    })?;
    Ok((parsed.timestamp(), parsed.format("%z").to_string()))
}

fn mail_commit_message(subject: &str, body: &str) -> String {
    let body = body.trim_end_matches('\n');
    if body.is_empty() {
        format!("{subject}\n")
    } else {
        format!("{subject}\n\n{body}\n")
    }
}

pub(crate) fn format_patch(
    output_directory: Option<PathBuf>,
    stdout: bool,
    one: bool,
    revs: Vec<String>,
) -> Result<()> {
    let _trace = phase_trace("format_patch");
    if stdout && output_directory.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "`format-patch --stdout` cannot be combined with --output-directory".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let revs = if revs.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        revs
    };
    let revs = {
        let _trace = phase_trace("format_patch.collect_revs");
        collect_rev_list_revs(&repo, &store, false, revs)?
    };
    let packed_store = store.packed_first();
    let commit_cache = CommitObjectCache::new(&packed_store);
    let mut commits = {
        let _trace = phase_trace("format_patch.collect_commits");
        collect_commit_objects_with_exclusions_cached(
            &repo,
            &store,
            &commit_cache,
            &revs,
            if one { Some(1) } else { None },
        )?
    };
    commits.reverse();
    let abbrev_len = default_abbrev_len(&store)?;
    let format_context = FormatPatchContext {
        repo: &repo,
        store: &store,
        abbrev_len,
        total: commits.len(),
    };
    let tree_cache = TreeObjectCache::new(&packed_store);
    let mut blob_cache = FormatPatchBlobCache::new(&store);
    if stdout {
        let mut out = io::BufWriter::new(io::stdout().lock());
        for (idx, entry) in commits.iter().enumerate() {
            let _trace = phase_trace("format_patch.emit_stdout_patch");
            if idx > 0 {
                out.write_all(b"\n")?;
            }
            write_format_patch_with_tree_diff_cached(
                &mut out,
                &format_context,
                FormatPatchEntry {
                    id: &entry.id,
                    commit: entry.commit.as_ref(),
                    number: idx + 1,
                },
                &tree_cache,
                format_patch_old_tree(&commit_cache, entry.commit.as_ref())?.as_ref(),
                &entry.commit.tree,
                &mut blob_cache,
            )?;
        }
        return Ok(());
    }

    let output_directory = output_directory.unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&output_directory)?;
    for (idx, entry) in commits.iter().enumerate() {
        let _trace = phase_trace("format_patch.emit_file_patch");
        let filename = format_patch_filename(idx + 1, &commit_subject(&entry.commit.message));
        let path = output_directory.join(filename);
        let mut file = io::BufWriter::new(fs::File::create(&path)?);
        write_format_patch_with_tree_diff_cached(
            &mut file,
            &format_context,
            FormatPatchEntry {
                id: &entry.id,
                commit: entry.commit.as_ref(),
                number: idx + 1,
            },
            &tree_cache,
            format_patch_old_tree(&commit_cache, entry.commit.as_ref())?.as_ref(),
            &entry.commit.tree,
            &mut blob_cache,
        )?;
        println!("{}", path.display());
    }
    Ok(())
}

fn format_patch_old_tree<S>(
    commit_cache: &CommitObjectCache<'_, S>,
    commit: &CommitObject,
) -> Result<Option<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    commit
        .parents
        .first()
        .map(|parent| {
            commit_cache
                .read_commit_links(parent)
                .map(|links| links.tree.clone())
                .map_err(CliError::from)
        })
        .transpose()
}

pub(crate) fn send_email(
    dump_aliases: bool,
    translate_aliases: bool,
    args: Vec<String>,
) -> Result<()> {
    if !args.is_empty() {
        return send_email_patches(args);
    }
    if dump_aliases == translate_aliases {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git send-email (--dump-aliases|--translate-aliases)".into(),
        });
    }
    let aliases = read_send_email_aliases()?;
    if dump_aliases {
        for name in aliases.keys() {
            println!("{name}");
        }
        return Ok(());
    }

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            println!();
            continue;
        }
        let translated = line
            .split_whitespace()
            .map(|word| {
                aliases
                    .get(word)
                    .cloned()
                    .unwrap_or_else(|| word.to_owned())
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("{translated}");
    }
    Ok(())
}

fn send_email_patches(paths: Vec<String>) -> Result<()> {
    let repo = find_repo()?;
    let smtp_server =
        read_config_value(&repo, "sendemail.smtpserver")?.ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "sendemail.smtpserver is required for SMTP patch sending".into(),
        })?;
    let smtp_port =
        read_config_value(&repo, "sendemail.smtpserverport")?.and_then(|value| value.parse().ok());
    let endpoint = parse_smtp_endpoint(
        &smtp_server,
        smtp_port,
        read_config_value(&repo, "sendemail.smtpencryption")?.as_deref(),
    )?;
    let from = read_config_value(&repo, "sendemail.from")?
        .or_else(|| read_config_value(&repo, "user.email").ok().flatten())
        .ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "sendemail.from or user.email is required".into(),
        })?;
    let recipients = read_multi_config_values("sendemail.to")?;
    if recipients.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "sendemail.to is required".into(),
        });
    }
    let mut client = SmtpClient::connect(&endpoint)?;
    client.ehlo()?;
    for path in paths {
        let mut message = fs::read(&path)?;
        ensure_send_email_headers(&mut message, &from, &recipients)?;
        client.send_message(&from, &recipients, &message)?;
        println!("OK. Log says:");
        println!("Server: {}", endpoint.host);
        println!("MAIL FROM:<{}>", smtp_addr(&from));
        for recipient in &recipients {
            println!("RCPT TO:<{}>", smtp_addr(recipient));
        }
    }
    client.quit()
}

fn ensure_send_email_headers(
    message: &mut Vec<u8>,
    from: &str,
    recipients: &[String],
) -> Result<()> {
    let text = String::from_utf8_lossy(message);
    let (headers, _) = split_mail_headers(&text);
    let header_map = parse_mail_headers(headers);
    let mut prefix = Vec::new();
    if !header_map.contains_key("from") {
        prefix.extend_from_slice(format!("From: {from}\n").as_bytes());
    }
    if !header_map.contains_key("to") {
        prefix.extend_from_slice(format!("To: {}\n", recipients.join(", ")).as_bytes());
    }
    if !prefix.is_empty() {
        prefix.extend_from_slice(message);
        *message = prefix;
    }
    if !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    Ok(())
}

#[derive(Clone)]
struct SmtpEndpoint {
    host: String,
    port: u16,
    tls: bool,
}

fn parse_smtp_endpoint(
    server: &str,
    port: Option<u16>,
    encryption: Option<&str>,
) -> Result<SmtpEndpoint> {
    let (rest, scheme_tls, default_port) = if let Some(rest) = server.strip_prefix("smtps://") {
        (rest, true, 465)
    } else if let Some(rest) = server.strip_prefix("smtp://") {
        (rest, false, 25)
    } else {
        (
            server,
            matches!(encryption, Some(value) if value.eq_ignore_ascii_case("ssl")),
            25,
        )
    };
    let rest = rest.trim_start_matches('/').trim_end_matches('/');
    let (host, parsed_port) = match rest.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            (host, port.parse::<u16>().ok())
        }
        _ => (rest, None),
    };
    Ok(SmtpEndpoint {
        host: host.to_owned(),
        port: port.or(parsed_port).unwrap_or(default_port),
        tls: scheme_tls || matches!(encryption, Some(value) if value.eq_ignore_ascii_case("ssl")),
    })
}

struct SmtpClient {
    stream: io::BufReader<Box<dyn NetworkStream>>,
}

impl SmtpClient {
    fn connect(endpoint: &SmtpEndpoint) -> Result<Self> {
        let mut client = Self {
            stream: io::BufReader::new(connect_network_stream(
                &endpoint.host,
                endpoint.port,
                endpoint.tls,
            )?),
        };
        client.expect_code(220)?;
        Ok(client)
    }

    fn ehlo(&mut self) -> Result<()> {
        self.write_line("EHLO localhost")?;
        self.expect_code(250)
    }

    fn send_message(&mut self, from: &str, recipients: &[String], message: &[u8]) -> Result<()> {
        self.write_line(&format!("MAIL FROM:<{}>", smtp_addr(from)))?;
        self.expect_code(250)?;
        for recipient in recipients {
            self.write_line(&format!("RCPT TO:<{}>", smtp_addr(recipient)))?;
            self.expect_code(250)?;
        }
        self.write_line("DATA")?;
        self.expect_code(354)?;
        self.write_data(message)?;
        self.expect_code(250)
    }

    fn quit(&mut self) -> Result<()> {
        self.write_line("QUIT")?;
        self.expect_code(221)
    }

    fn write_line(&mut self, line: &str) -> Result<()> {
        self.stream.get_mut().write_all(line.as_bytes())?;
        self.stream.get_mut().write_all(b"\r\n")?;
        self.stream.get_mut().flush()?;
        Ok(())
    }

    fn write_data(&mut self, message: &[u8]) -> Result<()> {
        for line in message.split_inclusive(|byte| *byte == b'\n') {
            if line.starts_with(b".") {
                self.stream.get_mut().write_all(b".")?;
            }
            self.stream.get_mut().write_all(line)?;
            if !line.ends_with(b"\n") {
                self.stream.get_mut().write_all(b"\r\n")?;
            }
        }
        self.stream.get_mut().write_all(b".\r\n")?;
        self.stream.get_mut().flush()?;
        Ok(())
    }

    fn expect_code(&mut self, expected: u16) -> Result<()> {
        loop {
            let mut line = String::new();
            self.stream.read_line(&mut line)?;
            if line.len() < 4 {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("malformed SMTP response: {}", line.trim_end()),
                });
            }
            let code = line[..3].parse::<u16>().unwrap_or(0);
            if code != expected {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("unexpected SMTP response: {}", line.trim_end()),
                });
            }
            if line.as_bytes().get(3) != Some(&b'-') {
                return Ok(());
            }
        }
    }
}

fn smtp_addr(value: &str) -> String {
    if let Some(start) = value.rfind('<')
        && let Some(end) = value[start + 1..].find('>')
    {
        return value[start + 1..start + 1 + end].trim().to_owned();
    }
    value.trim().to_owned()
}

fn read_send_email_aliases() -> Result<BTreeMap<String, String>> {
    let mut aliases = BTreeMap::new();
    let alias_type = read_multi_config_values("sendemail.aliasfiletype")?
        .pop()
        .unwrap_or_else(|| "mutt".into());
    for file in read_multi_config_values("sendemail.aliasesfile")? {
        let content = fs::read_to_string(&file)?;
        parse_send_email_alias_file(&content, &alias_type, &mut aliases);
    }
    Ok(aliases)
}

fn parse_send_email_alias_file(
    content: &str,
    alias_type: &str,
    aliases: &mut BTreeMap<String, String>,
) {
    match alias_type {
        "mutt" => parse_mutt_aliases(content, aliases),
        "mailrc" => parse_mailrc_aliases(content, aliases),
        "pine" => parse_pine_aliases(content, aliases),
        "elm" => parse_elm_aliases(content, aliases),
        "sendmail" => parse_sendmail_aliases(content, aliases),
        "gnus" => parse_gnus_aliases(content, aliases),
        _ => {}
    }
}

fn parse_mutt_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if parts.next() != Some("alias") {
            continue;
        }
        let mut name = parts.next();
        while name == Some("-group") {
            let _group = parts.next();
            name = parts.next();
        }
        let Some(name) = name else {
            continue;
        };
        let address = parts
            .collect::<Vec<_>>()
            .join(" ")
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .replace("\\\"", "\"");
        insert_alias(aliases, name, address);
    }
}

fn parse_mailrc_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("alias ") else {
            continue;
        };
        let mut parts = rest.splitn(2, char::is_whitespace);
        let Some(name) = parts.next() else {
            continue;
        };
        let address = parts.next().unwrap_or("").trim().replace('"', "");
        insert_alias(aliases, name, address);
    }
}

fn parse_pine_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        let address = fields[2]
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .to_owned();
        insert_alias(aliases, fields[0].trim(), address);
    }
}

fn parse_elm_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let fields = line.split('=').map(str::trim).collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        insert_alias(aliases, fields[0], fields[2].to_owned());
    }
}

fn parse_sendmail_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    let mut current = String::new();
    for line in content.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            current.push(' ');
            current.push_str(trimmed.trim());
            continue;
        }
        parse_sendmail_alias_line(&current, aliases);
        current.clear();
        current.push_str(trimmed);
    }
    parse_sendmail_alias_line(&current, aliases);
}

fn parse_sendmail_alias_line(line: &str, aliases: &mut BTreeMap<String, String>) {
    let Some((name, address)) = line.split_once(':') else {
        return;
    };
    insert_alias(aliases, name.trim(), address.trim().to_owned());
}

fn parse_gnus_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let Some(rest) = line.trim().strip_prefix("(define-mail-alias ") else {
            continue;
        };
        let values = rest
            .split('"')
            .skip(1)
            .step_by(2)
            .take(2)
            .collect::<Vec<_>>();
        if values.len() == 2 {
            insert_alias(aliases, values[0], values[1].to_owned());
        }
    }
}

fn insert_alias(aliases: &mut BTreeMap<String, String>, name: &str, address: String) {
    if !name.is_empty() && !address.trim().is_empty() {
        aliases.insert(name.to_owned(), address.trim().to_owned());
    }
}

pub(crate) struct ImapSendOptions {
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) folder: Option<String>,
    pub(crate) list: bool,
    pub(crate) curl: bool,
    pub(crate) no_curl: bool,
}

pub(crate) fn imap_send(options: ImapSendOptions) -> Result<()> {
    let repo = find_repo()?;
    let folder = options
        .folder
        .or(read_config_value(&repo, "imap.folder")?)
        .ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "no imap store specified".into(),
        })?;
    let host = read_config_value(&repo, "imap.host")?.ok_or_else(|| CliError::Fatal {
        code: 1,
        message: "no imap host specified".into(),
    })?;
    let user = read_config_value(&repo, "imap.user")?.unwrap_or_default();
    let pass = read_config_value(&repo, "imap.pass")?.unwrap_or_default();
    let port = read_config_value(&repo, "imap.port")?.and_then(|value| value.parse::<u16>().ok());
    let endpoint = parse_imap_endpoint(&host, port)?;
    let _ = (options.verbose, options.curl, options.no_curl);
    if options.list {
        return imap_list(&endpoint, &user, &pass);
    }

    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let messages = split_mbox_messages(&input, false)?;
    if !options.quiet {
        eprintln!(
            "sending {} message{}",
            messages.len(),
            if messages.len() == 1 { "" } else { "s" }
        );
    }
    imap_append_messages(&endpoint, &user, &pass, &folder, &messages)
}

struct ImapEndpoint {
    host: String,
    port: u16,
    tls: bool,
}

fn parse_imap_endpoint(host: &str, port: Option<u16>) -> Result<ImapEndpoint> {
    let (host, default_port, tls) = host
        .strip_prefix("imap://")
        .map(|value| (value, 143, false))
        .or_else(|| host.strip_prefix("imap:").map(|value| (value, 143, false)))
        .or_else(|| {
            host.strip_prefix("imaps://")
                .map(|value| (value, 993, true))
        })
        .or_else(|| host.strip_prefix("imaps:").map(|value| (value, 993, true)))
        .or_else(|| host.strip_prefix("//").map(|value| (value, 143, false)))
        .unwrap_or((host, 143, false));
    let host = host.trim_start_matches('/').trim_end_matches('/');
    let (host, parsed_port) = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            (name, port.parse::<u16>().ok())
        }
        _ => (host, None),
    };
    Ok(ImapEndpoint {
        host: host.to_owned(),
        port: port.or(parsed_port).unwrap_or(default_port),
        tls,
    })
}

fn imap_append_messages(
    endpoint: &ImapEndpoint,
    user: &str,
    pass: &str,
    folder: &str,
    messages: &[Vec<u8>],
) -> Result<()> {
    let mut client = ImapClient::connect(endpoint)?;
    client.login(user, pass)?;
    for message in messages {
        client.append(folder, message)?;
    }
    client.logout()
}

fn imap_list(endpoint: &ImapEndpoint, user: &str, pass: &str) -> Result<()> {
    let mut client = ImapClient::connect(endpoint)?;
    client.login(user, pass)?;
    for line in client.list()? {
        println!("{line}");
    }
    client.logout()
}

struct ImapClient {
    stream: io::BufReader<Box<dyn NetworkStream>>,
    sequence: usize,
}

impl ImapClient {
    fn connect(endpoint: &ImapEndpoint) -> Result<Self> {
        let mut client = Self {
            stream: io::BufReader::new(connect_network_stream(
                &endpoint.host,
                endpoint.port,
                endpoint.tls,
            )?),
            sequence: 0,
        };
        let greeting = client.read_line()?;
        if !greeting.starts_with("* OK") {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("unexpected IMAP greeting: {greeting}"),
            });
        }
        Ok(client)
    }

    fn login(&mut self, user: &str, pass: &str) -> Result<()> {
        if user.is_empty() && pass.is_empty() {
            return Ok(());
        }
        let tag = self.next_tag();
        write!(
            self.stream.get_mut(),
            "{tag} LOGIN {} {}\r\n",
            imap_quote(user),
            imap_quote(pass)
        )?;
        self.expect_tag_ok(&tag)
    }

    fn append(&mut self, folder: &str, message: &[u8]) -> Result<()> {
        let tag = self.next_tag();
        write!(
            self.stream.get_mut(),
            "{tag} APPEND {} {{{}}}\r\n",
            imap_quote(folder),
            message.len()
        )?;
        self.stream.get_mut().flush()?;
        let continuation = self.read_line()?;
        if !continuation.starts_with('+') {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("expected IMAP continuation, got: {continuation}"),
            });
        }
        self.stream.get_mut().write_all(message)?;
        self.stream.get_mut().write_all(b"\r\n")?;
        self.expect_tag_ok(&tag)
    }

    fn list(&mut self) -> Result<Vec<String>> {
        let tag = self.next_tag();
        write!(self.stream.get_mut(), "{tag} LIST \"\" \"*\"\r\n")?;
        self.stream.get_mut().flush()?;
        let mut rows = Vec::new();
        loop {
            let line = self.read_line()?;
            if line.starts_with(&format!("{tag} OK")) {
                return Ok(rows);
            }
            if let Some(row) = line.strip_prefix("* LIST ") {
                rows.push(row.to_owned());
            }
        }
    }

    fn logout(&mut self) -> Result<()> {
        let tag = self.next_tag();
        write!(self.stream.get_mut(), "{tag} LOGOUT\r\n")?;
        self.stream.get_mut().flush()?;
        loop {
            let line = self.read_line()?;
            if line.starts_with(&format!("{tag} OK")) {
                return Ok(());
            }
        }
    }

    fn expect_tag_ok(&mut self, tag: &str) -> Result<()> {
        self.stream.get_mut().flush()?;
        loop {
            let line = self.read_line()?;
            if line.starts_with(&format!("{tag} OK")) {
                return Ok(());
            }
            if line.starts_with(&format!("{tag} NO")) || line.starts_with(&format!("{tag} BAD")) {
                return Err(CliError::Fatal {
                    code: 1,
                    message: line,
                });
            }
        }
    }

    fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        self.stream.read_line(&mut line)?;
        Ok(line.trim_end_matches(['\r', '\n']).to_owned())
    }

    fn next_tag(&mut self) -> String {
        self.sequence += 1;
        format!("A{:04}", self.sequence)
    }
}

trait NetworkStream: Read + Write {}

impl<T: Read + Write> NetworkStream for T {}

fn connect_network_stream(host: &str, port: u16, tls: bool) -> Result<Box<dyn NetworkStream>> {
    if tls {
        return connect_tls_network_stream(host, port);
    }
    let stream = std::net::TcpStream::connect((host, port))?;
    Ok(Box::new(stream))
}

#[cfg(feature = "mail-tls")]
fn connect_tls_network_stream(host: &str, port: u16) -> Result<Box<dyn NetworkStream>> {
    let stream = std::net::TcpStream::connect((host, port))?;
    let connector = native_tls::TlsConnector::new().map_err(|error| CliError::Fatal {
        code: 1,
        message: format!("failed to create TLS connector: {error}"),
    })?;
    let stream = connector
        .connect(host, stream)
        .map_err(|error| CliError::Fatal {
            code: 1,
            message: format!("TLS connection failed: {error}"),
        })?;
    Ok(Box::new(stream))
}

#[cfg(not(feature = "mail-tls"))]
fn connect_tls_network_stream(_host: &str, _port: u16) -> Result<Box<dyn NetworkStream>> {
    Err(CliError::Fatal {
        code: 1,
        message: "TLS mail transport requires the 'mail-tls' build feature".into(),
    })
}

fn imap_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub(crate) fn interpret_trailers(options: InterpretTrailersOptions<'_>) -> Result<()> {
    if options.in_place && options.files.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "interpret-trailers --in-place requires file arguments".into(),
        });
    }
    if options.files.is_empty() {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        print!("{}", interpret_trailers_content(&input, &options)?);
        return Ok(());
    }
    for path in &options.files {
        let input = fs::read_to_string(path)?;
        let output = interpret_trailers_content(&input, &options)?;
        if options.in_place {
            fs::write(path, output)?;
        } else {
            print!("{output}");
        }
    }
    Ok(())
}

pub(crate) fn interpret_trailers_content(
    input: &str,
    options: &InterpretTrailersOptions<'_>,
) -> Result<String> {
    let placement = parse_trailer_placement(options.where_)?;
    let if_exists = parse_trailer_if_exists(options.if_exists)?;
    let if_missing = parse_trailer_if_missing(options.if_missing)?;
    let lines = split_text_lines(input);
    let divider_index = if options.no_divider {
        None
    } else {
        lines.iter().position(|line| trailer_divider_line(line))
    };
    let message_end = divider_index.unwrap_or(lines.len());
    let message = &lines[..message_end];
    let suffix = divider_index.map_or([].as_slice(), |index| &lines[index..]);
    let trailer_start = trailer_block_start(message);
    let had_existing_block = trailer_start < message.len();
    let prefix = if trailer_start < message.len() {
        &message[..trailer_start]
    } else {
        message
    };
    let mut entries = if trailer_start < message.len() {
        parse_trailer_entries(&message[trailer_start..])
    } else {
        Vec::new()
    };
    if options.trim_empty {
        entries.retain(|entry| !entry.value.trim().is_empty());
    }
    if !options.only_input {
        for trailer in &options.trailers {
            let addition = parse_trailer_argument(trailer)?;
            apply_trailer_addition(&mut entries, addition, placement, if_exists, if_missing);
        }
    }

    if options.only_trailers {
        return Ok(lines_with_final_newline(&trailer_output_lines(
            &entries,
            options.unfold,
        )));
    }

    let mut output = prefix.to_vec();
    while output.last().is_some_and(|line| line.trim().is_empty()) {
        output.pop();
    }
    if !entries.is_empty() {
        let last_line_is_trailer = output
            .last()
            .is_some_and(|line| split_existing_trailer(line).is_some());
        if !output.is_empty() && (had_existing_block || !last_line_is_trailer) {
            output.push(String::new());
        }
        output.extend(trailer_output_lines(&entries, options.unfold));
    }
    output.extend_from_slice(suffix);
    Ok(lines_with_final_newline(&output))
}

fn split_text_lines(input: &str) -> Vec<String> {
    let trimmed = input.trim_end_matches('\n');
    if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed
            .split('\n')
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect()
    }
}

fn parse_trailer_placement(value: Option<&str>) -> Result<TrailerPlacement> {
    match value.unwrap_or("end") {
        "end" => Ok(TrailerPlacement::End),
        "start" => Ok(TrailerPlacement::Start),
        "after" => Ok(TrailerPlacement::After),
        "before" => Ok(TrailerPlacement::Before),
        other => Err(CliError::Fatal {
            code: 129,
            message: format!("unknown trailer placement '{other}'"),
        }),
    }
}

fn parse_trailer_if_exists(value: Option<&str>) -> Result<TrailerIfExists> {
    match value.unwrap_or("addIfDifferentNeighbor") {
        "addIfDifferentNeighbor" => Ok(TrailerIfExists::AddIfDifferentNeighbor),
        "addIfDifferent" => Ok(TrailerIfExists::AddIfDifferent),
        "add" => Ok(TrailerIfExists::Add),
        "replace" => Ok(TrailerIfExists::Replace),
        "doNothing" => Ok(TrailerIfExists::DoNothing),
        other => Err(CliError::Fatal {
            code: 129,
            message: format!("unknown trailer if-exists action '{other}'"),
        }),
    }
}

fn parse_trailer_if_missing(value: Option<&str>) -> Result<TrailerIfMissing> {
    match value.unwrap_or("add") {
        "add" => Ok(TrailerIfMissing::Add),
        "doNothing" => Ok(TrailerIfMissing::DoNothing),
        other => Err(CliError::Fatal {
            code: 129,
            message: format!("unknown trailer if-missing action '{other}'"),
        }),
    }
}

fn trailer_divider_line(line: &str) -> bool {
    line == "---" || line.starts_with("--- ")
}

fn trailer_block_start(lines: &[String]) -> usize {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && !lines[start - 1].trim().is_empty() {
        start -= 1;
    }
    if start == end || (start > 0 && !lines[start - 1].trim().is_empty()) {
        return lines.len();
    }
    let group = &lines[start..end];
    if trailer_group_is_valid(group) {
        start
    } else {
        lines.len()
    }
}

fn trailer_group_is_valid(lines: &[String]) -> bool {
    if lines.is_empty() {
        return false;
    }
    let entries = parse_trailer_entries(lines);
    if entries.is_empty() {
        return false;
    }
    let entry_line_count = entries.iter().map(|entry| entry.lines.len()).sum::<usize>();
    if entry_line_count == lines.len() {
        return true;
    }
    false
}

fn parse_trailer_entries(lines: &[String]) -> Vec<TrailerEntry> {
    let mut entries: Vec<TrailerEntry> = Vec::new();
    for line in lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(entry) = entries.last_mut() {
                entry.lines.push(line.clone());
                if !entry.value.is_empty() {
                    entry.value.push(' ');
                }
                entry.value.push_str(line.trim());
            }
            continue;
        }
        let Some((key, value)) = split_existing_trailer(line) else {
            continue;
        };
        entries.push(TrailerEntry {
            lines: vec![line.clone()],
            key: key.to_owned(),
            value: value.trim().to_owned(),
        });
    }
    entries
}

fn split_existing_trailer(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim_end_matches([' ', '\t']);
    trailer_key_is_valid(key).then_some((key, value))
}

fn parse_trailer_argument(trailer: &str) -> Result<TrailerEntry> {
    let (key, value) = trailer
        .split_once(':')
        .or_else(|| trailer.split_once('='))
        .unwrap_or((trailer, ""));
    let key = key.trim();
    if !trailer_key_is_valid(key) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid trailer '{trailer}'"),
        });
    }
    let value = value.trim().to_owned();
    Ok(TrailerEntry {
        lines: vec![format!("{key}: {value}")],
        key: key.to_owned(),
        value,
    })
}

fn trailer_key_is_valid(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn apply_trailer_addition(
    entries: &mut Vec<TrailerEntry>,
    addition: TrailerEntry,
    placement: TrailerPlacement,
    if_exists: TrailerIfExists,
    if_missing: TrailerIfMissing,
) {
    let matching_positions = matching_trailer_positions(entries, &addition.key);
    if matching_positions.is_empty() {
        if if_missing == TrailerIfMissing::Add {
            let position = insertion_position(entries, &addition.key, placement);
            entries.insert(position, addition);
        }
        return;
    }

    match if_exists {
        TrailerIfExists::DoNothing => {}
        TrailerIfExists::AddIfDifferent => {
            if !entries
                .iter()
                .any(|entry| same_trailer_pair(entry, &addition))
            {
                let position = insertion_position(entries, &addition.key, placement);
                entries.insert(position, addition);
            }
        }
        TrailerIfExists::AddIfDifferentNeighbor => {
            let position = insertion_position(entries, &addition.key, placement);
            let duplicate_before = position
                .checked_sub(1)
                .and_then(|index| entries.get(index))
                .is_some_and(|entry| same_trailer_pair(entry, &addition));
            let duplicate_after = entries
                .get(position)
                .is_some_and(|entry| same_trailer_pair(entry, &addition));
            if !duplicate_before && !duplicate_after {
                entries.insert(position, addition);
            }
        }
        TrailerIfExists::Add => {
            let position = insertion_position(entries, &addition.key, placement);
            entries.insert(position, addition);
        }
        TrailerIfExists::Replace => {
            let position = insertion_position(entries, &addition.key, placement);
            if let Some(index) = closest_matching_position(&matching_positions, position) {
                entries.remove(index);
            }
            let position = insertion_position(entries, &addition.key, placement);
            entries.insert(position, addition);
        }
    }
}

fn matching_trailer_positions(entries: &[TrailerEntry], key: &str) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| same_trailer_key(&entry.key, key).then_some(index))
        .collect()
}

fn insertion_position(entries: &[TrailerEntry], key: &str, placement: TrailerPlacement) -> usize {
    match placement {
        TrailerPlacement::End => entries.len(),
        TrailerPlacement::Start => 0,
        TrailerPlacement::After => entries
            .iter()
            .rposition(|entry| same_trailer_key(&entry.key, key))
            .map_or(entries.len(), |index| index + 1),
        TrailerPlacement::Before => entries
            .iter()
            .position(|entry| same_trailer_key(&entry.key, key))
            .unwrap_or(entries.len()),
    }
}

fn closest_matching_position(positions: &[usize], insertion_position: usize) -> Option<usize> {
    positions
        .iter()
        .copied()
        .min_by_key(|position| position.abs_diff(insertion_position))
}

fn same_trailer_key(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn same_trailer_pair(left: &TrailerEntry, right: &TrailerEntry) -> bool {
    same_trailer_key(&left.key, &right.key) && left.value.trim() == right.value.trim()
}

fn trailer_output_lines(entries: &[TrailerEntry], unfold: bool) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| {
            if unfold {
                vec![format!("{}: {}", entry.key, entry.value.trim())]
            } else {
                entry.lines.clone()
            }
        })
        .collect()
}

pub(crate) fn lines_with_final_newline(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }
}

pub(crate) fn mailsplit(
    precision: Option<usize>,
    first: Option<usize>,
    _keep_from: bool,
    keep_cr: bool,
    output: PathBuf,
    paths: Vec<PathBuf>,
) -> Result<()> {
    let precision = precision.unwrap_or(4);
    let mut next = first.unwrap_or(0) + 1;
    let mut written = 0usize;
    for path in paths {
        let messages = if path.join("cur").is_dir() && path.join("new").is_dir() {
            read_maildir_messages(&path, keep_cr)?
        } else {
            split_mbox_messages(&fs::read(&path)?, keep_cr)?
        };
        for message in messages {
            let filename = format!("{next:0precision$}");
            fs::write(output.join(filename), message)?;
            next += 1;
            written += 1;
        }
    }
    println!("{written}");
    Ok(())
}

fn read_maildir_messages(path: &std::path::Path, keep_cr: bool) -> Result<Vec<Vec<u8>>> {
    let mut messages = Vec::new();
    for dirname in ["cur", "new"] {
        let mut entries =
            fs::read_dir(path.join(dirname))?.collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_file() {
                messages.push(normalize_mail_bytes(fs::read(entry.path())?, keep_cr));
            }
        }
    }
    Ok(messages)
}

fn split_mbox_messages(input: &[u8], keep_cr: bool) -> Result<Vec<Vec<u8>>> {
    let input = normalize_mail_bytes(input.to_vec(), keep_cr);
    if input.is_empty() || !input.starts_with(b"From ") {
        println!("corrupt mailbox");
        return Err(CliError::Exit(1));
    }
    let mut starts = vec![0usize];
    let mut index = 0usize;
    while let Some(relative) = find_bytes(&input[index..], b"\nFrom ") {
        let start = index + relative + 1;
        starts.push(start);
        index = start + 1;
    }
    starts.push(input.len());
    let messages = starts
        .windows(2)
        .map(|window| input[window[0]..window[1]].to_vec())
        .collect();
    Ok(messages)
}

fn normalize_mail_bytes(mut input: Vec<u8>, keep_cr: bool) -> Vec<u8> {
    if keep_cr {
        return input;
    }
    input.retain(|byte| *byte != b'\r');
    input
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(crate) fn mailinfo(
    keep_subject: bool,
    keep_non_patch_brackets: bool,
    message_id: bool,
    msg: PathBuf,
    patch: PathBuf,
) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let (headers, body) = split_mail_headers(&input);
    let header_map = parse_mail_headers(headers);
    let (author, email) = parse_mail_author(header_map.get("from").map_or("", String::as_str));
    let subject = header_map
        .get("subject")
        .map(|value| clean_mail_subject(value, keep_subject, keep_non_patch_brackets))
        .unwrap_or_default();
    let (message, patch_text) = split_mail_body_patch(body);
    let mut message_lines = message;
    if message_id && let Some(id) = header_map.get("message-id") {
        message_lines = message_lines.trim_end_matches('\n').to_owned();
        if !message_lines.is_empty() {
            message_lines.push_str("\n\n");
        }
        message_lines.push_str("Message-ID: ");
        message_lines.push_str(id.trim());
        message_lines.push('\n');
    }
    fs::write(msg, message_lines)?;
    fs::write(patch, patch_text)?;

    println!("Author: {author}");
    println!("Email: {email}");
    println!("Subject: {subject}");
    if let Some(date) = header_map.get("date") {
        println!("Date: {}", date.trim());
    }
    println!();
    Ok(())
}

pub(crate) fn split_mail_headers(input: &str) -> (&str, &str) {
    input
        .split_once("\n\n")
        .map_or((input, ""), |(headers, body)| (headers, body))
}

pub(crate) fn parse_mail_headers(headers: &str) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut current_key: Option<String> = None;
    for line in headers.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = &current_key
                && let Some(value) = map.get_mut(key)
            {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let normalized_key = key.trim().to_ascii_lowercase();
        current_key = Some(normalized_key.clone());
        map.insert(normalized_key, value.trim().to_owned());
    }
    map
}

pub(crate) fn parse_mail_author(from: &str) -> (String, String) {
    if let Some(start) = from.rfind('<')
        && let Some(end) = from[start + 1..].find('>')
    {
        let end = start + 1 + end;
        let name = from[..start].trim().trim_matches('"').to_owned();
        let email = from[start + 1..end].trim().to_owned();
        return (name, email);
    }
    (from.trim().to_owned(), from.trim().to_owned())
}

pub(crate) fn clean_mail_subject(
    subject: &str,
    keep_subject: bool,
    keep_non_patch_brackets: bool,
) -> String {
    if keep_subject {
        return subject.trim().to_owned();
    }
    let mut remaining = subject.trim();
    let mut removed_patch = false;
    while let Some(rest) = remaining.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            break;
        };
        let bracket = &rest[..end];
        let after = rest[end + 1..].trim_start();
        let is_patch = bracket
            .split_whitespace()
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("patch"));
        if is_patch {
            removed_patch = true;
            remaining = after;
            continue;
        }
        if keep_non_patch_brackets && removed_patch {
            break;
        }
        remaining = after;
    }
    remaining.to_owned()
}

pub(crate) fn split_mail_body_patch(body: &str) -> (String, String) {
    let mut message = Vec::new();
    let mut patch = Vec::new();
    let mut in_patch = false;
    for line in body.lines() {
        if !in_patch && (line == "---" || line.starts_with("diff --git ")) {
            in_patch = true;
        }
        if in_patch {
            patch.push(line.to_owned());
        } else {
            message.push(line.to_owned());
        }
    }
    let mut message_text = message.join("\n");
    if !message_text.is_empty() {
        message_text.push('\n');
    }
    let mut patch_text = patch.join("\n");
    if !patch_text.is_empty() {
        patch_text.push('\n');
    }
    (message_text, patch_text)
}

pub(crate) fn fmt_merge_msg(
    log: Option<usize>,
    no_log: bool,
    message: Option<&str>,
    into_name: Option<&str>,
    file: Option<PathBuf>,
) -> Result<()> {
    let input = if let Some(path) = file {
        if path.as_os_str() == "-" {
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            input
        } else {
            fs::read_to_string(path)?
        }
    } else {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        input
    };
    let entries = parse_fetch_head_for_merge(&input);
    if entries.is_empty() {
        return Ok(());
    }
    if let Some(message) = message {
        println!("{message}");
        return Ok(());
    }

    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let current = current_branch_ref(&refs)?
        .map(|name| branch_display_name(&name))
        .unwrap_or_else(|| "HEAD".to_owned());
    let target = into_name.unwrap_or(&current);
    let mut title = fmt_merge_title(&entries);
    if target != current {
        title.push_str(" into ");
        title.push_str(target);
    }
    println!("{title}");
    if log.is_some() && !no_log {
        println!();
        for source in entries {
            println!("* {}:", source.description);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FetchHeadMergeEntry {
    description: String,
}

fn parse_fetch_head_for_merge(input: &str) -> Vec<FetchHeadMergeEntry> {
    input
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let _oid = parts.next()?;
            let marker = parts.next().unwrap_or_default();
            let description = parts.next().unwrap_or_default().trim();
            if marker == "not-for-merge" || description.is_empty() {
                return None;
            }
            Some(FetchHeadMergeEntry {
                description: description.to_owned(),
            })
        })
        .collect()
}

fn fmt_merge_title(entries: &[FetchHeadMergeEntry]) -> String {
    if entries.len() == 1 {
        return format!("Merge {}", entries[0].description);
    }
    let descriptions = entries
        .iter()
        .map(|entry| entry.description.as_str())
        .collect::<Vec<_>>();
    format!("Merge {}", join_english_list(&descriptions))
}

fn join_english_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [first, second] => format!("{first} and {second}"),
        _ => {
            let mut out = items[..items.len() - 1].join(", ");
            out.push_str(", and ");
            out.push_str(items[items.len() - 1]);
            out
        }
    }
}
