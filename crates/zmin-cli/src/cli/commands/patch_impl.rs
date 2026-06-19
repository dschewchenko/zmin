use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ApplyOptions {
    pub(crate) check: bool,
    pub(crate) cached: bool,
    pub(crate) index: bool,
    pub(crate) reverse: bool,
    pub(crate) patches: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ApplyFilePatch {
    pub(crate) old_path: Option<Vec<u8>>,
    pub(crate) new_path: Option<Vec<u8>>,
    pub(crate) new_mode: Option<IndexMode>,
    pub(crate) rename: bool,
    pub(crate) deleted: bool,
    pub(crate) binary: Option<ApplyBinaryPatch>,
    pub(crate) hunks: Vec<ApplyHunk>,
}

#[derive(Debug, Clone)]
pub(crate) struct ApplyBinaryPatch {
    pub(crate) forward: ApplyBinaryRecord,
    pub(crate) reverse: ApplyBinaryRecord,
}

#[derive(Debug, Clone)]
pub(crate) enum ApplyBinaryRecord {
    Literal(Vec<u8>),
    Delta(Vec<u8>),
}

#[derive(Debug, Clone)]
pub(crate) struct ApplyHunk {
    pub(crate) old_start: usize,
    pub(crate) old_count: usize,
    pub(crate) new_start: usize,
    pub(crate) new_count: usize,
    pub(crate) lines: Vec<ApplyHunkLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplyHunkLine {
    Context(Vec<u8>),
    Delete(Vec<u8>),
    Insert(Vec<u8>),
}

type ApplyPaths = (Option<Vec<u8>>, Option<Vec<u8>>);

#[derive(Debug, Clone)]
pub(crate) struct ApplyUpdate {
    path: Vec<u8>,
    remove_path: Option<Vec<u8>>,
    content: Vec<u8>,
    mode: IndexMode,
    deleted: bool,
}

#[derive(Debug)]
pub(crate) struct PatchAnswers {
    answers: VecDeque<PatchAnswer>,
    quit: bool,
}

#[derive(Debug, Clone, Copy)]
enum PatchAnswer {
    Yes,
    No,
    All,
    Done,
    Quit,
    Split,
}

pub(crate) fn run_apply(
    check: bool,
    cached: bool,
    index: bool,
    reverse: bool,
    patches: Vec<PathBuf>,
) -> Result<()> {
    apply(ApplyOptions {
        check,
        cached,
        index,
        reverse,
        patches,
    })
}

pub(crate) fn apply(options: ApplyOptions) -> Result<()> {
    if options.cached && options.index {
        return Err(CliError::Fatal {
            code: 129,
            message: "apply --cached and --index cannot be used together".into(),
        });
    }
    let repo = find_repo()?;
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let store = runtime.object_store_adapter().as_object_store();
    let mut index = read_repo_index(&repo)?;
    let patch_bytes = read_apply_patch_inputs(&options.patches)?;
    let patches = parse_apply_patches(&patch_bytes)?;
    let mut updates = Vec::new();
    for patch in patches {
        let patch = if options.reverse {
            reverse_apply_patch(patch)
        } else {
            patch
        };
        updates.push(apply_file_patch(&repo, &store, &index, &patch, &options)?);
    }
    if options.check {
        return Ok(());
    }
    for update in updates {
        write_apply_update(&repo, &store, &mut index, update, &options)?;
    }
    if options.cached || options.index {
        index.write_to_path(&repo.index_path)?;
    }
    Ok(())
}

pub(crate) fn parse_apply_patches(input: &[u8]) -> Result<Vec<ApplyFilePatch>> {
    let lines = split_diff_lines(input);
    let mut patches = Vec::new();
    let mut cursor = 0usize;
    while cursor < lines.len() {
        let line = trim_patch_line(lines[cursor]);
        if !line.starts_with(b"diff --git ") {
            cursor += 1;
            continue;
        }
        let (patch, next) = parse_apply_file_patch(&lines, cursor)?;
        patches.push(patch);
        cursor = next;
    }
    if patches.is_empty() && !input.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "No valid patches in input".into(),
        });
    }
    Ok(patches)
}

