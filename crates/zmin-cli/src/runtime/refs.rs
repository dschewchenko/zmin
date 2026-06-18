use std::fs;
use std::io::{self, Write};

use zmin_git_core::{GitHashAlgorithm, ObjectId, RefStore, RefTarget, check_ref_format};

use super::{
    CliError, GitRepo, Result, resolve_objectish, signature_from_identity, zero_object_id,
};

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
    if name == "HEAD" {
        return Err(invalid_branch_name_error(name));
    }
    let ref_name = if name.starts_with("refs/heads/") {
        name.to_owned()
    } else if name.starts_with("refs/") || name.is_empty() {
        return Err(invalid_branch_name_error(name));
    } else {
        format!("refs/heads/{name}")
    };
    if !check_ref_format(&ref_name, false) {
        return Err(invalid_branch_name_error(name));
    }
    Ok(ref_name)
}

pub(crate) fn invalid_branch_name_error(name: &str) -> CliError {
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: '{name}' is not a valid branch name\n\
             hint: See 'git help check-ref-format'\n\
             hint: Disable this message with \"git config set advice.refSyntax false\"\n"
        ),
    }
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
    match refs.read_head()? {
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
        .or_else(|| value.strip_prefix("refs/"))
        .unwrap_or(value)
}

pub(crate) fn abbrev_ref_name(repo: &GitRepo, rev: &str) -> Result<String> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if rev == "HEAD" {
        return Ok(current_branch_ref(&refs)?
            .map(|name| branch_display_name(&name))
            .unwrap_or_else(|| "HEAD".to_owned()));
    }
    if rev.starts_with("refs/heads/") {
        return Ok(branch_display_name(rev));
    }
    if ref_exists(&refs, &branch_ref_name(rev)?)? {
        return Ok(rev.to_owned());
    }
    resolve_objectish(repo, rev).map_err(CliError::Io)?;
    Ok(rev.to_owned())
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

pub(crate) fn write_ref_with_reflog(
    repo: &GitRepo,
    refs: &RefStore,
    name: &str,
    id: &ObjectId,
    message: &str,
) -> Result<()> {
    let old_id = reflog_old_id(refs, name, false)?;
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
