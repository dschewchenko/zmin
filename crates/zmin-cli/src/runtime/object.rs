use std::collections::HashSet;
use std::fs;
use std::io;

use regex::bytes::Regex;
use zmin_git_core::{
    CommitObjectCache, GitHashAlgorithm, GitObjectKind, GitObjectStore, LooseObjectStore, ObjectId,
    RefStore, RefTarget, decode_pack_index_object_ids_from_path, decode_tag, find_tree_entry,
    read_index_with_algorithm,
};

use super::{
    CliError, ConfigScope, GitRepo, Result, current_branch_ref, current_unix_timestamp,
    expand_repo_sparse_index, is_per_worktree_ref, parse_git_date, partial_clone_enabled,
    previous_checkout_syntax_index, previous_checkout_target_from_repo, read_branch_upstream,
    read_common_git_dir, read_config_entry, read_config_value, repo_hash_algorithm_from_config,
    resolve_commitish_io, resolve_previous_checkout_expression, resolve_previous_checkout_name,
    signature_timestamp, sparse_index_path_requires_expansion, trace2_region,
};

const DEFAULT_ABBREV_OBJECT_ID_INITIAL_CAPACITY_LIMIT: usize = 8192;

pub(crate) enum BatchCommand<'a> {
    Object(&'a str, &'a str, bool),
    Flush,
}

pub(crate) struct ResolvedObjectish {
    pub(crate) id: ObjectId,
    pub(crate) mode: Option<String>,
}

pub(crate) fn zero_object_id() -> ObjectId {
    ObjectId::new(GitHashAlgorithm::Sha1, &[0; 20])
}

pub(crate) fn print_rev_parse_object(
    repo: &GitRepo,
    rev: &str,
    short: Option<usize>,
    verify: bool,
    quiet: bool,
) -> Result<()> {
    let id = resolve_objectish_with_reflog_warnings(repo, rev, !quiet).map_err(|_| {
        if verify && quiet {
            return CliError::Exit(1);
        }
        if verify {
            CliError::Fatal {
                code: 128,
                message: "Needed a single revision".to_owned(),
            }
        } else {
            CliError::Message(format!("unknown revision `{rev}`"))
        }
    })?;
    if let Some(length) = short {
        let hex_len = id.hex_len();
        if length == 0 {
            return Err(CliError::Message(format!(
                "invalid --short length `{length}`"
            )));
        }
        println!("{}", id.short_hex(length.min(hex_len)));
    } else {
        println!("{id}");
    }
    Ok(())
}

pub(crate) fn resolve_objectish(repo: &GitRepo, objectish: &str) -> io::Result<ObjectId> {
    resolve_objectish_with_mode(repo, objectish).map(|resolved| resolved.id)
}

fn resolve_objectish_with_reflog_warnings(
    repo: &GitRepo,
    objectish: &str,
    reflog_warnings: bool,
) -> io::Result<ObjectId> {
    resolve_objectish_with_mode_and_reflog_warnings(repo, objectish, reflog_warnings)
        .map(|resolved| resolved.id)
}

pub(crate) fn resolve_objectish_with_mode(
    repo: &GitRepo,
    objectish: &str,
) -> io::Result<ResolvedObjectish> {
    resolve_objectish_with_mode_and_reflog_warnings(repo, objectish, true)
}

fn resolve_objectish_with_mode_and_reflog_warnings(
    repo: &GitRepo,
    objectish: &str,
    reflog_warnings: bool,
) -> io::Result<ResolvedObjectish> {
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    let store = LooseObjectStore::new(&repo.objects_dir, algorithm);
    if let Some(pattern) = objectish.strip_prefix(":/") {
        return resolve_message_search_from_refs(repo, &store, pattern).map(resolved_without_mode);
    }
    if let Some((base, pattern)) = split_message_search_suffix(objectish) {
        return resolve_message_search_from_base(repo, &store, base, pattern)
            .map(resolved_without_mode);
    }
    if let Some((base, peel)) = split_peel_suffix(objectish) {
        return resolve_typed_objectish(repo, &store, base, peel).map(resolved_without_mode);
    }
    if let Some((base, path)) = split_objectish_path(objectish) {
        if base.is_empty() {
            return resolve_index_object_path_with_mode(repo, path);
        }
        let path = normalize_git_path(path)?;
        let tree_id = resolve_treeish(repo, &store, base)?;
        if path.is_empty() {
            return Ok(resolved_without_mode(tree_id));
        }
        return find_tree_entry(&store, &tree_id, path.as_bytes())?
            .map(|entry| ResolvedObjectish {
                id: entry.id,
                mode: Some(String::from_utf8_lossy(entry.mode.as_bytes()).into_owned()),
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "path not found in tree"));
    }
    if objectish.contains('~') || contains_parent_shorthand(objectish) {
        return resolve_commitish_io(repo, &store, objectish).map(resolved_without_mode);
    }
    resolve_plain_objectish_with_reflog_warnings(repo, &store, objectish, reflog_warnings)
        .map(resolved_without_mode)
}

fn resolved_without_mode(id: ObjectId) -> ResolvedObjectish {
    ResolvedObjectish { id, mode: None }
}

fn split_objectish_path(objectish: &str) -> Option<(&str, &str)> {
    let mut brace_depth = 0usize;
    let mut previous = None;
    for (index, byte) in objectish.bytes().enumerate() {
        match byte {
            b'{' if matches!(previous, Some(b'@' | b'^')) => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b':' if brace_depth == 0 => {
                return Some((&objectish[..index], &objectish[index + 1..]));
            }
            _ => {}
        }
        previous = Some(byte);
    }
    None
}