pub(crate) fn reverse_apply_patch(mut patch: ApplyFilePatch) -> ApplyFilePatch {
    std::mem::swap(&mut patch.old_path, &mut patch.new_path);
    patch.deleted = patch.new_path.is_none();
    if let Some(binary) = &mut patch.binary {
        std::mem::swap(&mut binary.forward, &mut binary.reverse);
    }
    for hunk in &mut patch.hunks {
        std::mem::swap(&mut hunk.old_start, &mut hunk.new_start);
        std::mem::swap(&mut hunk.old_count, &mut hunk.new_count);
        for line in &mut hunk.lines {
            *line = match line {
                ApplyHunkLine::Context(bytes) => ApplyHunkLine::Context(bytes.clone()),
                ApplyHunkLine::Delete(bytes) => ApplyHunkLine::Insert(bytes.clone()),
                ApplyHunkLine::Insert(bytes) => ApplyHunkLine::Delete(bytes.clone()),
            };
        }
    }
    patch
}

pub(crate) fn apply_file_patch(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    patch: &ApplyFilePatch,
    options: &ApplyOptions,
) -> Result<ApplyUpdate> {
    let target_path = patch
        .new_path
        .as_ref()
        .or(patch.old_path.as_ref())
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "patch has no target path".into(),
        })?
        .clone();
    let source_path = patch.old_path.as_ref().unwrap_or(&target_path);
    let index_entry = find_index_entry(index, source_path);
    let base = if options.cached {
        index_entry
            .map(|entry| read_index_entry_content(store, entry))
            .transpose()?
            .unwrap_or_default()
    } else {
        read_apply_worktree_content(repo, source_path)?
    };
    if options.index {
        let index_content = index_entry
            .map(|entry| read_index_entry_content(store, entry))
            .transpose()?
            .unwrap_or_default();
        if index_content != base {
            return Err(CliError::Fatal {
                code: 1,
                message: format!(
                    "{}: does not match index",
                    String::from_utf8_lossy(source_path)
                ),
            });
        }
    }
    let content = if let Some(binary) = &patch.binary {
        apply_binary_record(&base, &binary.forward, source_path)?
    } else if patch.hunks.is_empty() {
        base
    } else {
        apply_hunks_to_content(&base, &patch.hunks, source_path)?
    };
    let mode = patch
        .new_mode
        .or_else(|| index_entry.map(|entry| entry.mode))
        .unwrap_or(IndexMode::File);
    let remove_path = patch
        .rename
        .then(|| patch.old_path.clone())
        .flatten()
        .filter(|old_path| old_path != &target_path);
    Ok(ApplyUpdate {
        path: target_path,
        remove_path,
        content,
        mode,
        deleted: patch.deleted,
    })
}

pub(crate) fn apply_hunks_to_content(
    base: &[u8],
    hunks: &[ApplyHunk],
    path: &[u8],
) -> Result<Vec<u8>> {
    let base_lines = split_diff_lines(base)
        .into_iter()
        .map(|line| line.to_vec())
        .collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut cursor = 0usize;
    for hunk in hunks {
        let hunk_start = if hunk.old_count == 0 {
            hunk.old_start
        } else {
            hunk.old_start.saturating_sub(1)
        };
        if hunk_start < cursor || hunk_start > base_lines.len() {
            return Err(apply_mismatch_error(path));
        }
        output.extend(base_lines[cursor..hunk_start].iter().flatten().copied());
        let mut local = hunk_start;
        let mut seen_old = 0usize;
        let mut seen_new = 0usize;
        for line in &hunk.lines {
            match line {
                ApplyHunkLine::Context(expected) => {
                    apply_expect_line(&base_lines, local, expected, path)?;
                    output.extend_from_slice(expected);
                    local += 1;
                    seen_old += 1;
                    seen_new += 1;
                }
                ApplyHunkLine::Delete(expected) => {
                    apply_expect_line(&base_lines, local, expected, path)?;
                    local += 1;
                    seen_old += 1;
                }
                ApplyHunkLine::Insert(inserted) => {
                    output.extend_from_slice(inserted);
                    seen_new += 1;
                }
            }
        }
        if seen_old != hunk.old_count || seen_new != hunk.new_count {
            return Err(CliError::Fatal {
                code: 128,
                message: "hunk line counts do not match header".into(),
            });
        }
        cursor = local;
    }
    output.extend(base_lines[cursor..].iter().flatten().copied());
    Ok(output)
}

