use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::runtime::PackOperationLock;
use zmin_git_core::{GitObjectHash, GitObjectSink};

pub(crate) fn quiltimport(
    dry_run: bool,
    author: Option<&str>,
    patches: Option<PathBuf>,
    series: Option<PathBuf>,
    keep_non_patch: bool,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if !dry_run && !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot quiltimport with a dirty worktree".into(),
        });
    }
    let patches_dir = resolve_quilt_patches_dir(patches);
    let series_path = resolve_quilt_series_path(&patches_dir, series);
    let patch_names = read_quilt_series(&series_path)?;
    if patch_names.is_empty() {
        return Ok(());
    }
    let fallback_author = author.map(parse_author_identity).transpose()?;
    for patch_name in patch_names {
        println!("{patch_name}");
        let patch_path = patches_dir.join(&patch_name);
        let patch_bytes = fs::read(&patch_path)?;
        if keep_non_patch && !quilt_patch_has_diff(&patch_bytes) {
            println!("Patch is empty.  Was it split wrong?");
            return Err(CliError::Exit(1));
        }
        let description = quilt_patch_description(&patch_bytes)?;
        let author = quilt_patch_author(description.as_str(), fallback_author)?;
        if dry_run {
            continue;
        }
        apply_quilt_patch(
            &repo,
            &store,
            &patch_name,
            description.as_str(),
            &patch_bytes,
            author,
        )?;
    }
    Ok(())
}

fn quilt_patch_has_diff(patch: &[u8]) -> bool {
    patch
        .split(|byte| *byte == b'\n')
        .any(|line| line.starts_with(b"diff --git "))
}

fn resolve_quilt_patches_dir(patches: Option<PathBuf>) -> PathBuf {
    patches
        .or_else(|| std::env::var_os("QUILT_PATCHES").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("patches"))
}

fn resolve_quilt_series_path(patches_dir: &std::path::Path, series: Option<PathBuf>) -> PathBuf {
    series
        .or_else(|| std::env::var_os("QUILT_SERIES").map(PathBuf::from))
        .unwrap_or_else(|| patches_dir.join("series"))
}

fn read_quilt_series(path: &std::path::Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)?;
    let mut patches = Vec::new();
    for line in raw.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.split_whitespace().next() {
            patches.push(name.to_owned());
        }
    }
    Ok(patches)
}

fn quilt_patch_description(patch: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(patch).map_err(|_| CliError::Fatal {
        code: 128,
        message: "quilt patch description is not valid UTF-8".into(),
    })?;
    let mut description = Vec::new();
    for line in text.lines() {
        if line.starts_with("diff --git ") {
            break;
        }
        if line == "---" {
            break;
        }
        description.push(line);
    }
    Ok(description.join("\n").trim_matches('\n').to_owned())
}

fn quilt_patch_author<'a>(
    description: &str,
    fallback: Option<(&'a str, &'a str)>,
) -> Result<Signature> {
    let discovered = description.lines().find_map(|line| {
        line.strip_prefix("From:")
            .or_else(|| line.strip_prefix("Author:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
    });
    let (name, email) = match discovered {
        Some(value) => {
            let (name, email) = parse_mail_author(value);
            (name, email)
        }
        None => match fallback {
            Some((name, email)) => (name.to_owned(), email.to_owned()),
            None => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "quilt patch is missing author; pass --author".into(),
                });
            }
        },
    };
    let env_author_date = std::env::var("GIT_AUTHOR_DATE").ok();
    let (timestamp, timezone) = signature_date(env_author_date.as_deref())?;
    Ok(Signature::new(name, email, timestamp, timezone)?)
}

fn apply_quilt_patch(
    repo: &GitRepo,
    store: &LooseObjectStore,
    patch_name: &str,
    description: &str,
    patch_bytes: &[u8],
    author: Signature,
) -> Result<()> {
    let mut index = read_repo_index(repo)?;
    let options = patch_commands::ApplyOptions {
        allow_empty: false,
        allow_binary_replacement: false,
        apply: false,
        binary: false,
        check: false,
        cached: false,
        stat: false,
        numstat: false,
        summary: false,
        build_fake_ancestor: None,
        index: true,
        recount: false,
        quiet: false,
        verbose: false,
        unsafe_paths: false,
        unidiff_zero: false,
        ignore_space_change: false,
        ignore_whitespace: false,
        inaccurate_eof: false,
        whitespace: None,
        strip: None,
        context: None,
        directory: None,
        include: Vec::new(),
        exclude: Vec::new(),
        intent_to_add: false,
        no_add: false,
        z: false,
        reject: false,
        three_way: false,
        ours: false,
        theirs: false,
        union: false,
        reverse: false,
        patches: Vec::new(),
    };
    for patch in patch_commands::parse_apply_patches(patch_bytes)? {
        let update = patch_commands::apply_file_patch(repo, store, &index, &patch, &options)?;
        patch_commands::write_apply_update(repo, store, &mut index, update, &options)?;
    }
    index.write_to_path(&repo.index_path)?;
    let tree = write_tree_from_index(store, &index)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let parent = refs.resolve("HEAD")?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = quilt_commit_message(patch_name, description);
    let commit = CommitBuilder::new(tree, author, committer)
        .parent(parent)
        .message(message.as_bytes().to_vec())?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)
}

fn quilt_commit_message(patch_name: &str, description: &str) -> String {
    let subject = std::path::Path::new(patch_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(patch_name);
    let body = description.trim();
    if body.is_empty() {
        format!("{subject}\n")
    } else {
        format!("{subject}\n\n{body}\n")
    }
}

pub(crate) struct FastExportOptions {
    pub(crate) all: bool,
    pub(crate) anonymize: bool,
    pub(crate) anonymize_map: Vec<String>,
    pub(crate) progress: Option<String>,
    pub(crate) signed_tags: Option<String>,
    pub(crate) tag_of_filtered_object: Option<String>,
    pub(crate) reencode: Option<String>,
    pub(crate) export_marks: Option<PathBuf>,
    pub(crate) import_marks: Option<PathBuf>,
    pub(crate) import_marks_if_exists: Option<PathBuf>,
    pub(crate) fake_missing_tagger: bool,
    pub(crate) full_tree: bool,
    pub(crate) use_done_feature: bool,
    pub(crate) no_data: bool,
    pub(crate) refspec: Option<String>,
    pub(crate) reference_excluded_parents: bool,
    pub(crate) show_original_ids: bool,
    pub(crate) mark_tags: bool,
    pub(crate) detect_copies: bool,
    pub(crate) detect_renames: bool,
    pub(crate) refs: Vec<String>,
}

pub(crate) struct FastImportOptions {
    pub(crate) date_format: Option<String>,
    pub(crate) quiet: bool,
    pub(crate) stats: bool,
    pub(crate) force: bool,
    pub(crate) done: bool,
    pub(crate) allow_unsafe_features: bool,
    pub(crate) active_branches: Option<String>,
    pub(crate) depth: Option<String>,
    pub(crate) big_file_threshold: Option<String>,
    pub(crate) cat_blob_fd: Option<String>,
    pub(crate) export_marks: Option<PathBuf>,
    pub(crate) export_pack_edges: Option<PathBuf>,
    pub(crate) import_marks: Vec<PathBuf>,
    pub(crate) import_marks_if_exists: Vec<PathBuf>,
    pub(crate) max_pack_size: Option<String>,
    pub(crate) max_pack_size_warning: Option<String>,
    pub(crate) no_relative_marks: bool,
    pub(crate) relative_marks_enabled: bool,
    pub(crate) relative_marks_invalid_value: Option<String>,
    pub(crate) rewrite_submodules_from: Option<String>,
    pub(crate) rewrite_submodules_to: Option<String>,
}

pub(crate) struct FastImportMarksResolution {
    pub(crate) export_marks: Option<PathBuf>,
    pub(crate) import_marks: Vec<PathBuf>,
    pub(crate) import_marks_if_exists: Vec<PathBuf>,
    pub(crate) relative_marks_enabled: bool,
    pub(crate) relative_marks_invalid_value: Option<String>,
}

pub(crate) fn fast_export(options: FastExportOptions) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    fast_export_preflight(&options)?;
    fast_export_modeled_noop_surface(&options);
    let refs = fast_export_refs(&repo, options.all, options.refs.clone())?;
    let mut state = FastExportState::default();
    preload_fast_export_marks(&mut state, options.import_marks.as_deref())?;
    preload_fast_export_marks_if_exists(&mut state, options.import_marks_if_exists.as_deref())?;
    let mut out = io::stdout().lock();
    if options.use_done_feature {
        writeln!(out, "feature done")?;
    }
    for (ref_name, tip) in refs {
        let mut commits = collect_commits_cached(
            &repo,
            &store,
            &commit_cache,
            std::slice::from_ref(&tip),
            None,
        )?;
        commits.reverse();
        let mut wrote_ref_commit = false;
        for id in &commits {
            if !state.commit_marks.contains_key(&id.to_hex()) {
                write_fast_export_commit(
                    &mut out,
                    &store,
                    &commit_cache,
                    &tree_cache,
                    &mut state,
                    &options,
                    &ref_name,
                    id,
                    !wrote_ref_commit,
                )?;
                wrote_ref_commit = true;
            }
        }
        if !wrote_ref_commit && let Some(mark) = state.commit_marks.get(&tip) {
            let ref_name = if options.anonymize {
                state.anonymizer.ref_name(&ref_name)
            } else {
                ref_name.clone()
            };
            writeln!(out, "reset {ref_name}")?;
            writeln!(out, "from :{mark}")?;
            writeln!(out)?;
        }
    }
    if let Some(path) = options.export_marks.as_deref() {
        write_fast_export_marks_file(path, &state)?;
    }
    if options.use_done_feature {
        writeln!(out, "done")?;
    }
    Ok(())
}

fn fast_export_modeled_noop_surface(options: &FastExportOptions) {
    let _ = (
        &options.signed_tags,
        &options.tag_of_filtered_object,
        &options.reencode,
        &options.anonymize_map,
        options.fake_missing_tagger,
        &options.refspec,
        options.reference_excluded_parents,
        options.mark_tags,
        options.detect_copies,
        options.detect_renames,
    );
}

fn fast_export_preflight(options: &FastExportOptions) -> Result<()> {
    if !options.anonymize_map.is_empty() && !options.anonymize {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--anonymize-map' requires '--anonymize'".into(),
        });
    }
    let _ = fast_export_progress_step(options.progress.as_deref())?;
    fast_export_signed_tags_mode(options.signed_tags.as_deref())?;
    fast_export_tag_of_filtered_mode(options.tag_of_filtered_object.as_deref())?;
    fast_export_reencode_mode(options.reencode.as_deref())?;
    Ok(())
}

fn fast_export_progress_step(value: Option<&str>) -> Result<Option<usize>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(fast_export_value_error(
            "option `progress' expects an integer value with an optional k/m/g suffix",
        ));
    }
    let (digits, multiplier) = match value.as_bytes().last().copied() {
        Some(b'k') | Some(b'K') => (&value[..value.len() - 1], 1024usize),
        Some(b'm') | Some(b'M') => (&value[..value.len() - 1], 1024usize * 1024),
        Some(b'g') | Some(b'G') => (&value[..value.len() - 1], 1024usize * 1024 * 1024),
        _ => (value, 1),
    };
    let Ok(base) = digits.parse::<usize>() else {
        return Err(fast_export_value_error(
            "option `progress' expects an integer value with an optional k/m/g suffix",
        ));
    };
    Ok(Some(base.saturating_mul(multiplier)))
}

fn fast_export_signed_tags_mode(value: Option<&str>) -> Result<()> {
    match value {
        None => Ok(()),
        Some("abort" | "verbatim" | "warn" | "warn-strip" | "strip") => Ok(()),
        Some(value) => Err(fast_export_value_error(format!(
            "Unknown signed-tags mode: {value}"
        ))),
    }
}

fn fast_export_tag_of_filtered_mode(value: Option<&str>) -> Result<()> {
    match value {
        None => Ok(()),
        Some("abort" | "drop" | "rewrite") => Ok(()),
        Some(value) => Err(fast_export_value_error(format!(
            "Unknown tag-of-filtered mode: {value}"
        ))),
    }
}

fn fast_export_reencode_mode(value: Option<&str>) -> Result<()> {
    match value {
        None => Ok(()),
        Some("yes" | "no" | "abort") => Ok(()),
        Some(value) => Err(fast_export_value_error(format!(
            "Unknown reencoding mode: {value}"
        ))),
    }
}

fn fast_export_value_error(message: impl Into<String>) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!("error: {}\n", message.into()),
    }
}

fn fast_export_refs(repo: &GitRepo, all: bool, refs: Vec<String>) -> Result<Vec<(String, String)>> {
    let ref_store = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let names = if all {
        let mut names = Vec::new();
        ref_store.for_each_ref_name("refs/heads/", |name| {
            names.push(name.to_owned());
            Ok::<(), CliError>(())
        })?;
        names
    } else if refs.is_empty() {
        vec![current_branch_ref(&ref_store)?.unwrap_or_else(|| "HEAD".to_owned())]
    } else {
        let mut resolved_refs = Vec::new();
        for name in refs {
            let resolved = if name == "HEAD" {
                current_branch_ref(&ref_store)?.unwrap_or(name)
            } else if name.starts_with("refs/") {
                name
            } else {
                format!("refs/heads/{name}")
            };
            if !resolved_refs.contains(&resolved) {
                resolved_refs.push(resolved);
            }
        }
        resolved_refs
    };
    names
        .into_iter()
        .map(|name| {
            let id = ref_store.resolve(&name)?;
            Ok((name, id.to_hex()))
        })
        .collect()
}

#[derive(Default)]
struct FastExportState {
    next_mark: usize,
    blob_marks: HashMap<String, usize>,
    commit_marks: HashMap<String, usize>,
    object_count: usize,
    anonymizer: FastExportAnonymizer,
}

impl FastExportState {
    fn alloc_mark(&mut self) -> usize {
        self.next_mark += 1;
        self.next_mark
    }
}

#[derive(Default)]
struct FastExportAnonymizer {
    next_ref: usize,
    next_path: usize,
    next_blob: usize,
    next_subject: usize,
    refs: HashMap<String, String>,
    path_segments: HashMap<Vec<u8>, Vec<u8>>,
    blob_contents: HashMap<String, Vec<u8>>,
}

impl FastExportAnonymizer {
    fn ref_name(&mut self, ref_name: &str) -> String {
        if let Some(mapped) = self.refs.get(ref_name) {
            return mapped.clone();
        }
        let mapped = if ref_name.starts_with("refs/heads/") {
            format!("refs/heads/ref{}", self.alloc_ref())
        } else if ref_name.starts_with("refs/tags/") {
            format!("refs/tags/ref{}", self.alloc_ref())
        } else {
            format!("ref{}", self.alloc_ref())
        };
        self.refs.insert(ref_name.to_owned(), mapped.clone());
        mapped
    }

    fn path(&mut self, path: &[u8]) -> Vec<u8> {
        let segments = path.split(|byte| *byte == b'/').map(|segment| {
            if let Some(mapped) = self.path_segments.get(segment) {
                return mapped.clone();
            }
            let mapped = format!("path{}", self.alloc_path()).into_bytes();
            self.path_segments.insert(segment.to_vec(), mapped.clone());
            mapped
        });
        let mut out = Vec::new();
        for segment in segments {
            if !out.is_empty() {
                out.push(b'/');
            }
            out.extend_from_slice(&segment);
        }
        out
    }

    fn blob_content(&mut self, id: &ObjectId) -> Vec<u8> {
        let hex = id.to_hex();
        if let Some(mapped) = self.blob_contents.get(&hex) {
            return mapped.clone();
        }
        let mapped = format!("anonymous blob {}", self.alloc_blob()).into_bytes();
        self.blob_contents.insert(hex, mapped.clone());
        mapped
    }

    fn message(&mut self) -> Vec<u8> {
        let index = self.next_subject;
        self.next_subject += 1;
        format!("subject {index}\n\nbody\n").into_bytes()
    }

    fn signature(&self, raw: &[u8]) -> Vec<u8> {
        let text = String::from_utf8_lossy(raw);
        let mut parts = text.rsplitn(3, ' ');
        let timezone = parts.next().unwrap_or("+0000");
        let timestamp = parts.next().unwrap_or("0");
        format!("User 0 <user0@example.com> {timestamp} {timezone}").into_bytes()
    }

    fn alloc_ref(&mut self) -> usize {
        let index = self.next_ref;
        self.next_ref += 1;
        index
    }

    fn alloc_path(&mut self) -> usize {
        let index = self.next_path;
        self.next_path += 1;
        index
    }

    fn alloc_blob(&mut self) -> usize {
        let index = self.next_blob;
        self.next_blob += 1;
        index
    }
}