pub(crate) fn objectish_path_component(objectish: &str) -> io::Result<Option<Vec<u8>>> {
    let Some((_, raw_path)) = split_objectish_path(objectish) else {
        return Ok(None);
    };
    let raw_path = match raw_path.as_bytes() {
        [b'0'..=b'3', b':', ..] => &raw_path[2..],
        _ => raw_path,
    };
    let path = normalize_git_path(raw_path)?;
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(path.into_bytes()))
    }
}

fn split_message_search_suffix(objectish: &str) -> Option<(&str, &str)> {
    let rest = objectish.strip_suffix('}')?;
    rest.rsplit_once("^{/")
}

fn contains_parent_shorthand(objectish: &str) -> bool {
    objectish.rsplit_once('^').is_some_and(|(_, suffix)| {
        suffix.is_empty() || suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn resolve_message_search_from_refs(
    repo: &GitRepo,
    store: &LooseObjectStore,
    pattern: &str,
) -> io::Result<ObjectId> {
    let refs = common_ref_store(repo)?;
    let mut starts = Vec::new();
    if let Ok(head) = resolve_repo_ref(repo, "HEAD") {
        starts.push(head);
    }
    refs.for_each_resolved_ref("refs/", |_, id| {
        starts.push(id.clone());
        Ok::<(), io::Error>(())
    })?;
    resolve_message_search(repo, store, starts, pattern)
}

fn resolve_message_search_from_base(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    pattern: &str,
) -> io::Result<ObjectId> {
    let start = resolve_commitish_io(repo, store, base)?;
    resolve_message_search(repo, store, vec![start], pattern)
}

fn resolve_message_search(
    _repo: &GitRepo,
    store: &LooseObjectStore,
    starts: Vec<ObjectId>,
    pattern: &str,
) -> io::Result<ObjectId> {
    let (exclude, match_pattern) = if let Some(pattern) = pattern.strip_prefix("!-") {
        (true, pattern.to_owned())
    } else if let Some(pattern) = pattern.strip_prefix("!!") {
        (false, format!("!{pattern}"))
    } else if pattern.starts_with('!') {
        (false, r"\b\B".to_owned())
    } else {
        (false, pattern.to_owned())
    };
    let matcher = Regex::new(&match_pattern)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let commit_cache = CommitObjectCache::new(store);
    let mut stack = starts;
    let mut seen = HashSet::new();
    let mut best = None::<(i64, usize, ObjectId)>;
    let mut sequence = 0usize;
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let object = store.read_object(&id)?;
        if object.kind != GitObjectKind::Commit {
            continue;
        }
        let commit = commit_cache.read_loaded_commit(object)?;
        if exclude != matcher.is_match(&commit.message) {
            let timestamp = signature_timestamp(&commit.committer).unwrap_or(i64::MIN);
            let replace = best
                .as_ref()
                .is_none_or(|(best_timestamp, best_sequence, _)| {
                    timestamp > *best_timestamp
                        || (timestamp == *best_timestamp && sequence < *best_sequence)
                });
            if replace {
                best = Some((timestamp, sequence, id.clone()));
            }
        }
        sequence += 1;
        for parent in commit.parents.iter().rev() {
            stack.push(parent.clone());
        }
    }
    best.map(|(_, _, id)| id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("no commit message matches {pattern:?}"),
        )
    })
}

fn resolve_index_object_path_with_mode(
    repo: &GitRepo,
    raw_path: &str,
) -> io::Result<ResolvedObjectish> {
    let (stage, path) = match raw_path.as_bytes() {
        [stage @ b'0'..=b'3', b':', ..] => (stage - b'0', &raw_path[2..]),
        _ => (0, raw_path),
    };
    let path = normalize_git_path(path)?;
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    let raw_index = read_index_with_algorithm(&repo.index_path, algorithm)?;
    if let Some(resolved) = raw_index
        .entries()
        .iter()
        .find(|entry| entry.stage == stage && entry.path.as_slice() == path.as_bytes())
        .map(|entry| ResolvedObjectish {
            id: entry.id.clone(),
            mode: Some(format!("{:o}", entry.mode_bits())),
        })
    {
        return Ok(resolved);
    }
    let path = path.as_bytes();
    let inside_sparse_directory =
        stage == 0 && sparse_index_path_requires_expansion(&raw_index, path);
    if !inside_sparse_directory {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "path not found in index",
        ));
    }
    let _region = trace2_region("index", "ensure_full_index");
    let expanded_index = expand_repo_sparse_index(repo, &raw_index)?;
    expanded_index
        .entries()
        .iter()
        .find(|entry| entry.stage == stage && entry.path.as_slice() == path)
        .map(|entry| ResolvedObjectish {
            id: entry.id.clone(),
            mode: Some(format!("{:o}", entry.mode_bits())),
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "path not found in index"))
}

pub(crate) fn resolve_plain_objectish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
) -> io::Result<ObjectId> {
    resolve_plain_objectish_with_reflog_warnings(repo, store, objectish, true)
}