pub(crate) fn write_apply_update(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    update: ApplyUpdate,
    options: &ApplyOptions,
) -> Result<()> {
    if !options.cached {
        if let Some(remove_path) = &update.remove_path {
            let absolute = repo
                .root
                .join(String::from_utf8_lossy(remove_path).as_ref());
            match fs::remove_file(&absolute) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(CliError::Io(error)),
            }
        }
        let absolute = repo
            .root
            .join(String::from_utf8_lossy(&update.path).as_ref());
        if update.deleted {
            match fs::remove_file(&absolute) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(CliError::Io(error)),
            }
        } else {
            if let Some(parent) = absolute.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&absolute, &update.content)?;
            apply_worktree_mode(&absolute, update.mode)?;
        }
    }
    if options.cached || options.index {
        if let Some(remove_path) = &update.remove_path {
            index.remove_path(remove_path)?;
        }
        if update.deleted {
            index.remove_path(&update.path)?;
        } else {
            let id = store.write_object(GitObjectKind::Blob, &update.content)?;
            let mut entry = IndexEntry::new(
                update.path.clone(),
                id,
                update.mode,
                update.content.len().min(u32::MAX as usize) as u32,
            )?;
            let absolute = repo
                .root
                .join(String::from_utf8_lossy(&update.path).as_ref());
            if let Ok(metadata) = fs::symlink_metadata(&absolute) {
                apply_index_entry_metadata(&mut entry, &metadata);
            }
            index.upsert(entry)?;
        }
    }
    Ok(())
}

pub(crate) fn select_patch_hunks(
    patch: &ApplyFilePatch,
    answers: &mut PatchAnswers,
) -> Result<Vec<ApplyHunk>> {
    let mut selected = Vec::new();
    let mut all_remaining = None;
    for hunk in &patch.hunks {
        if answers.quit {
            break;
        }
        let take = match all_remaining {
            Some(value) => value,
            None => match answers.next() {
                PatchAnswer::Yes => true,
                PatchAnswer::No => false,
                PatchAnswer::All => {
                    all_remaining = Some(true);
                    true
                }
                PatchAnswer::Done => {
                    all_remaining = Some(false);
                    false
                }
                PatchAnswer::Quit => false,
                PatchAnswer::Split => {
                    let split = split_apply_hunk(hunk);
                    if split.len() <= 1 {
                        false
                    } else {
                        for split_hunk in split {
                            match answers.next() {
                                PatchAnswer::Yes | PatchAnswer::All => {
                                    selected.push(split_hunk);
                                }
                                PatchAnswer::Done => {
                                    all_remaining = Some(false);
                                    break;
                                }
                                PatchAnswer::Quit => break,
                                PatchAnswer::No | PatchAnswer::Split => {}
                            }
                            if answers.quit {
                                break;
                            }
                        }
                        continue;
                    }
                }
            },
        };
        if take {
            selected.push(hunk.clone());
        }
    }
    Ok(selected)
}

pub(crate) fn rejected_hunks_for_selection(
    patch: &ApplyFilePatch,
    selected_hunks: &[ApplyHunk],
) -> Vec<ApplyHunk> {
    let mut rejected = Vec::new();
    for hunk in &patch.hunks {
        if selected_hunks
            .iter()
            .any(|selected| same_hunk(selected, hunk))
        {
            continue;
        }
        let split = split_apply_hunk(hunk);
        if split.len() > 1
            && split.iter().any(|part| {
                selected_hunks
                    .iter()
                    .any(|selected| same_hunk(selected, part))
            })
        {
            rejected.extend(split.into_iter().filter(|part| {
                !selected_hunks
                    .iter()
                    .any(|selected| same_hunk(selected, part))
            }));
        } else {
            rejected.push(hunk.clone());
        }
    }
    rejected
}

pub(crate) fn same_hunk(left: &ApplyHunk, right: &ApplyHunk) -> bool {
    left.old_start == right.old_start
        && left.old_count == right.old_count
        && left.new_start == right.new_start
        && left.new_count == right.new_count
        && left.lines == right.lines
}

