use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;

use regex::bytes::Regex;
use zmin_git_core::{
    CommitObjectCache, GitHashAlgorithm, GitObjectKind, LooseObjectStore, ObjectId, RefStore,
    RefTarget, decode_pack_index_object_ids_from_path, decode_tag, find_tree_entry,
    read_index_with_algorithm,
};

use super::{
    CliError, GitRepo, Result, auto_abbrev_len_from_object_count, current_branch_ref,
    current_unix_timestamp, expand_repo_sparse_index, is_per_worktree_ref,
    parse_git_config_integer, parse_git_date, partial_clone_enabled,
    previous_checkout_syntax_index, previous_checkout_target_from_repo, read_branch_upstream,
    read_common_git_dir, read_config_value, repo_hash_algorithm_from_config, repo_relative_path,
    resolve_commitish_io, resolve_previous_checkout_expression, resolve_previous_checkout_name,
    signature_timestamp, sparse_index_path_requires_expansion, trace2_region,
};

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
    let id = resolve_objectish_with_reflog_warnings(repo, rev, !quiet).map_err(|error| {
        if verify && quiet {
            return CliError::Exit(1);
        }
        if verify {
            CliError::Fatal {
                code: 128,
                message: if rev.contains("@{") {
                    rev_parse_failure_message(repo, rev, &error)
                } else {
                    "Needed a single revision".to_owned()
                },
            }
        } else {
            CliError::Fatal {
                code: 128,
                message: rev_parse_failure_message(repo, rev, &error),
            }
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

fn rev_parse_failure_message(repo: &GitRepo, rev: &str, error: &io::Error) -> String {
    let error_text = error.to_string();
    if error_text.starts_with("no upstream configured")
        || error_text.starts_with("no such branch:")
        || error_text.starts_with("upstream branch ")
    {
        return error_text;
    }
    if error_text == "HEAD is not a branch" {
        return "HEAD does not point to a branch".to_owned();
    }
    if error_text.starts_with("reflog for ") && error_text.ends_with(" does not exist") {
        return "Needed a single revision".to_owned();
    }
    if error_text.starts_with("log for ") && error_text.ends_with(" is empty") {
        return error_text;
    }
    if let Some((base, path)) = split_objectish_path(rev) {
        let stage = path.as_bytes().first().and_then(|byte| {
            (b'0'..=b'3')
                .contains(byte)
                .then(|| path.as_bytes().get(1) == Some(&b':'))
                .and_then(|is_stage| is_stage.then_some(*byte - b'0'))
        });
        let raw_path = stage.map(|_| path.get(2..).unwrap_or(path)).unwrap_or(path);
        let display_path =
            normalize_repo_object_path(repo, raw_path).unwrap_or_else(|_| raw_path.to_owned());
        if base.is_empty() {
            let cwd_prefix = std::env::current_dir()
                .ok()
                .and_then(|cwd| repo_relative_path(&repo.root, &cwd).ok())
                .map(|relative| PathBuf::from(String::from_utf8_lossy(&relative).into_owned()))
                .unwrap_or_default();
            let candidate_path = repo.root.join(&cwd_prefix).join(&display_path);
            let candidate_display = candidate_path
                .strip_prefix(&repo.root)
                .unwrap_or(&candidate_path)
                .display()
                .to_string();
            if let Ok(index) = read_index_with_algorithm(
                &repo.index_path,
                repo_hash_algorithm_from_config(repo).unwrap_or(GitHashAlgorithm::Sha1),
            ) {
                if stage.is_none()
                    && let Some(entry) = index.entries().iter().find(|entry| {
                        entry.path.strip_prefix(display_path.as_bytes()).is_none()
                            && entry.path.ends_with(display_path.as_bytes())
                            && entry.path.len() > display_path.len()
                    })
                {
                    let candidate_display = String::from_utf8_lossy(&entry.path);
                    return format!(
                        "path '{candidate_display}' is in the index, but not '{display_path}'\n\
hint: Did you mean ':0:{candidate_display}' aka ':0:./{display_path}'?"
                    );
                }
                if stage.is_none()
                    && candidate_display != display_path
                    && index
                        .entries()
                        .iter()
                        .any(|entry| entry.path.as_slice() == candidate_display.as_bytes())
                {
                    return format!(
                        "path '{candidate_display}' is in the index, but not '{display_path}'\n\
hint: Did you mean ':0:{candidate_display}' aka ':0:./{display_path}'?"
                    );
                }
                if let Some(entry) = index.entries().iter().find(|entry| {
                    (entry.path.as_slice() == display_path.as_bytes()
                        || entry.path.as_slice() == candidate_display.as_bytes())
                        && stage.is_some_and(|requested| requested != entry.stage)
                }) {
                    let requested = stage.unwrap_or(0);
                    let use_candidate = entry.path.as_slice() == candidate_display.as_bytes()
                        && candidate_display != display_path;
                    let shown_path = if use_candidate {
                        candidate_display.as_str()
                    } else {
                        display_path.as_str()
                    };
                    let hint_path = if use_candidate {
                        format!("':0:{candidate_display}' aka ':0:./{display_path}'")
                    } else {
                        format!("':0:{display_path}'")
                    };
                    if use_candidate {
                        return format!(
                            "path '{candidate_display}' is in the index, but not '{display_path}'\n\
hint: Did you mean {hint_path}?"
                        );
                    }
                    return format!(
                        "path '{shown_path}' is in the index, but not at stage {requested}\n\
hint: Did you mean {hint_path}?"
                    );
                }
            }
            if stage.is_none()
                && !cwd_prefix.as_os_str().is_empty()
                && candidate_display != display_path
            {
                return format!(
                    "path '{candidate_display}' is in the index, but not '{display_path}'\n\
hint: Did you mean ':0:{candidate_display}' aka ':0:./{display_path}'?"
                );
            }
            if stage.is_none() && !display_path.contains('/') {
                if let Ok(entries) = fs::read_dir(&repo.root) {
                    for entry in entries.flatten() {
                        let candidate = entry.path().join(&display_path);
                        if !candidate.is_file() {
                            continue;
                        }
                        let Ok(relative) = candidate.strip_prefix(&repo.root) else {
                            continue;
                        };
                        let candidate_display = relative.display();
                        return format!(
                            "path '{candidate_display}' is in the index, but not '{display_path}'\n\
hint: Did you mean ':0:{candidate_display}' aka ':0:./{display_path}'?"
                        );
                    }
                }
                if let Ok(cwd) = std::env::current_dir()
                    && let Some(component) = cwd.file_name().and_then(|name| name.to_str())
                    && cwd.parent().is_some_and(|parent| {
                        parent.join(&display_path).is_file()
                            || parent.join(component).join(&display_path).is_file()
                    })
                {
                    let candidate_display = format!("{component}/{display_path}");
                    return format!(
                        "path '{candidate_display}' is in the index, but not '{display_path}'\n\
hint: Did you mean ':0:{candidate_display}' aka ':0:./{display_path}'?"
                    );
                }
            }
            if repo.root.join(&display_path).exists() {
                return format!("path '{display_path}' exists on disk, but not in the index");
            }
            return format!(
                "path '{display_path}' does not exist (neither on disk nor in the index)"
            );
        }
        if error.to_string().contains("relative path syntax") {
            return error.to_string();
        }
        if path.contains("..") || error.to_string().contains("outside") {
            return format!("path '{display_path}' is outside repository");
        }
        if resolve_objectish(repo, base).is_err() {
            return format!("invalid object name '{base}'.");
        }
        if !repo.root.join(&display_path).exists() {
            let cwd_prefix = std::env::current_dir()
                .ok()
                .and_then(|cwd| repo_relative_path(&repo.root, &cwd).ok())
                .map(|relative| PathBuf::from(String::from_utf8_lossy(&relative).into_owned()))
                .unwrap_or_default();
            let candidate = repo.root.join(&cwd_prefix).join(raw_path);
            if candidate.exists() {
                let candidate_display = candidate
                    .strip_prefix(&repo.root)
                    .unwrap_or(&candidate)
                    .display();
                return format!(
                    "path '{candidate_display}' exists, but not '{display_path}'\n\
hint: Did you mean '{base}:{candidate_display}' aka '{base}:./{display_path}'?"
                );
            }
            return format!("path '{display_path}' does not exist in '{base}'");
        }
        return format!("path '{display_path}' exists on disk, but not in '{base}'");
    }
    if rev.contains("@{") && error.to_string().contains("reflog") {
        return format!("log for '{rev}' only has 0 entries");
    }
    format!(
        "ambiguous argument '{rev}': unknown revision or path not in the working tree.\n\
Use '--' to separate paths from revisions, like this:\n\
'git <command> [<revision>...] -- [<file>...]'"
    )
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
        let path = normalize_repo_object_path(repo, path)?;
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
    let path = normalize_repo_object_path(repo, path)?;
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
    if let Some(id) = resolve_full_objectish(repo, store, objectish)? {
        return Ok(id);
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

fn resolve_full_objectish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
) -> io::Result<Option<ObjectId>> {
    let algorithm = store.algorithm();
    if objectish.len() != algorithm.digest_len() * 2 {
        return Ok(None);
    }
    let Ok(id) = ObjectId::from_hex(algorithm, objectish) else {
        return Ok(None);
    };
    let available = match algorithm {
        GitHashAlgorithm::Sha1 => {
            store.contains_object(&id)?
                || promisor_pack_contains_object(repo, &id)?
                || partial_clone_enabled(repo).map_err(cli_error_to_io)?
        }
        GitHashAlgorithm::Sha256 => store.object_kind_hint(&id)?.is_some(),
    };
    Ok(available.then_some(id))
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
    selector
        .eq_ignore_ascii_case("u")
        .then_some(base)
        .or_else(|| selector.eq_ignore_ascii_case("upstream").then_some(base))
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
    let ref_name = upstream_ref_name(repo, base)?;
    resolve_repo_ref(repo, &ref_name)
}

pub(crate) fn upstream_ref_name(repo: &GitRepo, base: &str) -> io::Result<String> {
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
    match refs.read_ref(&upstream.ref_name) {
        Ok(_) => Ok(upstream.ref_name),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "upstream branch '{}' not stored as a remote-tracking branch",
                upstream.ref_name
            ),
        )),
        Err(error) => Err(error),
    }
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
    let id = ObjectId::from_hex(store.algorithm(), hex)
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
        let id = ObjectId::from_hex(store.algorithm(), hex)
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
    if selector.contains("@{") {
        return None;
    }
    Some((base, classify_reflog_selector(selector)))
}

fn classify_reflog_selector(raw: &str) -> ReflogSelector<'_> {
    const MAX_ORDINAL: u64 = 100_000_000;
    let mut value = 0_u64;
    let mut all_digits = !raw.is_empty();
    for byte in raw.bytes() {
        let Some(digit) = byte.checked_sub(b'0').filter(|digit| *digit <= 9) else {
            all_digits = false;
            break;
        };
        let Some(next) = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit)))
        else {
            return ReflogSelector::Date(raw);
        };
        value = next;
        if value >= MAX_ORDINAL {
            return ReflogSelector::Date(raw);
        }
    }
    if all_digits && value <= usize::MAX as u64 {
        ReflogSelector::Index(value as usize)
    } else {
        ReflogSelector::Date(raw)
    }
}