fn write_fast_export_commit<W: Write>(
    out: &mut W,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    state: &mut FastExportState,
    options: &FastExportOptions,
    ref_name: &str,
    id: &ObjectId,
    reset_ref: bool,
) -> Result<()> {
    let commit = commit_cache.read_commit(id)?;
    let ref_name = if options.anonymize {
        state.anonymizer.ref_name(ref_name)
    } else {
        ref_name.to_owned()
    };
    let parent_tree = commit
        .parents
        .first()
        .map(|parent| commit_cache.read_commit(parent))
        .transpose()?
        .map(|parent| parent.tree.clone());
    let commands =
        collect_fast_export_commands(tree_cache, parent_tree.as_ref(), &commit.tree, options)?;
    write_fast_export_blob_records(out, store, state, options, &commands)?;

    let mark = state.alloc_mark();
    state.commit_marks.insert(id.to_hex(), mark);
    if reset_ref {
        writeln!(out, "reset {ref_name}")?;
    }
    writeln!(out, "commit {ref_name}")?;
    writeln!(out, "mark :{mark}")?;
    if options.show_original_ids {
        writeln!(out, "original-oid {}", id.to_hex())?;
    }
    let author = if options.anonymize {
        state.anonymizer.signature(&commit.author)
    } else {
        commit.author.clone()
    };
    writeln!(out, "author {}", String::from_utf8_lossy(&author))?;
    let committer = if options.anonymize {
        state.anonymizer.signature(&commit.committer)
    } else {
        commit.committer.clone()
    };
    writeln!(out, "committer {}", String::from_utf8_lossy(&committer))?;
    let message = if options.anonymize {
        state.anonymizer.message()
    } else {
        commit.message.clone()
    };
    writeln!(out, "data {}", message.len())?;
    out.write_all(&message)?;
    if !message.ends_with(b"\n") {
        writeln!(out)?;
    }
    if let Some(parent) = commit.parents.first()
        && let Some(parent_mark) = state.commit_marks.get(&parent.to_hex())
    {
        writeln!(out, "from :{parent_mark}")?;
    }
    for command in commands {
        write_fast_export_file_command(out, state, &command, options)?;
    }
    writeln!(out)?;
    note_fast_export_progress(
        out,
        state,
        fast_export_progress_step(options.progress.as_deref())?,
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
struct FastExportFile {
    path: Vec<u8>,
    mode: IndexMode,
    id: ObjectId,
}

#[derive(Debug, Clone)]
enum FastExportCommand {
    DeleteAll,
    Delete(Vec<u8>),
    Modify(FastExportFile),
}

fn collect_tree_blobs(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_id: &ObjectId,
) -> Result<Vec<FastExportFile>> {
    let mut files = Vec::new();
    collect_tree_blobs_at(tree_cache, tree_id, Vec::new(), &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_tree_blobs_at(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_id: &ObjectId,
    prefix: Vec<u8>,
    files: &mut Vec<FastExportFile>,
) -> Result<()> {
    for entry in tree_cache.read_tree(tree_id)?.iter() {
        let mut path = prefix.clone();
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(&entry.name);
        if entry.mode == TreeMode::Tree {
            collect_tree_blobs_at(tree_cache, &entry.id, path, files)?;
        } else {
            files.push(FastExportFile {
                path,
                mode: fast_export_index_mode(entry.mode),
                id: entry.id.clone(),
            });
        }
    }
    Ok(())
}

fn write_fast_export_file_command<W: Write>(
    out: &mut W,
    state: &mut FastExportState,
    command: &FastExportCommand,
    options: &FastExportOptions,
) -> Result<()> {
    match command {
        FastExportCommand::DeleteAll => {
            writeln!(out, "deleteall")?;
        }
        FastExportCommand::Delete(path) => {
            let path = if options.anonymize {
                state.anonymizer.path(path)
            } else {
                path.clone()
            };
            writeln!(out, "D {}", String::from_utf8_lossy(&path))?;
        }
        FastExportCommand::Modify(file) => {
            let path = if options.anonymize {
                state.anonymizer.path(&file.path)
            } else {
                file.path.clone()
            };
            let path = String::from_utf8_lossy(&path);
            match file.mode {
                IndexMode::File | IndexMode::Executable | IndexMode::Symlink => {
                    if options.no_data {
                        writeln!(
                            out,
                            "M {} {} {path}",
                            fast_export_mode(file.mode).unwrap_or("100644"),
                            file.id.to_hex()
                        )?;
                    } else {
                        let mark = state.blob_marks.get(&file.id.to_hex()).ok_or_else(|| {
                            CliError::Fatal {
                                code: 128,
                                message: "fast-export missing blob mark".into(),
                            }
                        })?;
                        writeln!(
                            out,
                            "M {} :{mark} {path}",
                            fast_export_mode(file.mode).unwrap_or("100644")
                        )?;
                    }
                }
                IndexMode::Gitlink => {
                    writeln!(out, "M 160000 {} {path}", file.id.to_hex())?;
                }
                IndexMode::Tree => {}
            }
        }
    }
    Ok(())
}

fn fast_export_mode(mode: IndexMode) -> Option<&'static str> {
    match mode {
        IndexMode::File => Some("100644"),
        IndexMode::Executable => Some("100755"),
        IndexMode::Symlink => Some("120000"),
        _ => None,
    }
}

fn fast_export_index_mode(mode: TreeMode) -> IndexMode {
    match mode {
        TreeMode::File => IndexMode::File,
        TreeMode::Executable => IndexMode::Executable,
        TreeMode::Symlink => IndexMode::Symlink,
        TreeMode::Gitlink => IndexMode::Gitlink,
        TreeMode::Tree => IndexMode::Tree,
    }
}

fn collect_fast_export_commands(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    old_tree: Option<&ObjectId>,
    new_tree: &ObjectId,
    options: &FastExportOptions,
) -> Result<Vec<FastExportCommand>> {
    if options.full_tree {
        let mut commands = vec![FastExportCommand::DeleteAll];
        commands.extend(
            collect_tree_blobs(tree_cache, new_tree)?
                .into_iter()
                .map(FastExportCommand::Modify),
        );
        return Ok(commands);
    }

    let mut commands = Vec::new();
    for entry in zmin_git_core::diff_trees(tree_cache, old_tree, new_tree)? {
        match entry.status {
            IndexDiffStatus::Deleted => commands.push(FastExportCommand::Delete(entry.path)),
            IndexDiffStatus::Added
            | IndexDiffStatus::Modified
            | IndexDiffStatus::Copied
            | IndexDiffStatus::Renamed => {
                let Some(new_entry) = entry.new_entry else {
                    continue;
                };
                commands.push(FastExportCommand::Modify(FastExportFile {
                    path: new_entry.path,
                    mode: new_entry.mode,
                    id: new_entry.id,
                }));
            }
        }
    }
    Ok(commands)
}

fn write_fast_export_blob_records<W: Write>(
    out: &mut W,
    store: &LooseObjectStore,
    state: &mut FastExportState,
    options: &FastExportOptions,
    commands: &[FastExportCommand],
) -> Result<()> {
    if options.no_data {
        return Ok(());
    }
    for command in commands {
        let FastExportCommand::Modify(file) = command else {
            continue;
        };
        if !matches!(
            file.mode,
            IndexMode::File | IndexMode::Executable | IndexMode::Symlink
        ) || state.blob_marks.contains_key(&file.id.to_hex())
        {
            continue;
        }
        let mark = state.alloc_mark();
        state.blob_marks.insert(file.id.to_hex(), mark);
        let content = if options.anonymize {
            state.anonymizer.blob_content(&file.id)
        } else {
            store.read_object(&file.id)?.content
        };
        writeln!(out, "blob")?;
        writeln!(out, "mark :{mark}")?;
        if options.show_original_ids {
            writeln!(out, "original-oid {}", file.id.to_hex())?;
        }
        writeln!(out, "data {}", content.len())?;
        out.write_all(&content)?;
        if !content.ends_with(b"\n") {
            writeln!(out)?;
        }
        if !options.anonymize {
            writeln!(out)?;
        }
        note_fast_export_progress(
            out,
            state,
            fast_export_progress_step(options.progress.as_deref())?,
        )?;
    }
    Ok(())
}

fn note_fast_export_progress<W: Write>(
    out: &mut W,
    state: &mut FastExportState,
    progress: Option<usize>,
) -> Result<()> {
    let Some(step) = progress else {
        return Ok(());
    };
    if step == 0 {
        return Ok(());
    }
    state.object_count += 1;
    if state.object_count % step == 0 {
        writeln!(out, "progress {} objects", state.object_count)?;
    }
    Ok(())
}

fn preload_fast_export_marks(state: &mut FastExportState, path: Option<&Path>) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let raw = fs::read_to_string(path)?;
    for line in raw.lines() {
        let Some((mark, oid)) = line.split_once(' ') else {
            continue;
        };
        let Some(mark) = mark.strip_prefix(':') else {
            continue;
        };
        let Ok(mark) = mark.parse::<usize>() else {
            continue;
        };
        state.next_mark = state.next_mark.max(mark);
        state.commit_marks.insert(oid.to_owned(), mark);
    }
    Ok(())
}

fn preload_fast_export_marks_if_exists(
    state: &mut FastExportState,
    path: Option<&Path>,
) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    preload_fast_export_marks(state, Some(path))
}

fn write_fast_export_marks_file(path: &Path, state: &FastExportState) -> Result<()> {
    let mut marks = state
        .commit_marks
        .iter()
        .map(|(oid, mark)| (*mark, oid.as_str()))
        .collect::<Vec<_>>();
    marks.sort_by(|left, right| right.0.cmp(&left.0));
    let mut out = String::new();
    for (mark, oid) in marks {
        out.push_str(&format!(":{mark} {oid}\n"));
    }
    fs::write(path, out)?;
    Ok(())
}

pub(crate) fn fast_import(options: FastImportOptions) -> Result<()> {
    let repo = find_repo()?;
    let algorithm = repo_hash_algorithm_from_config(&repo)?;
    fast_import_preflight(&repo.git_dir, &options)?;
    let unpack_limit = fast_import_unpack_limit(&repo)?;
    fast_import_modeled_noop_surface(&options);
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let edge_pack_dir = repo
        .objects_dir
        .strip_prefix(&repo.root)
        .map(|path| path.join("pack"))
        .unwrap_or_else(|_| repo.objects_dir.join("pack"));
    let pack_state = Rc::new(RefCell::new(FastImportPackState::new(
        repo.objects_dir.join("pack"),
        edge_pack_dir,
        algorithm,
        unpack_limit,
    )));
    let fast_store = FastImportObjectStore::new(&store, Rc::clone(&pack_state));
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let ref_repo = GitRepo {
        root: repo.root.clone(),
        git_dir: common_git_dir.clone(),
        objects_dir: repo.objects_dir.clone(),
        index_path: repo.index_path.clone(),
    };
    let refs = RefStore::new(&common_git_dir, algorithm);
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut transaction = FastImportTransaction::new(Rc::clone(&pack_state));
    let mut parser = FastImportParser::new(
        stdin.lock(),
        stdout.lock(),
        &ref_repo,
        &fast_store,
        &refs,
        FastImportDateFormat::from_cli(options.date_format.as_deref(), &repo.git_dir)?,
        options,
    );
    let result = parser.parse();
    match result {
        Ok(()) => match transaction.commit() {
            Ok(()) => Ok(()),
            Err(primary) => match transaction.abort(&fast_store, &[]) {
                Ok(()) => Err(CliError::Io(primary)),
                Err(cleanup) => Err(CliError::Io(io::Error::new(
                    primary.kind(),
                    format!("{primary}; fast-import rollback failed: {cleanup}"),
                ))),
            },
        },
        Err(primary) => {
            // Git keeps objects, refs already committed at checkpoint, marks, and
            // edge output on a fatal stream error.  Finish the current pack and
            // failure-time exports before releasing only run-owned keeps.
            let failure_edge_ids = parser.current_pack_edge_ids();
            let mut failure = primary;
            if let Err(error) = parser.finalize_pack() {
                failure = fast_import_cleanup_error(
                    failure,
                    io::Error::other(fast_import_error_text(&error)),
                );
            }
            if let Err(error) = parser.export_marks() {
                failure = fast_import_cleanup_error(
                    failure,
                    io::Error::other(fast_import_error_text(&error)),
                );
            }
            if let Err(error) = parser.export_pack_edges() {
                failure = fast_import_cleanup_error(
                    failure,
                    io::Error::other(fast_import_error_text(&error)),
                );
            }
            match transaction.abort(&fast_store, &failure_edge_ids) {
                Ok(()) => Err(failure),
                Err(cleanup) => Err(fast_import_cleanup_error(failure, cleanup)),
            }
        }
    }
}

struct FastImportTransaction {
    pack_state: Rc<RefCell<FastImportPackState>>,
    completed: bool,
}

impl FastImportTransaction {
    fn new(pack_state: Rc<RefCell<FastImportPackState>>) -> Self {
        Self {
            pack_state,
            completed: false,
        }
    }

    fn commit(&mut self) -> io::Result<()> {
        self.pack_state.borrow_mut().commit()?;
        self.completed = true;
        Ok(())
    }

    fn abort(
        &mut self,
        store: &FastImportObjectStore<'_>,
        edge_ids: &[ObjectId],
    ) -> io::Result<()> {
        let result = self.pack_state.borrow_mut().abort(store, edge_ids);
        self.completed = true;
        result
    }
}

impl Drop for FastImportTransaction {
    fn drop(&mut self) {
        if !self.completed {
            if let Err(error) = self.pack_state.borrow_mut().release_keeps() {
                eprintln!("fast-import cleanup failed during unwind: {error}");
            }
        }
    }
}

fn fast_import_cleanup_error(primary: CliError, cleanup: io::Error) -> CliError {
    let cleanup = format!("fast-import cleanup failed: {cleanup}");
    match primary {
        CliError::Fatal { code, message } => CliError::Fatal {
            code,
            message: format!("{message}; {cleanup}"),
        },
        CliError::Stderr { code, text } => CliError::Stderr {
            code,
            text: format!("{text}; {cleanup}"),
        },
        CliError::Message(message) => CliError::Message(format!("{message}; {cleanup}")),
        CliError::Io(error) => {
            CliError::Io(io::Error::new(error.kind(), format!("{error}; {cleanup}")))
        }
        CliError::Exit(code) => CliError::Fatal {
            code,
            message: cleanup,
        },
    }
}

fn fast_import_error_text(error: &CliError) -> String {
    match error {
        CliError::Exit(code) => format!("exit status {code}"),
        CliError::Fatal { code, message } => format!("fatal ({code}): {message}"),
        CliError::Stderr { code, text } => format!("stderr ({code}): {text}"),
        CliError::Message(message) => message.clone(),
        CliError::Io(error) => error.to_string(),
    }
}

fn fast_import_modeled_noop_surface(options: &FastImportOptions) {
    // Pack emission and the remaining import modes are still modeled as no-ops.
    let _ = (
        options.force,
        options.allow_unsafe_features,
        options.active_branches.as_deref(),
        options.depth.as_deref(),
        options.big_file_threshold.as_deref(),
        options.cat_blob_fd.as_deref(),
        options.max_pack_size.as_deref(),
        options.no_relative_marks,
    );
}

pub(crate) fn resolve_fast_import_stats_mode(
    raw_args: &[String],
    quiet: bool,
    stats: bool,
) -> (bool, bool) {
    let mut last_mode = None;
    for arg in raw_args.iter().skip(1) {
        match arg.as_str() {
            "--quiet" => last_mode = Some("quiet"),
            "--stats" => last_mode = Some("stats"),
            _ => {}
        }
    }
    match last_mode {
        Some("quiet") => (true, false),
        Some("stats") => (false, true),
        _ => (quiet, stats),
    }
}

pub(crate) fn resolve_fast_import_last_value<T: Clone>(values: &[T]) -> Option<T> {
    values.last().cloned()
}

pub(crate) fn resolve_fast_import_max_pack_size_warning(raw_args: &[String]) -> Option<String> {
    let mut warning_value = None;
    for arg in raw_args.iter().skip(1) {
        if let Some(value) = arg.strip_prefix("--max-pack-size=")
            && value.as_bytes().iter().all(|byte| byte.is_ascii_digit())
        {
            warning_value = Some(value.to_owned());
        }
    }
    warning_value
}

pub(crate) fn resolve_fast_import_marks_resolution(
    raw_args: &[String],
) -> FastImportMarksResolution {
    let mut relative_marks_enabled = false;
    let mut relative_marks_invalid_value = None;
    let mut export_marks = None;
    let mut import_marks = Vec::new();
    let mut import_marks_if_exists = Vec::new();
    for arg in raw_args.iter().skip(1) {
        if arg == "--relative-marks" {
            relative_marks_enabled = true;
            continue;
        }
        if arg == "--no-relative-marks" {
            relative_marks_enabled = false;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--relative-marks=") {
            relative_marks_invalid_value = Some(value.to_owned());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--export-marks=") {
            export_marks = Some(resolve_fast_import_marks_path(
                value,
                relative_marks_enabled,
            ));
            continue;
        }
        if let Some(value) = arg.strip_prefix("--import-marks=") {
            import_marks.push(resolve_fast_import_marks_path(
                value,
                relative_marks_enabled,
            ));
            continue;
        }
        if let Some(value) = arg.strip_prefix("--import-marks-if-exists=") {
            import_marks_if_exists.push(resolve_fast_import_marks_path(
                value,
                relative_marks_enabled,
            ));
        }
    }
    FastImportMarksResolution {
        export_marks,
        import_marks,
        import_marks_if_exists,
        relative_marks_enabled,
        relative_marks_invalid_value,
    }
}

fn resolve_fast_import_marks_path(path: &str, relative_marks_enabled: bool) -> PathBuf {
    if relative_marks_enabled {
        Path::new(".git")
            .join("info")
            .join("fast-import")
            .join(path)
    } else {
        PathBuf::from(path)
    }
}

fn fast_import_preflight(git_dir: &Path, options: &FastImportOptions) -> Result<()> {
    if options.max_pack_size.is_some() {
        return Err(fast_import_crash_error(
            git_dir,
            "--max-pack-size is unsupported pending B2b".to_owned(),
            None,
        )?);
    }
    if let Some(value) = options.cat_blob_fd.as_deref() {
        if let Err(error) = parse_fast_import_fd(value) {
            return Err(fast_import_crash_error(git_dir, error.to_string(), None)?);
        }
    }
    if let Some(value) = options.relative_marks_invalid_value.as_deref() {
        return Err(fast_import_crash_error(
            git_dir,
            format!("unknown option --relative-marks={value}"),
            None,
        )?);
    }
    if let Some(value) = options.rewrite_submodules_from.as_deref() {
        return Err(fast_import_submodule_rewrite_error(git_dir, value)?);
    }
    if let Some(value) = options.rewrite_submodules_to.as_deref() {
        return Err(fast_import_submodule_rewrite_error(git_dir, value)?);
    }
    Ok(())
}

fn fast_import_unpack_limit(repo: &GitRepo) -> Result<Option<usize>> {
    let (key, value) = if let Some(value) = read_config_value(repo, "fastimport.unpackLimit")? {
        ("fastimport.unpackLimit", value)
    } else if let Some(value) = read_config_value(repo, "transfer.unpackLimit")? {
        ("transfer.unpackLimit", value)
    } else {
        return Ok(Some(100));
    };
    let parsed = parse_fast_import_config_int(value.trim()).map_err(|reason| CliError::Fatal {
        code: 128,
        message: format!(
            "bad numeric config value '{}' for '{}': {reason}",
            value.trim(),
            key
        ),
    })?;
    if parsed > i32::MAX as i64 || parsed < i32::MIN as i64 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "bad numeric config value '{}' for '{}': out of range",
                value.trim(),
                key
            ),
        });
    }
    if parsed < 0 {
        return Ok(Some(usize::MAX));
    }
    usize::try_from(parsed)
        .map(Some)
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: format!(
                "bad numeric config value '{}' for '{}': out of range",
                value.trim(),
                key
            ),
        })
}

fn parse_fast_import_config_int(value: &str) -> std::result::Result<i64, &'static str> {
    if value.is_empty() {
        return Err("invalid integer");
    }
    let (number, multiplier) = match value.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&value[..value.len() - 1], 1024_i64),
        Some(b'm' | b'M') => (&value[..value.len() - 1], 1024_i64 * 1024),
        Some(b'g' | b'G') => (&value[..value.len() - 1], 1024_i64 * 1024 * 1024),
        Some(byte) if byte.is_ascii_digit() => (value, 1),
        _ => return Err("invalid unit"),
    };
    if number.is_empty() {
        return Err("invalid unit");
    }
    let (sign, digits) = match number.as_bytes().first().copied() {
        Some(b'+') => (1_i64, &number[1..]),
        Some(b'-') => (-1_i64, &number[1..]),
        _ => (1_i64, number),
    };
    if digits.is_empty() {
        return Err("invalid unit");
    }
    let (digits, radix) = if digits.starts_with("0x") || digits.starts_with("0X") {
        (&digits[2..], 16)
    } else if digits.starts_with('0') && digits.len() > 1 {
        (digits, 8)
    } else {
        (digits, 10)
    };
    if digits.is_empty() {
        return Err("invalid integer");
    }
    let magnitude = i64::from_str_radix(digits, radix).map_err(|_| "invalid integer")?;
    sign.checked_mul(magnitude)
        .and_then(|value| value.checked_mul(multiplier))
        .ok_or("out of range")
}

fn fast_import_submodule_rewrite_error(git_dir: &Path, value: &str) -> Result<CliError> {
    let missing = value.split_once(':').map(|(_, path)| path).unwrap_or(value);
    fast_import_crash_error(
        git_dir,
        format!("cannot read '{missing}': No such file or directory"),
        None,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FastImportDateFormat {
    Raw,
    Rfc2822,
    Now,
}

impl FastImportDateFormat {
    fn from_cli(value: Option<&str>, git_dir: &std::path::Path) -> Result<Self> {
        match value {
            None | Some("raw") => Ok(Self::Raw),
            Some("raw-permissive") => Ok(Self::Raw),
            Some("rfc2822") => Ok(Self::Rfc2822),
            Some("now") => Ok(Self::Now),
            Some(value) => Err(fast_import_crash_error(
                git_dir,
                format!("unknown --date-format argument {value}"),
                None,
            )?),
        }
    }
}

struct FastImportParser<'a, R: BufRead, W: Write> {
    input: FastImportInput<R>,
    output: FastImportOutput<W>,
    date_format: FastImportDateFormat,
    repo: &'a GitRepo,
    store: &'a FastImportObjectStore<'a>,
    commit_cache: CommitObjectCache<'a, FastImportObjectStore<'a>>,
    tree_cache: TreeObjectCache<'a, FastImportObjectStore<'a>>,
    refs: &'a RefStore,
    marks: HashMap<usize, ObjectId>,
    ref_indexes: HashMap<String, GitIndex>,
    ref_empty_tree_states: HashMap<String, FastImportEmptyTreeState>,
    empty_tree_id: Option<ObjectId>,
    ref_tips: HashMap<String, ObjectId>,
    reset_to_empty_refs: FastImportResetToEmptyRefs,
    branch_state: FastImportBranchLru,
    commit_clock: u64,
    pending_ref_updates: Vec<FastImportRefUpdate>,
    pack_state: Rc<RefCell<FastImportPackState>>,
    pending_line: Option<Vec<u8>>,
    stats: FastImportStats,
    command_history: FastImportCommandHistory,
    options: FastImportOptions,
    stream_import_marks_seen: bool,
    relative_marks_enabled: bool,
    exported_pack_count: usize,
}

#[derive(Clone)]
struct FastImportRefUpdate {
    name: String,
    id: ObjectId,
}

#[derive(Default)]
struct FastImportResetToEmptyRefs {
    refs: std::collections::HashSet<String>,
}

impl FastImportResetToEmptyRefs {
    fn insert(&mut self, ref_name: &str) {
        self.refs.insert(ref_name.to_owned());
    }

    fn contains(&self, ref_name: &str) -> bool {
        self.refs.contains(ref_name)
    }

    fn remove(&mut self, ref_name: &str) {
        self.refs.remove(ref_name);
    }
}

const FAST_IMPORT_MAX_ACTIVE_BRANCHES: usize = 5;
const FAST_IMPORT_BRANCH_TABLE_SIZE: u32 = 1039;
const FAST_IMPORT_MAX_PACK_ID: u32 = (1 << 16) - 1;

#[derive(Clone)]
struct FastImportBranchRecord {
    name: String,
    active: bool,
    loaded: bool,
    clock: u64,
    tip_commit: ObjectId,
    old_tree: ObjectId,
    current_tree: ObjectId,
    last_pack: Option<u32>,
}

struct FastImportUnreferencedCommit {
    id: ObjectId,
    tree: ObjectId,
    newly_written: bool,
}

#[derive(Clone)]
struct FastImportBranchLru {
    max_active_branches: usize,
    active_order: Vec<String>,
    branches: std::collections::BTreeMap<String, FastImportBranchRecord>,
    table_buckets: std::collections::BTreeMap<u32, Vec<String>>,
}

impl Default for FastImportBranchLru {
    fn default() -> Self {
        Self {
            max_active_branches: FAST_IMPORT_MAX_ACTIVE_BRANCHES,
            active_order: Vec::new(),
            branches: std::collections::BTreeMap::new(),
            table_buckets: std::collections::BTreeMap::new(),
        }
    }
}

impl FastImportBranchLru {
    fn branch_bucket(name: &str) -> u32 {
        name.bytes().fold(0_u32, |hash, byte| {
            hash.wrapping_mul(31).wrapping_add(byte as u32)
        }) % FAST_IMPORT_BRANCH_TABLE_SIZE
    }

    fn register(&mut self, name: &str, null_id: &ObjectId) {
        if self.branches.contains_key(name) {
            return;
        }
        self.branches.insert(
            name.to_owned(),
            FastImportBranchRecord {
                name: name.to_owned(),
                active: false,
                loaded: false,
                clock: 0,
                tip_commit: null_id.clone(),
                old_tree: null_id.clone(),
                current_tree: null_id.clone(),
                last_pack: None,
            },
        );
        self.table_buckets
            .entry(Self::branch_bucket(name))
            .or_default()
            .insert(0, name.to_owned());
    }

    fn reset(&mut self, name: &str, null_id: &ObjectId) {
        self.register(name, null_id);
        if let Some(branch) = self.branches.get_mut(name) {
            branch.loaded = false;
            branch.tip_commit = null_id.clone();
            branch.old_tree = null_id.clone();
            branch.current_tree = null_id.clone();
        }
    }

    fn reset_from(&mut self, name: &str, tip_commit: ObjectId, tree: ObjectId) {
        self.register(name, &fast_import_zero_object_id(tip_commit.algorithm()));
        if let Some(branch) = self.branches.get_mut(name) {
            branch.loaded = false;
            branch.tip_commit = tip_commit;
            branch.old_tree = tree.clone();
            branch.current_tree = tree;
        }
    }

    fn activate(
        &mut self,
        name: &str,
        tip_commit: ObjectId,
        old_tree: ObjectId,
        current_tree: ObjectId,
    ) {
        let null_id = fast_import_zero_object_id(tip_commit.algorithm());
        self.register(name, &null_id);
        let is_active = self.branches.get(name).is_some_and(|branch| branch.active);
        if !is_active {
            while !self.active_order.is_empty()
                && self.active_order.len() >= self.max_active_branches
            {
                let evicted = self.active_order.pop().expect("active branch exists");
                if let Some(branch) = self.branches.get_mut(&evicted) {
                    branch.active = false;
                    branch.loaded = false;
                }
            }
            if let Some(branch) = self.branches.get_mut(name) {
                branch.active = true;
                branch.loaded = true;
                branch.tip_commit = tip_commit;
                branch.old_tree = old_tree;
                branch.current_tree = current_tree;
            } else {
                unreachable!("registered fast-import branch must exist");
            }
            self.active_order.retain(|active| active != name);
            self.active_order.insert(0, name.to_owned());
        } else if let Some(branch) = self.branches.get_mut(name) {
            branch.tip_commit = tip_commit;
            branch.old_tree = old_tree;
            branch.current_tree = current_tree;
        }
    }

    fn mark_dirty(&mut self, name: &str) {
        let Some(branch) = self.branches.get_mut(name) else {
            return;
        };
        branch.current_tree = fast_import_zero_object_id(branch.tip_commit.algorithm());
    }

    fn record_commit(
        &mut self,
        name: &str,
        tip_commit: ObjectId,
        old_tree: ObjectId,
        current_tree: ObjectId,
        clock: u64,
        last_pack: Option<u32>,
        update_last_pack: bool,
    ) {
        let Some(branch) = self.branches.get_mut(name) else {
            return;
        };
        branch.tip_commit = tip_commit;
        branch.old_tree = old_tree;
        branch.current_tree = current_tree;
        branch.clock = clock;
        if update_last_pack {
            branch.last_pack = last_pack;
        }
    }

    fn invalidate_pack(&mut self, pack_id: u32) {
        for branch in self.branches.values_mut() {
            if branch.last_pack == Some(pack_id) {
                branch.last_pack = None;
            }
        }
    }

    fn render_report(&self, report: &mut Vec<u8>) {
        report.extend_from_slice(
            format!(
                "\nActive Branch LRU\n-----------------\n    active_branches = {} cur, {} max\n\n  pos  clock name\n  ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\n",
                self.active_order.len(), self.max_active_branches
            )
            .as_bytes(),
        );
        for (position, name) in self.active_order.iter().enumerate() {
            if let Some(branch) = self.branches.get(name) {
                report.extend_from_slice(
                    format!("  {:2}) {:6} {}\n", position + 1, branch.clock, branch.name)
                        .as_bytes(),
                );
            }
        }
        report.extend_from_slice(b"\nInactive Branches\n-----------------\n");
        for names in self.table_buckets.values() {
            for name in names {
                let Some(branch) = self.branches.get(name) else {
                    continue;
                };
                report.extend_from_slice(format!("{}:\n", branch.name).as_bytes());
                report.extend_from_slice(b"  status      :");
                if branch.active {
                    report.extend_from_slice(b" active");
                }
                if branch.loaded {
                    report.extend_from_slice(b" loaded");
                }
                let null_tree = fast_import_zero_object_id(branch.current_tree.algorithm());
                if branch.current_tree == null_tree {
                    report.extend_from_slice(b" dirty");
                }
                report.extend_from_slice(b"\n");
                report.extend_from_slice(
                    format!("  tip commit  : {}\n", branch.tip_commit.to_hex()).as_bytes(),
                );
                report.extend_from_slice(
                    format!("  old tree    : {}\n", branch.old_tree.to_hex()).as_bytes(),
                );
                report.extend_from_slice(
                    format!("  cur tree    : {}\n", branch.current_tree.to_hex()).as_bytes(),
                );
                report.extend_from_slice(format!("  commit clock: {}\n", branch.clock).as_bytes());
                report.extend_from_slice(b"  last pack   : ");
                if let Some(last_pack) = branch.last_pack {
                    report.extend_from_slice(last_pack.to_string().as_bytes());
                }
                report.extend_from_slice(b"\n\n");
            }
        }
        report.push(b'\n');
        report.extend_from_slice(b"Marks\n-----\n");
    }
}

const FAST_IMPORT_COMMAND_HISTORY_LIMIT: usize = 100;
const FAST_IMPORT_COMMAND_HISTORY_BYTES: usize = 64 * 1024;
const FAST_IMPORT_CONTROL_LINE_BYTES: usize = 64 * 1024;
const FAST_IMPORT_CONTROL_LINE_LIMIT_MESSAGE: &str =
    "fast-import control line exceeds configured limit";

#[derive(Default)]
struct FastImportCommandHistory {
    commands: Vec<Vec<u8>>,
    bytes: usize,
}

impl FastImportCommandHistory {
    fn record(&mut self, command: &[u8]) {
        let command = command
            .split(|byte| *byte == b'\0')
            .next()
            .unwrap_or(command);
        let command = if command.len() > FAST_IMPORT_COMMAND_HISTORY_BYTES {
            format!("[command truncated: {} bytes]", command.len()).into_bytes()
        } else {
            command.to_vec()
        };
        while !self.commands.is_empty()
            && (self.commands.len() >= FAST_IMPORT_COMMAND_HISTORY_LIMIT
                || self.bytes + command.len() > FAST_IMPORT_COMMAND_HISTORY_BYTES)
        {
            self.bytes -= self.commands.remove(0).len();
        }
        self.bytes += command.len();
        self.commands.push(command);
    }

    fn snapshot(&self) -> Vec<Vec<u8>> {
        self.commands.clone()
    }
}

#[derive(Default)]
struct FastImportCrashReportState {
    recent_commands: Vec<Vec<u8>>,
    marks: Vec<(usize, String)>,
    branch_state: FastImportBranchLru,
}

enum FastImportResponseTarget {
    Stdout,
    Pending(String),
    FileDescriptor(i32),
}

struct FastImportOutput<W: Write> {
    stdout: io::BufWriter<W>,
    target: FastImportResponseTarget,
}

impl<W: Write> FastImportOutput<W> {
    fn new(stdout: W, cat_blob_fd: Option<&str>) -> Self {
        Self {
            stdout: io::BufWriter::new(stdout),
            target: cat_blob_fd.map_or(FastImportResponseTarget::Stdout, |value| {
                FastImportResponseTarget::Pending(value.to_owned())
            }),
        }
    }

    fn activate_target(&mut self) -> io::Result<()> {
        let FastImportResponseTarget::Pending(value) = &self.target else {
            return Ok(());
        };
        let value = value.clone();
        let fd = parse_fast_import_fd(&value)?;
        self.target = if fd == 1 {
            FastImportResponseTarget::Stdout
        } else {
            FastImportResponseTarget::FileDescriptor(fd)
        };
        Ok(())
    }

    fn write_response(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stdout.flush()?;
        self.activate_target()?;
        match &self.target {
            FastImportResponseTarget::Stdout | FastImportResponseTarget::Pending(_) => {
                self.stdout.write_all(bytes)
            }
            FastImportResponseTarget::FileDescriptor(fd) => fast_import_write_fd(*fd, bytes),
        }
    }

    fn flush_response(&mut self) -> io::Result<()> {
        match &self.target {
            FastImportResponseTarget::Stdout | FastImportResponseTarget::Pending(_) => {
                self.stdout.flush()
            }
            FastImportResponseTarget::FileDescriptor(_) => Ok(()),
        }
    }

    fn write_progress(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stdout.write_all(bytes)
    }

    fn flush_stdout(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}

fn parse_fast_import_fd(value: &str) -> io::Result<i32> {
    let value = value.strip_prefix('+').unwrap_or(value);
    if value.is_empty() || value.starts_with('-') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--cat-blob-fd: argument must be a non-negative integer",
        ));
    }
    let (digits, radix) = if let Some(value) = value.strip_prefix("0x") {
        (value, 16)
    } else if let Some(value) = value.strip_prefix("0X") {
        (value, 16)
    } else if value.starts_with('0') && value.len() > 1 {
        (value, 8)
    } else {
        (value, 10)
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--cat-blob-fd: argument must be a non-negative integer",
        ));
    }
    let fd = i64::from_str_radix(digits, radix).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "--cat-blob-fd: argument must be a non-negative integer",
        )
    })?;
    i32::try_from(fd).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "--cat-blob-fd cannot exceed 2147483647",
        )
    })
}

