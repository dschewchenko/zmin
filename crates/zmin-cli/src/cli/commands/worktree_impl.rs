use super::*;
use chrono::{Datelike, Timelike};
use std::borrow::Cow;
use zmin_git_core::commit::CommitObjectCache;
use zmin_primitives::git_runtime::GitPrimitiveRuntime;

pub(crate) type TrackedPathSet<'a> = HashSet<&'a [u8]>;

pub(crate) fn tracked_path_set(index: &GitIndex) -> TrackedPathSet<'_> {
    index
        .entries()
        .iter()
        .map(|entry| entry.path.as_slice())
        .collect()
}

struct CleanOptions {
    dry_run: bool,
    force_count: usize,
    quiet: bool,
    directories: bool,
    excludes: Vec<String>,
    ignored: bool,
    ignored_only: bool,
    interactive: bool,
    paths: Vec<PathBuf>,
}

pub(crate) fn clean(args: Vec<String>) -> Result<()> {
    let options = parse_clean_args(args)?;
    let repo = find_repo_or_bare()?;
    if !options.dry_run && options.force_count == 0 && clean_require_force(&repo)? {
        return Err(CliError::Fatal {
            code: 128,
            message: "clean.requireForce is true and -f not given: refusing to clean".into(),
        });
    }

    let index = read_repo_index(&repo)?;
    let tracked_paths = tracked_path_set(&index);
    let mut ignore = GitIgnore::load_from_root(&repo.root)?;
    let extra_ignore = GitIgnore::parse(&options.excludes.join("\n"));
    if !options.ignored {
        ignore.append(extra_ignore.clone());
    }
    let pathspecs = options
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let clean_mode = if options.ignored_only {
        CleanIgnoredMode::Only
    } else if options.ignored {
        CleanIgnoredMode::All
    } else {
        CleanIgnoredMode::Normal
    };
    let mut entries = clean_untracked_files(
        &repo.root,
        &tracked_paths,
        &ignore,
        options.directories,
        clean_mode,
    )?
    .into_iter()
    .filter(|entry| options.force_count >= 2 || !clean_entry_is_nested_repo(&repo.root, entry))
    .filter(|entry| {
        clean_mode != CleanIgnoredMode::All || !clean_exclude_matches(&extra_ignore, entry)
    })
    .filter(|entry| pathspec_matches(entry, &pathspecs))
    .collect::<Vec<_>>();
    entries.sort();

    for entry in entries {
        let display = String::from_utf8_lossy(&entry);
        if options.dry_run {
            if !options.quiet {
                println!("Would remove {display}");
            }
            continue;
        }
        if !options.quiet {
            println!("Removing {display}");
        }
        if entry.ends_with(b"/") {
            let relative_dir = String::from_utf8_lossy(&entry[..entry.len() - 1]);
            fs::remove_dir_all(repo.root.join(relative_dir.as_ref()))?;
        } else {
            fs::remove_file(repo.root.join(display.as_ref()))?;
        }
    }
    Ok(())
}

fn parse_clean_args(args: Vec<String>) -> Result<CleanOptions> {
    let mut options = CleanOptions {
        dry_run: false,
        force_count: 0,
        quiet: false,
        directories: false,
        excludes: Vec::new(),
        ignored: false,
        ignored_only: false,
        interactive: false,
        paths: Vec::new(),
    };
    let mut pathspec_mode = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if pathspec_mode {
            options.paths.push(PathBuf::from(arg));
            cursor += 1;
            continue;
        }
        match arg.as_str() {
            "--" => pathspec_mode = true,
            "-n" | "--dry-run" => options.dry_run = true,
            "--no-dry-run" => options.dry_run = false,
            "-f" | "--force" => options.force_count = options.force_count.saturating_add(1),
            "--no-force" => options.force_count = 0,
            "-q" | "--quiet" => options.quiet = true,
            "--no-quiet" => options.quiet = false,
            "-i" | "--interactive" => options.interactive = true,
            "--no-interactive" => options.interactive = false,
            "-d" => options.directories = true,
            "-x" => {
                options.ignored = true;
                options.ignored_only = false;
            }
            "-X" => {
                options.ignored = false;
                options.ignored_only = true;
            }
            "-e" | "--exclude" => {
                cursor += 1;
                let Some(pattern) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "clean -e requires a pattern".into(),
                    });
                };
                options.excludes.push(pattern.clone());
            }
            value if value.starts_with("--exclude=") => {
                let Some(pattern) = value.strip_prefix("--exclude=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("unsupported clean option '{value}'"),
                    });
                };
                options.excludes.push(pattern.to_owned());
            }
            value if value.starts_with('-') && value.len() > 2 && !value.starts_with("--") => {
                parse_clean_short_cluster(value, &mut options)?;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported clean option '{value}'"),
                });
            }
            value => options.paths.push(PathBuf::from(value)),
        }
        cursor += 1;
    }
    if options.interactive {
        return Err(CliError::Fatal {
            code: 129,
            message: "unsupported clean option '--interactive'".into(),
        });
    }
    Ok(options)
}

fn clean_require_force(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "clean.requireForce")? else {
        return Ok(true);
    };
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!(
            "bad boolean config value '{}' for 'clean.requireforce'",
            entry.value
        ),
    })
}

fn parse_clean_short_cluster(value: &str, options: &mut CleanOptions) -> Result<()> {
    let mut chars = value[1..].char_indices().peekable();
    while let Some((index, flag)) = chars.next() {
        match flag {
            'n' => options.dry_run = true,
            'f' => options.force_count = options.force_count.saturating_add(1),
            'q' => options.quiet = true,
            'i' => options.interactive = true,
            'd' => options.directories = true,
            'x' => {
                options.ignored = true;
                options.ignored_only = false;
            }
            'X' => {
                options.ignored = false;
                options.ignored_only = true;
            }
            'e' => {
                let pattern_start = 1 + index + flag.len_utf8();
                if pattern_start >= value.len() {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "clean -e requires a pattern".into(),
                    });
                }
                options.excludes.push(value[pattern_start..].to_owned());
                return Ok(());
            }
            _ => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported clean option '-{flag}'"),
                });
            }
        }
        if chars.peek().is_none() {
            break;
        }
    }
    Ok(())
}

fn clean_exclude_matches(ignore: &GitIgnore, entry: &[u8]) -> bool {
    let is_dir = entry.ends_with(b"/");
    let path = if is_dir {
        &entry[..entry.len().saturating_sub(1)]
    } else {
        entry
    };
    ignore.is_ignored(path, is_dir)
}

fn clean_entry_is_nested_repo(root: &Path, entry: &[u8]) -> bool {
    if !entry.ends_with(b"/") {
        return false;
    }
    let relative_dir = String::from_utf8_lossy(&entry[..entry.len().saturating_sub(1)]);
    root.join(relative_dir.as_ref()).join(".git").exists()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanIgnoredMode {
    Normal,
    All,
    Only,
}

fn clean_untracked_files(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    directories: bool,
    mode: CleanIgnoredMode,
) -> Result<Vec<Vec<u8>>> {
    if mode == CleanIgnoredMode::Normal {
        return Ok(untracked_files(root, tracked_paths, ignore)?
            .into_iter()
            .filter(|entry| directories || !entry.ends_with(b"/"))
            .collect());
    }

    let mut files = Vec::new();
    collect_clean_untracked_files(
        root,
        root,
        tracked_paths,
        ignore,
        directories,
        mode,
        &mut files,
    )?;
    files.sort();
    Ok(files)
}

fn collect_clean_untracked_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    directories: bool,
    mode: CleanIgnoredMode,
    files: &mut Vec<Vec<u8>>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        let relative = repo_relative_path(root, &path)?;
        let is_dir = metadata.is_dir();
        let is_ignored = ignore.is_ignored(&relative, is_dir);
        if is_dir {
            if mode == CleanIgnoredMode::Only && !is_ignored {
                collect_clean_untracked_files(
                    root,
                    &path,
                    tracked_paths,
                    ignore,
                    directories,
                    mode,
                    files,
                )?;
                continue;
            }
            if tracked_paths_under(tracked_paths, &relative) {
                collect_clean_untracked_files(
                    root,
                    &path,
                    tracked_paths,
                    ignore,
                    directories,
                    mode,
                    files,
                )?;
            } else if directories {
                let mut dir = relative;
                dir.push(b'/');
                files.push(dir);
            }
        } else if (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
            && (mode == CleanIgnoredMode::All || is_ignored)
        {
            files.push(relative);
        }
    }
    Ok(())
}

pub(crate) fn status(
    porcelain: Option<&str>,
    branch: bool,
    short: bool,
    null: bool,
    ignored: Option<&str>,
    untracked_files: Option<&str>,
    pathspecs: Vec<PathBuf>,
) -> Result<()> {
    let _trace = phase_trace("status.total");
    let ignored_mode = IgnoredMode::parse(ignored)?;
    let porcelain_version = match porcelain {
        None => PorcelainVersion::V1,
        Some("v1") => PorcelainVersion::V1,
        Some("v2") if !short => PorcelainVersion::V2,
        Some(value) => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unsupported porcelain version '{value}'"),
            });
        }
    };
    let untracked_mode = UntrackedMode::parse(untracked_files)?;

    let repo = {
        let _trace = phase_trace("status.find_repo");
        find_repo_or_bare()?
    };
    if repo_is_bare(&repo) {
        return Err(CliError::Fatal {
            code: 128,
            message: "this operation must be run in a work tree".into(),
        });
    }
    let machine_readable = porcelain.is_some() || short;
    if machine_readable && branch {
        let _trace = phase_trace("status.branch_header");
        if porcelain_version == PorcelainVersion::V2 {
            for row in porcelain_v2_branch_header(&repo)? {
                write_status_record(&row, null)?;
            }
        } else {
            write_status_record(&porcelain_branch_header(&repo)?, null)?;
        }
    }

    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let head_tree = {
        let _trace = phase_trace("status.read_head_tree");
        read_head_tree_id_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?
    };
    let index = if repo.index_path.exists() {
        let _trace = phase_trace("status.read_index");
        read_index(&repo.index_path)?
    } else {
        GitIndex::new()
    };
    let pathspecs = {
        let _trace = phase_trace("status.pathspecs");
        pathspecs
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?
    };

    let unmerged_paths = {
        let _trace = phase_trace("status.unmerged_paths");
        merge_index_unmerged_paths(&index)
            .into_iter()
            .collect::<HashSet<_>>()
    };
    let status_index = if unmerged_paths.is_empty() {
        Cow::Borrowed(&index)
    } else {
        let _trace = phase_trace("status.stage_zero_index");
        Cow::Owned(stage_zero_index(&index)?)
    };
    let status_index = status_index.as_ref();

    let mut paths: HashMap<Vec<u8>, (char, char)> = HashMap::new();
    for path in &unmerged_paths {
        paths.insert(path.clone(), status_unmerged_code(&index, path));
    }
    {
        let _trace = phase_trace("status.head_index_diff");
        for entry in status_head_index_diff(
            runtime.object_store_adapter(),
            head_tree.as_ref(),
            status_index,
        )? {
            if unmerged_paths.contains::<[u8]>(entry.path.as_slice()) {
                continue;
            }
            paths.entry(entry.path).or_insert((' ', ' ')).0 = status_code(entry.status);
        }
    }
    {
        let _trace = phase_trace("status.worktree_status");
        for (path, code) in worktree_status(&repo, status_index)? {
            if unmerged_paths.contains(&path) {
                continue;
            }
            paths.entry(path).or_insert((' ', ' ')).1 = code;
        }
    }

    let tracked_paths = tracked_path_set(&index);
    let untracked = if untracked_mode == UntrackedMode::No {
        Vec::new()
    } else {
        let _trace = phase_trace("status.untracked");
        let ignore = status_excludes(&repo)?;
        untracked_files_with_mode(&repo.root, &tracked_paths, &ignore, untracked_mode)?
    };
    let ignored = if ignored_mode == IgnoredMode::No {
        Vec::new()
    } else {
        let _trace = phase_trace("status.ignored");
        let ignore = status_excludes(&repo)?;
        ignored_untracked_files_for_status(&repo.root, &tracked_paths, &ignore)?
    };
    if !machine_readable {
        let _trace = phase_trace("status.render_human");
        return print_human_status(&repo, &paths, &untracked, &ignored, untracked_mode);
    }

    {
        let _trace = phase_trace("status.render_porcelain");
        if porcelain_version == PorcelainVersion::V2 {
            return print_porcelain_v2_status(
                &paths,
                status_index,
                runtime.object_store_adapter(),
                head_tree.as_ref(),
                &untracked,
                &ignored,
                &pathspecs,
                null,
            );
        }
        let mut rows = paths
            .into_iter()
            .filter(|(path, _)| pathspecs.is_empty() || pathspec_matches(path, &pathspecs))
            .map(|(path, (index_status, worktree_status))| {
                (
                    path.clone(),
                    format!(
                        "{}{} {}",
                        index_status,
                        worktree_status,
                        String::from_utf8_lossy(&path)
                    ),
                )
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        for (_, row) in rows {
            write_status_record(&row, null)?;
        }
        for path in untracked
            .into_iter()
            .filter(|path| pathspecs.is_empty() || pathspec_matches(path, &pathspecs))
        {
            write_status_record(&format!("?? {}", String::from_utf8_lossy(&path)), null)?;
        }
        for path in ignored
            .into_iter()
            .filter(|path| pathspecs.is_empty() || pathspec_matches(path, &pathspecs))
        {
            write_status_record(&format!("!! {}", String::from_utf8_lossy(&path)), null)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PorcelainVersion {
    V1,
    V2,
}

fn print_porcelain_v2_status(
    paths: &HashMap<Vec<u8>, (char, char)>,
    index: &GitIndex,
    store: &dyn GitObjectStore,
    head_tree: Option<&ObjectId>,
    untracked: &[Vec<u8>],
    ignored: &[Vec<u8>],
    pathspecs: &[Vec<u8>],
    null: bool,
) -> Result<()> {
    let mut rows = paths
        .iter()
        .filter(|(path, _)| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
        .map(|(path, (index_status, worktree_status))| {
            let metadata = status_v2_metadata(store, head_tree, index, path, *worktree_status)?;
            Ok((
                path.clone(),
                format!(
                    "1 {}{} N... {} {} {} {} {} {}",
                    status_v2_code(*index_status),
                    status_v2_code(*worktree_status),
                    metadata.head_mode,
                    metadata.index_mode,
                    metadata.worktree_mode,
                    metadata.head_id,
                    metadata.index_id,
                    String::from_utf8_lossy(path)
                ),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    rows.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    for (_, row) in rows {
        write_status_record(&row, null)?;
    }
    for path in untracked
        .iter()
        .filter(|path| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
    {
        write_status_record(&format!("? {}", String::from_utf8_lossy(path)), null)?;
    }
    for path in ignored
        .iter()
        .filter(|path| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
    {
        write_status_record(&format!("! {}", String::from_utf8_lossy(path)), null)?;
    }
    Ok(())
}

struct StatusV2Metadata {
    head_mode: &'static str,
    index_mode: &'static str,
    worktree_mode: &'static str,
    head_id: String,
    index_id: String,
}

fn status_v2_metadata(
    store: &dyn GitObjectStore,
    head_tree: Option<&ObjectId>,
    index: &GitIndex,
    path: &[u8],
    worktree_status: char,
) -> Result<StatusV2Metadata> {
    let head = match head_tree {
        Some(tree) => find_tree_entry(store, tree, path)?,
        None => None,
    };
    let entry = status_find_index_entry(index, path);
    let head_mode = head
        .as_ref()
        .map(|entry| status_v2_tree_mode(entry.mode))
        .unwrap_or("000000");
    let index_mode = entry
        .map(|entry| status_v2_mode(entry.mode))
        .unwrap_or("000000");
    let worktree_mode = if worktree_status == 'D' {
        "000000"
    } else {
        index_mode
    };
    let head_id = head
        .as_ref()
        .map(|entry| entry.id.to_string())
        .unwrap_or_else(|| status_zero_object_id().to_owned());
    let index_id = entry
        .map(|entry| entry.id.to_string())
        .unwrap_or_else(|| status_zero_object_id().to_owned());
    Ok(StatusV2Metadata {
        head_mode,
        index_mode,
        worktree_mode,
        head_id,
        index_id,
    })
}

fn write_status_record(row: &str, null: bool) -> Result<()> {
    if null {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(row.as_bytes())?;
        stdout.write_all(b"\0")?;
    } else {
        println!("{row}");
    }
    Ok(())
}

fn status_v2_mode(mode: IndexMode) -> &'static str {
    match mode {
        IndexMode::File => "100644",
        IndexMode::Executable => "100755",
        IndexMode::Symlink => "120000",
        IndexMode::Gitlink => "160000",
    }
}

fn status_v2_tree_mode(mode: TreeMode) -> &'static str {
    match mode {
        TreeMode::File => "100644",
        TreeMode::Executable => "100755",
        TreeMode::Symlink => "120000",
        TreeMode::Gitlink => "160000",
        TreeMode::Tree => "040000",
    }
}

fn status_v2_code(code: char) -> char {
    if code == ' ' { '.' } else { code }
}

fn status_zero_object_id() -> &'static str {
    "0000000000000000000000000000000000000000"
}

fn porcelain_v2_branch_header(repo: &GitRepo) -> Result<Vec<String>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head = refs.read_head()?;
    let mut rows = Vec::new();
    let oid = refs
        .resolve("HEAD")
        .map(|id| id.to_string())
        .unwrap_or_else(|_| "(initial)".to_owned());
    rows.push(format!("# branch.oid {oid}"));
    match head {
        RefTarget::Symbolic(target) if target.starts_with("refs/heads/") => {
            let branch = target.strip_prefix("refs/heads/").unwrap_or(&target);
            rows.push(format!("# branch.head {branch}"));
            if let Some(upstream) = read_branch_upstream(repo, branch)? {
                rows.push(format!("# branch.upstream {}", upstream.display));
                if let Some((ahead, behind)) = upstream_counts(repo, &upstream.ref_name)? {
                    rows.push(format!("# branch.ab +{ahead} -{behind}"));
                }
            }
        }
        RefTarget::Direct(_) => rows.push("# branch.head (detached)".to_owned()),
        RefTarget::Symbolic(target) => rows.push(format!(
            "# branch.head {}",
            target
                .strip_prefix("refs/")
                .unwrap_or(&target)
                .strip_prefix("heads/")
                .unwrap_or(target.as_str())
        )),
    }
    Ok(rows)
}

fn status_excludes(repo: &GitRepo) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if let Some(path) = ls_files_global_excludes_file(repo)? {
        append_ignore_file(&mut ignore, &path, "")?;
    }
    append_ignore_file(&mut ignore, &repo.git_dir.join("info/exclude"), "")?;
    append_per_directory_excludes(&repo.root, &repo.root, ".gitignore", &mut ignore)?;
    Ok(ignore)
}

fn status_head_index_diff<S>(
    store: &S,
    head_tree: Option<&ObjectId>,
    index: &GitIndex,
) -> Result<Vec<IndexDiffEntry>>
where
    S: GitObjectStore,
{
    let Some(head_tree) = head_tree else {
        return Ok(index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0)
            .map(|entry| IndexDiffEntry {
                status: IndexDiffStatus::Added,
                path: entry.path.to_vec(),
                old_path: None,
                similarity: None,
            })
            .collect());
    };
    let tree_cache = TreeObjectCache::new(store);
    let mut seen = HashSet::new();
    let mut diff = Vec::new();
    let mut path = Vec::new();
    status_head_tree_diff(
        &tree_cache,
        head_tree,
        index,
        &mut path,
        &mut seen,
        &mut diff,
    )?;
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        if !seen.contains(entry.path.as_slice()) {
            diff.push(IndexDiffEntry {
                status: IndexDiffStatus::Added,
                path: entry.path.to_vec(),
                old_path: None,
                similarity: None,
            });
        }
    }
    diff.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(diff)
}

fn status_head_tree_diff<S>(
    tree_cache: &TreeObjectCache<'_, S>,
    tree_id: &ObjectId,
    index: &GitIndex,
    path: &mut Vec<u8>,
    seen: &mut HashSet<Vec<u8>>,
    diff: &mut Vec<IndexDiffEntry>,
) -> Result<()>
where
    S: GitObjectStore,
    S: ?Sized,
{
    for entry in tree_cache.read_tree(tree_id)?.iter() {
        let original_len = path.len();
        path.extend_from_slice(&entry.name);
        match entry.mode {
            TreeMode::Tree => {
                path.push(b'/');
                status_head_tree_diff(tree_cache, &entry.id, index, path, seen, diff)?;
            }
            TreeMode::File | TreeMode::Executable | TreeMode::Symlink | TreeMode::Gitlink => {
                seen.insert(path.clone());
                match status_find_index_entry(index, path) {
                    Some(index_entry)
                        if index_entry.id == entry.id
                            && index_entry.mode
                                == index_mode_from_tree_mode_for_status(entry.mode) => {}
                    Some(_) => diff.push(IndexDiffEntry {
                        status: IndexDiffStatus::Modified,
                        path: path.clone(),
                        old_path: None,
                        similarity: None,
                    }),
                    None => diff.push(IndexDiffEntry {
                        status: IndexDiffStatus::Deleted,
                        path: path.clone(),
                        old_path: None,
                        similarity: None,
                    }),
                }
            }
        }
        path.truncate(original_len);
    }
    Ok(())
}

fn status_find_index_entry<'a>(index: &'a GitIndex, path: &[u8]) -> Option<&'a IndexEntry> {
    let entries = index.entries();
    let mut left = 0usize;
    let mut right = entries.len();
    while left < right {
        let mid = left + (right - left) / 2;
        match entries[mid].path.as_slice().cmp(path) {
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Greater => right = mid,
            std::cmp::Ordering::Equal => {
                let mut idx = mid;
                while idx > 0 && entries[idx - 1].path.as_slice() == path {
                    idx -= 1;
                }
                return entries[idx..]
                    .iter()
                    .take_while(|entry| entry.path.as_slice() == path)
                    .find(|entry| entry.stage == 0);
            }
        }
    }
    None
}

fn index_mode_from_tree_mode_for_status(mode: TreeMode) -> IndexMode {
    match mode {
        TreeMode::File => IndexMode::File,
        TreeMode::Executable => IndexMode::Executable,
        TreeMode::Symlink => IndexMode::Symlink,
        TreeMode::Gitlink => IndexMode::Gitlink,
        TreeMode::Tree => IndexMode::File,
    }
}

fn status_unmerged_code(index: &GitIndex, path: &[u8]) -> (char, char) {
    match merge_index_stages(index, path) {
        (Some(_), Some(_), Some(_)) => ('U', 'U'),
        (Some(_), Some(_), None) => ('U', 'D'),
        (Some(_), None, Some(_)) => ('D', 'U'),
        (None, Some(_), Some(_)) => ('A', 'A'),
        _ => ('U', 'U'),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UntrackedMode {
    No,
    Normal,
    All,
    Directory,
}

impl UntrackedMode {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("normal") => Ok(Self::Normal),
            Some("no") => Ok(Self::No),
            Some("all") => Ok(Self::All),
            Some(value) => Err(CliError::Fatal {
                code: 129,
                message: format!("unsupported untracked-files mode: {value}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IgnoredMode {
    No,
    Traditional,
    Matching,
}

impl IgnoredMode {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("no") => Ok(Self::No),
            Some("traditional") => Ok(Self::Traditional),
            Some("matching") => Ok(Self::Matching),
            Some(value) => Err(CliError::Fatal {
                code: 128,
                message: format!("Invalid ignored mode '{value}'"),
            }),
        }
    }
}

fn print_human_status(
    repo: &GitRepo,
    paths: &HashMap<Vec<u8>, (char, char)>,
    untracked: &[Vec<u8>],
    ignored: &[Vec<u8>],
    untracked_mode: UntrackedMode,
) -> Result<()> {
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(&common_git_dir, GitHashAlgorithm::Sha1);
    let head_has_commit = refs.resolve("HEAD").is_ok();
    println!("{}", human_status_branch_header(&refs)?);
    if !head_has_commit {
        println!();
        println!("No commits yet");
    }
    let mut printed_body = !head_has_commit;
    if head_has_commit && let Some(lines) = human_status_upstream(repo, &refs)? {
        for line in lines {
            println!("{line}");
        }
        printed_body = true;
    }

    let mut staged = paths
        .iter()
        .filter(|(_, (index_status, _))| *index_status != ' ')
        .map(|(path, (index_status, _))| (path.clone(), *index_status))
        .collect::<Vec<_>>();
    let mut worktree = paths
        .iter()
        .filter(|(_, (_, worktree_status))| *worktree_status != ' ')
        .map(|(path, (_, worktree_status))| (path.clone(), *worktree_status))
        .collect::<Vec<_>>();
    staged.sort_by(|left, right| left.0.cmp(&right.0));
    worktree.sort_by(|left, right| left.0.cmp(&right.0));

    if !staged.is_empty() {
        if printed_body {
            println!();
        }
        println!("Changes to be committed:");
        if head_has_commit {
            println!("  (use \"git restore --staged <file>...\" to unstage)");
        } else {
            println!("  (use \"git rm --cached <file>...\" to unstage)");
        }
        for (path, status) in &staged {
            println!(
                "\t{:<12}{}",
                human_status_label(*status),
                String::from_utf8_lossy(path)
            );
        }
        printed_body = true;
    }

    if !worktree.is_empty() {
        if printed_body {
            println!();
        }
        println!("Changes not staged for commit:");
        if worktree.iter().any(|(_, status)| *status == 'D') {
            println!("  (use \"git add/rm <file>...\" to update what will be committed)");
        } else {
            println!("  (use \"git add <file>...\" to update what will be committed)");
        }
        println!("  (use \"git restore <file>...\" to discard changes in working directory)");
        for (path, status) in &worktree {
            println!(
                "\t{:<12}{}",
                human_status_label(*status),
                String::from_utf8_lossy(path)
            );
        }
        printed_body = true;
    }

    if !untracked.is_empty() {
        if printed_body {
            println!();
        }
        println!("Untracked files:");
        println!("  (use \"git add <file>...\" to include in what will be committed)");
        for path in untracked {
            println!("\t{}", String::from_utf8_lossy(path));
        }
        printed_body = true;
    }

    if !ignored.is_empty() {
        if printed_body {
            println!();
        }
        println!("Ignored files:");
        println!("  (use \"git add -f <file>...\" to include in what will be committed)");
        for path in ignored {
            println!("\t{}", String::from_utf8_lossy(path));
        }
        printed_body = true;
    }

    if staged.is_empty() {
        if printed_body {
            println!();
        }
        if !worktree.is_empty() {
            println!("no changes added to commit (use \"git add\" and/or \"git commit -a\")");
        } else if !untracked.is_empty() {
            println!(
                "nothing added to commit but untracked files present (use \"git add\" to track)"
            );
        } else if head_has_commit && untracked_mode == UntrackedMode::No {
            println!("nothing to commit (use -u to show untracked files)");
        } else if head_has_commit {
            println!("nothing to commit, working tree clean");
        } else {
            println!("nothing to commit (create/copy files and use \"git add\" to track)");
        }
    } else if worktree.is_empty() && untracked.is_empty() {
        println!();
    }
    Ok(())
}

fn human_status_branch_header(refs: &RefStore) -> Result<String> {
    match refs.read_head()? {
        RefTarget::Symbolic(target) if target.starts_with("refs/heads/") => Ok(format!(
            "On branch {}",
            target.strip_prefix("refs/heads/").unwrap_or(&target)
        )),
        RefTarget::Direct(id) => Ok(format!("HEAD detached at {}", short_object_id(&id))),
        RefTarget::Symbolic(target) => Ok(format!(
            "On branch {}",
            target
                .strip_prefix("refs/")
                .unwrap_or(&target)
                .strip_prefix("heads/")
                .unwrap_or(target.as_str())
        )),
    }
}

fn human_status_upstream(repo: &GitRepo, refs: &RefStore) -> Result<Option<Vec<String>>> {
    let Some(current) = current_branch_ref(refs)? else {
        return Ok(None);
    };
    let branch = branch_display_name(&current);
    let Some(upstream) = read_branch_upstream(repo, &branch)? else {
        return Ok(None);
    };
    let Some((ahead, behind)) = upstream_counts(repo, &upstream.ref_name)? else {
        return Ok(None);
    };
    let mut lines = Vec::new();
    match (ahead, behind) {
        (0, 0) => lines.push(format!(
            "Your branch is up to date with '{}'.",
            upstream.display
        )),
        (ahead, 0) => {
            lines.push(format!(
                "Your branch is ahead of '{}' by {} {}.",
                upstream.display,
                ahead,
                plural(ahead, "commit", "commits")
            ));
            lines.push("  (use \"git push\" to publish your local commits)".to_owned());
        }
        (0, behind) => {
            lines.push(format!(
                "Your branch is behind '{}' by {} {}, and can be fast-forwarded.",
                upstream.display,
                behind,
                plural(behind, "commit", "commits")
            ));
            lines.push("  (use \"git pull\" to update your local branch)".to_owned());
        }
        (ahead, behind) => {
            lines.push(format!(
                "Your branch and '{}' have diverged,",
                upstream.display
            ));
            lines.push(format!(
                "and have {} and {} different {} each, respectively.",
                ahead,
                behind,
                plural(ahead + behind, "commit", "commits")
            ));
            lines.push(
                "  (use \"git pull\" if you want to integrate the remote branch with yours)"
                    .to_owned(),
            );
        }
    }
    Ok(Some(lines))
}

pub(crate) fn human_status_label(status: char) -> &'static str {
    match status {
        'A' => "new file:",
        'D' => "deleted:",
        'M' => "modified:",
        _ => "changed:",
    }
}

pub(crate) fn untracked_files(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<Vec<Vec<u8>>> {
    untracked_files_with_mode(root, tracked_paths, ignore, UntrackedMode::Normal)
}

pub(crate) fn ignored_untracked_files(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    collect_ignored_untracked_files(root, root, tracked_paths, ignore, false, &mut files)?;
    files.sort();
    Ok(files)
}

pub(crate) fn ignored_untracked_files_for_status(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    collect_ignored_untracked_status(root, root, tracked_paths, ignore, false, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_ignored_untracked_status(
    root: &std::path::Path,
    dir: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    parent_ignored: bool,
    files: &mut Vec<Vec<u8>>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = entry.metadata()?;
        let relative = repo_relative_path(root, &path)?;
        let is_ignored = parent_ignored || ignore.is_ignored(&relative, metadata.is_dir());
        if metadata.is_dir() {
            if is_ignored && !tracked_paths_under(tracked_paths, &relative) {
                let mut dir = relative;
                dir.push(b'/');
                files.push(dir);
            } else {
                collect_ignored_untracked_status(
                    root,
                    &path,
                    tracked_paths,
                    ignore,
                    is_ignored,
                    files,
                )?;
            }
        } else if is_ignored
            && (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            files.push(relative);
        }
    }
    Ok(())
}

pub(crate) fn killed_files(
    repo: &GitRepo,
    index: &GitIndex,
    directory: bool,
) -> Result<Vec<Vec<u8>>> {
    let tracked_paths = tracked_path_set(index);
    let mut killed = BTreeSet::new();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        let full_path = repo
            .root
            .join(String::from_utf8_lossy(&entry.path).as_ref());
        if let Ok(metadata) = fs::symlink_metadata(&full_path)
            && metadata.is_dir()
            && !matches!(entry.mode, IndexMode::Gitlink)
        {
            if directory {
                let mut path = entry.path.to_vec();
                path.push(b'/');
                killed.insert(path);
            } else {
                collect_killed_files_under_dir(
                    &repo.root,
                    &full_path,
                    &tracked_paths,
                    &mut killed,
                )?;
            }
        }
        for ancestor in index_path_ancestors(&entry.path) {
            let ancestor_path = repo.root.join(String::from_utf8_lossy(&ancestor).as_ref());
            match fs::symlink_metadata(ancestor_path) {
                Ok(metadata) if !metadata.is_dir() => {
                    killed.insert(ancestor);
                    break;
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(killed.into_iter().collect())
}

fn collect_killed_files_under_dir(
    root: &std::path::Path,
    dir: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    killed: &mut BTreeSet<Vec<u8>>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        let relative = repo_relative_path(root, &path)?;
        if metadata.is_dir() {
            collect_killed_files_under_dir(root, &path, tracked_paths, killed)?;
        } else if (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            killed.insert(relative);
        }
    }
    Ok(())
}

fn index_path_ancestors(path: &[u8]) -> Vec<Vec<u8>> {
    path.iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'/')
        .map(|(index, _)| path[..index].to_vec())
        .collect()
}

fn collect_ignored_untracked_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    parent_ignored: bool,
    files: &mut Vec<Vec<u8>>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = entry.metadata()?;
        let relative = repo_relative_path(root, &path)?;
        let is_ignored = parent_ignored || ignore.is_ignored(&relative, metadata.is_dir());
        if metadata.is_dir() {
            collect_ignored_untracked_files(root, &path, tracked_paths, ignore, is_ignored, files)?;
        } else if is_ignored
            && (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            files.push(relative);
        }
    }
    Ok(())
}

pub(crate) fn untracked_files_with_mode(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
) -> Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    collect_untracked_files(root, root, tracked_paths, ignore, mode, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_untracked_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    files: &mut Vec<Vec<u8>>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = entry.metadata()?;
        let relative = repo_relative_path(root, &path)?;
        if ignore.is_ignored(&relative, metadata.is_dir()) {
            continue;
        }
        if metadata.is_dir() {
            if mode == UntrackedMode::Directory && tracked_paths.contains(relative.as_slice()) {
                continue;
            }
            if mode == UntrackedMode::All || tracked_paths_under(tracked_paths, &relative) {
                collect_untracked_files(root, &path, tracked_paths, ignore, mode, files)?;
            } else if mode == UntrackedMode::Directory
                || untracked_dir_contains_reportable_file(root, &path, tracked_paths, ignore)?
            {
                let mut dir = relative;
                dir.push(b'/');
                files.push(dir);
            }
        } else if (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            files.push(relative);
        }
    }
    Ok(())
}

fn untracked_dir_contains_reportable_file(
    root: &std::path::Path,
    dir: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<bool> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = entry.metadata()?;
        let relative = repo_relative_path(root, &path)?;
        if ignore.is_ignored(&relative, metadata.is_dir()) {
            continue;
        }
        if metadata.is_dir() {
            if untracked_dir_contains_reportable_file(root, &path, tracked_paths, ignore)? {
                return Ok(true);
            }
        } else if (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn tracked_paths_under(tracked_paths: &TrackedPathSet<'_>, relative_dir: &[u8]) -> bool {
    let mut prefix = relative_dir.to_vec();
    prefix.push(b'/');
    tracked_paths
        .iter()
        .any(|path| path.starts_with(prefix.as_slice()))
}

pub(crate) fn ls_files(options: LsFilesOptions) -> Result<()> {
    let _empty_directory = options.empty_directory;
    let _sparse = options.sparse;
    let recurse_submodules = options.recurse_submodules && !options.no_recurse_submodules;
    let has_exclude_patterns = options.exclude_standard
        || !options.excludes.is_empty()
        || !options.exclude_from.is_empty()
        || options.exclude_per_directory.is_some();
    if options.ignored && !options.others && !options.cached {
        return Err(CliError::Fatal {
            code: 128,
            message: "ls-files -i must be used with either -o or -c".into(),
        });
    }
    if options.ignored && !has_exclude_patterns {
        return Err(CliError::Fatal {
            code: 128,
            message: "ls-files --ignored needs some exclude pattern".into(),
        });
    }
    if options.exclude_standard && !options.others && !(options.ignored && options.cached) {
        return Err(CliError::Fatal {
            code: 129,
            message: "--exclude-standard is only supported with --others".into(),
        });
    }
    if recurse_submodules
        && (options.others
            || options.killed
            || options.deleted
            || options.modified
            || (options.ignored && !options.cached)
            || options.unmerged
            || options.resolve_undo
            || options.with_tree.is_some())
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "ls-files --recurse-submodules unsupported mode".into(),
        });
    }
    let repo = find_repo()?;
    if options.with_tree.is_some() && (options.stage || options.unmerged) {
        return Err(CliError::Fatal {
            code: 128,
            message: "options 'ls-files --with-tree' and '-s/-u' cannot be used together".into(),
        });
    }
    let _deduplicate = options.deduplicate;
    let pathspecs = options
        .path_args
        .iter()
        .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let cwd_prefix = repo_relative_path(&repo.root, &std::env::current_dir()?)?;
    let effective_pathspecs = if pathspecs.is_empty() && !cwd_prefix.is_empty() {
        vec![cwd_prefix.clone()]
    } else {
        pathspecs
    };
    let show_stage_format = options.stage || options.unmerged;
    let cached_ignored_recurse_submodules =
        recurse_submodules && options.ignored && options.cached;
    let include_all_cached = !options.ignored
        && !options.others
        && !options.stage
        && !options.unmerged
        && !options.deleted
        && !options.modified
        && !options.killed;
    let mut index = read_repo_index(&repo)?;
    if recurse_submodules && !cached_ignored_recurse_submodules {
        index = ls_files_index_with_submodules(&repo, index)?;
    }
    let mut with_tree_paths = HashSet::new();
    if let Some(treeish) = &options.with_tree {
        let (merged_index, virtual_paths) = ls_files_index_with_tree(&repo, index, treeish)?;
        index = merged_index;
        with_tree_paths = virtual_paths;
    }
    if can_stream_plain_ls_files(&options, include_all_cached, show_stage_format) {
        let mut stdout = io::stdout().lock();
        for entry in index
            .entries()
            .iter()
            .filter(|entry| pathspec_matches(&entry.path, &effective_pathspecs))
        {
            if !write_ls_files_plain_path_record(
                &mut stdout,
                &entry.path,
                &cwd_prefix,
                options.full_name,
                options.zero,
            )? {
                continue;
            }
        }
        return Ok(());
    }
    let ignore = if ls_files_needs_ignore(&options) {
        Some(ls_files_excludes(&repo, &options)?)
    } else {
        None
    };
    let eol_store = if options.eol {
        Some(LooseObjectStore::new(
            repo.objects_dir.clone(),
            GitHashAlgorithm::Sha1,
        ))
    } else {
        None
    };
    let eol_attrs = if options.eol {
        Some(GitAttributes::load_from_root(&repo.root)?)
    } else {
        None
    };
    let other_paths = if options.others {
        let tracked_paths = tracked_path_set(&index);
        if options.ignored {
            Some(ignored_untracked_files(
                &repo.root,
                &tracked_paths,
                ignore.as_ref().expect("ignore graph for ignored others"),
            )?)
        } else if options.directory {
            Some(untracked_files_with_mode(
                &repo.root,
                &tracked_paths,
                ignore.as_ref().expect("ignore graph for directory others"),
                UntrackedMode::Directory,
            )?)
        } else {
            Some(untracked_files_with_mode(
                &repo.root,
                &tracked_paths,
                ignore.as_ref().expect("ignore graph for plain others"),
                UntrackedMode::All,
            )?)
        }
    } else {
        None
    };
    let unmatched_pathspec = if options.error_unmatch {
        ls_files_first_unmatched_pathspec(
            &index,
            other_paths.as_deref().unwrap_or(&[]),
            &effective_pathspecs,
        )
    } else {
        None
    };
    if options.format.is_some() && options.resolve_undo {
        return Err(CliError::Fatal {
            code: 129,
            message:
                "--format cannot be used with -s, -o, -k, -t, --resolve-undo, --deduplicate, --eol"
                    .into(),
        });
    }
    if options.resolve_undo && !options.stage && !options.unmerged {
        let mut stdout = io::stdout().lock();
        write_ls_files_resolve_undo_records(
            &mut stdout,
            &index,
            &effective_pathspecs,
            &cwd_prefix,
            &options,
        )?;
        if let Some(pathspec) = unmatched_pathspec {
            return Err(ls_files_error_unmatch(pathspec));
        }
        return Ok(());
    }
    if let Some(format) = &options.format {
        if options.stage
            || options.others
            || options.killed
            || options.ignored
            || options.tagged
            || options.deduplicate
            || options.eol
            || options.unmerged
            || options.lowercase_assume_valid
            || options.resolve_undo
        {
            return Err(CliError::Fatal {
                code: 129,
                message:
                    "--format cannot be used with -s, -o, -k, -t, --resolve-undo, --deduplicate, --eol"
                        .into(),
            });
        }
        let mut stdout = io::stdout().lock();
        let mut seen_paths = BTreeSet::new();
        for entry in index
            .entries()
            .iter()
            .filter(|entry| pathspec_matches(&entry.path, &effective_pathspecs))
        {
            if options.error_unmatch
                && !effective_pathspecs.is_empty()
                && !seen_paths.insert(entry.path.to_vec())
            {
                continue;
            }
            let Some(display_path) =
                ls_files_display_path(&entry.path, &cwd_prefix, options.full_name)
            else {
                continue;
            };
            let record = render_ls_files_format(format, entry, &display_path, options.abbrev)?;
            write_ls_files_record(&mut stdout, &record, options.zero)?;
            if options.debug {
                write_ls_files_debug(&mut stdout, entry)?;
            }
        }
        if let Some(pathspec) = unmatched_pathspec {
            return Err(ls_files_error_unmatch(pathspec));
        }
        return Ok(());
    }
    if options.others {
        let mut stdout = io::stdout().lock();
        for path in other_paths
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|path| pathspec_matches(path, &effective_pathspecs))
        {
            if let Some(display_path) = ls_files_display_path(path, &cwd_prefix, options.full_name)
            {
                write_ls_files_record(
                    &mut stdout,
                    &ls_files_other_record(
                        &repo,
                        path,
                        &display_path,
                        &options,
                        eol_attrs.as_ref(),
                    )?,
                    options.zero,
                )?;
            }
        }
        if !options.cached
            && !options.stage
            && !options.deleted
            && !options.modified
            && !options.killed
        {
            if let Some(pathspec) = unmatched_pathspec {
                return Err(ls_files_error_unmatch(pathspec));
            }
            return Ok(());
        }
    }

    let include_cached = include_all_cached || options.cached;
    let mut records = Vec::new();
    let mut seen_records = BTreeSet::new();
    if include_all_cached || show_stage_format {
        let mut stdout = io::stdout().lock();
        let mut seen_stage_paths = BTreeSet::new();
        for entry in index.entries().iter().filter(|entry| {
            (!options.unmerged || entry.stage > 0)
                && (!options.ignored
                    || ignore
                        .as_ref()
                        .expect("ignore graph for ignored cached entries")
                        .is_ignored(&entry.path, false))
                && pathspec_matches(&entry.path, &effective_pathspecs)
        }) {
            if show_stage_format {
                if options.error_unmatch
                    && !effective_pathspecs.is_empty()
                    && !seen_stage_paths.insert(entry.path.to_vec())
                {
                    continue;
                }
                let Some(display_path) =
                    ls_files_display_path(&entry.path, &cwd_prefix, options.full_name)
                else {
                    continue;
                };
                let record = ls_files_stage_record(
                    &repo,
                    entry,
                    &display_path,
                    &options,
                    eol_store.as_ref(),
                    eol_attrs.as_ref(),
                )?;
                write_ls_files_record(&mut stdout, &record, options.zero)?;
                if options.debug {
                    write_ls_files_debug(&mut stdout, entry)?;
                }
            } else {
                push_ls_files_path_record(
                    &mut records,
                    &mut seen_records,
                    entry.path.to_vec(),
                    ls_files_tag(entry, &options, &with_tree_paths).unwrap_or(b'H'),
                    &options,
                );
            }
        }
        if options.resolve_undo {
            write_ls_files_resolve_undo_records(
                &mut stdout,
                &index,
                &effective_pathspecs,
                &cwd_prefix,
                &options,
            )?;
            if let Some(pathspec) = unmatched_pathspec {
                return Err(ls_files_error_unmatch(pathspec));
            }
            return Ok(());
        }
    }
    if include_cached && !include_all_cached && !show_stage_format {
        for entry in index.entries().iter().filter(|entry| {
            pathspec_matches(&entry.path, &effective_pathspecs)
                && (!options.ignored
                    || ignore
                        .as_ref()
                        .expect("ignore graph for ignored explicit cached entries")
                        .is_ignored(&entry.path, false))
        }) {
            push_ls_files_path_record(
                &mut records,
                &mut seen_records,
                entry.path.to_vec(),
                ls_files_tag(entry, &options, &with_tree_paths).unwrap_or(b'H'),
                &options,
            );
        }
    }
    if options.killed {
        for path in killed_files(&repo, &index, options.directory)? {
            if pathspec_matches(&path, &effective_pathspecs) {
                push_ls_files_path_record(&mut records, &mut seen_records, path, b'K', &options);
            }
        }
    }
    if options.deleted || options.modified {
        for (path, status) in worktree_status(&repo, &index)? {
            if pathspec_matches(&path, &effective_pathspecs)
                && ((options.deleted && status == 'D')
                    || (options.modified && matches!(status, 'M' | 'D')))
            {
                if options.deleted && status == 'D' {
                    push_ls_files_path_record(
                        &mut records,
                        &mut seen_records,
                        path.clone(),
                        b'R',
                        &options,
                    );
                }
                if options.modified && matches!(status, 'M' | 'D') {
                    push_ls_files_path_record(
                        &mut records,
                        &mut seen_records,
                        path.clone(),
                        b'C',
                        &options,
                    );
                }
            }
        }
    }
    let mut stdout = io::stdout().lock();
    for (path, tag) in records {
        if let Some(display_path) = ls_files_display_path(&path, &cwd_prefix, options.full_name) {
            let record = if options.eol {
                let entry = index
                    .entries()
                    .iter()
                    .find(|entry| entry.path.as_slice() == path.as_slice());
                ls_files_eol_record(LsFilesEolRecord {
                    repo: &repo,
                    entry,
                    path: &path,
                    display_path: &display_path,
                    options: &options,
                    tag,
                    prefix_tag: true,
                    store: eol_store.as_ref(),
                    attrs: eol_attrs.as_ref(),
                })?
            } else {
                ls_files_display_record(tag, &String::from_utf8_lossy(&display_path), &options)
            };
            write_ls_files_record(&mut stdout, &record, options.zero)?;
            if options.debug
                && let Some(entry) = index
                    .entries()
                    .iter()
                    .find(|entry| entry.path.as_slice() == path.as_slice())
            {
                write_ls_files_debug(&mut stdout, entry)?;
            }
        }
    }
    if let Some(pathspec) = unmatched_pathspec {
        return Err(ls_files_error_unmatch(pathspec));
    }
    Ok(())
}

fn ls_files_needs_ignore(options: &LsFilesOptions) -> bool {
    options.others || options.ignored
}

fn can_stream_plain_ls_files(
    options: &LsFilesOptions,
    include_all_cached: bool,
    show_stage_format: bool,
) -> bool {
    include_all_cached
        && !show_stage_format
        && !options.cached
        && !options.others
        && !options.resolve_undo
        && !options.error_unmatch
        && !options.tagged
        && !options.lowercase_assume_valid
        && !options.fsmonitor_clean
        && !options.debug
        && !options.eol
        && options.format.is_none()
}

fn ls_files_index_with_tree(
    repo: &GitRepo,
    index: GitIndex,
    treeish: &str,
) -> Result<(GitIndex, HashSet<Vec<u8>>)> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let tree_index = read_treeish_index(repo, &store, treeish)?;
    let mut existing_paths = index
        .entries()
        .iter()
        .map(|entry| entry.path.to_vec())
        .collect::<HashSet<_>>();
    let mut entries = index.entries().to_vec();
    let mut virtual_paths = HashSet::new();
    for entry in tree_index.entries() {
        if existing_paths.insert(entry.path.to_vec()) {
            virtual_paths.insert(entry.path.to_vec());
            entries.push(entry.clone());
        }
    }
    Ok((GitIndex::from_entries(entries)?, virtual_paths))
}

fn ls_files_index_with_submodules(repo: &GitRepo, index: GitIndex) -> Result<GitIndex> {
    let mut entries = Vec::new();
    for entry in index.entries() {
        if entry.mode != IndexMode::Gitlink {
            entries.push(entry.clone());
            continue;
        }
        let submodule_root = repo
            .root
            .join(String::from_utf8_lossy(&entry.path).as_ref());
        let submodule_repo = repo_from_worktree_root(submodule_root)?;
        let submodule_index = read_repo_index(&submodule_repo)?;
        for sub_entry in submodule_index.entries() {
            let mut nested = sub_entry.clone();
            let mut path = entry.path.to_vec();
            path.push(b'/');
            path.extend_from_slice(&sub_entry.path);
            nested.path = path;
            entries.push(nested);
        }
    }
    Ok(GitIndex::from_entries(entries)?)
}

fn ls_files_excludes(repo: &GitRepo, options: &LsFilesOptions) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if options.exclude_standard {
        append_per_directory_excludes(&repo.root, &repo.root, ".gitignore", &mut ignore)?;
        append_ignore_file(&mut ignore, &repo.git_dir.join("info/exclude"), "")?;
        if let Some(path) = ls_files_global_excludes_file(repo)? {
            append_ignore_file(&mut ignore, &path, "")?;
        }
    }
    if !options.excludes.is_empty() {
        ignore.append(GitIgnore::parse(&options.excludes.join("\n")));
    }
    for path in &options.exclude_from {
        let content = fs::read_to_string(path)?;
        ignore.append(GitIgnore::parse(&content));
    }
    if let Some(name) = &options.exclude_per_directory {
        append_per_directory_excludes(&repo.root, &repo.root, name, &mut ignore)?;
    }
    Ok(ignore)
}