fn split_apply_hunk(hunk: &ApplyHunk) -> Vec<ApplyHunk> {
    let mut groups = Vec::new();
    let mut cursor = 0usize;
    while cursor < hunk.lines.len() {
        while cursor < hunk.lines.len() && matches!(hunk.lines[cursor], ApplyHunkLine::Context(_)) {
            cursor += 1;
        }
        if cursor >= hunk.lines.len() {
            break;
        }
        let start = cursor;
        while cursor < hunk.lines.len() && !matches!(hunk.lines[cursor], ApplyHunkLine::Context(_))
        {
            cursor += 1;
        }
        groups.push(start..cursor);
    }
    if groups.len() <= 1 {
        return vec![hunk.clone()];
    }

    let old_positions = hunk_old_positions(hunk);
    let new_positions = hunk_new_positions(hunk);
    let mut split = Vec::new();
    for group in groups {
        let old_count = hunk.lines[group.clone()]
            .iter()
            .filter(|line| !matches!(line, ApplyHunkLine::Insert(_)))
            .count();
        let new_count = hunk.lines[group.clone()]
            .iter()
            .filter(|line| !matches!(line, ApplyHunkLine::Delete(_)))
            .count();
        if old_count == 0 {
            let prev_context = group
                .start
                .checked_sub(1)
                .filter(|idx| matches!(hunk.lines[*idx], ApplyHunkLine::Context(_)));
            let next_context = (group.end < hunk.lines.len())
                .then_some(group.end)
                .filter(|idx| matches!(hunk.lines[*idx], ApplyHunkLine::Context(_)));
            if let Some(context_idx) = next_context {
                let mut lines = hunk.lines[group.clone()].to_vec();
                lines.push(hunk.lines[context_idx].clone());
                split.push(ApplyHunk {
                    old_start: old_positions[context_idx].unwrap_or(hunk.old_start),
                    old_count: 1,
                    new_start: new_positions[group.start].unwrap_or(hunk.new_start),
                    new_count: lines.len(),
                    lines,
                });
            } else if let Some(context_idx) = prev_context {
                let mut lines = Vec::with_capacity(group.len() + 1);
                lines.push(hunk.lines[context_idx].clone());
                lines.extend(hunk.lines[group.clone()].iter().cloned());
                split.push(ApplyHunk {
                    old_start: old_positions[context_idx].unwrap_or(hunk.old_start),
                    old_count: 1,
                    new_start: new_positions[context_idx].unwrap_or(hunk.new_start),
                    new_count: lines.len(),
                    lines,
                });
            } else {
                split.push(ApplyHunk {
                    old_start: old_positions[group.start].unwrap_or(hunk.old_start),
                    old_count,
                    new_start: new_positions[group.start].unwrap_or(hunk.new_start),
                    new_count,
                    lines: hunk.lines[group.clone()].to_vec(),
                });
            }
        } else {
            let old_start = group
                .clone()
                .find_map(|idx| old_positions[idx])
                .unwrap_or(hunk.old_start);
            let new_start = group
                .clone()
                .find_map(|idx| new_positions[idx])
                .unwrap_or(hunk.new_start);
            split.push(ApplyHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                lines: hunk.lines[group.clone()].to_vec(),
            });
        }
    }
    split
}

fn hunk_old_positions(hunk: &ApplyHunk) -> Vec<Option<usize>> {
    let mut old = hunk.old_start;
    hunk.lines
        .iter()
        .map(|line| match line {
            ApplyHunkLine::Context(_) | ApplyHunkLine::Delete(_) => {
                let current = old;
                old += 1;
                Some(current)
            }
            ApplyHunkLine::Insert(_) => Some(old),
        })
        .collect()
}

fn hunk_new_positions(hunk: &ApplyHunk) -> Vec<Option<usize>> {
    let mut new = hunk.new_start;
    hunk.lines
        .iter()
        .map(|line| match line {
            ApplyHunkLine::Context(_) | ApplyHunkLine::Insert(_) => {
                let current = new;
                new += 1;
                Some(current)
            }
            ApplyHunkLine::Delete(_) => Some(new),
        })
        .collect()
}

impl PatchAnswers {
    pub(crate) fn read() -> Result<Self> {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        let answers = input
            .chars()
            .filter_map(PatchAnswer::from_char)
            .collect::<VecDeque<_>>();
        Ok(Self {
            answers,
            quit: false,
        })
    }

    fn next(&mut self) -> PatchAnswer {
        let answer = self.answers.pop_front().unwrap_or(PatchAnswer::No);
        if matches!(answer, PatchAnswer::Quit) {
            self.quit = true;
        }
        answer
    }
}

