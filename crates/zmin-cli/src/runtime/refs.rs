use std::fs;
use std::io::{self, Write};

use zmin_git_core::{
    GitHashAlgorithm, ObjectId, RefStore, RefTarget, ReftableLogRecord, check_ref_format,
};

use super::{
    CliError, GitRepo, Result, find_repo, read_common_git_dir, read_config_entry,
    read_config_value, repo_is_bare, resolve_objectish, signature_from_identity, zero_object_id,
};

pub(crate) fn is_per_worktree_ref(name: &str) -> bool {
    name == "HEAD"
        || name.starts_with("refs/bisect/")
        || name.starts_with("refs/worktree/")
        || (!name.contains('/')
            && !name.ends_with(".lock")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
}

pub(crate) fn ref_exists(refs: &RefStore, name: &str) -> Result<bool> {
    match refs.read_ref(name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn tag_ref_name(name: &str) -> Result<String> {
    let ref_name = if name.starts_with("refs/tags/") {
        name.to_owned()
    } else if name.starts_with("refs/") || name.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{name}' is not a valid tag name."),
        });
    } else {
        format!("refs/tags/{name}")
    };
    if !check_ref_format(&ref_name, false) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{name}' is not a valid tag name."),
        });
    }
    Ok(ref_name)
}

pub(crate) fn branch_ref_name(name: &str) -> Result<String> {
    let name = match resolve_previous_checkout_name_from_cwd(name) {
        Some(Ok(name)) => name,
        Some(Err(_)) => return Err(invalid_branch_name_error(name)),
        None => name.to_owned(),
    };
    if name == "HEAD" {
        return Err(invalid_branch_name_error(&name));
    }
    let ref_name = if name.starts_with("refs/heads/") {
        name.clone()
    } else if name.starts_with("refs/") || name.is_empty() {
        return Err(invalid_branch_name_error(&name));
    } else {
        format!("refs/heads/{name}")
    };
    if !check_ref_format(&ref_name, false) {
        return Err(invalid_branch_name_error(&name));
    }
    Ok(ref_name)
}

pub(crate) fn invalid_branch_name_error(name: &str) -> CliError {
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: '{name}' is not a valid branch name\n\
             hint: See `man git check-ref-format`\n\
             hint: Disable this message with \"git config advice.refSyntax false\"\n"
        ),
    }
}

pub(crate) fn previous_checkout_syntax_index(value: &str) -> Option<usize> {
    value
        .strip_prefix("@{-")
        .and_then(|inner| inner.strip_suffix('}'))
        .and_then(|inner| inner.parse::<usize>().ok())
        .filter(|index| *index > 0)
}

fn previous_checkout_prefix(value: &str) -> Option<(usize, &str)> {
    let rest = value.strip_prefix("@{-")?;
    let end = rest.find('}')?;
    let (index, suffix) = rest.split_at(end);
    let suffix = suffix.strip_prefix('}')?;
    index
        .parse::<usize>()
        .ok()
        .filter(|index| *index > 0)
        .map(|index| (index, suffix))
}

pub(crate) fn previous_checkout_target_from_repo(
    repo: &GitRepo,
    index: usize,
) -> io::Result<String> {
    let refs = RefStore::new(
        &repo.git_dir,
        super::repo_hash_algorithm_from_config(repo)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, format!("{error:?}")))?,
    );
    if refs.storage_kind()? == zmin_git_core::refs::RefStorageKind::Reftable {
        let mut logs = refs
            .reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == "HEAD")
            .collect::<Vec<_>>();
        logs.sort_by_key(|record| std::cmp::Reverse(record.update_index));
        return logs
            .into_iter()
            .filter_map(|record| {
                record
                    .message
                    .strip_prefix("checkout: moving from ")
                    .and_then(|rest| rest.split_once(" to "))
                    .map(|(previous, _)| previous.to_owned())
            })
            .nth(index - 1)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "previous checkout not found"));
    }
    let contents = fs::read_to_string(repo.git_dir.join("logs").join("HEAD"))?;
    contents
        .lines()
        .rev()
        .filter_map(|line| line.split_once('\t').map(|(_, message)| message))
        .filter_map(|message| {
            message
                .strip_prefix("checkout: moving from ")
                .and_then(|rest| rest.split_once(" to "))
                .map(|(previous, _)| previous.to_owned())
        })
        .nth(index - 1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "previous checkout not found"))
}

pub(crate) fn resolve_previous_checkout_name(
    repo: &GitRepo,
    value: &str,
) -> io::Result<Option<String>> {
    let Some(index) = previous_checkout_syntax_index(value) else {
        return Ok(None);
    };
    previous_checkout_target_from_repo(repo, index).map(Some)
}