fn resolve_plain_objectish_with_reflog_warnings(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
    reflog_warnings: bool,
) -> io::Result<ObjectId> {
    if let Some(expanded) = resolve_previous_checkout_expression(repo, objectish)? {
        return resolve_plain_objectish_with_reflog_warnings(
            repo,
            store,
            &expanded,
            reflog_warnings,
        );
    }
    if let Some(index) = previous_checkout_syntax_index(objectish) {
        let previous = previous_checkout_target_from_repo(repo, index)?;
        return resolve_plain_objectish_with_reflog_warnings(
            repo,
            store,
            &previous,
            reflog_warnings,
        );
    }
    match objectish.len() {
        40 => {
            if let Ok(id) = ObjectId::from_hex(GitHashAlgorithm::Sha1, objectish) {
                if store.contains_object(&id)? {
                    return Ok(id);
                }
                if promisor_pack_contains_object(repo, &id)? {
                    return Ok(id);
                }
                if partial_clone_enabled(repo).map_err(cli_error_to_io)? {
                    return Ok(id);
                }
            }
        }
        64 => {
            if let Ok(id) = ObjectId::from_hex(GitHashAlgorithm::Sha256, objectish) {
                if store.object_kind_hint(&id)?.is_some() {
                    return Ok(id);
                }
            }
        }
        _ => {}
    }

    if let Some(base) = split_upstream_suffix(objectish) {
        return resolve_upstream_suffix(repo, base);
    }
    if let Some(base) = split_push_suffix(objectish) {
        let ref_name = push_ref_name(repo, base)?;
        return resolve_repo_ref(repo, &ref_name);
    }
    if let Some((base, selector)) = split_reflog_selector_suffix(objectish) {
        return resolve_reflog_selector(repo, store, base, selector, reflog_warnings);
    }
    if objectish == "FETCH_HEAD" {
        return resolve_fetch_head(repo, store);
    }
    if is_valid_pseudoref_name(objectish)
        && let Ok(id) = resolve_pseudoref(repo, store, objectish)
    {
        return Ok(id);
    }
    if objectish == "@" {
        return resolve_repo_ref(repo, "HEAD");
    }
    if let Some(result) = resolve_worktree_ref_expression(repo, objectish) {
        return result;
    }
    if objectish == "HEAD" || objectish.starts_with("refs/") {
        return resolve_repo_ref(repo, objectish);
    }
    if let Some(ref_name) = objectish.strip_prefix("heads/") {
        return resolve_repo_ref(repo, &format!("refs/heads/{ref_name}"));
    }
    if let Some(ref_name) = objectish.strip_prefix("tags/") {
        return resolve_repo_ref(repo, &format!("refs/tags/{ref_name}"));
    }
    if let Some(id) = resolve_named_ref(repo, objectish)? {
        return Ok(id);
    }
    store.resolve_prefix(objectish)
}

fn resolve_worktree_ref_expression(
    repo: &GitRepo,
    objectish: &str,
) -> Option<io::Result<ObjectId>> {
    if let Some(name) = objectish.strip_prefix("worktree/") {
        return Some(resolve_repo_ref(repo, &format!("refs/worktree/{name}")));
    }
    let common_dir = match read_common_git_dir(&repo.git_dir).map_err(cli_error_to_io) {
        Ok(common_dir) => common_dir,
        Err(error) => return Some(Err(error)),
    };
    let (git_dir, name) = if let Some(name) = objectish.strip_prefix("main-worktree/") {
        (common_dir.clone(), name)
    } else if let Some(rest) = objectish.strip_prefix("worktrees/") {
        let (worktree, name) = rest.split_once('/')?;
        (common_dir.join("worktrees").join(worktree), name)
    } else {
        return None;
    };
    if common_ref_store(repo)
        .and_then(|refs| refs.resolve(&format!("refs/heads/{objectish}")))
        .is_ok()
    {
        eprintln!("warning: refname '{objectish}' is ambiguous.");
    }
    let algorithm = match repo_hash_algorithm_from_config(repo) {
        Ok(algorithm) => algorithm,
        Err(error) => return Some(Err(error)),
    };
    let local_refs = RefStore::new(git_dir, algorithm);
    Some(if name == "HEAD" {
        match local_refs.read_head() {
            Ok(RefTarget::Direct(id)) => Ok(id),
            Ok(RefTarget::Symbolic(target)) => match common_ref_store(repo) {
                Ok(refs) => refs.resolve(&target),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        }
    } else if name.starts_with("refs/") {
        local_refs.resolve(name)
    } else {
        local_refs.resolve(&format!("refs/worktree/{name}"))
    })
}

fn promisor_pack_contains_object(repo: &GitRepo, id: &ObjectId) -> io::Result<bool> {
    let entries = match fs::read_dir(repo.objects_dir.join("pack")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("promisor") {
            continue;
        }
        let pack_path = path.with_extension("pack");
        if !pack_path.is_file() {
            continue;
        }
        let object_ids = decode_pack_index_object_ids_from_path(id.algorithm(), &pack_path)?;
        if object_ids.iter().any(|candidate| candidate == id) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn split_upstream_suffix(objectish: &str) -> Option<&str> {
    let base = objectish.strip_suffix('}')?;
    let (base, selector) = base.rsplit_once("@{")?;
    matches!(selector, "u" | "upstream").then_some(base)
}

fn split_push_suffix(objectish: &str) -> Option<&str> {
    let base = objectish.strip_suffix('}')?;
    let (base, selector) = base.rsplit_once("@{")?;
    selector.eq_ignore_ascii_case("push").then_some(base)
}

pub(crate) fn push_ref_name(repo: &GitRepo, base: &str) -> io::Result<String> {
    let refs = common_ref_store(repo)?;
    let resolved_previous = resolve_previous_checkout_name(repo, base)?;
    let base = resolved_previous.as_deref().unwrap_or(base);
    let branch = match base {
        "" | "@" | "HEAD" => current_branch_ref(&refs)
            .map_err(|error| io::Error::other(format!("read current branch failed: {error:?}")))?
            .and_then(|ref_name| ref_name.strip_prefix("refs/heads/").map(str::to_owned))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HEAD is not a branch"))?,
        branch if branch.starts_with("refs/heads/") || branch.starts_with("heads/") => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no such branch: '{branch}'"),
            ));
        }
        branch => {
            let ref_name = format!("refs/heads/{branch}");
            refs.read_ref(&ref_name)?;
            branch.to_owned()
        }
    };
    let remote = read_config_value(repo, &format!("branch.{branch}.pushRemote"))?
        .or(read_config_value(repo, "remote.pushDefault")?)
        .or(read_config_value(repo, &format!("branch.{branch}.remote"))?);
    let Some(remote) = remote.filter(|remote| !remote.is_empty() && remote != ".") else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no push destination configured for branch '{branch}'"),
        ));
    };
    let source = format!("refs/heads/{branch}");
    let configured_push = read_config_value(repo, &format!("remote.{remote}.push"))?;
    let push_default = read_config_value(repo, "push.default")?
        .unwrap_or_else(|| "simple".to_owned())
        .to_ascii_lowercase();
    let upstream_remote = read_config_value(repo, &format!("branch.{branch}.remote"))?;
    let upstream_merge = read_config_value(repo, &format!("branch.{branch}.merge"))?;
    if configured_push.is_none() {
        match push_default.as_str() {
            "nothing" => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("push.default is nothing for branch '{branch}'"),
                ));
            }
            "simple"
                if upstream_remote.as_deref() != Some(remote.as_str())
                    || upstream_merge.as_deref() != Some(source.as_str()) =>
            {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("cannot resolve simple push for branch '{branch}'"),
                ));
            }
            _ => {}
        }
    }
    let remote_ref = configured_push
        .as_deref()
        .and_then(|refspec| map_push_refspec(refspec, &source))
        .unwrap_or(source);
    let tracking_suffix = remote_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&remote_ref);
    Ok(format!("refs/remotes/{remote}/{tracking_suffix}"))
}