impl PatchAnswer {
    fn from_char(value: char) -> Option<Self> {
        match value {
            'y' | 'Y' => Some(Self::Yes),
            'n' | 'N' => Some(Self::No),
            'a' | 'A' => Some(Self::All),
            'd' | 'D' => Some(Self::Done),
            'q' | 'Q' => Some(Self::Quit),
            's' | 'S' => Some(Self::Split),
            _ if value.is_whitespace() => None,
            _ => Some(Self::No),
        }
    }
}

fn read_apply_patch_inputs(paths: &[PathBuf]) -> Result<Vec<u8>> {
    if paths.is_empty() {
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input)?;
        return Ok(input);
    }
    let mut out = Vec::new();
    for path in paths {
        if path == std::path::Path::new("-") {
            io::stdin().read_to_end(&mut out)?;
        } else {
            let bytes = fs::read(path).map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    CliError::Stderr {
                        code: 128,
                        text: format!(
                            "error: can't open patch '{}': No such file or directory\n",
                            path.display()
                        ),
                    }
                } else {
                    CliError::Io(error)
                }
            })?;
            out.extend_from_slice(&bytes);
        }
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
    Ok(out)
}

fn parse_apply_file_patch(lines: &[&[u8]], start: usize) -> Result<(ApplyFilePatch, usize)> {
    let (mut old_path, mut new_path) = parse_diff_git_paths(lines[start])?;
    let mut new_mode = None;
    let mut rename = false;
    let mut deleted = false;
    let mut binary = None;
    let mut hunks = Vec::new();
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = trim_patch_line(lines[cursor]);
        if line.starts_with(b"diff --git ") {
            break;
        }
        if line.starts_with(b"GIT binary patch") {
            let (patch, next) = parse_apply_binary_patch(lines, cursor + 1)?;
            binary = Some(patch);
            cursor = next;
            continue;
        }
        if line.starts_with(b"Binary files ") {
            return Err(CliError::Fatal {
                code: 128,
                message: "binary patch payload is required".into(),
            });
        } else if let Some(path) = line.strip_prefix(b"rename from ") {
            old_path = Some(parse_apply_rename_path(path)?);
            rename = true;
        } else if let Some(path) = line.strip_prefix(b"rename to ") {
            new_path = Some(parse_apply_rename_path(path)?);
            rename = true;
        } else if let Some(mode) = line.strip_prefix(b"new file mode ") {
            new_mode = Some(parse_index_mode_bytes(mode)?);
        } else if let Some(mode) = line.strip_prefix(b"new mode ") {
            new_mode = Some(parse_index_mode_bytes(mode)?);
        } else if line.starts_with(b"deleted file mode ") {
            deleted = true;
        } else if let Some(path) = line.strip_prefix(b"--- ") {
            old_path = parse_apply_header_path(path)?;
        } else if let Some(path) = line.strip_prefix(b"+++ ") {
            new_path = parse_apply_header_path(path)?;
        } else if line.starts_with(b"@@ ") {
            let (hunk, next) = parse_apply_hunk(lines, cursor)?;
            hunks.push(hunk);
            cursor = next;
            continue;
        }
        cursor += 1;
    }
    if hunks.is_empty() && binary.is_none() && !rename && new_mode.is_none() && !deleted {
        return Err(CliError::Fatal {
            code: 128,
            message: "No valid patches in input".into(),
        });
    }
    let deleted = deleted || new_path.is_none();
    Ok((
        ApplyFilePatch {
            old_path,
            new_path,
            new_mode,
            rename,
            deleted,
            binary,
            hunks,
        },
        cursor,
    ))
}

fn parse_diff_git_paths(line: &[u8]) -> Result<ApplyPaths> {
    let line = std::str::from_utf8(trim_patch_line(line)).map_err(|_| CliError::Fatal {
        code: 128,
        message: "diff header path is not valid UTF-8".into(),
    })?;
    let rest = line
        .strip_prefix("diff --git a/")
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "diff header is malformed".into(),
        })?;
    let (old, new) = rest.split_once(" b/").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "diff header is malformed".into(),
    })?;
    Ok((
        Some(normalize_git_path(old)?.into_bytes()),
        Some(normalize_git_path(new)?.into_bytes()),
    ))
}

fn parse_apply_header_path(path: &[u8]) -> Result<Option<Vec<u8>>> {
    let path = std::str::from_utf8(path).map_err(|_| CliError::Fatal {
        code: 128,
        message: "patch path is not valid UTF-8".into(),
    })?;
    if path == "/dev/null" {
        return Ok(None);
    }
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    let normalized = normalize_git_path(path)?;
    if normalized.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "patch path is empty".into(),
        });
    }
    Ok(Some(normalized.into_bytes()))
}

