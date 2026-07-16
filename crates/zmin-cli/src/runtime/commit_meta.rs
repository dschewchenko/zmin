use super::*;

const UPSTREAM_HISTORY_PACK_OBJECT_CACHE_BYTES: usize = 1024 * 1024;

pub(crate) fn signature_timestamp(signature: &[u8]) -> Option<i64> {
    let mut fields = signature.rsplit(|byte| *byte == b' ');
    fields.next()?;
    let timestamp = fields.next()?;
    std::str::from_utf8(timestamp).ok()?.parse().ok()
}

pub(crate) fn commit_subject(message: &[u8]) -> String {
    let first_line = message
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let subject = first_line.strip_suffix(b"\r").unwrap_or(first_line);
    String::from_utf8_lossy(subject).into_owned()
}

pub(crate) fn tag_subject(message: &[u8]) -> String {
    let mut subject = String::new();
    for line in String::from_utf8_lossy(message).lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "-----BEGIN PGP SIGNATURE-----" {
            break;
        }
        if !subject.is_empty() {
            subject.push(' ');
        }
        subject.push_str(line);
    }
    subject
}

pub(crate) fn split_log_message_lines(message: &[u8]) -> Vec<&[u8]> {
    let message = message
        .iter()
        .rposition(|byte| *byte != b'\n' && *byte != b'\r')
        .map(|index| &message[..=index])
        .unwrap_or_default();
    if message.is_empty() {
        return Vec::new();
    }
    message.split(|byte| *byte == b'\n').collect()
}

pub(crate) fn signature_name(signature: &[u8]) -> String {
    let (name, _) = signature_name_email_parts(signature);
    String::from_utf8_lossy(name).into_owned()
}

pub(crate) fn signature_email(signature: &[u8]) -> String {
    let (_, email) = signature_name_email_parts(signature);
    String::from_utf8_lossy(email).into_owned()
}

pub(crate) fn signature_name_email(signature: &[u8]) -> (String, String) {
    let (name, email) = signature_name_email_parts(signature);
    (
        String::from_utf8_lossy(name).into_owned(),
        String::from_utf8_lossy(email).into_owned(),
    )
}

fn signature_name_email_parts(signature: &[u8]) -> (&[u8], &[u8]) {
    let Some(start) = signature.windows(2).position(|window| window == b" <") else {
        return (signature, b"");
    };
    let name = &signature[..start];
    let email = &signature[start + 2..];
    let email = email
        .iter()
        .position(|byte| *byte == b'>')
        .map_or(email, |end| &email[..end]);
    (name, email)
}

pub(crate) fn signature_from_commit_bytes(signature: &[u8]) -> Result<Signature> {
    let signature = std::str::from_utf8(signature).map_err(|_| CliError::Fatal {
        code: 128,
        message: "commit has invalid author signature".into(),
    })?;
    let (prefix, timezone) = signature.rsplit_once(' ').ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let (name_email, timestamp) = prefix.rsplit_once(' ').ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timestamp".into(),
    })?;
    let (name, email) = name_email
        .rsplit_once(" <")
        .and_then(|(name, email)| email.strip_suffix('>').map(|email| (name, email)))
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author identity".into(),
        })?;
    let timestamp = timestamp.parse().map_err(|_| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timestamp".into(),
    })?;
    Ok(Signature::new(name, email, timestamp, timezone)?)
}

pub(crate) fn signature_log_date(signature: &[u8]) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit author timestamp is out of range".into(),
    })?;
    Ok(utc
        .with_timezone(&offset)
        .format("%a %b %-d %H:%M:%S %Y %z")
        .to_string())
}

pub(crate) fn signature_mail_date(signature: &[u8]) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit author timestamp is out of range".into(),
    })?;
    Ok(utc
        .with_timezone(&offset)
        .format("%a, %-d %b %Y %H:%M:%S %z")
        .to_string())
}

pub(crate) fn signature_blame_date(signature: &[u8]) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid author date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit author timestamp is out of range".into(),
    })?;
    Ok(utc
        .with_timezone(&offset)
        .format("%Y-%m-%d %H:%M:%S %z")
        .to_string())
}

pub(crate) fn signature_timestamp_timezone(signature: &[u8]) -> Option<(i64, &str)> {
    let signature = std::str::from_utf8(signature).ok()?;
    let (prefix, timezone) = signature.rsplit_once(' ')?;
    let (_, timestamp) = prefix.rsplit_once(' ')?;
    Some((timestamp.parse().ok()?, timezone))
}