#[cfg(unix)]
fn fast_import_write_fd(fd: i32, bytes: &[u8]) -> io::Result<()> {
    let mut offset: usize = 0;
    while offset < bytes.len() {
        // SAFETY: the borrowed descriptor and byte slice are valid for this call.
        let written = unsafe {
            libc::write(
                fd,
                bytes[offset..].as_ptr().cast::<libc::c_void>(),
                bytes.len() - offset,
            )
        };
        if written < 0 {
            return Err(io::Error::last_os_error());
        }
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "cat-blob response write made no progress",
            ));
        }
        offset += written as usize;
    }
    Ok(())
}

#[cfg(windows)]
fn fast_import_write_fd(fd: i32, bytes: &[u8]) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::WriteFile;

    let raw_handle = unsafe { libc::get_osfhandle(fd) };
    if raw_handle == -1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--cat-blob-fd does not identify an open Windows descriptor",
        ));
    }
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = bytes.len() - offset;
        let count = u32::try_from(remaining).unwrap_or(u32::MAX);
        let mut written = 0_u32;
        // SAFETY: `_get_osfhandle` returned the live OS handle borrowed by the CRT
        // descriptor. The byte slice and output count remain valid for the call.
        let result = unsafe {
            WriteFile(
                raw_handle as _,
                bytes[offset..].as_ptr(),
                count,
                &mut written,
                std::ptr::null_mut(),
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "cat-blob response write made no progress",
            ));
        }
        offset += written as usize;
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn fast_import_write_fd(_fd: i32, _bytes: &[u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "--cat-blob-fd is unsupported on this platform",
    ))
}

struct FastImportPublishedPack {
    directory: FastImportDirectory,
    pack_path: PathBuf,
    pack_owned: Option<FastImportOwnedPath>,
    idx_owned: Option<FastImportOwnedPath>,
    keep_path: Option<FastImportOwnedPath>,
    edge_ids: Vec<ObjectId>,
}

struct FastImportInstalledPack {
    directory: FastImportDirectory,
    pack_path: PathBuf,
    pack_owned: Option<FastImportOwnedPath>,
    idx_owned: Option<FastImportOwnedPath>,
    keep_owned: Option<FastImportOwnedPath>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FastImportPathIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows { volume_serial: u32, file_index: u64 },
    #[cfg(not(any(unix, windows)))]
    Unsupported,
}

struct FastImportOwnedPath {
    path: PathBuf,
    identity: FastImportPathIdentity,
    parent_identity: FastImportPathIdentity,
    file: Option<fs::File>,
}

struct FastImportPackTemp {
    file: fs::File,
    owned: FastImportOwnedPath,
    stem: std::ffi::OsString,
}

struct FastImportDirectory {
    path: PathBuf,
    file: fs::File,
    identity: FastImportPathIdentity,
}

fn fast_import_directory_path(path: &Path) -> io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        for (alias, target) in [("/var", "private/var"), ("/tmp", "private/tmp")] {
            if let Some(rest) = path.strip_prefix(alias).ok()
                && fs::read_link(alias).ok().as_deref() == Some(Path::new(target))
            {
                return Ok(Path::new("/").join(target).join(rest));
            }
        }
    }
    Ok(path.to_owned())
}

impl FastImportDirectory {
    fn open(path: &Path) -> io::Result<Self> {
        let path = fast_import_directory_path(path)?;
        #[cfg(unix)]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::io::FromRawFd;

            let mut current = if path.is_absolute() {
                fs::OpenOptions::new().read(true).open("/")?
            } else {
                fs::OpenOptions::new().read(true).open(".")?
            };
            for component in path.components() {
                let name = match component {
                    std::path::Component::RootDir | std::path::Component::CurDir => continue,
                    std::path::Component::Normal(name) => name,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "fast-import directory path contains an unsafe component",
                        ));
                    }
                };
                let name = CString::new(name.as_bytes()).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "fast-import directory path contains NUL",
                    )
                })?;
                let fd = unsafe {
                    libc::openat(
                        std::os::unix::io::AsRawFd::as_raw_fd(&current),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    )
                };
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                let next = unsafe { fs::File::from_raw_fd(fd) };
                current = next;
            }
            let identity = fast_import_file_identity(&current)?;
            return Ok(Self {
                path: path.to_owned(),
                file: current,
                identity,
            });
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::os::windows::io::FromRawHandle;
            use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
            use windows_sys::Win32::Storage::FileSystem::{
                CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ,
                FILE_SHARE_WRITE, OPEN_EXISTING,
            };

            fast_import_reject_windows_reparse_components(&path, false)?;
            let wide = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let file = unsafe { fs::File::from_raw_handle(handle) };
            let identity = fast_import_file_identity(&file)?;
            return Ok(Self {
                path: path.to_owned(),
                file,
                identity,
            });
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "fast-import directory handles are unsupported on this platform",
            ))
        }
    }

    #[cfg(unix)]
    fn sync(&self) -> io::Result<()> {
        self.file.sync_all()
    }

    #[cfg(windows)]
    fn sync(&self) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::FlushFileBuffers;

        let result = unsafe { FlushFileBuffers(self.file.as_raw_handle() as _) };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn sync(&self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import directory durability is unsupported on this platform",
        ))
    }

    fn identity(&self) -> FastImportPathIdentity {
        self.identity
    }

    fn existing_regular(
        &self,
        name: &std::ffi::OsStr,
    ) -> io::Result<Option<FastImportPathIdentity>> {
        match self.open_read(name) {
            Ok(file) => {
                let metadata = file.metadata()?;
                fast_import_require_regular_metadata(&metadata)?;
                fast_import_require_single_link(&file)?;
                Ok(Some(fast_import_file_identity(&file)?))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(unix)]
    fn open_read(&self, name: &std::ffi::OsStr) -> io::Result<fs::File> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::{AsRawFd, FromRawFd};

        let name = CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import filename contains NUL",
            )
        })?;
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    #[cfg(windows)]
    fn open_read(&self, name: &std::ffi::OsStr) -> io::Result<fs::File> {
        let _ = (self, name);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import Windows child access requires handle-relative APIs",
        ))
    }

    fn owned_path_matching(
        &self,
        name: &std::ffi::OsStr,
        expected: Option<FastImportPathIdentity>,
    ) -> io::Result<FastImportOwnedPath> {
        let file = self.open_read(name)?;
        let metadata = file.metadata()?;
        fast_import_require_regular_metadata(&metadata)?;
        if expected.is_none() {
            fast_import_require_single_link(&file)?;
        }
        let identity = fast_import_file_identity(&file)?;
        if expected.is_some_and(|expected| expected != identity) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import publication path was replaced",
            ));
        }
        Ok(FastImportOwnedPath {
            path: self.path.join(name),
            identity,
            parent_identity: self.identity,
            file: Some(file),
        })
    }

    #[cfg(unix)]
    fn open_append(&self, name: &std::ffi::OsStr, create_new: bool) -> io::Result<fs::File> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::{AsRawFd, FromRawFd};

        let name = CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import filename contains NUL",
            )
        })?;
        let mut flags = libc::O_RDWR | libc::O_APPEND | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        if create_new {
            flags |= libc::O_CREAT | libc::O_EXCL;
        }
        let fd = unsafe { libc::openat(self.file.as_raw_fd(), name.as_ptr(), flags, 0o666) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }

    #[cfg(windows)]
    fn open_append(&self, name: &std::ffi::OsStr, create_new: bool) -> io::Result<fs::File> {
        let _ = (self, name, create_new);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import Windows child access requires handle-relative APIs",
        ))
    }

    #[cfg(unix)]
    fn create_child_dir(&self, name: &std::ffi::OsStr) -> io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::AsRawFd;

        let name = CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import directory contains NUL",
            )
        })?;
        let result = unsafe { libc::mkdirat(self.file.as_raw_fd(), name.as_ptr(), 0o777) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(windows)]
    fn create_child_dir(&self, name: &std::ffi::OsStr) -> io::Result<()> {
        let _ = (self, name);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import Windows directory creation requires handle-relative APIs",
        ))
    }

    #[cfg(unix)]
    fn rename_noreplace(
        &self,
        source: &std::ffi::OsStr,
        destination: &std::ffi::OsStr,
    ) -> io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::AsRawFd;

        let source = CString::new(source.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import filename contains NUL",
            )
        })?;
        let destination = CString::new(destination.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import filename contains NUL",
            )
        })?;
        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                self.file.as_raw_fd(),
                source.as_ptr(),
                self.file.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe {
            libc::renameatx_np(
                self.file.as_raw_fd(),
                source.as_ptr(),
                self.file.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let result = {
            let _ = (source, destination);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "fast-import cleanup requires a no-replace rename primitive",
            ));
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(windows)]
    fn unlink_opened(&self, _name: &std::ffi::OsStr, file: &fs::File) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
            FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX, FileDispositionInfoEx,
            SetFileInformationByHandle,
        };

        let disposition = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        };
        let result = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle() as _,
                FileDispositionInfoEx,
                (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        };
        if result == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    #[cfg(unix)]
    fn unlink_opened(&self, name: &std::ffi::OsStr, file: &fs::File) -> io::Result<()> {
        let metadata = file.metadata()?;
        fast_import_require_regular_metadata(&metadata)?;
        let identity = fast_import_file_identity(file)?;
        let current = self.open_read(name)?;
        let current_identity = fast_import_file_identity(&current)?;
        if current_identity != identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import opened cleanup file was replaced",
            ));
        }
        self.unlink(name)
    }

    #[cfg(unix)]
    fn unlink(&self, name: &std::ffi::OsStr) -> io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::AsRawFd;

        let name = CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import filename contains NUL",
            )
        })?;
        let result = unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(unix)]
    fn link_from_directory(
        &self,
        source_directory: &FastImportDirectory,
        source: &std::ffi::OsStr,
        destination: &std::ffi::OsStr,
    ) -> io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::io::AsRawFd;

        let source = CString::new(source.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import filename contains NUL",
            )
        })?;
        let destination = CString::new(destination.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import filename contains NUL",
            )
        })?;
        let result = unsafe {
            libc::linkat(
                source_directory.file.as_raw_fd(),
                source.as_ptr(),
                self.file.as_raw_fd(),
                destination.as_ptr(),
                0,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(unix)]
    fn link_owned(
        &self,
        source: &FastImportOwnedPath,
        destination: &std::ffi::OsStr,
    ) -> io::Result<()> {
        self.link_owned_with_hook(source, destination, || {})
    }

    #[cfg(unix)]
    fn link_owned_with_hook<F>(
        &self,
        source: &FastImportOwnedPath,
        destination: &std::ffi::OsStr,
        before_link: F,
    ) -> io::Result<()>
    where
        F: FnOnce(),
    {
        #[cfg(target_os = "linux")]
        {
            use std::ffi::CString;
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::io::AsRawFd;

            let retained = source.file.as_ref().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fast-import source has no retained descriptor",
                )
            })?;
            let metadata = retained.metadata()?;
            fast_import_require_regular_metadata(&metadata)?;
            fast_import_require_single_link(retained)?;
            if fast_import_file_identity(retained)? != source.identity {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fast-import source descriptor was replaced",
                ));
            }
            if !Path::new("/proc/self/fd").is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "fast-import descriptor publication requires /proc/self/fd",
                ));
            }
            let source_fd_path = CString::new(format!("/proc/self/fd/{}", retained.as_raw_fd()))
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "fast-import source descriptor path contains NUL",
                    )
                })?;
            let destination_cstring = CString::new(destination.as_bytes()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "fast-import filename contains NUL",
                )
            })?;
            before_link();
            let result = unsafe {
                libc::linkat(
                    libc::AT_FDCWD,
                    source_fd_path.as_ptr(),
                    self.file.as_raw_fd(),
                    destination_cstring.as_ptr(),
                    libc::AT_SYMLINK_FOLLOW,
                )
            };
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            return self.validate_link_destination(source.identity, destination);
        }
        #[cfg(target_os = "macos")]
        {
            let parent = source.path.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "fast-import source has no parent",
                )
            })?;
            let source_directory = FastImportDirectory::open(parent)?;
            if source_directory.identity() != source.parent_identity {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fast-import source directory was replaced",
                ));
            }
            let source_name = source.path.file_name().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "fast-import source has no name",
                )
            })?;
            let source_file = source_directory.open_read(source_name)?;
            let source_metadata = source_file.metadata()?;
            fast_import_require_regular_metadata(&source_metadata)?;
            fast_import_require_single_link(&source_file)?;
            if fast_import_file_identity(&source_file)? != source.identity {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fast-import source was replaced",
                ));
            }
            before_link();
            self.link_from_directory(&source_directory, source_name, destination)?;
            return self.validate_link_destination(source.identity, destination);
        }
        #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
        {
            let _ = (source, destination, before_link);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "fast-import publication requires a platform link primitive",
            ))
        }
    }

    #[cfg(unix)]
    fn validate_link_destination(
        &self,
        source_identity: FastImportPathIdentity,
        destination: &std::ffi::OsStr,
    ) -> io::Result<()> {
        let destination_file = self.open_read(destination)?;
        let destination_metadata = destination_file.metadata()?;
        fast_import_require_regular_metadata(&destination_metadata)?;
        if fast_import_file_identity(&destination_file)? != source_identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import linked destination has the wrong identity",
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn link_owned(
        &self,
        source: &FastImportOwnedPath,
        destination: &std::ffi::OsStr,
    ) -> io::Result<()> {
        let _ = (self, source, destination);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import publication requires handle-relative APIs",
        ))
    }
}

#[cfg(windows)]
fn fast_import_reject_windows_reparse_components(
    path: &Path,
    allow_missing_leaf: bool,
) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let mut current = PathBuf::new();
    let components = path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error)
                if allow_missing_leaf
                    && index + 1 == components.len()
                    && error.kind() == io::ErrorKind::NotFound =>
            {
                break;
            }
            Err(error) => return Err(error),
        };
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import path contains a reparse point",
            ));
        }
    }
    Ok(())
}

struct FastImportPackState {
    algorithm: GitHashAlgorithm,
    pack_dir: PathBuf,
    edge_pack_dir: PathBuf,
    unpack_limit: Option<usize>,
    seen: std::collections::HashSet<ObjectId>,
    loose_paths: std::collections::HashMap<ObjectId, FastImportOwnedPath>,
    pending: Vec<ObjectId>,
    published: Vec<FastImportPublishedPack>,
}

impl FastImportPackState {
    fn new(
        pack_dir: PathBuf,
        edge_pack_dir: PathBuf,
        algorithm: GitHashAlgorithm,
        unpack_limit: Option<usize>,
    ) -> Self {
        Self {
            algorithm,
            pack_dir,
            edge_pack_dir,
            unpack_limit,
            seen: std::collections::HashSet::new(),
            loose_paths: std::collections::HashMap::new(),
            pending: Vec::new(),
            published: Vec::new(),
        }
    }

    fn record(&mut self, id: &ObjectId, loose_path: FastImportOwnedPath) -> io::Result<()> {
        if id.algorithm() != self.algorithm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "fast-import object id algorithm does not match repository",
            ));
        }
        if self.seen.contains(id) {
            return Ok(());
        }
        if self.seen.len() >= u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import object count exceeds pack v2 limit",
            ));
        }
        self.seen.insert(id.clone());
        self.loose_paths.insert(id.clone(), loose_path);
        self.pending.push(id.clone());
        Ok(())
    }

    fn pending_contains(&self, id: &ObjectId) -> bool {
        self.pending.iter().any(|pending| pending == id)
    }

    fn finalize(
        &mut self,
        store: &FastImportObjectStore<'_>,
        edge_ids: &[ObjectId],
    ) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        if self
            .unpack_limit
            .is_some_and(|limit| self.pending.len() <= limit)
        {
            self.pending.clear();
            return Ok(());
        }
        let ids = std::mem::take(&mut self.pending);
        match install_fast_import_pack(&self.pack_dir, self.algorithm, store, &ids) {
            Ok(installed) => {
                self.published.push(FastImportPublishedPack {
                    directory: installed.directory,
                    pack_path: installed.pack_path,
                    pack_owned: installed.pack_owned,
                    idx_owned: installed.idx_owned,
                    keep_path: installed.keep_owned,
                    edge_ids: edge_ids.to_vec(),
                });
                let mut cleanup_failures = Vec::new();
                for id in &ids {
                    if let Some(path) = self.loose_paths.get(id) {
                        if let Err(error) = remove_fast_import_owned_file(path) {
                            cleanup_failures.push(error.to_string());
                        }
                    }
                }
                if cleanup_failures.is_empty() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "fast-import published pack but loose-object cleanup failed: {}",
                        cleanup_failures.join("; ")
                    )))
                }
            }
            Err(error) => Err(error),
        }
    }

    fn commit(&mut self) -> io::Result<()> {
        self.release_keeps()?;
        self.published.clear();
        self.loose_paths.clear();
        self.pending.clear();
        Ok(())
    }

    fn release_keeps(&mut self) -> io::Result<()> {
        let mut failures = Vec::new();
        for published in &self.published {
            let mut final_pair_is_valid = true;
            if let Some(path) = published.pack_owned.as_ref() {
                if let Err(error) =
                    verify_fast_import_owned_file_in_directory(&published.directory, path)
                {
                    final_pair_is_valid = false;
                    failures.push(error.to_string());
                }
            }
            if let Some(path) = published.idx_owned.as_ref() {
                if let Err(error) =
                    verify_fast_import_owned_file_in_directory(&published.directory, path)
                {
                    final_pair_is_valid = false;
                    failures.push(error.to_string());
                }
            }
            if final_pair_is_valid && let Some(path) = published.keep_path.as_ref() {
                match remove_fast_import_owned_file_locked(path) {
                    Ok(true) => {
                        if let Err(error) = published.directory.sync() {
                            failures.push(error.to_string());
                        }
                    }
                    Ok(false) => {}
                    Err(error) => failures.push(error.to_string()),
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(failures.join("; ")))
        }
    }

    fn abort(
        &mut self,
        store: &FastImportObjectStore<'_>,
        edge_ids: &[ObjectId],
    ) -> io::Result<()> {
        let mut failures = Vec::new();
        if !self.pending.is_empty()
            && let Err(error) = self.finalize(store, edge_ids)
        {
            failures.push(format!("finalizing current pack: {error}"));
        }
        if let Err(error) = self.release_keeps() {
            failures.push(error.to_string());
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(failures.join("; ")))
        }
    }
}

struct FastImportObjectStore<'a> {
    inner: &'a LooseObjectStore,
    pack_state: Rc<RefCell<FastImportPackState>>,
}