fn map_push_refspec(refspec: &str, source: &str) -> Option<String> {
    let refspec = refspec.strip_prefix('+').unwrap_or(refspec);
    let (from, to) = refspec.split_once(':')?;
    match (from.split_once('*'), to.split_once('*')) {
        (Some((from_prefix, from_suffix)), Some((to_prefix, to_suffix))) => {
            let wildcard = source
                .strip_prefix(from_prefix)?
                .strip_suffix(from_suffix)?;
            Some(format!("{to_prefix}{wildcard}{to_suffix}"))
        }
        _ if from == source => Some(to.to_owned()),
        _ => None,
    }
}

fn resolve_upstream_suffix(repo: &GitRepo, base: &str) -> io::Result<ObjectId> {
    let refs = common_ref_store(repo)?;
    let resolved_previous = resolve_previous_checkout_name(repo, base)?;
    let base = resolved_previous.as_deref().unwrap_or(base);
    let branch = match base {
        "" | "@" | "HEAD" => current_branch_ref(&refs)
            .map_err(|error| io::Error::other(format!("read current branch failed: {error:?}")))?
            .and_then(|ref_name| ref_name.strip_prefix("refs/heads/").map(str::to_owned))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HEAD is not a branch"))?,
        branch if branch.starts_with("refs/heads/") || branch.starts_with("heads/") => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no such branch: '{branch}'"),
            ));
        }
        branch => {
            let ref_name = format!("refs/heads/{branch}");
            match refs.read_ref(&ref_name) {
                Ok(_) => branch.to_owned(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("no such branch: '{branch}'"),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
    };
    let upstream = read_branch_upstream(repo, &branch)
        .map_err(|error| io::Error::other(format!("read branch upstream failed: {error:?}")))?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no upstream configured for branch '{branch}'"),
            )
        })?;
    resolve_repo_ref(repo, &upstream.ref_name)
}

pub(crate) fn is_valid_pseudoref_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.ends_with(".lock")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn resolve_pseudoref(repo: &GitRepo, store: &LooseObjectStore, name: &str) -> io::Result<ObjectId> {
    let raw = fs::read_to_string(repo.git_dir.join(name))?;
    if let Some(target) = raw.trim_end().strip_prefix("ref: ") {
        return resolve_repo_ref(repo, target);
    }
    let hex = raw.split_whitespace().next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pseudo-ref {name} has no object id"),
        )
    })?;
    let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, hex)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    if store.contains_object(&id)? {
        Ok(id)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("pseudo-ref {name} object not found"),
        ))
    }
}

fn resolve_fetch_head(repo: &GitRepo, store: &LooseObjectStore) -> io::Result<ObjectId> {
    let contents = fs::read_to_string(repo.git_dir.join("FETCH_HEAD"))?;
    for line in contents.lines() {
        let Some(hex) = line.split_whitespace().next() else {
            continue;
        };
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, hex)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        if store.contains_object(&id)? {
            return Ok(id);
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "FETCH_HEAD object not found",
        ));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "FETCH_HEAD has no object ids",
    ))
}

enum ReflogSelector<'a> {
    Index(usize),
    Date(&'a str),
}

fn split_reflog_selector_suffix(objectish: &str) -> Option<(&str, ReflogSelector<'_>)> {
    let base = objectish.strip_suffix('}')?;
    let (base, selector) = base.rsplit_once("@{")?;
    if let Ok(index) = selector.parse::<usize>() {
        return Some((base, ReflogSelector::Index(index)));
    }
    Some((base, ReflogSelector::Date(selector)))
}