fn append_ignore_file(ignore: &mut GitIgnore, path: &std::path::Path, base: &str) -> Result<()> {
    match fs::read_to_string(path) {
        Ok(content) => ignore.append(GitIgnore::parse_with_base(&content, base)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn ls_files_global_excludes_file(repo: &GitRepo) -> Result<Option<PathBuf>> {
    if let Some(path) = read_config_value(repo, "core.excludesFile")? {
        if path.is_empty() {
            return Ok(None);
        }
        return Ok(Some(expand_user_path(&path)));
    }
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(Some(PathBuf::from(config_home).join("git/ignore")));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(Some(PathBuf::from(home).join(".config/git/ignore")));
    }
    Ok(None)
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

fn append_per_directory_excludes(
    root: &std::path::Path,
    dir: &std::path::Path,
    name: &str,
    ignore: &mut GitIgnore,
) -> Result<()> {
    let exclude_path = dir.join(name);
    let base = repo_relative_path(root, dir)?;
    let base = String::from_utf8_lossy(&base);
    append_ignore_file(ignore, &exclude_path, &base)?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            append_per_directory_excludes(root, &path, name, ignore)?;
        }
    }
    Ok(())
}

fn push_ls_files_path_record(
    records: &mut Vec<(Vec<u8>, u8)>,
    seen: &mut BTreeSet<Vec<u8>>,
    path: Vec<u8>,
    tag: u8,
    options: &LsFilesOptions,
) {
    let should_deduplicate = (options.deduplicate || options.error_unmatch)
        && !options.tagged
        && !options.lowercase_assume_valid
        && !options.fsmonitor_clean;
    if should_deduplicate && !seen.insert(path.clone()) {
        return;
    }
    records.push((path, tag));
}

fn ls_files_tag(
    entry: &IndexEntry,
    options: &LsFilesOptions,
    with_tree_paths: &HashSet<Vec<u8>>,
) -> Option<u8> {
    if with_tree_paths.contains(entry.path.as_slice()) {
        return (options.tagged || options.lowercase_assume_valid || options.fsmonitor_clean)
            .then_some(b'M');
    }
    if entry.stage > 0 {
        return (options.tagged || options.lowercase_assume_valid || options.fsmonitor_clean)
            .then_some(b'M');
    }
    if entry.skip_worktree() {
        return (options.tagged || options.lowercase_assume_valid || options.fsmonitor_clean)
            .then_some(b'S');
    }
    if options.lowercase_assume_valid {
        return Some(if entry.assume_valid() { b'h' } else { b'H' });
    }
    (options.tagged || options.fsmonitor_clean).then_some(b'H')
}

fn ls_files_display_record(tag: u8, path: &str, options: &LsFilesOptions) -> String {
    if options.tagged || options.lowercase_assume_valid || options.fsmonitor_clean {
        format!("{} {path}", tag as char)
    } else {
        path.to_owned()
    }
}

fn write_ls_files_debug(out: &mut impl Write, entry: &IndexEntry) -> Result<()> {
    writeln!(
        out,
        "  ctime: {}:{}",
        entry.ctime_seconds, entry.ctime_nanoseconds
    )?;
    writeln!(
        out,
        "  mtime: {}:{}",
        entry.mtime_seconds, entry.mtime_nanoseconds
    )?;
    writeln!(out, "  dev: {}\tino: {}", entry.dev, entry.ino)?;
    writeln!(out, "  uid: {}\tgid: {}", entry.uid, entry.gid)?;
    writeln!(
        out,
        "  size: {}\tflags: {:x}",
        entry.size,
        ls_files_debug_flags(entry)
    )?;
    Ok(())
}

fn ls_files_debug_flags(entry: &IndexEntry) -> u32 {
    let mut flags = (entry.stage as u32) << 12;
    if entry.assume_valid() {
        flags |= 0x8000;
    }
    let mut extended = 0;
    if entry.skip_worktree() {
        flags |= 0x4000;
        extended |= 0x4000;
    }
    if entry.intent_to_add() {
        flags |= 0x4000;
        extended |= 0x2000;
    }
    flags | (extended << 16)
}

fn ls_files_object_name(id: &ObjectId, abbrev: Option<usize>) -> String {
    let hex = id.to_hex();
    match abbrev {
        Some(width) => hex.chars().take(width.min(hex.len())).collect(),
        None => hex,
    }
}

fn ls_files_resolve_undo_record(
    stage: &ResolveUndoStage,
    stage_number: u8,
    display_path: &[u8],
    options: &LsFilesOptions,
) -> String {
    let prefix = if options.tagged || options.lowercase_assume_valid || options.fsmonitor_clean {
        "U "
    } else {
        ""
    };
    format!(
        "{prefix}{:o} {} {}\t{}",
        stage.mode.bits(),
        ls_files_object_name(&stage.id, options.abbrev),
        stage_number,
        String::from_utf8_lossy(display_path)
    )
}

fn write_ls_files_resolve_undo_records(
    out: &mut impl Write,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
    cwd_prefix: &[u8],
    options: &LsFilesOptions,
) -> Result<()> {
    for entry in index
        .resolve_undo()
        .iter()
        .filter(|entry| pathspec_matches(&entry.path, pathspecs))
    {
        let Some(display_path) = ls_files_display_path(&entry.path, cwd_prefix, options.full_name)
        else {
            continue;
        };
        for (stage_idx, stage) in entry.stages.iter().enumerate() {
            let Some(stage) = stage else {
                continue;
            };
            let record =
                ls_files_resolve_undo_record(stage, (stage_idx + 1) as u8, &display_path, options);
            write_ls_files_record(out, &record, options.zero)?;
        }
    }
    Ok(())
}

fn ls_files_stage_record(
    repo: &GitRepo,
    entry: &IndexEntry,
    display_path: &[u8],
    options: &LsFilesOptions,
    store: Option<&LooseObjectStore>,
    attrs: Option<&GitAttributes>,
) -> Result<String> {
    let tag = ls_files_tag(entry, options, &HashSet::new())
        .map(|tag| format!("{} ", tag as char))
        .unwrap_or_default();
    if options.eol {
        let eol = ls_files_eol_record(LsFilesEolRecord {
            repo,
            entry: Some(entry),
            path: &entry.path,
            display_path,
            options,
            tag: b'H',
            prefix_tag: false,
            store,
            attrs,
        })?;
        Ok(format!(
            "{}{:06o} {} {}\t{}",
            tag,
            entry.mode.bits(),
            ls_files_object_name(&entry.id, options.abbrev),
            entry.stage,
            eol
        ))
    } else {
        Ok(format!(
            "{}{:06o} {} {}\t{}",
            tag,
            entry.mode.bits(),
            ls_files_object_name(&entry.id, options.abbrev),
            entry.stage,
            String::from_utf8_lossy(display_path)
        ))
    }
}

fn ls_files_other_record(
    repo: &GitRepo,
    path: &[u8],
    display_path: &[u8],
    options: &LsFilesOptions,
    attrs: Option<&GitAttributes>,
) -> Result<String> {
    if options.eol {
        ls_files_eol_record(LsFilesEolRecord {
            repo,
            entry: None,
            path,
            display_path,
            options,
            tag: b'?',
            prefix_tag: true,
            store: None,
            attrs,
        })
    } else {
        Ok(ls_files_display_record(
            b'?',
            &String::from_utf8_lossy(display_path),
            options,
        ))
    }
}

struct LsFilesEolRecord<'a> {
    repo: &'a GitRepo,
    entry: Option<&'a IndexEntry>,
    path: &'a [u8],
    display_path: &'a [u8],
    options: &'a LsFilesOptions,
    tag: u8,
    prefix_tag: bool,
    store: Option<&'a LooseObjectStore>,
    attrs: Option<&'a GitAttributes>,
}

fn ls_files_eol_record(record: LsFilesEolRecord<'_>) -> Result<String> {
    let index_eol = match (record.entry, record.store) {
        (Some(entry), Some(store)) => {
            let object = store.read_object(&entry.id)?;
            classify_eol(&object.content)
        }
        _ => "",
    };
    let worktree_eol = read_worktree_eol(record.repo, record.path)?;
    let attr = ls_files_eol_attr(record.path, record.attrs);
    let body = format!(
        "i/{:<5} w/{:<5} attr/{:<17}\t{}",
        index_eol,
        worktree_eol,
        attr,
        String::from_utf8_lossy(record.display_path)
    );
    if record.prefix_tag && (record.options.tagged || record.options.lowercase_assume_valid) {
        Ok(format!("{} {body}", record.tag as char))
    } else {
        Ok(body)
    }
}

fn read_worktree_eol(repo: &GitRepo, path: &[u8]) -> Result<&'static str> {
    let path = String::from_utf8_lossy(path);
    let full_path = repo.root.join(path.as_ref());
    match fs::read(full_path) {
        Ok(content) => Ok(classify_eol(&content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(""),
        Err(error) if error.kind() == io::ErrorKind::IsADirectory => Ok(""),
        Err(error) if cfg!(windows) && error.kind() == io::ErrorKind::PermissionDenied => Ok(""),
        Err(error) => Err(error.into()),
    }
}

fn classify_eol(content: &[u8]) -> &'static str {
    let mut nul_count = 0usize;
    let mut bare_cr_count = 0usize;
    let mut crlf_count = 0usize;
    let mut lf_count = 0usize;
    let mut printable_count = 0usize;
    let mut nonprintable_count = 0usize;

    let mut index = 0;
    while index < content.len() {
        match content[index] {
            b'\r' if content.get(index + 1) == Some(&b'\n') => {
                crlf_count += 1;
                index += 2;
                continue;
            }
            b'\r' => {
                bare_cr_count += 1;
            }
            b'\n' => {
                lf_count += 1;
            }
            127 => {
                nonprintable_count += 1;
            }
            byte if byte < 32 => match byte {
                b'\x08' | b'\t' | b'\x1b' | b'\x0c' => {
                    printable_count += 1;
                }
                0 => {
                    nul_count += 1;
                    nonprintable_count += 1;
                }
                _ => {
                    nonprintable_count += 1;
                }
            },
            _ => {
                printable_count += 1;
            }
        }
        index += 1;
    }
    if content.last() == Some(&b'\x1a') {
        nonprintable_count = nonprintable_count.saturating_sub(1);
    }
    if bare_cr_count > 0 || nul_count > 0 || (printable_count >> 7) < nonprintable_count {
        return "-text";
    }
    match (crlf_count > 0, lf_count > 0) {
        (false, false) => "none",
        (true, false) => "crlf",
        (false, true) => "lf",
        (true, true) => "mixed",
    }
}

fn ls_files_eol_attr(path: &[u8], attrs: Option<&GitAttributes>) -> String {
    let Some(attrs) = attrs else {
        return String::new();
    };
    let names = vec!["text".to_owned(), "eol".to_owned()];
    let values = attrs.check(path, &names);
    if values
        .iter()
        .any(|(name, value)| name == "text" && *value == AttributeValue::Unset)
    {
        return "-text".to_owned();
    }
    let mut parts = Vec::new();
    let has_text = values
        .iter()
        .any(|(name, value)| name == "text" && *value != AttributeValue::Unspecified);
    for (name, value) in values {
        match (name.as_str(), value) {
            ("text", AttributeValue::Set) => parts.push("text".to_owned()),
            ("text", AttributeValue::Unset) => parts.push("-text".to_owned()),
            ("text", AttributeValue::Value(value)) => {
                parts.push(format!("text={value}"));
            }
            ("eol", AttributeValue::Value(value)) => {
                if !has_text {
                    parts.push("text".to_owned());
                }
                parts.push(format!("eol={value}"));
            }
            ("eol", AttributeValue::Set) => parts.push("eol".to_owned()),
            ("eol", AttributeValue::Unset) => parts.push("-eol".to_owned()),
            _ => {}
        }
    }
    parts.join(" ")
}