pub(crate) fn parse_timezone_offset(timezone: &str) -> Option<chrono::FixedOffset> {
    let bytes = timezone.as_bytes();
    if bytes.len() != 5 || (bytes[0] != b'+' && bytes[0] != b'-') {
        return None;
    }
    let hours: i32 = std::str::from_utf8(&bytes[1..3]).ok()?.parse().ok()?;
    let minutes: i32 = std::str::from_utf8(&bytes[3..5]).ok()?.parse().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    let seconds = hours * 3600 + minutes * 60;
    if bytes[0] == b'-' {
        chrono::FixedOffset::west_opt(seconds)
    } else {
        chrono::FixedOffset::east_opt(seconds)
    }
}

pub(crate) fn porcelain_branch_header(repo: &GitRepo, ahead_behind: bool) -> Result<String> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    match refs.read_head()? {
        RefTarget::Symbolic(target) if target.starts_with("refs/heads/") => {
            let branch = target.strip_prefix("refs/heads/").unwrap_or(&target);
            if refs.resolve("HEAD").is_ok() {
                if let Some(upstream) = read_branch_upstream(repo, branch)? {
                    let marker = if ahead_behind {
                        format_upstream_counts(upstream_counts(repo, &upstream.ref_name)?)
                    } else if upstream_differs_from_head(repo, &upstream.ref_name)? {
                        " [different]".to_owned()
                    } else {
                        String::new()
                    };
                    Ok(format!("## {branch}...{}{}", upstream.display, marker))
                } else {
                    Ok(format!("## {branch}"))
                }
            } else {
                Ok(format!("## No commits yet on {branch}"))
            }
        }
        RefTarget::Direct(_) => Ok("## HEAD (no branch)".to_owned()),
        RefTarget::Symbolic(target) => Ok(format!(
            "## {}",
            target
                .strip_prefix("refs/")
                .unwrap_or(&target)
                .strip_prefix("heads/")
                .unwrap_or(target.as_str())
        )),
    }
}

pub(crate) fn upstream_differs_from_head(repo: &GitRepo, upstream_ref: &str) -> Result<bool> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head = refs.resolve("HEAD")?;
    let upstream = refs.resolve(upstream_ref)?;
    Ok(head != upstream)
}

#[derive(Debug, Clone)]
pub(crate) struct BranchUpstream {
    pub(crate) display: String,
    pub(crate) ref_name: String,
}

pub(crate) fn read_branch_upstream(repo: &GitRepo, branch: &str) -> Result<Option<BranchUpstream>> {
    let Some(remote) = read_config_section_value(repo, "branch", branch, "remote")? else {
        return Ok(None);
    };
    let Some(merge) = read_config_section_value(repo, "branch", branch, "merge")? else {
        return Ok(None);
    };
    if remote == "." {
        return Ok(Some(BranchUpstream {
            display: short_ref_name(&merge),
            ref_name: merge,
        }));
    }
    let short_merge = short_ref_name(&merge);
    let ref_name = if remote_fetch_maps_branch(repo, &remote, &short_merge)? {
        format!("refs/remotes/{remote}/{short_merge}")
    } else {
        merge
    };
    Ok(Some(BranchUpstream {
        display: format!("{remote}/{short_merge}"),
        ref_name,
    }))
}

fn remote_fetch_maps_branch(repo: &GitRepo, remote: &str, branch: &str) -> io::Result<bool> {
    let fetches = read_common_config_entries(repo)?
        .into_iter()
        .filter(|entry| {
            entry.section == "remote" && entry.subsection == remote && entry.key == "fetch"
        })
        .map(|entry| entry.value.trim_start_matches('+').to_owned())
        .collect::<Vec<_>>();
    if fetches.is_empty() {
        return Ok(true);
    }
    let source = format!("refs/heads/{branch}");
    let destination = format!("refs/remotes/{remote}/{branch}");
    Ok(fetches
        .iter()
        .any(|refspec| refspec_maps_ref(refspec, &source, &destination)))
}

fn refspec_maps_ref(refspec: &str, source: &str, destination: &str) -> bool {
    let Some((from, to)) = refspec.split_once(':') else {
        return false;
    };
    match (from.split_once('*'), to.split_once('*')) {
        (Some((from_prefix, from_suffix)), Some((to_prefix, to_suffix))) => {
            source
                .strip_prefix(from_prefix)
                .and_then(|wildcard| wildcard.strip_suffix(from_suffix))
                .is_some_and(|wildcard| {
                    destination == format!("{to_prefix}{wildcard}{to_suffix}")
                })
        }
        _ => from == source && to == destination,
    }
}