fn resolve_reflog_selector(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    selector: ReflogSelector<'_>,
    warnings: bool,
) -> io::Result<ObjectId> {
    let resolved_base = split_upstream_suffix(base)
        .map(|upstream| upstream_ref_name(repo, upstream))
        .transpose()?;
    let reflog_name = reflog_ref_name(repo, resolved_base.as_deref().unwrap_or(base))?;
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
        return match selector {
            ReflogSelector::Index(index) => {
                let mut scan = ReflogOrdinalScan::default();
                refs.for_each_reftable_log_newest_first(&reflog_name, |record| {
                    scan.observe(record.old_id, record.new_id, index)
                })?;
                if let Some(selected) = scan.selected {
                    return Ok(selected);
                }
                if scan.count == 0 {
                    return if refs.reftable_log_exists(&reflog_name)? {
                        resolve_empty_reflog(repo, store, &reflog_name, index)
                    } else {
                        Err(reflog_missing_error(&reflog_name))
                    };
                }
                if index == scan.count
                    && let Some(oldest_old_id) = scan.oldest_old_id
                    && !is_zero_object_id(&oldest_old_id)
                {
                    return Ok(oldest_old_id);
                }
                Err(reflog_count_error(base, scan.count))
            }
            ReflogSelector::Date(raw) => {
                let timestamp = parse_reflog_selector_timestamp(raw)?;
                let mut scan = ReflogNewestFirstDateScan::new(timestamp);
                refs.for_each_reftable_log_newest_first(&reflog_name, |record| {
                    scan.observe(ReflogEntrySummary {
                        old_id: record.old_id,
                        new_id: record.new_id,
                        timestamp: i64::try_from(record.timestamp).unwrap_or(i64::MAX),
                        timezone: format_reflog_timezone(record.timezone_offset),
                    });
                    Ok(())
                })?;
                if scan.oldest.is_none() && !refs.reftable_log_exists(&reflog_name)? {
                    return Err(reflog_missing_error(&reflog_name));
                }
                finish_reflog_newest_first_scan(repo, store, base, &reflog_name, scan, warnings)
            }
        };
    }
    let log_root = if is_per_worktree_ref(&reflog_name) {
        repo.git_dir.clone()
    } else {
        common_dir.clone()
    };
    let path = log_root.join("logs").join(&reflog_name);
    match selector {
        ReflogSelector::Index(index) => {
            resolve_file_reflog_ordinal(repo, store, &path, algorithm, base, &reflog_name, index)
        }
        ReflogSelector::Date(raw) => {
            let file = File::open(path).map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    reflog_missing_error(&reflog_name)
                } else {
                    error
                }
            })?;
            let mut cursor = FileReflogCursor::new(file, algorithm);
            resolve_reflog_date_cursor(repo, store, base, &reflog_name, &mut cursor, raw, warnings)
        }
    }
}