struct FastImportHashingWriter<'a> {
    inner: &'a mut dyn Write,
    hasher: GitObjectHash,
    remaining: usize,
}

impl<'a> FastImportHashingWriter<'a> {
    fn new(inner: &'a mut dyn Write, algorithm: GitHashAlgorithm, size: usize) -> Self {
        let mut hasher = GitObjectHash::new(algorithm);
        hasher.update_object_header(GitObjectKind::Blob, size);
        Self {
            inner,
            hasher,
            remaining: size,
        }
    }

    fn finish_stream(self, expected_size: usize) -> io::Result<ObjectId> {
        if self.remaining != 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("fast-import blob ended before declared size {expected_size}"),
            ));
        }
        Ok(self.hasher.finalize())
    }
}

impl Write for FastImportHashingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import blob exceeded declared size",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..written]);
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> FastImportObjectStore<'a> {
    fn new(inner: &'a LooseObjectStore, pack_state: Rc<RefCell<FastImportPackState>>) -> Self {
        Self { inner, pack_state }
    }

    fn record_created(&self, id: &ObjectId, path: FastImportOwnedPath) -> io::Result<()> {
        self.pack_state.borrow_mut().record(id, path)
    }

    fn algorithm(&self) -> GitHashAlgorithm {
        self.inner.algorithm()
    }

    fn max_object_bytes(&self) -> usize {
        self.inner.max_object_bytes()
    }

    fn write_object_with_newness(
        &self,
        kind: GitObjectKind,
        content: &[u8],
    ) -> io::Result<(ObjectId, bool)> {
        let id = hash_object(self.inner.algorithm(), kind, content);
        let compressed = encode_loose_object(kind, content)?;
        let path = install_fast_import_loose_object(self.inner.objects_dir(), &id, &compressed)?;
        if let Some(path) = path {
            self.record_created(&id, path)?;
            return Ok((id, true));
        }
        Ok((id, false))
    }
}

impl GitObjectStore for FastImportObjectStore<'_> {
    fn read_object(&self, id: &ObjectId) -> io::Result<LooseObject> {
        self.inner.read_object(id)
    }

    fn try_write_reusable_pack_object_with_buffer(
        &self,
        id: &ObjectId,
        writer: &mut dyn Write,
        buffer: &mut [u8],
    ) -> io::Result<bool> {
        self.inner
            .try_write_reusable_pack_object_with_buffer(id, writer, buffer)
    }

    fn streamable_blob_size_hint(&self, id: &ObjectId) -> io::Result<Option<usize>> {
        self.inner.streamable_blob_size_hint(id)
    }

    fn write_streamable_blob(&self, id: &ObjectId, writer: &mut dyn Write) -> io::Result<bool> {
        self.inner.write_streamable_blob(id, writer)
    }
}

impl GitObjectSink for FastImportObjectStore<'_> {
    fn write_object(&self, kind: GitObjectKind, content: &[u8]) -> io::Result<ObjectId> {
        self.write_object_with_newness(kind, content)
            .map(|(id, _)| id)
    }

    fn write_streamed_blob_content<F>(&self, size: usize, write_content: F) -> io::Result<ObjectId>
    where
        F: FnOnce(&mut dyn Write) -> io::Result<()>,
    {
        let objects_dir = self.inner.objects_dir().to_owned();
        let objects_directory = FastImportDirectory::open(&objects_dir)?;
        let (temp, temp_owned) = fast_import_temp_file_in_directory(&objects_directory, "object")?;
        let result = (|| {
            let mut encoder = ZlibEncoder::new(temp, Compression::default());
            let header = format!("blob {size}\0");
            encoder.write_all(header.as_bytes())?;
            let mut content = FastImportHashingWriter::new(&mut encoder, self.algorithm(), size);
            write_content(&mut content)?;
            let id = content.finish_stream(size)?;
            let mut temp = encoder.finish()?;
            temp.flush()?;
            temp.sync_all()?;
            drop(temp);
            let path = fast_import_install_loose_temp(&objects_dir, &id, &temp_owned)?;
            if let Some(path) = path {
                self.record_created(&id, path)?;
            }
            Ok(id)
        })();
        fast_import_cleanup_after_error(result, &temp_owned)
    }
}

fn fast_import_temp_file_in_directory(
    directory: &FastImportDirectory,
    suffix: &str,
) -> io::Result<(fs::File, FastImportOwnedPath)> {
    use std::ffi::OsString;

    for attempt in 0..1024_u32 {
        let name = OsString::from(format!(
            ".zmin-fast-import-{}-{attempt}.{suffix}",
            std::process::id()
        ));
        match directory.open_append(&name, true) {
            Ok(file) => {
                let path = directory.path.join(&name);
                let identity = fast_import_file_identity(&file)?;
                let retained = file.try_clone()?;
                return Ok((
                    file,
                    FastImportOwnedPath {
                        path,
                        identity,
                        parent_identity: directory.identity,
                        file: Some(retained),
                    },
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique fast-import temporary file",
    ))
}

fn fast_import_temp_pack_file_in_directory(
    directory: &FastImportDirectory,
) -> io::Result<FastImportPackTemp> {
    use std::ffi::OsString;

    for attempt in 0..1024_u32 {
        let stem = format!(".zmin-fast-import-{}-{attempt}", std::process::id());
        let pack_name = OsString::from(format!("{stem}.pack"));
        match directory.open_append(&pack_name, true) {
            Ok(file) => {
                let path = directory.path.join(&pack_name);
                let identity = fast_import_file_identity(&file)?;
                let retained = file.try_clone()?;
                return Ok(FastImportPackTemp {
                    file,
                    owned: FastImportOwnedPath {
                        path,
                        identity,
                        parent_identity: directory.identity,
                        file: Some(retained),
                    },
                    stem: OsString::from(stem),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique fast-import pack temporary",
    ))
}

fn fast_import_temp_index_file_in_directory(
    directory: &FastImportDirectory,
    stem: &std::ffi::OsStr,
) -> io::Result<(fs::File, FastImportOwnedPath)> {
    let mut name = stem.to_owned();
    name.push(".idx");
    let file = directory.open_append(&name, true)?;
    let path = directory.path.join(&name);
    let identity = fast_import_file_identity(&file)?;
    let retained = file.try_clone()?;
    Ok((
        file,
        FastImportOwnedPath {
            path,
            identity,
            parent_identity: directory.identity,
            file: Some(retained),
        },
    ))
}

fn fast_import_rewind_file(path: &mut FastImportOwnedPath) -> io::Result<&fs::File> {
    let file = path.file.as_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import temporary has no retained descriptor",
        )
    })?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}

fn fast_import_loose_object_path(objects_dir: &Path, id: &ObjectId) -> PathBuf {
    let hex = id.to_hex();
    objects_dir.join(&hex[..2]).join(&hex[2..])
}

fn install_fast_import_loose_object(
    objects_dir: &Path,
    id: &ObjectId,
    compressed: &[u8],
) -> io::Result<Option<FastImportOwnedPath>> {
    let path = fast_import_loose_object_path(objects_dir, id);
    let parent = fast_import_ensure_loose_parent(&path)?;
    let (mut temp, temp_owned) = fast_import_temp_file_in_directory(&parent, "object")?;
    let result = (|| {
        temp.write_all(compressed)?;
        temp.flush()?;
        temp.sync_all()?;
        drop(temp);
        fast_import_install_loose_temp(objects_dir, id, &temp_owned)
    })();
    fast_import_cleanup_after_error(result, &temp_owned)
}

fn fast_import_install_loose_temp(
    objects_dir: &Path,
    id: &ObjectId,
    temp: &FastImportOwnedPath,
) -> io::Result<Option<FastImportOwnedPath>> {
    let path = fast_import_loose_object_path(objects_dir, id);
    let directory = fast_import_ensure_loose_parent(&path)?;
    let name = path.file_name().expect("object filename");
    match directory.link_owned(temp, name) {
        Ok(()) => {
            remove_fast_import_owned_file(temp)?;
            Ok(Some(
                directory.owned_path_matching(name, Some(temp.identity))?,
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            directory.existing_regular(name)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fast-import loose object collision disappeared",
                )
            })?;
            remove_fast_import_owned_file(temp)?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn fast_import_ensure_loose_parent(path: &Path) -> io::Result<FastImportDirectory> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import loose object path has no parent",
        )
    })?;
    match FastImportDirectory::open(parent) {
        Ok(directory) => return Ok(directory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let grandparent = parent.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import loose object parent has no grandparent",
        )
    })?;
    let directory = FastImportDirectory::open(grandparent)?;
    let name = parent.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import loose object parent has no name",
        )
    })?;
    #[cfg(unix)]
    match directory.create_child_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    #[cfg(windows)]
    match directory.create_child_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    FastImportDirectory::open(parent)
}

fn fast_import_file_identity(file: &fs::File) -> io::Result<FastImportPathIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = file.metadata()?;
        return Ok(FastImportPathIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        });
    }
    #[cfg(windows)]
    {
        let information = fast_import_windows_file_information(file)?;

        return Ok(FastImportPathIdentity::Windows {
            volume_serial: information.dwVolumeSerialNumber,
            file_index: fast_import_windows_file_index(
                information.nFileIndexHigh,
                information.nFileIndexLow,
            ),
        });
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import publication identity is unsupported on this platform",
        ))
    }
}

#[cfg(unix)]
fn fast_import_metadata_identity(metadata: &fs::Metadata) -> io::Result<FastImportPathIdentity> {
    use std::os::unix::fs::MetadataExt;

    Ok(FastImportPathIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn fast_import_windows_file_information(
    file: &fs::File,
) -> io::Result<windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information)
}

#[cfg(any(windows, test))]
fn fast_import_windows_file_index(high: u32, low: u32) -> u64 {
    (u64::from(high) << u32::BITS) | u64::from(low)
}

fn fast_import_require_regular_metadata(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import publication path is not a regular file",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import publication path is a reparse point",
            ));
        }
    }
    Ok(())
}

fn fast_import_require_single_link(file: &fs::File) -> io::Result<()> {
    let links = fast_import_file_link_count(file)?;
    if links != 1 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "fast-import final path is a hard link",
        ));
    }
    Ok(())
}

fn fast_import_file_link_count(file: &fs::File) -> io::Result<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        return Ok(file.metadata()?.nlink());
    }
    #[cfg(windows)]
    {
        return Ok(u64::from(
            fast_import_windows_file_information(file)?.nNumberOfLinks,
        ));
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import final path ownership is unsupported on this platform",
        ))
    }
}

#[cfg(unix)]
fn fast_import_metadata_link_count(metadata: &fs::Metadata) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;

    Ok(metadata.nlink())
}

#[cfg(unix)]
fn fast_import_require_single_link_metadata(metadata: &fs::Metadata) -> io::Result<()> {
    let links = fast_import_metadata_link_count(metadata)?;
    if links != 1 {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "fast-import final path is a hard link",
        ));
    }
    Ok(())
}

fn remove_fast_import_owned_file(path: &FastImportOwnedPath) -> io::Result<bool> {
    remove_fast_import_owned_file_after_open(path, || {})
}

#[cfg(unix)]
fn fast_import_restore_quarantine(
    directory: &FastImportDirectory,
    quarantine: &std::ffi::OsStr,
    name: &std::ffi::OsStr,
    primary: io::Error,
) -> io::Error {
    match directory.rename_noreplace(quarantine, name) {
        Ok(()) => io::Error::new(
            primary.kind(),
            format!("{primary}; fast-import cleanup pathname was restored"),
        ),
        Err(restore) => io::Error::new(
            restore.kind(),
            format!("{primary}; fast-import cleanup replacement could not be restored: {restore}"),
        ),
    }
}

fn remove_fast_import_owned_file_after_open<F>(
    path: &FastImportOwnedPath,
    after_open: F,
) -> io::Result<bool>
where
    F: FnOnce(),
{
    let parent = path.path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import owned path has no parent",
        )
    })?;
    let directory = FastImportDirectory::open(parent)?;
    if directory.identity() != path.parent_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import owned directory was replaced",
        ));
    }
    let name = path.path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import owned path has no filename",
        )
    })?;
    let current_file = match path.file.as_ref() {
        Some(file) => file.try_clone()?,
        None => match directory.open_read(name) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        },
    };
    let current = fast_import_file_identity(&current_file)?;
    if current != path.identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import owned path was replaced",
        ));
    }
    after_open();
    #[cfg(unix)]
    {
        use std::ffi::OsString;

        for attempt in 0..1024_u32 {
            let quarantine = OsString::from(format!(
                ".zmin-fast-import-trash-{}-{attempt}",
                std::process::id()
            ));
            match directory.rename_noreplace(name, &quarantine) {
                Ok(()) => {
                    let quarantined = match directory.open_read(&quarantine) {
                        Ok(file) => file,
                        Err(error) => {
                            return Err(fast_import_restore_quarantine(
                                &directory,
                                &quarantine,
                                name,
                                error,
                            ));
                        }
                    };
                    let quarantined_identity = match fast_import_file_identity(&quarantined) {
                        Ok(identity) => identity,
                        Err(error) => {
                            return Err(fast_import_restore_quarantine(
                                &directory,
                                &quarantine,
                                name,
                                error,
                            ));
                        }
                    };
                    if quarantined_identity == path.identity {
                        directory.unlink_opened(&quarantine, &quarantined)?;
                        return Ok(true);
                    }
                    let restore = directory.rename_noreplace(&quarantine, name);
                    return match restore {
                        Ok(()) => Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "fast-import owned path was replaced during cleanup",
                        )),
                        Err(error) => Err(io::Error::new(
                            error.kind(),
                            format!(
                                "fast-import cleanup replacement could not be restored: {error}"
                            ),
                        )),
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a fast-import cleanup quarantine name",
        ));
    }
    #[cfg(windows)]
    {
        directory.unlink_opened(name, &current_file)?;
        return Ok(true);
    }
    #[cfg(not(any(unix, windows)))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "fast-import owned path removal is unsupported on this platform",
    ));
}

fn verify_fast_import_owned_file(path: &FastImportOwnedPath) -> io::Result<()> {
    let parent = path.path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import owned path has no parent",
        )
    })?;
    let directory = FastImportDirectory::open(parent)?;
    verify_fast_import_owned_file_in_directory(&directory, path)
}

fn verify_fast_import_owned_file_in_directory(
    directory: &FastImportDirectory,
    path: &FastImportOwnedPath,
) -> io::Result<()> {
    if directory.identity() != path.parent_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import owned directory was replaced",
        ));
    }
    let name = path.path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import owned path has no filename",
        )
    })?;
    let current_file = directory.open_read(name)?;
    let current_metadata = current_file.metadata()?;
    fast_import_require_regular_metadata(&current_metadata)?;
    let retained_file = path.file.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import owned path has no retained descriptor",
        )
    })?;
    let retained_metadata = retained_file.metadata()?;
    fast_import_require_regular_metadata(&retained_metadata)?;
    let current_identity = fast_import_file_identity(&current_file)?;
    let retained_identity = fast_import_file_identity(retained_file)?;
    if current_identity != path.identity
        || retained_identity != path.identity
        || fast_import_file_link_count(&current_file)?
            != fast_import_file_link_count(retained_file)?
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import owned path was replaced",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_fast_import_edges_file(path: &Path) -> io::Result<fs::File> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = FastImportDirectory::open(parent)?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import pack-edges path has no filename",
        )
    })?;
    for _ in 0..2 {
        let expected = match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "fast-import pack-edges path is not a regular file",
                    ));
                }
                fast_import_require_regular_metadata(&metadata)?;
                fast_import_require_single_link_metadata(&metadata)?;
                Some(fast_import_metadata_identity(&metadata)?)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let file = match directory.open_append(name, expected.is_none()) {
            Ok(file) => file,
            Err(error) if expected.is_none() && error.kind() == io::ErrorKind::AlreadyExists => {
                continue;
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        fast_import_require_regular_metadata(&metadata)?;
        fast_import_require_single_link(&file)?;
        let actual = fast_import_file_identity(&file)?;
        if expected.is_some_and(|identity| identity != actual) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "fast-import pack-edges path was replaced",
            ));
        }
        return Ok(file);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "fast-import pack-edges path changed during open",
    ))
}

#[cfg(windows)]
fn open_fast_import_edges_file(path: &Path) -> io::Result<fs::File> {
    let _ = path;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "fast-import pack-edges requires handle-relative Windows APIs",
    ))
}

#[cfg(not(any(unix, windows)))]
fn open_fast_import_edges_file(_path: &Path) -> io::Result<fs::File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "fast-import pack-edges publication is unsupported on this platform",
    ))
}

fn ensure_fast_import_pack_dir(pack_dir: &Path) -> io::Result<FastImportDirectory> {
    match FastImportDirectory::open(pack_dir) {
        Ok(directory) => {
            directory.sync()?;
            return Ok(directory);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = pack_dir.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import objects/pack has no parent",
        )
    })?;
    let parent_directory = FastImportDirectory::open(parent)?;
    let name = pack_dir.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import objects/pack has no name",
        )
    })?;
    #[cfg(unix)]
    match parent_directory.create_child_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    #[cfg(windows)]
    match parent_directory.create_child_dir(name) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let directory = FastImportDirectory::open(pack_dir)?;
    directory.sync()?;
    Ok(directory)
}

fn cleanup_fast_import_owned_paths(paths: &[&FastImportOwnedPath]) -> io::Result<()> {
    let mut failures = Vec::new();
    for path in paths {
        let result = if path.path.extension().and_then(|value| value.to_str()) == Some("keep") {
            remove_fast_import_owned_file_locked(path)
        } else {
            remove_fast_import_owned_file(path)
        };
        if let Err(error) = result {
            failures.push(error);
        }
    }
    if failures.is_empty() {
        return Ok(());
    }
    let message = failures
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    Err(io::Error::new(
        failures[0].kind(),
        format!("fast-import cleanup failed: {message}"),
    ))
}

fn remove_fast_import_owned_file_locked(path: &FastImportOwnedPath) -> io::Result<bool> {
    let parent = path.path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "fast-import owned keep has no parent",
        )
    })?;
    let _pack_lock = PackOperationLock::acquire(parent)?;
    remove_fast_import_owned_file(path)
}

fn combine_fast_import_cleanup_error(primary: io::Error, cleanup: io::Error) -> io::Error {
    io::Error::new(primary.kind(), format!("{primary}; {cleanup}"))
}

fn fast_import_cleanup_after_error<T>(
    result: io::Result<T>,
    owned: &FastImportOwnedPath,
) -> io::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(primary) => match remove_fast_import_owned_file(owned) {
            Ok(_) => Err(primary),
            Err(cleanup) => Err(combine_fast_import_cleanup_error(primary, cleanup)),
        },
    }
}