fn parse_apply_rename_path(path: &[u8]) -> Result<Vec<u8>> {
    let path = std::str::from_utf8(path).map_err(|_| CliError::Fatal {
        code: 128,
        message: "rename path is not valid UTF-8".into(),
    })?;
    let normalized = normalize_git_path(path)?;
    if normalized.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "rename path is empty".into(),
        });
    }
    Ok(normalized.into_bytes())
}

fn parse_apply_binary_patch(lines: &[&[u8]], start: usize) -> Result<(ApplyBinaryPatch, usize)> {
    let (forward, cursor) = parse_apply_binary_record(lines, start)?;
    let (reverse, cursor) = parse_apply_binary_record(lines, cursor)?;
    Ok((ApplyBinaryPatch { forward, reverse }, cursor))
}

fn parse_apply_binary_record(lines: &[&[u8]], start: usize) -> Result<(ApplyBinaryRecord, usize)> {
    let header = trim_patch_line(lines.get(start).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "truncated binary patch".into(),
    })?);
    let header = std::str::from_utf8(header).map_err(|_| CliError::Fatal {
        code: 128,
        message: "binary patch header is not valid UTF-8".into(),
    })?;
    let (record_kind, size) = if let Some(value) = header.strip_prefix("literal ") {
        ("literal", value)
    } else if let Some(value) = header.strip_prefix("delta ") {
        ("delta", value)
    } else {
        return Err(CliError::Fatal {
            code: 128,
            message: "binary patch record must be literal or delta".into(),
        });
    };
    let size = size.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: "binary patch record size is invalid".into(),
    })?;
    let mut encoded = Vec::new();
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = trim_patch_line(lines[cursor]);
        if line.is_empty() {
            cursor += 1;
            break;
        }
        if line.starts_with(b"literal ")
            || line.starts_with(b"delta ")
            || line.starts_with(b"diff --git ")
        {
            break;
        }
        encoded.extend_from_slice(line);
        cursor += 1;
    }
    let compressed = decode_git_base85(&encoded)?;
    let mut decoder = ZlibDecoder::new(compressed.as_slice());
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded)?;
    if decoded.len() != size {
        return Err(CliError::Fatal {
            code: 128,
            message: "binary patch record size mismatch".into(),
        });
    }
    let record = match record_kind {
        "literal" => ApplyBinaryRecord::Literal(decoded),
        "delta" => ApplyBinaryRecord::Delta(decoded),
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown binary patch record kind '{record_kind}'"),
            });
        }
    };
    Ok((record, cursor))
}

fn decode_git_base85(encoded: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor < encoded.len() {
        let len = git_base85_line_length(encoded[cursor]).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "invalid binary patch line length".into(),
        })?;
        cursor += 1;
        let encoded_len = len.div_ceil(4) * 5;
        let end = cursor
            .checked_add(encoded_len)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "binary patch line is too long".into(),
            })?;
        if end > encoded.len() {
            return Err(CliError::Fatal {
                code: 128,
                message: "truncated binary patch line".into(),
            });
        }
        let mut produced = Vec::with_capacity(encoded_len / 5 * 4);
        for chunk in encoded[cursor..end].chunks_exact(5) {
            let mut value = 0u32;
            for byte in chunk {
                let digit = u32::from(git_base85_value(*byte)?);
                value = value
                    .checked_mul(85)
                    .and_then(|value| value.checked_add(digit))
                    .ok_or_else(|| CliError::Fatal {
                        code: 128,
                        message: "invalid binary patch base85 value".into(),
                    })?;
            }
            produced.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&produced[..len]);
        cursor = end;
    }
    Ok(out)
}

fn git_base85_line_length(byte: u8) -> Option<usize> {
    match byte {
        b'A'..=b'Z' => Some((byte - b'A' + 1) as usize),
        b'a'..=b'z' => Some((byte - b'a' + 27) as usize),
        _ => None,
    }
}

fn git_base85_value(byte: u8) -> Result<u8> {
    const ALPHABET: &[u8; 85] =
        b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";
    ALPHABET
        .iter()
        .position(|candidate| *candidate == byte)
        .map(|idx| idx as u8)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "invalid binary patch base85 character".into(),
        })
}