pub(crate) fn resolve_previous_checkout_expression(
    repo: &GitRepo,
    value: &str,
) -> io::Result<Option<String>> {
    let Some((index, suffix)) = previous_checkout_prefix(value) else {
        return Ok(None);
    };
    let previous = previous_checkout_target_from_repo(repo, index)?;
    Ok(Some(format!("{previous}{suffix}")))
}

fn resolve_previous_checkout_name_from_cwd(value: &str) -> Option<io::Result<String>> {
    let index = previous_checkout_syntax_index(value)?;
    let repo = find_repo()
        .map_err(|error| io::Error::new(io::ErrorKind::NotFound, format!("{error:?}")))
        .ok()?;
    Some(previous_checkout_target_from_repo(&repo, index))
}

fn contains_revision_suffix(value: &str) -> bool {
    value.contains('~')
        || value.rsplit_once('^').is_some_and(|(_, suffix)| {
            suffix.is_empty() || suffix.bytes().all(|b| b.is_ascii_digit())
        })
}

pub(crate) fn branch_checkout_ref(refs: &RefStore, name: &str) -> Result<Option<String>> {
    if name.starts_with("refs/heads/") {
        return ref_exists(refs, name).map(|exists| exists.then(|| name.to_owned()));
    }
    let ref_name = match branch_ref_name(name) {
        Ok(ref_name) => ref_name,
        Err(_) => return Ok(None),
    };
    ref_exists(refs, &ref_name).map(|exists| exists.then_some(ref_name))
}

pub(crate) fn current_branch_ref(refs: &RefStore) -> Result<Option<String>> {
    match refs.read_head().map_err(|error| {
        if matches!(
            error.kind(),
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData
        ) {
            CliError::Fatal {
                code: 128,
                message: "your current branch appears to be broken".into(),
            }
        } else {
            CliError::Io(error)
        }
    })? {
        RefTarget::Symbolic(target) if target.starts_with("refs/heads/") => Ok(Some(target)),
        _ => Ok(None),
    }
}

pub(crate) fn branch_display_name(ref_name: &str) -> String {
    ref_name
        .strip_prefix("refs/heads/")
        .unwrap_or(ref_name)
        .to_owned()
}

pub(crate) fn tag_display_name(ref_name: &str) -> String {
    ref_name
        .strip_prefix("refs/tags/")
        .unwrap_or(ref_name)
        .to_owned()
}

pub(crate) fn short_ref_name(value: &str) -> String {
    short_ref_name_str(value).to_owned()
}

pub(crate) fn short_ref_name_str(value: &str) -> &str {
    value
        .strip_prefix("refs/heads/")
        .or_else(|| value.strip_prefix("refs/tags/"))
        .or_else(|| value.strip_prefix("refs/remotes/"))
        .or_else(|| value.strip_prefix("refs/"))
        .unwrap_or(value)
}

pub(crate) fn abbrev_ref_name(repo: &GitRepo, rev: &str) -> Result<String> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let resolved_previous = resolve_previous_checkout_expression(repo, rev)
        .ok()
        .flatten();
    let rev = resolved_previous.as_deref().unwrap_or(rev);
    if rev == "HEAD" {
        return Ok(current_branch_ref(&refs)?
            .map(|name| branch_display_name(&name))
            .unwrap_or_else(|| "HEAD".to_owned()));
    }
    if rev.starts_with("refs/heads/") {
        return Ok(branch_display_name(rev));
    }
    if let Some(base) = split_push_suffix_name(rev) {
        let ref_name = super::push_ref_name(repo, base).map_err(CliError::Io)?;
        return Ok(ref_name
            .strip_prefix("refs/remotes/")
            .unwrap_or(&ref_name)
            .to_owned());
    }
    if contains_revision_suffix(rev) {
        resolve_objectish(repo, rev).map_err(CliError::Io)?;
        return Ok(rev.to_owned());
    }
    if ref_exists(&refs, &branch_ref_name(rev)?)? {
        return Ok(rev.to_owned());
    }
    resolve_objectish(repo, rev).map_err(CliError::Io)?;
    Ok(rev.to_owned())
}