fn render_ls_files_format(
    format: &str,
    entry: &IndexEntry,
    display_path: &[u8],
    abbrev: Option<usize>,
) -> Result<String> {
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some('x') => {
                chars.next();
                let hi = chars.next().ok_or_else(|| bad_ls_files_format(format))?;
                let lo = chars.next().ok_or_else(|| bad_ls_files_format(format))?;
                let value = hex_pair_value(hi, lo).ok_or_else(|| bad_ls_files_format(format))?;
                out.push(char::from(value));
            }
            Some('(') => {
                chars.next();
                let mut atom = String::new();
                loop {
                    match chars.next() {
                        Some(')') => break,
                        Some(ch) => atom.push(ch),
                        None => return Err(bad_ls_files_format(format)),
                    }
                }
                match atom.as_str() {
                    "objectmode" => out.push_str(&format!("{:06o}", entry.mode.bits())),
                    "objectname" => out.push_str(&ls_files_object_name(&entry.id, abbrev)),
                    "stage" => out.push_str(&entry.stage.to_string()),
                    "path" => out.push_str(&String::from_utf8_lossy(display_path)),
                    _ => return Err(bad_ls_files_format(format)),
                }
            }
            _ => return Err(bad_ls_files_format(format)),
        }
    }
    Ok(out)
}

fn bad_ls_files_format(format: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("bad ls-files format: {format}"),
    }
}

fn hex_pair_value(hi: char, lo: char) -> Option<u8> {
    let hi = hi.to_digit(16)?;
    let lo = lo.to_digit(16)?;
    Some(((hi << 4) | lo) as u8)
}

fn ls_files_first_unmatched_pathspec(
    index: &GitIndex,
    other_paths: &[Vec<u8>],
    pathspecs: &[Vec<u8>],
) -> Option<String> {
    pathspecs.iter().find_map(|pathspec| {
        let rule = parse_pathspec_rule(pathspec);
        if rule.exclude {
            return None;
        }
        let matches_index = index
            .entries()
            .iter()
            .any(|entry| pathspec_rule_matches(&entry.path, rule));
        let matches_other = other_paths
            .iter()
            .any(|path| pathspec_rule_matches(path, rule));
        (!matches_index && !matches_other).then(|| String::from_utf8_lossy(pathspec).into_owned())
    })
}

fn ls_files_error_unmatch(pathspec: String) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!(
            "error: pathspec '{pathspec}' did not match any file(s) known to git\n\
             Did you forget to 'git add'?\n"
        ),
    }
}

fn ls_files_display_path(path: &[u8], cwd_prefix: &[u8], full_name: bool) -> Option<Vec<u8>> {
    if full_name || cwd_prefix.is_empty() {
        return Some(path.to_vec());
    }
    if path == cwd_prefix {
        return Some(Vec::new());
    }
    if let Some(rest) = path
        .strip_prefix(cwd_prefix)
        .and_then(|rest| rest.strip_prefix(b"/"))
    {
        return Some(rest.to_vec());
    }
    Some(relative_pathspec_bytes(cwd_prefix, path))
}

fn write_ls_files_plain_path_record(
    out: &mut impl Write,
    path: &[u8],
    cwd_prefix: &[u8],
    full_name: bool,
    zero: bool,
) -> Result<bool> {
    let display_path = if full_name || cwd_prefix.is_empty() {
        Some(path)
    } else if path == cwd_prefix {
        Some(&[][..])
    } else if let Some(rest) = path
        .strip_prefix(cwd_prefix)
        .and_then(|rest| rest.strip_prefix(b"/"))
    {
        Some(rest)
    } else {
        None
    };

    if let Some(display_path) = display_path {
        out.write_all(display_path)?;
    } else {
        out.write_all(&relative_pathspec_bytes(cwd_prefix, path))?;
    }
    if zero {
        out.write_all(&[0])?;
    } else {
        out.write_all(b"\n")?;
    }
    Ok(true)
}

fn relative_pathspec_bytes(from: &[u8], to: &[u8]) -> Vec<u8> {
    let from_components = from
        .split(|byte| *byte == b'/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    let to_components = to
        .split(|byte| *byte == b'/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    let mut out = Vec::new();
    for _ in common..from_components.len() {
        if !out.is_empty() {
            out.push(b'/');
        }
        out.extend_from_slice(b"..");
    }
    for component in &to_components[common..] {
        if !out.is_empty() {
            out.push(b'/');
        }
        out.extend_from_slice(component);
    }
    out
}

fn write_ls_files_record(out: &mut impl Write, record: &str, zero: bool) -> Result<()> {
    if zero {
        out.write_all(record.as_bytes())?;
        out.write_all(&[0])?;
    } else {
        writeln!(out, "{record}")?;
    }
    Ok(())
}

pub(crate) fn add(
    all: bool,
    force: bool,
    update: bool,
    intent_to_add: bool,
    refresh: bool,
    _verbose: bool,
    ignore_errors: bool,
    ignore_missing: bool,
    chmod: Option<String>,
    dry_run: bool,
    pathspec_from_file: Option<PathBuf>,
    pathspec_file_nul: bool,
    mut paths: Vec<PathBuf>,
) -> Result<()> {
    let _trace = phase_trace("add.total");
    if all && update {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-A' and '-u' cannot be used together".into(),
        });
    }
    if intent_to_add && (all || update || refresh) {
        return Err(CliError::Fatal {
            code: 128,
            message: "--intent-to-add cannot be combined with -A, -u, or --refresh".into(),
        });
    }
    if let Some(pathspec_file) = pathspec_from_file {
        let loaded = read_pathspec_file(&pathspec_file, pathspec_file_nul)?;
        paths.extend(loaded);
    } else if pathspec_file_nul {
        return Err(CliError::Fatal {
            code: 129,
            message: "--pathspec-file-nul requires --pathspec-from-file".into(),
        });
    }
    if paths.is_empty() && !all && !update {
        eprintln!("Nothing specified, nothing added.");
        eprintln!("hint: Maybe you wanted to say 'git add .'?");
        eprintln!(
            "hint: Disable this message with \"git config set advice.addEmptyPathspec false\""
        );
        return Ok(());
    }
    let _setup_trace = phase_trace("add.setup");
    let repo = find_repo()?;
    preflight_explicit_submodule_hash_mismatch(&repo, all, update || refresh, &paths)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut index = read_repo_index(&repo)?;
    let ignore_errors = ignore_errors || add_ignore_errors_config_enabled(&repo)?;
    let chmod = chmod.as_deref().map(parse_add_chmod).transpose()?;
    drop(_setup_trace);
    if refresh {
        let _trace = phase_trace("add.refresh");
        let pathspecs = paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?;
        ensure_add_pathspecs_match(&repo, &index, &pathspecs)?;
        refresh_tracked_index_metadata_matching(&repo, &mut index, &pathspecs)?;
        index.write_to_path(&repo.index_path)?;
        return Ok(());
    }
    if update {
        let _trace = phase_trace("add.update");
        let pathspecs = paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?;
        ensure_add_pathspecs_match(&repo, &index, &pathspecs)?;
        stage_tracked_worktree_changes_matching(
            &repo,
            &store,
            &mut index,
            &pathspecs,
            &HashSet::new(),
        )?;
        index.write_to_path(&repo.index_path)?;
        return Ok(());
    }

    let _collect_trace = phase_trace("add.collect_files");
    let ignore = GitIgnore::load_from_root(&repo.root)?;
    let mut files = Vec::new();
    let all_pathspecs = if all && paths.is_empty() {
        Vec::new()
    } else if all {
        paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    if all {
        ensure_add_pathspecs_match(&repo, &index, &all_pathspecs)?;
    }
    let add_paths = if all && paths.is_empty() {
        vec![repo.root.clone()]
    } else {
        paths
    };
    let mut ignored_explicit_paths = Vec::new();
    for path in add_paths {
        let raw_path = path.to_string_lossy();
        let unescaped_path = unescape_pathspec_literal_arg(&raw_path);
        let path_for_lookup;
        let lookup_path = if unescaped_path.as_ref() == raw_path.as_ref() {
            &path
        } else {
            path_for_lookup = PathBuf::from(unescaped_path.into_owned());
            &path_for_lookup
        };
        let absolute = absolute_path_from_arg(lookup_path)?;
        if all && !path_exists(&absolute) {
            continue;
        }
        if !path_exists(&absolute) {
            if dry_run && ignore_missing {
                let relative = path_arg_to_repo_relative(&repo, &path)?;
                if !force && ignore.is_ignored(&relative, false) {
                    ignored_explicit_paths.push(relative);
                }
                continue;
            }
            return Err(CliError::Fatal {
                code: 128,
                message: format!("pathspec '{}' did not match any files", path.display()),
            });
        }
        if !force {
            if let Some(ignored) = explicit_ignored_add_path(&repo, &index, &ignore, &absolute)? {
                ignored_explicit_paths.push(ignored);
                continue;
            }
        }
        if explicit_unmerged_add_path(&repo, &index, &absolute)? {
            files.push(absolute);
            continue;
        }
        if explicit_tracked_add_path(&repo, &index, &absolute)? {
            files.push(absolute);
            continue;
        }
        collect_add_files(&repo.root, &absolute, &ignore, force, &mut files)?;
    }
    files.sort();
    files.dedup();
    let file_entries = files
        .iter()
        .map(|file| Ok((file.clone(), repo_relative_path(&repo.root, file)?)))
        .collect::<Result<Vec<_>>>()?;
    let files_to_stage = file_entries
        .iter()
        .map(|(_, relative)| relative.clone())
        .collect::<HashSet<_>>();
    drop(_collect_trace);
    if all {
        let already_staged = if chmod.is_none() {
            HashSet::new()
        } else {
            files_to_stage.clone()
        };
        let _trace = phase_trace("add.stage_tracked");
        stage_tracked_worktree_changes_matching(
            &repo,
            &store,
            &mut index,
            &all_pathspecs,
            &already_staged,
        )?;
    }
    if dry_run {
        if let Some(chmod) = chmod {
            for file in &files {
                ensure_add_chmod_candidate(&repo, &index, file, chmod)?;
            }
            if !ignored_explicit_paths.is_empty() {
                return Err(explicit_ignored_add_error(&ignored_explicit_paths));
            }
            return Ok(());
        }
        for (_, relative) in &file_entries {
            println!("add '{}'", String::from_utf8_lossy(&relative));
        }
        if !ignored_explicit_paths.is_empty() {
            return Err(explicit_ignored_add_error(&ignored_explicit_paths));
        }
        return Ok(());
    }
    let mut ignored_errors = false;
    let mut embedded_repo_hint_printed = false;
    let mut chmod_error = None;
    let stage_index_mtime = repo_index_mtime(&repo)?;
    let stage_options = WorktreeStageOptions::load(&repo)?;
    let stage_file_entries = if all && chmod.is_none() {
        let _trace = phase_trace("add.filter_tracked_file_entries");
        file_entries
            .into_iter()
            .filter(|(_, relative)| {
                if let Some(entry) = find_index_entry(&index, relative)
                    && matches!(
                        entry.mode,
                        IndexMode::File | IndexMode::Executable | IndexMode::Symlink
                    )
                {
                    return false;
                }
                true
            })
            .collect::<Vec<_>>()
    } else {
        file_entries
    };
    {
        let _trace = phase_trace("add.stage_files");
        let mut stage_files_trace = StageFilesTrace::new();
        let parallel_staged = if all && chmod.is_none() && !intent_to_add && !ignore_errors {
            try_stage_regular_files_parallel(
                &store,
                &mut index,
                &stage_file_entries,
                stage_index_mtime,
                &stage_options,
                &mut stage_files_trace,
            )?
        } else {
            false
        };
        if !parallel_staged {
            for (file, _) in stage_file_entries {
                let chmod_mode = if let Some(chmod) = chmod {
                    if let Err(error) = ensure_add_chmod_candidate(&repo, &index, &file, chmod) {
                        if chmod_error.is_none() {
                            chmod_error = Some(error);
                        }
                        continue;
                    }
                    Some(chmod.index_mode())
                } else {
                    None
                };
                warn_add_embedded_repo(&repo, &index, &file, &mut embedded_repo_hint_printed)?;
                let stage_result = if intent_to_add {
                    stage_intent_to_add_file(&repo, &store, &mut index, &file)
                } else if stage_files_trace.enabled() {
                    stage_file_with_trace(
                        &repo,
                        &store,
                        &mut index,
                        &file,
                        chmod_mode,
                        stage_index_mtime,
                        &stage_options,
                        &mut stage_files_trace,
                    )
                } else {
                    stage_file_with_mode_and_index_mtime_and_options(
                        &repo,
                        &store,
                        &mut index,
                        &file,
                        chmod_mode,
                        stage_index_mtime,
                        &stage_options,
                    )
                };
                if let Err(error) = stage_result {
                    stage_files_trace.record_error();
                    if ignore_errors {
                        ignored_errors = true;
                        eprintln!("error: unable to add '{}'", file.display());
                        continue;
                    }
                    return Err(error);
                }
            }
        }
        stage_files_trace.emit();
    }
    {
        let _trace = phase_trace("add.write_index");
        index.write_to_path(&repo.index_path)?;
    }
    if !ignored_explicit_paths.is_empty() {
        return Err(explicit_ignored_add_error(&ignored_explicit_paths));
    }
    if let Some(error) = chmod_error {
        return Err(error);
    }
    if ignored_errors {
        return Err(CliError::Exit(1));
    }
    Ok(())
}

fn warn_add_embedded_repo(
    repo: &GitRepo,
    index: &GitIndex,
    path: &Path,
    hint_printed: &mut bool,
) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    if canonical_or_absolute(path.to_path_buf()) == canonical_or_absolute(repo.root.clone())
        || exact_repo_at(path).is_none()
    {
        return Ok(());
    }
    let nested_repo = exact_repo_at(path).expect("nested repo checked above");
    if RefStore::new(&nested_repo.git_dir, GitHashAlgorithm::Sha1)
        .resolve("HEAD")
        .is_err()
    {
        return Ok(());
    }
    let relative = repo_relative_path(&repo.root, path)?;
    if find_index_entry(index, &relative).is_some() {
        return Ok(());
    }
    let display = String::from_utf8_lossy(&relative);
    eprintln!("warning: adding embedded git repository: {display}");
    if !*hint_printed && add_embedded_repo_advice_enabled(repo)? {
        eprintln!("hint: You've added another git repository inside your current repository.");
        eprintln!("hint: Clones of the outer repository will not contain the contents of");
        eprintln!("hint: the embedded repository and will not know how to obtain it.");
        eprintln!("hint: If you meant to add a submodule, use:");
        eprintln!("hint:");
        eprintln!("hint: \tgit submodule add <url> {display}");
        eprintln!("hint:");
        eprintln!("hint: If you added this path by mistake, you can remove it from the");
        eprintln!("hint: index with:");
        eprintln!("hint:");
        eprintln!("hint: \tgit rm --cached {display}");
        eprintln!("hint:");
        eprintln!("hint: See \"git help submodule\" for more information.");
        eprintln!(
            "hint: Disable this message with \"git config set advice.addEmbeddedRepo false\""
        );
        *hint_printed = true;
    }
    Ok(())
}

fn add_embedded_repo_advice_enabled(repo: &GitRepo) -> Result<bool> {
    if let Some(entry) = read_local_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "advice"
                && entry.subsection.is_empty()
                && entry.key == "addEmbeddedRepo"
        })
    {
        return entry.bool_value().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{}'", entry.value),
        });
    }
    Ok(true)
}

fn add_ignore_errors_config_enabled(repo: &GitRepo) -> Result<bool> {
    if let Some(entry) = read_local_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "add" && entry.subsection.is_empty() && entry.key == "ignore-errors"
        })
    {
        return entry.bool_value().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("bad boolean config value '{}'", entry.value),
        });
    }
    Ok(false)
}

fn preflight_explicit_submodule_hash_mismatch(
    repo: &GitRepo,
    all: bool,
    skip: bool,
    paths: &[PathBuf],
) -> Result<()> {
    if all || skip {
        return Ok(());
    }
    let parent_algorithm = repo_object_format(repo)?;
    for path in paths {
        let raw_path = path.to_string_lossy();
        let unescaped_path = unescape_pathspec_literal_arg(&raw_path);
        let path_for_lookup;
        let lookup_path = if unescaped_path.as_ref() == raw_path.as_ref() {
            path
        } else {
            path_for_lookup = PathBuf::from(unescaped_path.into_owned());
            &path_for_lookup
        };
        let absolute = absolute_path_from_arg(lookup_path)?;
        let Some(nested_repo) = exact_repo_at(&absolute) else {
            continue;
        };
        if parent_algorithm != repo_object_format(&nested_repo)? {
            return Err(CliError::Stderr {
                code: 128,
                text: "error: cannot add a submodule of a different hash algorithm\n".to_owned(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum AddChmod {
    Executable,
    NonExecutable,
}

impl AddChmod {
    fn index_mode(self) -> IndexMode {
        match self {
            Self::Executable => IndexMode::Executable,
            Self::NonExecutable => IndexMode::File,
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::Executable => "+x",
            Self::NonExecutable => "-x",
        }
    }
}

fn parse_add_chmod(value: &str) -> Result<AddChmod> {
    match value {
        "+x" => Ok(AddChmod::Executable),
        "-x" => Ok(AddChmod::NonExecutable),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("--chmod param '{value}' must be either -x or +x"),
        }),
    }
}

fn ensure_add_chmod_candidate(
    repo: &GitRepo,
    index: &GitIndex,
    path: &Path,
    chmod: AddChmod,
) -> Result<()> {
    let relative = repo_relative_path(&repo.root, path)?;
    if find_index_entry(index, &relative)
        .is_some_and(|entry| !matches!(entry.mode, IndexMode::File | IndexMode::Executable))
    {
        return Err(add_chmod_error(chmod, &relative));
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(());
    }
    Err(add_chmod_error(chmod, &relative))
}

fn add_chmod_error(chmod: AddChmod, relative: &[u8]) -> CliError {
    CliError::Stderr {
        code: 255,
        text: format!(
            "error: cannot chmod {} '{}'\n",
            chmod.display(),
            String::from_utf8_lossy(&relative)
        ),
    }
}

fn explicit_ignored_add_path(
    repo: &GitRepo,
    index: &GitIndex,
    ignore: &GitIgnore,
    absolute: &Path,
) -> Result<Option<Vec<u8>>> {
    let metadata = fs::symlink_metadata(absolute)?;
    let relative = repo_relative_path(&repo.root, absolute)?;
    if index
        .entries()
        .iter()
        .any(|entry| entry.path.as_slice() == relative.as_slice())
    {
        return Ok(None);
    }
    if ignore.is_ignored(&relative, metadata.is_dir()) {
        return Ok(Some(relative));
    }
    Ok(None)
}

fn explicit_unmerged_add_path(repo: &GitRepo, index: &GitIndex, absolute: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(absolute)?;
    if !(metadata.is_file() || metadata.file_type().is_symlink()) {
        return Ok(false);
    }
    let relative = repo_relative_path(&repo.root, absolute)?;
    Ok(index
        .entries()
        .iter()
        .any(|entry| entry.path.as_slice() == relative.as_slice() && entry.stage != 0))
}

fn explicit_tracked_add_path(repo: &GitRepo, index: &GitIndex, absolute: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(absolute)?;
    if !(metadata.is_file() || metadata.file_type().is_symlink()) {
        return Ok(false);
    }
    let relative = repo_relative_path(&repo.root, absolute)?;
    Ok(find_index_entry(index, &relative).is_some())
}

fn explicit_ignored_add_error(paths: &[Vec<u8>]) -> CliError {
    let mut message =
        "The following paths are ignored by one of your .gitignore files:\n".to_owned();
    for path in paths {
        message.push_str(&String::from_utf8_lossy(path));
        message.push('\n');
    }
    message.push_str("hint: Use -f if you really want to add them.");
    message.push('\n');
    message
        .push_str("hint: Disable this message with \"git config set advice.addIgnoredFile false\"");
    message.push('\n');
    CliError::Stderr {
        code: 1,
        text: message,
    }
}

pub(crate) fn rm(options: RmOptions) -> Result<()> {
    let mut paths = options.paths;
    if let Some(pathspec_file) = options.pathspec_from_file {
        let loaded = read_pathspec_file(&pathspec_file, options.pathspec_file_nul)?;
        paths.extend(loaded);
    } else if options.pathspec_file_nul {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--pathspec-file-nul' requires '--pathspec-from-file'".into(),
        });
    }
    if paths.is_empty() {
        return Err(CliError::Message("`rm` requires at least one path".into()));
    }
    let repo = find_repo()?;
    let _store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let mut index = read_repo_index(&repo)?;
    let mut removed = Vec::new();

    for path in paths {
        let relative = if options.recursive {
            path_arg_to_repo_relative_allow_root(&repo, &path)?
        } else {
            path_arg_to_repo_relative(&repo, &path)?
        };
        let matches = rm_path_matches(&index, &relative, options.recursive)?;
        if matches.is_empty() {
            if relative.is_empty() {
                continue;
            }
            if options.ignore_unmatch {
                continue;
            }
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "pathspec '{}' did not match any files",
                    String::from_utf8_lossy(&relative)
                ),
            });
        }
        for matched in matches {
            if !options.force {
                ensure_rm_safe(&repo, &head_index, &index, &matched, options.cached)?;
            }
            if !options.dry_run {
                index.remove_path(&matched)?;
            }
            removed.push(matched);
        }
    }

    removed.sort();
    removed.dedup();
    if !options.dry_run && !options.cached {
        for path in &removed {
            remove_worktree_path(&repo, path)?;
        }
    }
    if !options.quiet {
        for path in &removed {
            println!("rm '{}'", String::from_utf8_lossy(path));
        }
    }
    if !options.dry_run {
        index.write_to_path(&repo.index_path)?;
    }
    Ok(())
}

pub(crate) fn mv(force: bool, paths: Vec<PathBuf>) -> Result<()> {
    if paths.len() < 2 {
        return Err(CliError::Message(
            "`mv` requires at least one source and a destination".into(),
        ));
    }
    let repo = find_repo()?;
    let mut index = read_repo_index(&repo)?;
    let Some(destination) = paths.last().cloned() else {
        return Err(CliError::Message(
            "`mv` requires at least one source and a destination".into(),
        ));
    };
    let sources = &paths[..paths.len() - 1];
    let destination_absolute = absolute_path_from_arg(&destination)?;
    let multiple_sources = sources.len() > 1;
    if multiple_sources && !destination_absolute.is_dir() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("destination '{}' is not a directory", destination.display()),
        });
    }

    for source in sources {
        let source_absolute = absolute_path_from_arg(source)?;
        let source_relative = path_arg_to_repo_relative(&repo, source)?;
        let target_absolute =
            mv_target_path(&source_absolute, &destination_absolute, multiple_sources)?;
        let target_relative = repo_relative_path(&repo.root, &target_absolute)?;
        let moves = mv_index_moves(&index, &source_relative, &target_relative)?;
        if moves.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "bad source, source={}, destination={}",
                    source.display(),
                    destination.display()
                ),
            });
        }
        ensure_mv_destination_available(&index, &target_relative, force)?;
        rename_worktree_path(&source_absolute, &target_absolute, force)?;
        apply_index_moves(&mut index, moves)?;
    }

    index.write_to_path(&repo.index_path)?;
    Ok(())
}

pub(crate) fn read_tree_command(
    empty: bool,
    _merge: bool,
    prefix: Option<&str>,
    treeish: Option<&str>,
) -> Result<()> {
    if empty && (prefix.is_some() || treeish.is_some()) {
        return Err(CliError::Fatal {
            code: 128,
            message: "passing trees as arguments contradicts --empty".into(),
        });
    }
    let repo = find_repo()?;
    if empty {
        GitIndex::new().write_to_path(&repo.index_path)?;
        return Ok(());
    }
    let Some(treeish) = treeish else {
        return Err(CliError::Fatal {
            code: 129,
            message: "read-tree requires --empty or a tree-ish".into(),
        });
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let tree_id = resolve_treeish_or_invalid_object(&repo, &store, treeish)?;
    let tree_cache = TreeObjectCache::new(&store);
    let mut index = tree_cache.read_tree_to_index(&tree_id)?;
    if let Some(prefix) = prefix {
        index = prefix_index(index, prefix)?;
    }
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn prefix_index(index: GitIndex, prefix: &str) -> Result<GitIndex> {
    let mut prefix = prefix.trim_start_matches('/').as_bytes().to_vec();
    if !prefix.is_empty() && !prefix.ends_with(b"/") {
        prefix.push(b'/');
    }
    let entries = index
        .entries()
        .iter()
        .cloned()
        .map(|mut entry| {
            let mut path = prefix.clone();
            path.extend_from_slice(&entry.path);
            entry.path = path;
            entry
        })
        .collect::<Vec<_>>();
    Ok(GitIndex::from_entries(entries)?)
}

pub(crate) fn checkout_index_command(
    all: bool,
    force: bool,
    quiet: bool,
    stdin: bool,
    prefix: Option<PathBuf>,
    paths: Vec<PathBuf>,
) -> Result<()> {
    if all && (!paths.is_empty() || stdin) {
        return Err(CliError::Fatal {
            code: 128,
            message: "git checkout-index: don't mix '--all' and explicit filenames".into(),
        });
    }

    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let selected = if all {
        index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        let inputs = checkout_index_inputs(stdin, paths)?;
        if inputs.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "checkout-index requires --all or pathnames".into(),
            });
        }
        let mut selected = Vec::new();
        for path in inputs {
            let relative = path_arg_to_repo_relative(&repo, &path)?;
            match find_index_entry(&index, &relative) {
                Some(entry) if entry.stage == 0 => selected.push(entry.clone()),
                _ if quiet => {}
                _ => {
                    return Err(CliError::Stderr {
                        code: 1,
                        text: format!(
                            "git checkout-index: {} is not in the cache\n",
                            String::from_utf8_lossy(&relative)
                        ),
                    });
                }
            }
        }
        selected
    };
    let original_selected = selected;
    let (root, checkout_entries, prefixed_paths) = match prefix {
        Some(prefix) if prefix.is_absolute() => (prefix, original_selected.clone(), Vec::new()),
        Some(prefix) => {
            let prefix = checkout_index_prefix_bytes(&prefix);
            let mut prefixed_paths = Vec::with_capacity(original_selected.len());
            let checkout_entries = original_selected
                .iter()
                .cloned()
                .map(|mut entry| {
                    let mut path = prefix.clone();
                    path.extend_from_slice(&entry.path);
                    prefixed_paths.push(path.clone());
                    entry.path = path;
                    entry
                })
                .collect::<Vec<_>>();
            (repo.root.clone(), checkout_entries, prefixed_paths)
        }
        None => (repo.root.clone(), original_selected.clone(), Vec::new()),
    };
    let selected_index = GitIndex::from_entries(checkout_entries)?;
    checkout_index(
        &store,
        &selected_index,
        root,
        CheckoutIndexOptions { force },
    )?;
    if prefixed_paths.is_empty() {
        smudge_worktree_filter_entries(&repo, &selected_index)?;
    } else {
        for (entry, path) in original_selected.iter().zip(prefixed_paths) {
            smudge_worktree_filter_entry_at_path(
                &repo,
                entry,
                &repo.root.join(String::from_utf8_lossy(&path).as_ref()),
            )?;
        }
    }
    Ok(())
}

fn checkout_index_prefix_bytes(prefix: &Path) -> Vec<u8> {
    prefix.to_string_lossy().replace('\\', "/").into_bytes()
}

fn checkout_index_inputs(stdin: bool, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    if stdin && !paths.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "checkout-index paths cannot be combined with --stdin".into(),
        });
    }
    if !stdin {
        return Ok(paths);
    }
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer.lines().map(PathBuf::from).collect())
}

pub(crate) fn restore(
    source: Option<&str>,
    staged: bool,
    worktree: bool,
    paths: Vec<PathBuf>,
) -> Result<usize> {
    if paths.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "`restore` requires at least one path".into(),
        });
    }
    let restore_index = staged;
    let restore_worktree = worktree || !staged;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let mut index = read_repo_index(&repo)?;
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
        .collect::<Result<Vec<_>>>()?;

    let source_index = if let Some(source) = source {
        let source_id = resolve_commitish(&repo, &store, source).map_err(|_| CliError::Fatal {
            code: 128,
            message: restore_source_resolve_error(source),
        })?;
        let source_commit = commit_cache.read_commit(&source_id)?;
        tree_cache.read_tree_to_index(&source_commit.tree)?
    } else if restore_index {
        read_head_index_with_caches(&repo, &commit_cache, &tree_cache)?
    } else {
        index.clone()
    };
    let original_index = index.clone();
    for pathspec in &pathspecs {
        let source_matches = matching_index_entries(&source_index, pathspec);
        let current_matches = matching_index_entries(&original_index, pathspec);
        if source_matches.is_empty() && current_matches.is_empty() {
            return Err(unmatched_restore_pathspec_error(std::slice::from_ref(
                pathspec,
            )));
        }
    }
    let mut checkout_entries = Vec::new();

    if restore_index {
        for pathspec in &pathspecs {
            let source_matches = matching_index_entries(&source_index, pathspec);
            let current_matches = matching_index_entries(&index, pathspec);
            if source_matches.is_empty() && current_matches.is_empty() {
                continue;
            }
            remove_index_path_or_dir(&mut index, pathspec)?;
            for entry in source_matches {
                index.upsert(entry)?;
            }
        }
        index.write_to_path(&repo.index_path)?;
    }

    if restore_worktree {
        for pathspec in &pathspecs {
            let source_matches = matching_index_entries(&source_index, pathspec);
            let current_matches = matching_index_entries(&original_index, pathspec);
            if source_matches.is_empty() && current_matches.is_empty() {
                continue;
            }
            let source_paths = source_matches
                .iter()
                .map(|entry| entry.path.as_slice())
                .collect::<HashSet<_>>();
            for entry in current_matches {
                if !source_paths.contains(entry.path.as_slice()) {
                    remove_worktree_path(&repo, &entry.path)?;
                }
            }
            for entry in source_matches {
                let path = worktree_path_for_index_entry(&repo.root, &entry.path);
                let needs_checkout = if path_exists(&path) {
                    worktree_entry_modified(&repo, &path, &entry)?
                } else {
                    true
                };
                if needs_checkout {
                    checkout_entries.push(entry);
                }
            }
        }
        let checkout_index_entries = GitIndex::from_entries(checkout_entries)?;
        checkout_index(
            &store,
            &checkout_index_entries,
            &repo.root,
            CheckoutIndexOptions { force: true },
        )?;
        let updated_paths = checkout_index_entries.entries().len();
        smudge_worktree_filter_entries(&repo, &checkout_index_entries)?;
        return Ok(updated_paths);
    }

    Ok(0)
}