fn resolve_reflog_date_cursor(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    reflog_name: &str,
    cursor: &mut FileReflogCursor,
    raw: &str,
    warnings: bool,
) -> io::Result<ObjectId> {
    let timestamp = parse_reflog_selector_timestamp(raw)?;
    let mut scan = ReflogDateScan::new(timestamp);
    while let Some(entry) = cursor.next_entry()? {
        scan.observe(entry);
    }
    finish_reflog_date_scan(repo, store, base, reflog_name, scan, warnings)
}

fn resolve_file_reflog_ordinal(
    repo: &GitRepo,
    store: &LooseObjectStore,
    path: &std::path::Path,
    algorithm: GitHashAlgorithm,
    base: &str,
    reflog_name: &str,
    index: usize,
) -> io::Result<ObjectId> {
    let scan = match scan_file_reflog_ordinal(path, algorithm) {
        Ok(scan) => scan,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(reflog_missing_error(reflog_name));
        }
        Err(error) => return Err(error),
    };
    if scan.count == 0 {
        return resolve_empty_reflog(repo, store, reflog_name, index);
    }
    if index < scan.count {
        let target_from_oldest = scan.count - 1 - index;
        let file = File::open(path)?;
        let mut cursor = FileReflogCursor::new(file, algorithm);
        let mut current = 0;
        while let Some(entry) = cursor.next_entry()? {
            if current == target_from_oldest {
                return Ok(entry.new_id);
            }
            current += 1;
        }
        return Err(io::Error::other("reflog changed during ordinal resolution"));
    }
    if index == scan.count
        && let Some(oldest_old_id) = scan.oldest_old_id
        && !is_zero_object_id(&oldest_old_id)
    {
        return Ok(oldest_old_id);
    }
    Err(reflog_count_error(base, scan.count))
}

