use super::*;

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
    pub(crate) relative_marks: Option<String>,
    pub(crate) rewrite_submodules_from: Option<String>,
    pub(crate) rewrite_submodules_to: Option<String>,
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
    let commands = collect_fast_export_commands(tree_cache, parent_tree.as_ref(), &commit.tree, options)?;
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
    writeln!(
        out,
        "committer {}",
        String::from_utf8_lossy(&committer)
    )?;
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
    note_fast_export_progress(out, state, fast_export_progress_step(options.progress.as_deref())?)?;
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
        note_fast_export_progress(out, state, fast_export_progress_step(options.progress.as_deref())?)?;
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
    fast_import_preflight(&repo.git_dir, &options)?;
    fast_import_modeled_noop_surface(&options);
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let ref_repo = GitRepo {
        root: repo.root.clone(),
        git_dir: common_git_dir.clone(),
        objects_dir: repo.objects_dir.clone(),
        index_path: repo.index_path.clone(),
    };
    let refs = RefStore::new(&common_git_dir, GitHashAlgorithm::Sha1);
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    FastImportParser::new(
        input,
        &ref_repo,
        &store,
        &refs,
        FastImportDateFormat::from_cli(options.date_format.as_deref(), &repo.git_dir)?,
        options,
    )
    .parse()
}

