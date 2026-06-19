use super::*;

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
    let message = message.strip_suffix(b"\n").unwrap_or(message);
    if message.is_empty() {
        return Vec::new();
    }
    message.split(|byte| *byte == b'\n').collect()
}

pub(crate) fn signature_name(signature: &[u8]) -> String {
    let name = signature
        .windows(2)
        .position(|window| window == b" <")
        .map_or(signature, |index| &signature[..index]);
    String::from_utf8_lossy(name).into_owned()
}

pub(crate) fn signature_email(signature: &[u8]) -> String {
    let Some(start) = signature.windows(2).position(|window| window == b" <") else {
        return String::new();
    };
    let email = &signature[start + 2..];
    let email = email
        .iter()
        .position(|byte| *byte == b'>')
        .map_or(email, |end| &email[..end]);
    String::from_utf8_lossy(email).into_owned()
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
        .format("%a, %d %b %Y %H:%M:%S %z")
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
    Ok(Some(BranchUpstream {
        display: format!("{remote}/{short_merge}"),
        ref_name: format!("refs/remotes/{remote}/{short_merge}"),
    }))
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
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
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

fn identity_name_email(repo: &GitRepo, prefix: &str) -> Result<(String, String)> {
    let name = std::env::var(format!("{prefix}_NAME"))
        .ok()
        .or_else(|| read_config_value(repo, "user.name").ok().flatten())
        .ok_or_else(|| {
            CliError::Message(format!("{prefix}_NAME or config user.name is required"))
        })?;
    let email = std::env::var(format!("{prefix}_EMAIL"))
        .ok()
        .or_else(|| read_config_value(repo, "user.email").ok().flatten())
        .ok_or_else(|| {
            CliError::Message(format!("{prefix}_EMAIL or config user.email is required"))
        })?;
    Ok((name, email))
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
    let formats_with_timezone = ["%Y-%m-%d %H:%M:%S %z", "%Y-%m-%d %H:%M %z"];
    for format in formats_with_timezone {
        if let Ok(datetime) = chrono::DateTime::parse_from_str(value, format) {
            return Ok((datetime.timestamp(), datetime.format("%z").to_string()));
        }
    }
    let timezone = current_timezone_offset();
    let offset_seconds = timezone_offset_seconds(&timezone).unwrap_or(0);
    let formats = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"];
    for format in formats {
        if let Ok(datetime) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return Ok((datetime.and_utc().timestamp() - offset_seconds, timezone));
        }
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
    match std::process::Command::new("date").arg("+%z").output() {
        Ok(output) if output.status.success() => String::from_utf8(output.stdout)
            .unwrap_or_else(|_| "+0000\n".to_owned())
            .trim()
            .to_owned(),
        _ => "+0000".to_owned(),
    }
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
    fn signature_identity_helpers_decode_only_selected_slice() {
        let signature = b"Example User <user@example.com> 1716200000 +0300";

        assert_eq!(signature_name(signature), "Example User");
        assert_eq!(signature_email(signature), "user@example.com");
        assert_eq!(signature_email(b"Example User"), "");
    }
}