fn scan_file_reflog_ordinal(
    path: &std::path::Path,
    algorithm: GitHashAlgorithm,
) -> io::Result<ReflogOrdinalScan> {
    let file = File::open(path)?;
    let mut cursor = FileReflogCursor::new(file, algorithm);
    let mut scan = ReflogOrdinalScan::default();
    while let Some(entry) = cursor.next_entry()? {
        scan.observe(entry.old_id, entry.new_id, usize::MAX)?;
    }
    Ok(scan)
}

fn resolve_empty_reflog(
    repo: &GitRepo,
    store: &LooseObjectStore,
    reflog_name: &str,
    index: usize,
) -> io::Result<ObjectId> {
    if index == 0 {
        return resolve_plain_objectish(repo, store, reflog_name);
    }
    Err(reflog_empty_error(reflog_name))
}

fn reflog_empty_error(reflog_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("log for {reflog_name} is empty"),
    )
}

fn reflog_missing_error(reflog_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("reflog for {reflog_name} does not exist"),
    )
}

fn reflog_count_error(base: &str, count: usize) -> io::Error {
    let display_name = if base.is_empty() { "HEAD" } else { base };
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("log for '{display_name}' only has {count} entries"),
    )
}

fn is_zero_object_id(id: &ObjectId) -> bool {
    id.as_bytes().iter().all(|byte| *byte == 0)
}