fn install_fast_import_pack(
    pack_dir: &Path,
    algorithm: GitHashAlgorithm,
    store: &FastImportObjectStore<'_>,
    ids: &[ObjectId],
) -> io::Result<FastImportInstalledPack> {
    #[cfg(windows)]
    {
        let _ = (pack_dir, algorithm, store, ids);
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fast-import pack publication requires descriptor-relative Windows APIs",
        ));
    }
    let pack_directory = ensure_fast_import_pack_dir(pack_dir)?;

    let FastImportPackTemp {
        file: mut pack_file,
        owned: mut pack_temp_owned,
        stem,
    } = fast_import_temp_pack_file_in_directory(&pack_directory)?;
    let mut idx_file: Option<fs::File> = None;
    let mut idx_temp_owned: Option<FastImportOwnedPath> = None;
    let mut keep_temp: Option<FastImportOwnedPath> = None;
    let mut owned_pack = None;
    let mut owned_idx = None;
    let mut owned_keep = None;
    let mut pair_published = false;
    let result = (|| {
        write_undeltified_pack_from_store(store, algorithm, ids, &mut pack_file)?;
        pack_file.flush()?;
        pack_file.sync_all()?;

        let (created_idx_file, created_idx_owned) =
            fast_import_temp_index_file_in_directory(&pack_directory, &stem)?;
        idx_file = Some(created_idx_file);
        idx_temp_owned = Some(created_idx_owned);

        let indexed = zmin_git_core::pack::index_pack_file_index_only_from_file(
            algorithm,
            fast_import_rewind_file(&mut pack_temp_owned)?,
        )?;
        if indexed.objects != ids.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "generated fast-import pack object count mismatch",
            ));
        }
        let idx_file = idx_file.as_mut().expect("index temp file exists");
        idx_file.write_all(&indexed.index)?;
        idx_file.flush()?;
        idx_file.sync_all()?;
        let idx_temp_owned = idx_temp_owned
            .as_mut()
            .expect("index temp ownership exists");
        zmin_git_core::pack::validate_pack_index_file_from_file(
            algorithm,
            fast_import_rewind_file(idx_temp_owned)?,
        )?;
        zmin_git_core::pack::verify_pack_file_matches_index_from_files(
            algorithm,
            fast_import_rewind_file(&mut pack_temp_owned)?,
            fast_import_rewind_file(idx_temp_owned)?,
            false,
        )?;

        let _pack_lock = PackOperationLock::acquire(pack_dir)?;

        let pack_name = format!("pack-{}.pack", indexed.pack_id.to_hex());
        let idx_name = format!("pack-{}.idx", indexed.pack_id.to_hex());
        let keep_name = format!("pack-{}.keep", indexed.pack_id.to_hex());
        let pack_path = pack_dir.join(&pack_name);
        let pack_exists = pack_directory
            .existing_regular(Path::new(&pack_name).file_name().expect("pack filename"))?;
        let idx_exists = pack_directory
            .existing_regular(Path::new(&idx_name).file_name().expect("index filename"))?;
        if pack_exists.is_some() || idx_exists.is_some() {
            if !(pack_exists.is_some() && idx_exists.is_some()) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "existing fast-import pack pair is incomplete",
                ));
            }
            let mut pack_owned = pack_directory.owned_path_matching(
                Path::new(&pack_name).file_name().expect("pack filename"),
                pack_exists,
            )?;
            let mut idx_owned = pack_directory.owned_path_matching(
                Path::new(&idx_name).file_name().expect("index filename"),
                idx_exists,
            )?;
            verify_fast_import_owned_file(&pack_owned)?;
            verify_fast_import_owned_file(&idx_owned)?;
            zmin_git_core::pack::verify_pack_file_matches_index_from_files(
                algorithm,
                fast_import_rewind_file(&mut pack_owned)?,
                fast_import_rewind_file(&mut idx_owned)?,
                false,
            )?;
            verify_fast_import_owned_file(&pack_owned)?;
            verify_fast_import_owned_file(&idx_owned)?;
            return Ok(FastImportInstalledPack {
                directory: pack_directory,
                pack_path,
                pack_owned: Some(pack_owned),
                idx_owned: Some(idx_owned),
                keep_owned: None,
            });
        }
        if pack_directory
            .existing_regular(Path::new(&keep_name).file_name().expect("keep filename"))?
            .is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "existing fast-import pack keep file has no pack pair",
            ));
        }

        let (mut keep_file, keep_owned_temp) =
            fast_import_temp_file_in_directory(&pack_directory, "keep")?;
        let keep_expected_identity = keep_owned_temp.identity;
        keep_file.write_all(b"fast-import")?;
        keep_file.flush()?;
        keep_file.sync_all()?;
        drop(keep_file);
        keep_temp = Some(keep_owned_temp);
        let keep_path_temp = keep_temp.as_ref().expect("keep temporary path exists");
        pack_directory.link_owned(
            keep_path_temp,
            Path::new(&keep_name).file_name().expect("keep filename"),
        )?;
        owned_keep = Some(pack_directory.owned_path_matching(
            Path::new(&keep_name).file_name().expect("keep filename"),
            Some(keep_expected_identity),
        )?);
        remove_fast_import_owned_file(keep_temp.as_ref().expect("keep temporary path exists"))?;
        keep_temp = None;

        pack_directory.link_owned(
            &pack_temp_owned,
            Path::new(&pack_name).file_name().expect("pack filename"),
        )?;
        owned_pack = Some(pack_directory.owned_path_matching(
            Path::new(&pack_name).file_name().expect("pack filename"),
            Some(pack_temp_owned.identity),
        )?);
        pack_directory.link_owned(
            idx_temp_owned,
            Path::new(&idx_name).file_name().expect("index filename"),
        )?;
        owned_idx = Some(pack_directory.owned_path_matching(
            Path::new(&idx_name).file_name().expect("index filename"),
            Some(idx_temp_owned.identity),
        )?);
        verify_fast_import_owned_file(
            owned_pack
                .as_ref()
                .expect("published pack ownership was captured"),
        )?;
        verify_fast_import_owned_file(
            owned_idx
                .as_ref()
                .expect("published index ownership was captured"),
        )?;
        pair_published = true;
        pack_directory.sync()?;
        let published_keep = owned_keep
            .take()
            .ok_or_else(|| io::Error::other("fast-import keep ownership was not captured"))?;
        Ok(FastImportInstalledPack {
            directory: pack_directory,
            pack_path,
            pack_owned: owned_pack.take(),
            idx_owned: owned_idx.take(),
            keep_owned: Some(published_keep),
        })
    })();

    match result {
        Ok(installed) => {
            let mut temporary = vec![&pack_temp_owned];
            if let Some(path) = idx_temp_owned.as_ref() {
                temporary.push(path);
            }
            if let Err(cleanup) = cleanup_fast_import_owned_paths(&temporary) {
                let cleanup = if let Some(keep) = installed.keep_owned.as_ref() {
                    match remove_fast_import_owned_file_locked(keep) {
                        Ok(_) => cleanup,
                        Err(release) => combine_fast_import_cleanup_error(cleanup, release),
                    }
                } else {
                    cleanup
                };
                return Err(cleanup);
            }
            Ok(installed)
        }
        Err(primary) => {
            let mut cleanup_paths = vec![&pack_temp_owned];
            if let Some(path) = idx_temp_owned.as_ref() {
                cleanup_paths.push(path);
            }
            if let Some(path) = keep_temp.as_ref() {
                cleanup_paths.push(path);
            }
            if pair_published {
                if let Some(path) = owned_keep.as_ref() {
                    cleanup_paths.push(path);
                }
            } else {
                if let Some(path) = owned_idx.as_ref() {
                    cleanup_paths.push(path);
                }
                if let Some(path) = owned_pack.as_ref() {
                    cleanup_paths.push(path);
                }
                if let Some(path) = owned_keep.as_ref() {
                    cleanup_paths.push(path);
                }
            }
            match cleanup_fast_import_owned_paths(&cleanup_paths) {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(combine_fast_import_cleanup_error(primary, cleanup)),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FastImportPath(Vec<u8>);

impl FastImportPath {
    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FastImportPathDelimiter {
    Eol,
    Space,
}

struct FastImportPathCursor<'a> {
    value: &'a [u8],
    position: usize,
}

impl<'a> FastImportPathCursor<'a> {
    fn new(value: &'a [u8]) -> Self {
        Self { value, position: 0 }
    }

    fn remainder(&self) -> &'a [u8] {
        &self.value[self.position..]
    }

    fn take_space_field(&mut self) -> io::Result<&'a [u8]> {
        let start = self.position;
        let Some(offset) = self.value[start..].iter().position(|byte| *byte == b' ') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing space after field",
            ));
        };
        let end = start + offset;
        self.position = end + 1;
        Ok(&self.value[start..end])
    }

    fn parse_path(&mut self, delimiter: FastImportPathDelimiter) -> io::Result<FastImportPath> {
        let start = self.position;
        if self.value.get(start) == Some(&b'"') {
            return self.parse_quoted_path(delimiter);
        }

        let remainder = &self.value[start..];
        let nul = remainder.iter().position(|byte| *byte == 0);
        let limit = nul.unwrap_or(remainder.len());
        let bytes = &remainder[..limit];
        match delimiter {
            FastImportPathDelimiter::Eol => {
                self.position = self.value.len();
                Ok(FastImportPath(bytes.to_vec()))
            }
            FastImportPathDelimiter::Space => {
                let Some(space) = bytes.iter().position(|byte| *byte == b' ') else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "missing space after path",
                    ));
                };
                self.position = start + space + 1;
                Ok(FastImportPath(bytes[..space].to_vec()))
            }
        }
    }

    fn parse_quoted_path(
        &mut self,
        delimiter: FastImportPathDelimiter,
    ) -> io::Result<FastImportPath> {
        let mut path = Vec::with_capacity(self.value.len().saturating_sub(self.position + 2));
        let mut cursor = self.position + 1;
        while cursor < self.value.len() {
            let byte = self.value[cursor];
            cursor += 1;
            match byte {
                b'"' => {
                    match delimiter {
                        FastImportPathDelimiter::Eol => {
                            if self.value.get(cursor) == Some(&b'\0') {
                                cursor = self.value.len();
                            } else if cursor != self.value.len() {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "garbage after path",
                                ));
                            }
                        }
                        FastImportPathDelimiter::Space => {
                            if self.value.get(cursor) != Some(&b' ') {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "missing space after path",
                                ));
                            }
                            cursor += 1;
                        }
                    }
                    self.position = cursor;
                    return Ok(FastImportPath(path));
                }
                b'\0' => return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path")),
                b'\\' => {
                    let escape = *self.value.get(cursor).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "invalid path")
                    })?;
                    cursor += 1;
                    let decoded = match escape {
                        b'a' => 0x07,
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'v' => 0x0b,
                        b'\\' => b'\\',
                        b'"' => b'"',
                        b'0'..=b'3' => {
                            let first = escape - b'0';
                            let second = *self.value.get(cursor).ok_or_else(|| {
                                io::Error::new(io::ErrorKind::InvalidInput, "invalid path")
                            })?;
                            let third = *self.value.get(cursor + 1).ok_or_else(|| {
                                io::Error::new(io::ErrorKind::InvalidInput, "invalid path")
                            })?;
                            if !(b'0'..=b'7').contains(&second) || !(b'0'..=b'7').contains(&third) {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "invalid path",
                                ));
                            }
                            cursor += 2;
                            (first << 6) | ((second - b'0') << 3) | (third - b'0')
                        }
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "invalid path",
                            ));
                        }
                    };
                    if decoded == 0 {
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"));
                    }
                    path.push(decoded);
                }
                _ => path.push(byte),
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))
    }
}

fn parse_path_eol(value: &[u8]) -> io::Result<FastImportPath> {
    FastImportPathCursor::new(value).parse_path(FastImportPathDelimiter::Eol)
}

fn parse_path_space(value: &[u8]) -> io::Result<(FastImportPath, &[u8])> {
    let mut cursor = FastImportPathCursor::new(value);
    let path = cursor.parse_path(FastImportPathDelimiter::Space)?;
    Ok((path, cursor.remainder()))
}

struct FastImportLsRequest {
    dataref: Option<FastImportLsDataRef>,
    path: FastImportPath,
}

enum FastImportLsDataRef {
    Mark(usize),
    Object(ObjectId),
}

struct FastImportLsEntry {
    mode: TreeMode,
    id: ObjectId,
    path: Vec<u8>,
}

#[derive(Clone, Copy)]
enum FastImportCopyKind {
    File(IndexMode),
    Tree,
}

#[derive(Clone, Default)]
struct FastImportEmptyTreeState {
    entries: std::collections::BTreeMap<Vec<u8>, ObjectId>,
    root_empty_tree_id: Option<ObjectId>,
}

impl FastImportEmptyTreeState {
    fn from_index(index: &mut GitIndex) -> Self {
        let tree_entries = index
            .entries()
            .iter()
            .filter(|entry| entry.mode == IndexMode::Tree)
            .map(|entry| (entry.path.clone(), entry.id.clone()))
            .collect::<Vec<_>>();
        let mut state = Self::default();
        for (path, id) in tree_entries {
            index.remove_fast_import_path(&path);
            if path.is_empty() {
                state.root_empty_tree_id = Some(id);
            } else {
                state.entries.insert(path, id);
            }
        }
        state
    }

    fn remove_path(&mut self, path: &[u8]) {
        if path.is_empty() {
            self.entries.clear();
            self.root_empty_tree_id = None;
            return;
        }
        let mut prefix = path.to_vec();
        prefix.push(b'/');
        self.entries.retain(|entry_path, _| {
            entry_path.as_slice() != path && !entry_path.starts_with(&prefix)
        });
    }

    fn remove_ancestor_trees(&mut self, path: &[u8]) {
        let mut end = path.len();
        while let Some(separator) = path[..end].iter().rposition(|byte| *byte == b'/') {
            self.entries.remove(&path[..separator]);
            end = separator;
        }
    }

    fn remove_for_leaf(&mut self, path: &[u8]) {
        self.remove_path(path);
        self.remove_ancestor_trees(path);
    }

    fn insert(&mut self, path: Vec<u8>, id: ObjectId) {
        if path.is_empty() {
            return;
        }
        self.remove_path(&path);
        self.remove_ancestor_trees(&path);
        self.entries.insert(path, id);
    }
}

#[derive(Clone)]
struct FastImportPathSnapshot {
    kind: FastImportCopyKind,
    entries: Vec<IndexEntry>,
    empty_trees: Vec<(Vec<u8>, ObjectId)>,
    root_empty_tree_id: Option<ObjectId>,
}

impl FastImportPathSnapshot {
    fn from_candidate(
        index: &GitIndex,
        empty_trees: &FastImportEmptyTreeState,
        source: &[u8],
    ) -> Option<Self> {
        if source.is_empty() {
            return Some(Self {
                kind: FastImportCopyKind::Tree,
                entries: index.entries().to_vec(),
                empty_trees: empty_trees
                    .entries
                    .iter()
                    .map(|(path, id)| (path.clone(), id.clone()))
                    .collect(),
                root_empty_tree_id: empty_trees.root_empty_tree_id.clone(),
            });
        }

        if let Some(entry) = index.entry(source, 0) {
            let kind = match entry.mode {
                IndexMode::Tree => FastImportCopyKind::Tree,
                mode => FastImportCopyKind::File(mode),
            };
            let mut entry = entry.clone();
            entry.path.clear();
            return Some(Self {
                kind,
                entries: vec![entry],
                empty_trees: Vec::new(),
                root_empty_tree_id: None,
            });
        }
        if let Some(id) = empty_trees.entries.get(source) {
            return Some(Self {
                kind: FastImportCopyKind::Tree,
                entries: Vec::new(),
                empty_trees: vec![(Vec::new(), id.clone())],
                root_empty_tree_id: None,
            });
        }

        let mut prefix = source.to_vec();
        prefix.push(b'/');
        let mut entries = Vec::new();
        for entry in index.entries() {
            let Some(relative) = entry.path.strip_prefix(prefix.as_slice()) else {
                continue;
            };
            let mut entry = entry.clone();
            entry.path = relative.to_vec();
            entries.push(entry);
        }
        let empty_trees = empty_trees
            .entries
            .iter()
            .filter_map(|(path, id)| {
                path.strip_prefix(prefix.as_slice())
                    .map(|relative| (relative.to_vec(), id.clone()))
            })
            .collect::<Vec<_>>();
        if entries.is_empty() && empty_trees.is_empty() {
            None
        } else {
            Some(Self {
                kind: FastImportCopyKind::Tree,
                entries,
                empty_trees,
                root_empty_tree_id: None,
            })
        }
    }

    fn destination_mode(&self) -> IndexMode {
        match self.kind {
            FastImportCopyKind::File(mode) => mode,
            FastImportCopyKind::Tree => IndexMode::Tree,
        }
    }

    fn is_tree(&self) -> bool {
        matches!(self.kind, FastImportCopyKind::Tree)
    }

    fn entries_at(&self, destination: &[u8]) -> Vec<IndexEntry> {
        self.entries
            .iter()
            .cloned()
            .map(|mut entry| {
                if !entry.path.is_empty() {
                    let mut path = Vec::with_capacity(
                        destination
                            .len()
                            .saturating_add(1)
                            .saturating_add(entry.path.len()),
                    );
                    path.extend_from_slice(destination);
                    if !destination.is_empty() {
                        path.push(b'/');
                    }
                    path.extend_from_slice(&entry.path);
                    entry.path = path;
                } else {
                    entry.path = destination.to_vec();
                }
                entry
            })
            .collect()
    }

    fn empty_trees_at(&self, destination: &[u8]) -> Vec<(Vec<u8>, ObjectId)> {
        let mut paths = self
            .empty_trees
            .iter()
            .filter_map(|(relative, id)| {
                if relative.is_empty() && destination.is_empty() {
                    return None;
                }
                let mut path = destination.to_vec();
                if !relative.is_empty() {
                    if !path.is_empty() {
                        path.push(b'/');
                    }
                    path.extend_from_slice(relative);
                }
                Some((path, id.clone()))
            })
            .collect::<Vec<_>>();
        if !destination.is_empty() {
            if let Some(id) = &self.root_empty_tree_id {
                paths.push((destination.to_vec(), id.clone()));
            }
        }
        paths
    }
}

fn parse_fast_import_ls_request(
    value: &[u8],
    algorithm: GitHashAlgorithm,
) -> io::Result<FastImportLsRequest> {
    if value.is_empty() || value.starts_with(b"\"") {
        return Ok(FastImportLsRequest {
            dataref: None,
            path: parse_path_eol(value)?,
        });
    }
    let Some(_separator) = value.iter().position(|byte| *byte == b' ') else {
        if value.starts_with(b":") || value.len() == 40 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing space after tree-ish",
            ));
        }
        return Ok(FastImportLsRequest {
            dataref: None,
            path: parse_path_eol(value)?,
        });
    };
    let (raw_dataref_path, remainder) = parse_path_space(value)?;
    let raw_dataref = raw_dataref_path.as_bytes();
    let dataref = if let Some(mark) = raw_dataref.strip_prefix(b":") {
        if mark.is_empty() || !mark.iter().all(u8::is_ascii_digit) {
            let message = if mark.first().is_some_and(u8::is_ascii_digit)
                && mark.iter().any(|byte| !byte.is_ascii_digit())
            {
                "missing space after mark"
            } else {
                "invalid mark"
            };
            return Err(io::Error::new(io::ErrorKind::InvalidInput, message));
        }
        let mark = std::str::from_utf8(mark)
            .ok()
            .and_then(|mark| mark.parse::<usize>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid mark"))?;
        FastImportLsDataRef::Mark(mark)
    } else {
        let oid = std::str::from_utf8(raw_dataref)
            .ok()
            .and_then(|value| ObjectId::from_hex(algorithm, value).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid dataref"))?;
        FastImportLsDataRef::Object(oid)
    };
    Ok(FastImportLsRequest {
        dataref: Some(dataref),
        path: parse_path_eol(remainder)?,
    })
}

struct FastImportInput<R> {
    reader: R,
}

struct FastImportDataBuffer {
    content: Vec<u8>,
    max_bytes: usize,
}

impl FastImportDataBuffer {
    fn new(max_bytes: usize) -> Self {
        Self {
            content: Vec::new(),
            max_bytes,
        }
    }

    fn with_declared_length(length: usize, max_bytes: usize) -> io::Result<Self> {
        if length > max_bytes {
            return Err(fast_import_data_limit_error());
        }
        Ok(Self {
            content: Vec::with_capacity(length),
            max_bytes,
        })
    }

    fn extend(&mut self, bytes: &[u8]) -> io::Result<()> {
        let remaining = self.max_bytes.saturating_sub(self.content.len());
        if bytes.len() > remaining {
            return Err(fast_import_data_limit_error());
        }
        self.content.extend_from_slice(bytes);
        Ok(())
    }

    fn push_line(&mut self, line: &[u8]) -> io::Result<()> {
        let remaining = self.max_bytes.saturating_sub(self.content.len());
        let line_size = line
            .len()
            .checked_add(1)
            .ok_or_else(fast_import_data_limit_error)?;
        if line_size > remaining {
            return Err(fast_import_data_limit_error());
        }
        self.content.extend_from_slice(line);
        self.content.push(b'\n');
        Ok(())
    }

    fn into_inner(self) -> Vec<u8> {
        self.content
    }
}

impl<R: BufRead> FastImportInput<R> {
    fn new(reader: R) -> Self {
        Self { reader }
    }

    fn next_line_bytes_bounded(&mut self, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        self.next_line_bytes_bounded_with_error(max_bytes, fast_import_data_limit_error)
    }

    fn next_control_line_bytes_bounded(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.next_line_bytes_bounded_with_error(
            FAST_IMPORT_CONTROL_LINE_BYTES,
            fast_import_control_line_limit_error,
        )
    }

    fn next_line_bytes_bounded_with_error(
        &mut self,
        max_bytes: usize,
        limit_error: fn() -> io::Error,
    ) -> io::Result<Option<Vec<u8>>> {
        let line_limit = max_bytes.checked_add(2).ok_or_else(limit_error)?;
        let mut line = Vec::new();
        loop {
            let chunk = self.reader.fill_buf()?;
            if chunk.is_empty() {
                if line.is_empty() {
                    return Ok(None);
                }
                if line.len() > max_bytes {
                    return Err(limit_error());
                }
                return Ok(Some(line));
            }
            let newline = chunk.iter().position(|byte| *byte == b'\n');
            let take = newline.map_or(chunk.len(), |position| position + 1);
            let new_length = line.len().checked_add(take).ok_or_else(limit_error)?;
            if new_length > line_limit {
                return Err(limit_error());
            }
            if line.capacity() < new_length {
                line.reserve_exact(new_length - line.len());
            }
            line.extend_from_slice(&chunk[..take]);
            self.reader.consume(take);
            if newline.is_some() {
                if line.last() == Some(&b'\n') {
                    line.pop();
                }
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.len() > max_bytes {
                    return Err(limit_error());
                }
                return Ok(Some(line));
            }
        }
    }

    fn read_exact_bytes(&mut self, length: usize, max_bytes: usize) -> io::Result<Vec<u8>> {
        let mut content = FastImportDataBuffer::with_declared_length(length, max_bytes)?;
        let mut remaining = length;
        while remaining > 0 {
            let chunk = self.reader.fill_buf()?;
            if chunk.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "fast-import data ended before declared length",
                ));
            }
            let take = remaining.min(chunk.len());
            content.extend(&chunk[..take])?;
            self.reader.consume(take);
            remaining -= take;
        }
        Ok(content.into_inner())
    }

    fn copy_exact_to(&mut self, length: usize, writer: &mut dyn Write) -> io::Result<()> {
        let mut remaining = length;
        while remaining > 0 {
            let chunk = self.reader.fill_buf()?;
            if chunk.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "fast-import data ended before declared length",
                ));
            }
            let take = remaining.min(chunk.len());
            writer.write_all(&chunk[..take])?;
            self.reader.consume(take);
            remaining -= take;
        }
        Ok(())
    }

    fn consume_optional_line_feed(&mut self) -> io::Result<()> {
        if self.reader.fill_buf()?.first() == Some(&b'\n') {
            self.reader.consume(1);
        }
        Ok(())
    }
}

#[derive(Default)]
struct FastImportStats {
    blobs: usize,
    trees: usize,
    commits: usize,
    tags: usize,
    branches: BTreeSet<String>,
    atoms: BTreeSet<Vec<u8>>,
}

impl<'a, R: BufRead, W: Write> FastImportParser<'a, R, W> {
    fn new(
        input: R,
        response: W,
        repo: &'a GitRepo,
        store: &'a FastImportObjectStore<'a>,
        refs: &'a RefStore,
        date_format: FastImportDateFormat,
        options: FastImportOptions,
    ) -> Self {
        let relative_marks_enabled = options.relative_marks_enabled;
        Self {
            input: FastImportInput::new(input),
            output: FastImportOutput::new(response, options.cat_blob_fd.as_deref()),
            date_format,
            repo,
            store,
            commit_cache: CommitObjectCache::new(store),
            tree_cache: TreeObjectCache::new(store),
            refs,
            marks: HashMap::new(),
            ref_indexes: HashMap::new(),
            ref_empty_tree_states: HashMap::new(),
            empty_tree_id: None,
            ref_tips: HashMap::new(),
            reset_to_empty_refs: FastImportResetToEmptyRefs::default(),
            branch_state: FastImportBranchLru::default(),
            commit_clock: 0,
            pending_ref_updates: Vec::new(),
            pack_state: Rc::clone(&store.pack_state),
            pending_line: None,
            stats: FastImportStats::default(),
            command_history: FastImportCommandHistory::default(),
            options,
            stream_import_marks_seen: false,
            relative_marks_enabled,
            exported_pack_count: 0,
        }
    }

    fn parse(&mut self) -> Result<()> {
        self.preload_marks()?;
        let mut saw_done = false;
        while let Some(raw_line) = self.next_control_line_bytes()? {
            if raw_line.is_empty() {
                continue;
            }
            if raw_line.starts_with(b"ls ") {
                self.parse_ls(&raw_line[3..], None)?;
                continue;
            }
            if raw_line.starts_with(b"option cat-blob-fd=") {
                continue;
            }
            let line = String::from_utf8(raw_line).map_err(|_| fast_import_parse_error())?;
            if line.starts_with("feature ") {
                self.apply_feature_command(&line)?;
            } else if line == "blob" {
                self.parse_blob()?;
            } else if let Some(value) = line.strip_prefix("cat-blob ") {
                self.parse_cat_blob(value)?;
            } else if let Some(ref_name) = line.strip_prefix("commit ") {
                self.parse_commit(ref_name)?;
            } else if let Some(ref_name) = line.strip_prefix("reset ") {
                self.parse_reset(ref_name)?;
            } else if let Some(tag_name) = line.strip_prefix("tag ") {
                self.parse_tag(tag_name)?;
            } else if line == "checkpoint" {
                self.finalize_pack()?;
                self.publish_pending_refs()?;
                self.export_pack_edges()?;
                continue;
            } else if line == "done" {
                saw_done = true;
                self.finalize_pack()?;
                self.publish_pending_refs()?;
                self.export_pack_edges()?;
                break;
            } else if let Some(mark) = line.strip_prefix("get-mark ") {
                self.write_mark_response(mark)?;
            } else if line.starts_with("progress ") {
                self.output.write_progress(line.as_bytes())?;
                self.output.write_progress(b"\n")?;
                self.output.flush_stdout()?;
            } else {
                return Err(self.unsupported_fast_import_command(&line)?);
            }
        }
        if self.options.done && !saw_done {
            return Err(self.crash_error("stream ends early".to_owned())?);
        }
        self.finalize_pack()?;
        self.publish_pending_refs()?;
        self.export_marks()?;
        self.export_pack_edges()?;
        self.write_statistics()?;
        self.output.flush_response()?;
        self.output.flush_stdout()?;
        Ok(())
    }

    fn current_pack_edge_ids(&self) -> Vec<ObjectId> {
        let state = self.pack_state.borrow();
        let mut ids = self
            .ref_tips
            .values()
            .filter(|id| state.pending_contains(id))
            .cloned()
            .collect::<Vec<_>>();
        ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        ids.dedup();
        ids
    }

    fn current_pack_id(&self) -> Option<u32> {
        let pack_id = self.pack_state.borrow().published.len();
        u32::try_from(pack_id)
            .ok()
            .filter(|pack_id| *pack_id < FAST_IMPORT_MAX_PACK_ID)
    }