pub(crate) fn upstream_counts(
    repo: &GitRepo,
    upstream_ref: &str,
) -> Result<Option<(usize, usize)>> {
    upstream_counts_from_ref(repo, "HEAD", upstream_ref)
}

pub(crate) fn upstream_counts_from_ref(
    repo: &GitRepo,
    local_ref: &str,
    upstream_ref: &str,
) -> Result<Option<(usize, usize)>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if refs.resolve(upstream_ref).is_err() {
        return Ok(None);
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1)
        .with_transient_packed_object_reads()
        .with_trusted_packed_object_reads()
        .with_buffered_pack_reads()
        .with_packed_object_read_cache_byte_limit(UPSTREAM_HISTORY_PACK_OBJECT_CACHE_BYTES);
    let local_id = refs.resolve(local_ref)?;
    let upstream_id = refs.resolve(upstream_ref)?;
    if local_id == upstream_id {
        return Ok(Some((0, 0)));
    }
    if is_ancestor_commit_uncached(&store, &upstream_id, &local_id)? {
        let excluded = HashSet::from([upstream_id]);
        let ahead = count_commits_from_ids_uncached_with_excluded(
            repo,
            &store,
            std::slice::from_ref(&local_id),
            &excluded,
        )?;
        return Ok(Some((ahead, 0)));
    }
    if is_ancestor_commit_uncached(&store, &local_id, &upstream_id)? {
        let excluded = HashSet::from([local_id]);
        let behind = count_commits_from_ids_uncached_with_excluded(
            repo,
            &store,
            std::slice::from_ref(&upstream_id),
            &excluded,
        )?;
        return Ok(Some((0, behind)));
    }
    let commit_cache = CommitObjectCache::new(&store);
    let local = collect_commits_cached(repo, &store, &commit_cache, &[local_ref.to_owned()], None)?
        .into_iter()
        .map(|id| id.to_hex())
        .collect::<HashSet<_>>();
    let upstream = collect_commits_cached(
        repo,
        &store,
        &commit_cache,
        &[upstream_ref.to_owned()],
        None,
    )?
    .into_iter()
    .map(|id| id.to_hex())
    .collect::<HashSet<_>>();
    let ahead = local.difference(&upstream).count();
    let behind = upstream.difference(&local).count();
    Ok(Some((ahead, behind)))
}

fn format_upstream_counts(counts: Option<(usize, usize)>) -> String {
    match counts {
        Some((ahead, 0)) if ahead > 0 => format!(" [ahead {ahead}]"),
        Some((0, behind)) if behind > 0 => format!(" [behind {behind}]"),
        Some((ahead, behind)) if ahead > 0 && behind > 0 => {
            format!(" [ahead {ahead}, behind {behind}]")
        }
        _ => String::new(),
    }
}

pub(crate) fn status_code(status: IndexDiffStatus) -> char {
    match status {
        IndexDiffStatus::Added => 'A',
        IndexDiffStatus::Copied => 'C',
        IndexDiffStatus::Deleted => 'D',
        IndexDiffStatus::Modified => 'M',
        IndexDiffStatus::Renamed => 'R',
    }
}

pub(crate) fn signature_from_identity(repo: &GitRepo, prefix: &str) -> Result<Signature> {
    let (name, email) = identity_name_email(repo, prefix)?;
    let date = std::env::var(format!("{prefix}_DATE")).ok();
    let (timestamp, timezone) = signature_date(date.as_deref())?;
    Ok(Signature::new(name, email, timestamp, timezone)?)
}

pub(crate) fn signature_from_strict_identity(repo: &GitRepo, prefix: &str) -> Result<Signature> {
    let name = identity_env_value(&format!("{prefix}_NAME"))
        .or_else(|| read_config_value(repo, "user.name").ok().flatten())
        .ok_or_else(|| {
            CliError::Message(format!("{prefix}_NAME or config user.name is required"))
        })?;
    let email = identity_env_value(&format!("{prefix}_EMAIL"))
        .or_else(|| read_config_value(repo, "user.email").ok().flatten())
        .or_else(|| identity_env_value("EMAIL"))
        .ok_or_else(|| {
            CliError::Message(format!("{prefix}_EMAIL or config user.email is required"))
        })?;
    let date = std::env::var(format!("{prefix}_DATE")).ok();
    let (timestamp, timezone) = signature_date(date.as_deref())?;
    Ok(Signature::new(name, email, timestamp, timezone)?)
}

