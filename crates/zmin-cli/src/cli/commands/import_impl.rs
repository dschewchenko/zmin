use super::*;

pub(crate) fn quiltimport(
    dry_run: bool,
    author: Option<&str>,
    patches: Option<PathBuf>,
    series: Option<PathBuf>,
    _keep_non_patch: bool,
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
        check: false,
        cached: false,
        index: true,
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

pub(crate) fn fast_export(all: bool, refs: Vec<String>) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let refs = fast_export_refs(&repo, all, refs)?;
    let mut state = FastExportState::default();
    let mut out = io::stdout().lock();
    for (ref_name, tip) in refs {
        let mut commits = collect_commits_cached(
            &repo,
            &store,
            &commit_cache,
            std::slice::from_ref(&tip),
            None,
        )?;
        commits.reverse();
        for id in &commits {
            if !state.commit_marks.contains_key(&id.to_hex()) {
                write_fast_export_commit(
                    &mut out,
                    &store,
                    &commit_cache,
                    &tree_cache,
                    &mut state,
                    &ref_name,
                    id,
                )?;
            }
        }
        if let Some(mark) = state.commit_marks.get(&tip) {
            writeln!(out, "reset {ref_name}")?;
            writeln!(out, "from :{mark}")?;
            writeln!(out)?;
        }
    }
    Ok(())
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
        refs.into_iter()
            .map(|name| {
                if name.starts_with("refs/") {
                    name
                } else {
                    format!("refs/heads/{name}")
                }
            })
            .collect()
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
}

impl FastExportState {
    fn alloc_mark(&mut self) -> usize {
        self.next_mark += 1;
        self.next_mark
    }
}

fn write_fast_export_commit<W: Write>(
    out: &mut W,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    state: &mut FastExportState,
    ref_name: &str,
    id: &ObjectId,
) -> Result<()> {
    let commit = commit_cache.read_commit(id)?;
    let files = collect_tree_blobs(tree_cache, &commit.tree)?;
    for file in &files {
        if matches!(
            file.mode,
            TreeMode::File | TreeMode::Executable | TreeMode::Symlink
        ) && !state.blob_marks.contains_key(&file.id.to_hex())
        {
            let mark = state.alloc_mark();
            state.blob_marks.insert(file.id.to_hex(), mark);
            let object = store.read_object(&file.id)?;
            writeln!(out, "blob")?;
            writeln!(out, "mark :{mark}")?;
            writeln!(out, "data {}", object.content.len())?;
            out.write_all(&object.content)?;
            if !object.content.ends_with(b"\n") {
                writeln!(out)?;
            }
        }
    }

    let mark = state.alloc_mark();
    state.commit_marks.insert(id.to_hex(), mark);
    writeln!(out, "commit {ref_name}")?;
    writeln!(out, "mark :{mark}")?;
    writeln!(out, "author {}", String::from_utf8_lossy(&commit.author))?;
    writeln!(
        out,
        "committer {}",
        String::from_utf8_lossy(&commit.committer)
    )?;
    writeln!(out, "data {}", commit.message.len())?;
    out.write_all(&commit.message)?;
    if !commit.message.ends_with(b"\n") {
        writeln!(out)?;
    }
    if let Some(parent) = commit.parents.first()
        && let Some(parent_mark) = state.commit_marks.get(&parent.to_hex())
    {
        writeln!(out, "from :{parent_mark}")?;
    }
    writeln!(out, "deleteall")?;
    for file in files {
        write_fast_export_file_command(out, state, &file)?;
    }
    writeln!(out)?;
    Ok(())
}

#[derive(Debug, Clone)]
struct FastExportFile {
    path: Vec<u8>,
    mode: TreeMode,
    id: ObjectId,
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
                mode: entry.mode,
                id: entry.id.clone(),
            });
        }
    }
    Ok(())
}

fn write_fast_export_file_command<W: Write>(
    out: &mut W,
    state: &FastExportState,
    file: &FastExportFile,
) -> Result<()> {
    let path = String::from_utf8_lossy(&file.path);
    match file.mode {
        TreeMode::File | TreeMode::Executable | TreeMode::Symlink => {
            let mark = state
                .blob_marks
                .get(&file.id.to_hex())
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "fast-export missing blob mark".into(),
                })?;
            writeln!(
                out,
                "M {} :{mark} {path}",
                fast_export_mode(file.mode).unwrap_or("100644")
            )?;
        }
        TreeMode::Gitlink => {
            writeln!(out, "M 160000 {} {path}", file.id.to_hex())?;
        }
        TreeMode::Tree => {}
    }
    Ok(())
}

fn fast_export_mode(mode: TreeMode) -> Option<&'static str> {
    match mode {
        TreeMode::File => Some("100644"),
        TreeMode::Executable => Some("100755"),
        TreeMode::Symlink => Some("120000"),
        _ => None,
    }
}

pub(crate) fn fast_import(date_format: Option<&str>) -> Result<()> {
    let repo = find_repo()?;
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
        FastImportDateFormat::from_cli(date_format, &repo.git_dir)?,
    )
    .parse()
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
}

impl<'a> FastImportParser<'a> {
    fn new(
        input: Vec<u8>,
        repo: &'a GitRepo,
        store: &'a LooseObjectStore,
        refs: &'a RefStore,
        date_format: FastImportDateFormat,
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
        }
    }

    fn parse(&mut self) -> Result<()> {
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
                break;
            } else if line.starts_with("progress ") {
                println!("{line}");
            } else {
                return Err(self.unsupported_fast_import_command(&line)?);
            }
        }
        Ok(())
    }

    fn parse_blob(&mut self) -> Result<()> {
        let mark = self.expect_mark()?;
        let content = self.expect_data()?;
        let id = self.store.write_object(GitObjectKind::Blob, &content)?;
        self.marks.insert(mark, id);
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
            self.store.write_object(GitObjectKind::Blob, &content)?
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