pub(crate) fn symbolic_full_ref_name(repo: &GitRepo, rev: &str) -> Result<Option<String>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let resolved_previous = resolve_previous_checkout_expression(repo, rev)
        .ok()
        .flatten();
    let rev = resolved_previous.as_deref().unwrap_or(rev);
    if rev == "HEAD" {
        return Ok(match refs.read_head()? {
            RefTarget::Symbolic(target) => Some(target),
            RefTarget::Direct(_) => Some("HEAD".to_owned()),
        });
    }
    if rev.starts_with("refs/") {
        return ref_exists(&refs, rev).map(|exists| exists.then(|| rev.to_owned()));
    }
    if let Some(base) = split_push_suffix_name(rev) {
        return super::push_ref_name(repo, base)
            .map(Some)
            .map_err(CliError::Io);
    }
    if contains_revision_suffix(rev) {
        resolve_objectish(repo, rev).map_err(CliError::Io)?;
        return Ok(None);
    }
    let remote_name = rev.strip_prefix("remotes/").unwrap_or(rev);
    let mut candidates = vec![format!("refs/heads/{rev}"), format!("refs/tags/{rev}")];
    if !remote_name.contains('/') {
        candidates.push(format!("refs/remotes/{remote_name}/HEAD"));
    }
    candidates.push(format!("refs/remotes/{remote_name}"));
    for candidate in candidates {
        if ref_exists(&refs, &candidate)? {
            return Ok(Some(candidate));
        }
    }
    resolve_objectish(repo, rev).map_err(CliError::Io)?;
    Ok(None)
}

fn split_push_suffix_name(value: &str) -> Option<&str> {
    let base = value.strip_suffix('}')?;
    let (base, selector) = base.rsplit_once("@{")?;
    selector.eq_ignore_ascii_case("push").then_some(base)
}

pub(crate) fn remote_branch_display(
    refs: &RefStore,
    ref_name: &str,
    include_remotes_prefix: bool,
) -> Result<String> {
    let display = if include_remotes_prefix {
        ref_name.strip_prefix("refs/").unwrap_or(ref_name)
    } else {
        ref_name.strip_prefix("refs/remotes/").unwrap_or(ref_name)
    };
    match refs.read_ref(ref_name)? {
        RefTarget::Symbolic(target) => Ok(format!(
            "{} -> {}",
            display,
            target
                .strip_prefix("refs/remotes/")
                .or_else(|| target.strip_prefix("refs/heads/"))
                .unwrap_or(&target)
        )),
        RefTarget::Direct(_) => Ok(display.to_owned()),
    }
}

pub(crate) fn source_head_branch(refs: &RefStore) -> Result<Option<String>> {
    match refs.read_head()? {
        RefTarget::Symbolic(target) => Ok(target.strip_prefix("refs/heads/").map(str::to_owned)),
        RefTarget::Direct(_) => Ok(None),
    }
}

pub(crate) fn branch_head_ids(refs: &RefStore) -> Result<Vec<ObjectId>> {
    let mut ids = Vec::new();
    refs.for_each_resolved_ref("refs/heads/", |_, id| {
        ids.push(id.clone());
        Ok::<(), CliError>(())
    })?;
    Ok(ids)
}

pub(crate) fn update_head_to_commit(refs: &RefStore, id: &ObjectId) -> Result<()> {
    match refs.read_head()? {
        RefTarget::Symbolic(target) => Ok(refs.write_ref(&target, id)?),
        RefTarget::Direct(_) => Ok(refs.write_head_direct(id)?),
    }
}

pub(crate) fn update_head_to_commit_with_reflog(
    repo: &GitRepo,
    refs: &RefStore,
    id: &ObjectId,
    message: &str,
) -> Result<()> {
    if !automatic_reflog_enabled(repo)? {
        return update_head_to_commit(refs, id);
    }
    match refs.read_head()? {
        RefTarget::Symbolic(target) => {
            let old_id = reflog_old_id(refs, &target, false)?;
            refs.write_ref(&target, id)?;
            append_reflog(repo, &target, &old_id, id, message)?;
            append_reflog(repo, "HEAD", &old_id, id, message)
        }
        RefTarget::Direct(_) => write_head_direct_with_reflog(repo, refs, id, message),
    }
}

pub(crate) fn update_head_to_commit_with_optional_reflog(
    repo: &GitRepo,
    refs: &RefStore,
    id: &ObjectId,
    message: &str,
) -> Result<()> {
    if !automatic_reflog_enabled(repo)? {
        return update_head_to_commit(refs, id);
    }
    match refs.read_head()? {
        RefTarget::Symbolic(target) => {
            let old_id = reflog_old_id(refs, &target, false)?;
            refs.write_ref(&target, id)?;
            append_reflog_if_identity_available(repo, &target, &old_id, id, message)?;
            append_reflog_if_identity_available(repo, "HEAD", &old_id, id, message)
        }
        RefTarget::Direct(_) => {
            let old_id = reflog_old_id(refs, "HEAD", true)?;
            refs.write_head_direct(id)?;
            append_reflog_if_identity_available(repo, "HEAD", &old_id, id, message)
        }
    }
}