fn resolve_reflog_selector(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    selector: ReflogSelector<'_>,
    warnings: bool,
) -> io::Result<ObjectId> {
    let reflog_name = reflog_ref_name(repo, base)?;
    let common_dir = read_common_git_dir(&repo.git_dir).map_err(cli_error_to_io)?;
    let algorithm = super::repo_hash_algorithm_from_config(repo)?;
    let common_refs = RefStore::new(&common_dir, algorithm);
    if common_refs.storage_kind()? == zmin_git_core::refs::RefStorageKind::Reftable {
        let git_dir = if is_per_worktree_ref(&reflog_name) {
            &repo.git_dir
        } else {
            &common_dir
        };
        let refs = RefStore::new_with_storage_root(
            git_dir,
            git_dir,
            algorithm,
            zmin_git_core::refs::RefStorageKind::Reftable,
        );
        let mut records = refs
            .reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == reflog_name)
            .collect::<Vec<_>>();
        records.sort_by_key(|record| record.update_index);
        return match selector {
            ReflogSelector::Index(index) => records
                .into_iter()
                .rev()
                .nth(index)
                .map(|record| record.new_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "reflog entry not found")),
            ReflogSelector::Date(raw) => {
                let timestamp = parse_reflog_selector_timestamp(raw)?;
                records
                    .into_iter()
                    .rev()
                    .find(|record| i64::try_from(record.timestamp).unwrap_or(i64::MAX) <= timestamp)
                    .map(|record| record.new_id)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::NotFound, "reflog entry not found")
                    })
            }
        };
    }
    let path = repo.git_dir.join("logs").join(&reflog_name);
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && matches!(selector, ReflogSelector::Index(0))
                && (base.is_empty() || base == "HEAD" || base.ends_with('@')) =>
        {
            let fallback = if base.is_empty() { "HEAD" } else { base };
            return resolve_plain_objectish(repo, store, fallback);
        }
        Err(error) => return Err(error),
    };
    match selector {
        ReflogSelector::Index(index) => contents
            .lines()
            .rev()
            .filter_map(reflog_line_new_id)
            .nth(index)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "reflog entry not found")),
        ReflogSelector::Date(raw) => {
            resolve_reflog_date_selector(repo, store, base, &reflog_name, &contents, raw, warnings)
        }
    }
}

fn resolve_reflog_date_selector(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    reflog_name: &str,
    contents: &str,
    raw: &str,
    warnings: bool,
) -> io::Result<ObjectId> {
    let timestamp = parse_reflog_selector_timestamp(raw)?;
    let entries = contents
        .lines()
        .filter_map(parse_reflog_line)
        .collect::<Vec<_>>();
    let Some(first) = entries.first() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "reflog entry not found",
        ));
    };
    if timestamp < first.timestamp {
        let display_name = if base.is_empty() { "HEAD" } else { base };
        if warnings {
            eprintln!(
                "warning: log for '{display_name}' only goes back to {}",
                format_reflog_timestamp(first)
            );
        }
        return Ok(first.new_id.clone());
    }

    let selected_index = entries
        .iter()
        .rposition(|entry| entry.timestamp <= timestamp)
        .unwrap_or(0);
    let selected = &entries[selected_index];
    if let Some(next) = entries.get(selected_index + 1) {
        if warnings && next.old_id != selected.new_id {
            eprintln!(
                "warning: log for ref {reflog_name} has gap after {}",
                format_reflog_timestamp(selected)
            );
        }
        return Ok(selected.new_id.clone());
    }
    if timestamp == selected.timestamp {
        return Ok(selected.new_id.clone());
    }

    let current_name = if base.is_empty() { "HEAD" } else { base };
    let current = resolve_plain_objectish(repo, store, current_name)?;
    if warnings && current != selected.new_id {
        eprintln!(
            "warning: log for ref {reflog_name} unexpectedly ended on {}",
            format_reflog_timestamp(selected)
        );
    }
    Ok(current)
}

pub(crate) fn parse_reflog_selector_timestamp(raw: &str) -> io::Result<i64> {
    let normalized = raw.trim();
    if normalized.eq_ignore_ascii_case("now") {
        return current_unix_timestamp().map_err(cli_error_to_io);
    }
    if let Ok((timestamp, _)) = parse_git_date(normalized) {
        return Ok(timestamp);
    }
    let dotted = normalized.replace('.', " ");
    if let Ok((timestamp, _)) = parse_git_date(&dotted) {
        return Ok(timestamp);
    }
    for token in dotted.split_whitespace().rev() {
        if let Ok((timestamp, _)) = parse_git_date(token) {
            return Ok(timestamp);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid reflog date selector: {raw}"),
    ))
}

fn parse_reflog_line(line: &str) -> Option<ParsedReflogLine> {
    let header = line
        .split_once('\t')
        .map(|(header, _)| header)
        .unwrap_or(line);
    let mut fields = header.split_whitespace();
    let old_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, fields.next()?).ok()?;
    let new_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, fields.next()?).ok()?;
    let timezone = fields.next_back()?.to_owned();
    let timestamp = fields.next_back()?.parse().ok()?;
    Some(ParsedReflogLine {
        old_id,
        new_id,
        timestamp,
        timezone,
    })
}

struct ParsedReflogLine {
    old_id: ObjectId,
    new_id: ObjectId,
    timestamp: i64,
    timezone: String,
}

fn format_reflog_timestamp(entry: &ParsedReflogLine) -> String {
    use chrono::{FixedOffset, TimeZone};

    let offset = parse_reflog_timezone_offset(&entry.timezone)
        .and_then(FixedOffset::east_opt)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC offset is valid"));
    offset
        .timestamp_opt(entry.timestamp, 0)
        .single()
        .map(|date| date.format("%a, %-d %b %Y %H:%M:%S %z").to_string())
        .unwrap_or_else(|| entry.timestamp.to_string())
}

fn parse_reflog_timezone_offset(value: &str) -> Option<i32> {
    if value.len() != 5 {
        return None;
    }
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours = value[1..3].parse::<i32>().ok()?;
    let minutes = value[3..5].parse::<i32>().ok()?;
    Some(sign * (hours * 60 + minutes) * 60)
}