fn restore_source_resolve_error(source: &str) -> String {
    #[cfg(windows)]
    {
        format!("could not resolve '{source}'")
    }
    #[cfg(not(windows))]
    {
        format!("could not resolve {source}")
    }
}

pub(crate) fn unmatched_restore_pathspec_error(pathspecs: &[Vec<u8>]) -> CliError {
    let pathspec = pathspecs
        .first()
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .unwrap_or_else(|| "missing".to_owned());
    CliError::Message(format!(
        "pathspec '{pathspec}' did not match any file(s) known to git"
    ))
}

pub(crate) fn reset(soft: bool, mixed: bool, hard: bool, args: Vec<String>) -> Result<()> {
    let (soft, mixed, hard, args) = normalize_reset_mode_options(soft, mixed, hard, args)?;
    let selected = [soft, mixed, hard]
        .into_iter()
        .filter(|value| *value)
        .count();
    if selected > 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "reset mode must be one of --soft, --mixed, or --hard".into(),
        });
    }

    let mode = if soft {
        ResetMode::Soft
    } else if hard {
        ResetMode::Hard
    } else {
        let _ = mixed;
        ResetMode::Mixed
    };
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    if let Some((source, paths)) = reset_path_mode(&repo, &store, &args)? {
        if mode != ResetMode::Mixed {
            let mode_name = match mode {
                ResetMode::Soft => "soft",
                ResetMode::Hard => "hard",
                ResetMode::Mixed => "mixed",
            };
            return Err(CliError::Fatal {
                code: 128,
                message: format!("Cannot do {mode_name} reset with paths."),
            });
        }
        if mixed {
            eprintln!(
                "warning: --mixed with paths is deprecated; use 'git reset -- <paths>' instead."
            );
        }
        return reset_paths(&repo, &store, &commit_cache, &tree_cache, source, paths);
    }
    let target = args.first().map(String::as_str).unwrap_or("HEAD");
    let target_id = resolve_commitish(&repo, &store, target)?;
    let target_commit = commit_cache.read_commit(&target_id)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target_ref_name = branch_checkout_ref(&refs, target)?;
    update_head_to_commit_with_reflog(
        &repo,
        &refs,
        &target_id,
        &format!("reset: moving to {target}"),
    )?;

    match mode {
        ResetMode::Soft => {}
        ResetMode::Mixed => {
            let new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
            new_index.write_to_path(&repo.index_path)?;
        }
        ResetMode::Hard => {
            let old_index = read_repo_index(&repo)?;
            let mut new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
            remove_tracked_paths_missing_from_target(&repo, &old_index, &new_index)?;
            new_index.write_to_path(&repo.index_path)?;
            let checkout_metadata = WorktreeCheckoutMetadata {
                ref_name: target_ref_name,
                treeish: Some(target_id.clone()),
            };
            checkout_worktree_updates_to_index_with_metadata(
                &repo,
                &store,
                &new_index,
                &checkout_metadata,
            )?;
            refresh_tracked_index_metadata_matching(&repo, &mut new_index, &[])?;
            new_index.write_to_path(&repo.index_path)?;
            println!(
                "HEAD is now at {} {}",
                short_object_id(&target_id),
                commit_subject(&target_commit.message)
            );
        }
    }
    Ok(())
}

fn normalize_reset_mode_options(
    mut soft: bool,
    mut mixed: bool,
    mut hard: bool,
    args: Vec<String>,
) -> Result<(bool, bool, bool, Vec<String>)> {
    let mut normalized = Vec::with_capacity(args.len());
    let mut pathspec_mode = false;
    for arg in args {
        if pathspec_mode {
            normalized.push(arg);
            continue;
        }
        match arg.as_str() {
            "--" => {
                pathspec_mode = true;
                normalized.push(arg);
            }
            "--soft" => soft = true,
            "--mixed" => mixed = true,
            "--hard" => hard = true,
            value
                if value.starts_with("--soft=")
                    || value.starts_with("--mixed=")
                    || value.starts_with("--hard=") =>
            {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unknown option `{value}`"),
                });
            }
            _ => normalized.push(arg),
        }
    }
    Ok((soft, mixed, hard, normalized))
}

fn reset_path_mode<'a>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    args: &'a [String],
) -> Result<Option<(&'a str, Vec<PathBuf>)>> {
    match args {
        [] => Ok(None),
        [separator, paths @ ..] if separator == "--" => {
            Ok(Some(("HEAD", paths.iter().map(PathBuf::from).collect())))
        }
        [source, separator, paths @ ..] if separator == "--" => Ok(Some((
            source.as_str(),
            paths.iter().map(PathBuf::from).collect(),
        ))),
        [target] if resolve_commitish(repo, store, target).is_ok() => Ok(None),
        [path] => Ok(Some(("HEAD", vec![PathBuf::from(path)]))),
        [source, paths @ ..] => Ok(Some((
            source.as_str(),
            paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
        ))),
    }
}

fn reset_paths(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    source: &str,
    paths: Vec<PathBuf>,
) -> Result<()> {
    let source_id = resolve_commitish(repo, store, source)?;
    let source_commit = commit_cache.read_commit(&source_id)?;
    let source_index = tree_cache.read_tree_to_index(&source_commit.tree)?;
    let mut index = read_repo_index(repo)?;
    for path in paths {
        let pathspec = path_arg_to_repo_relative(repo, &path)?;
        let source_matches = matching_index_entries(&source_index, &pathspec);
        let current_matches = matching_index_entries(&index, &pathspec);
        if source_matches.is_empty() && current_matches.is_empty() {
            continue;
        }
        remove_index_path_or_dir(&mut index, &pathspec)?;
        for entry in source_matches {
            index.upsert(entry)?;
        }
    }
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

pub(crate) fn worktree(args: Vec<String>) -> Result<()> {
    let subcommand = args.first().map(String::as_str).unwrap_or("list");
    match subcommand {
        "list" => worktree_list(args.iter().any(|arg| arg == "--porcelain")),
        "add" => worktree_add(&args[1..]),
        "move" => worktree_move(&args[1..]),
        "lock" => worktree_lock(&args[1..]),
        "unlock" => worktree_unlock(&args[1..]),
        "remove" => worktree_remove(&args[1..]),
        "prune" => worktree_prune(&args[1..]),
        "repair" => worktree_repair(&args[1..]),
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported worktree subcommand '{subcommand}'"),
        }),
    }
}

pub(crate) fn sparse_checkout(args: Vec<String>) -> Result<()> {
    let subcommand = args.first().map(String::as_str).unwrap_or("list");
    match subcommand {
        "set" => sparse_checkout_set(&args[1..]),
        "add" => sparse_checkout_add(&args[1..]),
        "reapply" => sparse_checkout_reapply(),
        "list" => sparse_checkout_list(),
        "disable" => sparse_checkout_disable(),
        "init" => sparse_checkout_init(&args[1..]),
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported sparse-checkout subcommand '{subcommand}'"),
        }),
    }
}

pub(crate) fn submodule(args: Vec<String>) -> Result<()> {
    let mut args = args;
    let mut quiet = false;
    while args
        .first()
        .is_some_and(|arg| arg == "--quiet" || arg == "-q")
    {
        quiet = true;
        args.remove(0);
    }
    if args.is_empty() {
        let quiet_args = quiet.then(|| "--quiet".to_owned()).into_iter().collect::<Vec<_>>();
        return submodule_status(&quiet_args);
    }
    let prefixed_args = |args: &[String]| {
        let mut values = Vec::with_capacity(args.len() + usize::from(quiet));
        if quiet {
            values.push("--quiet".to_owned());
        }
        values.extend_from_slice(args);
        values
    };
    if args.first().is_some_and(|arg| arg == "--cached") {
        let values = prefixed_args(&args);
        return submodule_status(&values);
    }
    if args.is_empty() {
        return submodule_status(&[]);
    }
    let subcommand = args.first().map(String::as_str).unwrap_or("status");
    match subcommand {
        "add" => submodule_add(&prefixed_args(&args[1..])),
        "status" => submodule_status(&prefixed_args(&args[1..])),
        "init" => init_submodules(&prefixed_args(&args[1..])),
        "sync" => sync_submodules(&prefixed_args(&args[1..])),
        "update" => update_submodules(&prefixed_args(&args[1..])),
        "foreach" => foreach_submodules(&prefixed_args(&args[1..])),
        "deinit" => deinit_submodules(&prefixed_args(&args[1..])),
        "set-branch" => set_submodule_branch(&prefixed_args(&args[1..])),
        "set-url" => set_submodule_url(&prefixed_args(&args[1..])),
        "summary" => summary_submodules(&prefixed_args(&args[1..])),
        "absorbgitdirs" => absorb_submodule_gitdirs(&prefixed_args(&args[1..])),
        "--cached" | "--quiet" => submodule_status(&args),
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported submodule subcommand '{subcommand}'"),
        }),
    }
}

fn worktree_prune(args: &[String]) -> Result<()> {
    let mut dry_run = false;
    let mut verbose = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if arg == "-n" || arg == "--dry-run" {
            dry_run = true;
            cursor += 1;
            continue;
        }
        if arg == "-v" || arg == "--verbose" {
            verbose = true;
            cursor += 1;
            continue;
        }
        let value = if arg == "--expire" {
            cursor += 1;
            args.get(cursor)
                .map(String::as_str)
                .ok_or_else(|| CliError::Fatal {
                    code: 129,
                    message: "--expire requires a value".into(),
                })?
        } else if let Some(value) = arg.strip_prefix("--expire=") {
            value
        } else {
            cursor += 1;
            continue;
        };
        if parse_worktree_expire(value).is_none() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("malformed expiration date '{value}'"),
            });
        }
        cursor += 1;
    }
    let repo = find_repo()?;
    let worktrees = repo.git_dir.join("worktrees");
    let entries = match fs::read_dir(&worktrees) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    for entry in entries {
        let admin_dir = entry?.path();
        if worktree_lock_reason(&admin_dir)?.is_some() {
            continue;
        }
        let gitdir_path = admin_dir.join("gitdir");
        let raw_gitdir = match fs::read_to_string(&gitdir_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        let gitfile = PathBuf::from(raw_gitdir.trim());
        let worktree_missing = !gitfile.exists()
            || gitfile
                .parent()
                .map(|parent| !parent.exists())
                .unwrap_or(true);
        if worktree_missing {
            if dry_run || verbose {
                eprintln!(
                    "Removing worktrees/{}: gitdir file points to non-existent location",
                    admin_dir
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("<unknown>")
                );
            }
            if !dry_run {
                fs::remove_dir_all(&admin_dir)?;
            }
        }
    }
    Ok(())
}

fn parse_worktree_expire(value: &str) -> Option<i64> {
    if value == "now" || value == "never" {
        return Some(0);
    }
    if let Ok(timestamp) = value.parse::<i64>() {
        return Some(timestamp);
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(datetime.timestamp());
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|datetime| datetime.and_utc().timestamp())
}

fn worktree_add(args: &[String]) -> Result<()> {
    let mut detach = false;
    let mut branch_option: Option<(&str, bool)> = None;
    let mut force_count = 0usize;
    let mut values = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if arg == "--detach" {
            detach = true;
        } else if arg == "-f" || arg == "--force" {
            force_count += 1;
        } else if arg == "-b" || arg == "-B" {
            cursor += 1;
            let branch = args
                .get(cursor)
                .map(String::as_str)
                .ok_or_else(|| CliError::Fatal {
                    code: 129,
                    message: format!("{arg} requires a branch name"),
                })?;
            branch_option = Some((branch, arg == "-B"));
        } else {
            values.push(arg.as_str());
        }
        cursor += 1;
    }
    if detach && branch_option.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "options '-b'/'-B' and '--detach' cannot be used together".into(),
        });
    }
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    if values.is_empty() || values.len() > 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "worktree add requires <path> [commit-ish]".into(),
        });
    }
    let target_root = absolute_path_from_arg(std::path::Path::new(values[0]))?;
    let commitish = values.get(1).copied().unwrap_or("HEAD");
    let mut id = resolve_commitish(&repo, &store, commitish).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid reference: {commitish}"),
    })?;
    if target_root.exists() && fs::read_dir(&target_root)?.next().is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{}' already exists", target_root.display()),
        });
    }
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(&common_git_dir, GitHashAlgorithm::Sha1);
    let common_repo = GitRepo {
        root: repo.root.clone(),
        git_dir: common_git_dir.clone(),
        objects_dir: repo.objects_dir.clone(),
        index_path: repo.index_path.clone(),
    };
    let branch_ref = if detach {
        None
    } else if let Some((branch, reset)) = branch_option {
        let ref_name = branch_ref_name(branch)?;
        let exists = ref_exists(&refs, &ref_name)?;
        if exists && !reset {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("a branch named '{branch}' already exists"),
            });
        }
        if force_count == 0
            && let Some(path) = branch_checked_out_worktree(&repo, &ref_name)?
        {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "'{}' is already used by worktree at '{}'",
                    branch_display_name(&ref_name),
                    path.display()
                ),
            });
        }
        write_ref_with_reflog(
            &common_repo,
            &refs,
            &ref_name,
            &id,
            "branch: Created from HEAD",
        )?;
        Some(ref_name)
    } else if values.len() == 1 {
        let branch = target_root
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("cannot derive branch name from '{}'", target_root.display()),
            })?;
        let ref_name = branch_ref_name(branch)?;
        if ref_exists(&refs, &ref_name)? {
            if force_count == 0
                && let Some(path) = branch_checked_out_worktree(&repo, &ref_name)?
            {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "'{}' is already used by worktree at '{}'",
                        branch_display_name(&ref_name),
                        path.display()
                    ),
                });
            }
            id = refs.resolve(&ref_name)?;
        } else {
            write_ref_with_reflog(
                &common_repo,
                &refs,
                &ref_name,
                &id,
                "branch: Created from HEAD",
            )?;
        }
        Some(ref_name)
    } else {
        if let Some(ref_name) = branch_ref_name(commitish)
            .ok()
            .filter(|ref_name| refs.resolve(ref_name).is_ok())
        {
            if force_count == 0
                && let Some(path) = branch_checked_out_worktree(&repo, &ref_name)?
            {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "'{}' is already used by worktree at '{}'",
                        branch_display_name(&ref_name),
                        path.display()
                    ),
                });
            }
            Some(ref_name)
        } else {
            None
        }
    };
    let admin_dir = allocate_worktree_admin_dir(&repo, &target_root)?;
    fs::create_dir_all(&target_root)?;
    fs::create_dir_all(&admin_dir)?;
    let git_file = target_root.join(".git");
    fs::write(&git_file, format!("gitdir: {}\n", admin_dir.display()))?;
    fs::write(
        admin_dir.join("gitdir"),
        format!("{}\n", git_file.display()),
    )?;
    fs::write(admin_dir.join("commondir"), "../..\n")?;
    if let Some(branch_ref) = &branch_ref {
        fs::write(admin_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;
    } else {
        fs::write(admin_dir.join("HEAD"), format!("{}\n", id.to_hex()))?;
    }
    let linked_repo = GitRepo {
        root: target_root.clone(),
        git_dir: admin_dir,
        objects_dir: repo.objects_dir.clone(),
        index_path: target_root.join(".git").with_file_name("index"),
    };
    let linked_repo = GitRepo {
        index_path: linked_repo.git_dir.join("index"),
        ..linked_repo
    };
    append_reflog_if_identity_available(
        &linked_repo,
        "HEAD",
        &zero_object_id(),
        &id,
        "worktree: Created from HEAD",
    )?;
    let commit = commit_cache.read_commit(&id)?;
    let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
    new_index.write_to_path(&linked_repo.index_path)?;
    checkout_index(
        &store,
        &new_index,
        &linked_repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    if let Some(branch_ref) = &branch_ref {
        let action = if let Some((_, reset)) = branch_option {
            if reset {
                "resetting branch"
            } else {
                "new branch"
            }
        } else if values.len() == 1 {
            "new branch"
        } else {
            "checking out"
        };
        eprintln!(
            "Preparing worktree ({action} '{}')",
            branch_display_name(branch_ref)
        );
        println!(
            "HEAD is now at {} {}",
            short_object_id(&id),
            commit_subject(&commit.message)
        );
    } else {
        eprintln!(
            "Preparing worktree (detached HEAD {})",
            short_object_id(&id)
        );
    }
    Ok(())
}

fn worktree_list(porcelain: bool) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head = refs.resolve("HEAD")?;
    if porcelain {
        println!("worktree {}", repo.root.display());
        println!("HEAD {}", head.to_hex());
        if let Some(branch) = current_branch_ref(&refs)? {
            println!("branch {branch}");
        } else {
            println!("detached");
        }
        println!();
    } else {
        let label = current_branch_ref(&refs)?
            .map(|branch| format!("[{}]", branch_display_name(&branch)))
            .unwrap_or_else(|| "(detached HEAD)".into());
        println!(
            "{} {} {}",
            repo.root.display(),
            short_object_id(&head),
            label
        );
    }
    for linked in linked_worktrees(&repo)? {
        let linked_refs = RefStore::new(&linked.git_dir, GitHashAlgorithm::Sha1);
        let (id, branch) = linked_head_id_and_branch(&repo, &linked_refs)?;
        if porcelain {
            println!("worktree {}", linked.root.display());
            println!("HEAD {}", id.to_hex());
            if let Some(branch) = branch {
                println!("branch {branch}");
            } else {
                println!("detached");
            }
            if let Some(reason) = worktree_lock_reason(&linked.git_dir)? {
                if reason.is_empty() {
                    println!("locked");
                } else {
                    println!("locked {reason}");
                }
            }
            println!();
        } else {
            let label = branch
                .map(|branch| format!("[{}]", branch_display_name(&branch)))
                .unwrap_or_else(|| "(detached HEAD)".into());
            println!(
                "{} {} {}",
                linked.root.display(),
                short_object_id(&id),
                label
            );
        }
        let _ = store.read_object(&id)?;
    }
    Ok(())
}

fn linked_head_id_and_branch(
    common_repo: &GitRepo,
    linked_refs: &RefStore,
) -> Result<(ObjectId, Option<String>)> {
    match linked_refs.read_head()? {
        RefTarget::Direct(id) => Ok((id, None)),
        RefTarget::Symbolic(target) => {
            let common_refs = RefStore::new(&common_repo.git_dir, GitHashAlgorithm::Sha1);
            let id = common_refs.resolve(&target)?;
            let branch = target.starts_with("refs/heads/").then_some(target);
            Ok((id, branch))
        }
    }
}

fn worktree_remove(args: &[String]) -> Result<()> {
    let (force_count, values) = parse_worktree_force_args(args);
    if values.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "worktree remove requires <path>".into(),
        });
    }
    let (target, admin_dir) = linked_worktree_path_and_admin(values[0])?;
    if let Some(reason) = worktree_lock_reason(&admin_dir)?
        && force_count < 2
    {
        return Err(locked_worktree_error("remove", &reason));
    }
    fs::remove_dir_all(&target)?;
    fs::remove_dir_all(admin_dir)?;
    Ok(())
}

fn worktree_move(args: &[String]) -> Result<()> {
    let (force_count, values) = parse_worktree_force_args(args);
    if values.len() != 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "worktree move requires <worktree> <new-path>".into(),
        });
    }
    let (source, admin_dir) = linked_worktree_path_and_admin(values[0])?;
    if let Some(reason) = worktree_lock_reason(&admin_dir)?
        && force_count < 2
    {
        return Err(locked_worktree_error("move", &reason));
    }
    let target_arg = absolute_path_from_arg(std::path::Path::new(values[1]))?;
    let target_is_parent = target_arg.is_dir();
    let target_display = if target_is_parent {
        let name = source.file_name().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("cannot derive worktree name from '{}'", source.display()),
        })?;
        std::path::Path::new(values[1]).join(name)
    } else {
        std::path::Path::new(values[1]).to_path_buf()
    };
    let target = if target_is_parent {
        let name = source
            .file_name()
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("cannot derive worktree name from '{}'", source.display()),
            })?
            .to_owned();
        target_arg.join(name)
    } else {
        target_arg
    };
    if target.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{}' already exists", target_display.display()),
        });
    }
    fs::rename(&source, &target)?;
    let moved_git = target.join(".git");
    fs::write(&moved_git, format!("gitdir: {}\n", admin_dir.display()))?;
    fs::write(
        admin_dir.join("gitdir"),
        format!("{}\n", moved_git.display()),
    )?;
    Ok(())
}

fn worktree_lock(args: &[String]) -> Result<()> {
    let mut reason = String::new();
    let mut values = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if arg == "--reason" {
            cursor += 1;
            reason = args.get(cursor).cloned().ok_or_else(|| CliError::Fatal {
                code: 129,
                message: "--reason requires a value".into(),
            })?;
        } else if let Some(value) = arg.strip_prefix("--reason=") {
            reason = value.to_owned();
        } else {
            values.push(arg.as_str());
        }
        cursor += 1;
    }
    if values.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "worktree lock requires <worktree>".into(),
        });
    }
    ensure_worktree_lock_target_is_not_main(values[0])?;
    let (_, admin_dir) = linked_worktree_path_and_admin(values[0])?;
    fs::write(admin_dir.join("locked"), format!("{reason}\n"))?;
    Ok(())
}

fn worktree_unlock(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "worktree unlock requires <worktree>".into(),
        });
    }
    ensure_worktree_lock_target_is_not_main(&args[0])?;
    let (_, admin_dir) = linked_worktree_path_and_admin(&args[0])?;
    match fs::remove_file(admin_dir.join("locked")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn ensure_worktree_lock_target_is_not_main(path: &str) -> Result<()> {
    let target = absolute_path_from_arg(std::path::Path::new(path))?;
    if target.join(".git").is_dir() {
        return Err(CliError::Fatal {
            code: 128,
            message: "The main working tree cannot be locked or unlocked".into(),
        });
    }
    Ok(())
}

fn worktree_repair(args: &[String]) -> Result<()> {
    let mut paths = Vec::new();
    let mut relative_paths = None;
    for arg in args {
        if arg == "--relative-paths" {
            relative_paths = Some(true);
            continue;
        }
        if arg == "--no-relative-paths" {
            relative_paths = Some(false);
            continue;
        }
        paths.push(arg.as_str());
    }
    for path in paths {
        let worktree = absolute_path_from_arg(std::path::Path::new(path))?;
        let gitfile = worktree.join(".git");
        if !gitfile.exists() || gitfile.is_dir() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("'{}' is not a linked working tree", path),
            });
        }
        let admin_dir = read_gitdir_file(&gitfile)?;
        if !admin_dir.exists() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("gitdir '{}' does not exist", admin_dir.display()),
            });
        }
        if let Some(use_relative_paths) = relative_paths {
            worktree_repair_gitfile(&worktree, &gitfile, &admin_dir, use_relative_paths)?;
        }
        let admin_gitdir = admin_dir.join("gitdir");
        let current = fs::read_to_string(&admin_gitdir).unwrap_or_default();
        let expected_path = if let Some(use_relative_paths) = relative_paths {
            worktree_repair_path_value(&admin_dir, &gitfile, use_relative_paths)
        } else {
            gitfile.clone()
        };
        let expected = format!("{}\n", expected_path.display());
        if current != expected {
            if relative_paths.is_some() {
                eprintln!(
                    "repair: gitdir absolute/relative path mismatch: {}",
                    admin_gitdir.display()
                );
            } else {
                eprintln!("repair: gitdir incorrect: {}", admin_gitdir.display());
            }
            fs::write(admin_gitdir, expected)?;
        }
    }
    Ok(())
}

fn worktree_repair_gitfile(
    worktree: &Path,
    gitfile: &Path,
    admin_dir: &Path,
    use_relative_paths: bool,
) -> Result<()> {
    let expected_path = worktree_repair_path_value(worktree, admin_dir, use_relative_paths);
    let expected = format!("gitdir: {}\n", expected_path.display());
    let current = fs::read_to_string(gitfile).unwrap_or_default();
    if current != expected {
        eprintln!(
            "repair: .git file absolute/relative path mismatch: {}",
            worktree.display()
        );
        fs::write(gitfile, expected)?;
    }
    Ok(())
}

fn worktree_repair_path_value(
    from_dir: &Path,
    to_path: &Path,
    use_relative_paths: bool,
) -> PathBuf {
    let from_dir = canonical_or_absolute(from_dir.to_path_buf());
    let to_path = canonical_or_absolute(to_path.to_path_buf());
    if use_relative_paths {
        relative_path_between(&from_dir, &to_path).unwrap_or(to_path)
    } else {
        to_path
    }
}

fn parse_worktree_force_args(args: &[String]) -> (usize, Vec<&str>) {
    let mut force_count = 0usize;
    let mut values = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-f" | "--force" => force_count += 1,
            _ => values.push(arg.as_str()),
        }
    }
    (force_count, values)
}

fn linked_worktree_path_and_admin(path: &str) -> Result<(PathBuf, PathBuf)> {
    let target = absolute_path_from_arg(std::path::Path::new(path))?;
    let target_git = target.join(".git");
    if target_git.is_dir() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{path}' is a main working tree"),
        });
    }
    if !target_git.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{path}' is not a working tree"),
        });
    }
    let admin_dir = read_gitdir_file(&target_git)?;
    if !admin_dir.join("gitdir").exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{path}' is not a linked working tree"),
        });
    }
    Ok((target, admin_dir))
}