    fn finalize_pack(&mut self) -> Result<()> {
        let (published_before, had_pending) = {
            let state = self.pack_state.borrow();
            (state.published.len(), !state.pending.is_empty())
        };
        let edge_ids = self.current_pack_edge_ids();
        self.pack_state
            .borrow_mut()
            .finalize(self.store, &edge_ids)
            .map_err(CliError::Io)?;
        if had_pending
            && self.pack_state.borrow().published.len() == published_before
            && let Ok(pack_id) = u32::try_from(published_before)
            && pack_id < FAST_IMPORT_MAX_PACK_ID
        {
            self.branch_state.invalidate_pack(pack_id);
        }
        Ok(())
    }

    fn publish_pending_refs(&mut self) -> Result<()> {
        let updates = std::mem::take(&mut self.pending_ref_updates);
        for (index, update) in updates.iter().enumerate() {
            if let Err(error) = self.write_fast_import_ref(&update.name, &update.id) {
                self.pending_ref_updates = updates[index..].to_vec();
                return Err(error);
            }
        }
        Ok(())
    }

    fn fast_import_tree_index(&self, tree: &ObjectId) -> Result<GitIndex> {
        let index = self.tree_cache.read_tree_to_index(tree)?;
        if index.hash_algorithm() == self.store.algorithm() {
            return Ok(index);
        }
        if index.entries().is_empty() {
            return Ok(GitIndex::new_with_algorithm(self.store.algorithm()));
        }
        Err(CliError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "fast-import tree index object format does not match the repository",
        )))
    }

    fn fast_import_empty_trees_from_tree(
        &self,
        root: &ObjectId,
    ) -> Result<FastImportEmptyTreeState> {
        let mut state = FastImportEmptyTreeState::default();
        let root_entries = self.tree_cache.read_tree(root).map_err(CliError::Io)?;
        if root_entries.is_empty() {
            state.root_empty_tree_id = Some(root.clone());
            return Ok(state);
        }
        let mut stack = vec![(root.clone(), Vec::new())];
        while let Some((tree, prefix)) = stack.pop() {
            let entries = self.tree_cache.read_tree(&tree).map_err(CliError::Io)?;
            for entry in entries.iter() {
                if entry.mode != TreeMode::Tree {
                    continue;
                }
                let mut path = prefix.clone();
                if !path.is_empty() {
                    path.push(b'/');
                }
                path.extend_from_slice(&entry.name);
                let child_entries = self.tree_cache.read_tree(&entry.id).map_err(CliError::Io)?;
                if child_entries.is_empty() {
                    state.entries.insert(path, entry.id.clone());
                } else {
                    stack.push((entry.id.clone(), path));
                }
            }
        }
        Ok(state)
    }

    fn ensure_fast_import_empty_tree(&mut self) -> Result<ObjectId> {
        if let Some(id) = &self.empty_tree_id {
            return Ok(id.clone());
        }
        let (id, _) = self
            .store
            .write_object_with_newness(GitObjectKind::Tree, &[])
            .map_err(CliError::Io)?;
        self.empty_tree_id = Some(id.clone());
        Ok(id)
    }

    fn fast_import_empty_tree_identity(&self) -> ObjectId {
        hash_object(self.store.algorithm(), GitObjectKind::Tree, &[])
    }

    fn materialize_fast_import_empty_trees(
        &mut self,
        index: &GitIndex,
        empty_trees: &FastImportEmptyTreeState,
    ) -> Result<GitIndex> {
        if empty_trees.root_empty_tree_id.is_some() || !empty_trees.entries.is_empty() {
            let actual_id = self.ensure_fast_import_empty_tree()?;
            if let Some(expected_id) = &empty_trees.root_empty_tree_id
                && actual_id != *expected_id
            {
                return Err(CliError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "fast-import empty tree identity mismatch",
                )));
            }
        }
        let mut materialized = index.clone();
        for (path, id) in &empty_trees.entries {
            materialized.upsert(IndexEntry::new(
                path.clone(),
                id.clone(),
                IndexMode::Tree,
                0,
            )?)?;
        }
        Ok(materialized)
    }

    fn apply_feature_command(&mut self, line: &str) -> Result<()> {
        let Some(feature) = line.strip_prefix("feature ") else {
            return Err(self.unsupported_fast_import_command(line)?);
        };
        if feature == "no-relative-marks" {
            self.relative_marks_enabled = false;
            return Ok(());
        }
        if feature == "relative-marks" {
            self.relative_marks_enabled = true;
            return Ok(());
        }
        if feature == "cat-blob" {
            return Ok(());
        }
        if feature == "ls" {
            return Ok(());
        }
        if let Some(value) = feature.strip_prefix("relative-marks=") {
            return Err(self.crash_error(format!(
                "this version of fast-import does not support feature relative-marks={value}."
            ))?);
        }
        if let Some(path) = feature.strip_prefix("export-marks=") {
            self.ensure_unsafe_fast_import_feature(feature)?;
            self.options.export_marks = Some(resolve_fast_import_marks_path(
                path,
                self.relative_marks_enabled,
            ));
            return Ok(());
        }
        if let Some(path) = feature.strip_prefix("import-marks=") {
            self.ensure_unsafe_fast_import_feature("import-marks")?;
            self.apply_stream_import_marks_feature(path, false)?;
            return Ok(());
        }
        if let Some(path) = feature.strip_prefix("import-marks-if-exists=") {
            self.ensure_unsafe_fast_import_feature("import-marks-if-exists")?;
            self.apply_stream_import_marks_feature(path, true)?;
            return Ok(());
        }
        Err(self.unsupported_fast_import_command(line)?)
    }

    fn ensure_unsafe_fast_import_feature(&self, feature: &str) -> Result<()> {
        if self.options.allow_unsafe_features {
            return Ok(());
        }
        Err(self.crash_error(format!(
            "feature '{feature}' forbidden in input without --allow-unsafe-features"
        ))?)
    }

    fn apply_stream_import_marks_feature(&mut self, path: &str, missing_ok: bool) -> Result<()> {
        if self.stream_import_marks_seen {
            return Err(
                self.crash_error("only one import-marks command allowed per stream".to_owned())?
            );
        }
        self.stream_import_marks_seen = true;
        if !self.options.import_marks.is_empty() || !self.options.import_marks_if_exists.is_empty()
        {
            return Ok(());
        }
        let path = resolve_fast_import_marks_path(path, self.relative_marks_enabled);
        if missing_ok {
            if path.exists() {
                self.load_marks_file(&path)?;
            }
        } else {
            self.load_marks_file(&path)?;
        }
        Ok(())
    }

    fn preload_marks(&mut self) -> Result<()> {
        for path in self.options.import_marks.clone() {
            self.load_marks_file(&path)?;
        }
        for path in self.options.import_marks_if_exists.clone() {
            if path.exists() {
                self.load_marks_file(&path)?;
            }
        }
        Ok(())
    }

    fn load_marks_file(&mut self, path: &Path) -> Result<()> {
        for line in fs::read_to_string(path)?.lines() {
            let Some((mark, oid)) = line.split_once(' ') else {
                continue;
            };
            let Some(mark) = mark.strip_prefix(':') else {
                continue;
            };
            let mark = mark
                .parse::<usize>()
                .map_err(|_| fast_import_parse_error())?;
            let id =
                ObjectId::from_hex(self.store.algorithm(), oid.trim()).map_err(CliError::Io)?;
            self.marks.insert(mark, id);
        }
        Ok(())
    }

    fn export_marks(&self) -> Result<()> {
        let Some(path) = self.options.export_marks.as_deref() else {
            return Ok(());
        };
        let mut marks = self
            .marks
            .iter()
            .map(|(mark, id)| (*mark, id.to_hex()))
            .collect::<Vec<_>>();
        marks.sort_by_key(|(mark, _)| *mark);
        let mut out = Vec::new();
        for (mark, id) in marks {
            writeln!(&mut out, ":{mark} {id}")?;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, out)?;
        Ok(())
    }

    fn export_pack_edges(&mut self) -> Result<()> {
        let Some(path) = self.options.export_pack_edges.as_deref() else {
            return Ok(());
        };
        let mut file = open_fast_import_edges_file(path)?;
        let original_len = file.metadata()?.len();
        let state = self.pack_state.borrow();
        let result = (|| -> io::Result<()> {
            for published in state.published.iter().skip(self.exported_pack_count) {
                let name = published.pack_path.file_name().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "fast-import pack has no filename",
                    )
                })?;
                write!(file, "{}:", state.edge_pack_dir.join(name).display())?;
                for id in &published.edge_ids {
                    write!(file, " {}", id.to_hex())?;
                }
                file.write_all(b"\n")?;
                file.flush()?;
            }
            file.sync_all()
        })();
        let published_count = state.published.len();
        drop(state);
        if let Err(primary) = result {
            let rollback = file
                .set_len(original_len)
                .and_then(|()| file.seek(SeekFrom::Start(original_len)).map(|_| ()))
                .and_then(|()| file.sync_all());
            return match rollback {
                Ok(()) => Err(CliError::Io(primary)),
                Err(error) => Err(CliError::Io(combine_fast_import_cleanup_error(
                    primary, error,
                ))),
            };
        }
        self.exported_pack_count = published_count;
        Ok(())
    }

    fn parse_blob(&mut self) -> Result<()> {
        let mark = self.expect_mark()?;
        let id = self.write_blob_data()?;
        self.marks.insert(mark, id);
        self.stats.blobs += 1;
        Ok(())
    }

    fn write_blob_data(&mut self) -> Result<ObjectId> {
        let line = self.next_required_line()?;
        if let Some(delimiter) = line.strip_prefix("data <<") {
            let content = self.read_delimited_data(delimiter)?;
            return Ok(self.store.write_object(GitObjectKind::Blob, &content)?);
        }
        let Some(length) = line.strip_prefix("data ") else {
            return Err(fast_import_parse_error());
        };
        let length = length
            .parse::<usize>()
            .map_err(|_| fast_import_parse_error())?;
        let input = &mut self.input;
        let id = self
            .store
            .write_streamed_blob_content(length, |writer| input.copy_exact_to(length, writer))?;
        self.input.consume_optional_line_feed()?;
        Ok(id)
    }

    fn parse_cat_blob(&mut self, value: &str) -> Result<()> {
        let id = self.resolve_cat_blob_value(value)?;
        let object = match self.store.read_object(&id) {
            Ok(object) if object.kind == GitObjectKind::Blob => object,
            Ok(object) => {
                return Err(self.crash_error(format!(
                    "object {} is a {} but a blob was expected.",
                    id.to_hex(),
                    object.kind.as_str()
                ))?);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut response = id.to_hex().into_bytes();
                response.extend_from_slice(b" missing\n");
                self.write_response(&response)?;
                return Ok(());
            }
            Err(error) => return Err(CliError::Io(error)),
        };
        let mut response = format!("{} blob {}\n", id.to_hex(), object.content.len()).into_bytes();
        response.extend_from_slice(&object.content);
        response.push(b'\n');
        self.write_response(&response)?;
        Ok(())
    }

    fn resolve_cat_blob_value(&self, value: &str) -> Result<ObjectId> {
        if let Some(mark) = value.strip_prefix(':') {
            let mark = match mark.parse::<usize>() {
                Ok(mark) => mark,
                Err(_) => {
                    return Err(self.crash_error(format!("garbage after mark: cat-blob {value}"))?);
                }
            };
            return match self.marks.get(&mark).cloned() {
                Some(id) => Ok(id),
                None => Err(self.crash_error(format!("mark {value} not declared"))?),
            };
        }
        match ObjectId::from_hex(self.store.algorithm(), value) {
            Ok(id) => Ok(id),
            Err(_) => Err(self.crash_error(format!("invalid dataref: cat-blob {value}"))?),
        }
    }

    fn write_mark_response(&mut self, value: &str) -> Result<()> {
        let Some(mark) = value.strip_prefix(':') else {
            return Err(self.crash_error(format!("not a mark: {value}"))?);
        };
        let mark = match mark.parse::<usize>() {
            Ok(mark) => mark,
            Err(_) => {
                return Err(self.crash_error(format!("garbage after mark: get-mark {value}"))?);
            }
        };
        let id = match self.marks.get(&mark) {
            Some(id) => id,
            None => return Err(self.crash_error(format!("mark {value} not declared"))?),
        };
        let mut response = id.to_hex().into_bytes();
        response.push(b'\n');
        self.write_response(&response)?;
        Ok(())
    }

    fn parse_commit(&mut self, ref_name: &str) -> Result<()> {
        let null_id = fast_import_zero_object_id(self.store.algorithm());
        self.branch_state.register(ref_name, &null_id);
        let mark = {
            let result = self.next_optional_mark();
            self.crash_commit_result(result)?
        };
        let (author, committer) = {
            let result = self.expect_commit_signatures();
            self.crash_commit_result(result)?
        };
        let message = {
            let result = self.expect_data();
            self.crash_commit_result(result)?
        };
        let mut parent = if self.reset_to_empty_refs.contains(ref_name) {
            None
        } else {
            self.ref_tips
                .get(ref_name)
                .cloned()
                .or_else(|| self.refs.resolve(ref_name).ok())
        };
        let mut old_tree = None;
        let prior_root_empty_tree_id = self
            .ref_empty_tree_states
            .remove(ref_name)
            .and_then(|state| state.root_empty_tree_id);
        let index = if let Some(mut index) = self.ref_indexes.remove(ref_name) {
            let mut empty_trees = FastImportEmptyTreeState::from_index(&mut index);
            empty_trees.root_empty_tree_id = prior_root_empty_tree_id;
            if let Some(parent_id) = parent.as_ref() {
                old_tree = Some(self.commit_cache.read_commit(parent_id)?.tree.clone());
            }
            (index, empty_trees)
        } else if let Some(parent_id) = parent.as_ref() {
            let parent_commit = self.commit_cache.read_commit(parent_id)?;
            old_tree = Some(parent_commit.tree.clone());
            (
                self.fast_import_tree_index(&parent_commit.tree)?,
                self.fast_import_empty_trees_from_tree(&parent_commit.tree)?,
            )
        } else {
            (
                GitIndex::new_with_algorithm(self.store.algorithm()),
                FastImportEmptyTreeState {
                    root_empty_tree_id: prior_root_empty_tree_id,
                    ..FastImportEmptyTreeState::default()
                },
            )
        };
        let (mut index, mut empty_trees) = index;
        if index.entries().is_empty()
            && empty_trees.entries.is_empty()
            && empty_trees.root_empty_tree_id.is_none()
        {
            empty_trees.root_empty_tree_id = Some(self.fast_import_empty_tree_identity());
        }
        let old_tree = old_tree.unwrap_or_else(|| null_id.clone());

        // Git registers the branch at commit dispatch, but does not load it
        // into the active LRU until the metadata and `from` parent have been
        // accepted.  That distinction is visible in early crash reports.
        let first_line = {
            let result = self.next_control_line_bytes();
            self.crash_commit_result(result)?
        };
        if let Some(raw_line) = first_line {
            if raw_line.starts_with(b"from ") {
                let line = String::from_utf8(raw_line).map_err(|_| fast_import_parse_error())?;
                let parent_id = {
                    let result = self.resolve_fast_import_commit_parent(&line[5..]);
                    self.crash_commit_result(result)?
                };
                let parent_commit = {
                    let result = self.commit_cache.read_commit(&parent_id);
                    self.crash_commit_result(result)?
                };
                let parent_tree = parent_commit.tree.clone();
                parent = Some(parent_id.clone());
                index = {
                    let result = self.fast_import_tree_index(&parent_tree);
                    self.crash_commit_result(result)?
                };
                empty_trees = self.fast_import_empty_trees_from_tree(&parent_tree)?;
                self.branch_state
                    .activate(ref_name, parent_id, parent_tree.clone(), parent_tree);
            } else {
                self.pending_line = Some(raw_line);
                let tip_commit = parent.clone().unwrap_or_else(|| null_id.clone());
                self.branch_state.activate(
                    ref_name,
                    tip_commit,
                    old_tree.clone(),
                    old_tree.clone(),
                );
            }
        } else {
            let tip_commit = parent.clone().unwrap_or_else(|| null_id.clone());
            self.branch_state
                .activate(ref_name, tip_commit, old_tree.clone(), old_tree.clone());
        }

        loop {
            let next_line = {
                let result = self.next_control_line_bytes();
                self.crash_commit_result(result)?
            };
            let Some(raw_line) = next_line else {
                break;
            };
            if raw_line.is_empty() {
                break;
            }
            if raw_line.starts_with(b"ls ") {
                let result = self.parse_ls(&raw_line[3..], Some(&index));
                self.crash_commit_result(result)?;
                continue;
            }
            if raw_line.starts_with(b"D ") {
                let result =
                    self.apply_fast_import_delete(&mut index, &mut empty_trees, &raw_line[2..]);
                self.crash_commit_apply_result(result)?;
                self.branch_state.mark_dirty(ref_name);
                continue;
            }
            if raw_line.starts_with(b"M ") {
                let result =
                    self.apply_fast_import_modify(&mut index, &mut empty_trees, &raw_line[2..]);
                self.crash_commit_apply_result(result)?;
                self.branch_state.mark_dirty(ref_name);
                continue;
            }
            if raw_line.starts_with(b"C ") || raw_line.starts_with(b"R ") {
                let rename = raw_line[0] == b'R';
                let result = self.apply_fast_import_copy_or_rename(
                    &mut index,
                    &mut empty_trees,
                    &raw_line[2..],
                    rename,
                    ref_name,
                );
                self.crash_commit_apply_result(result)?;
                self.branch_state.mark_dirty(ref_name);
                continue;
            }
            if raw_line.starts_with(b"option cat-blob-fd=") {
                self.pending_line = Some(raw_line);
                break;
            }
            let line = String::from_utf8(raw_line).map_err(|_| fast_import_parse_error())?;
            if let Some(value) = line.strip_prefix("from ") {
                let parent_id = {
                    let result = self.resolve_fast_import_commit_parent(value);
                    self.crash_commit_result(result)?
                };
                parent = Some(parent_id.clone());
                let parent_commit = {
                    let result = self.commit_cache.read_commit(&parent_id);
                    self.crash_commit_result(result)?
                };
                let parent_tree = parent_commit.tree.clone();
                index = {
                    let result = self.fast_import_tree_index(&parent_tree);
                    self.crash_commit_result(result)?
                };
                empty_trees = self.fast_import_empty_trees_from_tree(&parent_tree)?;
                self.branch_state
                    .activate(ref_name, parent_id, parent_tree.clone(), parent_tree);
            } else if line == "deleteall" {
                index = GitIndex::new_with_algorithm(self.store.algorithm());
                empty_trees.entries.clear();
                empty_trees.root_empty_tree_id = Some(self.ensure_fast_import_empty_tree()?);
                self.branch_state.mark_dirty(ref_name);
            } else if is_fast_import_top_level_command(&line) {
                self.pending_line = Some(line.into_bytes());
                break;
            } else {
                let materialized_index =
                    self.materialize_fast_import_empty_trees(&index, &empty_trees)?;
                let unreferenced = self.write_fast_import_unreferenced_commit(
                    &materialized_index,
                    author.clone(),
                    committer.clone(),
                    message.clone(),
                    parent.clone(),
                )?;
                if unreferenced.newly_written {
                    self.commit_clock = self.commit_clock.saturating_add(1);
                }
                self.branch_state.record_commit(
                    ref_name,
                    unreferenced.id,
                    unreferenced.tree.clone(),
                    unreferenced.tree,
                    self.commit_clock,
                    self.current_pack_id(),
                    unreferenced.newly_written,
                );
                return Err(self.unsupported_fast_import_command(&line)?);
            }
        }

        if index.entries().is_empty()
            && empty_trees.entries.is_empty()
            && empty_trees.root_empty_tree_id.is_none()
        {
            empty_trees.root_empty_tree_id = Some(self.ensure_fast_import_empty_tree()?);
        }
        let materialized_index = self.materialize_fast_import_empty_trees(&index, &empty_trees)?;
        let tree = {
            let result = write_tree_from_index(self.store, &materialized_index);
            self.crash_commit_result(result)?
        };
        self.stats.trees += fast_import_tree_count(&materialized_index);
        self.stats.commits += 1;
        self.stats.branches.insert(ref_name.to_owned());
        fast_import_record_path_atoms(&materialized_index, &mut self.stats.atoms);
        let current_tree = tree.clone();
        let mut builder = CommitBuilder::new(tree, author, committer);
        if let Some(parent) = parent {
            builder = builder.parent(parent);
        }
        let encoded = {
            let result = builder
                .message(message)
                .and_then(|builder| builder.encode());
            self.crash_commit_result(result)?
        };
        let (id, newly_written) = {
            let result = self
                .store
                .write_object_with_newness(GitObjectKind::Commit, &encoded);
            self.crash_commit_result(result)?
        };
        self.stage_fast_import_ref(ref_name, &id);
        if let Some(mark) = mark {
            self.marks.insert(mark, id.clone());
        }
        self.ref_tips.insert(ref_name.to_owned(), id.clone());
        self.ref_indexes
            .insert(ref_name.to_owned(), materialized_index);
        self.ref_empty_tree_states
            .insert(ref_name.to_owned(), empty_trees);
        self.reset_to_empty_refs.remove(ref_name);
        if newly_written {
            self.commit_clock = self.commit_clock.saturating_add(1);
        }
        self.branch_state.record_commit(
            ref_name,
            id,
            current_tree.clone(),
            current_tree,
            self.commit_clock,
            self.current_pack_id(),
            newly_written,
        );
        Ok(())
    }

    fn parse_tag(&mut self, tag_name: &str) -> Result<()> {
        let mark = self.next_optional_mark()?;
        let from = self.next_required_line()?;
        let target = from
            .strip_prefix("from ")
            .ok_or_else(fast_import_parse_error)
            .and_then(|value| self.resolve_fast_import_value(value))?;
        let target_kind = self.store.read_object(&target)?.kind;
        let mut line = self.next_required_line()?;
        if line.starts_with("original-oid ") {
            line = self.next_required_line()?;
        }
        let tagger = line
            .strip_prefix("tagger ")
            .map(|value| self.parse_signature(value))
            .transpose()?
            .ok_or_else(fast_import_parse_error)?;
        let message = self.expect_data()?;
        let encoded = TagBuilder::new(target, target_kind, tag_name, tagger)?
            .message(message)?
            .encode()?;
        let id = self.store.write_object(GitObjectKind::Tag, &encoded)?;
        let ref_name = format!("refs/tags/{tag_name}");
        self.stage_fast_import_ref(&ref_name, &id);
        if let Some(mark) = mark {
            self.marks.insert(mark, id.clone());
        }
        self.ref_tips.insert(ref_name, id);
        self.stats.tags += 1;
        Ok(())
    }

    fn expect_commit_signatures(&mut self) -> Result<(Signature, Signature)> {
        let line = self.next_required_line()?;
        if let Some(raw) = line.strip_prefix("author ") {
            let author = self.parse_signature(raw)?;
            let committer = self.expect_signature("committer ")?;
            return Ok((author, committer));
        }
        if let Some(raw) = line.strip_prefix("committer ") {
            let committer = self.parse_signature(raw)?;
            return Ok((committer.clone(), committer));
        }
        Err(fast_import_parse_error())
    }

    fn parse_reset(&mut self, ref_name: &str) -> Result<()> {
        let null_id = fast_import_zero_object_id(self.store.algorithm());
        self.branch_state.reset(ref_name, &null_id);
        self.ref_tips.remove(ref_name);
        self.ref_indexes.remove(ref_name);
        self.ref_empty_tree_states.remove(ref_name);
        self.pending_ref_updates
            .retain(|update| update.name != ref_name);
        self.reset_to_empty_refs.insert(ref_name);
        let line = {
            let result = self.next_control_line();
            self.crash_commit_result(result)?
        };
        let Some(line) = line else {
            return Ok(());
        };
        if line.is_empty() {
            return Ok(());
        }
        let Some(value) = line.strip_prefix("from ") else {
            self.pending_line = Some(line.into_bytes());
            return Ok(());
        };
        let id = {
            let result = self.resolve_fast_import_commit_parent(value);
            self.crash_commit_result(result)?
        };
        let commit = {
            let result = self.commit_cache.read_commit(&id);
            self.crash_commit_result(result)?
        };
        let index = {
            let result = self.fast_import_tree_index(&commit.tree);
            self.crash_commit_result(result)?
        };
        let empty_trees = self.fast_import_empty_trees_from_tree(&commit.tree)?;
        let index = self.materialize_fast_import_empty_trees(&index, &empty_trees)?;
        self.stage_fast_import_ref(ref_name, &id);
        self.branch_state
            .reset_from(ref_name, id.clone(), commit.tree.clone());
        self.ref_tips.insert(ref_name.to_owned(), id);
        self.ref_indexes.insert(ref_name.to_owned(), index);
        self.ref_empty_tree_states
            .insert(ref_name.to_owned(), empty_trees);
        self.reset_to_empty_refs.remove(ref_name);
        self.consume_optional_blank_line()?;
        Ok(())
    }

    fn replace_fast_import_tree(
        &self,
        index: &mut GitIndex,
        path: &[u8],
        replacement: GitIndex,
    ) -> Result<()> {
        if path.is_empty() {
            *index = replacement;
            return Ok(());
        }

        let mut candidate = index.clone();
        candidate.remove_fast_import_path(path);
        for mut entry in replacement.entries().iter().cloned() {
            let mut prefixed = Vec::with_capacity(path.len() + 1 + entry.path.len());
            prefixed.extend_from_slice(path);
            prefixed.push(b'/');
            prefixed.extend_from_slice(&entry.path);
            entry.path = prefixed;
            candidate.upsert(entry)?;
        }
        *index = candidate;
        Ok(())
    }

    fn apply_fast_import_modify(
        &mut self,
        index: &mut GitIndex,
        empty_trees: &mut FastImportEmptyTreeState,
        rest: &[u8],
    ) -> Result<()> {
        let mut cursor = FastImportPathCursor::new(rest);
        let mode_bytes = match cursor.take_space_field() {
            Ok(value) => value,
            Err(_) => {
                return self.crash_raw_fast_import_fatal(fast_import_corrupt_mode_fatal(rest));
            }
        };
        let value_bytes = match cursor.take_space_field() {
            Ok(value) => value,
            Err(_) => {
                return self.crash_raw_fast_import_fatal(fast_import_corrupt_mode_fatal(rest));
            }
        };
        let path = match cursor.parse_path(FastImportPathDelimiter::Eol) {
            Ok(path) => path,
            Err(error) => {
                return self.crash_raw_fast_import_fatal(fast_import_path_fatal(b"M", rest, error));
            }
        };
        let mode_text = match std::str::from_utf8(mode_bytes) {
            Ok(value) => value,
            Err(_) => {
                return self.crash_raw_fast_import_fatal(fast_import_corrupt_mode_fatal(rest));
            }
        };
        let value = match std::str::from_utf8(value_bytes) {
            Ok(value) => value,
            Err(_) => {
                return self.crash_raw_fast_import_fatal(fast_import_corrupt_mode_fatal(rest));
            }
        };
        let mode = match parse_fast_import_mode(mode_text) {
            Ok(mode) => mode,
            Err(_) => {
                return self.crash_raw_fast_import_fatal(fast_import_corrupt_mode_fatal(rest));
            }
        };
        if value == "inline" && mode == IndexMode::Tree {
            return self.crash_raw_fast_import_fatal(fast_import_semantic_fatal(
                b"directories cannot be specified 'inline': M ",
                rest,
            ));
        }
        if value == "inline" && mode == IndexMode::Gitlink {
            return self.crash_raw_fast_import_fatal(fast_import_semantic_fatal(
                b"Git links cannot be specified 'inline': M ",
                rest,
            ));
        }
        let id = if value == "inline" {
            let content = self.expect_data()?;
            let id = self.store.write_object(GitObjectKind::Blob, &content)?;
            self.stats.blobs += 1;
            id
        } else {
            self.resolve_fast_import_value(value)?
        };
        let path = path.into_bytes();
        if mode == IndexMode::Tree {
            let replacement = self.fast_import_tree_index(&id)?;
            let replacement_empty_trees = self.fast_import_empty_trees_from_tree(&id)?;
            if !path.is_empty() && replacement.entries().is_empty() {
                index.remove_fast_import_path(&path);
                empty_trees.remove_path(&path);
                empty_trees.remove_ancestor_trees(&path);
                return Ok(());
            }
            if !path.is_empty() && !fast_import_verify_path(&path, mode) {
                return self
                    .crash_raw_fast_import_fatal(fast_import_invalid_path_fatal(path.as_slice()));
            }
            self.replace_fast_import_tree(index, &path, replacement)?;
            empty_trees.remove_path(&path);
            empty_trees.remove_ancestor_trees(&path);
            empty_trees.root_empty_tree_id =
                if path.is_empty() && replacement_empty_trees.entries.is_empty() {
                    Some(id.clone())
                } else {
                    None
                };
            for (relative, tree_id) in replacement_empty_trees.entries {
                let mut empty_path = path.clone();
                if !empty_path.is_empty() {
                    empty_path.push(b'/');
                }
                empty_path.extend_from_slice(&relative);
                empty_trees.insert(empty_path, tree_id);
            }
            return Ok(());
        }
        if path.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "root cannot be a non-directory".into(),
            });
        }
        let size = if mode == IndexMode::Gitlink {
            if value.starts_with(':') {
                let object = self.store.read_object(&id)?;
                if object.kind != GitObjectKind::Commit {
                    return self.crash_raw_fast_import_fatal(fast_import_kind_fatal(
                        b"not a commit (actually a ",
                        object.kind.as_str().as_bytes(),
                        b"): M ",
                        rest,
                    ));
                }
            }
            0
        } else {
            let object = self.store.read_object(&id)?;
            if object.kind != GitObjectKind::Blob {
                return self.crash_raw_fast_import_fatal(fast_import_kind_fatal(
                    b"not a blob (actually a ",
                    object.kind.as_str().as_bytes(),
                    b"): M ",
                    rest,
                ));
            }
            object.content.len().min(u32::MAX as usize) as u32
        };
        if !fast_import_verify_path(&path, mode) {
            return self
                .crash_raw_fast_import_fatal(fast_import_invalid_path_fatal(path.as_slice()));
        }
        let entry = match IndexEntry::new(path.clone(), id, mode, size) {
            Ok(entry) => entry,
            Err(_) => {
                return self
                    .crash_raw_fast_import_fatal(fast_import_invalid_path_fatal(path.as_slice()));
            }
        };
        index.upsert(entry)?;
        empty_trees.remove_for_leaf(&path);
        empty_trees.root_empty_tree_id = None;
        Ok(())
    }

    fn apply_fast_import_delete(
        &mut self,
        index: &mut GitIndex,
        empty_trees: &mut FastImportEmptyTreeState,
        rest: &[u8],
    ) -> Result<()> {
        let path = match parse_path_eol(rest) {
            Ok(path) => path,
            Err(error) => {
                return self.crash_raw_fast_import_fatal(fast_import_path_fatal(b"D", rest, error));
            }
        };
        index.remove_fast_import_path(path.as_bytes());
        empty_trees.remove_path(path.as_bytes());
        if index.entries().is_empty() && empty_trees.entries.is_empty() {
            empty_trees.root_empty_tree_id = Some(self.ensure_fast_import_empty_tree()?);
        }
        Ok(())
    }

    fn apply_fast_import_copy_or_rename(
        &mut self,
        index: &mut GitIndex,
        empty_trees: &mut FastImportEmptyTreeState,
        rest: &[u8],
        rename: bool,
        ref_name: &str,
    ) -> Result<()> {
        let command = if rename {
            b"R".as_slice()
        } else {
            b"C".as_slice()
        };
        let (source, remainder) = match parse_path_space(rest) {
            Ok(value) => value,
            Err(error) => {
                return self.crash_raw_fast_import_fatal(fast_import_copy_path_fatal(
                    command, rest, error, "source",
                ));
            }
        };
        let destination = match parse_path_eol(remainder) {
            Ok(path) => path,
            Err(error) => {
                return self.crash_raw_fast_import_fatal(fast_import_copy_path_fatal(
                    command, rest, error, "dest",
                ));
            }
        };
        let source_path = fast_import_copy_source_lookup_path(source.as_bytes(), rename);
        if let Some(prefix) = fast_import_path_prefix_before_empty_component(source_path) {
            let prefix_is_tree = FastImportPathSnapshot::from_candidate(index, empty_trees, prefix)
                .is_some_and(|snapshot| snapshot.is_tree());
            if !rename && prefix_is_tree {
                return self
                    .crash_raw_fast_import_fatal(b"empty path component found in input".to_vec());
            }
            return self.crash_raw_fast_import_fatal(fast_import_source_not_in_branch_fatal(
                source.as_bytes(),
            ));
        }
        let Some(snapshot) =
            FastImportPathSnapshot::from_candidate(index, empty_trees, source_path)
        else {
            return self.crash_raw_fast_import_fatal(fast_import_source_not_in_branch_fatal(
                source.as_bytes(),
            ));
        };
        if rename && source_path != destination.as_bytes() {
            index.remove_fast_import_path(source_path);
            empty_trees.remove_path(source_path);
            self.branch_state.mark_dirty(ref_name);
        }
        if !snapshot.is_tree() && destination.as_bytes().is_empty() {
            return self.crash_raw_fast_import_fatal(b"root cannot be a non-directory".to_vec());
        }
        if !destination.as_bytes().is_empty() && destination.as_bytes().ends_with(b"/") {
            return self.crash_raw_fast_import_fatal(fast_import_invalid_path_fatal(
                destination.as_bytes(),
            ));
        }
        if !destination.as_bytes().is_empty()
            && !fast_import_verify_path(destination.as_bytes(), snapshot.destination_mode())
        {
            return self.crash_raw_fast_import_fatal(fast_import_invalid_path_fatal(
                destination.as_bytes(),
            ));
        }
        if rename && source_path == destination.as_bytes() {
            return Ok(());
        }

        let mut candidate = index.clone();
        let mut candidate_empty_trees = empty_trees.clone();
        remove_fast_import_destination_conflicts(
            &mut candidate,
            &mut candidate_empty_trees,
            destination.as_bytes(),
        );
        for entry in snapshot.entries_at(destination.as_bytes()) {
            candidate.upsert(entry)?;
        }
        for (path, id) in snapshot.empty_trees_at(destination.as_bytes()) {
            candidate_empty_trees.insert(path, id);
        }
        candidate_empty_trees.root_empty_tree_id = if destination.as_bytes().is_empty() {
            snapshot.root_empty_tree_id.clone()
        } else {
            None
        };
        *index = candidate;
        *empty_trees = candidate_empty_trees;
        Ok(())
    }

    fn next_optional_mark(&mut self) -> Result<Option<usize>> {
        let line = self.next_required_line()?;
        if let Some(mark) = line.strip_prefix("mark :") {
            return Ok(Some(parse_fast_import_mark_prefix(mark)));
        }
        self.pending_line = Some(line.into_bytes());
        Ok(None)
    }

    fn expect_mark(&mut self) -> Result<usize> {
        let line = self.next_required_line()?;
        let Some(mark) = line.strip_prefix("mark :") else {
            return Err(fast_import_parse_error());
        };
        mark.parse::<usize>().map_err(|_| fast_import_parse_error())
    }

    fn expect_signature(&mut self, prefix: &str) -> Result<Signature> {
        let line = self.next_required_line()?;
        let Some(raw) = line.strip_prefix(prefix) else {
            return Err(fast_import_parse_error());
        };
        self.parse_signature(raw)
    }

    fn parse_signature(&self, raw: &str) -> Result<Signature> {
        match self.date_format {
            FastImportDateFormat::Raw => {
                if !raw.contains(" <") {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: format!("missing < in ident string: {raw}"),
                    });
                }
                return signature_from_commit_bytes(raw.as_bytes());
            }
            FastImportDateFormat::Rfc2822 => return parse_fast_import_rfc2822_signature(raw),
            FastImportDateFormat::Now => {}
        }
        let Some(name_email) = raw.strip_suffix(" now") else {
            return signature_from_commit_bytes(raw.as_bytes());
        };
        let (name, email) = name_email
            .rsplit_once(" <")
            .and_then(|(name, email)| email.strip_suffix('>').map(|email| (name, email)))
            .ok_or_else(fast_import_parse_error)?;
        Ok(Signature::new(
            name,
            email,
            current_unix_timestamp()?,
            "+0000",
        )?)
    }

    fn expect_data(&mut self) -> Result<Vec<u8>> {
        let line = self.next_control_line()?.ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "expected 'data n' command, found: ".into(),
        })?;
        if let Some(delimiter) = line.strip_prefix("data <<") {
            return self.read_delimited_data(delimiter);
        }
        let Some(len) = line.strip_prefix("data ") else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("expected 'data n' command, found: {line}"),
            });
        };
        let len = len
            .parse::<usize>()
            .map_err(|_| fast_import_parse_error())?;
        let content = self
            .input
            .read_exact_bytes(len, self.store.max_object_bytes())?;
        self.input.consume_optional_line_feed()?;
        Ok(content)
    }

    fn read_delimited_data(&mut self, delimiter: &str) -> Result<Vec<u8>> {
        let mut content = FastImportDataBuffer::new(self.store.max_object_bytes());
        loop {
            let Some(line) = self
                .input
                .next_line_bytes_bounded(self.store.max_object_bytes())?
            else {
                return Err(fast_import_parse_error());
            };
            if line == delimiter.as_bytes() {
                break;
            }
            content.push_line(&line)?;
        }
        Ok(content.into_inner())
    }

    fn resolve_fast_import_value(&self, value: &str) -> Result<ObjectId> {
        if let Some(mark) = value.strip_prefix(':') {
            let mark = mark
                .parse::<usize>()
                .map_err(|_| fast_import_parse_error())?;
            self.marks
                .get(&mark)
                .cloned()
                .ok_or_else(fast_import_parse_error)
        } else {
            let value = value.strip_suffix("^0").unwrap_or(value);
            if value == "HEAD" || value.starts_with("refs/") {
                return self
                    .refs
                    .resolve(value)
                    .map_err(|_| fast_import_parse_error());
            }
            ObjectId::from_hex(self.store.algorithm(), value).map_err(CliError::Io)
        }
    }

    fn resolve_fast_import_commit_parent(&self, value: &str) -> Result<ObjectId> {
        match self.resolve_fast_import_value(value) {
            Ok(id) => Ok(id),
            Err(_error) if value.starts_with(':') => Err(CliError::Fatal {
                code: 128,
                message: format!("mark {value} not declared"),
            }),
            Err(_error) if value.starts_with("refs/") => Err(CliError::Fatal {
                code: 128,
                message: format!("invalid ref name or SHA1 expression: {value}"),
            }),
            Err(error) => Err(error),
        }
    }

    fn write_fast_import_ref(&self, ref_name: &str, id: &ObjectId) -> Result<()> {
        let old_id = self
            .refs
            .resolve(ref_name)
            .unwrap_or_else(|_| fast_import_zero_object_id(self.store.algorithm()));
        if ref_name == "HEAD" {
            match self.refs.read_head()? {
                RefTarget::Symbolic(target) => self.refs.write_ref(&target, id)?,
                RefTarget::Direct(_) => self.refs.write_head_direct(id)?,
            }
        } else {
            self.refs.write_ref(ref_name, id)?;
        }
        if fast_import_should_write_reflog(self.repo, ref_name)? {
            append_reflog_if_identity_available(self.repo, ref_name, &old_id, id, "fast-import")?;
        }
        Ok(())
    }

    fn stage_fast_import_ref(&mut self, ref_name: &str, id: &ObjectId) {
        self.pending_ref_updates.push(FastImportRefUpdate {
            name: ref_name.to_owned(),
            id: id.clone(),
        });
    }

    fn write_fast_import_unreferenced_commit(
        &self,
        index: &GitIndex,
        author: Signature,
        committer: Signature,
        message: Vec<u8>,
        parent: Option<ObjectId>,
    ) -> Result<FastImportUnreferencedCommit> {
        let tree = write_tree_from_index(self.store, index)?;
        let mut builder = CommitBuilder::new(tree.clone(), author, committer);
        if let Some(parent) = parent {
            builder = builder.parent(parent);
        }
        let encoded = builder.message(message)?.encode()?;
        let (id, newly_written) = self
            .store
            .write_object_with_newness(GitObjectKind::Commit, &encoded)?;
        Ok(FastImportUnreferencedCommit {
            id,
            tree,
            newly_written,
        })
    }

    fn next_required_line(&mut self) -> Result<String> {
        self.next_control_line()?
            .ok_or_else(fast_import_parse_error)
    }

    fn next_control_line_bytes(&mut self) -> Result<Option<Vec<u8>>> {
        if self.pending_line.is_some() {
            return Ok(self.pending_line.take());
        }
        let line = match self.input.next_control_line_bytes_bounded() {
            Ok(line) => line,
            Err(error)
                if error.kind() == io::ErrorKind::InvalidData
                    && error.to_string() == FAST_IMPORT_CONTROL_LINE_LIMIT_MESSAGE =>
            {
                return Err(self.crash_error(FAST_IMPORT_CONTROL_LINE_LIMIT_MESSAGE.to_owned())?);
            }
            Err(error) => return Err(CliError::Io(error)),
        };
        let Some(line) = line else {
            return Ok(None);
        };
        if !line.is_empty() {
            self.command_history.record(&line);
        }
        Ok(Some(line))
    }

    fn next_control_line(&mut self) -> Result<Option<String>> {
        self.next_control_line_bytes()?
            .map(|line| String::from_utf8(line).map_err(|_| fast_import_parse_error()))
            .transpose()
    }

    fn write_response(&mut self, bytes: &[u8]) -> Result<()> {
        if let Err(error) = self.output.write_response(bytes) {
            let message = if error.kind() == io::ErrorKind::InvalidInput {
                error.to_string()
            } else {
                format!("write to frontend failed: {error}")
            };
            return Err(self.crash_error(message)?);
        }
        if let Err(error) = self.output.flush_response() {
            return Err(self.crash_error(format!("write to frontend failed: {error}"))?);
        }
        Ok(())
    }

    fn parse_ls(&mut self, value: &[u8], active_index: Option<&GitIndex>) -> Result<()> {
        if active_index.is_none() && !fast_import_ls_has_named_dataref(value) {
            return Err(self.crash_error(format!(
                "not in a commit: ls {}",
                String::from_utf8_lossy(value)
            ))?);
        }
        let request = match parse_fast_import_ls_request(value, self.store.algorithm()) {
            Ok(request) => request,
            Err(error) => {
                return Err(self.crash_error(format!(
                    "{}: ls {}",
                    error,
                    String::from_utf8_lossy(value)
                ))?);
            }
        };
        let tree_id = if let Some(dataref) = request.dataref.as_ref() {
            self.resolve_ls_dataref(dataref)?
        } else {
            let Some(index) = active_index else {
                return Err(self.crash_error("ls outside a commit".to_owned())?);
            };
            write_tree_from_index(self.store, index).map_err(CliError::Io)?
        };
        let path = request.path.into_bytes();
        let entry = if path.is_empty() {
            Some(FastImportLsEntry {
                mode: TreeMode::Tree,
                id: tree_id,
                path: path.clone(),
            })
        } else {
            find_tree_entry(self.store, &tree_id, &path)
                .map_err(CliError::Io)?
                .map(|entry| FastImportLsEntry {
                    mode: entry.mode,
                    id: entry.id,
                    path: path.clone(),
                })
        };
        let quote_non_ascii = read_config_value(self.repo, "core.quotePath")?
            .map(|value| value != "false")
            .unwrap_or(true);
        let quoted_path = reference_commands::quote_git_path(
            entry
                .as_ref()
                .map(|entry| entry.path.as_slice())
                .unwrap_or(path.as_slice()),
            quote_non_ascii,
        );
        let mut response = Vec::new();
        if let Some(entry) = entry {
            let mode = if entry.mode == TreeMode::Tree {
                b"040000".as_slice()
            } else {
                entry.mode.as_bytes()
            };
            response.extend_from_slice(mode);
            response.push(b' ');
            response.extend_from_slice(match entry.mode {
                TreeMode::Tree => b"tree",
                TreeMode::Gitlink => b"commit",
                TreeMode::File | TreeMode::Executable | TreeMode::Symlink => b"blob",
            });
            response.push(b' ');
            response.extend_from_slice(entry.id.to_hex().as_bytes());
            response.extend_from_slice(b"\t");
        } else {
            response.extend_from_slice(b"missing ");
        }
        response.extend_from_slice(&quoted_path);
        response.push(b'\n');
        self.write_response(&response)
    }

    fn resolve_ls_dataref(&self, dataref: &FastImportLsDataRef) -> Result<ObjectId> {
        let initial = match dataref {
            FastImportLsDataRef::Mark(mark) => {
                let Some(id) = self.marks.get(mark).cloned() else {
                    return Err(self.crash_error(format!("mark :{mark} not declared"))?);
                };
                id
            }
            FastImportLsDataRef::Object(id) => id.clone(),
        };
        let mut current = initial;
        for _ in 0..64 {
            let object = match self.store.read_object(&current) {
                Ok(object) => object,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(
                        self.crash_error(format!("object not found: {}", current.to_hex()))?
                    );
                }
                Err(error) => return Err(CliError::Io(error)),
            };
            match object.kind {
                GitObjectKind::Tree => return Ok(current),
                GitObjectKind::Commit => {
                    current = decode_commit(current.algorithm(), &object.content)
                        .map_err(CliError::Io)?
                        .tree;
                }
                GitObjectKind::Tag => {
                    current = decode_tag(current.algorithm(), &object.content)
                        .map_err(CliError::Io)?
                        .target;
                }
                _ => {
                    return Err(
                        self.crash_error(format!("object {} is not a tree-ish", current.to_hex()))?
                    );
                }
            }
        }
        Err(self.crash_error("tree-ish peel depth exceeded".to_owned())?)
    }

    fn crash_error(&self, fatal: String) -> Result<CliError> {
        let mut marks = self
            .marks
            .iter()
            .map(|(mark, id)| (*mark, id.to_hex()))
            .collect::<Vec<_>>();
        marks.sort_by_key(|(mark, _)| *mark);
        let state = FastImportCrashReportState {
            recent_commands: self.command_history.snapshot(),
            marks,
            branch_state: self.branch_state.clone(),
        };
        fast_import_crash_error_with_state(&self.repo.git_dir, fatal, &state)
    }

    fn crash_raw_fast_import_fatal(&self, fatal: Vec<u8>) -> Result<()> {
        let mut marks = self
            .marks
            .iter()
            .map(|(mark, id)| (*mark, id.to_hex()))
            .collect::<Vec<_>>();
        marks.sort_by_key(|(mark, _)| *mark);
        let state = FastImportCrashReportState {
            recent_commands: self.command_history.snapshot(),
            marks,
            branch_state: self.branch_state.clone(),
        };
        Err(fast_import_crash_error_with_state_bytes(
            &self.repo.git_dir,
            &fatal,
            &state,
        )?)
    }

    fn crash_commit_apply_result<T>(&self, result: Result<T>) -> Result<T> {
        if matches!(&result, Err(CliError::Exit(128))) {
            return result;
        }
        self.crash_commit_result(result)
    }

    fn crash_commit_result<T, E>(&self, result: std::result::Result<T, E>) -> Result<T>
    where
        E: Into<CliError>,
    {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let error = error.into();
                let already_reported = matches!(
                    &error,
                    CliError::Fatal { message, .. }
                        if message.contains("\nfast-import: dumping crash report to ")
                );
                if already_reported {
                    return Err(error);
                }
                let fatal = match &error {
                    CliError::Exit(code) => format!("exit status {code}"),
                    CliError::Fatal { message, .. } => message.clone(),
                    CliError::Stderr { text, .. } => text.clone(),
                    CliError::Message(message) => message.clone(),
                    CliError::Io(error) => error.to_string(),
                };
                Err(self.crash_error(fatal)?)
            }
        }
    }

    fn consume_optional_blank_line(&mut self) -> Result<()> {
        self.input.consume_optional_line_feed()?;
        Ok(())
    }

    fn unsupported_fast_import_command(&self, line: &str) -> Result<CliError> {
        self.crash_error(format!("unsupported command: {line}"))
    }

    fn write_statistics(&self) -> Result<()> {
        if self.options.quiet && !self.options.stats {
            return Ok(());
        }
        let mut err = io::stderr().lock();
        if let Some(value) = self.options.max_pack_size_warning.as_deref() {
            writeln!(
                err,
                "warning: max-pack-size is now in bytes, assuming --max-pack-size={value}m"
            )?;
        }
        let total_objects =
            self.stats.blobs + self.stats.trees + self.stats.commits + self.stats.tags;
        let branches = self.stats.branches.len();
        let mark_count = self.marks.len();
        let mark_slots = fast_import_mark_slot_count(mark_count);
        let allocated_objects = self.pack_state.borrow().seen.len();
        writeln!(err, "fast-import statistics:")?;
        writeln!(err, "{FAST_IMPORT_STAT_SEPARATOR}")?;
        writeln!(err, "Alloc'd objects:       {allocated_objects}")?;
        writeln!(
            err,
            "Total objects:{:13} ({:10} duplicates                  )",
            total_objects, 0
        )?;
        writeln!(
            err,
            "      blobs  :{:13} ({:10} duplicates{:11} deltas of{:11} attempts)",
            self.stats.blobs, 0, 0, 0
        )?;
        writeln!(
            err,
            "      trees  :{:13} ({:10} duplicates{:11} deltas of{:11} attempts)",
            self.stats.trees, 0, 0, 0
        )?;
        writeln!(
            err,
            "      commits:{:13} ({:10} duplicates{:11} deltas of{:11} attempts)",
            self.stats.commits, 0, 0, 0
        )?;
        writeln!(
            err,
            "      tags   :{:13} ({:10} duplicates{:11} deltas of{:11} attempts)",
            self.stats.tags, 0, 0, 0
        )?;
        writeln!(
            err,
            "Total branches:{:12} ({:10} loads     )",
            branches, branches
        )?;
        writeln!(
            err,
            "      marks:{:15} ({:10} unique    )",
            mark_slots, mark_count
        )?;
        writeln!(err, "      atoms:{:15}", self.stats.atoms.len())?;
        writeln!(err, "{FAST_IMPORT_STAT_SEPARATOR}")?;
        writeln!(err, "{FAST_IMPORT_STAT_SEPARATOR}")?;
        writeln!(err)?;
        Ok(())
    }
}