fn reflog_ref_name(repo: &GitRepo, base: &str) -> io::Result<String> {
    let resolved_previous = resolve_previous_checkout_name(repo, base)?;
    let base = resolved_previous.as_deref().unwrap_or(base);
    if base.is_empty() || base == "HEAD" {
        return Ok("HEAD".to_owned());
    }
    if base == "stash" {
        return Ok("refs/stash".to_owned());
    }
    if base.starts_with("refs/") {
        return Ok(base.to_owned());
    }
    if let Some(ref_name) = base.strip_prefix("heads/") {
        return Ok(format!("refs/heads/{ref_name}"));
    }
    let refs = common_ref_store(repo)?;
    let branch = format!("refs/heads/{base}");
    match refs.read_ref(&branch) {
        Ok(_) => return Ok(branch),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(base.to_owned())
}

fn reflog_line_new_id(line: &str) -> Option<ObjectId> {
    let mut fields = line.split_whitespace();
    fields.next()?;
    ObjectId::from_hex(GitHashAlgorithm::Sha1, fields.next()?).ok()
}

fn split_peel_suffix(objectish: &str) -> Option<(&str, &str)> {
    for suffix in [
        "^{commit}",
        "^{tree}",
        "^{blob}",
        "^{tag}",
        "^{object}",
        "^{}",
    ] {
        if let Some(base) = objectish.strip_suffix(suffix) {
            return Some((base, suffix));
        }
    }
    None
}

fn resolve_typed_objectish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    peel: &str,
) -> io::Result<ObjectId> {
    if base.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty object name before peel operator",
        ));
    }
    if peel == "^{tree}" {
        return resolve_treeish(repo, &store, base);
    }
    let mut id = if base.contains('~') {
        resolve_commitish_io(repo, &store, base)?
    } else {
        resolve_plain_objectish(repo, &store, base)?
    };
    match peel {
        "^{object}" => {
            store.read_object(&id)?;
            Ok(id)
        }
        "^{tag}" => {
            let object = store.read_object(&id)?;
            if object.kind == GitObjectKind::Tag {
                Ok(id)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("revision `{base}` is not a tag"),
                ))
            }
        }
        "^{blob}" | "^{}" | "^{commit}" => {
            for _ in 0..8 {
                let object = store.read_object(&id)?;
                match object.kind {
                    GitObjectKind::Tag => {
                        id = decode_tag(GitHashAlgorithm::Sha1, &object.content)?.target;
                    }
                    GitObjectKind::Blob if peel == "^{blob}" || peel == "^{}" => {
                        return Ok(id);
                    }
                    GitObjectKind::Commit if peel == "^{commit}" || peel == "^{}" => {
                        return Ok(id);
                    }
                    _ if peel == "^{}" => return Ok(id),
                    _ => {
                        let expected = match peel {
                            "^{blob}" => "blob",
                            _ => "commit",
                        };
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("revision `{base}` is not a {expected}"),
                        ));
                    }
                }
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tag nesting is too deep",
            ))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid peel operator {peel}"),
        )),
    }
}

pub(crate) fn resolve_treeish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    treeish: &str,
) -> io::Result<ObjectId> {
    let commit_cache = CommitObjectCache::new(store);
    let mut id = if let Some(pattern) = treeish.strip_prefix(":/") {
        resolve_message_search_from_refs(repo, store, pattern)?
    } else if let Some((base, pattern)) = split_message_search_suffix(treeish) {
        resolve_message_search_from_base(repo, store, base, pattern)?
    } else {
        resolve_objectish(repo, treeish)?
    };
    for _ in 0..8 {
        let object = store.read_object(&id)?;
        match object.kind {
            GitObjectKind::Tree => return Ok(id),
            GitObjectKind::Commit => {
                return Ok(commit_cache.read_commit(&id)?.tree.clone());
            }
            GitObjectKind::Tag => {
                id = decode_tag(GitHashAlgorithm::Sha1, &object.content)?.target;
            }
            GitObjectKind::Blob => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "object cannot be used as a tree",
                ));
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "tag nesting is too deep",
    ))
}

pub(crate) fn normalize_git_path(path: &str) -> io::Result<String> {
    let normalized = path
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_owned();
    if normalized.contains('\0')
        || normalized
            .split('/')
            .any(|component| component == "." || component == "..")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid git tree path",
        ));
    }
    Ok(normalized)
}