fn worktree_lock_reason(admin_dir: &std::path::Path) -> Result<Option<String>> {
    match fs::read_to_string(admin_dir.join("locked")) {
        Ok(raw) => Ok(Some(raw.trim_end_matches('\n').to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn locked_worktree_error(action: &str, reason: &str) -> CliError {
    let reason = if reason.is_empty() {
        String::new()
    } else {
        format!(", lock reason: {reason}")
    };
    CliError::Fatal {
        code: 128,
        message: format!(
            "cannot {action} a locked working tree{reason}\nuse '{action} -f -f' to override or unlock first"
        ),
    }
}

fn allocate_worktree_admin_dir(repo: &GitRepo, target_root: &std::path::Path) -> Result<PathBuf> {
    let name = target_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("worktree")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let base = if name.is_empty() {
        "worktree".to_owned()
    } else {
        name
    };
    let root = read_common_git_dir(&repo.git_dir)?.join("worktrees");
    for idx in 0..1000 {
        let candidate = if idx == 0 {
            root.join(&base)
        } else {
            root.join(format!("{base}-{idx}"))
        };
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(CliError::Fatal {
        code: 128,
        message: "unable to allocate worktree metadata directory".into(),
    })
}

fn linked_worktrees(repo: &GitRepo) -> Result<Vec<GitRepo>> {
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let root = common_git_dir.join("worktrees");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut out = Vec::new();
    for entry in entries {
        let admin = entry?.path();
        let git_file = fs::read_to_string(admin.join("gitdir"))?;
        let git_file = PathBuf::from(git_file.trim());
        let Some(root) = git_file.parent() else {
            continue;
        };
        out.push(GitRepo {
            root: root.to_path_buf(),
            git_dir: admin.clone(),
            objects_dir: repo.objects_dir.clone(),
            index_path: admin.join("index"),
        });
    }
    out.sort_by(|left, right| left.root.cmp(&right.root));
    Ok(out)
}

fn branch_checked_out_worktree(repo: &GitRepo, ref_name: &str) -> Result<Option<PathBuf>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if current_branch_ref(&refs)?.as_deref() == Some(ref_name) {
        return Ok(Some(repo.root.clone()));
    }
    for linked in linked_worktrees(repo)? {
        let refs = RefStore::new(&linked.git_dir, GitHashAlgorithm::Sha1);
        if current_branch_ref(&refs)?.as_deref() == Some(ref_name) {
            return Ok(Some(linked.root));
        }
    }
    Ok(None)
}

fn sparse_checkout_set(patterns: &[String]) -> Result<()> {
    let options = parse_sparse_checkout_options(patterns, true)?;
    let repo = find_repo()?;
    apply_sparse_checkout_config_options(&repo, &options)?;
    set_config_value(&repo, "core.sparseCheckout", "true")?;
    let patterns = options.patterns();
    write_sparse_checkout_patterns(&repo, patterns)?;
    apply_sparse_checkout(&repo, patterns)
}

fn sparse_checkout_add(patterns: &[String]) -> Result<()> {
    let options = parse_sparse_checkout_options(patterns, true)?;
    let repo = find_repo()?;
    ensure_sparse_checkout_enabled(&repo, "no sparse-checkout to add to")?;
    apply_sparse_checkout_config_options(&repo, &options)?;
    set_config_value(&repo, "core.sparseCheckout", "true")?;
    let mut combined = read_sparse_checkout_patterns(&repo)?;
    for pattern in options.patterns() {
        let pattern = normalize_sparse_pattern(pattern)?;
        if !combined.iter().any(|existing| existing == &pattern) {
            combined.push(pattern);
        }
    }
    write_sparse_checkout_patterns(&repo, &combined)?;
    apply_sparse_checkout(&repo, &combined)
}

fn sparse_checkout_init(args: &[String]) -> Result<()> {
    let options = parse_sparse_checkout_options(args, false)?;
    if !options.patterns.is_empty() || options.stdin {
        return Err(CliError::Fatal {
            code: 129,
            message: "sparse-checkout init does not take patterns".into(),
        });
    }
    let repo = find_repo()?;
    set_config_value(&repo, "core.sparseCheckout", "true")?;
    apply_sparse_checkout_config_options(&repo, &options)?;
    write_sparse_checkout_patterns(&repo, &[])?;
    Ok(())
}

fn sparse_checkout_reapply() -> Result<()> {
    let repo = find_repo()?;
    ensure_sparse_checkout_enabled(
        &repo,
        "must be in a sparse-checkout to reapply sparsity patterns",
    )?;
    remove_sparse_excluded_paths(&repo, &read_sparse_checkout_patterns(&repo)?)
}

fn sparse_checkout_list() -> Result<()> {
    let repo = find_repo()?;
    ensure_sparse_checkout_enabled(&repo, "this worktree is not sparse")?;
    for pattern in read_sparse_checkout_patterns(&repo)? {
        println!("{}", quote_sparse_list_pattern(&pattern));
    }
    Ok(())
}

fn sparse_checkout_disable() -> Result<()> {
    let repo = find_repo()?;
    let patterns = read_sparse_checkout_patterns(&repo)?;
    set_config_value(&repo, "core.sparseCheckout", "false")?;
    remove_file_if_exists(&sparse_checkout_file(&repo))?;
    checkout_sparse_excluded_entries(&repo, &patterns)
}

fn write_sparse_checkout_patterns(repo: &GitRepo, patterns: &[String]) -> Result<()> {
    let path = sparse_checkout_file(repo);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = String::from("/*\n!/*/\n");
    for pattern in patterns {
        let pattern = normalize_sparse_pattern(pattern)?;
        out.push('/');
        out.push_str(&pattern);
        if !out.ends_with('/') {
            out.push('/');
        }
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

fn read_sparse_checkout_patterns(repo: &GitRepo) -> Result<Vec<String>> {
    let raw = match fs::read_to_string(sparse_checkout_file(repo)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    Ok(raw
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line == "/*" || line == "!/*/" || line.is_empty() {
                return None;
            }
            Some(line.trim_matches('/').to_owned())
        })
        .collect())
}

fn apply_sparse_checkout(repo: &GitRepo, patterns: &[String]) -> Result<()> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = sparse_checkout_index(repo, &store, patterns)?;
    let mut keep_entries = Vec::new();
    for entry in index.entries() {
        if entry.skip_worktree() {
            remove_worktree_path(repo, &entry.path)?;
        } else {
            keep_entries.push(entry.clone());
        }
    }
    index.write_to_path(&repo.index_path)?;
    let checkout = GitIndex::from_entries(keep_entries)?;
    checkout_index(
        &store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )
    .map_err(CliError::Io)
}

fn remove_sparse_excluded_paths(repo: &GitRepo, patterns: &[String]) -> Result<()> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = sparse_checkout_index(repo, &store, patterns)?;
    for entry in index.entries() {
        if entry.skip_worktree() {
            remove_worktree_path(repo, &entry.path)?;
        }
    }
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn checkout_sparse_excluded_entries(repo: &GitRepo, patterns: &[String]) -> Result<()> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let mut index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let patterns = patterns
        .iter()
        .map(|pattern| normalize_sparse_pattern(pattern))
        .collect::<Result<Vec<_>>>()?;
    let mut restore_entries = Vec::new();
    let mut full_entries = Vec::new();
    for mut entry in index.entries().to_vec() {
        if !sparse_path_matches(&entry.path, &patterns) {
            restore_entries.push(entry.clone());
        }
        entry.set_skip_worktree(false);
        full_entries.push(entry);
    }
    index = GitIndex::from_entries(full_entries)?;
    index.write_to_path(&repo.index_path)?;
    let checkout = GitIndex::from_entries(restore_entries)?;
    checkout_index(
        &store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )
    .map_err(CliError::Io)
}

fn sparse_checkout_index(
    repo: &GitRepo,
    _store: &LooseObjectStore,
    patterns: &[String],
) -> Result<GitIndex> {
    let patterns = patterns
        .iter()
        .map(|pattern| normalize_sparse_pattern(pattern))
        .collect::<Result<Vec<_>>>()?;
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let entries =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?
            .entries()
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.set_skip_worktree(!sparse_path_matches(&entry.path, &patterns));
                entry
            })
            .collect::<Vec<_>>();
    Ok(GitIndex::from_entries(entries)?)
}

fn sparse_path_matches(path: &[u8], patterns: &[String]) -> bool {
    let path = String::from_utf8_lossy(path);
    if !path.contains('/') {
        return true;
    }
    patterns.iter().any(|pattern| {
        path == pattern.as_str()
            || path
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn normalize_sparse_pattern(pattern: &str) -> Result<String> {
    #[cfg(windows)]
    let normalized;
    #[cfg(windows)]
    let pattern = {
        normalized = pattern.replace('\\', "/");
        normalized.as_str()
    };
    let pattern = pattern.trim().trim_matches('/');
    if pattern.is_empty() || pattern.contains("..") {
        return Err(CliError::Stderr {
            code: 128,
            text: format!("fatal: could not normalize path {pattern}\n"),
        });
    }
    Ok(pattern.to_owned())
}

fn quote_sparse_list_pattern(pattern: &str) -> String {
    if !pattern.contains('\\') && !pattern.contains('"') {
        return pattern.to_owned();
    }
    let mut quoted = String::with_capacity(pattern.len() + 2);
    quoted.push('"');
    for ch in pattern.chars() {
        if ch == '\\' || ch == '"' {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    quoted
}

fn sparse_checkout_file(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("info/sparse-checkout")
}

#[derive(Debug, Default)]
struct SparseCheckoutOptions {
    patterns: Vec<String>,
    stdin: bool,
    cone: Option<bool>,
    sparse_index: Option<bool>,
}

impl SparseCheckoutOptions {
    fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

fn parse_sparse_checkout_options(
    args: &[String],
    allow_stdin: bool,
) -> Result<SparseCheckoutOptions> {
    let mut options = SparseCheckoutOptions::default();
    for arg in args {
        match arg.as_str() {
            "--stdin" if allow_stdin => options.stdin = true,
            "--cone" => options.cone = Some(true),
            "--no-cone" => options.cone = Some(false),
            "--sparse-index" => options.sparse_index = Some(true),
            "--no-sparse-index" => options.sparse_index = Some(false),
            "--skip-checks" => {}
            option if option.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unknown sparse-checkout option '{option}'"),
                });
            }
            pattern => options.patterns.push(pattern.to_owned()),
        }
    }
    if options.stdin {
        if !options.patterns.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "sparse-checkout --stdin cannot be combined with path arguments".into(),
            });
        }
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        options.patterns = input
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect();
    }
    Ok(options)
}

fn apply_sparse_checkout_config_options(
    repo: &GitRepo,
    options: &SparseCheckoutOptions,
) -> Result<()> {
    if let Some(cone) = options.cone {
        set_config_value(
            repo,
            "core.sparseCheckoutCone",
            if cone { "true" } else { "false" },
        )?;
    } else if read_config_value(repo, "core.sparseCheckoutCone")?.is_none() {
        set_config_value(repo, "core.sparseCheckoutCone", "true")?;
    }
    if let Some(sparse_index) = options.sparse_index {
        set_config_value(
            repo,
            "index.sparse",
            if sparse_index { "true" } else { "false" },
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_sparse_checkout_enabled(repo: &GitRepo, message: &str) -> Result<()> {
    let enabled =
        sparse_checkout_file(repo).exists() || config_bool_enabled(repo, "core.sparseCheckout")?;
    if enabled {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: message.to_owned(),
    })
}

struct SubmoduleAddOptions {
    quiet: bool,
    force: bool,
    name: Option<String>,
    branch: Option<String>,
    references: Vec<PathBuf>,
    repository: String,
    path: Option<PathBuf>,
}

fn submodule_add(args: &[String]) -> Result<()> {
    let options = parse_submodule_add_options(args)?;
    let repo = find_repo()?;
    let submodule_path = options
        .path
        .clone()
        .unwrap_or_else(|| default_submodule_path(&options.repository));
    let absolute_submodule_path = absolute_path_from_arg(&submodule_path)?;
    if exact_repo_at(&absolute_submodule_path).is_none() {
        transport_commands::clone(CloneOptions {
            quiet: options.quiet,
            configs: Vec::new(),
            template: None,
            reject_shallow: false,
            recurse_submodules: Vec::new(),
            remote_submodules: false,
            shallow_submodules: false,
            bare: false,
            mirror: false,
            no_checkout: false,
            worktree_first: false,
            background_fetch: false,
            demand_hydrate: false,
            remote_name: "origin".to_owned(),
            no_tags: false,
            single_branch: false,
            no_single_branch: false,
            separate_git_dir: None,
            references: options.references.clone(),
            reference_if_able: Vec::new(),
            shared: false,
            dissociate: false,
            no_hardlinks: false,
            no_local: false,
            depth: None,
            branch: options.branch.clone(),
            keep_partial_on_missing_branch: false,
            repository: options.repository.clone(),
            directory: Some(absolute_submodule_path.clone()),
        })?;
    } else if !options.force {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "'{}' already exists in the index",
                submodule_path.display()
            ),
        });
    }
    let submodule_path_string = submodule_path.to_string_lossy().replace('\\', "/");
    let submodule_name = options
        .name
        .clone()
        .unwrap_or_else(|| submodule_path_string.clone());
    write_gitmodules_named_entry(
        &repo,
        &submodule_name,
        &options.repository,
        &submodule_path_string,
        options.branch.as_deref(),
    )?;
    set_config_value(
        &repo,
        &format!("submodule.{submodule_name}.url"),
        &options.repository,
    )?;
    set_config_value(
        &repo,
        &format!("submodule.{submodule_name}.active"),
        "true",
    )?;

    let submodule_repo = find_repo_at(&absolute_submodule_path)?;
    let submodule_refs = RefStore::new(&submodule_repo.git_dir, GitHashAlgorithm::Sha1);
    let submodule_head = submodule_refs.resolve("HEAD")?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut index = read_repo_index(&repo)?;
    stage_file(&repo, &store, &mut index, &repo.root.join(".gitmodules"))?;
    let relative = path_arg_to_repo_relative(&repo, &submodule_path)?;
    index.upsert(IndexEntry::new(
        relative,
        submodule_head,
        IndexMode::Gitlink,
        0,
    )?)?;
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn parse_submodule_add_options(args: &[String]) -> Result<SubmoduleAddOptions> {
    let mut quiet = false;
    let mut force = false;
    let mut name = None;
    let mut branch = None;
    let mut references = Vec::new();
    let mut values = Vec::new();
    let mut path_args = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet") {
            quiet = true;
        } else if !path_args && (arg == "-f" || arg == "--force") {
            force = true;
        } else if !path_args && (arg == "-b" || arg == "--branch") {
            cursor += 1;
            branch = Some(required_submodule_option_value(args, cursor, arg)?);
        } else if !path_args && arg.starts_with("--branch=") {
            branch = Some(arg["--branch=".len()..].to_owned());
        } else if !path_args && arg == "--name" {
            cursor += 1;
            name = Some(required_submodule_option_value(args, cursor, arg)?);
        } else if !path_args && arg.starts_with("--name=") {
            name = Some(arg["--name=".len()..].to_owned());
        } else if !path_args && arg == "--reference" {
            cursor += 1;
            references.push(PathBuf::from(required_submodule_option_value(
                args, cursor, arg,
            )?));
        } else if !path_args && arg.starts_with("--reference=") {
            references.push(PathBuf::from(arg["--reference=".len()..].to_owned()));
        } else if !path_args && arg.starts_with('-') {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("unsupported submodule add option '{arg}'"),
            });
        } else {
            values.push(arg.clone());
        }
        cursor += 1;
    }
    if values.is_empty() || values.len() > 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule add requires <repository> <path>".into(),
        });
    }
    Ok(SubmoduleAddOptions {
        quiet,
        force,
        name,
        branch,
        references,
        repository: values[0].clone(),
        path: values.get(1).map(PathBuf::from),
    })
}

fn required_submodule_option_value(args: &[String], cursor: usize, option: &str) -> Result<String> {
    args.get(cursor).cloned().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: format!("{option} requires a value"),
    })
}

fn default_submodule_path(repository: &str) -> PathBuf {
    let trimmed = repository.trim_end_matches(['/', '\\']);
    Path::new(trimmed)
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(trimmed))
}

fn submodule_status(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let mut cached = false;
    let mut recursive = false;
    let mut quiet = false;
    let mut paths = Vec::new();
    let mut path_args = false;
    for arg in args {
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && arg == "--cached" {
            cached = true;
        } else if !path_args && arg == "--recursive" {
            recursive = true;
        } else if !path_args && arg == "--quiet" {
            quiet = true;
        } else if !path_args && arg.starts_with('-') {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("unsupported submodule status option '{arg}'"),
            });
        } else {
            paths.push(arg.clone());
        }
    }
    submodule_status_for_repo(&repo, &paths, cached, recursive, quiet, "")
}

fn submodule_status_for_repo(
    repo: &GitRepo,
    paths: &[String],
    cached: bool,
    recursive: bool,
    quiet: bool,
    prefix: &str,
) -> Result<()> {
    let index = read_repo_index(repo)?;
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, std::path::Path::new(path)))
        .collect::<Result<Vec<_>>>()?;
    let mut matched = false;
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.mode == IndexMode::Gitlink)
        .filter(|entry| pathspec_matches(&entry.path, &pathspecs))
    {
        matched = true;
        let path = String::from_utf8_lossy(&entry.path);
        let display_path = format!("{prefix}{path}");
        let submodule_path = repo.root.join(path.as_ref());
        let Some(state) = submodule_head_state(&submodule_path, &entry.id, cached) else {
            if !quiet {
                println!("-{} {display_path}", entry.id.to_hex());
            }
            continue;
        };
        if !quiet {
            println!(
                "{}{} {display_path} ({})",
                state.prefix,
                state.id.to_hex(),
                state.display
            );
        }
        if recursive {
            let submodule_repo = exact_repo_at(&submodule_path).ok_or_else(|| {
                CliError::Message(format!(
                    "not a git repository: {}",
                    submodule_path.display()
                ))
            })?;
            submodule_status_for_repo(
                &submodule_repo,
                &[],
                cached,
                true,
                quiet,
                &format!("{display_path}/"),
            )?;
        }
    }
    if !paths.is_empty() && !matched {
        return Err(CliError::Message(format!(
            "pathspec '{}' did not match any file(s) known to git",
            paths[0]
        )));
    }
    Ok(())
}

pub(crate) fn stash(args: Vec<String>) -> Result<()> {
    let subcommand = args.first().map(String::as_str).unwrap_or("push");
    match subcommand {
        _ if args.is_empty() => stash_push(&args, StashPushUsage::TopLevel),
        "push" => stash_push(&args[1..], StashPushUsage::Push),
        "list" => stash_list(&args[1..]),
        "show" => stash_show(&args[1..]),
        "apply" => {
            let options = parse_stash_reference_options(&args[1..], "apply")?;
            stash_apply(
                false,
                options.stash.as_deref(),
                options.quiet,
                options.index,
                options.no_index,
            )
        }
        "pop" => {
            let options = parse_stash_reference_options(&args[1..], "pop")?;
            stash_apply(
                true,
                options.stash.as_deref(),
                options.quiet,
                options.index,
                options.no_index,
            )
        }
        "drop" => stash_drop(&args[1..]),
        "clear" => stash_clear(),
        "branch" => stash_branch(&args[1..]),
        "create" => stash_create(&args[1..]),
        "store" => stash_store(&args[1..]),
        "export" => stash_export(&args[1..]),
        "import" => stash_import(&args[1..]),
        "save" => stash_save(&args[1..]),
        _ if subcommand.starts_with('-') => {
            validate_top_level_stash_push_assumption(&args)?;
            stash_push(&args, StashPushUsage::TopLevel)
        }
        _ if args.iter().skip(1).any(|arg| arg.starts_with('-')) => {
            Err(stash_unexpected_top_level_token(subcommand))
        }
        _ => stash_push(&args, StashPushUsage::TopLevel),
    }
}

#[derive(Clone, Copy)]
enum StashPushUsage {
    TopLevel,
    Push,
}

const STASH_TOP_LEVEL_USAGE: &str = "\
usage: git stash list [<log-options>]
   or: git stash show [-u | --include-untracked | --only-untracked] [<diff-options>] [<stash>]
   or: git stash drop [-q | --quiet] [<stash>]
   or: git stash pop [--index] [-q | --quiet] [<stash>]
   or: git stash apply [--index] [-q | --quiet] [<stash>]
   or: git stash branch <branchname> [<stash>]
   or: git stash [push [-p | --patch] [-S | --staged] [-k | --[no-]keep-index] [-q | --quiet]
                 [-u | --include-untracked] [-a | --all] [(-m | --message) <message>]
                 [--pathspec-from-file=<file> [--pathspec-file-nul]]
                 [--] [<pathspec>...]]
   or: git stash save [-p | --patch] [-S | --staged] [-k | --[no-]keep-index] [-q | --quiet]
                 [-u | --include-untracked] [-a | --all] [<message>]
   or: git stash clear
   or: git stash create [<message>]
   or: git stash store [(-m | --message) <message>] [-q | --quiet] <commit>

    -k, --[no-]keep-index keep index
    -S, --[no-]staged     stash staged changes only
    -p, --[no-]patch      stash in patch mode
    -q, --[no-]quiet      quiet mode
    -u, --[no-]include-untracked
                          include untracked files in stash
    -a, --[no-]all        include ignore files
    -m, --[no-]message <message>
                          stash message
    --[no-]pathspec-from-file <file>
                          read pathspec from file
    --[no-]pathspec-file-nul
                          with --pathspec-from-file, pathspec elements are separated with NUL character
";

const STASH_PUSH_USAGE: &str = "\
usage: git stash [push [-p | --patch] [-S | --staged] [-k | --[no-]keep-index] [-q | --quiet]
                 [-u | --include-untracked] [-a | --all] [(-m | --message) <message>]
                 [--pathspec-from-file=<file> [--pathspec-file-nul]]
                 [--] [<pathspec>...]]

    -k, --[no-]keep-index keep index
    -S, --[no-]staged     stash staged changes only
    -p, --[no-]patch      stash in patch mode
    -q, --[no-]quiet      quiet mode
    -u, --[no-]include-untracked
                          include untracked files in stash
    -a, --[no-]all        include ignore files
    -m, --[no-]message <message>
                          stash message
    --[no-]pathspec-from-file <file>
                          read pathspec from file
    --[no-]pathspec-file-nul
                          with --pathspec-from-file, pathspec elements are separated with NUL character
";

fn stash_push(args: &[String], usage: StashPushUsage) -> Result<()> {
    let repo = find_repo()?;
    let mut options = StashPushOptions::default();
    let mut cursor = 0usize;
    let mut pathspec_mode = false;
    while cursor < args.len() {
        let arg = args[cursor].as_str();
        if pathspec_mode {
            options
                .pathspecs
                .push(path_arg_to_repo_relative_allow_root(&repo, Path::new(arg))?);
            cursor += 1;
            continue;
        }
        match arg {
            "-h" | "--help" => {
                return stash_usage(usage);
            }
            "-m" | "--message" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash push -m requires a message".into(),
                    });
                };
                options.message = Some(value.clone());
            }
            other if other.starts_with("-m") && other.len() > 2 => {
                options.message = Some(other[2..].to_owned());
            }
            other if other.starts_with("--message=") => {
                let Some(value) = other.strip_prefix("--message=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash push --message requires a message".into(),
                    });
                };
                options.message = Some(value.to_owned());
            }
            "--no-message" => {
                options.message = None;
            }
            "-u" | "--include-untracked" => {
                options.include_untracked = true;
            }
            "--no-include-untracked" => {
                options.include_untracked = false;
                options.include_ignored = false;
            }
            "-q" | "--quiet" => {
                options.quiet = true;
            }
            "--no-quiet" => {
                options.quiet = false;
            }
            "-a" | "--all" => {
                options.include_untracked = true;
                options.include_ignored = true;
            }
            "--no-all" => {
                options.include_untracked = false;
                options.include_ignored = false;
            }
            "-S" | "--staged" => {
                options.staged = true;
            }
            "--no-staged" => {
                options.staged = false;
            }
            "-k" | "--keep-index" => {
                options.keep_index = true;
            }
            "--no-keep-index" => {
                options.keep_index = false;
            }
            "-p" | "--patch" => {
                options.patch = true;
            }
            "--no-patch" => {
                options.patch = false;
            }
            "--pathspec-from-file" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash push --pathspec-from-file requires a file".into(),
                    });
                };
                options.pathspec_from_file = Some(PathBuf::from(value));
            }
            other if other.starts_with("--pathspec-from-file=") => {
                let Some(value) = other.strip_prefix("--pathspec-from-file=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash push --pathspec-from-file requires a file".into(),
                    });
                };
                options.pathspec_from_file = Some(PathBuf::from(value));
            }
            "--no-pathspec-from-file" => {
                if !cfg!(windows) {
                    options.pathspec_from_file = None;
                }
            }
            "--pathspec-file-nul" => {
                options.pathspec_file_nul = true;
            }
            "--no-pathspec-file-nul" => {
                options.pathspec_file_nul = false;
            }
            "--" => {
                pathspec_mode = true;
            }
            other if !other.starts_with('-') => {
                options.pathspecs.push(path_arg_to_repo_relative_allow_root(
                    &repo,
                    Path::new(other),
                )?);
            }
            other => {
                return Err(stash_unknown_option(other, usage));
            }
        }
        cursor += 1;
    }
    if let Some(pathspec_file) = &options.pathspec_from_file {
        let loaded = read_pathspec_file(pathspec_file, options.pathspec_file_nul)?;
        for path in loaded {
            options
                .pathspecs
                .push(path_arg_to_repo_relative_allow_root(&repo, &path)?);
        }
    } else if options.pathspec_file_nul {
        return Err(CliError::Fatal {
            code: 129,
            message: "--pathspec-file-nul requires --pathspec-from-file".into(),
        });
    }

    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = match refs.resolve("HEAD") {
        Ok(id) => id,
        Err(_) if options.quiet => return Err(CliError::Exit(1)),
        Err(_) => {
            return Err(CliError::Stderr {
                code: 1,
                text: "You do not have the initial commit yet\n".into(),
            });
        }
    };
    if options.staged && (options.include_untracked || options.include_ignored) {
        return Err(CliError::Stderr {
            code: 1,
            text: "Can't use --staged and --include-untracked or --all at the same time\n".into(),
        });
    }
    if options.patch {
        return stash_push_patch(&repo, &store, &commit_cache, &refs, &options);
    }
    if options.staged {
        return stash_push_staged(&repo, &store, &commit_cache, &refs, &options);
    }
    let mut snapshot = read_repo_index(&repo)?;
    if snapshot.entries().iter().any(IndexEntry::intent_to_add) {
        return Err(CliError::Exit(1));
    }
    let original_index = snapshot.clone();
    if let Some(error) = stash_locked_index_error(&repo) {
        return Err(error);
    }
    if !options.include_untracked
        && !options.pathspecs.is_empty()
        && !stash_pathspecs_match_known_files(&repo, &snapshot, &options.pathspecs)?
    {
        return Err(stash_unmatched_pathspec_error(&options.pathspecs[0]));
    }
    let mut untracked = if options.include_untracked {
        stash_untracked_paths(&repo, &snapshot, options.include_ignored)?
    } else {
        Vec::new()
    };
    if !options.pathspecs.is_empty() {
        untracked.retain(|path| pathspec_matches(path, &options.pathspecs));
    }
    if stash_selection_clean(&repo, &store, &snapshot, &options.pathspecs)? && untracked.is_empty()
    {
        if !options.quiet {
            println!("No local changes to save");
        }
        return Ok(());
    }
    let head_commit = commit_cache.read_commit(&head_id)?;
    stage_tracked_worktree_changes_matching(
        &repo,
        &store,
        &mut snapshot,
        &options.pathspecs,
        &HashSet::new(),
    )?;
    stage_recreated_tracked_worktree_paths(
        &repo,
        &store,
        &mut snapshot,
        &head_commit.tree,
        &options.pathspecs,
    )?;
    let untracked_index = stash_untracked_index(&repo, &store, &untracked)?;
    let stash_tree = write_tree_from_index(&store, &snapshot)?;
    let author = stash_signature_from_identity(&repo, "GIT_AUTHOR")?;
    let committer = stash_signature_from_identity(&repo, "GIT_COMMITTER")?;
    let message = stash_push_message(&repo, &refs, &head_id, &head_commit, options.message);
    let index_tree = write_tree_from_index(&store, &original_index)?;
    let index_commit = create_stash_index_commit(
        &repo,
        &store,
        &head_id,
        index_tree,
        &message,
        author.clone(),
        committer.clone(),
    )?;
    let untracked_commit = if untracked.is_empty() {
        None
    } else {
        Some(create_stash_untracked_commit(
            &repo,
            &store,
            &head_id,
            write_tree_from_index(&store, &untracked_index)?,
            &head_commit,
            author.clone(),
            committer.clone(),
        )?)
    };
    let mut commit = CommitBuilder::new(stash_tree, author, committer.clone())
        .parent(head_id)
        .parent(index_commit);
    if let Some(untracked_commit) = untracked_commit {
        commit = commit.parent(untracked_commit);
    }
    let commit = commit
        .message(format!("{message}\n").into_bytes())?
        .encode()?;
    let stash_id = store.write_object(GitObjectKind::Commit, &commit)?;
    write_stash_ref_update(&repo, &refs, &stash_id, &committer, &message)?;
    for path in &untracked {
        remove_worktree_path(&repo, path)?;
    }
    reset_stashed_worktree_paths_to_head(&repo, &store, &options.pathspecs)?;
    if options.keep_index {
        if options.pathspecs.is_empty() {
            restore_index_to_worktree(&repo, &store, &original_index)?;
        } else {
            restore_index_paths_to_worktree(&repo, &store, &original_index, &options.pathspecs)?;
        }
    }
    if !options.quiet {
        println!("Saved working directory and index state {message}");
    }
    Ok(())
}

fn stash_unknown_option(option: &str, usage: StashPushUsage) -> CliError {
    let usage_text = match usage {
        StashPushUsage::TopLevel => STASH_TOP_LEVEL_USAGE,
        StashPushUsage::Push => STASH_PUSH_USAGE,
    };
    CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown option `{}`\n{}\n",
            option.trim_start_matches('-'),
            usage_text
        ),
    }
}

fn stash_locked_index_error(repo: &GitRepo) -> Option<CliError> {
    let lock_path = repo.index_path.with_extension("lock");
    lock_path.exists().then(|| CliError::Stderr {
        code: 1,
        text: format!(
            "error: could not write index\nerror: Unable to create '{}': File exists.\n",
            lock_path.display()
        ),
    })
}

fn stage_recreated_tracked_worktree_paths(
    repo: &GitRepo,
    store: &LooseObjectStore,
    snapshot: &mut GitIndex,
    head_tree: &ObjectId,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let tree_cache = TreeObjectCache::new(store);
    let head_index = tree_cache.read_tree_to_index(head_tree)?;
    let snapshot_paths = snapshot
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .map(|entry| entry.path.clone())
        .collect::<HashSet<_>>();
    for entry in head_index.entries().iter().filter(|entry| entry.stage == 0) {
        if snapshot_paths.contains(&entry.path) || !pathspec_matches(&entry.path, pathspecs) {
            continue;
        }
        let absolute = repo
            .root
            .join(String::from_utf8_lossy(&entry.path).as_ref());
        let Ok(metadata) = fs::symlink_metadata(&absolute) else {
            continue;
        };
        if metadata.is_dir() {
            snapshot.upsert(entry.clone())?;
        } else if metadata.is_file() || metadata.file_type().is_symlink() {
            stage_file(repo, store, snapshot, &absolute)?;
        }
    }
    Ok(())
}

fn stash_usage(usage: StashPushUsage) -> Result<()> {
    match usage {
        StashPushUsage::TopLevel => print!("{STASH_TOP_LEVEL_USAGE}\n"),
        StashPushUsage::Push => print!("{STASH_PUSH_USAGE}\n"),
    }
    Err(CliError::Exit(129))
}

fn validate_top_level_stash_push_assumption(args: &[String]) -> Result<()> {
    let mut after_separator = false;
    for arg in args {
        if after_separator {
            continue;
        }
        if arg == "--" {
            after_separator = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        if is_stash_subcommand(arg) {
            return Err(stash_unexpected_top_level_token(arg));
        }
    }
    Ok(())
}

fn stash_unexpected_top_level_token(token: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!(
            "subcommand wasn't specified; 'push' can't be assumed due to unexpected token '{token}'"
        ),
    }
}

fn is_stash_subcommand(value: &str) -> bool {
    matches!(
        value,
        "push"
            | "list"
            | "show"
            | "apply"
            | "pop"
            | "drop"
            | "clear"
            | "branch"
            | "create"
            | "store"
            | "save"
    )
}

fn stash_save(args: &[String]) -> Result<()> {
    let mut push_args = Vec::new();
    let mut message = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = args[cursor].as_str();
        match arg {
            "-h" | "--help" => return stash_usage(StashPushUsage::TopLevel),
            "-q"
            | "--quiet"
            | "--no-quiet"
            | "-u"
            | "--include-untracked"
            | "--no-include-untracked"
            | "-a"
            | "--all"
            | "--no-all"
            | "-S"
            | "--staged"
            | "--no-staged"
            | "-k"
            | "--keep-index"
            | "--no-keep-index"
            | "-p"
            | "--patch"
            | "--no-patch" => push_args.push(arg.to_owned()),
            "--" => {
                message.extend(args.iter().skip(cursor + 1).cloned());
                break;
            }
            other if other.starts_with('-') => {
                return Err(stash_unknown_option(other, StashPushUsage::TopLevel));
            }
            other => message.push(other.to_owned()),
        }
        cursor += 1;
    }
    if !message.is_empty() {
        push_args.push("-m".to_owned());
        push_args.push(message.join(" "));
    }
    stash_push(&push_args, StashPushUsage::TopLevel)
}

fn stash_create(args: &[String]) -> Result<()> {
    let message = if args.is_empty() {
        None
    } else {
        Some(args.join(" "))
    };
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut snapshot = read_repo_index(&repo)?;
    let original_index = snapshot.clone();
    if let Some(error) = stash_locked_index_error(&repo) {
        return Err(error);
    }
    if stash_selection_clean(&repo, &store, &snapshot, &[])? {
        return Ok(());
    }
    stage_tracked_worktree_changes_matching(&repo, &store, &mut snapshot, &[], &HashSet::new())?;
    let stash_id = create_stash_commit(
        &repo,
        &store,
        &commit_cache,
        &refs,
        &snapshot,
        &original_index,
        message,
    )?;
    println!("{}", stash_id.to_hex());
    Ok(())
}

fn stash_store(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    const USAGE: &str = "\"git stash store\" requires one <commit> argument\n";
    let mut message = None;
    let mut _quiet = false;
    let mut commit = None;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = args[cursor].as_str();
        match arg {
            "-m" | "--message" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Stderr {
                        code: 1,
                        text: USAGE.into(),
                    });
                };
                message = Some(value.clone());
            }
            other if other.starts_with("-m") && other.len() > 2 => {
                message = Some(other[2..].to_owned());
            }
            other if other.starts_with("--message=") => {
                let Some(value) = other.strip_prefix("--message=") else {
                    return Err(CliError::Stderr {
                        code: 1,
                        text: USAGE.into(),
                    });
                };
                message = Some(value.to_owned());
            }
            "--no-message" => message = None,
            "-q" | "--quiet" => _quiet = true,
            "--no-quiet" => _quiet = false,
            other if !other.starts_with('-') && commit.is_none() => commit = Some(other.to_owned()),
            other if !other.starts_with('-') => {
                return Err(CliError::Stderr {
                    code: 1,
                    text: USAGE.into(),
                });
            }
            _other => {
                return Err(CliError::Stderr {
                    code: 1,
                    text: USAGE.into(),
                });
            }
        }
        cursor += 1;
    }
    let Some(commit) = commit else {
        return Err(CliError::Stderr {
            code: 1,
            text: USAGE.into(),
        });
    };
    let id = resolve_objectish(&repo, &commit).map_err(|_| CliError::Stderr {
        code: 1,
        text: format!("Cannot update refs/stash with {commit}\n"),
    })?;
    let stash_commit = commit_cache.read_commit(&id)?;
    if stash_commit.parents.len() < 2 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{}' is not a stash-like commit", id.to_hex()),
        });
    }
    let committer = stash_signature_from_identity(&repo, "GIT_COMMITTER")?;
    let message = message.unwrap_or_else(|| "Created via \"git stash store\".".to_owned());
    if stash_entries(&repo, &store)?
        .iter()
        .any(|entry| entry.id == id)
    {
        return Ok(());
    }
    write_stash_ref_update(&repo, &refs, &id, &committer, &message)?;
    Ok(())
}