fn parse_apply_hunk(lines: &[&[u8]], start: usize) -> Result<(ApplyHunk, usize)> {
    let header =
        std::str::from_utf8(trim_patch_line(lines[start])).map_err(|_| CliError::Fatal {
            code: 128,
            message: "hunk header is not valid UTF-8".into(),
        })?;
    let (old_start, old_count, new_start, new_count) = parse_apply_hunk_header(header)?;
    let mut hunk_lines = Vec::new();
    let mut cursor = start + 1;
    let mut seen_old = 0usize;
    let mut seen_new = 0usize;
    while cursor < lines.len() {
        let line = lines[cursor];
        let trimmed = trim_patch_line(line);
        if seen_old == old_count && seen_new == new_count {
            if trimmed == b"\\ No newline at end of file" {
                apply_no_newline_marker(&mut hunk_lines)?;
                cursor += 1;
            }
            break;
        }
        if trimmed.starts_with(b"diff --git ") || trimmed.starts_with(b"@@ ") {
            break;
        }
        match line.first().copied() {
            Some(b' ') => {
                hunk_lines.push(ApplyHunkLine::Context(line[1..].to_vec()));
                seen_old += 1;
                seen_new += 1;
            }
            Some(b'-') => {
                hunk_lines.push(ApplyHunkLine::Delete(line[1..].to_vec()));
                seen_old += 1;
            }
            Some(b'+') => {
                hunk_lines.push(ApplyHunkLine::Insert(line[1..].to_vec()));
                seen_new += 1;
            }
            Some(b'\\') if trimmed == b"\\ No newline at end of file" => {
                apply_no_newline_marker(&mut hunk_lines)?;
            }
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "patch hunk line is malformed".into(),
                });
            }
        }
        cursor += 1;
    }
    Ok((
        ApplyHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines: hunk_lines,
        },
        cursor,
    ))
}

fn parse_apply_hunk_header(header: &str) -> Result<(usize, usize, usize, usize)> {
    let rest = header.strip_prefix("@@ -").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "hunk header is malformed".into(),
    })?;
    let (old_range, rest) = rest.split_once(" +").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "hunk header is malformed".into(),
    })?;
    let (new_range, _) = rest.split_once(" @@").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "hunk header is malformed".into(),
    })?;
    let (old_start, old_count) = parse_apply_hunk_range(old_range)?;
    let (new_start, new_count) = parse_apply_hunk_range(new_range)?;
    Ok((old_start, old_count, new_start, new_count))
}

fn parse_apply_hunk_range(range: &str) -> Result<(usize, usize)> {
    let (start, count) = match range.split_once(',') {
        Some((start, count)) => (
            start,
            count.parse().map_err(|_| CliError::Fatal {
                code: 128,
                message: "hunk range count is invalid".into(),
            })?,
        ),
        None => (range, 1),
    };
    let start = start.parse().map_err(|_| CliError::Fatal {
        code: 128,
        message: "hunk range start is invalid".into(),
    })?;
    Ok((start, count))
}

fn apply_no_newline_marker(lines: &mut [ApplyHunkLine]) -> Result<()> {
    let Some(line) = lines.last_mut() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "no-newline marker has no preceding hunk line".into(),
        });
    };
    let bytes = match line {
        ApplyHunkLine::Context(bytes)
        | ApplyHunkLine::Delete(bytes)
        | ApplyHunkLine::Insert(bytes) => bytes,
    };
    if bytes.ends_with(b"\n") {
        bytes.pop();
    }
    Ok(())
}

fn apply_binary_record(base: &[u8], record: &ApplyBinaryRecord, path: &[u8]) -> Result<Vec<u8>> {
    match record {
        ApplyBinaryRecord::Literal(content) => Ok(content.clone()),
        ApplyBinaryRecord::Delta(delta) => apply_git_binary_delta(base, delta, path),
    }
}