pub(crate) fn resolve_named_ref(repo: &GitRepo, name: &str) -> io::Result<Option<ObjectId>> {
    let refs = common_ref_store(repo)?;
    let remote_name = name.strip_prefix("remotes/").unwrap_or(name);
    let mut candidates = vec![format!("refs/heads/{name}"), format!("refs/tags/{name}")];
    if name == "stash" {
        candidates.push("refs/stash".to_owned());
    }
    if !remote_name.contains('/') {
        candidates.push(format!("refs/remotes/{remote_name}/HEAD"));
    }
    candidates.push(format!("refs/remotes/{remote_name}"));
    let mut resolved = None;
    for candidate in candidates {
        match refs.resolve(&candidate) {
            Ok(id) => {
                if resolved.is_none() {
                    resolved = Some(id);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::NotADirectory
                        | io::ErrorKind::IsADirectory
                ) => {}
            Err(error)
                if error.kind() == io::ErrorKind::InvalidData
                    && fs::read_to_string(refs.git_dir().join(&candidate))
                        .ok()
                        .is_some_and(|raw| raw.trim_start().starts_with("ref:")) =>
            {
                eprintln!("warning: ignoring dangling symref {candidate}");
            }
            Err(error) => return Err(error),
        }
    }
    if resolved.is_some() {
        return Ok(resolved);
    }
    if !name.contains('/')
        && let Some(id) = resolve_unique_remote_tracking_ref(&refs, name)?
    {
        return Ok(Some(id));
    }
    Ok(None)
}

fn resolve_unique_remote_tracking_ref(
    refs: &RefStore,
    short_name: &str,
) -> io::Result<Option<ObjectId>> {
    let mut resolved = None::<ObjectId>;
    let mut ambiguous = false;
    refs.for_each_resolved_ref("refs/remotes/", |ref_name, id| {
        if ref_name.ends_with("/HEAD") || !ref_name.ends_with(&format!("/{short_name}")) {
            return Ok(());
        }
        match &resolved {
            None => resolved = Some(id.clone()),
            Some(existing) if existing == id => {}
            Some(_) => ambiguous = true,
        }
        Ok::<(), io::Error>(())
    })?;
    if ambiguous {
        return Ok(None);
    }
    Ok(resolved)
}

fn common_ref_store(repo: &GitRepo) -> io::Result<RefStore> {
    let common_dir = read_common_git_dir(&repo.git_dir).map_err(cli_error_to_io)?;
    Ok(RefStore::new(
        common_dir,
        repo_hash_algorithm_from_config(repo)?,
    ))
}

fn resolve_repo_ref(repo: &GitRepo, name: &str) -> io::Result<ObjectId> {
    if name != "HEAD" {
        let algorithm = repo_hash_algorithm_from_config(repo)?;
        let refs = if is_per_worktree_ref(name) {
            RefStore::new(&repo.git_dir, algorithm)
        } else {
            common_ref_store(repo)?
        };
        return refs.resolve(name);
    }
    let head_refs = RefStore::new(&repo.git_dir, repo_hash_algorithm_from_config(repo)?);
    match head_refs.read_head()? {
        RefTarget::Direct(id) => Ok(id),
        RefTarget::Symbolic(target) => common_ref_store(repo)?.resolve(&target),
    }
}

fn cli_error_to_io(error: CliError) -> io::Error {
    match error {
        CliError::Io(error) => error,
        other => io::Error::other(format!("{other:?}")),
    }
}

pub(crate) fn short_object_id(id: &ObjectId) -> String {
    short_object_id_len(id, 7)
}

pub(crate) fn short_object_id_len(id: &ObjectId, len: usize) -> String {
    id.short_hex(len.min(id.hex_len()))
}

pub(crate) fn default_abbrev_len(store: &LooseObjectStore) -> Result<usize> {
    default_abbrev_len_for_store(store)
}

pub(crate) fn configured_default_abbrev_len(
    repo: &GitRepo,
    store: &LooseObjectStore,
) -> Result<usize> {
    let Some(minimum) = configured_abbrev_minimum(repo, store)? else {
        return Ok(GitHashAlgorithm::Sha1.digest_len() * 2);
    };
    default_abbrev_len_for_store_with_minimum(store, minimum)
}

pub(crate) fn configured_default_abbrev_len_for_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
    ids: &[ObjectId],
) -> Result<usize> {
    let Some(minimum) = configured_abbrev_minimum(repo, store)? else {
        return Ok(GitHashAlgorithm::Sha1.digest_len() * 2);
    };
    default_abbrev_len_for_ids_with_minimum(store, ids, minimum)
}

fn configured_abbrev_minimum(repo: &GitRepo, store: &LooseObjectStore) -> Result<Option<usize>> {
    const MINIMUM_ABBREV: usize = 4;

    let Some(entry) = read_config_entry(repo, "core.abbrev")? else {
        return Ok(Some(default_auto_abbrev_len(store)?));
    };
    let value = &entry.value;
    if value.eq_ignore_ascii_case("auto") {
        return Ok(Some(default_auto_abbrev_len(store)?));
    }
    if value.is_empty()
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("off")
    {
        return Ok(None);
    }
    let minimum = value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!(
            "bad numeric config value '{}' for 'core.abbrev': invalid unit",
            value
        ),
    })?;
    if minimum < MINIMUM_ABBREV {
        let command_line_suffix = if entry.scope == ConfigScope::Command {
            "fatal: unable to parse 'core.abbrev' from command-line config\n"
        } else {
            ""
        };
        return Err(CliError::Stderr {
            code: 128,
            text: format!("error: abbrev length out of range: {minimum}\n{command_line_suffix}"),
        });
    }
    Ok(Some(minimum))
}

pub(crate) fn auto_abbrev_len_from_object_count(object_count: usize) -> usize {
    const MIN_ABBREV: usize = 7;
    if object_count == 0 {
        return MIN_ABBREV;
    }
    let squared = (object_count as u128).saturating_mul(object_count as u128);
    let hex_digits = ((u128::BITS as usize) - squared.leading_zeros() as usize).div_ceil(4);
    MIN_ABBREV.max(hex_digits)
}

pub(crate) fn default_auto_abbrev_len(store: &impl GitObjectStore) -> Result<usize> {
    Ok(auto_abbrev_len_from_object_count(
        store.object_id_capacity_hint()?,
    ))
}

pub(crate) fn default_abbrev_len_for_ids(
    store: &LooseObjectStore,
    ids: &[ObjectId],
) -> Result<usize> {
    default_abbrev_len_for_ids_with_minimum(store, ids, default_auto_abbrev_len(store)?)
}

fn default_abbrev_len_for_ids_with_minimum(
    store: &LooseObjectStore,
    ids: &[ObjectId],
    minimum: usize,
) -> Result<usize> {
    store
        .minimum_unique_abbrev_len_for_ids(ids, minimum)
        .map_err(CliError::Io)
}