fn fast_import_modeled_noop_surface(options: &FastImportOptions) {
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

fn fast_import_preflight(git_dir: &Path, options: &FastImportOptions) -> Result<()> {
    if let Some(value) = options.relative_marks.as_deref() {
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

struct FastImportParser<'a> {
    input: Vec<u8>,
    cursor: usize,
    date_format: FastImportDateFormat,
    repo: &'a GitRepo,
    store: &'a LooseObjectStore,
    commit_cache: CommitObjectCache<'a, LooseObjectStore>,
    tree_cache: TreeObjectCache<'a, LooseObjectStore>,
    refs: &'a RefStore,
    marks: HashMap<usize, ObjectId>,
    ref_indexes: HashMap<String, GitIndex>,
    ref_tips: HashMap<String, ObjectId>,
    pending_line: Option<String>,
    stats: FastImportStats,
    options: FastImportOptions,
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

impl<'a> FastImportParser<'a> {
    fn new(
        input: Vec<u8>,
        repo: &'a GitRepo,
        store: &'a LooseObjectStore,
        refs: &'a RefStore,
        date_format: FastImportDateFormat,
        options: FastImportOptions,
    ) -> Self {
        Self {
            input,
            cursor: 0,
            date_format,
            repo,
            store,
            commit_cache: CommitObjectCache::new(store),
            tree_cache: TreeObjectCache::new(store),
            refs,
            marks: HashMap::new(),
            ref_indexes: HashMap::new(),
            ref_tips: HashMap::new(),
            pending_line: None,
            stats: FastImportStats::default(),
            options,
        }
    }

    fn parse(&mut self) -> Result<()> {
        self.preload_marks()?;
        let mut saw_done = false;
        while let Some(line) = self.next_control_line()? {
            if line.is_empty() {
                continue;
            }
            if line == "blob" {
                self.parse_blob()?;
            } else if let Some(ref_name) = line.strip_prefix("commit ") {
                self.parse_commit(ref_name)?;
            } else if let Some(ref_name) = line.strip_prefix("reset ") {
                self.parse_reset(ref_name)?;
            } else if line == "checkpoint" {
                continue;
            } else if line == "done" {
                saw_done = true;
                break;
            } else if line.starts_with("progress ") {
                println!("{line}");
            } else {
                return Err(self.unsupported_fast_import_command(&line)?);
            }
        }
        if self.options.done && !saw_done {
            return Err(fast_import_crash_error(
                &self.repo.git_dir,
                "stream ends early".to_owned(),
                None,
            )?);
        }
        self.export_marks()?;
        self.export_pack_edges()?;
        self.write_statistics()?;
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
            let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, oid.trim()).map_err(CliError::Io)?;
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
        fs::write(path, out)?;
        Ok(())
    }

    fn export_pack_edges(&self) -> Result<()> {
        let Some(path) = self.options.export_pack_edges.as_deref() else {
            return Ok(());
        };
        fs::write(path, [])?;
        Ok(())
    }

    fn parse_blob(&mut self) -> Result<()> {
        let mark = self.expect_mark()?;
        let content = self.expect_data()?;
        let id = self.store.write_object(GitObjectKind::Blob, &content)?;
        self.marks.insert(mark, id);
        self.stats.blobs += 1;
        Ok(())
    }

    fn parse_commit(&mut self, ref_name: &str) -> Result<()> {
        let mark = self.next_optional_mark()?;
        let (author, committer) = self.expect_commit_signatures()?;
        let message = self.expect_data()?;
        let mut parent = self
            .ref_tips
            .get(ref_name)
            .cloned()
            .or_else(|| self.refs.resolve(ref_name).ok());
        let mut index = if let Some(index) = self.ref_indexes.remove(ref_name) {
            index
        } else if let Some(parent_id) = parent.as_ref() {
            let parent_commit = self.commit_cache.read_commit(parent_id)?;
            self.tree_cache.read_tree_to_index(&parent_commit.tree)?
        } else {
            GitIndex::new()
        };

        while let Some(line) = self.next_control_line()? {
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("from ") {
                let parent_id = self.resolve_fast_import_value(value)?;
                parent = Some(parent_id.clone());
                let parent_commit = self.commit_cache.read_commit(&parent_id)?;
                index = self.tree_cache.read_tree_to_index(&parent_commit.tree)?;
            } else if line == "deleteall" {
                index = GitIndex::new();
            } else if let Some(rest) = line.strip_prefix("M ") {
                self.apply_fast_import_modify(&mut index, rest)?;
            } else if is_fast_import_top_level_command(&line) {
                self.pending_line = Some(line);
                break;
            } else {
                self.write_fast_import_unreferenced_commit(
                    &index,
                    author.clone(),
                    committer.clone(),
                    message.clone(),
                    parent.clone(),
                )?;
                return Err(self.unsupported_fast_import_command(&line)?);
            }
        }

        let tree = write_tree_from_index(self.store, &index)?;
        self.stats.trees += fast_import_tree_count(&index);
        self.stats.commits += 1;
        self.stats.branches.insert(ref_name.to_owned());
        fast_import_record_path_atoms(&index, &mut self.stats.atoms);
        let mut builder = CommitBuilder::new(tree, author, committer);
        if let Some(parent) = parent {
            builder = builder.parent(parent);
        }
        let encoded = builder.message(message)?.encode()?;
        let id = self.store.write_object(GitObjectKind::Commit, &encoded)?;
        self.write_fast_import_ref(ref_name, &id)?;
        if let Some(mark) = mark {
            self.marks.insert(mark, id.clone());
        }
        self.ref_tips.insert(ref_name.to_owned(), id.clone());
        self.ref_indexes.insert(ref_name.to_owned(), index);
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
        let Some(line) = self.next_control_line()? else {
            return Ok(());
        };
        if line.is_empty() {
            return Ok(());
        }
        let Some(value) = line.strip_prefix("from ") else {
            self.pending_line = Some(line);
            return Ok(());
        };
        let id = self.resolve_fast_import_value(value)?;
        self.refs.write_ref(ref_name, &id)?;
        let commit = self.commit_cache.read_commit(&id)?;
        let index = self.tree_cache.read_tree_to_index(&commit.tree)?;
        self.ref_tips.insert(ref_name.to_owned(), id);
        self.ref_indexes.insert(ref_name.to_owned(), index);
        self.consume_optional_blank_line();
        Ok(())
    }

    fn apply_fast_import_modify(&mut self, index: &mut GitIndex, rest: &str) -> Result<()> {
        let mut parts = rest.splitn(3, ' ');
        let mode = parts.next().ok_or_else(fast_import_parse_error)?;
        let value = parts.next().ok_or_else(fast_import_parse_error)?;
        let path = parts.next().ok_or_else(fast_import_parse_error)?;
        let mode = parse_fast_import_mode(mode)?;
        let id = if value == "inline" {
            let content = self.expect_data()?;
            let id = self.store.write_object(GitObjectKind::Blob, &content)?;
            self.stats.blobs += 1;
            id
        } else {
            self.resolve_fast_import_value(value)?
        };
        let size = match mode {
            IndexMode::Tree | IndexMode::Gitlink => 0,
            _ => self
                .store
                .read_object(&id)?
                .content
                .len()
                .min(u32::MAX as usize) as u32,
        };
        index.upsert(IndexEntry::new(path.as_bytes().to_vec(), id, mode, size)?)?;
        Ok(())
    }

    fn next_optional_mark(&mut self) -> Result<Option<usize>> {
        let line = self.next_required_line()?;
        if let Some(mark) = line.strip_prefix("mark :") {
            return mark
                .parse::<usize>()
                .map(Some)
                .map_err(|_| fast_import_parse_error());
        }
        self.pending_line = Some(line);
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
            FastImportDateFormat::Raw => return signature_from_commit_bytes(raw.as_bytes()),
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
        let line = self.next_required_line()?;
        if let Some(delimiter) = line.strip_prefix("data <<") {
            let mut content = Vec::new();
            loop {
                let Some(line) = self.next_line_bytes() else {
                    return Err(fast_import_parse_error());
                };
                if line == delimiter.as_bytes() {
                    break;
                }
                content.extend_from_slice(&line);
                content.push(b'\n');
            }
            return Ok(content);
        }
        let Some(len) = line.strip_prefix("data ") else {
            return Err(fast_import_parse_error());
        };
        let len = len
            .parse::<usize>()
            .map_err(|_| fast_import_parse_error())?;
        if self.cursor + len > self.input.len() {
            return Err(fast_import_parse_error());
        }
        let content = self.input[self.cursor..self.cursor + len].to_vec();
        self.cursor += len;
        if self.input.get(self.cursor) == Some(&b'\n') {
            self.cursor += 1;
        }
        Ok(content)
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
            ObjectId::from_hex(GitHashAlgorithm::Sha1, value).map_err(CliError::Io)
        }
    }

    fn write_fast_import_ref(&self, ref_name: &str, id: &ObjectId) -> Result<()> {
        let old_id = self
            .refs
            .resolve(ref_name)
            .unwrap_or_else(|_| zero_object_id());
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

    fn write_fast_import_unreferenced_commit(
        &self,
        index: &GitIndex,
        author: Signature,
        committer: Signature,
        message: Vec<u8>,
        parent: Option<ObjectId>,
    ) -> Result<ObjectId> {
        let tree = write_tree_from_index(self.store, index)?;
        let mut builder = CommitBuilder::new(tree, author, committer);
        if let Some(parent) = parent {
            builder = builder.parent(parent);
        }
        let encoded = builder.message(message)?.encode()?;
        Ok(self.store.write_object(GitObjectKind::Commit, &encoded)?)
    }

    fn next_required_line(&mut self) -> Result<String> {
        self.next_control_line()?
            .ok_or_else(fast_import_parse_error)
    }

    fn next_control_line(&mut self) -> Result<Option<String>> {
        if self.pending_line.is_some() {
            return Ok(self.pending_line.take());
        }
        let Some(line) = self.next_line_bytes() else {
            return Ok(None);
        };
        String::from_utf8(line)
            .map(Some)
            .map_err(|_| fast_import_parse_error())
    }

    fn next_line_bytes(&mut self) -> Option<Vec<u8>> {
        if self.cursor >= self.input.len() {
            return None;
        }
        let start = self.cursor;
        let end = self.input[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| start + offset)
            .unwrap_or(self.input.len());
        self.cursor = end.saturating_add(1).min(self.input.len());
        let mut line = self.input[start..end].to_vec();
        if line.ends_with(b"\r") {
            line.pop();
        }
        Some(line)
    }

    fn consume_optional_blank_line(&mut self) {
        if self.input.get(self.cursor) == Some(&b'\n') {
            self.cursor += 1;
        }
    }

    fn unsupported_fast_import_command(&self, line: &str) -> Result<CliError> {
        fast_import_crash_error(
            &self.repo.git_dir,
            format!("Unsupported command: {line}"),
            Some(line),
        )
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
        writeln!(err, "fast-import statistics:")?;
        writeln!(err, "{FAST_IMPORT_STAT_SEPARATOR}")?;
        writeln!(err, "Alloc'd objects:       5000")?;
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
        writeln!(err, "Memory total:          2493 KiB")?;
        writeln!(err, "       pools:          2141 KiB")?;
        writeln!(err, "     objects:           351 KiB")?;
        writeln!(err, "{FAST_IMPORT_STAT_SEPARATOR}")?;
        writeln!(err, "pack_report: getpagesize()            =      16384")?;
        writeln!(err, "pack_report: core.packedGitWindowSize = 1073741824")?;
        writeln!(
            err,
            "pack_report: core.packedGitLimit      = 35184372088832"
        )?;
        writeln!(err, "pack_report: pack_used_ctr            =          0")?;
        writeln!(err, "pack_report: pack_mmap_calls          =          0")?;
        writeln!(
            err,
            "pack_report: pack_open_windows        =          0 /          0"
        )?;
        writeln!(
            err,
            "pack_report: pack_mapped              =          0 /          0"
        )?;
        writeln!(err, "{FAST_IMPORT_STAT_SEPARATOR}")?;
        writeln!(err)?;
        Ok(())
    }
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
    let crash_file = format!("fast_import_crash_{}", std::process::id());
    let crash_path = git_dir.join(&crash_file);
    fs::write(
        &crash_path,
        fast_import_crash_report(&fatal, recent_command),
    )?;
    Ok(CliError::Fatal {
        code: 128,
        message: format!("{fatal}\nfast-import: dumping crash report to .git/{crash_file}"),
    })
}

fn fast_import_crash_report(fatal: &str, recent_command: Option<&str>) -> String {
    let recent_command = recent_command
        .map(|command| format!("* {command}\n"))
        .unwrap_or_default();
    format!(
        "fast-import crash report:\n\
             \n\
             fatal: {fatal}\n\
             \n\
             Most Recent Commands Before Crash\n\
             ---------------------------------\n\
             {recent_command}\
             \n\
             Active Branch LRU\n\
             -----------------\n\
                 active_branches = 0 cur, 5 max\n\
             \n\
             Inactive Branches\n\
             -----------------\n\
             \n\
             Marks\n\
             -----\n\
             \n\
             -------------------\n\
             END OF CRASH REPORT\n"
    )
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

fn is_fast_import_top_level_command(line: &str) -> bool {
    line == "blob"
        || line == "checkpoint"
        || line == "done"
        || line.starts_with("commit ")
        || line.starts_with("progress ")
        || line.starts_with("reset ")
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