pub(crate) fn automatic_reflog_enabled(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "core.logAllRefUpdates")? else {
        return Ok(!repo_is_bare(repo));
    };
    if entry.value.eq_ignore_ascii_case("always") {
        return Ok(true);
    }
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("bad boolean config value '{}'", entry.value),
    })
}

pub(crate) fn write_ref_with_reflog(
    repo: &GitRepo,
    refs: &RefStore,
    name: &str,
    id: &ObjectId,
    message: &str,
) -> Result<()> {
    let old_id = reflog_old_id(refs, name, false)?;
    if refs.storage_kind()? == zmin_git_core::refs::RefStorageKind::Reftable {
        let log = reftable_log_record(repo, name, &old_id, id, message)?;
        return refs
            .write_reftable_ref_with_logs(name, RefTarget::Direct(id.clone()), vec![log])
            .map_err(CliError::Io);
    }
    refs.write_ref(name, id)?;
    append_reflog(repo, name, &old_id, id, message)
}

pub(crate) fn write_head_symbolic_with_reflog(
    repo: &GitRepo,
    refs: &RefStore,
    target: &str,
    message: &str,
) -> Result<()> {
    let old_id = reflog_old_id(refs, "HEAD", true)?;
    refs.write_head_symbolic(target)?;
    let new_id = refs.resolve(target).unwrap_or_else(|_| zero_object_id());
    append_reflog(repo, "HEAD", &old_id, &new_id, message)
}

fn symbolic_ref_prefers_symlinks(repo: &GitRepo) -> Result<bool> {
    Ok(matches!(
        read_config_value(repo, "core.preferSymlinkRefs")?
            .as_deref()
            .map(|value| value.eq_ignore_ascii_case("true")),
        Some(true)
    ))
}

fn validate_symbolic_ref_storage_name(name: &str) -> Result<()> {
    let is_pseudoref = !name.is_empty()
        && !name.contains('/')
        && !name.ends_with(".lock")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    if name == "HEAD" || is_pseudoref || check_ref_format(name, true) {
        Ok(())
    } else {
        Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref name",
        )))
    }
}

fn write_symbolic_ref_deprecation_warning() -> Result<()> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(
        b"warning: 'core.preferSymlinkRefs=true' is nominated for removal.\n\
hint: The use of symbolic links for symbolic refs is deprecated\n\
hint: and will be removed in Git 3.0. The configuration that\n\
hint: tells Git to use them is thus going away. You can unset\n\
hint: it with:\n\
hint:\n\
hint:\tgit config unset core.preferSymlinkRefs\n\
hint:\n\
hint: Git will then use the textual symref format instead.\n",
    )?;
    Ok(())
}

#[cfg(unix)]
fn write_symbolic_ref_symlink(repo: &GitRepo, name: &str, target: &str) -> Result<()> {
    use std::os::unix::fs::symlink;

    validate_symbolic_ref_storage_name(name)?;
    if !check_ref_format(target, name != "HEAD") {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git ref name",
        )));
    }
    let path = repo.git_dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            return Err(CliError::Io(io::Error::new(
                io::ErrorKind::IsADirectory,
                "non-empty ref directory",
            )));
        }
        Ok(_) => fs::remove_file(&path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    }
    symlink(target, &path).map_err(CliError::Io)?;
    write_symbolic_ref_deprecation_warning()
}

#[cfg(not(unix))]
fn write_symbolic_ref_symlink(repo: &GitRepo, name: &str, target: &str) -> Result<()> {
    let _ = repo;
    let _ = name;
    let _ = target;
    Err(CliError::Fatal {
        code: 128,
        message: "core.preferSymlinkRefs is unsupported on this platform".into(),
    })
}

pub(crate) fn write_symbolic_ref_with_repo_config(
    repo: &GitRepo,
    refs: &RefStore,
    name: &str,
    target: &str,
) -> Result<()> {
    if symbolic_ref_prefers_symlinks(repo)? {
        write_symbolic_ref_symlink(repo, name, target)
    } else {
        refs.write_symbolic_ref(name, target).map_err(CliError::Io)
    }
}

pub(crate) fn write_head_direct_with_reflog(
    repo: &GitRepo,
    refs: &RefStore,
    id: &ObjectId,
    message: &str,
) -> Result<()> {
    let old_id = reflog_old_id(refs, "HEAD", true)?;
    refs.write_head_direct(id)?;
    append_reflog(repo, "HEAD", &old_id, id, message)
}