fn default_abbrev_len_for_store(store: &impl GitObjectStore) -> Result<usize> {
    default_abbrev_len_for_store_with_minimum(store, default_auto_abbrev_len(store)?)
}

fn default_abbrev_len_for_store_with_minimum(
    store: &impl GitObjectStore,
    minimum: usize,
) -> Result<usize> {
    let full_len = GitHashAlgorithm::Sha1.digest_len() * 2;
    if minimum >= full_len {
        return Ok(full_len);
    }
    let mut ids = Vec::with_capacity(default_abbrev_object_id_initial_capacity(
        store.object_id_capacity_hint()?,
    ));
    store.for_each_object_id(&mut |id| {
        ids.push(id.clone());
        Ok(())
    })?;
    ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

    let mut required = minimum;
    for pair in ids.windows(2) {
        let [left, right] = pair else {
            continue;
        };
        if left.as_bytes() == right.as_bytes() {
            continue;
        }
        required = required.max(object_hex_common_prefix_len(left, right) + 1);
    }
    Ok(required.min(full_len))
}

fn default_abbrev_object_id_initial_capacity(object_hint: usize) -> usize {
    object_hint.min(DEFAULT_ABBREV_OBJECT_ID_INITIAL_CAPACITY_LIMIT)
}

fn object_hex_common_prefix_len(left: &ObjectId, right: &ObjectId) -> usize {
    let mut len = 0_usize;
    for (left, right) in left.as_bytes().iter().zip(right.as_bytes()) {
        if left == right {
            len += 2;
            continue;
        }
        if left >> 4 == right >> 4 {
            len += 1;
        }
        break;
    }
    len
}

pub(crate) fn parse_object_kind(value: &str) -> Result<GitObjectKind> {
    match value {
        "blob" => Ok(GitObjectKind::Blob),
        "tree" => Ok(GitObjectKind::Tree),
        "commit" => Ok(GitObjectKind::Commit),
        "tag" => Ok(GitObjectKind::Tag),
        _ => Err(CliError::Message(format!(
            "unsupported git object type `{value}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha1_id(hex: &str) -> ObjectId {
        ObjectId::from_hex(GitHashAlgorithm::Sha1, hex).expect("sha1 id")
    }

    #[test]
    fn object_hex_common_prefix_len_handles_odd_and_even_lengths() {
        let left = sha1_id("a012345678901234567890123456789012345678");
        let same_first_nibble = sha1_id("af12345678901234567890123456789012345678");
        let different_first_nibble = sha1_id("b012345678901234567890123456789012345678");

        assert_eq!(object_hex_common_prefix_len(&left, &same_first_nibble), 1);
        assert_eq!(
            object_hex_common_prefix_len(&left, &different_first_nibble),
            0
        );
    }

    #[test]
    fn auto_abbrev_len_stays_at_minimum_for_small_object_counts() {
        assert_eq!(auto_abbrev_len_from_object_count(0), 7);
        assert_eq!(auto_abbrev_len_from_object_count(3), 7);
        assert_eq!(auto_abbrev_len_from_object_count(10_000), 7);
    }

    #[test]
    fn auto_abbrev_len_grows_with_repo_object_count() {
        assert_eq!(auto_abbrev_len_from_object_count(16_384), 8);
        assert_eq!(auto_abbrev_len_from_object_count(27_688), 8);
        assert_eq!(auto_abbrev_len_from_object_count(1_000_000), 10);
    }

    struct CountingObjectStore {
        ids: Vec<ObjectId>,
        calls: std::cell::Cell<usize>,
    }

    impl GitObjectStore for CountingObjectStore {
        fn read_object(&self, _id: &ObjectId) -> io::Result<zmin_git_core::LooseObject> {
            Err(io::Error::new(io::ErrorKind::NotFound, "test store"))
        }

        fn object_id_capacity_hint(&self) -> io::Result<usize> {
            Ok(self.ids.len())
        }

        fn for_each_object_id(
            &self,
            for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
        ) -> io::Result<()> {
            self.calls.set(self.calls.get() + 1);
            for id in &self.ids {
                for_each(id)?;
            }
            Ok(())
        }
    }

    #[test]
    fn default_abbrev_len_scans_object_ids_once() {
        let store = CountingObjectStore {
            ids: vec![
                sha1_id("abc0000000000000000000000000000000000000"),
                sha1_id("abc0001000000000000000000000000000000000"),
                sha1_id("def0000000000000000000000000000000000000"),
            ],
            calls: std::cell::Cell::new(0),
        };

        assert_eq!(default_abbrev_len_for_store(&store).unwrap(), 7);
        assert_eq!(store.calls.get(), 1);
    }

    #[test]
    fn default_abbrev_len_extends_only_for_real_collisions() {
        let store = CountingObjectStore {
            ids: vec![
                sha1_id("1234567000000000000000000000000000000000"),
                sha1_id("1234567100000000000000000000000000000000"),
                sha1_id("1234567100000000000000000000000000000000"),
            ],
            calls: std::cell::Cell::new(0),
        };

        assert_eq!(default_abbrev_len_for_store(&store).unwrap(), 8);
        assert_eq!(store.calls.get(), 1);
    }

    #[test]
    fn default_abbrev_initial_capacity_is_bounded() {
        assert_eq!(
            default_abbrev_object_id_initial_capacity(usize::MAX),
            DEFAULT_ABBREV_OBJECT_ID_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(default_abbrev_object_id_initial_capacity(2), 2);
        assert_eq!(default_abbrev_object_id_initial_capacity(0), 0);
    }
}