fn stash_export(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let mut print = false;
    let mut to_ref = None::<String>;
    let mut selectors = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        match args[cursor].as_str() {
            "--print" => print = true,
            "--to-ref" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash export --to-ref requires a ref".into(),
                    });
                };
                to_ref = Some(value.clone());
            }
            value if value.starts_with("--to-ref=") => {
                to_ref = value.strip_prefix("--to-ref=").map(str::to_owned);
            }
            "--" => {}
            value => selectors.push(value.to_owned()),
        }
        cursor += 1;
    }
    if print == to_ref.is_some() {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: exactly one of --print and --to-ref is required\n".into(),
        });
    }

    let empty_index = GitIndex::from_entries(Vec::new())?;
    let empty_tree = write_tree_from_index(&store, &empty_index)?;
    let export_signature = stash_export_signature()?;
    let base = CommitBuilder::new(
        empty_tree.clone(),
        export_signature.clone(),
        export_signature,
    )
    .message(Vec::new())?
    .encode()?;
    let mut previous = store.write_object(GitObjectKind::Commit, &base)?;
    let stash_ids = if selectors.is_empty() {
        stash_entries(&repo, &store)?
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    } else {
        selectors
            .iter()
            .map(|selector| resolve_stash_id(&repo, Some(selector)))
            .collect::<Result<Vec<_>>>()?
    };
    for stash_id in stash_ids.iter().rev() {
        let stash_commit = commit_cache.read_commit(stash_id)?;
        if stash_commit.parents.len() < 2 {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("{} does not look like a stash commit", stash_id.to_hex()),
            });
        }
        let author = signature_from_commit_bytes(&stash_commit.author)?;
        let committer = signature_from_commit_bytes(&stash_commit.committer)?;
        let mut message = b"git stash: ".to_vec();
        message.extend_from_slice(&stash_commit.message);
        if !message.ends_with(b"\n") {
            message.push(b'\n');
        }
        let commit = CommitBuilder::new(empty_tree.clone(), author, committer)
            .parent(previous.clone())
            .parent(stash_id.clone())
            .message(message)?
            .encode()?;
        previous = store.write_object(GitObjectKind::Commit, &commit)?;
    }
    if let Some(ref_name) = to_ref {
        refs.write_ref(&ref_name, &previous)?;
    } else {
        println!("{}", previous.to_hex());
    }
    Ok(())
}

fn stash_import(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "a revision is required".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let mut current = resolve_commitish(&repo, &store, &args[0])?;
    let empty_index = GitIndex::from_entries(Vec::new())?;
    let empty_tree = write_tree_from_index(&store, &empty_index)?;
    let mut stash_ids = Vec::new();
    loop {
        let commit = commit_cache.read_commit(&current)?;
        if commit.tree != empty_tree {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("{} is not a valid exported stash commit", current.to_hex()),
            });
        }
        match commit.parents.as_slice() {
            [] => {
                if commit.author != b"git stash <git@stash> 1000684800 +0000"
                    || commit.committer != b"git stash <git@stash> 1000684800 +0000"
                {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: format!(
                            "found root commit {} with invalid data",
                            current.to_hex()
                        ),
                    });
                }
                break;
            }
            [previous, stash_id] => {
                if !commit.message.starts_with(b"git stash: ") {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: format!(
                            "found stash commit {} without expected prefix",
                            current.to_hex()
                        ),
                    });
                }
                let stash_commit = commit_cache.read_commit(stash_id)?;
                if stash_commit.parents.len() < 2 {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: format!("{} does not look like a stash commit", stash_id.to_hex()),
                    });
                }
                stash_ids.push(stash_id.clone());
                current = previous.clone();
            }
            _ => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("{} is not a valid exported stash commit", current.to_hex()),
                });
            }
        }
    }
    let committer = stash_signature_from_identity(&repo, "GIT_COMMITTER")?;
    for stash_id in stash_ids.into_iter().rev() {
        let stash_commit = commit_cache.read_commit(&stash_id)?;
        let message = commit_subject(&stash_commit.message);
        write_stash_ref_update(&repo, &refs, &stash_id, &committer, &message)?;
    }
    Ok(())
}

fn stash_export_signature() -> Result<zmin_git_core::Signature> {
    Ok(zmin_git_core::Signature::new(
        "git stash",
        "git@stash",
        1_000_684_800,
        "+0000",
    )?)
}

fn create_stash_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    refs: &RefStore,
    snapshot: &GitIndex,
    original_index: &GitIndex,
    message: Option<String>,
) -> Result<ObjectId> {
    let head_id = refs.resolve("HEAD")?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let stash_tree = write_tree_from_index(store, snapshot)?;
    let index_tree = write_tree_from_index(store, original_index)?;
    let message = stash_push_message(repo, refs, &head_id, &head_commit, message);
    create_stash_commit_with_message(repo, store, stash_tree, index_tree, head_id, &message)
}

fn create_stash_commit_with_message(
    repo: &GitRepo,
    store: &LooseObjectStore,
    stash_tree: ObjectId,
    index_tree: ObjectId,
    head_id: ObjectId,
    message: &str,
) -> Result<ObjectId> {
    let author = stash_signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = stash_signature_from_identity(repo, "GIT_COMMITTER")?;
    let index_commit = create_stash_index_commit(
        repo,
        store,
        &head_id,
        index_tree,
        message,
        author.clone(),
        committer.clone(),
    )?;
    let commit = CommitBuilder::new(stash_tree, author, committer)
        .parent(head_id)
        .parent(index_commit)
        .message(format!("{message}\n").into_bytes())?
        .encode()?;
    Ok(store.write_object(GitObjectKind::Commit, &commit)?)
}

fn stash_signature_from_identity(repo: &GitRepo, prefix: &str) -> Result<zmin_git_core::Signature> {
    match signature_from_identity(repo, prefix) {
        Ok(signature) => Ok(signature),
        Err(_) => {
            let date = std::env::var(format!("{prefix}_DATE")).ok();
            let (timestamp, timezone) = signature_date(date.as_deref())?;
            Ok(zmin_git_core::Signature::new(
                "git stash",
                "git@stash",
                timestamp,
                timezone,
            )?)
        }
    }
}

fn create_stash_index_commit(
    _repo: &GitRepo,
    store: &LooseObjectStore,
    head_id: &ObjectId,
    index_tree: ObjectId,
    message: &str,
    author: zmin_git_core::Signature,
    committer: zmin_git_core::Signature,
) -> Result<ObjectId> {
    let commit = CommitBuilder::new(index_tree, author, committer)
        .parent(head_id.clone())
        .message(format!("index on {message}\n").into_bytes())?
        .encode()?;
    Ok(store.write_object(GitObjectKind::Commit, &commit)?)
}

fn stash_untracked_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    paths: &[Vec<u8>],
) -> Result<GitIndex> {
    let mut index = GitIndex::new();
    for path in paths {
        let absolute = repo.root.join(String::from_utf8_lossy(path).as_ref());
        stage_file(repo, store, &mut index, &absolute)?;
    }
    Ok(index)
}

fn create_stash_untracked_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    head_id: &ObjectId,
    untracked_tree: ObjectId,
    head_commit: &zmin_git_core::CommitObject,
    author: zmin_git_core::Signature,
    committer: zmin_git_core::Signature,
) -> Result<ObjectId> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let branch = current_branch_ref(&refs)
        .ok()
        .flatten()
        .map(|name| branch_display_name(&name))
        .unwrap_or_else(|| "(no branch)".to_owned());
    let message = format!(
        "untracked files on {branch}: {} {}\n",
        short_object_id(head_id),
        commit_subject(&head_commit.message)
    );
    let commit = CommitBuilder::new(untracked_tree, author, committer)
        .message(message.into_bytes())?
        .encode()?;
    Ok(store.write_object(GitObjectKind::Commit, &commit)?)
}

fn stash_selection_clean(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<bool> {
    let runtime = CliPrimitiveRuntime::new_default(repo);
    if pathspecs.is_empty() {
        return worktree_clean(repo, store);
    }
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    if diff_indexes(&head_index, index)?
        .iter()
        .any(|entry| pathspec_matches(&entry.path, pathspecs))
    {
        return Ok(false);
    }
    Ok(worktree_status(repo, index)?
        .iter()
        .all(|(path, _)| !pathspec_matches(path, pathspecs)))
}

fn stash_pathspecs_match_known_files(
    repo: &GitRepo,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<bool> {
    if index
        .entries()
        .iter()
        .any(|entry| entry.stage == 0 && pathspec_matches(&entry.path, pathspecs))
    {
        return Ok(true);
    }
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    Ok(head_index
        .entries()
        .iter()
        .any(|entry| entry.stage == 0 && pathspec_matches(&entry.path, pathspecs)))
}

fn stash_unmatched_pathspec_error(pathspec: &[u8]) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!(
            "error: pathspec ':(prefix:0){}' did not match any file(s) known to git\nDid you forget to 'git add'?\n",
            String::from_utf8_lossy(pathspec)
        ),
    }
}

fn reset_stashed_worktree_paths_to_head(
    repo: &GitRepo,
    store: &LooseObjectStore,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let runtime = CliPrimitiveRuntime::new_default(repo);
    if pathspecs.is_empty() {
        return reset_worktree_to_head(repo, store);
    }
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let mut current_index = read_repo_index(repo)?;
    let head_paths = head_index
        .entries()
        .iter()
        .map(|entry| entry.path.as_slice())
        .collect::<HashSet<_>>();
    let mut checkout_entries = Vec::new();
    let selected_current = current_index
        .entries()
        .iter()
        .filter(|entry| pathspec_matches(&entry.path, pathspecs))
        .map(|entry| entry.path.to_vec())
        .collect::<Vec<_>>();
    for path in selected_current {
        if !head_paths.contains(path.as_slice()) {
            current_index.remove_path(&path)?;
            remove_worktree_path(repo, &path)?;
        }
    }
    for entry in head_index
        .entries()
        .iter()
        .filter(|entry| pathspec_matches(&entry.path, pathspecs))
    {
        current_index.upsert(entry.clone())?;
        checkout_entries.push(entry.clone());
    }
    let checkout = GitIndex::from_entries(checkout_entries)?;
    checkout_index(
        store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    refresh_tracked_index_metadata_matching(repo, &mut current_index, pathspecs)?;
    current_index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn reset_staged_paths_to_head(
    repo: &GitRepo,
    store: &LooseObjectStore,
    changes: &[zmin_git_core::IndexDiffEntry],
) -> Result<()> {
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let mut current_index = read_repo_index(repo)?;
    let mut checkout_entries = Vec::new();
    for change in changes {
        match find_index_entry(&head_index, &change.path) {
            Some(entry) => {
                current_index.upsert(entry.clone())?;
                checkout_entries.push(entry.clone());
            }
            None => {
                current_index.remove_path(&change.path)?;
                remove_worktree_path(repo, &change.path)?;
            }
        }
    }
    let checkout = GitIndex::from_entries(checkout_entries)?;
    checkout_index(
        store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    let refreshed_paths = changes
        .iter()
        .map(|change| change.path.to_vec())
        .collect::<Vec<_>>();
    refresh_tracked_index_metadata_matching(repo, &mut current_index, &refreshed_paths)?;
    current_index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn restore_index_to_worktree(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_index: &GitIndex,
) -> Result<()> {
    let current_index = read_repo_index(repo)?;
    remove_tracked_paths_missing_from_target(repo, &current_index, target_index)?;
    target_index.write_to_path(&repo.index_path)?;
    checkout_index(
        store,
        target_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    Ok(())
}

fn restore_index_paths_to_worktree(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target_index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let mut current_index = read_repo_index(repo)?;
    let target_paths = target_index
        .entries()
        .iter()
        .filter(|entry| pathspec_matches(&entry.path, pathspecs))
        .map(|entry| entry.path.as_slice())
        .collect::<HashSet<_>>();
    let selected_current = current_index
        .entries()
        .iter()
        .filter(|entry| pathspec_matches(&entry.path, pathspecs))
        .map(|entry| entry.path.to_vec())
        .collect::<Vec<_>>();
    for path in selected_current {
        if !target_paths.contains(path.as_slice()) {
            current_index.remove_path(&path)?;
            remove_worktree_path(repo, &path)?;
        }
    }
    let checkout_entries = target_index
        .entries()
        .iter()
        .filter(|entry| pathspec_matches(&entry.path, pathspecs))
        .cloned()
        .collect::<Vec<_>>();
    for entry in &checkout_entries {
        current_index.upsert(entry.clone())?;
    }
    let checkout = GitIndex::from_entries(checkout_entries)?;
    checkout_index(
        store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    refresh_tracked_index_metadata_matching(repo, &mut current_index, pathspecs)?;
    current_index.write_to_path(&repo.index_path)?;
    Ok(())
}

#[derive(Debug, Default)]
struct StashPushOptions {
    message: Option<String>,
    include_untracked: bool,
    include_ignored: bool,
    patch: bool,
    staged: bool,
    keep_index: bool,
    quiet: bool,
    pathspec_from_file: Option<PathBuf>,
    pathspec_file_nul: bool,
    pathspecs: Vec<Vec<u8>>,
}

fn stash_push_staged(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    refs: &RefStore,
    options: &StashPushOptions,
) -> Result<()> {
    let index = read_repo_index(repo)?;
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let staged_changes = diff_indexes(&head_index, &index)?
        .into_iter()
        .filter(|entry| pathspec_matches(&entry.path, &options.pathspecs))
        .collect::<Vec<_>>();
    if staged_changes.is_empty() {
        if !options.quiet {
            println!("No local changes to save");
        }
        return Ok(());
    }
    let mut stash_index = head_index.clone();
    for change in &staged_changes {
        match change.status {
            IndexDiffStatus::Added | IndexDiffStatus::Modified => {
                let entry = find_index_entry(&index, &change.path)
                    .ok_or_else(|| CliError::Fatal {
                        code: 128,
                        message: format!(
                            "missing staged index entry for {}",
                            String::from_utf8_lossy(&change.path)
                        ),
                    })?
                    .clone();
                stash_index.upsert(entry)?;
            }
            IndexDiffStatus::Deleted => {
                stash_index.remove_path(&change.path)?;
            }
            IndexDiffStatus::Copied | IndexDiffStatus::Renamed => {}
        }
    }
    let stash_tree = write_tree_from_index(store, &stash_index)?;
    let head_id = refs.resolve("HEAD")?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let author = stash_signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = stash_signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = stash_push_message(repo, refs, &head_id, &head_commit, options.message.clone());
    let commit = CommitBuilder::new(stash_tree, author, committer.clone())
        .parent(head_id)
        .message(format!("{message}\n").into_bytes())?
        .encode()?;
    let stash_id = store.write_object(GitObjectKind::Commit, &commit)?;
    write_stash_ref_update(repo, refs, &stash_id, &committer, &message)?;
    reset_staged_paths_to_head(repo, store, &staged_changes)?;
    if !options.quiet {
        println!("Saved working directory and index state {message}");
    }
    Ok(())
}

fn stash_push_patch(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    refs: &RefStore,
    options: &StashPushOptions,
) -> Result<()> {
    let current_index = read_repo_index(repo)?;
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let worktree_index = worktree_index_snapshot(repo, &current_index)?;
    let entries = diff_indexes(&head_index, &worktree_index)?
        .into_iter()
        .filter(|entry| pathspec_matches(&entry.path, &options.pathspecs))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        println!("No local changes to save");
        return Ok(());
    }
    let mut patch_bytes = Vec::new();
    write_patch_entries(
        &mut patch_bytes,
        repo,
        store,
        &head_index,
        &worktree_index,
        &entries,
        PatchFormatOptions::worktree(),
    )?;
    let patches = patch_commands::parse_apply_patches(&patch_bytes)?;
    let mut answers = patch_commands::PatchAnswers::read()?;
    let mut stash_index = head_index.clone();
    let mut selected_any = false;
    let mut worktree_updates = Vec::new();
    for patch in patches {
        let target_path = patch
            .new_path
            .as_ref()
            .or(patch.old_path.as_ref())
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "patch has no target path".into(),
            })?
            .clone();
        let selected_hunks = patch_commands::select_patch_hunks(&patch, &mut answers)?;
        if selected_hunks.is_empty() {
            continue;
        }
        selected_any = true;
        let rejected_hunks = patch_commands::rejected_hunks_for_selection(&patch, &selected_hunks);
        let base_entry = find_index_entry(&head_index, &target_path);
        let base = base_entry
            .map(|entry| read_index_entry_content(store, entry))
            .transpose()?
            .unwrap_or_default();
        let selected_content =
            patch_commands::apply_hunks_to_content(&base, &selected_hunks, &target_path)?;
        let remaining_content =
            patch_commands::apply_hunks_to_content(&base, &rejected_hunks, &target_path)?;
        let mode = patch
            .new_mode
            .or_else(|| find_index_entry(&worktree_index, &target_path).map(|entry| entry.mode))
            .or_else(|| base_entry.map(|entry| entry.mode))
            .unwrap_or(IndexMode::File);
        if patch.deleted && selected_hunks.len() == patch.hunks.len() {
            stash_index.remove_path(&target_path)?;
        } else {
            upsert_index_content(
                store,
                &mut stash_index,
                target_path.clone(),
                selected_content,
                mode,
            )?;
        }
        worktree_updates.push(PatchWorktreeUpdate {
            path: target_path,
            content: remaining_content,
            remove_if_empty_untracked: base_entry.is_none(),
        });
    }
    if !selected_any {
        return Err(CliError::Stderr {
            code: 1,
            text: "No changes selected\n".into(),
        });
    }
    let stash_tree = write_tree_from_index(store, &stash_index)?;
    let head_id = refs.resolve("HEAD")?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let author = stash_signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = stash_signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = stash_push_message(repo, refs, &head_id, &head_commit, options.message.clone());
    let commit = CommitBuilder::new(stash_tree, author, committer.clone())
        .parent(head_id)
        .message(format!("{message}\n").into_bytes())?
        .encode()?;
    let stash_id = store.write_object(GitObjectKind::Commit, &commit)?;
    write_stash_ref_update(repo, refs, &stash_id, &committer, &message)?;
    for update in worktree_updates {
        write_patch_worktree_update(repo, update)?;
    }
    if !options.quiet {
        println!("Saved working directory and index state {message}");
    }
    Ok(())
}

#[derive(Debug)]
struct PatchWorktreeUpdate {
    path: Vec<u8>,
    content: Vec<u8>,
    remove_if_empty_untracked: bool,
}

fn write_patch_worktree_update(repo: &GitRepo, update: PatchWorktreeUpdate) -> Result<()> {
    let absolute = repo
        .root
        .join(String::from_utf8_lossy(&update.path).as_ref());
    if update.remove_if_empty_untracked && update.content.is_empty() {
        return remove_worktree_path(repo, &update.path);
    }
    if let Some(parent) = absolute.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(absolute, update.content)?;
    Ok(())
}

fn stash_list(args: &[String]) -> Result<()> {
    let mut format = StashListFormat::Default;
    let mut max_count = None;
    let mut skip = 0usize;
    let mut grep = Vec::new();
    let mut invert_grep = false;
    let mut all_match = false;
    let mut ignore_case = false;
    let mut fixed_strings = false;
    let mut show_patch = false;
    let mut combined_diff = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        match arg.as_str() {
            "--oneline" => format = StashListFormat::Oneline,
            "--walk-reflogs" | "--no-walk" => {}
            value if value.starts_with("--pretty=") || value.starts_with("--format=") => {
                format = StashListFormat::Custom(parse_stash_list_format(value)?);
            }
            "-p" | "--patch" => show_patch = true,
            "--cc" => combined_diff = true,
            "-n" | "--max-count" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: "option requires a value: n".into(),
                    });
                };
                max_count = Some(parse_stash_list_count(value)?);
            }
            value if value.starts_with("--max-count=") => {
                max_count = Some(parse_stash_list_count(&value["--max-count=".len()..])?);
            }
            "--skip" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: "option requires a value: skip".into(),
                    });
                };
                skip = parse_stash_list_count(value)?;
            }
            value if value.starts_with("--skip=") => {
                skip = parse_stash_list_count(&value["--skip=".len()..])?;
            }
            "--grep" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 1,
                        message: "option requires a value: grep".into(),
                    });
                };
                grep.push(value.clone());
            }
            value if value.starts_with("--grep=") => {
                grep.push(value["--grep=".len()..].to_owned());
            }
            "--invert-grep" => {
                invert_grep = true;
            }
            "--all-match" => {
                all_match = true;
            }
            "-i" | "--regexp-ignore-case" => {
                ignore_case = true;
            }
            "-E" | "--extended-regexp" => {
                fixed_strings = false;
            }
            "-F" | "--fixed-strings" => {
                fixed_strings = true;
            }
            value
                if value.len() > 1
                    && value.starts_with('-')
                    && value[1..].chars().all(|ch| ch.is_ascii_digit()) =>
            {
                max_count = Some(parse_stash_list_count(&value[1..])?);
            }
            value => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("unrecognized argument: {value}"),
                });
            }
        }
        cursor += 1;
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let grep = compile_stash_list_grep_patterns(&grep, fixed_strings, ignore_case)?;
    for (index, entry) in stash_entries(&repo, &store)?
        .iter()
        .enumerate()
        .filter(|(_, entry)| stash_list_grep_matches(&entry.message, &grep, invert_grep, all_match))
        .skip(skip)
        .take(max_count.unwrap_or(usize::MAX))
    {
        match &format {
            StashListFormat::Default => println!("stash@{{{index}}}: {}", entry.message),
            StashListFormat::Oneline => {
                println!(
                    "{} refs/stash@{{{index}}}: {}",
                    short_object_id(&entry.id),
                    entry.message
                );
            }
            StashListFormat::Custom(format) => {
                println!(
                    "{}",
                    render_stash_list_format(format, index, entry, &store)?
                );
            }
        }
        if show_patch {
            println!();
            if combined_diff {
                print_stash_combined_diff(&repo, &store, &entry.id)?;
            } else {
                stash_show(&["-p".to_owned(), format!("stash@{{{index}}}")])?;
            }
        }
    }
    Ok(())
}

fn print_stash_combined_diff(
    repo: &GitRepo,
    store: &LooseObjectStore,
    stash_id: &ObjectId,
) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let stash_commit = commit_cache.read_commit(stash_id)?;
    let Some(base_parent) = stash_commit.parents.first() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "stash commit has no base parent".into(),
        });
    };
    let Some(index_parent) = stash_commit.parents.get(1) else {
        return Err(CliError::Fatal {
            code: 128,
            message: "stash commit has no index parent".into(),
        });
    };
    let base_commit = commit_cache.read_commit(base_parent)?;
    let index_commit = commit_cache.read_commit(index_parent)?;
    let base_index = read_commit_tree_index_cached(&tree_cache, &base_commit)?;
    let index_parent_index = read_commit_tree_index_cached(&tree_cache, &index_commit)?;
    let stash_index = read_commit_tree_index_cached(&tree_cache, &stash_commit)?;
    let mut paths = diff_indexes(&base_index, &stash_index)?
        .into_iter()
        .map(|entry| entry.path.to_vec())
        .collect::<BTreeSet<_>>();
    paths.extend(
        diff_indexes(&index_parent_index, &stash_index)?
            .into_iter()
            .map(|entry| entry.path.to_vec()),
    );
    for path in paths {
        let Some(base_entry) = find_index_entry(&base_index, &path) else {
            continue;
        };
        let Some(index_entry) = find_index_entry(&index_parent_index, &path) else {
            continue;
        };
        let Some(stash_entry) = find_index_entry(&stash_index, &path) else {
            continue;
        };
        let base_content = read_index_entry_content(store, base_entry)?;
        let index_content = read_index_entry_content(store, index_entry)?;
        let stash_content = read_index_entry_content(store, stash_entry)?;
        let Some(base_line) = single_line_diff_content(&base_content) else {
            continue;
        };
        let Some(index_line) = single_line_diff_content(&index_content) else {
            continue;
        };
        let Some(stash_line) = single_line_diff_content(&stash_content) else {
            continue;
        };
        let path_display = String::from_utf8_lossy(&path);
        println!("diff --cc {path_display}");
        println!(
            "index {},{}..{}",
            base_entry.id.short_hex(7),
            index_entry.id.short_hex(7),
            stash_entry.id.short_hex(7)
        );
        println!("--- a/{path_display}");
        println!("+++ b/{path_display}");
        println!("@@@ -1,1 -1,1 +1,1 @@@");
        println!("- {base_line}");
        println!(" -{index_line}");
        println!("++{stash_line}");
    }
    let _ = repo;
    Ok(())
}

fn single_line_diff_content(content: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(content).ok()?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    (!text.contains('\n')).then(|| text.to_owned())
}

enum StashListGrepPattern {
    Fixed(String),
    Regex(Regex),
}

impl StashListGrepPattern {
    fn is_match(&self, message: &str) -> bool {
        match self {
            Self::Fixed(pattern) => message.contains(pattern),
            Self::Regex(regex) => regex.is_match(message.as_bytes()),
        }
    }
}

fn compile_stash_list_grep_patterns(
    patterns: &[String],
    fixed_strings: bool,
    ignore_case: bool,
) -> Result<Vec<StashListGrepPattern>> {
    patterns
        .iter()
        .map(|pattern| {
            if fixed_strings {
                let pattern = if ignore_case {
                    pattern.to_ascii_lowercase()
                } else {
                    pattern.clone()
                };
                return Ok(StashListGrepPattern::Fixed(pattern));
            }
            let pattern = if ignore_case {
                format!("(?i:{pattern})")
            } else {
                pattern.clone()
            };
            Regex::new(&pattern)
                .map(StashListGrepPattern::Regex)
                .map_err(|error| CliError::Fatal {
                    code: 128,
                    message: format!("invalid grep pattern: {error}"),
                })
        })
        .collect()
}

fn stash_list_grep_matches(
    message: &str,
    patterns: &[StashListGrepPattern],
    invert_grep: bool,
    all_match: bool,
) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let lowered_message = message.to_ascii_lowercase();
    let message = patterns
        .iter()
        .find_map(|pattern| matches!(pattern, StashListGrepPattern::Fixed(_)).then_some(()))
        .map(|_| lowered_message.as_str())
        .unwrap_or(message);
    let matched = if all_match {
        patterns.iter().all(|pattern| pattern.is_match(message))
    } else {
        patterns.iter().any(|pattern| pattern.is_match(message))
    };
    matched ^ invert_grep
}

fn parse_stash_list_count(value: &str) -> Result<usize> {
    value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 1,
        message: format!("'{value}': not an integer"),
    })
}

#[derive(Debug, Clone)]
enum StashListFormat {
    Default,
    Oneline,
    Custom(String),
}

fn parse_stash_list_format(option: &str) -> Result<String> {
    let raw = option
        .strip_prefix("--pretty=")
        .or_else(|| option.strip_prefix("--format="))
        .unwrap_or(option);
    let format = raw
        .strip_prefix("format:")
        .or_else(|| raw.strip_prefix("tformat:"))
        .unwrap_or(raw);
    Ok(format.to_owned())
}

fn render_stash_list_format(
    format: &str,
    index: usize,
    entry: &StashEntry,
    store: &LooseObjectStore,
) -> Result<String> {
    let commit_cache = CommitObjectCache::new(store);
    let commit = commit_cache.read_commit(&entry.id)?;
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        let Some(atom) = chars.next() else {
            out.push('%');
            break;
        };
        match atom {
            '%' => out.push('%'),
            'n' => out.push('\n'),
            'H' => out.push_str(&entry.id.to_hex()),
            'h' => out.push_str(&entry.id.short_hex(7)),
            's' => out.push_str(&entry.message),
            'B' => out.push_str(&String::from_utf8_lossy(&commit.message)),
            'b' => out.push_str(&stash_format_body(&commit.message)),
            'f' => out.push_str(&stash_format_sanitized_subject(&commit.message)),
            'D' if index == 0 => out.push_str("refs/stash"),
            'D' => {}
            'd' if index == 0 => out.push_str(" (refs/stash)"),
            'd' => {}
            'e' | 'N' => {}
            'm' => out.push('>'),
            'S' => out.push_str("%S"),
            'P' => {
                for (index, parent) in commit.parents.iter().enumerate() {
                    if index > 0 {
                        out.push(' ');
                    }
                    out.push_str(&parent.to_hex());
                }
            }
            'p' => {
                for (index, parent) in commit.parents.iter().enumerate() {
                    if index > 0 {
                        out.push(' ');
                    }
                    out.push_str(&parent.short_hex(7));
                }
            }
            'T' => out.push_str(&commit.tree.to_hex()),
            't' => out.push_str(&commit.tree.short_hex(7)),
            'g' => {
                let Some(reflog_atom) = chars.next() else {
                    return Err(unsupported_stash_list_format_atom("%g"));
                };
                match reflog_atom {
                    'd' => out.push_str(&format!("stash@{{{index}}}")),
                    'D' => out.push_str(&format!("refs/stash@{{{index}}}")),
                    's' => out.push_str(&entry.message),
                    'N' | 'n' => out.push_str(&signature_name(entry.reflog_identity.as_bytes())),
                    'E' | 'e' => out.push_str(&signature_email(entry.reflog_identity.as_bytes())),
                    'K' => out.push_str("%gK"),
                    _ => stash_format_literal_atom(&mut out, "g", reflog_atom),
                }
            }
            'x' => {
                let high = chars
                    .next()
                    .ok_or_else(|| unsupported_stash_list_format_atom("%x"))?;
                let low = chars
                    .next()
                    .ok_or_else(|| unsupported_stash_list_format_atom("%x"))?;
                let byte = parse_hex_byte(high, low)
                    .ok_or_else(|| unsupported_stash_list_format_atom("%x"))?;
                out.push(byte as char);
            }
            'a' => {
                let Some(next) = chars.next() else {
                    return Err(unsupported_stash_list_format_atom("%a"));
                };
                match next {
                    'n' => out.push_str(&signature_name(&commit.author)),
                    'e' => out.push_str(&signature_email(&commit.author)),
                    'l' | 'L' => out.push_str(&signature_email_local_part(&commit.author)),
                    't' => {
                        out.push_str(&signature_timestamp(&commit.author).unwrap_or(0).to_string())
                    }
                    'd' => out.push_str(&signature_log_date(&commit.author)?),
                    'D' => out.push_str(&signature_mail_date(&commit.author)?),
                    'h' => out.push_str(&signature_human_date(&commit.author)?),
                    'r' => out.push_str(&signature_relative_date(&commit.author)?),
                    'i' => out.push_str(&signature_blame_date(&commit.author)?),
                    'I' => out.push_str(&signature_strict_iso_date(&commit.author)?),
                    's' => out.push_str(&signature_short_date(&commit.author)?),
                    _ => stash_format_literal_atom(&mut out, "a", next),
                }
            }
            'c' => {
                let Some(next) = chars.next() else {
                    return Err(unsupported_stash_list_format_atom("%c"));
                };
                match next {
                    'n' => out.push_str(&signature_name(&commit.committer)),
                    'e' => out.push_str(&signature_email(&commit.committer)),
                    'l' | 'L' => out.push_str(&signature_email_local_part(&commit.committer)),
                    't' => out.push_str(
                        &signature_timestamp(&commit.committer)
                            .unwrap_or(0)
                            .to_string(),
                    ),
                    'd' => out.push_str(&signature_log_date(&commit.committer)?),
                    'D' => out.push_str(&signature_mail_date(&commit.committer)?),
                    'h' => out.push_str(&signature_human_date(&commit.committer)?),
                    'r' => out.push_str(&signature_relative_date(&commit.committer)?),
                    'i' => out.push_str(&signature_blame_date(&commit.committer)?),
                    'I' => out.push_str(&signature_strict_iso_date(&commit.committer)?),
                    's' => out.push_str(&signature_short_date(&commit.committer)?),
                    _ => stash_format_literal_atom(&mut out, "c", next),
                }
            }
            'G' => {
                let Some(next) = chars.next() else {
                    return Err(unsupported_stash_list_format_atom("%G"));
                };
                match next {
                    '?' => out.push('N'),
                    'K' | 'F' | 'P' | 'S' | 'G' => {}
                    'T' => out.push_str("undefined"),
                    _ => stash_format_literal_atom(&mut out, "G", next),
                }
            }
            'C' | 'w' | '<' | '>' => {
                return Err(unsupported_stash_list_format_atom(&format!("%{atom}")));
            }
            _ => stash_format_literal_atom(&mut out, "", atom),
        }
    }
    Ok(out)
}