fn fast_import_ls_has_named_dataref(value: &[u8]) -> bool {
    if value.is_empty() || value.starts_with(b"\"") {
        return false;
    }
    let separator = value.iter().position(|byte| *byte == b' ');
    let Some(separator) = separator else {
        return value.len() == 40 && value.iter().all(u8::is_ascii_hexdigit);
    };
    let dataref = &value[..separator];
    if let Some(mark) = dataref.strip_prefix(b":") {
        return !mark.is_empty() && mark.iter().all(u8::is_ascii_digit);
    }
    true
}

const FAST_IMPORT_STAT_SEPARATOR: &str =
    "---------------------------------------------------------------------";

fn fast_import_mark_slot_count(mark_count: usize) -> usize {
    let mut slots = 1024;
    while mark_count > slots {
        slots *= 2;
    }
    slots
}

fn fast_import_zero_object_id(algorithm: GitHashAlgorithm) -> ObjectId {
    let zeros = [0_u8; 32];
    ObjectId::new(algorithm, &zeros[..algorithm.digest_len()])
}

fn fast_import_tree_count(index: &GitIndex) -> usize {
    let mut trees = BTreeSet::new();
    trees.insert(Vec::<u8>::new());
    for entry in index.entries() {
        let mut path = entry.path.as_slice();
        while let Some(position) = path.iter().position(|byte| *byte == b'/') {
            trees.insert(path[..position].to_vec());
            path = &path[position + 1..];
        }
    }
    trees.len()
}