fn finish_reflog_date_scan(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    reflog_name: &str,
    scan: ReflogDateScan,
    warnings: bool,
) -> io::Result<ObjectId> {
    let Some(first) = scan.first else {
        return Err(reflog_empty_error(reflog_name));
    };
    if scan.target < first.timestamp {
        let display_name = if base.is_empty() { "HEAD" } else { base };
        if warnings {
            eprintln!(
                "warning: log for '{display_name}' only goes back to {}",
                format_reflog_timestamp(&first)
            );
        }
        return Ok(if is_zero_object_id(&first.old_id) {
            first.new_id
        } else {
            first.old_id
        });
    }

    let selected = scan.latest_eligible.unwrap_or(first);
    if let Some(next) = scan.immediate_successor {
        if warnings && next.old_id != selected.new_id && !is_zero_object_id(&next.old_id) {
            eprintln!(
                "warning: log for ref {reflog_name} has gap after {}",
                format_reflog_timestamp(&selected)
            );
        }
        return Ok(selected.new_id);
    }
    if scan.target == selected.timestamp {
        return Ok(selected.new_id);
    }

    let newest = scan.newest.as_ref().unwrap_or(&selected);
    let current_name = if base.is_empty() { "HEAD" } else { base };
    let current = resolve_plain_objectish(repo, store, current_name)?;
    if warnings && current != newest.new_id {
        eprintln!(
            "warning: log for ref {reflog_name} unexpectedly ended on {}",
            format_reflog_timestamp(newest)
        );
    }
    Ok(current)
}