pub(crate) fn write_pseudoref(repo: &GitRepo, name: &str, id: &ObjectId) -> Result<()> {
    fs::write(repo.git_dir.join(name), format!("{}\n", id.to_hex())).map_err(CliError::Io)
}

fn reflog_old_id(refs: &RefStore, name: &str, no_deref: bool) -> Result<ObjectId> {
    if name == "HEAD" && no_deref {
        return match refs.read_head() {
            Ok(RefTarget::Direct(id)) => Ok(id),
            Ok(RefTarget::Symbolic(target)) => {
                refs.resolve(&target).or_else(|_| Ok(zero_object_id()))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(zero_object_id()),
            Err(error) => Err(CliError::Io(error)),
        };
    }
    match refs.resolve(name) {
        Ok(id) => Ok(id),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(zero_object_id()),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn append_reflog(
    repo: &GitRepo,
    name: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    message: &str,
) -> Result<()> {
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    append_reflog_with_committer(repo, name, old_id, new_id, message, &committer)
}

pub(crate) fn append_reflog_with_committer(
    repo: &GitRepo,
    name: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    message: &str,
    committer: &zmin_git_core::Signature,
) -> Result<()> {
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let ref_git_dir = if is_per_worktree_ref(name) {
        repo.git_dir.clone()
    } else {
        common_git_dir
    };
    let refs = RefStore::new(ref_git_dir, old_id.algorithm());
    if refs.storage_kind()? == zmin_git_core::refs::RefStorageKind::Reftable {
        refs.append_reftable_log(reftable_log_record_with_committer(
            name, old_id, new_id, message, committer,
        )?)?;
        return Ok(());
    }
    let path = repo.git_dir.join("logs").join(name);
    if path.is_dir() {
        fs::remove_dir_all(&path)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{} {} {} <{}> {} {}\t{}",
        old_id.to_hex(),
        new_id.to_hex(),
        committer.name,
        committer.email,
        committer.timestamp,
        committer.timezone,
        message
    )?;
    Ok(())
}

pub(crate) fn reftable_log_record(
    repo: &GitRepo,
    name: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    message: &str,
) -> Result<ReftableLogRecord> {
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    reftable_log_record_with_committer(name, old_id, new_id, message, &committer)
}

fn reftable_log_record_with_committer(
    name: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    message: &str,
    committer: &zmin_git_core::Signature,
) -> Result<ReftableLogRecord> {
    Ok(ReftableLogRecord {
        ref_name: name.to_owned(),
        update_index: 0,
        old_id: old_id.clone(),
        new_id: new_id.clone(),
        name: committer.name.clone(),
        email: committer.email.clone(),
        timestamp: u64::try_from(committer.timestamp).unwrap_or_default(),
        timezone_offset: reflog_timezone_offset(&committer.timezone)?,
        message: message.to_owned(),
    })
}

fn reflog_timezone_offset(value: &str) -> Result<i16> {
    let bytes = value.as_bytes();
    if bytes.len() != 5 || !matches!(bytes[0], b'+' | b'-') {
        return Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid reflog timezone",
        )));
    }
    let magnitude = value[1..]
        .parse::<i16>()
        .map_err(|error| CliError::Io(io::Error::new(io::ErrorKind::InvalidData, error)))?;
    Ok(if bytes[0] == b'-' {
        -magnitude
    } else {
        magnitude
    })
}

pub(crate) fn append_reflog_if_identity_available(
    repo: &GitRepo,
    name: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    message: &str,
) -> Result<()> {
    match append_reflog(repo, name, old_id, new_id, message) {
        Ok(()) => Ok(()),
        Err(CliError::Message(message))
            if message.contains("GIT_COMMITTER_NAME or config user.name is required")
                || message.contains("GIT_COMMITTER_EMAIL or config user.email is required") =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn push_destination_ref(value: &str) -> Result<String> {
    if value.starts_with("refs/") {
        Ok(value.to_owned())
    } else {
        branch_ref_name(value)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn branch_head_ids_use_loose_ref_over_stale_packed_ref() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(git_dir.join("objects")).expect("objects dir");
        let stale_id = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let live_id = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/main\n", stale_id.to_hex()),
        )
        .expect("write packed refs");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/main", &live_id)
            .expect("write loose ref");

        let ids = branch_head_ids(&refs).expect("branch ids");

        assert_eq!(ids, vec![live_id]);
    }
}