fn parse_fast_import_mark_prefix(value: &str) -> usize {
    let end = value
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(value.len());
    if end == 0 {
        0
    } else {
        value[..end].parse().unwrap_or(usize::MAX)
    }
}

fn fast_import_record_path_atoms(index: &GitIndex, atoms: &mut BTreeSet<Vec<u8>>) {
    for entry in index.entries() {
        for atom in entry.path.split(|byte| *byte == b'/') {
            if !atom.is_empty() {
                atoms.insert(atom.to_vec());
            }
        }
    }
}

fn fast_import_crash_error(
    git_dir: &std::path::Path,
    fatal: String,
    recent_command: Option<&str>,
) -> Result<CliError> {
    let recent_commands = recent_command
        .map(|command| vec![command.as_bytes().to_vec()])
        .unwrap_or_default();
    let state = FastImportCrashReportState {
        recent_commands,
        marks: Vec::new(),
        branch_state: FastImportBranchLru::default(),
    };
    fast_import_crash_error_with_state(git_dir, fatal, &state)
}

fn fast_import_crash_error_with_state(
    git_dir: &std::path::Path,
    fatal: String,
    state: &FastImportCrashReportState,
) -> Result<CliError> {
    let crash_file = format!("fast_import_crash_{}", std::process::id());
    let crash_path = git_dir.join(&crash_file);
    fs::write(&crash_path, fast_import_crash_report(&fatal, state))?;
    Ok(CliError::Fatal {
        code: 128,
        message: format!("{fatal}\nfast-import: dumping crash report to .git/{crash_file}"),
    })
}

fn fast_import_crash_error_with_state_bytes(
    git_dir: &std::path::Path,
    fatal: &[u8],
    state: &FastImportCrashReportState,
) -> Result<CliError> {
    let crash_file = format!("fast_import_crash_{}", std::process::id());
    let crash_path = git_dir.join(&crash_file);
    fs::write(&crash_path, fast_import_crash_report_bytes(fatal, state))?;
    let mut stderr = io::stderr().lock();
    stderr.write_all(b"fatal: ")?;
    stderr.write_all(fatal)?;
    stderr.write_all(b"\nfast-import: dumping crash report to .git/")?;
    stderr.write_all(crash_file.as_bytes())?;
    stderr.write_all(b"\n")?;
    stderr.flush()?;
    Ok(CliError::Exit(128))
}

fn fast_import_crash_report(fatal: &str, state: &FastImportCrashReportState) -> Vec<u8> {
    fast_import_crash_report_bytes(fatal.as_bytes(), state)
}

fn fast_import_crash_report_bytes(fatal: &[u8], state: &FastImportCrashReportState) -> Vec<u8> {
    let command_count = state.recent_commands.len();
    let recent_commands_header = format!(
        concat!(
            "fast-import crash report:\n",
            "    fast-import process: {}\n",
            "    parent process     : {}\n",
            "    at {}\n",
            "\n",
            "fatal: ",
        ),
        std::process::id(),
        fast_import_parent_process_id(),
        chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            .format("%Y-%m-%d %H:%M:%S %z"),
    );
    let mut report = recent_commands_header.into_bytes();
    report.extend_from_slice(fatal);
    report.extend_from_slice(
        concat!(
            "\n\n",
            "Most Recent Commands Before Crash\n",
            "---------------------------------\n",
        )
        .as_bytes(),
    );
    for (index, command) in state.recent_commands.iter().enumerate() {
        report.extend_from_slice(if index + 1 == command_count {
            b"* "
        } else {
            b"  "
        });
        report.extend_from_slice(command);
        report.push(b'\n');
    }
    state.branch_state.render_report(&mut report);
    for (mark, id) in &state.marks {
        report.extend_from_slice(format!(":{mark} {id}\n").as_bytes());
    }
    report.extend_from_slice(b"\n-------------------\nEND OF CRASH REPORT\n");
    report
}

fn fast_import_parent_process_id() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getppid has no preconditions and only reads process state.
        unsafe { libc::getppid() as u32 }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

fn parse_fast_import_rfc2822_signature(raw: &str) -> Result<Signature> {
    let (name_email, date) = raw.rsplit_once("> ").ok_or_else(fast_import_parse_error)?;
    let name_email = format!("{name_email}>");
    let (name, email) = name_email
        .rsplit_once(" <")
        .and_then(|(name, email)| email.strip_suffix('>').map(|email| (name, email)))
        .ok_or_else(fast_import_parse_error)?;
    let date = chrono::DateTime::parse_from_rfc2822(date).map_err(|_| fast_import_parse_error())?;
    Ok(Signature::new(
        name,
        email,
        date.timestamp(),
        date.format("%z").to_string(),
    )?)
}

fn fast_import_parse_error() -> CliError {
    CliError::Fatal {
        code: 1,
        message: "invalid fast-import stream".into(),
    }
}

fn fast_import_data_limit_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "fast-import data exceeds configured object size limit",
    )
}

fn fast_import_control_line_limit_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        FAST_IMPORT_CONTROL_LINE_LIMIT_MESSAGE,
    )
}

fn is_fast_import_top_level_command(line: &str) -> bool {
    line == "blob"
        || line == "checkpoint"
        || line == "done"
        || line.starts_with("cat-blob ")
        || line.starts_with("commit ")
        || line.starts_with("get-mark ")
        || line.starts_with("ls ")
        || line.starts_with("option cat-blob-fd=")
        || line.starts_with("progress ")
        || line.starts_with("reset ")
        || line.starts_with("tag ")
}

fn fast_import_should_write_reflog(repo: &GitRepo, ref_name: &str) -> Result<bool> {
    if let Some(entry) = read_config_entry(repo, "core.logAllRefUpdates")? {
        return entry.bool_value().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{}'", entry.value),
        });
    }
    Ok(ref_name == "HEAD"
        || ref_name.starts_with("refs/heads/")
        || ref_name.starts_with("refs/remotes/")
        || ref_name.starts_with("refs/notes/")
        || ref_name.starts_with("refs/worktree/"))
}

fn parse_fast_import_mode(mode: &str) -> Result<IndexMode> {
    match mode {
        "644" => Ok(IndexMode::File),
        "755" => Ok(IndexMode::Executable),
        "120000" => Ok(IndexMode::Symlink),
        "40000" | "040000" => Ok(IndexMode::Tree),
        "160000" => Ok(IndexMode::Gitlink),
        _ => parse_index_mode(mode),
    }
}

fn fast_import_corrupt_mode_fatal(rest: &[u8]) -> Vec<u8> {
    let mut fatal = b"corrupt mode: M ".to_vec();
    fatal.extend_from_slice(rest);
    fatal
}

fn fast_import_verify_path(path: &[u8], mode: IndexMode) -> bool {
    if path.is_empty()
        || path.starts_with(b"/")
        || path.ends_with(b"/")
        || (path.len() >= 3
            && path[0].is_ascii_alphabetic()
            && path[1] == b':'
            && matches!(path[2], b'/' | b'\\'))
    {
        return false;
    }
    for (index, component) in path.split(|byte| *byte == b'/').enumerate() {
        let last = index + 1 == path.split(|byte| *byte == b'/').count();
        if component.is_empty() {
            if last && mode == IndexMode::Tree {
                continue;
            }
            return false;
        }
        if component == b"." || component == b".." {
            return false;
        }
        if component.eq_ignore_ascii_case(b".git")
            || (mode == IndexMode::Symlink && component.eq_ignore_ascii_case(b".gitmodules"))
        {
            return false;
        }
    }
    true
}

fn fast_import_semantic_fatal(prefix: &[u8], rest: &[u8]) -> Vec<u8> {
    let mut fatal = prefix.to_vec();
    fatal.extend_from_slice(rest);
    fatal
}

fn fast_import_kind_fatal(
    prefix: &[u8],
    actual_kind: &[u8],
    suffix: &[u8],
    rest: &[u8],
) -> Vec<u8> {
    let mut fatal = prefix.to_vec();
    fatal.extend_from_slice(actual_kind);
    fatal.extend_from_slice(suffix);
    fatal.extend_from_slice(rest);
    fatal
}

fn fast_import_path_fatal(command: &[u8], rest: &[u8], error: io::Error) -> Vec<u8> {
    let mut fatal = error.to_string().into_bytes();
    fatal.extend_from_slice(b": ");
    fatal.extend_from_slice(command);
    fatal.push(b' ');
    fatal.extend_from_slice(rest);
    fatal
}

fn fast_import_copy_path_fatal(
    command: &[u8],
    rest: &[u8],
    error: io::Error,
    field: &str,
) -> Vec<u8> {
    let message = error.to_string();
    let message = if let Some(prefix) = message.strip_suffix("path") {
        format!("{prefix}{field}")
    } else {
        message
    };
    let mut fatal = message.into_bytes();
    fatal.extend_from_slice(b": ");
    fatal.extend_from_slice(command);
    fatal.push(b' ');
    let command_rest = rest.split(|byte| *byte == b'\0').next().unwrap_or(rest);
    fatal.extend_from_slice(command_rest);
    fatal
}

fn fast_import_invalid_path_fatal(path: &[u8]) -> Vec<u8> {
    let mut fatal = b"invalid path '".to_vec();
    fatal.extend_from_slice(path);
    fatal.push(b'\'');
    fatal
}

fn fast_import_source_not_in_branch_fatal(path: &[u8]) -> Vec<u8> {
    let mut fatal = b"path ".to_vec();
    fatal.extend_from_slice(path);
    fatal.extend_from_slice(b" not in branch");
    fatal
}

fn fast_import_path_prefix_before_empty_component(path: &[u8]) -> Option<&[u8]> {
    if path.is_empty() {
        return None;
    }
    let mut offset: usize = 0;
    for component in path.split(|byte| *byte == b'/') {
        if component.is_empty() {
            return Some(&path[..offset.saturating_sub(1)]);
        }
        offset = offset.saturating_add(component.len().saturating_add(1));
    }
    None
}

fn fast_import_copy_source_lookup_path(source: &[u8], rename: bool) -> &[u8] {
    if !rename && source.starts_with(b"/") {
        &[]
    } else {
        source
    }
}

fn remove_fast_import_destination_conflicts(
    index: &mut GitIndex,
    empty_trees: &mut FastImportEmptyTreeState,
    path: &[u8],
) {
    index.remove_fast_import_path(path);
    empty_trees.remove_path(path);
    empty_trees.remove_ancestor_trees(path);
    let mut ancestor_end = path.len();
    while let Some(separator) = path[..ancestor_end].iter().rposition(|byte| *byte == b'/') {
        let ancestor = &path[..separator];
        if index.entry(ancestor, 0).is_some() {
            index.remove_fast_import_path(ancestor);
        }
        ancestor_end = separator;
    }
}

#[cfg(test)]
mod fast_import_data_tests {
    #[cfg(unix)]
    use std::ffi::OsStr;
    #[cfg(any(unix, windows))]
    use std::fs;
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn windows_file_index_preserves_both_u32_halves() {
        assert_eq!(
            fast_import_windows_file_index(0x0123_4567, 0x89ab_cdef),
            0x0123_4567_89ab_cdef
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn file_identity_and_link_count_are_queried_from_the_open_handle() {
        let temp = tempfile::tempdir().expect("temporary identity directory");
        let original = temp.path().join("original");
        let linked = temp.path().join("linked");
        fs::write(&original, b"identity").expect("write identity fixture");
        let file = fs::File::open(&original).expect("open identity fixture");
        let identity = fast_import_file_identity(&file).expect("read identity from handle");
        fs::hard_link(&original, &linked).expect("create second name");
        let linked_file = fs::File::open(&linked).expect("open linked identity fixture");
        assert!(fast_import_file_identity(&linked_file).expect("read linked identity") == identity);
        assert_eq!(
            fast_import_file_link_count(&file).expect("read handle link count"),
            2
        );
        assert!(fast_import_require_single_link(&file).is_err());
    }

    #[test]
    fn fast_import_paths_preserve_bytes_and_git_delimiters() {
        assert_eq!(
            parse_path_eol(b"name with final space ")
                .unwrap()
                .as_bytes(),
            b"name with final space "
        );
        assert_eq!(
            parse_path_eol(b"raw\x80\0ignored").unwrap().as_bytes(),
            b"raw\x80"
        );
        let (field, remainder) = parse_path_space(b"field next").expect("space-delimited path");
        assert_eq!(field.as_bytes(), b"field");
        assert_eq!(remainder, b"next");
        assert_eq!(
            parse_path_eol(br#""a\a\b\f\n\r\t\v\\\"\377""#)
                .expect("quoted C escapes")
                .as_bytes(),
            b"a\x07\x08\x0c\n\r\t\x0b\\\"\xff"
        );
        assert!(parse_path_space(b"\"src\"\0").is_err());
        for value in [
            br#""unterminated"#.as_slice(),
            br#""bad\q""#.as_slice(),
            br#""short\07""#.as_slice(),
            br#""nul\000""#.as_slice(),
            br#""ok"tail"#.as_slice(),
        ] {
            assert!(
                parse_path_eol(value).is_err(),
                "accepted malformed path {value:?}"
            );
        }
    }

    #[test]
    fn data_buffer_rejects_declared_and_cumulative_overflow() {
        assert!(FastImportDataBuffer::with_declared_length(5, 4).is_err());

        let mut buffer = FastImportDataBuffer::new(4);
        buffer.extend(b"abc").expect("first bounded chunk");
        assert!(buffer.extend(b"de").is_err());
        assert!(buffer.push_line(b"x").is_err());
        assert_eq!(buffer.into_inner(), b"abc");
    }

    #[test]
    fn bounded_heredoc_line_rejects_before_unbounded_growth() {
        let input = FastImportInput::new(BufReader::new(Cursor::new(b"1234\n".to_vec())));
        let mut input = input;
        let line = input
            .next_line_bytes_bounded(4)
            .expect("bounded heredoc line read")
            .expect("heredoc line");
        let mut buffer = FastImportDataBuffer::new(4);
        let error = buffer
            .push_line(&line)
            .expect_err("the heredoc newline must count toward the object limit");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "fast-import data exceeds configured object size limit"
        );

        let input = FastImportInput::new(BufReader::with_capacity(
            2,
            Cursor::new(b"1234567\n".to_vec()),
        ));
        let mut input = input;
        let error = input
            .next_line_bytes_bounded(4)
            .expect_err("an oversized heredoc line must fail during bounded reading");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            "fast-import data exceeds configured object size limit"
        );
        assert_eq!(
            input.reader.buffer(),
            b"7\n",
            "the chunk crossing the bounded allocation limit must remain unconsumed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_replacement_after_open_preserves_foreign_inode() {
        let temp = tempfile::tempdir().expect("temporary cleanup directory");
        let item = temp.path().join("item");
        let foreign = temp.path().join("foreign");
        fs::write(&item, b"owned").expect("write owned file");
        fs::write(&foreign, b"foreign").expect("write foreign file");
        let directory = FastImportDirectory::open(temp.path()).expect("open cleanup directory");
        let identity = fast_import_metadata_identity(
            &directory
                .open_read(OsStr::new("item"))
                .expect("open owned file")
                .metadata()
                .expect("owned metadata"),
        )
        .expect("owned identity");
        let owned = FastImportOwnedPath {
            path: item.clone(),
            identity,
            parent_identity: directory.identity(),
            file: None,
        };

        let error = remove_fast_import_owned_file_after_open(&owned, || {
            fs::rename(&foreign, &item).expect("replace cleanup pathname");
        })
        .expect_err("replacement must fail closed");
        assert!(error.to_string().contains("replaced"));
        assert_eq!(
            fs::read(&item).expect("read surviving foreign file"),
            b"foreign"
        );
        assert!(
            !foreign.exists(),
            "foreign inode was restored at its target"
        );
    }

    #[cfg(unix)]
    #[test]
    fn release_keeps_rejects_pack_or_index_replacement() {
        for replaced_name in ["pack", "idx"] {
            let temp = tempfile::tempdir().expect("temporary pack directory");
            let directory = FastImportDirectory::open(temp.path()).expect("open pack directory");
            let pack_path = temp.path().join("pack");
            let idx_path = temp.path().join("idx");
            let keep_path = temp.path().join("pack.keep");
            fs::write(&pack_path, b"pack").expect("write pack");
            fs::write(&idx_path, b"idx").expect("write index");
            fs::write(&keep_path, b"fast-import").expect("write keep");
            let pack_owned = directory
                .owned_path_matching(OsStr::new("pack"), None)
                .expect("capture pack identity");
            let idx_owned = directory
                .owned_path_matching(OsStr::new("idx"), None)
                .expect("capture index identity");
            let keep_owned = directory
                .owned_path_matching(OsStr::new("pack.keep"), None)
                .expect("capture keep identity");
            let replaced_path = temp.path().join(replaced_name);
            fs::remove_file(&replaced_path).expect("remove final path");
            fs::write(&replaced_path, b"foreign").expect("replace final path");

            let mut state = FastImportPackState::new(
                temp.path().to_owned(),
                temp.path().to_owned(),
                GitHashAlgorithm::Sha1,
                None,
            );
            state.published.push(FastImportPublishedPack {
                directory,
                pack_path,
                pack_owned: Some(pack_owned),
                idx_owned: Some(idx_owned),
                keep_path: Some(keep_owned),
                edge_ids: Vec::new(),
            });
            let error = state
                .release_keeps()
                .expect_err("replaced final artifact must block keep release");
            assert!(error.to_string().contains("replaced"));
            assert!(keep_path.exists(), "keep must remain on identity mismatch");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn link_uses_retained_descriptor_after_source_replacement() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("temporary link directory");
        let source = temp.path().join("source");
        let foreign = temp.path().join("foreign");
        let destination = OsStr::new("destination");
        fs::write(&source, b"owned").expect("write source");
        fs::write(&foreign, b"foreign").expect("write foreign");
        let directory = FastImportDirectory::open(temp.path()).expect("open link directory");
        let owned = directory
            .owned_path_matching(OsStr::new("source"), None)
            .expect("capture source identity");
        directory
            .link_owned_with_hook(&owned, destination, || {
                fs::remove_file(&source).expect("remove source");
                symlink(&foreign, &source).expect("replace source with symlink");
            })
            .expect("retained descriptor must defeat source-name replacement");
        assert_eq!(
            fs::read(temp.path().join(destination)).expect("read linked destination"),
            b"owned"
        );
        assert_eq!(
            fs::read(&foreign).expect("read foreign sentinel"),
            b"foreign"
        );
        assert!(
            fs::symlink_metadata(&source)
                .expect("source replacement remains")
                .file_type()
                .is_symlink()
        );
    }
}