fn stash_format_literal_atom(out: &mut String, prefix: &str, atom: char) {
    out.push('%');
    out.push_str(prefix);
    out.push(atom);
}

fn stash_format_body(message: &[u8]) -> String {
    let message = message.strip_suffix(b"\n").unwrap_or(message);
    let Some(blank_line) = message.windows(2).position(|window| window == b"\n\n") else {
        return String::new();
    };
    String::from_utf8_lossy(&message[blank_line + 2..]).into_owned()
}

fn stash_format_sanitized_subject(message: &[u8]) -> String {
    let subject = commit_subject(message);
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in subject.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            slug.push(ch);
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    slug.trim_matches('-').to_owned()
}

fn signature_email_local_part(signature: &[u8]) -> String {
    let email = signature_email(signature);
    email.split_once('@')
        .map_or(email.as_str(), |(local, _)| local)
        .to_owned()
}

fn signature_short_date(signature: &[u8]) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit timestamp is out of range".into(),
    })?;
    Ok(utc.with_timezone(&offset).format("%Y-%m-%d").to_string())
}

fn signature_human_date(signature: &[u8]) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit timestamp is out of range".into(),
    })?;
    let commit = utc.with_timezone(&offset);
    let now = git_test_date_now()
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .map(|timestamp| timestamp.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    if commit.year() == now.year()
        && commit.month() == now.month()
        && commit.day() == now.day()
        && timestamp <= now.timestamp()
    {
        return Ok(signature_relative_date(signature)?);
    }
    if commit.year() == now.year()
        && commit.month() == now.month()
        && commit.day() < now.day()
        && commit.day() + 5 > now.day()
    {
        return Ok(commit.format("%a %H:%M").to_string());
    }
    if commit.year() == now.year() {
        return Ok(commit.format("%b %-d %H:%M").to_string());
    }
    Ok(commit.format("%b %-d %Y").to_string())
}

fn signature_relative_date(signature: &[u8]) -> Result<String> {
    let timestamp = signature_timestamp(signature).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid date".into(),
    })?;
    let now = git_test_date_now().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
            .unwrap_or(0)
    });
    if now < timestamp {
        return Ok("in the future".to_owned());
    }
    let mut diff = now - timestamp;
    if diff < 90 {
        return Ok(plural_date(diff, "second"));
    }
    diff = (diff + 30) / 60;
    if diff < 90 {
        return Ok(plural_date(diff, "minute"));
    }
    diff = (diff + 30) / 60;
    if diff < 36 {
        return Ok(plural_date(diff, "hour"));
    }
    diff = (diff + 12) / 24;
    if diff < 14 {
        return Ok(plural_date(diff, "day"));
    }
    if diff < 70 {
        return Ok(plural_date((diff + 3) / 7, "week"));
    }
    if diff < 365 {
        return Ok(plural_date((diff + 15) / 30, "month"));
    }
    if diff < 1825 {
        let total_months = (diff * 12 * 2 + 365) / (365 * 2);
        let years = total_months / 12;
        let months = total_months % 12;
        if months == 0 {
            return Ok(plural_date(years, "year"));
        }
        let year_unit = if years == 1 { "year" } else { "years" };
        let month_unit = if months == 1 { "month" } else { "months" };
        return Ok(format!("{years} {year_unit}, {months} {month_unit} ago"));
    }
    Ok(plural_date((diff + 183) / 365, "year"))
}

fn plural_date(value: i64, unit: &str) -> String {
    let suffix = if value == 1 { "" } else { "s" };
    format!("{value} {unit}{suffix} ago")
}

fn git_test_date_now() -> Option<i64> {
    std::env::var("GIT_TEST_DATE_NOW").ok()?.parse().ok()
}

fn signature_strict_iso_date(signature: &[u8]) -> Result<String> {
    let (timestamp, timezone) =
        signature_timestamp_timezone(signature).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit has invalid date".into(),
        })?;
    let offset = parse_timezone_offset(timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit timestamp is out of range".into(),
    })?;
    Ok(utc
        .with_timezone(&offset)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn unsupported_stash_list_format_atom(atom: &str) -> CliError {
    CliError::Fatal {
        code: 1,
        message: format!("unsupported stash list format atom '{atom}'"),
    }
}

fn parse_hex_byte(high: char, low: char) -> Option<u8> {
    let high = high.to_digit(16)?;
    let low = low.to_digit(16)?;
    Some(((high << 4) | low) as u8)
}

fn stash_show(args: &[String]) -> Result<()> {
    let mut show_stat = true;
    let mut show_patch = false;
    let mut show_numstat = false;
    let mut show_shortstat = false;
    let mut show_summary = false;
    let mut show_raw = false;
    let mut only_untracked = false;
    let mut nul_terminated = false;
    let mut name_only = false;
    let mut name_status = false;
    let mut abbrev_len = None;
    let mut full_index = false;
    let mut quiet = false;
    let mut exit_code = false;
    let mut binary = false;
    let mut irreversible_delete = false;
    let mut submodule_format = SubmoduleDiffFormat::Short;
    let mut ignore_submodules = IgnoreSubmodulesMode::None;
    let mut patch_default_requested = false;
    let mut diff_format_explicit = false;
    let mut old_prefix = "a/".to_owned();
    let mut new_prefix = "b/".to_owned();
    let mut unified_context = 3usize;
    let mut inter_hunk_context = 0usize;
    let mut unified_context_explicit = false;
    let mut whitespace_mode = DiffWhitespaceMode::None;
    let mut ignore_matching_lines = Vec::new();
    let mut minimal = false;
    let mut patience = false;
    let mut histogram = false;
    let mut diff_algorithm = None;
    let mut anchored = Vec::new();
    let mut diff_filter = DiffFilter::default();
    let mut detect_renames = Some(50);
    let mut break_rewrites = None;
    let mut detect_copies = None;
    let mut find_copies_harder = false;
    let mut pickaxe_string = None;
    let mut pickaxe_regex = None;
    let mut pickaxe_regex_mode = false;
    let mut pickaxe_all = false;
    let mut order_file = None;
    let mut skip_to = None;
    let mut rotate_to = None;
    let mut stash = None;
    for arg in args {
        match arg.as_str() {
            "-p" | "--patch" => {
                show_stat = false;
                show_patch = true;
                diff_format_explicit = true;
            }
            "--stat" => {
                show_stat = true;
                diff_format_explicit = true;
            }
            "--patch-with-stat" => {
                show_stat = true;
                show_patch = true;
                diff_format_explicit = true;
            }
            "--patch-with-raw" => {
                show_raw = true;
                show_patch = true;
                show_stat = false;
                diff_format_explicit = true;
            }
            "--numstat" => {
                show_numstat = true;
                show_stat = false;
                diff_format_explicit = true;
            }
            "--shortstat" => {
                show_shortstat = true;
                show_stat = false;
                diff_format_explicit = true;
            }
            "--summary" => {
                show_summary = true;
                show_stat = false;
                diff_format_explicit = true;
            }
            "--raw" => {
                show_raw = true;
                show_stat = false;
                diff_format_explicit = true;
            }
            "-z" => {
                nul_terminated = true;
            }
            "--abbrev" => {
                abbrev_len = None;
            }
            value if value.starts_with("--abbrev=") => {
                let Some(value) = value.strip_prefix("--abbrev=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash show --abbrev expects a value".into(),
                    });
                };
                abbrev_len = Some(parse_stash_show_abbrev(value)?);
            }
            "--no-abbrev" => {
                abbrev_len = Some(GitHashAlgorithm::Sha1.digest_len() * 2);
            }
            "--full-index" => {
                full_index = true;
            }
            "--no-full-index" => {
                full_index = false;
            }
            "--binary" => {
                binary = true;
                patch_default_requested = true;
            }
            "-D" | "--irreversible-delete" => {
                irreversible_delete = true;
                patch_default_requested = true;
            }
            "--submodule" => {
                submodule_format = SubmoduleDiffFormat::Log;
                patch_default_requested = true;
            }
            value if value.starts_with("--submodule=") => {
                let Some(value) = value.strip_prefix("--submodule=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash show --submodule expects a value".into(),
                    });
                };
                submodule_format = parse_submodule_diff_format(Some(value))?;
                patch_default_requested = true;
            }
            "--ignore-submodules" => {
                ignore_submodules = IgnoreSubmodulesMode::All;
            }
            value if value.starts_with("--ignore-submodules=") => {
                let Some(value) = value.strip_prefix("--ignore-submodules=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "diff --ignore-submodules expects a value".into(),
                    });
                };
                ignore_submodules = parse_ignore_submodules_mode(Some(value))?;
            }
            "--quiet" => {
                quiet = true;
                patch_default_requested = true;
            }
            "--no-quiet" => {
                quiet = false;
            }
            "--exit-code" => {
                exit_code = true;
                patch_default_requested = true;
            }
            "--no-exit-code" => {
                exit_code = false;
            }
            "--no-ext-diff"
            | "--no-textconv"
            | "--no-color"
            | "--no-color-moved"
            | "--no-color-moved-ws"
            | "--ignore-blank-lines"
            | "--default-prefix" => {}
            "-M" | "--find-renames" => {
                detect_renames = Some(50);
            }
            value if value.starts_with("-M") && value.len() > 2 => {
                detect_renames = Some(parse_similarity_threshold("-M", &value[2..])?);
            }
            value if value.starts_with("--find-renames=") => {
                detect_renames = Some(parse_similarity_threshold(
                    "--find-renames",
                    value
                        .strip_prefix("--find-renames=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --find-renames expects a value".into(),
                        })?,
                )?);
            }
            "-B" | "--break-rewrites" => {
                break_rewrites = Some(60);
                patch_default_requested = true;
            }
            value if value.starts_with("-B") && value.len() > 2 => {
                break_rewrites = parse_break_rewrites_option(Some(&value[2..]))?;
                patch_default_requested = true;
            }
            value if value.starts_with("--break-rewrites=") => {
                break_rewrites = parse_break_rewrites_option(Some(
                    value
                        .strip_prefix("--break-rewrites=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --break-rewrites expects a value".into(),
                        })?,
                ))?;
                patch_default_requested = true;
            }
            "-C" | "--find-copies" => {
                detect_copies = Some(50);
            }
            value if value.starts_with("-C") && value.len() > 2 => {
                detect_copies = Some(parse_similarity_threshold("-C", &value[2..])?);
            }
            value if value.starts_with("--find-copies=") => {
                detect_copies = Some(parse_similarity_threshold(
                    "--find-copies",
                    value
                        .strip_prefix("--find-copies=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --find-copies expects a value".into(),
                        })?,
                )?);
            }
            "--find-copies-harder" => {
                detect_copies = Some(50);
                find_copies_harder = true;
            }
            "--pickaxe-regex" => {
                pickaxe_regex_mode = true;
                patch_default_requested = true;
            }
            "--pickaxe-all" => {
                pickaxe_all = true;
                patch_default_requested = true;
            }
            value if value.starts_with("-S") && value.len() > 2 => {
                pickaxe_string = Some(value[2..].to_owned());
                patch_default_requested = true;
            }
            value if value.starts_with("-G") && value.len() > 2 => {
                pickaxe_regex = Some(value[2..].to_owned());
                patch_default_requested = true;
            }
            value if value.starts_with("-O") && value.len() > 2 => {
                order_file = Some(PathBuf::from(&value[2..]));
            }
            value if value.starts_with("--skip-to=") => {
                let Some(value) = value.strip_prefix("--skip-to=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash show --skip-to expects a value".into(),
                    });
                };
                skip_to = Some(value.to_owned());
            }
            value if value.starts_with("--rotate-to=") => {
                let Some(value) = value.strip_prefix("--rotate-to=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "stash show --rotate-to expects a value".into(),
                    });
                };
                rotate_to = Some(value.to_owned());
            }
            "--no-renames" => {
                detect_renames = None;
                detect_copies = None;
                find_copies_harder = false;
            }
            "--no-prefix" => {
                old_prefix.clear();
                new_prefix.clear();
            }
            "-U" | "--unified" => {
                unified_context = 3;
                unified_context_explicit = true;
            }
            value if value.starts_with("-U") && value.len() > 2 => {
                unified_context = parse_diff_context_value("-U", &value[2..])?;
                unified_context_explicit = true;
            }
            value if value.starts_with("--unified=") => {
                unified_context = parse_diff_context_value(
                    "--unified",
                    value
                        .strip_prefix("--unified=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --unified expects a value".into(),
                        })?,
                )?;
                unified_context_explicit = true;
            }
            value if value.starts_with("--inter-hunk-context=") => {
                inter_hunk_context = parse_diff_context_value(
                    "--inter-hunk-context",
                    value
                        .strip_prefix("--inter-hunk-context=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --inter-hunk-context expects a value".into(),
                        })?,
                )?;
            }
            "--minimal" => {
                minimal = true;
                patch_default_requested = true;
            }
            "--patience" => {
                patience = true;
                patch_default_requested = true;
            }
            "--histogram" => {
                histogram = true;
                patch_default_requested = true;
            }
            value if value.starts_with("--diff-algorithm=") => {
                diff_algorithm = Some(
                    value
                        .strip_prefix("--diff-algorithm=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --diff-algorithm expects a value".into(),
                        })?
                        .to_owned(),
                );
                patch_default_requested = true;
            }
            value if value.starts_with("--anchored=") => {
                anchored.push(
                    value
                        .strip_prefix("--anchored=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --anchored expects a value".into(),
                        })?
                        .to_owned(),
                );
                patch_default_requested = true;
            }
            value if value.starts_with("--diff-filter=") => {
                let Some(value) = value.strip_prefix("--diff-filter=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "diff --diff-filter expects a value".into(),
                    });
                };
                diff_filter = parse_diff_filter(value)?;
                patch_default_requested = true;
            }
            "--ignore-space-at-eol" => {
                whitespace_mode = DiffWhitespaceMode::AtEol;
                patch_default_requested = true;
            }
            "--ignore-cr-at-eol" => {
                whitespace_mode = DiffWhitespaceMode::CrAtEol;
                patch_default_requested = true;
            }
            value if value.starts_with("-I") && value.len() > 2 => {
                ignore_matching_lines.push(value[2..].to_owned());
                patch_default_requested = true;
            }
            value if value.starts_with("--ignore-matching-lines=") => {
                ignore_matching_lines.push(
                    value
                        .strip_prefix("--ignore-matching-lines=")
                        .ok_or_else(|| CliError::Fatal {
                            code: 129,
                            message: "diff --ignore-matching-lines expects a value".into(),
                        })?
                        .to_owned(),
                );
                patch_default_requested = true;
            }
            "-b" | "--ignore-space-change" => {
                whitespace_mode = DiffWhitespaceMode::Change;
                patch_default_requested = true;
            }
            "-w" | "--ignore-all-space" => {
                whitespace_mode = DiffWhitespaceMode::All;
                patch_default_requested = true;
            }
            value if value.starts_with("--src-prefix=") => {
                old_prefix = value
                    .strip_prefix("--src-prefix=")
                    .ok_or_else(|| CliError::Fatal {
                        code: 129,
                        message: "diff --src-prefix expects a value".into(),
                    })?
                    .to_owned();
            }
            value if value.starts_with("--dst-prefix=") => {
                new_prefix = value
                    .strip_prefix("--dst-prefix=")
                    .ok_or_else(|| CliError::Fatal {
                        code: 129,
                        message: "diff --dst-prefix expects a value".into(),
                    })?
                    .to_owned();
            }
            "-s" | "--no-patch" => {
                show_stat = false;
                show_patch = false;
                diff_format_explicit = true;
            }
            "--name-only" => {
                name_only = true;
                name_status = false;
                diff_format_explicit = true;
            }
            "--name-status" => {
                name_status = true;
                name_only = false;
                diff_format_explicit = true;
            }
            "-u" | "--include-untracked" | "--no-include-untracked" => {}
            "--only-untracked" => {
                only_untracked = true;
                show_stat = false;
                show_patch = false;
                diff_format_explicit = true;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!(
                        "error: unknown option `{}`\nusage: git stash show [-u | --include-untracked | --only-untracked] [<diff-options>] [<stash>]\n",
                        value.trim_start_matches('-')
                    ),
                });
            }
            value => {
                if let Some(previous) = stash.replace(value) {
                    return Err(too_many_stash_revisions_error(previous, value));
                }
            }
        }
    }
    if patch_default_requested && !diff_format_explicit {
        show_stat = false;
        show_patch = true;
    }
    validate_diff_algorithm_options(
        minimal,
        patience,
        histogram,
        diff_algorithm.as_deref(),
        &anchored,
    )?;
    let ignore_matching_lines = compile_ignore_matching_lines(&ignore_matching_lines)?;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let id = resolve_stash_id(&repo, stash)?;
    let commit = commit_cache.read_commit(&id)?;
    let Some(parent) = commit.parents.first() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "stash commit has no base parent".into(),
        });
    };
    let parent_commit = commit_cache.read_commit(parent)?;
    let tree_cache = TreeObjectCache::new(&store);
    let old_index = read_commit_tree_index_cached(&tree_cache, &parent_commit)?;
    let new_index = read_commit_tree_index_cached(&tree_cache, &commit)?;
    let entries = diff_entries_for_indexes(
        &old_index,
        &new_index,
        detect_renames,
        detect_copies,
        find_copies_harder,
    )?;
    let diff_context = DiffIndexContext {
        repo: &repo,
        store: &store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let entries = apply_similarity_detection(
        &diff_context,
        entries,
        SimilarityDetectionOptions {
            rename_threshold: detect_renames,
            copy_threshold: detect_copies,
            find_copies_harder,
        },
    )?;
    let entries =
        filter_ignored_submodule_entries(entries, &old_index, &new_index, ignore_submodules);
    let entries = apply_break_rewrites(&diff_context, entries, break_rewrites)?;
    let entries = apply_pickaxe_filter(
        &diff_context,
        entries,
        PickaxeOptions {
            string: pickaxe_string.as_deref(),
            regex: pickaxe_regex.as_deref(),
            regex_mode: pickaxe_regex_mode,
            all: pickaxe_all,
        },
    )?;
    #[cfg(windows)]
    let entries = if pickaxe_string.is_some() || pickaxe_regex.is_some() {
        Vec::new()
    } else {
        entries
    };
    let entries = apply_diff_filter(entries, diff_filter);
    let entries = apply_diff_order_file(entries, order_file.as_deref())?;
    let entries = apply_diff_skip_rotate(entries, skip_to.as_deref(), rotate_to.as_deref());
    if only_untracked {
        return Ok(());
    }
    let has_changes = !entries.is_empty();
    if quiet {
        return if has_changes {
            Err(CliError::Exit(1))
        } else {
            Ok(())
        };
    }
    if name_only {
        print_name_only_entries(&entries, None, nul_terminated)?;
        return if exit_code && has_changes {
            Err(CliError::Exit(1))
        } else {
            Ok(())
        };
    }
    if name_status {
        print_name_status_entries(&entries, None, nul_terminated)?;
        return if exit_code && has_changes {
            Err(CliError::Exit(1))
        } else {
            Ok(())
        };
    }
    let mut printed = false;
    let context = DiffIndexContext {
        repo: &repo,
        store: &store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let stat_options = DiffStatOptions {
        whitespace_mode,
        relative_prefix: None,
        ignore_matching_lines: &ignore_matching_lines,
        ignore_blank_lines: false,
        compact_summary: false,
    };
    if show_raw {
        print_raw_entries(
            &context,
            &entries,
            RawPrintOptions {
                abbrev_len,
                relative_prefix: None,
                nul_terminated,
            },
        )?;
        printed = true;
    }
    if show_numstat {
        print_numstat_entries(
            &context,
            &entries,
            NumstatOptions {
                stat: stat_options,
                nul_terminated,
            },
        )?;
        printed = true;
    }
    if show_shortstat {
        let rows = diff_stat_rows_with_whitespace(&context, &entries, stat_options)?;
        if !rows.is_empty() {
            print_diff_stat_summary(&rows);
        }
        printed = true;
    }
    if show_summary {
        print_summary_entries(&old_index, &new_index, &entries, None)?;
        printed = true;
    }
    if show_stat {
        print_stat_entries_with_whitespace(&context, &entries, stat_options)?;
        printed = true;
    }
    if printed && show_patch {
        println!();
    }
    if show_patch {
        let patch_abbrev_len = if full_index {
            Some(GitHashAlgorithm::Sha1.digest_len() * 2)
        } else {
            abbrev_len
        };
        print_patch_entries(
            &repo,
            &store,
            &old_index,
            &new_index,
            &entries,
            PatchFormatOptions::cached()
                .with_abbrev_len(patch_abbrev_len)
                .with_prefixes(old_prefix, new_prefix)
                .with_context(unified_context, inter_hunk_context)
                .with_whitespace_mode(whitespace_mode)
                .with_ignore_matching_lines(ignore_matching_lines)
                .with_binary(binary)
                .with_irreversible_delete(irreversible_delete)
                .with_submodule_format(submodule_format)
                .with_hunk_headers(!unified_context_explicit),
        )?;
    }
    if exit_code && has_changes {
        Err(CliError::Exit(1))
    } else {
        Ok(())
    }
}

fn stash_apply(
    drop: bool,
    stash: Option<&str>,
    quiet: bool,
    restore_index: bool,
    no_restore_index: bool,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let restore_index = restore_index || (!no_restore_index && stash_index_config_enabled(&repo)?);
    let drop_index = if drop {
        Some(parse_stash_selector(stash.unwrap_or("stash@{0}"))?)
    } else {
        None
    };
    let id = resolve_stash_id(&repo, stash)?;
    if !stash_apply_can_preserve_dirty_paths(&repo, &store, &commit_cache, &tree_cache, &id)? {
        return Err(CliError::Fatal {
            code: 1,
            message: "local changes would be overwritten by stash apply".into(),
        });
    }
    apply_stash_commit(
        &repo,
        &store,
        &commit_cache,
        &tree_cache,
        &id,
        restore_index,
    )?;
    if !quiet {
        status(None, false, false, false, None, None, Vec::new())?;
    }
    if let Some(index) = drop_index {
        drop_stash_entry(&repo, index, quiet)?;
    }
    Ok(())
}

fn stash_apply_can_preserve_dirty_paths(
    repo: &GitRepo,
    _store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
) -> Result<bool> {
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let stash_commit = commit_cache.read_commit(id)?;
    let Some(parent) = stash_commit.parents.first() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "stash commit has no base parent".into(),
        });
    };
    let base_commit = commit_cache.read_commit(parent)?;
    let base_index = read_commit_tree_index_cached(tree_cache, &base_commit)?;
    let patch_index = read_commit_tree_index_cached(tree_cache, &stash_commit)?;
    let changed_paths = diff_indexes(&base_index, &patch_index)?
        .into_iter()
        .map(|entry| entry.path.to_vec())
        .collect::<HashSet<_>>();
    if changed_paths.is_empty() {
        return Ok(true);
    }
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let index = read_repo_index(repo)?;
    for entry in diff_indexes(&head_index, &index)? {
        if changed_paths.contains::<[u8]>(entry.path.as_slice()) {
            if let (Some(current), Some(target)) = (
                find_index_entry(&index, &entry.path),
                find_index_entry(&patch_index, &entry.path),
            ) && current.id == target.id
                && current.mode == target.mode
            {
                continue;
            }
            return Ok(false);
        }
    }
    for (path, _) in worktree_status(repo, &index)? {
        if changed_paths.contains(&path) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn stash_drop(args: &[String]) -> Result<()> {
    let options = parse_stash_reference_options(args, "drop")?;
    let repo = find_repo()?;
    let index = parse_stash_selector(options.stash.as_deref().unwrap_or("stash@{0}"))?;
    drop_stash_entry(&repo, index, options.quiet)?;
    Ok(())
}

struct StashReferenceOptions {
    quiet: bool,
    index: bool,
    no_index: bool,
    stash: Option<String>,
}

fn parse_stash_reference_options(
    args: &[String],
    operation: &str,
) -> Result<StashReferenceOptions> {
    let mut quiet = false;
    let mut index = false;
    let mut no_index = false;
    let mut stash = None;
    for arg in args {
        match arg.as_str() {
            "-q" | "--quiet" => quiet = true,
            "--no-quiet" => quiet = false,
            "--index" if operation == "apply" || operation == "pop" => {
                index = true;
                no_index = false;
            }
            "--no-index" if operation == "apply" || operation == "pop" => {
                index = false;
                no_index = true;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported stash {operation} option '{value}'"),
                });
            }
            value => {
                if let Some(previous) = stash.replace(value.to_owned()) {
                    return Err(too_many_stash_revisions_error(&previous, value));
                }
            }
        }
    }
    Ok(StashReferenceOptions {
        quiet,
        index,
        no_index,
        stash,
    })
}

fn stash_index_config_enabled(repo: &GitRepo) -> Result<bool> {
    Ok(read_config_entry(repo, "stash.index")?
        .and_then(|entry| entry.bool_value())
        .unwrap_or(false))
}

fn stash_branch(args: &[String]) -> Result<()> {
    let Some(branch) = args.first() else {
        return Err(CliError::Stderr {
            code: 1,
            text: "No branch name specified\n".into(),
        });
    };
    if args.len() > 2 {
        return Err(too_many_stash_revisions_error(&args[1], &args[2]));
    }
    let stash = args.get(1).map(String::as_str).unwrap_or("stash@{0}");
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let stack_index = parse_stash_selector(stash).ok();
    let id = resolve_stash_id(&repo, Some(stash))?;
    let commit = commit_cache.read_commit(&id)?;
    let Some(base) = commit.parents.first() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "stash commit has no base parent".into(),
        });
    };
    checkout_new_branch(false, branch, &base.to_hex(), false, false, false)?;
    let repo = find_repo()?;
    let tree_cache = TreeObjectCache::new(&store);
    if !stash_apply_can_preserve_dirty_paths(&repo, &store, &commit_cache, &tree_cache, &id)? {
        return Err(CliError::Fatal {
            code: 1,
            message: "local changes would be overwritten by stash apply".into(),
        });
    }
    apply_stash_commit(&repo, &store, &commit_cache, &tree_cache, &id, true)?;
    if let Some(index) = stack_index {
        drop_stash_entry(&repo, index, false)?;
    }
    Ok(())
}

fn too_many_stash_revisions_error(first: &str, second: &str) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!("Too many revisions specified: '{first}' '{second}'\n"),
    }
}