fn identity_name_email(repo: &GitRepo, prefix: &str) -> Result<(String, String)> {
    let use_config_only = read_config_entry(repo, "user.useconfigonly")?
        .and_then(|entry| entry.bool_value())
        .unwrap_or(false);
    let auto = (!use_config_only)
        .then(auto_detect_identity_name_email)
        .flatten();
    let name = identity_env_value(&format!("{prefix}_NAME"))
        .or_else(|| read_config_value(repo, "user.name").ok().flatten())
        .or_else(|| auto.as_ref().map(|(name, _)| name.clone()))
        .ok_or_else(|| {
            CliError::Message(format!("{prefix}_NAME or config user.name is required"))
        })?;
    let email = identity_env_value(&format!("{prefix}_EMAIL"))
        .or_else(|| read_config_value(repo, "user.email").ok().flatten())
        .or_else(|| identity_env_value("EMAIL"))
        .or_else(|| auto.as_ref().map(|(_, email)| email.clone()))
        .ok_or_else(|| {
            CliError::Message(format!("{prefix}_EMAIL or config user.email is required"))
        })?;
    Ok((name, email))
}

fn identity_env_value(name: &str) -> Option<String> {
    let value = std::env::var_os(name)?;
    if value.is_empty() {
        return None;
    }
    Some(value.to_string_lossy().into_owned())
}

fn auto_detect_identity_name_email() -> Option<(String, String)> {
    auto_detect_identity_name().zip(auto_detect_identity_email())
}

fn auto_detect_identity_name() -> Option<String> {
    #[cfg(unix)]
    if let Some(name) = auto_detect_unix_identity_name() {
        return Some(name);
    }
    identity_env_value("GIT_AUTHOR_NAME")
        .or_else(|| identity_env_value("GIT_COMMITTER_NAME"))
        .or_else(|| identity_env_value("USER"))
        .or_else(|| identity_env_value("USERNAME"))
}

fn auto_detect_identity_email() -> Option<String> {
    identity_env_value("EMAIL").or_else(auto_detect_email_from_account_host)
}

fn auto_detect_email_from_account_host() -> Option<String> {
    let account = auto_detect_account_name()?;
    let host = auto_detect_host_name()?;
    Some(format!("{account}@{host}"))
}

fn auto_detect_account_name() -> Option<String> {
    #[cfg(unix)]
    if let Some(account) = auto_detect_unix_account_name() {
        return Some(account);
    }
    identity_env_value("USER").or_else(|| identity_env_value("USERNAME"))
}

#[cfg(unix)]
fn auto_detect_unix_identity_name() -> Option<String> {
    use std::ffi::CStr;

    let passwd = unix_passwd_entry()?;
    let gecos = unsafe { CStr::from_ptr(passwd.pw_gecos) }
        .to_string_lossy()
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    gecos.or_else(auto_detect_unix_account_name)
}

#[cfg(unix)]
fn auto_detect_unix_account_name() -> Option<String> {
    use std::ffi::CStr;

    let passwd = unix_passwd_entry()?;
    let name = unsafe { CStr::from_ptr(passwd.pw_name) }
        .to_string_lossy()
        .trim()
        .to_owned();
    (!name.is_empty()).then_some(name)
}

#[cfg(unix)]
fn unix_passwd_entry() -> Option<libc::passwd> {
    let uid = unsafe { libc::geteuid() };
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buf = vec![0u8; 4096];
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    Some(unsafe { pwd.assume_init() })
}

fn auto_detect_host_name() -> Option<String> {
    #[cfg(unix)]
    {
        let mut uts = std::mem::MaybeUninit::<libc::utsname>::uninit();
        let rc = unsafe { libc::uname(uts.as_mut_ptr()) };
        if rc == 0 {
            let uts = unsafe { uts.assume_init() };
            let host = unsafe { std::ffi::CStr::from_ptr(uts.nodename.as_ptr()) }
                .to_string_lossy()
                .trim()
                .to_owned();
            if !host.is_empty() {
                return Some(host);
            }
        }
    }
    identity_env_value("HOSTNAME").or_else(|| identity_env_value("COMPUTERNAME"))
}

pub(crate) fn signature_date(date: Option<&str>) -> Result<(i64, String)> {
    match date {
        Some(value) => parse_git_date(value),
        None => Ok((current_unix_timestamp()?, current_timezone_offset())),
    }
}