fn apply_git_binary_delta(base: &[u8], delta: &[u8], path: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = 0usize;
    let source_size = read_git_delta_size(delta, &mut cursor)?;
    if source_size != base.len() {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "{}: binary patch source size mismatch",
                String::from_utf8_lossy(path)
            ),
        });
    }
    let target_size = read_git_delta_size(delta, &mut cursor)?;
    let mut out = Vec::with_capacity(target_size);
    while cursor < delta.len() {
        let opcode = delta[cursor];
        cursor += 1;
        if opcode & 0x80 != 0 {
            let copy_offset = read_git_delta_copy_value(delta, &mut cursor, opcode, 0x0f)?;
            let mut copy_size = read_git_delta_copy_value(delta, &mut cursor, opcode >> 4, 0x07)?;
            if copy_size == 0 {
                copy_size = 0x10000;
            }
            let copy_end = copy_offset
                .checked_add(copy_size)
                .ok_or_else(|| malformed_binary_delta(path))?;
            if copy_end > base.len() {
                return Err(malformed_binary_delta(path));
            }
            out.extend_from_slice(&base[copy_offset..copy_end]);
        } else if opcode != 0 {
            let insert_size = usize::from(opcode);
            let insert_end = cursor
                .checked_add(insert_size)
                .ok_or_else(|| malformed_binary_delta(path))?;
            if insert_end > delta.len() {
                return Err(malformed_binary_delta(path));
            }
            out.extend_from_slice(&delta[cursor..insert_end]);
            cursor = insert_end;
        } else {
            return Err(malformed_binary_delta(path));
        }
        if out.len() > target_size {
            return Err(malformed_binary_delta(path));
        }
    }
    if out.len() != target_size {
        return Err(malformed_binary_delta(path));
    }
    Ok(out)
}

fn read_git_delta_size(delta: &[u8], cursor: &mut usize) -> Result<usize> {
    let mut size = 0usize;
    let mut shift = 0usize;
    loop {
        let byte = *delta.get(*cursor).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "binary patch delta is truncated".into(),
        })?;
        *cursor += 1;
        size |= usize::from(byte & 0x7f)
            .checked_shl(shift as u32)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "binary patch delta size is too large".into(),
            })?;
        if byte & 0x80 == 0 {
            return Ok(size);
        }
        shift = shift.checked_add(7).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "binary patch delta size is too large".into(),
        })?;
    }
}

fn read_git_delta_copy_value(
    delta: &[u8],
    cursor: &mut usize,
    flags: u8,
    mask: u8,
) -> Result<usize> {
    let mut value = 0usize;
    let mut shift = 0usize;
    for bit in 0..8 {
        let flag = 1_u8 << bit;
        if mask & flag != 0 && flags & flag != 0 {
            let byte = *delta.get(*cursor).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "binary patch delta is truncated".into(),
            })?;
            *cursor += 1;
            value |= usize::from(byte) << shift;
        }
        shift += 8;
    }
    Ok(value)
}

fn malformed_binary_delta(path: &[u8]) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "{}: binary patch delta is malformed",
            String::from_utf8_lossy(path)
        ),
    }
}

fn read_apply_worktree_content(repo: &GitRepo, path: &[u8]) -> Result<Vec<u8>> {
    let absolute = repo.root.join(String::from_utf8_lossy(path).as_ref());
    match fs::read(absolute) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn apply_expect_line(
    base_lines: &[Vec<u8>],
    index: usize,
    expected: &[u8],
    path: &[u8],
) -> Result<()> {
    if base_lines
        .get(index)
        .is_some_and(|actual| actual == expected)
    {
        Ok(())
    } else {
        Err(apply_mismatch_error(path))
    }
}

fn apply_mismatch_error(path: &[u8]) -> CliError {
    CliError::Fatal {
        code: 1,
        message: format!("patch failed: {}", String::from_utf8_lossy(path)),
    }
}

#[cfg(unix)]
fn apply_worktree_mode(path: &std::path::Path, mode: IndexMode) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let bits = match mode {
        IndexMode::Executable => 0o755,
        IndexMode::File => 0o644,
        IndexMode::Symlink | IndexMode::Gitlink => return Ok(()),
    };
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(bits);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn apply_worktree_mode(_path: &std::path::Path, _mode: IndexMode) -> Result<()> {
    Ok(())
}

fn trim_patch_line(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n")
        .unwrap_or(line)
        .strip_suffix(b"\r")
        .unwrap_or_else(|| line.strip_suffix(b"\n").unwrap_or(line))
}

fn parse_index_mode_bytes(mode: &[u8]) -> Result<IndexMode> {
    let mode = std::str::from_utf8(mode).map_err(|_| CliError::Fatal {
        code: 128,
        message: "file mode is not valid UTF-8".into(),
    })?;
    parse_index_mode(mode)
}