fn stash_clear() -> Result<()> {
    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    match refs.delete_ref(stash_ref_name()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    };
    match fs::remove_file(stash_reflog_path(&repo)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn apply_stash_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
    restore_index: bool,
) -> Result<()> {
    if let Some(error) = stash_locked_index_error(repo) {
        return Err(error);
    }
    let stash_commit = commit_cache.read_commit(id)?;
    let Some(parent) = stash_commit.parents.first() else {
        return Err(CliError::Fatal {
            code: 128,
            message: "stash commit has no base parent".into(),
        });
    };
    let base_commit = commit_cache.read_commit(parent)?;
    let base_index = read_commit_tree_index_cached(tree_cache, &base_commit)?;
    let patch_index = read_commit_tree_index_cached(tree_cache, &stash_commit)?;
    let head_index = read_head_index_with_caches(repo, commit_cache, tree_cache)?;
    let current_index = read_repo_index(repo)?;
    let untracked_index = stash_untracked_parent_index(commit_cache, tree_cache, &stash_commit)?;
    let stash_changed_paths = diff_indexes(&base_index, &patch_index)?
        .into_iter()
        .map(|entry| entry.path.to_vec())
        .collect::<HashSet<_>>();
    let final_index = if restore_index {
        let Some(index_parent) = stash_commit.parents.get(1) else {
            return Err(CliError::Fatal {
                code: 128,
                message: "stash commit has no index parent".into(),
            });
        };
        let index_commit = commit_cache.read_commit(index_parent)?;
        let index_parent_index = read_commit_tree_index_cached(tree_cache, &index_commit)?;
        let index_changed_paths = diff_indexes(&base_index, &index_parent_index)?
            .into_iter()
            .map(|entry| entry.path.to_vec())
            .collect::<HashSet<_>>();
        let applied_index = match apply_tree_delta(&base_index, &index_parent_index, &head_index) {
            Ok(index) => index,
            Err(error) => {
                restore_stash_untracked_index(repo, store, untracked_index.as_ref())?;
                return Err(error);
            }
        };
        match merge_stash_apply_index(&current_index, &applied_index, &index_changed_paths) {
            Ok(index) => index,
            Err(error) => {
                restore_stash_untracked_index(repo, store, untracked_index.as_ref())?;
                return Err(error);
            }
        }
    } else {
        let mut final_index = current_index.clone();
        if let Some(index_parent) = stash_commit.parents.get(1) {
            let index_commit = commit_cache.read_commit(index_parent)?;
            let index_parent_index = read_commit_tree_index_cached(tree_cache, &index_commit)?;
            for path in &stash_changed_paths {
                if find_index_entry(&base_index, path).is_none()
                    && find_index_entry(&current_index, path).is_none()
                    && let Some(entry) = find_index_entry(&index_parent_index, path)
                {
                    final_index.upsert(entry.clone())?;
                }
            }
        }
        final_index
    };
    let applied_head = match apply_tree_delta(&base_index, &patch_index, &head_index) {
        Ok(index) => index,
        Err(error) => {
            restore_stash_untracked_index(repo, store, untracked_index.as_ref())?;
            return Err(error);
        }
    };
    let checkout_source =
        match merge_stash_apply_index(&current_index, &applied_head, &stash_changed_paths) {
            Ok(index) => index,
            Err(error) => {
                restore_stash_untracked_index(repo, store, untracked_index.as_ref())?;
                return Err(error);
            }
        };
    remove_stash_deleted_paths(repo, &stash_changed_paths, &applied_head)?;
    let checkout_index_entries = GitIndex::from_entries(
        checkout_source
            .entries()
            .iter()
            .filter(|entry| stash_changed_paths.contains(entry.path.as_slice()))
            .cloned()
            .collect(),
    )?;
    checkout_source.write_to_path(&repo.index_path)?;
    checkout_index(
        store,
        &checkout_index_entries,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    restore_stash_untracked_index(repo, store, untracked_index.as_ref())?;
    final_index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn stash_untracked_parent_index(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    stash_commit: &zmin_git_core::CommitObject,
) -> Result<Option<GitIndex>> {
    let Some(untracked_parent) = stash_commit.parents.get(2) else {
        return Ok(None);
    };
    let untracked_commit = commit_cache.read_commit(untracked_parent)?;
    Ok(Some(read_commit_tree_index_cached(
        tree_cache,
        &untracked_commit,
    )?))
}

fn restore_stash_untracked_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    untracked_index: Option<&GitIndex>,
) -> Result<()> {
    let Some(untracked_index) = untracked_index else {
        return Ok(());
    };
    checkout_index(
        store,
        untracked_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    Ok(())
}

fn merge_stash_apply_index(
    current_index: &GitIndex,
    applied_head: &GitIndex,
    stash_changed_paths: &HashSet<Vec<u8>>,
) -> Result<GitIndex> {
    let mut entries = current_index
        .entries()
        .iter()
        .filter(|entry| !stash_changed_paths.contains(entry.path.as_slice()))
        .cloned()
        .collect::<Vec<_>>();
    entries.extend(
        applied_head
            .entries()
            .iter()
            .filter(|entry| stash_changed_paths.contains(entry.path.as_slice()))
            .cloned(),
    );
    Ok(GitIndex::from_entries(entries)?)
}

fn remove_stash_deleted_paths(
    repo: &GitRepo,
    stash_changed_paths: &HashSet<Vec<u8>>,
    applied_head: &GitIndex,
) -> Result<()> {
    let applied_paths = applied_head
        .entries()
        .iter()
        .map(|entry| entry.path.as_slice())
        .collect::<HashSet<_>>();
    for path in stash_changed_paths {
        if !applied_paths.contains(path.as_slice()) {
            remove_worktree_path(repo, path)?;
        }
    }
    Ok(())
}

fn reset_worktree_to_head(repo: &GitRepo, store: &LooseObjectStore) -> Result<()> {
    let old_index = read_repo_index(repo)?;
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    remove_tracked_paths_missing_from_target(repo, &old_index, &head_index)?;
    let mut head_index = head_index;
    checkout_index(
        store,
        &head_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    refresh_tracked_index_metadata_matching(repo, &mut head_index, &[])?;
    head_index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn stash_default_message(
    _repo: &GitRepo,
    refs: &RefStore,
    head_id: &ObjectId,
    head_commit: &zmin_git_core::CommitObject,
) -> String {
    let branch = current_branch_ref(refs)
        .ok()
        .flatten()
        .map(|name| branch_display_name(&name))
        .unwrap_or_else(|| "(no branch)".to_owned());
    format!(
        "WIP on {branch}: {} {}",
        short_object_id(head_id),
        commit_subject(&head_commit.message)
    )
}

fn stash_push_message(
    repo: &GitRepo,
    refs: &RefStore,
    head_id: &ObjectId,
    head_commit: &zmin_git_core::CommitObject,
    message: Option<String>,
) -> String {
    if let Some(message) = message {
        let branch = current_branch_ref(refs)
            .ok()
            .flatten()
            .map(|name| branch_display_name(&name))
            .unwrap_or_else(|| "(no branch)".to_owned());
        return format!("On {branch}: {message}");
    }
    stash_default_message(repo, refs, head_id, head_commit)
}

fn stash_untracked_paths(
    repo: &GitRepo,
    index: &GitIndex,
    include_ignored: bool,
) -> Result<Vec<Vec<u8>>> {
    let tracked_paths = tracked_path_set(index);
    let ignore = GitIgnore::load_from_root(&repo.root)?;
    let mut paths = worktree_commands::untracked_files_with_mode(
        &repo.root,
        &tracked_paths,
        &ignore,
        worktree_commands::UntrackedMode::All,
    )?;
    if include_ignored {
        paths.extend(worktree_commands::ignored_untracked_files(
            &repo.root,
            &tracked_paths,
            &ignore,
        )?);
        paths.sort();
        paths.dedup();
    }
    Ok(paths)
}

fn resolve_stash_id(repo: &GitRepo, stash: Option<&str>) -> Result<ObjectId> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let selector = stash.unwrap_or("stash@{0}");
    if let Ok(index) = parse_stash_selector(selector) {
        return stash_entries(repo, &store)?
            .get(index)
            .map(|entry| entry.id.clone())
            .ok_or_else(|| CliError::Stderr {
                code: 1,
                text: format!("error: {selector} is not a valid reference\n"),
            });
    }
    if let Some(id) = resolve_stash_date_selector(repo, selector)? {
        return Ok(id);
    }
    resolve_commitish(repo, &store, selector).map_err(|_| CliError::Stderr {
        code: 1,
        text: format!("error: {selector} is not a valid reference\n"),
    })
}

fn resolve_stash_date_selector(repo: &GitRepo, selector: &str) -> Result<Option<ObjectId>> {
    let Some(raw) = selector
        .strip_prefix("stash@{")
        .or_else(|| selector.strip_prefix("refs/stash@{"))
    else {
        return Ok(None);
    };
    let Some(date) = raw.strip_suffix('}') else {
        return Ok(None);
    };
    let Some(timestamp) = parse_stash_selector_date(date) else {
        return Ok(None);
    };
    let contents = match fs::read_to_string(stash_reflog_path(repo)) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CliError::Io(error)),
    };
    Ok(contents
        .lines()
        .rev()
        .filter_map(parse_reflog_entry)
        .find(|entry| entry.timestamp <= timestamp)
        .map(|entry| entry.new_id))
}

fn parse_stash_selector_date(date: &str) -> Option<i64> {
    chrono::DateTime::parse_from_str(date, "%a %b %e %H:%M:%S %Y %z")
        .or_else(|_| chrono::DateTime::parse_from_str(date, "%a %b %d %H:%M:%S %Y %z"))
        .map(|datetime| datetime.timestamp())
        .ok()
}

#[derive(Debug, Clone)]
struct StashEntry {
    id: ObjectId,
    message: String,
    reflog_identity: String,
}

fn parse_stash_selector(selector: &str) -> Result<usize> {
    match selector {
        "stash" | "refs/stash" => return Ok(0),
        _ => {}
    }
    if selector.bytes().all(|byte| byte.is_ascii_digit()) {
        return selector.parse::<usize>().map_err(|_| CliError::Stderr {
            code: 1,
            text: format!("error: {selector} is not a valid reference\n"),
        });
    }
    let Some(raw) = selector
        .strip_prefix("stash@{")
        .or_else(|| selector.strip_prefix("refs/stash@{"))
    else {
        return Err(CliError::Stderr {
            code: 1,
            text: format!("error: {selector} is not a valid reference\n"),
        });
    };
    let Some(index) = raw.strip_suffix('}') else {
        return Err(CliError::Stderr {
            code: 1,
            text: format!("error: {selector} is not a valid reference\n"),
        });
    };
    index.parse::<usize>().map_err(|_| CliError::Stderr {
        code: 1,
        text: format!("error: {selector} is not a valid reference\n"),
    })
}

fn stash_entries(repo: &GitRepo, store: &LooseObjectStore) -> Result<Vec<StashEntry>> {
    let path = stash_reflog_path(repo);
    match fs::read_to_string(path) {
        Ok(content) => {
            let mut entries = content
                .lines()
                .filter_map(parse_reflog_entry)
                .map(|entry| StashEntry {
                    id: entry.new_id,
                    message: entry.message,
                    reflog_identity: entry.identity,
                })
                .collect::<Vec<_>>();
            entries.reverse();
            Ok(entries)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
            match refs.resolve(stash_ref_name()) {
                Ok(id) => {
                    let commit_cache = CommitObjectCache::new(store);
                    let commit = commit_cache.read_commit(&id)?;
                    Ok(vec![StashEntry {
                        id,
                        message: commit_subject(&commit.message),
                        reflog_identity: String::from_utf8_lossy(&commit.committer).into_owned(),
                    }])
                }
                Err(_) => Ok(Vec::new()),
            }
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn write_stash_ref_update(
    repo: &GitRepo,
    refs: &RefStore,
    new_id: &ObjectId,
    committer: &Signature,
    message: &str,
) -> Result<()> {
    let old_id = refs
        .resolve(stash_ref_name())
        .unwrap_or_else(|_| zero_object_id());
    refs.write_ref(stash_ref_name(), new_id)?;
    append_stash_reflog(repo, &old_id, new_id, committer, message)
}

fn append_stash_reflog(
    repo: &GitRepo,
    old_id: &ObjectId,
    new_id: &ObjectId,
    committer: &Signature,
    message: &str,
) -> Result<()> {
    let path = stash_reflog_path(repo);
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

fn drop_stash_entry(repo: &GitRepo, index: usize, quiet: bool) -> Result<()> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut entries = stash_entries(repo, &store)?;
    if index >= entries.len() {
        return Err(CliError::Stderr {
            code: 1,
            text: format!("error: stash@{{{index}}} is not a valid reference\n"),
        });
    }
    entries.remove(index);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if let Some(top) = entries.first() {
        refs.write_ref(stash_ref_name(), &top.id)?;
        rewrite_stash_reflog(repo, &entries)?;
    } else {
        match refs.delete_ref(stash_ref_name()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        }
        match fs::remove_file(stash_reflog_path(repo)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    if !quiet {
        println!("Dropped stash@{{{index}}}");
    }
    Ok(())
}

fn rewrite_stash_reflog(repo: &GitRepo, entries: &[StashEntry]) -> Result<()> {
    let path = stash_reflog_path(repo);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut content = String::new();
    let signature = signature_from_identity(repo, "GIT_COMMITTER")?;
    let mut old_id = zero_object_id();
    for entry in entries.iter().rev() {
        content.push_str(&format!(
            "{} {} {} <{}> {} {}\t{}\n",
            old_id.to_hex(),
            entry.id.to_hex(),
            signature.name,
            signature.email,
            signature.timestamp,
            signature.timezone,
            entry.message
        ));
        old_id = entry.id.clone();
    }
    fs::write(path, content)?;
    Ok(())
}

fn stash_reflog_path(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("logs").join(stash_ref_name())
}

fn stash_ref_name() -> &'static str {
    "refs/stash"
}

pub(crate) fn checkout(
    force: bool,
    _quiet: bool,
    _no_progress: bool,
    detach: bool,
    _recurse_submodules: bool,
    _no_recurse_submodules: bool,
    create: Option<String>,
    reset_create: Option<String>,
    create_reflog: bool,
    orphan: Option<String>,
    args: Vec<String>,
) -> Result<()> {
    let branch_modes = [create.is_some(), reset_create.is_some(), orphan.is_some()]
        .into_iter()
        .filter(|mode| *mode)
        .count();
    if branch_modes > 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-b', '-B', and '--orphan' cannot be used together".into(),
        });
    }
    if detach && branch_modes > 0 {
        return Err(CliError::Fatal {
            code: 128,
            message: "'--detach' cannot be used with '-b/-B/--orphan'".into(),
        });
    }
    if let Some(branch) = create {
        if args.len() > 1 {
            return Err(CliError::Fatal {
                code: 129,
                message: "`checkout -b` accepts at most one start point".into(),
            });
        }
        let explicit_start = args.first().map(String::as_str);
        let start = explicit_start.unwrap_or("HEAD");
        return checkout_new_branch(
            force,
            &branch,
            start,
            explicit_start.is_none(),
            false,
            create_reflog,
        );
    }
    if let Some(branch) = reset_create {
        if args.len() > 1 {
            return Err(CliError::Fatal {
                code: 129,
                message: "`checkout -B` accepts at most one start point".into(),
            });
        }
        let explicit_start = args.first().map(String::as_str);
        let start = explicit_start.unwrap_or("HEAD");
        return checkout_new_branch(
            force,
            &branch,
            start,
            explicit_start.is_none(),
            true,
            create_reflog,
        );
    }
    if let Some(branch) = orphan {
        return orphan_checkout(force, &branch);
    }
    let explicit_pathspec_separator = checkout_raw_args_have_separator();
    if detach && args.len() > 1 {
        resolve_checkout_detach_target(&args[0])?;
        let path_arg = if args.get(1).is_some_and(|arg| arg == "--") {
            args.get(2).unwrap_or(&args[1])
        } else {
            &args[1]
        };
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "git checkout: --detach does not take a path argument '{}'",
                path_arg
            ),
        });
    }
    if let Some((source, paths, report_updated_paths)) = checkout_path_mode(&args)? {
        return checkout_paths(
            source,
            paths,
            report_updated_paths && !explicit_pathspec_separator,
        );
    }
    let Some(target) = args.first() else {
        if detach {
            return checkout_detached(force, "HEAD", "checkout");
        }
        return Err(CliError::Fatal {
            code: 129,
            message: "`checkout` requires a branch, commit, or -b <branch>".into(),
        });
    };
    if detach {
        return checkout_detached(force, target, "checkout");
    }
    if target == "HEAD" || target == "@" {
        return checkout_current_head(force);
    }
    if target == "-" {
        let previous = previous_checkout_target()?;
        return checkout_existing(force, &previous);
    }
    if target.starts_with("refs/heads/") {
        return checkout_detached(force, target, "checkout");
    }
    if !checkout_target_exists(target)? {
        return checkout_paths(
            None,
            vec![PathBuf::from(target)],
            !explicit_pathspec_separator,
        );
    }
    checkout_existing(force, target)
}

fn checkout_raw_args_have_separator() -> bool {
    std::env::args_os().any(|arg| arg == "--")
}

fn checkout_current_head(force: bool) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target_id = refs.resolve("HEAD")?;
    if force {
        let checkout_metadata = WorktreeCheckoutMetadata {
            ref_name: current_branch_ref(&refs)?,
            treeish: Some(target_id.clone()),
        };
        checkout_worktree_with_metadata(&repo, &store, &target_id, &checkout_metadata)?;
    } else {
        refs.read_head()?;
    }
    Ok(())
}

fn previous_checkout_target() -> Result<String> {
    let repo = find_repo()?;
    let contents = fs::read_to_string(repo.git_dir.join("logs").join("HEAD"))?;
    contents
        .lines()
        .rev()
        .filter_map(|line| line.split_once('\t').map(|(_, message)| message))
        .find_map(|message| {
            message
                .strip_prefix("checkout: moving from ")
                .and_then(|rest| rest.split_once(" to "))
                .map(|(previous, _)| previous.to_owned())
        })
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "could not resolve previous checkout".into(),
        })
}

fn checkout_target_exists(target: &str) -> Result<bool> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(common_git_dir, GitHashAlgorithm::Sha1);
    if branch_checkout_ref(&refs, target)?.is_some() {
        return Ok(true);
    }
    Ok(resolve_commitish(&repo, &store, target).is_ok())
}

fn resolve_checkout_detach_target(target: &str) -> Result<ObjectId> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    resolve_commitish(&repo, &store, target).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid reference: {target}"),
    })
}

fn checkout_path_mode(args: &[String]) -> Result<Option<(Option<&str>, Vec<PathBuf>, bool)>> {
    match args {
        [] => Ok(None),
        [separator, paths @ ..] if separator == "--" => Ok(Some((
            None,
            paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
            false,
        ))),
        [source, separator, paths @ ..] if separator == "--" => Ok(Some((
            Some(source.as_str()),
            paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
            false,
        ))),
        [source, paths @ ..] if !paths.is_empty() => {
            let repo = find_repo()?;
            let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
            if resolve_commitish(&repo, &store, source).is_ok() {
                Ok(Some((
                    Some(source.as_str()),
                    paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
                    false,
                )))
            } else {
                Ok(Some((
                    None,
                    args.iter().map(PathBuf::from).collect::<Vec<_>>(),
                    true,
                )))
            }
        }
        _ => Ok(None),
    }
}

fn checkout_paths(
    source: Option<&str>,
    paths: Vec<PathBuf>,
    report_updated_paths: bool,
) -> Result<()> {
    let report_from_index = source.is_none();
    let updated_paths = if source.is_some() {
        worktree_commands::restore(source, true, true, paths)?
    } else {
        worktree_commands::restore(None, false, true, paths)?
    };
    if report_updated_paths && report_from_index && updated_paths > 0 {
        let noun = if updated_paths == 1 { "path" } else { "paths" };
        eprintln!("Updated {updated_paths} {noun} from the index");
    }
    Ok(())
}

fn checkout_new_branch(
    force: bool,
    branch: &str,
    start: &str,
    default_start: bool,
    reset_existing: bool,
    create_reflog: bool,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);

    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let ref_name = branch_ref_name(branch)?;
    if !reset_existing && ref_exists(&refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("a branch named '{branch}' already exists"),
        });
    }
    let id = match resolve_commitish(&repo, &store, start) {
        Ok(id) => id,
        Err(_) if default_start && start == "HEAD" => {
            refs.write_symbolic_ref("HEAD", &ref_name)?;
            eprintln!("Switched to a new branch '{branch}'");
            return Ok(());
        }
        Err(_) => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "'{start}' is not a commit and a branch '{branch}' cannot be created from it"
                ),
            });
        }
    };
    let branch_reflog_message = if reset_existing {
        format!("branch: Reset to {start}")
    } else {
        format!("branch: Created from {start}")
    };
    if create_reflog || checkout_branch_reflog_enabled(&repo)? {
        write_ref_with_reflog(&repo, &refs, &ref_name, &id, &branch_reflog_message)?;
    } else {
        refs.write_ref(&ref_name, &id)?;
    }
    checkout_existing(force, branch)
}

fn checkout_branch_reflog_enabled(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "core.logAllRefUpdates")? else {
        return Ok(true);
    };
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("bad boolean config value '{}'", entry.value),
    })
}

fn orphan_checkout(force: bool, branch: &str) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let ref_name = branch_ref_name(branch)?;
    if ref_exists(&refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("a branch named '{branch}' already exists"),
        });
    }
    if !force {
        reject_orphan_checkout_dirty_worktree(&repo, &store)?;
    }
    let old_index = read_repo_index(&repo)?;
    let empty_index = GitIndex::new();
    remove_tracked_paths_missing_from_target(&repo, &old_index, &empty_index)?;
    empty_index.write_to_path(&repo.index_path)?;
    refs.write_head_symbolic(&ref_name)?;
    eprintln!("Switched to a new branch '{branch}'");
    Ok(())
}

fn reject_orphan_checkout_dirty_worktree(repo: &GitRepo, _store: &LooseObjectStore) -> Result<()> {
    let index = read_repo_index(repo)?;
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let mut paths = diff_indexes(&head_index, &index)?
        .into_iter()
        .map(|entry| entry.path.to_vec())
        .collect::<BTreeSet<_>>();
    for (path, _) in worktree_status(repo, &index)? {
        paths.insert(path.into());
    }
    if paths.is_empty() {
        return Ok(());
    }
    let mut text = String::from(
        "error: Your local changes to the following files would be overwritten by checkout:\n",
    );
    for path in paths {
        text.push('\t');
        text.push_str(&String::from_utf8_lossy(&path));
        text.push('\n');
    }
    text.push_str(
        "Please commit your changes or stash them before you switch branches.\nAborting\n",
    );
    Err(CliError::Stderr { code: 1, text })
}

pub(crate) fn checkout_existing(force: bool, target: &str) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);

    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(common_git_dir, GitHashAlgorithm::Sha1);
    let head_refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target_branch_ref = branch_checkout_ref(&refs, target)?;
    let target_id = if let Some(ref_name) = target_branch_ref.as_deref() {
        refs.resolve(ref_name)?
    } else {
        resolve_commitish(&repo, &store, target)?
    };
    let current_id = head_refs.resolve("HEAD").ok();
    if current_id.as_ref() != Some(&target_id) {
        let checkout_metadata = WorktreeCheckoutMetadata {
            ref_name: target_branch_ref.clone(),
            treeish: Some(target_id.clone()),
        };
        if force {
            checkout_worktree_with_metadata(&repo, &store, &target_id, &checkout_metadata)?;
        } else {
            checkout_clean_worktree_transition_with_metadata(
                &repo,
                &store,
                &target_id,
                &checkout_metadata,
            )?;
        }
    }

    let source = current_head_reflog_name(&head_refs)?;
    if let Some(ref_name) = target_branch_ref {
        let reflog_message = format!("checkout: moving from {source} to {target}");
        if !print_detached_orphan_warning(&store, &head_refs, &[])? {
            print_previous_detached_head_position(&repo, &store, &head_refs)?;
        }
        write_head_symbolic_with_reflog(&repo, &head_refs, &ref_name, &reflog_message)?;
        eprintln!("Switched to branch '{target}'");
        if let Some(lines) = human_status_upstream(&repo, &head_refs)? {
            for line in lines {
                println!("{line}");
            }
        }
    } else {
        let reflog_message = format!(
            "checkout: moving from {source} to {}",
            short_object_id(&target_id)
        );
        print_detached_checkout_notice(
            &repo,
            &store,
            &head_refs,
            &target_id,
            target,
            DetachedCheckoutMode::Implicit,
        )?;
        write_head_direct_with_reflog(&repo, &head_refs, &target_id, &reflog_message)?;
    }
    Ok(())
}

fn checkout_detached(force: bool, target: &str, operation: &str) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let _ = operation;

    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target_id = resolve_commitish(&repo, &store, target).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid reference: {target}"),
    })?;
    if force {
        checkout_worktree(&repo, &store, &target_id)?;
    } else {
        checkout_clean_worktree_transition(&repo, &store, &target_id)?;
    }
    let source = current_head_reflog_name(&refs)?;
    let mode = if operation == "checkout" {
        DetachedCheckoutMode::Explicit
    } else {
        DetachedCheckoutMode::Implicit
    };
    print_detached_checkout_notice(&repo, &store, &refs, &target_id, target, mode)?;
    let reflog_message = format!(
        "checkout: moving from {source} to {}",
        short_object_id(&target_id)
    );
    write_head_direct_with_reflog(&repo, &refs, &target_id, &reflog_message)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetachedCheckoutMode {
    Explicit,
    Implicit,
}

fn print_detached_checkout_notice(
    repo: &GitRepo,
    store: &LooseObjectStore,
    refs: &RefStore,
    target_id: &ObjectId,
    target_display: &str,
    mode: DetachedCheckoutMode,
) -> Result<()> {
    let previous_head = refs.read_head()?;
    if matches!(previous_head, RefTarget::Direct(_)) {
        if !print_detached_orphan_warning(store, refs, std::slice::from_ref(target_id))? {
            print_previous_detached_head_position(repo, store, refs)?;
        }
    } else if mode == DetachedCheckoutMode::Implicit && detached_head_advice_enabled(repo)? {
        eprintln!("Note: switching to '{target_display}'.");
        eprintln!();
        eprintln!("You are in 'detached HEAD' state. You can look around, make experimental");
        eprintln!("changes and commit them, and you can discard any commits you make in this");
        eprintln!("state without impacting any branches by switching back to a branch.");
        eprintln!();
        eprintln!("If you want to create a new branch to retain commits you create, you may");
        eprintln!("do so (now or later) by using -c with the switch command. Example:");
        eprintln!();
        eprintln!("  git switch -c <new-branch-name>");
        eprintln!();
        eprintln!("Or undo this operation with:");
        eprintln!();
        eprintln!("  git switch -");
        eprintln!();
        eprintln!("Turn off this advice by setting config variable advice.detachedHead to false");
        eprintln!();
    }
    eprintln!(
        "HEAD is now at {} {}",
        detached_checkout_short_id(repo, target_id)?,
        commit_summary_for_checkout(store, target_id)?
    );
    Ok(())
}

fn print_previous_detached_head_position(
    repo: &GitRepo,
    store: &LooseObjectStore,
    refs: &RefStore,
) -> Result<()> {
    let RefTarget::Direct(id) = refs.read_head()? else {
        return Ok(());
    };
    eprintln!(
        "Previous HEAD position was {} {}",
        detached_checkout_short_id(repo, &id)?,
        commit_summary_for_checkout(store, &id)?
    );
    Ok(())
}

fn print_detached_orphan_warning(
    store: &LooseObjectStore,
    refs: &RefStore,
    protected_heads: &[ObjectId],
) -> Result<bool> {
    let RefTarget::Direct(head_id) = refs.read_head()? else {
        return Ok(false);
    };
    let commit_cache = CommitObjectCache::new(store);
    let mut branch_heads = branch_head_ids(refs)?;
    branch_heads.extend(protected_heads.iter().cloned());
    let mut orphaned = Vec::new();
    let mut current = head_id;
    loop {
        if branch_heads.iter().any(|branch| {
            is_ancestor_commit_cached(&commit_cache, &current, branch).unwrap_or(false)
        }) {
            break;
        }
        orphaned.push(current.clone());
        let commit = commit_cache.read_commit(&current)?;
        let Some(parent) = commit.parents.first() else {
            break;
        };
        current = parent.clone();
    }
    if orphaned.is_empty() {
        return Ok(false);
    }
    let count = orphaned.len();
    eprintln!(
        "Warning: you are leaving {count} {} behind, not connected to",
        plural(count, "commit", "commits")
    );
    eprintln!("any of your branches:");
    eprintln!();
    for id in &orphaned {
        eprintln!(
            "  {} {}",
            short_object_id(id),
            commit_summary_for_checkout(store, id)?
        );
    }
    eprintln!();
    eprintln!("If you want to keep them by creating a new branch, this may be a good time");
    eprintln!("to do so with:");
    eprintln!();
    eprintln!(
        " git branch <new-branch-name> {}",
        short_object_id(&orphaned[0])
    );
    eprintln!();
    Ok(true)
}

fn detached_checkout_short_id(repo: &GitRepo, id: &ObjectId) -> Result<String> {
    let len = checkout_abbrev_len(repo)?;
    let mut short = short_object_id_len(id, len);
    if print_sha1_ellipsis_enabled() {
        short.push_str("...");
    }
    Ok(short)
}

fn checkout_abbrev_len(repo: &GitRepo) -> Result<usize> {
    let Some(value) = read_config_value(repo, "core.abbrev")? else {
        return Ok(7);
    };
    value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("bad numeric config value '{}' for 'core.abbrev'", value),
    })
}

fn print_sha1_ellipsis_enabled() -> bool {
    std::env::var("GIT_PRINT_SHA1_ELLIPSIS")
        .map(|value| value.eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
}

fn detached_head_advice_enabled(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "advice.detachedHead")? else {
        return Ok(true);
    };
    Ok(entry.bool_value().unwrap_or(true))
}

fn commit_summary_for_checkout(store: &LooseObjectStore, id: &ObjectId) -> Result<String> {
    let commit_cache = CommitObjectCache::new(store);
    let commit = commit_cache.read_commit(id)?;
    Ok(commit_subject(&commit.message))
}

fn current_head_reflog_name(refs: &RefStore) -> Result<String> {
    match refs.read_head()? {
        RefTarget::Symbolic(target) => Ok(branch_display_name(&target)),
        RefTarget::Direct(id) => Ok(short_object_id(&id)),
    }
}

pub(crate) fn switch(
    force: bool,
    discard_changes: bool,
    create: Option<String>,
    orphan: Option<String>,
    detach: bool,
    target: Option<String>,
) -> Result<()> {
    if (create.is_some() || orphan.is_some()) && detach {
        return Err(CliError::Fatal {
            code: 128,
            message: "'--detach' cannot be used with '-b/-B/--orphan'".into(),
        });
    }
    if create.is_some() && orphan.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "'--orphan' cannot be used with '-c'".into(),
        });
    }
    let force = force || discard_changes;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);

    if let Some(branch) = orphan {
        return orphan_checkout(force, &branch);
    }

    if let Some(branch) = create {
        if !force && !worktree_clean(&repo, &store)? {
            return Err(CliError::Fatal {
                code: 1,
                message: "local changes would be overwritten by switch".into(),
            });
        }
        let ref_name = branch_ref_name(&branch)?;
        if ref_exists(&refs, &ref_name)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("a branch named '{branch}' already exists"),
            });
        }
        let start = target.as_deref().unwrap_or("HEAD");
        let id = resolve_commitish(&repo, &store, start).map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("invalid reference: {start}"),
        })?;
        write_ref_with_reflog(&repo, &refs, &ref_name, &id, "branch: Created from HEAD")?;
        return checkout_existing(force, &branch);
    }

    let Some(target) = target else {
        return Err(CliError::Fatal {
            code: 129,
            message: "`switch` requires a branch, -c <branch>, or --detach <commit>".into(),
        });
    };
    if detach {
        return checkout_detached(force, &target, "switch");
    }
    if !ref_exists(&refs, &branch_ref_name(&target)?)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid reference: {target}"),
        });
    }
    checkout_existing(force, &target)
}