pub(crate) fn signature_from_author_options(
    repo: &GitRepo,
    base: Option<&Signature>,
    author: Option<&str>,
    date: Option<&str>,
) -> Result<Signature> {
    let (name, email) = match author {
        Some(author) => {
            let (name, email) = parse_author_identity(author)?;
            (name.to_owned(), email.to_owned())
        }
        None => match base {
            Some(signature) => (signature.name.clone(), signature.email.clone()),
            None => identity_name_email(repo, "GIT_AUTHOR")?,
        },
    };
    let env_author_date = std::env::var("GIT_AUTHOR_DATE").ok();
    let (timestamp, timezone) = match (date, base) {
        (Some(value), _) => parse_git_date(value)?,
        (None, Some(signature)) => (signature.timestamp, signature.timezone.clone()),
        (None, None) => signature_date(env_author_date.as_deref())?,
    };
    Ok(Signature::new(name, email, timestamp, timezone)?)
}

pub(crate) fn parse_author_identity(author: &str) -> Result<(&str, &str)> {
    author
        .rsplit_once(" <")
        .and_then(|(name, email)| email.strip_suffix('>').map(|email| (name, email)))
        .filter(|(name, email)| !name.is_empty() && !email.is_empty())
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("malformed --author value: {author}"),
        })
}

pub(crate) fn parse_git_date(value: &str) -> Result<(i64, String)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CliError::Message("git date is empty".into()));
    }
    let mut parts = trimmed.split_whitespace();
    let first = parts.next().unwrap_or_default();
    if let Ok(timestamp) = first.trim_start_matches('@').parse::<i64>() {
        let timezone = parts.next().unwrap_or("+0000").to_owned();
        if parts.next().is_some() {
            return Err(CliError::Message(
                "git date must use `<unix-seconds> <+/-HHMM>`".into(),
            ));
        }
        return Ok((timestamp, timezone));
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Ok((datetime.timestamp(), datetime.format("%z").to_string()));
    }
    parse_git_absolute_date(trimmed)
}

fn parse_git_absolute_date(value: &str) -> Result<(i64, String)> {
    let formats_with_timezone = [
        "%Y-%m-%d %H:%M:%S %z",
        "%Y-%m-%d %H:%M %z",
        "%Y-%m-%dT%H:%M:%S %z",
        "%Y-%m-%dT%H:%M %z",
    ];
    for format in formats_with_timezone {
        if let Ok(datetime) = chrono::DateTime::parse_from_str(value, format) {
            return Ok((datetime.timestamp(), datetime.format("%z").to_string()));
        }
    }
    let timezone = current_timezone_offset();
    let offset_seconds = timezone_offset_seconds(&timezone).unwrap_or(0);
    let formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%b %d %Y %H:%M:%S",
        "%b %d %Y %H:%M",
    ];
    for format in formats {
        if let Ok(datetime) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return Ok((datetime.and_utc().timestamp() - offset_seconds, timezone));
        }
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        && let Some(datetime) = date.and_hms_opt(0, 0, 0)
    {
        return Ok((datetime.and_utc().timestamp() - offset_seconds, timezone));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%b %d %Y")
        && let Some(datetime) = date.and_hms_opt(0, 0, 0)
    {
        return Ok((datetime.and_utc().timestamp() - offset_seconds, timezone));
    }
    Err(CliError::Message(format!(
        "git date timestamp is invalid: invalid digit found in string: {value}"
    )))
}

fn timezone_offset_seconds(value: &str) -> Option<i64> {
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    if value.len() != 5 {
        return None;
    }
    let hours = value[1..3].parse::<i64>().ok()?;
    let minutes = value[3..5].parse::<i64>().ok()?;
    Some(sign * ((hours * 60 + minutes) * 60))
}

pub(crate) fn current_unix_timestamp() -> Result<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| CliError::Message(format!("system clock is before UNIX epoch: {err}")))?;
    Ok(duration.as_secs().min(i64::MAX as u64) as i64)
}

fn current_timezone_offset() -> String {
    local_now().format("%z").to_string()
}
#[derive(Clone, Copy)]
pub(crate) enum CommitCleanupMode {
    Default,
    Strip,
    Whitespace,
    Verbatim,
    Scissors,
}

pub(crate) fn strip_commit_message_line_whitespace(line: &[u8]) -> &[u8] {
    let new_len = line
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|idx| idx + 1)
        .unwrap_or(0);
    &line[..new_len]
}