fn finish_reflog_newest_first_scan(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    reflog_name: &str,
    scan: ReflogNewestFirstDateScan,
    warnings: bool,
) -> io::Result<ObjectId> {
    let Some(oldest) = scan.oldest else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("log for {reflog_name} is empty"),
        ));
    };
    let Some(selected) = scan.selected else {
        let display_name = if base.is_empty() { "HEAD" } else { base };
        if warnings {
            eprintln!(
                "warning: log for '{display_name}' only goes back to {}",
                format_reflog_timestamp(&oldest)
            );
        }
        return Ok(if is_zero_object_id(&oldest.old_id) {
            oldest.new_id
        } else {
            oldest.old_id
        });
    };
    if let Some(successor) = scan.successor {
        if warnings && successor.old_id != selected.new_id && !is_zero_object_id(&successor.old_id)
        {
            eprintln!(
                "warning: log for ref {reflog_name} has gap after {}",
                format_reflog_timestamp(&selected)
            );
        }
        return Ok(selected.new_id);
    }
    if scan.target == selected.timestamp {
        return Ok(selected.new_id);
    }
    let current_name = if base.is_empty() { "HEAD" } else { base };
    let current = resolve_plain_objectish(repo, store, current_name)?;
    if warnings && current != selected.new_id {
        eprintln!(
            "warning: log for ref {reflog_name} unexpectedly ended on {}",
            format_reflog_timestamp(&selected)
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
    let dotted = normalized.replace('.', "-");
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

#[derive(Clone)]
struct ReflogEntrySummary {
    old_id: ObjectId,
    new_id: ObjectId,
    timestamp: i64,
    timezone: String,
}

struct ReflogDateScan {
    target: i64,
    first: Option<ReflogEntrySummary>,
    newest: Option<ReflogEntrySummary>,
    latest_eligible: Option<ReflogEntrySummary>,
    immediate_successor: Option<ReflogEntrySummary>,
}

#[derive(Default)]
struct ReflogOrdinalScan {
    count: usize,
    oldest_old_id: Option<ObjectId>,
    selected: Option<ObjectId>,
}

impl ReflogOrdinalScan {
    fn observe(&mut self, old_id: ObjectId, new_id: ObjectId, wanted: usize) -> io::Result<()> {
        let index = self.count;
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| io::Error::other("reflog entry count overflow"))?;
        self.oldest_old_id = Some(old_id);
        if index == wanted {
            self.selected = Some(new_id);
        }
        Ok(())
    }
}

struct ReflogNewestFirstDateScan {
    target: i64,
    newest: Option<ReflogEntrySummary>,
    oldest: Option<ReflogEntrySummary>,
    previous_newer: Option<ReflogEntrySummary>,
    selected: Option<ReflogEntrySummary>,
    successor: Option<ReflogEntrySummary>,
}

impl ReflogNewestFirstDateScan {
    fn new(target: i64) -> Self {
        Self {
            target,
            newest: None,
            oldest: None,
            previous_newer: None,
            selected: None,
            successor: None,
        }
    }

    fn observe(&mut self, entry: ReflogEntrySummary) {
        if self.newest.is_none() {
            self.newest = Some(entry.clone());
        }
        if self.selected.is_none() && entry.timestamp <= self.target {
            self.selected = Some(entry.clone());
            self.successor = self.previous_newer.clone();
        }
        self.oldest = Some(entry);
        self.previous_newer = self.oldest.clone();
    }
}

impl ReflogDateScan {
    fn new(target: i64) -> Self {
        Self {
            target,
            first: None,
            newest: None,
            latest_eligible: None,
            immediate_successor: None,
        }
    }

    fn observe(&mut self, entry: ReflogEntrySummary) {
        if self.first.is_none() {
            self.first = Some(entry.clone());
        }
        if entry.timestamp <= self.target {
            self.latest_eligible = Some(entry.clone());
            self.immediate_successor = None;
        } else if self.latest_eligible.is_some() && self.immediate_successor.is_none() {
            self.immediate_successor = Some(entry.clone());
        }
        self.newest = Some(entry);
    }
}

struct FileReflogCursor {
    reader: BufReader<File>,
    algorithm: GitHashAlgorithm,
    finished: bool,
}

impl FileReflogCursor {
    fn new(file: File, algorithm: GitHashAlgorithm) -> Self {
        Self {
            reader: BufReader::with_capacity(64 * 1024, file),
            algorithm,
            finished: false,
        }
    }

    fn next_entry(&mut self) -> io::Result<Option<ReflogEntrySummary>> {
        if self.finished {
            return Ok(None);
        }
        loop {
            let mut parser = ReflogHeaderParser::new(self.algorithm);
            let mut in_message = false;
            loop {
                let chunk = self.reader.fill_buf()?;
                if chunk.is_empty() {
                    self.finished = true;
                    return Ok(if parser.has_data() {
                        parser.finish()
                    } else {
                        None
                    });
                }
                let mut consumed = 0;
                let mut record_finished = false;
                for byte in chunk {
                    consumed += 1;
                    if in_message {
                        if *byte == b'\n' {
                            record_finished = true;
                            break;
                        }
                    } else if *byte == b'\t' {
                        in_message = true;
                    } else if *byte == b'\n' {
                        record_finished = true;
                        break;
                    } else {
                        parser.push_header_byte(*byte);
                    }
                }
                self.reader.consume(consumed);
                if record_finished {
                    if let Some(entry) = parser.finish() {
                        return Ok(Some(entry));
                    }
                    break;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct ReflogTailToken {
    bytes: [u8; 32],
    len: usize,
    oversized: bool,
}

impl ReflogTailToken {
    fn clear(&mut self) {
        self.len = 0;
        self.oversized = false;
    }

    fn push(&mut self, byte: u8) {
        if self.len == self.bytes.len() {
            self.oversized = true;
        } else {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }

    fn as_bytes(&self) -> Option<&[u8]> {
        (!self.oversized).then_some(&self.bytes[..self.len])
    }
}

struct ReflogHeaderParser {
    algorithm: GitHashAlgorithm,
    field_index: usize,
    current_id: [u8; 64],
    current_id_len: usize,
    current_id_oversized: bool,
    old_id: [u8; 64],
    old_id_len: usize,
    old_id_oversized: bool,
    new_id: [u8; 64],
    new_id_len: usize,
    new_id_oversized: bool,
    current_tail: ReflogTailToken,
    previous_tail: ReflogTailToken,
    last_tail: ReflogTailToken,
}

impl ReflogHeaderParser {
    fn new(algorithm: GitHashAlgorithm) -> Self {
        Self {
            algorithm,
            field_index: 0,
            current_id: [0; 64],
            current_id_len: 0,
            current_id_oversized: false,
            old_id: [0; 64],
            old_id_len: 0,
            old_id_oversized: false,
            new_id: [0; 64],
            new_id_len: 0,
            new_id_oversized: false,
            current_tail: ReflogTailToken::default(),
            previous_tail: ReflogTailToken::default(),
            last_tail: ReflogTailToken::default(),
        }
    }

    fn has_data(&self) -> bool {
        self.field_index != 0
            || self.current_id_len != 0
            || self.current_id_oversized
            || self.current_tail.len != 0
            || self.current_tail.oversized
    }

    fn push_header_byte(&mut self, byte: u8) {
        if byte.is_ascii_whitespace() {
            self.finish_token();
        } else if self.field_index < 2 {
            if self.current_id_len == self.current_id.len() {
                self.current_id_oversized = true;
            } else {
                self.current_id[self.current_id_len] = byte;
                self.current_id_len += 1;
            }
        } else {
            self.current_tail.push(byte);
        }
    }

    fn finish_token(&mut self) {
        let has_current = if self.field_index < 2 {
            self.current_id_len != 0 || self.current_id_oversized
        } else {
            self.current_tail.len != 0 || self.current_tail.oversized
        };
        if !has_current {
            return;
        }
        match self.field_index {
            0 => {
                self.old_id = self.current_id;
                self.old_id_len = self.current_id_len;
                self.old_id_oversized = self.current_id_oversized;
                self.current_id = [0; 64];
                self.current_id_len = 0;
                self.current_id_oversized = false;
            }
            1 => {
                self.new_id = self.current_id;
                self.new_id_len = self.current_id_len;
                self.new_id_oversized = self.current_id_oversized;
                self.current_id = [0; 64];
                self.current_id_len = 0;
                self.current_id_oversized = false;
            }
            _ => {
                self.previous_tail = self.last_tail;
                self.last_tail = self.current_tail;
                self.current_tail.clear();
            }
        }
        self.field_index += 1;
    }

    fn finish(mut self) -> Option<ReflogEntrySummary> {
        self.finish_token();
        if self.field_index < 4
            || self.old_id_oversized
            || self.new_id_oversized
            || self.old_id_len == 0
            || self.new_id_len == 0
        {
            return None;
        }
        let old_id = std::str::from_utf8(&self.old_id[..self.old_id_len])
            .ok()
            .and_then(|hex| ObjectId::from_hex(self.algorithm, hex).ok())?;
        let new_id = std::str::from_utf8(&self.new_id[..self.new_id_len])
            .ok()
            .and_then(|hex| ObjectId::from_hex(self.algorithm, hex).ok())?;
        let timestamp = std::str::from_utf8(self.previous_tail.as_bytes()?)
            .ok()?
            .parse()
            .ok()?;
        let timezone = std::str::from_utf8(self.last_tail.as_bytes()?)
            .ok()?
            .to_owned();
        parse_reflog_timezone_offset(&timezone)?;
        Some(ReflogEntrySummary {
            old_id,
            new_id,
            timestamp,
            timezone,
        })
    }
}

fn format_reflog_timestamp(entry: &ReflogEntrySummary) -> String {
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

fn format_reflog_timezone(offset: i16) -> String {
    let offset = i32::from(offset);
    let sign = if offset < 0 { '-' } else { '+' };
    let minutes = offset.unsigned_abs();
    format!("{sign}{:02}{:02}", minutes / 60, minutes % 60)
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
    if base.is_empty() {
        if let Some(ref_name) = fs::read_to_string(repo.git_dir.join("HEAD"))?
            .strip_prefix("ref: ")
            .map(str::trim)
            .filter(|name| name.starts_with("refs/heads/"))
        {
            return Ok(ref_name.to_owned());
        }
        return Ok("HEAD".to_owned());
    }
    if base == "HEAD" {
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
                        id = decode_tag(store.algorithm(), &object.content)?.target;
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
                id = decode_tag(store.algorithm(), &object.content)?.target;
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

fn normalize_repo_object_path(repo: &GitRepo, path: &str) -> io::Result<String> {
    if !path.starts_with("./") && !path.starts_with("../") {
        return normalize_git_path(path);
    }
    let cwd = std::env::current_dir()?;
    let relative_cwd = cwd.strip_prefix(&repo.root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "relative path syntax can't be used outside working tree",
        )
    })?;
    let mut components = relative_cwd
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "path is outside repository",
                    ));
                }
            }
            component => components.push(component.to_owned()),
        }
    }
    Ok(components.join("/"))
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

    #[test]
    fn auto_abbrev_len_stays_at_minimum_for_small_object_counts() {
        assert_eq!(auto_abbrev_len_from_object_count(0), 7);
        assert_eq!(auto_abbrev_len_from_object_count(3), 7);
        assert_eq!(auto_abbrev_len_from_object_count(10_000), 7);
    }

    #[test]
    fn auto_abbrev_len_grows_with_repo_object_count() {
        assert_eq!(auto_abbrev_len_from_object_count(16_383), 7);
        assert_eq!(auto_abbrev_len_from_object_count(16_384), 8);
        assert_eq!(auto_abbrev_len_from_object_count(27_688), 8);
        assert_eq!(auto_abbrev_len_from_object_count(1_000_000), 10);
    }

    #[test]
    fn parse_git_config_integer_matches_git_base_zero_and_int_range() {
        for (value, expected) in [
            ("0", 0),
            ("+0", 0),
            ("-0", 0),
            ("010", 8),
            ("+010", 8),
            ("+0x10", 16),
            ("-0x10", -16),
            ("1k", 1024),
            ("1M", 1024 * 1024),
            ("1G", 1024 * 1024 * 1024),
            ("2147483647", i32::MAX),
            ("-2147483648", i32::MIN),
        ] {
            assert_eq!(
                parse_git_config_integer(value),
                Ok(expected),
                "value={value:?}"
            );
        }
        for value in [
            "", "+", "-", "0x", "08", "09", "0b1", "00x10", "bogus", "1z",
        ] {
            assert_eq!(
                parse_git_config_integer(value),
                Err("invalid unit"),
                "value={value:?}"
            );
        }
        for value in [
            "2147483648",
            "-2147483649",
            "2G",
            "999999999999999999999999",
        ] {
            assert_eq!(
                parse_git_config_integer(value),
                Err("out of range"),
                "value={value:?}"
            );
        }
    }

    #[test]
    fn auto_abbrev_len_for_store_does_not_materialize_object_ids() {
        struct HintOnlyStore;

        impl zmin_git_core::GitObjectStore for HintOnlyStore {
            fn read_object(&self, _id: &ObjectId) -> io::Result<zmin_git_core::LooseObject> {
                Err(io::Error::new(io::ErrorKind::NotFound, "test store"))
            }

            fn object_id_capacity_hint(&self) -> io::Result<usize> {
                Ok(16_384)
            }

            fn for_each_object_id(
                &self,
                _for_each: &mut dyn FnMut(&ObjectId) -> io::Result<()>,
            ) -> io::Result<()> {
                panic!("auto abbreviation must not enumerate object IDs")
            }
        }

        assert_eq!(
            crate::runtime::auto_abbrev_len_for_store(&HintOnlyStore).unwrap(),
            8
        );
    }

    #[test]
    fn reflog_numeric_selector_switches_to_date_at_git_threshold() {
        assert!(matches!(
            classify_reflog_selector("1"),
            ReflogSelector::Index(1)
        ));
        assert!(matches!(
            classify_reflog_selector("99999999"),
            ReflogSelector::Index(99_999_999)
        ));
        assert!(matches!(
            classify_reflog_selector("100000000"),
            ReflogSelector::Date("100000000")
        ));
        assert!(matches!(
            classify_reflog_selector("1700000200"),
            ReflogSelector::Date("1700000200")
        ));
        assert!(matches!(
            classify_reflog_selector("999999999999999999999999"),
            ReflogSelector::Date(_)
        ));
        assert!(matches!(
            classify_reflog_selector("-1"),
            ReflogSelector::Date("-1")
        ));
    }

    #[test]
    fn file_reflog_cursor_discards_long_and_non_utf8_messages() {
        let directory = tempfile::TempDir::new().expect("temp directory");
        let log_file = directory.path().join("HEAD");
        let old_id = "0000000000000000000000000000000000000000";
        let first_id = "1111111111111111111111111111111111111111";
        let second_id = "2222222222222222222222222222222222222222";
        let mut bytes =
            format!("{old_id} {first_id} Bench <bench@example.test> 1700000000 +0000\t")
                .into_bytes();
        bytes.extend(std::iter::repeat_n(b'x', 128 * 1024));
        bytes.push(0xff);
        bytes.push(b'\n');
        bytes.extend_from_slice(
            format!("{first_id} {second_id} Bench <bench@example.test> 1700000100 +0000\tsecond\n")
                .as_bytes(),
        );
        fs::write(&log_file, bytes).expect("write reflog fixture");
        let file = File::open(log_file).expect("open reflog fixture");
        let mut cursor = FileReflogCursor::new(file, GitHashAlgorithm::Sha1);
        let first = cursor
            .next_entry()
            .expect("read first entry")
            .expect("first entry");
        let second = cursor
            .next_entry()
            .expect("read second entry")
            .expect("second entry");
        assert_eq!(first.timestamp, 1_700_000_000);
        assert_eq!(second.timestamp, 1_700_000_100);
        assert_eq!(
            second.new_id,
            ObjectId::from_hex(GitHashAlgorithm::Sha1, second_id).unwrap()
        );
        assert!(cursor.next_entry().expect("read EOF").is_none());
    }
}
