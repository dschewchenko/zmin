use std::collections::HashSet;
use std::fs;
use std::io;

use regex::bytes::Regex;
use zmin_git_core::{
    CommitObjectCache, GitHashAlgorithm, GitObjectKind, GitObjectStore, LooseObjectStore, ObjectId,
    RefStore, RefTarget, decode_tag, find_tree_entry, read_index,
};

use super::{
    CliError, GitRepo, Result, read_common_git_dir, resolve_commitish_io, signature_timestamp,
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
    let id = resolve_objectish(repo, rev).map_err(|_| {
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

pub(crate) fn resolve_objectish_with_mode(
    repo: &GitRepo,
    objectish: &str,
) -> io::Result<ResolvedObjectish> {
    let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
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
    resolve_plain_objectish(repo, &store, objectish).map(resolved_without_mode)
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
    let matcher = Regex::new(pattern)
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
        if matcher.is_match(&commit.message) {
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
    let index = read_index(&repo.index_path)?;
    index
        .entries()
        .iter()
        .find(|entry| entry.stage == stage && entry.path.as_slice() == path.as_bytes())
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
    match objectish.len() {
        40 => {
            if let Ok(id) = ObjectId::from_hex(GitHashAlgorithm::Sha1, objectish) {
                if store.contains_object(&id)? {
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

    if let Some((base, index)) = split_reflog_index_suffix(objectish) {
        return resolve_reflog_index(repo, store, base, index);
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

fn split_reflog_index_suffix(objectish: &str) -> Option<(&str, usize)> {
    let base = objectish.strip_suffix('}')?;
    let (base, index) = base.rsplit_once("@{")?;
    index.parse::<usize>().ok().map(|index| (base, index))
}

fn resolve_reflog_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    base: &str,
    index: usize,
) -> io::Result<ObjectId> {
    let reflog_name = reflog_ref_name(repo, base)?;
    let path = repo.git_dir.join("logs").join(&reflog_name);
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && index == 0
                && (base.is_empty() || base == "HEAD" || base.ends_with('@')) =>
        {
            let fallback = if base.is_empty() { "HEAD" } else { base };
            return resolve_plain_objectish(repo, store, fallback);
        }
        Err(error) => return Err(error),
    };
    contents
        .lines()
        .rev()
        .filter_map(reflog_line_new_id)
        .nth(index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "reflog entry not found"))
}

fn reflog_ref_name(repo: &GitRepo, base: &str) -> io::Result<String> {
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
    for suffix in ["^{commit}", "^{tree}", "^{tag}", "^{object}", "^{}"] {
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
        "^{}" | "^{commit}" => {
            for _ in 0..8 {
                let object = store.read_object(&id)?;
                match object.kind {
                    GitObjectKind::Tag => {
                        id = decode_tag(GitHashAlgorithm::Sha1, &object.content)?.target;
                    }
                    GitObjectKind::Commit if peel == "^{commit}" || peel == "^{}" => {
                        return Ok(id);
                    }
                    _ if peel == "^{}" => return Ok(id),
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("revision `{base}` is not a commit"),
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
            format!("unsupported peel operator {peel}"),
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
    } else if treeish.contains('~') || contains_parent_shorthand(treeish) {
        resolve_commitish_io(repo, store, treeish)?
    } else {
        resolve_plain_objectish(repo, store, treeish)?
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
    for candidate in candidates {
        match refs.resolve(&candidate) {
            Ok(id) => return Ok(Some(id)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn common_ref_store(repo: &GitRepo) -> io::Result<RefStore> {
    let common_dir = read_common_git_dir(&repo.git_dir).map_err(cli_error_to_io)?;
    Ok(RefStore::new(common_dir, GitHashAlgorithm::Sha1))
}

fn resolve_repo_ref(repo: &GitRepo, name: &str) -> io::Result<ObjectId> {
    if name != "HEAD" {
        return common_ref_store(repo)?.resolve(name);
    }
    let head_refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
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

fn default_abbrev_len_for_store(store: &impl GitObjectStore) -> Result<usize> {
    const MIN_ABBREV: usize = 7;

    let full_len = GitHashAlgorithm::Sha1.digest_len() * 2;
    let mut ids = Vec::with_capacity(default_abbrev_object_id_initial_capacity(
        store.object_id_capacity_hint()?,
    ));
    store.for_each_object_id(&mut |id| {
        ids.push(id.clone());
        Ok(())
    })?;
    ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

    let mut required = MIN_ABBREV;
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