pub(crate) fn is_commit_message_line_blank(line: &[u8]) -> bool {
    line.iter().all(|byte| byte.is_ascii_whitespace())
}

pub(crate) fn cleanup_commit_message(message: Vec<u8>, mode: CommitCleanupMode) -> Vec<u8> {
    if matches!(mode, CommitCleanupMode::Verbatim) {
        return message;
    }
    let mut has_content = false;
    let mut cleaned_lines = Vec::new();
    let mut previous_blank = false;
    for line in message.split(|byte| *byte == b'\n') {
        let line = strip_commit_message_line_whitespace(line);
        if matches!(mode, CommitCleanupMode::Strip) && line.starts_with(b"#") && !line.is_empty() {
            previous_blank = true;
            continue;
        }
        if matches!(mode, CommitCleanupMode::Scissors)
            && line == b"# ------------------------ >8 ------------------------"
        {
            break;
        }
        if is_commit_message_line_blank(line) {
            if previous_blank || cleaned_lines.is_empty() {
                previous_blank = true;
                continue;
            }
            previous_blank = true;
            continue;
        }
        if previous_blank {
            cleaned_lines.push(Vec::new());
        }
        previous_blank = false;
        cleaned_lines.push(line.to_vec());
        has_content = true;
    }
    if !has_content {
        return Vec::new();
    }
    let mut output = Vec::new();
    for line in cleaned_lines {
        output.extend_from_slice(&line);
        output.push(b'\n');
    }
    output
}

pub(crate) fn next_borrowed_option_value<'a>(
    iter: &mut impl Iterator<Item = &'a String>,
    option: &str,
) -> Result<&'a str> {
    iter.next()
        .map(String::as_str)
        .ok_or_else(|| CliError::Fatal {
            code: 129,
            message: format!("{option} requires a value"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_timestamp_reads_from_tail_without_field_collection() {
        assert_eq!(
            signature_timestamp(b"Example User <user@example.com> 1716200000 +0300"),
            Some(1716200000)
        );
        assert_eq!(
            signature_timestamp(b"Example User <user@example.com>"),
            None
        );
    }

    #[test]
    fn tag_subject_joins_subject_lines_until_blank_or_signature() {
        assert_eq!(
            tag_subject(b"first line\r\nsecond line\n\nbody"),
            "first line second line"
        );
        assert_eq!(
            tag_subject(b"first line\n-----BEGIN PGP SIGNATURE-----\nignored"),
            "first line"
        );
    }

    #[test]
    fn commit_subject_decodes_only_first_message_line() {
        assert_eq!(commit_subject(b"subject\r\nbody\nmore"), "subject");
        assert_eq!(commit_subject(b"subj\xffct\nbody"), "subj\u{fffd}ct");
    }

    #[test]
    fn split_log_message_lines_drops_only_trailing_blank_tail() {
        assert_eq!(
            split_log_message_lines(b"subject\n\nbody\n\n"),
            vec![b"subject".as_slice(), b"".as_slice(), b"body".as_slice()]
        );
        assert_eq!(
            split_log_message_lines(b"subject\n\n"),
            vec![b"subject".as_slice()]
        );
        assert!(split_log_message_lines(b"\n\n").is_empty());
    }

    #[test]
    fn signature_identity_helpers_decode_only_selected_slice() {
        let signature = b"Example User <user@example.com> 1716200000 +0300";

        assert_eq!(signature_name(signature), "Example User");
        assert_eq!(signature_email(signature), "user@example.com");
        assert_eq!(signature_email(b"Example User"), "");
    }

    #[test]
    fn current_timezone_offset_returns_git_offset_shape() {
        let timezone = current_timezone_offset();

        assert_eq!(timezone.len(), 5);
        assert!(matches!(timezone.as_bytes()[0], b'+' | b'-'));
        assert!(timezone.as_bytes()[1..].iter().all(u8::is_ascii_digit));
        assert!(parse_timezone_offset(&timezone).is_some());
    }

    #[test]
    #[cfg(unix)]
    fn identity_env_value_accepts_non_utf8_commit_identity_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let key = "ZMIN_TEST_NON_UTF8_IDENTITY";
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, OsString::from_vec(vec![0x69, 0x73, 0x6f, 0x2d, 0xff]));
        }

        let resolved = identity_env_value(key);

        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }

        assert_eq!(resolved, Some("iso-\u{fffd}".to_owned()));
    }
}
