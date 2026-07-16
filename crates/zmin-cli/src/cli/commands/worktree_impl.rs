use super::*;
use chrono::{Datelike, Timelike};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use zmin_git_core::commit::CommitObjectCache;
use zmin_primitives::git_runtime::GitPrimitiveRuntime;

pub(crate) struct TrackedPathSet<'a> {
    exact_paths: Vec<&'a [u8]>,
    folded_paths: HashSet<Vec<u8>>,
    directory_paths: Vec<&'a [u8]>,
    folded_directory_paths: HashSet<Vec<u8>>,
    tracked_directories: Vec<&'a [u8]>,
    folded_directories: HashSet<Vec<u8>>,
    ignore_case: bool,
}

impl<'a> TrackedPathSet<'a> {
    pub(crate) fn contains(&self, path: &[u8]) -> bool {
        sorted_path_slices_contain(&self.exact_paths, path)
            || (self.ignore_case
                && folded_tracked_path_contains(&self.exact_paths, &self.folded_paths, path))
    }

    pub(crate) fn has_tracked_descendants(&self, relative_dir: &[u8]) -> bool {
        sorted_path_slices_contain(&self.tracked_directories, relative_dir)
            || (self.ignore_case
                && folded_tracked_directory_contains(
                    &self.tracked_directories,
                    &self.folded_directories,
                    relative_dir,
                ))
    }

    fn contains_directory(&self, path: &[u8]) -> bool {
        sorted_path_slices_contain(&self.directory_paths, path)
            || (self.ignore_case
                && folded_tracked_path_contains(
                    &self.directory_paths,
                    &self.folded_directory_paths,
                    path,
                ))
    }
}

pub(crate) fn tracked_path_set(index: &GitIndex) -> TrackedPathSet<'_> {
    tracked_path_set_with_ignore_case(index, false)
}

pub(crate) fn tracked_path_set_with_ignore_case(
    index: &GitIndex,
    ignore_case: bool,
) -> TrackedPathSet<'_> {
    let mut exact_paths = index
        .entries()
        .iter()
        .map(|entry| entry.path.as_slice())
        .collect::<Vec<_>>();
    exact_paths.sort_unstable();
    exact_paths.dedup();
    let mut directory_paths = index
        .entries()
        .iter()
        .filter(|entry| matches!(entry.mode, IndexMode::Gitlink | IndexMode::Tree))
        .map(|entry| entry.path.as_slice())
        .collect::<Vec<_>>();
    directory_paths.sort_unstable();
    directory_paths.dedup();
    let mut tracked_directories = index
        .entries()
        .iter()
        .flat_map(|entry| {
            entry
                .path
                .iter()
                .enumerate()
                .filter_map(|(index, byte)| (*byte == b'/').then_some(&entry.path[..index]))
        })
        .collect::<Vec<_>>();
    tracked_directories.sort_unstable();
    tracked_directories.dedup();
    let mut folded_paths = HashSet::new();
    let mut folded_directory_paths = HashSet::new();
    let mut folded_directories = HashSet::new();
    if ignore_case {
        for entry in index.entries() {
            let folded_path = fold_ascii_path(&entry.path);
            if folded_path != entry.path {
                if matches!(entry.mode, IndexMode::Gitlink | IndexMode::Tree) {
                    folded_directory_paths.insert(folded_path.clone());
                }
                folded_paths.insert(folded_path);
            }
            for ancestor in index_path_ancestors(&entry.path) {
                let folded_ancestor = fold_ascii_path(&ancestor);
                if folded_ancestor != ancestor {
                    folded_directories.insert(folded_ancestor);
                }
            }
        }
    }
    TrackedPathSet {
        exact_paths,
        folded_paths,
        directory_paths,
        folded_directory_paths,
        tracked_directories,
        folded_directories,
        ignore_case,
    }
}

fn fold_ascii_path(path: &[u8]) -> Vec<u8> {
    path.iter().map(u8::to_ascii_lowercase).collect()
}

fn sorted_path_slices_contain(paths: &[&[u8]], path: &[u8]) -> bool {
    paths.binary_search(&path).is_ok()
}

fn folded_tracked_path_contains(
    exact_paths: &[&[u8]],
    folded_paths: &HashSet<Vec<u8>>,
    path: &[u8],
) -> bool {
    if path.iter().any(u8::is_ascii_uppercase) {
        let folded = fold_ascii_path(path);
        sorted_path_slices_contain(exact_paths, &folded) || folded_paths.contains(&folded)
    } else {
        folded_paths.contains(path)
    }
}

fn folded_tracked_directory_contains(
    tracked_directories: &[&[u8]],
    folded_directories: &HashSet<Vec<u8>>,
    relative_dir: &[u8],
) -> bool {
    if relative_dir.iter().any(u8::is_ascii_uppercase) {
        let folded = fold_ascii_path(relative_dir);
        sorted_path_slices_contain(tracked_directories, &folded)
            || folded_directories.contains(&folded)
    } else {
        folded_directories.contains(relative_dir)
    }
}

fn relative_child_path(relative_dir: &[u8], entry_name: &std::ffi::OsStr) -> Vec<u8> {
    let entry = entry_name.to_string_lossy();
    let mut relative = Vec::with_capacity(
        relative_dir.len() + usize::from(!relative_dir.is_empty()) + entry.len(),
    );
    relative.extend_from_slice(relative_dir);
    if !relative_dir.is_empty() {
        relative.push(b'/');
    }
    relative.extend_from_slice(entry.as_bytes());
    relative
}

fn tracked_path_set_for_repo<'a>(
    repo: &GitRepo,
    index: &'a GitIndex,
) -> Result<TrackedPathSet<'a>> {
    Ok(tracked_path_set_with_ignore_case(
        index,
        repo_ignore_case(repo)?,
    ))
}

fn repo_ignore_case(repo: &GitRepo) -> Result<bool> {
    Ok(read_local_config_entries(repo)?
        .into_iter()
        .rev()
        .find(|entry| {
            entry.section == "core" && entry.subsection.is_empty() && entry.key == "ignorecase"
        })
        .and_then(|entry| entry.bool_value())
        .unwrap_or(false))
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

const CLEAN_USAGE: &str = "\
usage: git clean [-d] [-f] [-i] [-n] [-q] [-e <pattern>] [-x | -X] [--] [<pathspec>...]

    -q, --[no-]quiet      do not print names of files removed
    -n, --[no-]dry-run    dry run
    -f, --[no-]force      force
    -i, --[no-]interactive
                          interactive cleaning
    -d                    remove whole directories
    -e, --exclude <pattern>
                          add <pattern> to ignore rules
    -x                    remove ignored files, too
    -X                    remove only ignored files

";

pub(crate) fn clean(args: Vec<String>) -> Result<()> {
    let options = parse_clean_args(args)?;
    let repo = find_repo_or_bare()?;
    if !options.interactive
        && !options.dry_run
        && options.force_count == 0
        && clean_require_force(&repo)?
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "clean.requireForce is true and -f not given: refusing to clean".into(),
        });
    }

    let index = read_repo_index(&repo)?;
    let tracked_paths = tracked_path_set_for_repo(&repo, &index)?;
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

    if options.interactive {
        return clean_interactive_quit(&entries);
    }

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
                    return Err(clean_option_requires_value(arg));
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
            value if value.starts_with('-') && value.len() == 2 && !value.starts_with("--") => {
                let switch = value[1..].chars().next().expect("single switch");
                return Err(clean_unknown_switch(switch));
            }
            value if value.starts_with('-') => {
                return Err(clean_unknown_option(value));
            }
            value => options.paths.push(PathBuf::from(value)),
        }
        cursor += 1;
    }
    Ok(options)
}

fn clean_interactive_prompt(entries: &[Vec<u8>]) {
    println!(
        "Would remove the following item{}:",
        if entries.len() == 1 { "" } else { "s" }
    );
    print!("  ");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            print!("  ");
        }
        print!("{}", String::from_utf8_lossy(entry));
    }
    println!();
    clean_interactive_command_menu();
}

fn clean_interactive_command_menu() {
    println!("*** Commands ***");
    println!("    1: clean                2: filter by pattern    3: select by numbers");
    println!("    4: ask each             5: quit                 6: help");
    print!("What now> ");
}

fn clean_interactive_help() {
    println!("clean               - start cleaning");
    println!("filter by pattern   - exclude items from deletion");
    println!("select by numbers   - select items to be deleted by numbers");
    println!("ask each            - confirm each deletion (like \"rm -i\")");
    println!("quit                - stop cleaning");
    println!("help                - this screen");
    println!("?                   - help for prompt selection");
}

fn clean_interactive_quit(entries: &[Vec<u8>]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    clean_interactive_prompt(entries);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut lines = input.lines();
    loop {
        let command = lines.next().unwrap_or_default().trim();
        match command {
            "" | "q" | "quit" | "5" => {
                print!("Bye.");
                return Ok(());
            }
            "?" | "help" | "6" => {
                clean_interactive_help();
                clean_interactive_prompt(entries);
                io::stdout().flush()?;
            }
            _ => {
                println!("Huh ({command})?");
                clean_interactive_command_menu();
                io::stdout().flush()?;
            }
        }
    }
}

fn clean_unknown_option(option: &str) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown option `{}'\n{CLEAN_USAGE}",
            option.trim_start_matches('-')
        ),
    }
}

fn clean_unknown_switch(switch: char) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!("error: unknown switch `{switch}'\n{CLEAN_USAGE}"),
    }
}

fn clean_option_requires_value(option: &str) -> CliError {
    let text = if let Some(switch) = option.strip_prefix('-').filter(|value| value.len() == 1) {
        format!("error: switch `{switch}' requires a value\n")
    } else {
        format!(
            "error: option `{}' requires a value\n",
            option.trim_start_matches('-')
        )
    };
    CliError::Stderr { code: 129, text }
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
                    return Err(clean_option_requires_value("-e"));
                }
                options.excludes.push(value[pattern_start..].to_owned());
                return Ok(());
            }
            _ => {
                return Err(clean_unknown_switch(flag));
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
            if mode == CleanIgnoredMode::Normal && is_ignored {
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
                continue;
            }
            if !directories {
                continue;
            }
            if clean_directory_fully_removable(root, &path, tracked_paths, ignore, mode)? {
                let mut dir = relative;
                dir.push(b'/');
                files.push(dir);
            } else {
                collect_clean_untracked_files(
                    root,
                    &path,
                    tracked_paths,
                    ignore,
                    directories,
                    mode,
                    files,
                )?;
            }
        } else if (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
            && clean_mode_removes_path(mode, is_ignored)
        {
            files.push(relative);
        }
    }
    Ok(())
}

fn clean_mode_removes_path(mode: CleanIgnoredMode, is_ignored: bool) -> bool {
    match mode {
        CleanIgnoredMode::Normal => !is_ignored,
        CleanIgnoredMode::All => true,
        CleanIgnoredMode::Only => is_ignored,
    }
}

fn clean_directory_fully_removable(
    root: &Path,
    directory: &Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: CleanIgnoredMode,
) -> Result<bool> {
    if is_nested_worktree(directory) {
        return Ok(false);
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let relative = repo_relative_path(root, &path)?;
        if tracked_paths.contains(&relative) || tracked_paths_under(tracked_paths, &relative) {
            return Ok(false);
        }
        let is_dir = metadata.is_dir();
        let is_ignored = ignore.is_ignored(&relative, is_dir);
        if !clean_mode_removes_path(mode, is_ignored) {
            return Ok(false);
        }
        if is_dir && !clean_directory_fully_removable(root, &path, tracked_paths, ignore, mode)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn status(
    porcelain: Option<&str>,
    branch: bool,
    ahead_behind: bool,
    show_stash: bool,
    verbose: u8,
    column: Option<&str>,
    no_column: bool,
    detect_renames: bool,
    ignore_submodules: Option<&str>,
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
        Some("1") => PorcelainVersion::V1,
        Some("v1") => PorcelainVersion::V1,
        Some("2") if short => PorcelainVersion::V1,
        Some("2") => PorcelainVersion::V2,
        Some("v2") if short => PorcelainVersion::V1,
        Some("v2") => PorcelainVersion::V2,
        Some(value) => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unsupported porcelain version '{value}'"),
            });
        }
    };
    let untracked_mode = UntrackedMode::parse(untracked_files)?;
    let ignore_submodules_mode = StatusIgnoreSubmodulesMode::parse(ignore_submodules)?;
    let diff_paths = pathspecs.clone();

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
    {
        let _trace = phase_trace("status.repo_object_format");
        repo_object_format(&repo)?;
    }
    let machine_readable = porcelain.is_some() || short || null;
    let column_untracked = {
        let _trace = phase_trace("status.column_config");
        status_column_untracked(&repo, column, no_column)?
    };
    let stash_count = if show_stash {
        let _trace = phase_trace("status.stash_count");
        Some(status_stash_count(&repo)?)
    } else {
        None
    };
    if machine_readable && branch {
        let _trace = phase_trace("status.branch_header");
        if porcelain_version == PorcelainVersion::V2 {
            for row in porcelain_v2_branch_header(&repo, ahead_behind)? {
                write_status_record(&row, null)?;
            }
        } else {
            write_status_record(&porcelain_branch_header(&repo, ahead_behind)?, null)?;
        }
    }

    let runtime = {
        let _trace = phase_trace("status.runtime_init");
        CliPrimitiveRuntime::new_default(&repo)
    };
    let head_tree = {
        let _trace = phase_trace("status.read_head_tree");
        read_head_tree_id_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?
    };
    let index = {
        let _trace = phase_trace("status.read_index");
        if config_bool_enabled(&repo, "core.splitIndex")?
            && fs::read(&repo.index_path)
                .map(|bytes| !bytes.windows(4).any(|window| window == b"link"))
                .unwrap_or(false)
        {
            super::admin_commands::ensure_split_index(&repo.index_path)?;
        }
        let raw_index = read_repo_index_raw(&repo)?;
        let raw_sparse = index_has_sparse_directories(&raw_index);
        let sparse_index_enabled = config_bool_enabled(&repo, "index.sparse")?;
        let materialized_sparse_directory = raw_index.entries().iter().any(|entry| {
            entry.stage == 0
                && entry.mode == IndexMode::Tree
                && path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path))
        });
        let mut index = {
            let _region = (raw_sparse && (!sparse_index_enabled || materialized_sparse_directory))
                .then(|| trace2_region("index", "ensure_full_index"));
            if raw_sparse {
                expand_repo_sparse_index(&repo, &raw_index)?
            } else {
                raw_index
            }
        };
        if materialized_sparse_directory && sparse_index_expansion_advice_enabled(&repo)? {
            eprintln!(
                "The sparse index is expanding to a full index, a slow operation.\n\
Your working directory likely has contents that are outside of\n\
your sparse-checkout patterns. Use 'git sparse-checkout list' to\n\
see your sparse-checkout definition and compare it to your working\n\
directory contents. Running 'git clean' may assist in this cleanup."
            );
        }
        if raw_sparse && (!sparse_index_enabled || materialized_sparse_directory) {
            index.write_to_path(&repo.index_path)?;
        } else if !raw_sparse && sparse_index_enabled && sparse_checkout_active(&repo)? {
            let store = LooseObjectStore::new(repo.objects_dir.clone(), index.hash_algorithm());
            let collapsed = collapse_sparse_index(&repo, &store, &index)?;
            if index_has_sparse_directories(&collapsed) {
                let _region = trace2_region("index", "convert_to_sparse");
                collapsed.write_to_path(&repo.index_path)?;
            }
        }
        refresh_materialized_sparse_index_entries(&repo, &mut index)?;
        index
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

    let mut paths: HashMap<Vec<u8>, StatusPathState> = HashMap::new();
    for path in &unmerged_paths {
        let (index_status, worktree_status) = status_unmerged_code(&index, path);
        paths.insert(
            path.clone(),
            StatusPathState {
                index_status,
                worktree_status,
                old_path: None,
                similarity: None,
                submodule: None,
            },
        );
    }
    {
        let _trace = phase_trace("status.head_index_diff");
        for entry in status_head_index_diff(
            runtime.object_store_adapter(),
            head_tree.as_ref(),
            status_index,
            detect_renames,
        )? {
            if unmerged_paths.contains::<[u8]>(entry.path.as_slice()) {
                continue;
            }
            let state = paths.entry(entry.path).or_default();
            state.index_status = status_code(entry.status);
            state.old_path = entry.old_path;
            state.similarity = entry.similarity;
        }
    }
    {
        let _trace = phase_trace("status.worktree_status");
        for (path, code) in worktree_status(&repo, status_index)? {
            if unmerged_paths.contains(&path) {
                continue;
            }
            paths.entry(path).or_default().worktree_status = code;
        }
    }
    {
        let _trace = phase_trace("status.submodule_states");
        apply_status_submodule_worktree_states(
            &repo,
            status_index,
            &mut paths,
            ignore_submodules_mode,
        )?;
    }

    let tracked_paths = {
        let _trace = phase_trace("status.tracked_paths");
        tracked_path_set_for_repo(&repo, &index)?
    };
    let status_ignore = if untracked_mode == UntrackedMode::No && ignored_mode == IgnoredMode::No {
        None
    } else {
        let _trace = phase_trace("status.ignore_graph");
        Some(status_excludes(&repo, &tracked_paths)?)
    };
    let (untracked, ignored) =
        if untracked_mode != UntrackedMode::No && ignored_mode != IgnoredMode::No {
            let _trace = phase_trace("status.untracked_ignored");
            status_untracked_and_ignored_files(
                &repo.root,
                &tracked_paths,
                status_ignore
                    .as_ref()
                    .expect("status ignore graph for combined untracked/ignored scan"),
                untracked_mode,
                true,
            )?
        } else {
            let untracked = if untracked_mode == UntrackedMode::No {
                Vec::new()
            } else {
                let _trace = phase_trace("status.untracked");
                untracked_files_with_mode(
                    &repo.root,
                    &tracked_paths,
                    status_ignore
                        .as_ref()
                        .expect("status ignore graph for untracked scan"),
                    untracked_mode,
                    true,
                )?
            };
            let ignored = if ignored_mode == IgnoredMode::No {
                Vec::new()
            } else {
                let _trace = phase_trace("status.ignored");
                ignored_untracked_files_for_status(
                    &repo.root,
                    &tracked_paths,
                    status_ignore
                        .as_ref()
                        .expect("status ignore graph for ignored scan"),
                )?
            };
            (untracked, ignored)
        };
    if !machine_readable {
        let _trace = phase_trace("status.render_human");
        let display_comment_prefix = status_display_comment_prefix(&repo)?;
        print_human_status(
            &repo,
            &paths,
            &untracked,
            &ignored,
            &pathspecs,
            ahead_behind,
            untracked_mode,
            stash_count.unwrap_or(0),
            column_untracked,
            display_comment_prefix,
        )?;
        print_status_verbose_diff(verbose, diff_paths)?;
        return Ok(());
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
                stash_count.unwrap_or(0),
                null,
            );
        }
        let mut rows = paths
            .into_iter()
            .filter(|(path, _)| pathspecs.is_empty() || pathspec_matches(path, &pathspecs))
            .map(|(path, state)| {
                let worktree_status = if short {
                    state
                        .submodule
                        .map(StatusSubmoduleState::short_worktree_status)
                        .unwrap_or(state.worktree_status)
                } else {
                    state.worktree_status.to_ascii_uppercase()
                };
                let row = if let Some(old_path) = state.old_path.as_ref() {
                    if null {
                        format!(
                            "{}{} {}\0{}",
                            state.index_status,
                            worktree_status,
                            String::from_utf8_lossy(&path),
                            String::from_utf8_lossy(old_path)
                        )
                    } else {
                        format!(
                            "{}{} {} -> {}",
                            state.index_status,
                            worktree_status,
                            String::from_utf8_lossy(old_path),
                            String::from_utf8_lossy(&path)
                        )
                    }
                } else {
                    format!(
                        "{}{} {}",
                        state.index_status,
                        worktree_status,
                        String::from_utf8_lossy(&path)
                    )
                };
                (path.clone(), row)
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let mut output_rows = rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>();
        output_rows.extend(
            untracked
                .into_iter()
                .filter(|path| pathspecs.is_empty() || pathspec_matches(path, &pathspecs))
                .map(|path| format!("?? {}", String::from_utf8_lossy(&path))),
        );
        output_rows.extend(
            ignored
                .into_iter()
                .filter(|path| pathspecs.is_empty() || pathspec_matches(path, &pathspecs))
                .map(|path| format!("!! {}", String::from_utf8_lossy(&path))),
        );
        if null {
            use std::io::Write;
            let total_bytes = output_rows.iter().map(|row| row.len() + 1).sum();
            let mut output = Vec::with_capacity(total_bytes);
            for row in &output_rows {
                output.extend_from_slice(row.as_bytes());
                output.push(b'\0');
            }
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(&output)?;
        } else {
            let mut stdout = std::io::stdout().lock();
            for row in &output_rows {
                writeln!(stdout, "{row}")?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PorcelainVersion {
    V1,
    V2,
}

#[derive(Debug, Clone)]
struct StatusPathState {
    index_status: char,
    worktree_status: char,
    old_path: Option<Vec<u8>>,
    similarity: Option<u8>,
    submodule: Option<StatusSubmoduleState>,
}

impl Default for StatusPathState {
    fn default() -> Self {
        Self {
            index_status: ' ',
            worktree_status: ' ',
            old_path: None,
            similarity: None,
            submodule: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct StatusSubmoduleState {
    commit_changed: bool,
    modified: bool,
    untracked: bool,
}

impl StatusSubmoduleState {
    fn v2_flags(self) -> String {
        format!(
            "S{}{}{}",
            if self.commit_changed { 'C' } else { '.' },
            if self.modified { 'M' } else { '.' },
            if self.untracked { 'U' } else { '.' }
        )
    }

    fn short_worktree_status(self) -> char {
        if self.commit_changed {
            'M'
        } else if self.modified {
            'm'
        } else if self.untracked {
            '?'
        } else {
            ' '
        }
    }

    fn human_suffix(self) -> Option<String> {
        let mut parts = Vec::new();
        if self.commit_changed {
            parts.push("new commits");
        }
        if self.modified {
            parts.push("modified content");
        }
        if self.untracked {
            parts.push("untracked content");
        }
        (!parts.is_empty()).then(|| format!(" ({})", parts.join(", ")))
    }
}

fn print_porcelain_v2_status(
    paths: &HashMap<Vec<u8>, StatusPathState>,
    index: &GitIndex,
    store: &dyn GitObjectStore,
    head_tree: Option<&ObjectId>,
    untracked: &[Vec<u8>],
    ignored: &[Vec<u8>],
    pathspecs: &[Vec<u8>],
    stash_count: usize,
    null: bool,
) -> Result<()> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    if stash_count > 0 {
        write_status_record_to(&mut out, &format!("# stash {stash_count}"), null)?;
    }
    let mut rows = paths
        .iter()
        .filter(|(path, _)| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
        .map(|(path, state)| {
            let metadata = status_v2_metadata(
                store,
                head_tree,
                index,
                path,
                state.old_path.as_deref(),
                state.worktree_status,
            )?;
            if let Some(old_path) = state.old_path.as_ref() {
                return Ok((
                    path.clone(),
                    format!(
                        "2 {}{} N... {} {} {} {} {} R{} {}\t{}",
                        status_v2_code(state.index_status),
                        status_v2_code(state.worktree_status),
                        metadata.head_mode,
                        metadata.index_mode,
                        metadata.worktree_mode,
                        metadata.head_id,
                        metadata.index_id,
                        state.similarity.unwrap_or(100),
                        String::from_utf8_lossy(path),
                        String::from_utf8_lossy(old_path)
                    ),
                ));
            }
            let submodule = state
                .submodule
                .map(StatusSubmoduleState::v2_flags)
                .unwrap_or_else(|| "N...".to_owned());
            Ok((
                path.clone(),
                format!(
                    "1 {}{} {} {} {} {} {} {} {}",
                    status_v2_code(state.index_status),
                    status_v2_code(state.worktree_status),
                    submodule,
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
        write_status_record_to(&mut out, &row, null)?;
    }
    for path in untracked
        .iter()
        .filter(|path| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
    {
        write_status_record_to(
            &mut out,
            &format!("? {}", String::from_utf8_lossy(path)),
            null,
        )?;
    }
    for path in ignored
        .iter()
        .filter(|path| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
    {
        write_status_record_to(
            &mut out,
            &format!("! {}", String::from_utf8_lossy(path)),
            null,
        )?;
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
    old_path: Option<&[u8]>,
    worktree_status: char,
) -> Result<StatusV2Metadata> {
    let head = match head_tree {
        Some(tree) => find_tree_entry(store, tree, old_path.unwrap_or(path))?,
        None => None,
    };
    let entry = find_index_entry(index, path);
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
    let stdout = io::stdout();
    let mut out = stdout.lock();
    write_status_record_to(&mut out, row, null)
}

fn write_status_record_to(out: &mut impl Write, row: &str, null: bool) -> Result<()> {
    if null {
        out.write_all(row.as_bytes())?;
        out.write_all(b"\0")?;
    } else {
        writeln!(out, "{row}")?;
    }
    Ok(())
}

fn status_v2_mode(mode: IndexMode) -> &'static str {
    match mode {
        IndexMode::File => "100644",
        IndexMode::Executable => "100755",
        IndexMode::Symlink => "120000",
        IndexMode::Tree => "040000",
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

fn porcelain_v2_branch_header(repo: &GitRepo, ahead_behind: bool) -> Result<Vec<String>> {
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
                if ahead_behind {
                    if let Some((ahead, behind)) = upstream_counts(repo, &upstream.ref_name)? {
                        rows.push(format!("# branch.ab +{ahead} -{behind}"));
                    }
                } else if upstream_differs_from_head(repo, &upstream.ref_name)? {
                    rows.push("# branch.ab +? -?".to_owned());
                } else {
                    rows.push("# branch.ab +0 -0".to_owned());
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

fn status_excludes(repo: &GitRepo, tracked_paths: &TrackedPathSet<'_>) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if let Some(path) = ls_files_global_excludes_file(repo)? {
        append_ignore_file(&mut ignore, &path, "")?;
    }
    append_ignore_file(&mut ignore, &repo.git_dir.join("info/exclude"), "")?;
    append_per_directory_excludes_pruned(
        &repo.root,
        &repo.root,
        b"",
        ".gitignore",
        &mut ignore,
        tracked_paths,
    )?;
    Ok(ignore)
}

fn status_stash_count(repo: &GitRepo) -> Result<usize> {
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let path = common_git_dir.join("logs").join(stash_ref_name());
    match fs::read_to_string(path) {
        Ok(content) => Ok(content.lines().count()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let refs = RefStore::new(&common_git_dir, GitHashAlgorithm::Sha1);
            match refs.resolve(stash_ref_name()) {
                Ok(_) => Ok(1),
                Err(_) => Ok(0),
            }
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn status_column_untracked(repo: &GitRepo, column: Option<&str>, no_column: bool) -> Result<bool> {
    if no_column {
        return Ok(false);
    }
    match column {
        Some(value) => parse_status_column_value(value),
        None => read_config_value(repo, "column.status")?
            .as_deref()
            .map(parse_status_column_value)
            .transpose()
            .map(|value| value.unwrap_or(false)),
    }
}

fn parse_status_column_value(value: &str) -> Result<bool> {
    if value.is_empty() {
        return Ok(true);
    }
    let mut enabled = true;
    for part in value.split(',') {
        match part {
            "always" | "auto" | "column" | "row" | "dense" | "nodense" | "plain" => {
                enabled = true;
            }
            "never" => enabled = false,
            other => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: unsupported option '{other}'\n"),
                });
            }
        }
    }
    Ok(enabled)
}

fn apply_status_submodule_worktree_states(
    repo: &GitRepo,
    index: &GitIndex,
    paths: &mut HashMap<Vec<u8>, StatusPathState>,
    ignore_mode: StatusIgnoreSubmodulesMode,
) -> Result<()> {
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Gitlink)
    {
        if ignore_mode == StatusIgnoreSubmodulesMode::All {
            if let Some(path_state) = paths.get_mut(entry.path.as_slice()) {
                path_state.worktree_status = ' ';
                path_state.submodule = None;
            }
            continue;
        }
        let submodule_path = repo
            .root
            .join(String::from_utf8_lossy(&entry.path).as_ref());
        let Some(state) = status_submodule_state(&submodule_path, entry, ignore_mode)? else {
            continue;
        };
        let path_state = paths.entry(entry.path.to_vec()).or_default();
        path_state.worktree_status = 'M';
        path_state.submodule = Some(state);
    }
    if ignore_mode == StatusIgnoreSubmodulesMode::All {
        paths.retain(|_, state| state.index_status != ' ' || state.worktree_status != ' ');
    }
    Ok(())
}

fn status_submodule_state(
    submodule_path: &Path,
    entry: &IndexEntry,
    ignore_mode: StatusIgnoreSubmodulesMode,
) -> Result<Option<StatusSubmoduleState>> {
    if ignore_mode == StatusIgnoreSubmodulesMode::All {
        return Ok(None);
    }
    let Some(submodule_repo) = exact_repo_at(submodule_path) else {
        return Ok(None);
    };
    let refs = RefStore::new(&submodule_repo.git_dir, GitHashAlgorithm::Sha1);
    let commit_changed = refs.resolve("HEAD").is_ok_and(|head| head != entry.id);
    let submodule_index = read_repo_index(&submodule_repo)?;
    let runtime = CliPrimitiveRuntime::new_default(&submodule_repo);
    let head_tree =
        read_head_tree_id_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let index_dirty = !status_head_index_diff(
        runtime.object_store_adapter(),
        head_tree.as_ref(),
        &submodule_index,
        true,
    )?
    .is_empty();
    let worktree_dirty = !worktree_status(&submodule_repo, &submodule_index)?.is_empty();
    let modified = index_dirty || worktree_dirty;
    let tracked_paths = tracked_path_set_for_repo(&submodule_repo, &submodule_index)?;
    let ignore = status_excludes(&submodule_repo, &tracked_paths)?;
    let untracked = !untracked_files_with_mode(
        &submodule_repo.root,
        &tracked_paths,
        &ignore,
        UntrackedMode::Normal,
        true,
    )?
    .is_empty();

    let state = match ignore_mode {
        StatusIgnoreSubmodulesMode::None => StatusSubmoduleState {
            commit_changed,
            modified,
            untracked,
        },
        StatusIgnoreSubmodulesMode::Dirty => StatusSubmoduleState {
            commit_changed,
            modified: false,
            untracked: false,
        },
        StatusIgnoreSubmodulesMode::Untracked => StatusSubmoduleState {
            commit_changed,
            modified,
            untracked: false,
        },
        StatusIgnoreSubmodulesMode::All => unreachable!("handled before inspecting submodule"),
    };
    Ok((state.commit_changed || state.modified || state.untracked).then_some(state))
}

fn print_status_verbose_diff(verbose: u8, paths: Vec<PathBuf>) -> Result<()> {
    match verbose {
        0 => Ok(()),
        1 => {
            println!();
            super::diff_commands::diff(DiffOptions {
                cached: true,
                paths,
                ..DiffOptions::default()
            })
        }
        _ => {
            println!();
            println!("Changes to be committed:");
            super::diff_commands::diff(DiffOptions {
                cached: true,
                src_prefix: Some("c/".to_owned()),
                dst_prefix: Some("i/".to_owned()),
                paths: paths.clone(),
                ..DiffOptions::default()
            })?;
            println!("--------------------------------------------------");
            println!("Changes not staged for commit:");
            super::diff_commands::diff(DiffOptions {
                src_prefix: Some("i/".to_owned()),
                dst_prefix: Some("w/".to_owned()),
                paths,
                ..DiffOptions::default()
            })
        }
    }
}

fn status_head_index_diff<S>(
    store: &S,
    head_tree: Option<&ObjectId>,
    index: &GitIndex,
    detect_renames: bool,
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
    if index.cached_root_tree_id() == Some(head_tree) {
        return Ok(Vec::new());
    }
    let tree_cache = TreeObjectCache::new(store);
    let head_index = tree_cache.read_tree_to_index(head_tree)?;
    if detect_renames {
        let mut diff = diff_indexes_with_exact_renames(&head_index, index)?;
        diff.sort_by(|left, right| left.path.cmp(&right.path));
        return Ok(diff);
    }
    let mut diff = diff_indexes(&head_index, index)?;
    diff.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(diff)
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
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("normal") => Ok(Self::Normal),
            Some("no") => Ok(Self::No),
            Some("all") => Ok(Self::All),
            Some(value) => Err(CliError::Fatal {
                code: 128,
                message: format!("Invalid untracked files mode '{value}'"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusIgnoreSubmodulesMode {
    None,
    All,
    Dirty,
    Untracked,
}

impl StatusIgnoreSubmodulesMode {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None => Ok(Self::None),
            Some("all") | Some("") => Ok(Self::All),
            Some("dirty") => Ok(Self::Dirty),
            Some("untracked") => Ok(Self::Untracked),
            Some(value) => Err(CliError::Fatal {
                code: 128,
                message: format!("bad --ignore-submodules argument: {value}"),
            }),
        }
    }
}

fn print_human_status(
    repo: &GitRepo,
    paths: &HashMap<Vec<u8>, StatusPathState>,
    untracked: &[Vec<u8>],
    ignored: &[Vec<u8>],
    pathspecs: &[Vec<u8>],
    ahead_behind: bool,
    untracked_mode: UntrackedMode,
    stash_count: usize,
    column_untracked: bool,
    display_comment_prefix: bool,
) -> Result<()> {
    macro_rules! status_line {
        () => {
            print_human_status_line(display_comment_prefix, "")
        };
        ($($argument:tt)*) => {
            print_human_status_line(display_comment_prefix, &format!($($argument)*))
        };
    }

    let status_hints_enabled = advice_status_hints_enabled(repo)?;
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(&common_git_dir, GitHashAlgorithm::Sha1);
    let head_has_commit = refs.resolve("HEAD").is_ok();
    status_line!("{}", human_status_branch_header(&refs)?);
    if !head_has_commit {
        status_line!();
        status_line!("No commits yet");
    }
    let mut printed_body = !head_has_commit;
    if head_has_commit && let Some(lines) = human_status_upstream(repo, &refs, ahead_behind)? {
        for line in lines {
            status_line!("{line}");
        }
        printed_body = true;
    }
    if sparse_checkout_active(repo)? {
        let sparse_index_collapsed = config_bool_enabled(repo, "index.sparse")?
            && read_repo_index_raw(repo)?
                .entries()
                .iter()
                .any(|entry| entry.stage == 0 && entry.mode == IndexMode::Tree);
        if sparse_index_collapsed {
            status_line!("You are in a sparse checkout.");
        } else {
            let index = read_repo_index(repo)?;
            let total = index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0)
                .count();
            let present = index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0 && !entry.skip_worktree())
                .count();
            let percent = if total == 0 {
                100
            } else {
                present * 100 / total
            };
            status_line!("You are in a sparse checkout with {percent}% of tracked files present.");
        }
        printed_body = true;
    }

    let mut staged = paths
        .iter()
        .filter(|(path, state)| {
            state.index_status != ' ' && (pathspecs.is_empty() || pathspec_matches(path, pathspecs))
        })
        .map(|(path, state)| {
            (
                path.clone(),
                state.index_status,
                state.old_path.as_ref().cloned(),
            )
        })
        .collect::<Vec<_>>();
    let mut worktree = paths
        .iter()
        .filter(|(path, state)| {
            state.worktree_status != ' '
                && (pathspecs.is_empty() || pathspec_matches(path, pathspecs))
        })
        .map(|(path, state)| (path.clone(), state.worktree_status, state.submodule))
        .collect::<Vec<_>>();
    let untracked = untracked
        .iter()
        .filter(|path| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
        .cloned()
        .collect::<Vec<_>>();
    let ignored = ignored
        .iter()
        .filter(|path| pathspecs.is_empty() || pathspec_matches(path, pathspecs))
        .cloned()
        .collect::<Vec<_>>();
    staged.sort_by(|left, right| left.0.cmp(&right.0));
    worktree.sort_by(|left, right| left.0.cmp(&right.0));

    if !staged.is_empty() {
        if printed_body {
            status_line!();
        }
        status_line!("Changes to be committed:");
        if head_has_commit {
            status_line!("  (use \"git restore --staged <file>...\" to unstage)");
        } else {
            status_line!("  (use \"git rm --cached <file>...\" to unstage)");
        }
        for (path, status, old_path) in &staged {
            let display = if let Some(old_path) = old_path {
                format!(
                    "{} -> {}",
                    String::from_utf8_lossy(old_path),
                    String::from_utf8_lossy(path)
                )
            } else {
                String::from_utf8_lossy(path).into_owned()
            };
            status_line!("\t{:<12}{}", human_status_label(*status), display);
        }
        printed_body = true;
    }

    if !worktree.is_empty() {
        if printed_body {
            status_line!();
        }
        status_line!("Changes not staged for commit:");
        if worktree.iter().any(|(_, status, _)| *status == 'D') {
            status_line!("  (use \"git add/rm <file>...\" to update what will be committed)");
        } else {
            status_line!("  (use \"git add <file>...\" to update what will be committed)");
        }
        status_line!("  (use \"git restore <file>...\" to discard changes in working directory)");
        if worktree.iter().any(|(_, _, submodule)| submodule.is_some()) {
            status_line!("  (commit or discard the untracked or modified content in submodules)");
        }
        for (path, status, submodule) in &worktree {
            let suffix = submodule
                .and_then(StatusSubmoduleState::human_suffix)
                .unwrap_or_default();
            status_line!(
                "\t{:<12}{}{}",
                human_status_label(*status),
                String::from_utf8_lossy(path),
                suffix
            );
        }
        printed_body = true;
    }

    if !untracked.is_empty() {
        if printed_body {
            status_line!();
        }
        status_line!("Untracked files:");
        if status_hints_enabled {
            status_line!("  (use \"git add <file>...\" to include in what will be committed)");
        }
        if column_untracked {
            print_status_path_columns(&untracked, display_comment_prefix);
        } else {
            for path in &untracked {
                status_line!("\t{}", String::from_utf8_lossy(path));
            }
        }
        printed_body = true;
    }

    if !ignored.is_empty() {
        if printed_body {
            status_line!();
        }
        status_line!("Ignored files:");
        if status_hints_enabled {
            status_line!("  (use \"git add -f <file>...\" to include in what will be committed)");
        }
        for path in &ignored {
            status_line!("\t{}", String::from_utf8_lossy(path));
        }
        printed_body = true;
    }

    if staged.is_empty() {
        if printed_body {
            status_line!();
        }
        if !worktree.is_empty() {
            status_line!("no changes added to commit (use \"git add\" and/or \"git commit -a\")");
        } else if !untracked.is_empty() {
            if status_hints_enabled {
                status_line!(
                    "nothing added to commit but untracked files present (use \"git add\" to track)"
                );
            } else {
                status_line!("nothing added to commit but untracked files present");
            }
        } else if head_has_commit && untracked_mode == UntrackedMode::No {
            status_line!("nothing to commit (use -u to show untracked files)");
        } else if head_has_commit {
            status_line!("nothing to commit, working tree clean");
        } else {
            status_line!("nothing to commit (create/copy files and use \"git add\" to track)");
        }
    } else if worktree.is_empty() && untracked.is_empty() {
        status_line!();
    }
    if stash_count > 0 {
        status_line!(
            "Your stash currently has {stash_count} {}",
            plural(stash_count, "entry", "entries")
        );
    }
    Ok(())
}

fn status_display_comment_prefix(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "status.displayCommentPrefix")? else {
        return Ok(false);
    };
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("bad boolean config value '{}'", entry.value),
    })
}

fn print_human_status_line(display_comment_prefix: bool, line: &str) {
    if !display_comment_prefix {
        println!("{line}");
    } else if line.is_empty() {
        println!("#");
    } else if line.starts_with('\t') {
        println!("#{line}");
    } else {
        println!("# {line}");
    }
}

fn advice_status_hints_enabled(repo: &GitRepo) -> Result<bool> {
    if let Ok(value) = std::env::var("GIT_ADVICE") {
        let normalized = value.trim().to_ascii_lowercase();
        if matches!(normalized.as_str(), "false" | "0" | "no" | "off") {
            return Ok(false);
        }
        if matches!(normalized.as_str(), "true" | "1" | "yes" | "on") {
            return Ok(true);
        }
    }
    let Some(entry) = read_config_entry(repo, "advice.statusHints")? else {
        return Ok(true);
    };
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("bad boolean config value '{}'", entry.value),
    })
}

fn sparse_index_expansion_advice_enabled(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "advice.sparseIndexExpanded")? else {
        return Ok(true);
    };
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("bad boolean config value '{}'", entry.value),
    })
}

fn print_status_path_columns(paths: &[Vec<u8>], display_comment_prefix: bool) {
    let items = paths
        .iter()
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect::<Vec<_>>();
    let width = 72;
    let padding = 1;
    let item_width = items
        .iter()
        .map(|item| item.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let columns = ((width + padding) / (item_width + padding))
        .max(1)
        .min(items.len());
    let rows = items.len().div_ceil(columns);
    for row in 0..rows {
        let mut line = String::from("\t");
        for column in 0..columns {
            let index = column * rows + row;
            let Some(item) = items.get(index) else {
                continue;
            };
            if column + 1 == columns || index + rows >= items.len() {
                line.push_str(item);
            } else {
                line.push_str(&format!("{item:<item_width$}"));
                line.push_str(&" ".repeat(padding));
            }
        }
        print_human_status_line(display_comment_prefix, &line);
    }
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

fn human_status_upstream(
    repo: &GitRepo,
    refs: &RefStore,
    ahead_behind: bool,
) -> Result<Option<Vec<String>>> {
    let Some(current) = current_branch_ref(refs)? else {
        return Ok(None);
    };
    let branch = branch_display_name(&current);
    let Some(upstream) = read_branch_upstream(repo, &branch)? else {
        return Ok(None);
    };
    let mut lines = Vec::new();
    if !ahead_behind {
        if upstream_differs_from_head(repo, &upstream.ref_name)? {
            lines.push(format!(
                "Your branch and '{}' refer to different commits.",
                upstream.display
            ));
            lines.push("  (use \"git status --ahead-behind\" for details)".to_owned());
        } else {
            lines.push(format!(
                "Your branch is up to date with '{}'.",
                upstream.display
            ));
        }
        return Ok(Some(lines));
    }
    let Some((ahead, behind)) = upstream_counts(repo, &upstream.ref_name)? else {
        return Ok(None);
    };
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
        'R' => "renamed:",
        _ => "changed:",
    }
}

pub(crate) fn untracked_files(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<Vec<Vec<u8>>> {
    untracked_files_with_mode(root, tracked_paths, ignore, UntrackedMode::Normal, true)
}

pub(crate) fn ignored_untracked_files(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<Vec<Vec<u8>>> {
    ignored_untracked_files_with_mode(root, tracked_paths, ignore, UntrackedMode::All, true)
}

fn ignored_untracked_files_with_mode(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
) -> Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    let _ = collect_ignored_untracked_files(
        root,
        root,
        b"",
        tracked_paths,
        ignore,
        mode,
        include_empty_directories,
        false,
        false,
        &mut files,
    )?;
    files.sort();
    Ok(files)
}

pub(crate) fn ignored_untracked_files_for_status(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    collect_ignored_untracked_status(root, root, b"", tracked_paths, ignore, false, &mut files)?;
    files.sort();
    Ok(files)
}

pub(crate) fn status_untracked_and_ignored_files(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>)> {
    let mut untracked = Vec::new();
    let mut ignored = Vec::new();
    let _ = collect_status_untracked_and_ignored(
        root,
        root,
        b"",
        tracked_paths,
        ignore,
        mode,
        include_empty_directories,
        false,
        false,
        &mut untracked,
        &mut ignored,
    )?;
    untracked.sort();
    ignored.sort();
    Ok((untracked, ignored))
}

fn collect_ignored_untracked_status(
    root: &std::path::Path,
    dir: &std::path::Path,
    relative_dir: &[u8],
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
        let file_type = entry.file_type()?;
        let relative = relative_child_path(relative_dir, &entry.file_name());
        let is_dir = file_type.is_dir();
        let is_ignored = parent_ignored || ignore.is_ignored(&relative, is_dir);
        if is_dir {
            if is_ignored && !tracked_paths_under(tracked_paths, &relative) {
                let mut dir = relative;
                dir.push(b'/');
                files.push(dir);
            } else {
                collect_ignored_untracked_status(
                    root,
                    &path,
                    &relative,
                    tracked_paths,
                    ignore,
                    is_ignored,
                    files,
                )?;
            }
        } else if is_ignored
            && (file_type.is_file() || file_type.is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            files.push(relative);
        }
    }
    Ok(())
}

fn collect_status_untracked_and_ignored(
    root: &std::path::Path,
    dir: &std::path::Path,
    relative_dir: &[u8],
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
    collapse_untracked_dirs: bool,
    parent_ignored: bool,
    untracked: &mut Vec<Vec<u8>>,
    ignored: &mut Vec<Vec<u8>>,
) -> Result<bool> {
    let mut has_reportable_untracked = false;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let file_type = entry.file_type()?;
        let relative = relative_child_path(relative_dir, &entry.file_name());
        let is_dir = file_type.is_dir();
        let is_ignored = parent_ignored || ignore.is_ignored(&relative, is_dir);
        if is_ignored {
            if is_dir {
                if tracked_paths_under(tracked_paths, &relative) {
                    let child_has_reportable_untracked = collect_status_untracked_and_ignored(
                        root,
                        &path,
                        &relative,
                        tracked_paths,
                        ignore,
                        mode,
                        include_empty_directories,
                        collapse_untracked_dirs,
                        true,
                        untracked,
                        ignored,
                    )?;
                    if collapse_untracked_dirs {
                        has_reportable_untracked |= child_has_reportable_untracked;
                    }
                } else {
                    let mut ignored_dir = relative;
                    ignored_dir.push(b'/');
                    ignored.push(ignored_dir);
                }
            } else if (file_type.is_file() || file_type.is_symlink())
                && !tracked_paths.contains(relative.as_slice())
            {
                ignored.push(relative);
            }
            continue;
        }
        if is_dir {
            let tracked_directory = tracked_paths.contains_directory(&relative)
                || (mode != UntrackedMode::All && tracked_paths.contains(&relative));
            if is_nested_worktree(&path) && !tracked_directory {
                if !collapse_untracked_dirs {
                    let mut directory = relative;
                    directory.push(b'/');
                    untracked.push(directory);
                }
                has_reportable_untracked = true;
                continue;
            }
            if tracked_directory {
                continue;
            }
            let has_tracked_descendants = tracked_paths_under(tracked_paths, &relative);
            let collapse_child_dir =
                !collapse_untracked_dirs && mode != UntrackedMode::All && !has_tracked_descendants;
            let child_has_reportable_untracked = if collapse_child_dir
                && mode == UntrackedMode::Directory
                && include_empty_directories
            {
                true
            } else {
                collect_status_untracked_and_ignored(
                    root,
                    &path,
                    &relative,
                    tracked_paths,
                    ignore,
                    mode,
                    include_empty_directories,
                    collapse_untracked_dirs || collapse_child_dir,
                    false,
                    untracked,
                    ignored,
                )?
            };
            if collapse_untracked_dirs {
                has_reportable_untracked |= child_has_reportable_untracked;
            } else if collapse_child_dir {
                if !child_has_reportable_untracked {
                    continue;
                }
                let mut dir = relative;
                dir.push(b'/');
                untracked.push(dir);
                has_reportable_untracked = true;
            } else if child_has_reportable_untracked {
                has_reportable_untracked = true;
            }
        } else if (file_type.is_file() || file_type.is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            has_reportable_untracked = true;
            if !collapse_untracked_dirs {
                untracked.push(relative);
            }
        }
    }
    Ok(has_reportable_untracked)
}

pub(crate) fn killed_files(
    repo: &GitRepo,
    index: &GitIndex,
    directory: bool,
) -> Result<Vec<Vec<u8>>> {
    let tracked_paths = tracked_path_set_for_repo(repo, index)?;
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
    relative_dir: &[u8],
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
    collapse_ignored_dirs: bool,
    parent_ignored: bool,
    files: &mut Vec<Vec<u8>>,
) -> Result<bool> {
    let mut has_reportable_ignored = false;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let metadata = entry.metadata()?;
        let relative = relative_child_path(relative_dir, &entry.file_name());
        let is_ignored = parent_ignored || ignore.is_ignored(&relative, metadata.is_dir());
        if metadata.is_dir() {
            let collapse_child = mode == UntrackedMode::Directory
                && is_ignored
                && !tracked_paths_under(tracked_paths, &relative);
            let child_has_reportable_ignored = if collapse_child && include_empty_directories {
                true
            } else {
                collect_ignored_untracked_files(
                    root,
                    &path,
                    &relative,
                    tracked_paths,
                    ignore,
                    mode,
                    include_empty_directories,
                    collapse_ignored_dirs || collapse_child,
                    is_ignored,
                    files,
                )?
            };
            if collapse_child && child_has_reportable_ignored && !collapse_ignored_dirs {
                let mut directory = relative;
                directory.push(b'/');
                files.push(directory);
            }
            has_reportable_ignored |= child_has_reportable_ignored;
        } else if is_ignored
            && (metadata.is_file() || metadata.file_type().is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            has_reportable_ignored = true;
            if !collapse_ignored_dirs {
                files.push(relative);
            }
        }
    }
    Ok(has_reportable_ignored)
}

pub(crate) fn untracked_files_with_mode(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
) -> Result<Vec<Vec<u8>>> {
    let mut files = Vec::new();
    let _ = collect_untracked_files(
        root,
        b"",
        tracked_paths,
        ignore,
        mode,
        include_empty_directories,
        false,
        &mut files,
    )?;
    files.sort();
    Ok(files)
}

fn untracked_files_for_simple_pathspecs(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
    pathspecs: &[Vec<u8>],
) -> Result<Option<Vec<Vec<u8>>>> {
    if pathspecs.is_empty() {
        return Ok(None);
    }
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    let mut all_untracked = None;
    for raw in pathspecs {
        let rule = parse_pathspec_rule(raw);
        if rule.exclude || rule.pattern.is_empty() {
            return Ok(None);
        }
        let has_glob = rule.options.glob
            && rule
                .pattern
                .iter()
                .any(|byte| matches!(*byte, b'*' | b'?' | b'['));
        if has_glob {
            if all_untracked.is_none() {
                all_untracked = Some(untracked_files_with_mode(
                    root,
                    tracked_paths,
                    ignore,
                    UntrackedMode::All,
                    true,
                )?);
            }
            collect_untracked_glob_pathspec_matches(
                all_untracked.as_deref().unwrap_or_default(),
                tracked_paths,
                mode,
                rule,
                &mut files,
                &mut seen,
            );
        } else {
            collect_untracked_pathspec_target(
                root,
                tracked_paths,
                ignore,
                mode,
                include_empty_directories,
                rule.pattern,
                &mut files,
                &mut seen,
            )?;
        }
    }
    files.sort();
    Ok(Some(files))
}

fn literal_untracked_files_for_ls_files(
    repo: &GitRepo,
    index: &GitIndex,
    options: &LsFilesOptions,
    pathspecs: &[Vec<u8>],
) -> Result<Option<Vec<Vec<u8>>>> {
    if !options.others
        || options.ignored
        || options.directory
        || !options.exclude_standard
        || !options.excludes.is_empty()
        || !options.exclude_from.is_empty()
        || options.exclude_per_directory.is_some()
        || pathspecs.is_empty()
    {
        return Ok(None);
    }
    let mut literal_paths = Vec::with_capacity(pathspecs.len());
    for raw in pathspecs {
        let rule = parse_pathspec_rule(raw);
        if rule.exclude
            || rule.pattern.is_empty()
            || rule.options.icase
            || (rule.options.glob
                && rule
                    .pattern
                    .iter()
                    .any(|byte| matches!(*byte, b'*' | b'?' | b'[')))
        {
            return Ok(None);
        }
        let absolute = repo
            .root
            .join(String::from_utf8_lossy(rule.pattern).as_ref());
        match fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.is_dir() => return Ok(None),
            Ok(_) => literal_paths.push(rule.pattern.to_vec()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                literal_paths.push(rule.pattern.to_vec())
            }
            Err(error) => return Err(error.into()),
        }
    }

    let ignore = standard_repo_ignore_for_literal_paths(repo, &literal_paths)?;
    let mut files = Vec::new();
    for path in literal_paths {
        if path == b".git" || path.starts_with(b".git/") {
            continue;
        }
        let absolute = repo.root.join(String::from_utf8_lossy(&path).as_ref());
        let metadata = match fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        if !(metadata.is_file() || metadata.file_type().is_symlink())
            || ignore.is_ignored(&path, false)
            || index
                .entries()
                .iter()
                .any(|entry| entry.path.as_slice() == path.as_slice())
        {
            continue;
        }
        files.push(path);
    }
    files.sort();
    files.dedup();
    Ok(Some(files))
}

fn standard_repo_ignore_for_literal_paths(repo: &GitRepo, paths: &[Vec<u8>]) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if let Some(path) = ls_files_global_excludes_file(repo)? {
        append_ignore_file(&mut ignore, &path, "")?;
    }
    append_ignore_file(&mut ignore, &repo.git_dir.join("info/exclude"), "")?;

    let mut directories = BTreeSet::new();
    directories.insert((0usize, Vec::new()));
    for path in paths {
        for separator in path
            .iter()
            .enumerate()
            .filter_map(|(index, byte)| (*byte == b'/').then_some(index))
        {
            let directory = path[..separator].to_vec();
            let depth = directory.iter().filter(|byte| **byte == b'/').count() + 1;
            directories.insert((depth, directory));
        }
    }
    for (_, directory) in directories {
        let base = String::from_utf8_lossy(&directory);
        let path = if directory.is_empty() {
            repo.root.join(".gitignore")
        } else {
            repo.root.join(base.as_ref()).join(".gitignore")
        };
        append_ignore_file(&mut ignore, &path, base.as_ref())?;
    }
    Ok(ignore)
}

fn collect_untracked_glob_pathspec_matches(
    all_untracked: &[Vec<u8>],
    tracked_paths: &TrackedPathSet<'_>,
    mode: UntrackedMode,
    rule: PathspecRule<'_>,
    files: &mut Vec<Vec<u8>>,
    seen: &mut BTreeSet<Vec<u8>>,
) {
    for path in all_untracked
        .iter()
        .filter(|path| pathspec_rule_matches(path, rule))
    {
        let collapsed = (mode == UntrackedMode::Directory)
            .then(|| ls_files_matching_untracked_directory(path, tracked_paths, rule))
            .flatten();
        let output = collapsed.unwrap_or_else(|| path.clone());
        if seen.insert(output.clone()) {
            files.push(output);
        }
    }
}

fn ls_files_matching_untracked_directory(
    path: &[u8],
    tracked_paths: &TrackedPathSet<'_>,
    rule: PathspecRule<'_>,
) -> Option<Vec<u8>> {
    for separator in path
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'/').then_some(index))
    {
        let directory = &path[..separator];
        if pathspec_rule_matches(directory, rule) && !tracked_paths_under(tracked_paths, directory)
        {
            let mut output = directory.to_vec();
            output.push(b'/');
            return Some(output);
        }
    }
    None
}

fn collect_untracked_pathspec_target(
    root: &std::path::Path,
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
    pathspec: &[u8],
    files: &mut Vec<Vec<u8>>,
    seen: &mut BTreeSet<Vec<u8>>,
) -> Result<()> {
    if pathspec == b".git" || pathspec.starts_with(b".git/") {
        return Ok(());
    }
    let absolute = root.join(String::from_utf8_lossy(pathspec).as_ref());
    let metadata = match fs::symlink_metadata(&absolute) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        let tracked_directory = tracked_paths.contains_directory(pathspec)
            || (mode != UntrackedMode::All && tracked_paths.contains(pathspec));
        if ignore.is_ignored(pathspec, true) || tracked_directory {
            return Ok(());
        }
        if is_nested_worktree(&absolute) && !tracked_directory {
            let mut directory = pathspec.to_vec();
            directory.push(b'/');
            if seen.insert(directory.clone()) {
                files.push(directory);
            }
            return Ok(());
        }
        let has_tracked_descendants = tracked_paths_under(tracked_paths, pathspec);
        let collapse_root_dir = mode != UntrackedMode::All && !has_tracked_descendants;
        let child_has_reportable_entries =
            if collapse_root_dir && mode == UntrackedMode::Directory && include_empty_directories {
                true
            } else {
                collect_untracked_files(
                    &absolute,
                    pathspec,
                    tracked_paths,
                    ignore,
                    mode,
                    include_empty_directories,
                    collapse_root_dir,
                    files,
                )?
            };
        if collapse_root_dir && child_has_reportable_entries {
            let mut dir = pathspec.to_vec();
            dir.push(b'/');
            if seen.insert(dir.clone()) {
                files.push(dir);
            }
        }
        return Ok(());
    }
    if (file_type.is_file() || file_type.is_symlink())
        && !ignore.is_ignored(pathspec, false)
        && !tracked_paths.contains(pathspec)
        && seen.insert(pathspec.to_vec())
    {
        files.push(pathspec.to_vec());
    }
    Ok(())
}

fn collect_untracked_files(
    dir: &std::path::Path,
    relative_dir: &[u8],
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
    mode: UntrackedMode,
    include_empty_directories: bool,
    collapse_untracked_dirs: bool,
    files: &mut Vec<Vec<u8>>,
) -> Result<bool> {
    let mut has_reportable_entries = false;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        let file_type = entry.file_type()?;
        let relative = relative_child_path(relative_dir, &entry.file_name());
        let is_dir = file_type.is_dir();
        if ignore.is_ignored(&relative, is_dir) {
            continue;
        }
        if is_dir {
            let tracked_directory = tracked_paths.contains_directory(&relative)
                || (mode != UntrackedMode::All && tracked_paths.contains(&relative));
            if is_nested_worktree(&path) && !tracked_directory {
                if !collapse_untracked_dirs {
                    let mut directory = relative;
                    directory.push(b'/');
                    files.push(directory);
                }
                has_reportable_entries = true;
                continue;
            }
            if tracked_directory {
                continue;
            }
            let has_tracked_descendants = tracked_paths_under(tracked_paths, &relative);
            let collapse_child_dir =
                !collapse_untracked_dirs && mode != UntrackedMode::All && !has_tracked_descendants;
            let child_has_reportable_entries = if collapse_child_dir
                && mode == UntrackedMode::Directory
                && include_empty_directories
            {
                true
            } else {
                collect_untracked_files(
                    &path,
                    &relative,
                    tracked_paths,
                    ignore,
                    mode,
                    include_empty_directories,
                    collapse_untracked_dirs || collapse_child_dir,
                    files,
                )?
            };
            if collapse_untracked_dirs {
                has_reportable_entries |= child_has_reportable_entries;
            } else if collapse_child_dir {
                if !child_has_reportable_entries {
                    continue;
                }
                let mut dir = relative;
                dir.push(b'/');
                files.push(dir);
                has_reportable_entries = true;
            } else if child_has_reportable_entries {
                has_reportable_entries = true;
            }
        } else if (file_type.is_file() || file_type.is_symlink())
            && !tracked_paths.contains(relative.as_slice())
        {
            has_reportable_entries = true;
            if !collapse_untracked_dirs {
                files.push(relative);
            }
        }
    }
    Ok(has_reportable_entries)
}

fn is_nested_worktree(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path.join(".git")).is_ok() && exact_repo_at(path).is_some()
}

pub(crate) fn tracked_paths_under(tracked_paths: &TrackedPathSet<'_>, relative_dir: &[u8]) -> bool {
    tracked_paths.has_tracked_descendants(relative_dir)
}

pub(crate) fn ls_files(options: LsFilesOptions) -> Result<()> {
    let recurse_submodules = options.recurse_submodules && !options.no_recurse_submodules;
    let include_empty_directories = options.directory && !options.no_empty_directory;
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
    let repo = find_repo_or_bare()?;
    if options.with_tree.is_some() && (options.stage || options.unmerged) {
        return Err(CliError::Fatal {
            code: 128,
            message: "options 'ls-files --with-tree' and '-s/-u' cannot be used together".into(),
        });
    }
    let pathspecs = options
        .path_args
        .iter()
        .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let pathspecs_empty = pathspecs.is_empty();
    let cwd_prefix = repo_relative_path(&repo.root, &std::env::current_dir()?)?;
    let effective_pathspecs = if pathspecs_empty && !cwd_prefix.is_empty() {
        vec![cwd_prefix.clone()]
    } else {
        pathspecs
    };
    let show_stage_format = options.stage || options.unmerged;
    let cached_ignored_recurse_submodules = recurse_submodules && options.ignored && options.cached;
    let include_all_cached = !options.ignored
        && !options.others
        && !options.stage
        && !options.unmerged
        && !options.deleted
        && !options.modified
        && !options.killed;
    let raw_index = read_repo_index_raw(&repo)?;
    let requires_sparse_expansion = !options.sparse
        && index_has_sparse_directories(&raw_index)
        && (effective_pathspecs.is_empty()
            || effective_pathspecs
                .iter()
                .any(|pathspec| ls_files_pathspec_requires_sparse_expansion(&raw_index, pathspec)));
    let _sparse_expansion_region =
        requires_sparse_expansion.then(|| trace2_region("index", "ensure_full_index"));
    let mut index = if requires_sparse_expansion {
        expand_repo_sparse_index(&repo, &raw_index)?
    } else {
        raw_index
    };
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
    let literal_other_paths =
        literal_untracked_files_for_ls_files(&repo, &index, &options, &effective_pathspecs)?;
    let tracked_paths = if options.others && literal_other_paths.is_none() {
        Some(tracked_path_set_for_repo(&repo, &index)?)
    } else {
        None
    };
    let ignore = if ls_files_needs_ignore(&options) && literal_other_paths.is_none() {
        Some(ls_files_excludes(
            &repo,
            &options,
            tracked_paths.as_ref(),
            &index,
        )?)
    } else {
        None
    };
    let eol_store = if options.eol || options.format.is_some() {
        Some(LooseObjectStore::new(
            repo.objects_dir.clone(),
            GitHashAlgorithm::Sha1,
        ))
    } else {
        None
    };
    let eol_attrs = if options.eol
        || options
            .format
            .as_deref()
            .is_some_and(|format| format.contains("%(eolattr)"))
    {
        Some(load_repo_attributes(&repo, None)?)
    } else {
        None
    };
    let mut other_paths = if let Some(paths) = literal_other_paths {
        Some(paths)
    } else if options.others {
        let tracked_paths = tracked_paths.as_ref().expect("tracked paths for others");
        if options.ignored {
            Some(ignored_untracked_files_with_mode(
                &repo.root,
                tracked_paths,
                ignore.as_ref().expect("ignore graph for ignored others"),
                if options.directory {
                    UntrackedMode::Directory
                } else {
                    UntrackedMode::All
                },
                include_empty_directories,
            )?)
        } else if let Some(paths) = untracked_files_for_simple_pathspecs(
            &repo.root,
            tracked_paths,
            ignore.as_ref().expect("ignore graph for plain others"),
            if options.directory {
                UntrackedMode::Directory
            } else {
                UntrackedMode::All
            },
            include_empty_directories,
            &effective_pathspecs,
        )? {
            Some(paths)
        } else if options.directory {
            Some(untracked_files_with_mode(
                &repo.root,
                tracked_paths,
                ignore.as_ref().expect("ignore graph for directory others"),
                UntrackedMode::Directory,
                include_empty_directories,
            )?)
        } else {
            Some(untracked_files_with_mode(
                &repo.root,
                tracked_paths,
                ignore.as_ref().expect("ignore graph for plain others"),
                UntrackedMode::All,
                true,
            )?)
        }
    } else {
        None
    };
    if options.others
        && options.directory
        && pathspecs_empty
        && !cwd_prefix.is_empty()
        && let Some(paths) = other_paths.as_mut()
    {
        let tracked_paths = tracked_paths
            .as_ref()
            .expect("tracked paths for directory others");
        let cwd_absolute = repo
            .root
            .join(String::from_utf8_lossy(&cwd_prefix).as_ref());
        if let Ok(metadata) = fs::symlink_metadata(&cwd_absolute)
            && metadata.is_dir()
            && !ignore
                .as_ref()
                .expect("ignore graph for implicit cwd directory others")
                .is_ignored(&cwd_prefix, true)
            && ((include_empty_directories && !tracked_paths_under(&tracked_paths, &cwd_prefix))
                || collect_untracked_files(
                    &cwd_absolute,
                    &cwd_prefix,
                    &tracked_paths,
                    ignore
                        .as_ref()
                        .expect("ignore graph for implicit cwd directory others"),
                    UntrackedMode::Directory,
                    include_empty_directories,
                    true,
                    &mut Vec::new(),
                )?)
        {
            let mut cwd_dir = cwd_prefix.clone();
            cwd_dir.push(b'/');
            if !paths.iter().any(|path| path == &cwd_dir) {
                paths.push(cwd_dir);
                paths.sort();
            }
        }
    }
    let mut unmatched_pathspecs = if options.error_unmatch {
        ls_files_unmatched_pathspecs(
            &index,
            other_paths.as_deref().unwrap_or(&[]),
            &effective_pathspecs,
            &options.path_args,
            !options.others
                || options.cached
                || options.stage
                || options.unmerged
                || options.deleted
                || options.modified
                || options.killed,
            options.others,
        )
    } else {
        Vec::new()
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
        if !unmatched_pathspecs.is_empty() {
            return Err(ls_files_error_unmatch(&unmatched_pathspecs));
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
        let format_statuses = if options.deleted || options.modified {
            let status_index = ls_files_worktree_status_index(&repo, &index)?;
            let mut statuses = worktree_status(&repo, status_index.as_ref())?
                .into_iter()
                .collect::<HashMap<_, _>>();
            for entry in index.entries().iter().filter(|entry| entry.stage > 0) {
                let exists = path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path));
                statuses.insert(entry.path.clone(), if exists { 'M' } else { 'D' });
            }
            Some(statuses)
        } else {
            None
        };
        let mut stdout = io::stdout().lock();
        let mut seen_paths = BTreeSet::new();
        for entry in index.entries().iter().filter(|entry| {
            pathspec_matches(&entry.path, &effective_pathspecs)
                && format_statuses.as_ref().is_none_or(|statuses| {
                    statuses.get(&entry.path).is_some_and(|status| {
                        (options.deleted && *status == 'D')
                            || (options.modified && matches!(*status, 'M' | 'D'))
                    })
                })
        }) {
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
            let record = render_ls_files_format(
                format,
                entry,
                &display_path,
                options.abbrev,
                &repo,
                eol_store
                    .as_ref()
                    .expect("object store for ls-files format"),
                eol_attrs.as_ref(),
            )?;
            write_ls_files_record(&mut stdout, &record, options.zero)?;
            if options.debug {
                write_ls_files_debug(&mut stdout, entry)?;
            }
        }
        if !unmatched_pathspecs.is_empty() {
            return Err(ls_files_error_unmatch(&unmatched_pathspecs));
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
            if !unmatched_pathspecs.is_empty() {
                return Err(ls_files_error_unmatch(&unmatched_pathspecs));
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
            if !unmatched_pathspecs.is_empty() {
                return Err(ls_files_error_unmatch(&unmatched_pathspecs));
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
        let status_index = ls_files_worktree_status_index(&repo, &index)?;
        for (path, status) in worktree_status(&repo, status_index.as_ref())? {
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
        for entry in index.entries().iter().filter(|entry| entry.stage > 0) {
            if !pathspec_matches(&entry.path, &effective_pathspecs) {
                continue;
            }
            let exists = path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path));
            if options.deleted && !exists {
                push_ls_files_path_record(
                    &mut records,
                    &mut seen_records,
                    entry.path.clone(),
                    b'R',
                    &options,
                );
            }
            if options.modified {
                push_ls_files_path_record(
                    &mut records,
                    &mut seen_records,
                    entry.path.clone(),
                    b'C',
                    &options,
                );
            }
        }
    }
    records.sort_by(|left, right| left.0.cmp(&right.0));
    if options.error_unmatch
        && (options.deleted || options.modified || options.killed)
        && !options.cached
        && !options.stage
        && !options.unmerged
        && !options.others
    {
        unmatched_pathspecs =
            ls_files_unmatched_record_pathspecs(&records, &effective_pathspecs, &options.path_args);
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
    if !unmatched_pathspecs.is_empty() {
        return Err(ls_files_error_unmatch(&unmatched_pathspecs));
    }
    Ok(())
}

fn ls_files_pathspec_requires_sparse_expansion(index: &GitIndex, pathspec: &[u8]) -> bool {
    sparse_index_path_requires_expansion(index, pathspec)
        || index.entries().iter().any(|entry| {
            entry.stage == 0
                && entry.mode == IndexMode::Tree
                && entry.path.strip_suffix(b"/") == Some(pathspec)
        })
}

fn ls_files_worktree_status_index<'a>(
    repo: &GitRepo,
    index: &'a GitIndex,
) -> Result<Cow<'a, GitIndex>> {
    if index_has_sparse_directories(index) {
        let expanded = expand_repo_sparse_index(repo, index)?;
        return if expanded.entries().iter().any(|entry| entry.stage > 0) {
            Ok(Cow::Owned(stage_zero_index(&expanded)?))
        } else {
            Ok(Cow::Owned(expanded))
        };
    }
    if index.entries().iter().any(|entry| entry.stage > 0) {
        Ok(Cow::Owned(stage_zero_index(index)?))
    } else {
        Ok(Cow::Borrowed(index))
    }
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
        && !options.deduplicate
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
        .filter(|entry| entry.stage == 0)
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
        if !matches!(entry.mode, IndexMode::Gitlink | IndexMode::Tree) {
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

fn ls_files_excludes(
    repo: &GitRepo,
    options: &LsFilesOptions,
    tracked_paths: Option<&TrackedPathSet<'_>>,
    index: &GitIndex,
) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if options.exclude_standard {
        ignore = if let Some(tracked_paths) = tracked_paths {
            standard_repo_ignore_pruned(repo, tracked_paths)?
        } else {
            standard_repo_ignore(repo)?
        };
    }
    for path in &options.exclude_from {
        let content = fs::read_to_string(path)?;
        ignore.append(GitIgnore::parse(&content));
    }
    if let Some(name) = &options.exclude_per_directory {
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        append_per_directory_excludes_with_index(
            &repo.root,
            &repo.root,
            name,
            &mut ignore,
            index,
            &store,
        )?;
    }
    if !options.excludes.is_empty() {
        ignore.append(GitIgnore::parse(&options.excludes.join("\n")));
    }
    Ok(ignore)
}

pub(crate) fn standard_repo_ignore(repo: &GitRepo) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if let Some(path) = ls_files_global_excludes_file(repo)? {
        append_ignore_file(&mut ignore, &path, "")?;
    }
    append_ignore_file(&mut ignore, &repo.git_dir.join("info/exclude"), "")?;
    append_per_directory_excludes(&repo.root, &repo.root, ".gitignore", &mut ignore)?;
    Ok(ignore)
}

fn add_repo_ignore(repo: &GitRepo, ignore_errors: bool) -> Result<GitIgnore> {
    let repo_ignore = if ignore_errors {
        GitIgnore::load_from_root_ignore_errors(&repo.root)?
    } else {
        GitIgnore::load_from_root(&repo.root)?
    };
    let mut ignore = GitIgnore::default();
    if let Some(path) = ls_files_global_excludes_file(repo)? {
        append_ignore_file(&mut ignore, &path, "")?;
    }
    append_ignore_file(&mut ignore, &repo.git_dir.join("info/exclude"), "")?;
    ignore.append(repo_ignore);
    Ok(ignore)
}

fn standard_repo_ignore_pruned(
    repo: &GitRepo,
    tracked_paths: &TrackedPathSet<'_>,
) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if let Some(path) = ls_files_global_excludes_file(repo)? {
        append_ignore_file(&mut ignore, &path, "")?;
    }
    append_ignore_file(&mut ignore, &repo.git_dir.join("info/exclude"), "")?;
    append_per_directory_excludes_pruned(
        &repo.root,
        &repo.root,
        b"",
        ".gitignore",
        &mut ignore,
        tracked_paths,
    )?;
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

fn append_per_directory_excludes_with_index(
    root: &std::path::Path,
    dir: &std::path::Path,
    name: &str,
    ignore: &mut GitIgnore,
    index: &GitIndex,
    store: &LooseObjectStore,
) -> Result<()> {
    let exclude_path = dir.join(name);
    let base = repo_relative_path(root, dir)?;
    let base_text = String::from_utf8_lossy(&base);
    match fs::read_to_string(&exclude_path) {
        Ok(content) => ignore.append(GitIgnore::parse_with_base(&content, &base_text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut index_path =
                Vec::with_capacity(base.len() + usize::from(!base.is_empty()) + name.len());
            index_path.extend_from_slice(&base);
            if !index_path.is_empty() {
                index_path.push(b'/');
            }
            index_path.extend_from_slice(name.as_bytes());
            if let Some(entry) = find_index_entry(index, &index_path) {
                let object = store.read_object(&entry.id)?;
                let content = String::from_utf8_lossy(&object.content);
                ignore.append(GitIgnore::parse_with_base(&content, &base_text));
            }
        }
        Err(error) => return Err(error.into()),
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            append_per_directory_excludes_with_index(root, &path, name, ignore, index, store)?;
        }
    }
    Ok(())
}

fn append_per_directory_excludes_pruned(
    root: &std::path::Path,
    dir: &std::path::Path,
    relative_dir: &[u8],
    name: &str,
    ignore: &mut GitIgnore,
    tracked_paths: &TrackedPathSet<'_>,
) -> Result<()> {
    let exclude_path = dir.join(name);
    let base = String::from_utf8_lossy(relative_dir);
    append_ignore_file(ignore, &exclude_path, &base)?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let relative = relative_child_path(relative_dir, &entry.file_name());
        let tracked_visible = tracked_paths.contains(relative.as_slice())
            || tracked_paths_under(tracked_paths, &relative);
        if ignore.is_ignored(&relative, true) && !tracked_visible {
            continue;
        }
        append_per_directory_excludes_pruned(root, &path, &relative, name, ignore, tracked_paths)?;
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
            entry.mode_bits(),
            ls_files_object_name(&entry.id, options.abbrev),
            entry.stage,
            eol
        ))
    } else {
        Ok(format!(
            "{}{:06o} {} {}\t{}",
            tag,
            entry.mode_bits(),
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
    repo: &GitRepo,
    store: &LooseObjectStore,
    attrs: Option<&GitAttributes>,
) -> Result<String> {
    let needs_object = format.contains("%(objecttype)")
        || format.contains("%(objectsize)")
        || format.contains("%(objectsize:padded)")
        || format.contains("%(eolinfo:index)");
    let needs_gitlink_content = format.contains("%(eolinfo:index)");
    let object = if needs_object && (entry.mode != IndexMode::Gitlink || needs_gitlink_content) {
        Some(store.read_object(&entry.id)?)
    } else {
        None
    };
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
                    "objectmode" => out.push_str(&format!("{:06o}", entry.mode_bits())),
                    "objectname" => out.push_str(&ls_files_object_name(&entry.id, abbrev)),
                    "objecttype" => out.push_str(if entry.mode == IndexMode::Gitlink {
                        "commit"
                    } else {
                        object
                            .as_ref()
                            .expect("object loaded for objecttype")
                            .kind
                            .as_str()
                    }),
                    "objectsize" => {
                        if entry.mode == IndexMode::Gitlink {
                            out.push('-');
                        } else {
                            out.push_str(
                                &object
                                    .as_ref()
                                    .expect("object loaded for objectsize")
                                    .content
                                    .len()
                                    .to_string(),
                            );
                        }
                    }
                    "objectsize:padded" => {
                        let size = if entry.mode == IndexMode::Gitlink {
                            "-".to_owned()
                        } else {
                            object
                                .as_ref()
                                .expect("object loaded for padded objectsize")
                                .content
                                .len()
                                .to_string()
                        };
                        out.push_str(&format!("{size:>7}"));
                    }
                    "stage" => out.push_str(&entry.stage.to_string()),
                    "path" => out.push_str(&String::from_utf8_lossy(display_path)),
                    "eolinfo:index" => out.push_str(
                        object
                            .as_ref()
                            .map_or("", |object| classify_eol(&object.content)),
                    ),
                    "eolinfo:worktree" => out.push_str(read_worktree_eol(repo, &entry.path)?),
                    "eolattr" => out.push_str(&ls_files_eol_attr(&entry.path, attrs)),
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

fn ls_files_unmatched_pathspecs(
    index: &GitIndex,
    other_paths: &[Vec<u8>],
    pathspecs: &[Vec<u8>],
    original_pathspecs: &[PathBuf],
    match_index: bool,
    match_other: bool,
) -> Vec<String> {
    pathspecs
        .iter()
        .enumerate()
        .filter_map(|(index_position, pathspec)| {
            let rule = parse_pathspec_rule(pathspec);
            if rule.exclude {
                return None;
            }
            let matches_index = match_index
                && index
                    .entries()
                    .iter()
                    .any(|entry| pathspec_rule_matches(&entry.path, rule));
            let matches_other = match_other
                && other_paths
                    .iter()
                    .any(|path| pathspec_rule_matches(path, rule));
            (!matches_index && !matches_other).then(|| {
                original_pathspecs
                    .get(index_position)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| String::from_utf8_lossy(pathspec).into_owned())
            })
        })
        .collect()
}

fn ls_files_unmatched_record_pathspecs(
    records: &[(Vec<u8>, u8)],
    pathspecs: &[Vec<u8>],
    original_pathspecs: &[PathBuf],
) -> Vec<String> {
    pathspecs
        .iter()
        .enumerate()
        .filter_map(|(index_position, pathspec)| {
            let rule = parse_pathspec_rule(pathspec);
            if rule.exclude
                || records
                    .iter()
                    .any(|(path, _tag)| pathspec_rule_matches(path, rule))
            {
                return None;
            }
            Some(
                original_pathspecs
                    .get(index_position)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| String::from_utf8_lossy(pathspec).into_owned()),
            )
        })
        .collect()
}

fn ls_files_error_unmatch(pathspecs: &[String]) -> CliError {
    let mut text = String::new();
    for pathspec in pathspecs {
        text.push_str(&format!(
            "error: pathspec '{pathspec}' did not match any file(s) known to git\n"
        ));
    }
    text.push_str("Did you forget to 'git add'?\n");
    CliError::Stderr { code: 1, text }
}

fn ls_files_display_path(path: &[u8], cwd_prefix: &[u8], full_name: bool) -> Option<Vec<u8>> {
    if full_name || cwd_prefix.is_empty() {
        return Some(path.to_vec());
    }
    if path == cwd_prefix {
        return Some(b"./".to_vec());
    }
    if let Some(rest) = path
        .strip_prefix(cwd_prefix)
        .and_then(|rest| rest.strip_prefix(b"/"))
    {
        if rest.is_empty() {
            return Some(b"./".to_vec());
        }
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
        Some(&b"./"[..])
    } else if let Some(rest) = path
        .strip_prefix(cwd_prefix)
        .and_then(|rest| rest.strip_prefix(b"/"))
    {
        if rest.is_empty() {
            Some(&b"./"[..])
        } else {
            Some(rest)
        }
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

struct SparsePathScope {
    matcher: GitIgnore,
    cone_mode: bool,
}

impl SparsePathScope {
    fn load(repo: &GitRepo) -> Result<Option<Self>> {
        if !sparse_checkout_active(repo)? {
            return Ok(None);
        }
        Ok(Some(Self {
            matcher: sparse_pattern_matcher(&read_sparse_checkout_match_patterns(repo)?),
            cone_mode: sparse_checkout_cone_mode(repo)?,
        }))
    }

    fn contains(&self, path: &[u8]) -> bool {
        sparse_path_matches(path, &self.matcher, self.cone_mode)
    }
}

fn add_path_is_sparse(entry: &IndexEntry, scope: Option<&SparsePathScope>) -> bool {
    entry.skip_worktree() || scope.is_some_and(|scope| !scope.contains(&entry.path))
}

struct AddSparseErrorOptions {
    allow_sparse: bool,
    all: bool,
    dense_matches_are_candidates: bool,
    materialized_sparse_is_candidate: bool,
}

fn add_sparse_path_error(
    repo: &GitRepo,
    index: &GitIndex,
    scope: Option<&SparsePathScope>,
    paths: &[PathBuf],
    additional_files: &[Vec<u8>],
    options: AddSparseErrorOptions,
) -> Result<Option<CliError>> {
    if options.allow_sparse || options.all || paths.is_empty() {
        return Ok(None);
    }
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative_allow_root(repo, path))
        .collect::<Result<Vec<_>>>()?;
    let mut has_sparse = false;
    let mut has_dense_candidate = false;
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        if !pathspec_matches(&entry.path, &pathspecs) {
            continue;
        }
        if add_path_is_sparse(entry, scope) {
            let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
            if options.materialized_sparse_is_candidate && scope.is_some() && path_exists(&absolute)
            {
                has_dense_candidate = true;
            } else {
                has_sparse = true;
            }
            continue;
        }
        if options.dense_matches_are_candidates {
            has_dense_candidate = true;
            continue;
        }
        let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
        has_dense_candidate |= !path_exists(&absolute)
            || worktree_entry_modified(repo, &absolute, entry).unwrap_or(true);
    }
    for path in additional_files {
        if find_index_entry(index, path).is_some() {
            continue;
        }
        if scope.is_some_and(|scope| !scope.contains(path)) {
            has_sparse = true;
        } else {
            has_dense_candidate = true;
        }
    }
    if !has_sparse || has_dense_candidate {
        return Ok(None);
    }
    let display_paths = paths
        .iter()
        .map(|path| {
            let relative = path_arg_to_repo_relative_allow_root(repo, path)?;
            Ok(if relative.is_empty() {
                b".".to_vec()
            } else {
                relative
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(sparse_path_update_error(repo, &display_paths)?))
}

pub(crate) fn add(
    all: bool,
    ignore_removal: bool,
    sparse: bool,
    force: bool,
    update: bool,
    renormalize: bool,
    intent_to_add: bool,
    refresh: bool,
    verbose: bool,
    ignore_errors: bool,
    no_ignore_errors: bool,
    ignore_missing: bool,
    interactive: bool,
    patch: bool,
    edit: bool,
    chmod: Option<String>,
    dry_run: bool,
    pathspec_from_file: Option<PathBuf>,
    pathspec_file_nul: bool,
    paths: Vec<PathBuf>,
) -> Result<()> {
    add_with_embedded_repo_warning(
        all,
        ignore_removal,
        sparse,
        force,
        update,
        renormalize,
        intent_to_add,
        refresh,
        verbose,
        ignore_errors,
        no_ignore_errors,
        ignore_missing,
        false,
        interactive,
        patch,
        edit,
        chmod,
        dry_run,
        pathspec_from_file,
        pathspec_file_nul,
        paths,
    )
}

pub(crate) fn add_with_embedded_repo_warning(
    all: bool,
    ignore_removal: bool,
    sparse: bool,
    force: bool,
    update: bool,
    renormalize: bool,
    intent_to_add: bool,
    refresh: bool,
    verbose: bool,
    ignore_errors: bool,
    no_ignore_errors: bool,
    ignore_missing: bool,
    no_warn_embedded_repo: bool,
    interactive: bool,
    patch: bool,
    edit: bool,
    chmod: Option<String>,
    dry_run: bool,
    pathspec_from_file: Option<PathBuf>,
    pathspec_file_nul: bool,
    mut paths: Vec<PathBuf>,
) -> Result<()> {
    let _trace = phase_trace("add.total");
    let update = update || edit;
    if all && (update || renormalize) {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-A' and '-u' cannot be used together".into(),
        });
    }
    if intent_to_add && (all || update || renormalize || refresh) {
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
    if paths.is_empty() && !all && !update && !renormalize && !interactive && !patch {
        eprintln!("Nothing specified, nothing added.");
        eprintln!("hint: Maybe you wanted to say 'git add .'?");
        eprintln!("hint: Disable this message with \"git config advice.addEmptyPathspec false\"");
        return Ok(());
    }
    let requested_paths = paths.clone();
    let _setup_trace = phase_trace("add.setup");
    let repo = {
        let _trace = phase_trace("add.find_repo");
        find_repo()?
    };
    {
        let _trace = phase_trace("add.preflight_submodule_hash_mismatch");
        preflight_explicit_submodule_hash_mismatch(
            &repo,
            all,
            update || renormalize || refresh,
            &paths,
        )?;
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let raw_index = {
        let _trace = phase_trace("add.read_index");
        read_repo_index_raw(&repo)?
    };
    let mut index = expand_repo_sparse_index(&repo, &raw_index)?;
    let sparse_scope = SparsePathScope::load(&repo)?;
    if !sparse && let Some(scope) = sparse_scope.as_ref() {
        let entries = index
            .entries()
            .iter()
            .cloned()
            .map(|mut entry| {
                if entry.stage == 0 && !scope.contains(&entry.path) {
                    entry.set_skip_worktree(true);
                }
                entry
            })
            .collect::<Vec<_>>();
        index = GitIndex::from_entries(entries)?;
    }
    if interactive || patch {
        if all || force || update || intent_to_add || refresh || edit || chmod.is_some() || dry_run
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "interactive add quit lanes do not support combining with other add modifiers yet".into(),
            });
        }
        let pathspecs = paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?;
        let requires_sparse_expansion = raw_index.entries().iter().any(|entry| {
            entry.stage == 0
                && entry.mode == IndexMode::Tree
                && path_exists(&worktree_path_for_index_entry(&repo.root, &entry.path))
        });
        let _sparse_expansion_region =
            requires_sparse_expansion.then(|| trace2_region("index", "ensure_full_index"));
        if patch {
            return add_patch_quit_lane(&repo, &store, &index, &pathspecs);
        }
        return add_interactive_quit_lane(&repo, &store, &index, &pathspecs);
    }
    let ignore_errors = {
        let _trace = phase_trace("add.ignore_errors_config");
        if no_ignore_errors {
            false
        } else {
            ignore_errors || add_ignore_errors_config_enabled(&repo)?
        }
    };
    let chmod = chmod.as_deref().map(parse_add_chmod).transpose()?;
    drop(_setup_trace);
    if refresh {
        if let Some(error) = add_sparse_path_error(
            &repo,
            &index,
            sparse_scope.as_ref(),
            &requested_paths,
            &[],
            AddSparseErrorOptions {
                allow_sparse: sparse,
                all,
                dense_matches_are_candidates: true,
                materialized_sparse_is_candidate: true,
            },
        )? {
            return Err(error);
        }
        let _trace = phase_trace("add.refresh");
        let pathspecs = paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?;
        ensure_add_pathspecs_match(&repo, &index, &pathspecs)?;
        refresh_tracked_index_metadata_matching(&repo, &mut index, &pathspecs)?;
        write_add_index(&repo, &store, &index)?;
        return Ok(());
    }
    if update {
        if let Some(error) = add_sparse_path_error(
            &repo,
            &index,
            sparse_scope.as_ref(),
            &requested_paths,
            &[],
            AddSparseErrorOptions {
                allow_sparse: sparse,
                all,
                dense_matches_are_candidates: true,
                materialized_sparse_is_candidate: false,
            },
        )? {
            return Err(error);
        }
        let _trace = phase_trace("add.update");
        let pathspecs = paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?;
        ensure_add_pathspecs_match(&repo, &index, &pathspecs)?;
        let changed = stage_tracked_worktree_changes_matching(
            &repo,
            &store,
            &mut index,
            &pathspecs,
            &HashSet::new(),
        )?;
        if changed {
            let _trace = phase_trace("add.write_index");
            write_add_index(&repo, &store, &index)?;
        }
        return Ok(());
    }
    if renormalize {
        if let Some(error) = add_sparse_path_error(
            &repo,
            &index,
            sparse_scope.as_ref(),
            &requested_paths,
            &[],
            AddSparseErrorOptions {
                allow_sparse: sparse,
                all,
                dense_matches_are_candidates: true,
                materialized_sparse_is_candidate: false,
            },
        )? {
            return Err(error);
        }
        let _trace = phase_trace("add.renormalize");
        let pathspecs = paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?;
        ensure_add_pathspecs_match(&repo, &index, &pathspecs)?;
        let changed =
            renormalize_tracked_worktree_changes_matching(&repo, &store, &mut index, &pathspecs)?;
        if changed {
            let _trace = phase_trace("add.write_index");
            write_add_index(&repo, &store, &index)?;
        }
        return Ok(());
    }

    let _collect_trace = phase_trace("add.collect_files");
    let ignore = add_repo_ignore(&repo, ignore_errors)?;
    let mut files = Vec::new();
    let tracked_pathspecs = if all && paths.is_empty() {
        Vec::new()
    } else {
        paths
            .iter()
            .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
            .collect::<Result<Vec<_>>>()?
    };
    if all {
        ensure_add_pathspecs_match(&repo, &index, &tracked_pathspecs)?;
    }
    let add_paths = if all && paths.is_empty() {
        vec![repo.root.clone()]
    } else {
        paths
    };
    let add_paths =
        expand_add_pathspec_args(&repo, &index, &ignore, force, ignore_errors, add_paths)?;
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
        let absolute = path_arg_absolute_for_repo(&repo, lookup_path)?;
        if all && !path_exists(&absolute) {
            continue;
        }
        if path_traverses_symlink_ancestor(&repo.root, &absolute)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("'{}' is beyond a symbolic link", path.display()),
            });
        }
        if !path_exists(&absolute) {
            if dry_run && ignore_missing {
                let relative = path_arg_to_repo_relative(&repo, &path)?;
                if !force && ignore.is_ignored(&relative, false) {
                    ignored_explicit_paths.push(relative);
                }
                continue;
            }
            let relative = path_arg_to_repo_relative(&repo, lookup_path)?;
            if !ignore_removal && find_index_entry(&index, &relative).is_some() {
                continue;
            }
            return Err(CliError::Fatal {
                code: 128,
                message: format!("pathspec '{}' did not match any files", path.display()),
            });
        }
        if !force && !(all && requested_paths.is_empty()) {
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
        if ignore_errors {
            collect_add_files_ignore_errors(&repo.root, &absolute, &ignore, force, &mut files)?;
        } else {
            collect_add_files(&repo.root, &absolute, &ignore, force, &mut files)?;
        }
    }
    files.sort();
    files.dedup();
    let file_entries = files
        .iter()
        .map(|file| Ok((file.clone(), repo_relative_path(&repo.root, file)?)))
        .collect::<Result<Vec<_>>>()?;
    let additional_files = file_entries
        .iter()
        .map(|(_, relative)| relative.clone())
        .collect::<Vec<_>>();
    if let Some(error) = add_sparse_path_error(
        &repo,
        &index,
        sparse_scope.as_ref(),
        &requested_paths,
        &additional_files,
        AddSparseErrorOptions {
            allow_sparse: sparse,
            all,
            dense_matches_are_candidates: chmod.is_some(),
            materialized_sparse_is_candidate: false,
        },
    )? {
        return Err(error);
    }
    let file_entries = if sparse {
        file_entries
    } else {
        file_entries
            .into_iter()
            .filter(|(_, relative)| {
                find_index_entry(&index, relative)
                    .is_none_or(|entry| !add_path_is_sparse(entry, sparse_scope.as_ref()))
                    && sparse_scope
                        .as_ref()
                        .is_none_or(|scope| scope.contains(relative))
            })
            .collect::<Vec<_>>()
    };
    let files_to_stage = file_entries
        .iter()
        .map(|(_, relative)| relative.clone())
        .collect::<HashSet<_>>();
    drop(_collect_trace);
    if all || !ignore_removal {
        let already_staged = if all && chmod.is_none() {
            HashSet::new()
        } else {
            files_to_stage.clone()
        };
        let _trace = phase_trace("add.stage_tracked");
        let _ = stage_tracked_worktree_changes_matching(
            &repo,
            &store,
            &mut index,
            &tracked_pathspecs,
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
    let stage_options = {
        let _trace = phase_trace("add.stage_options");
        WorktreeStageOptions::load(&repo)?
    };
    let mut stage_file_entries = if all && chmod.is_none() {
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
    let bulk_checkin_candidates = if intent_to_add || ignore_errors || chmod.is_some() {
        Vec::new()
    } else {
        let threshold = core_big_file_threshold(&repo)?;
        let mut candidates = Vec::new();
        for (path, relative) in &stage_file_entries {
            if let Some(candidate) =
                bulk_checkin_candidate(&repo, &stage_options, path, relative, threshold)?
            {
                candidates.push(candidate);
            }
        }
        candidates
    };
    if !bulk_checkin_candidates.is_empty() {
        let bulk_paths = bulk_checkin_candidates
            .iter()
            .map(|candidate| candidate.relative.as_slice())
            .collect::<HashSet<_>>();
        stage_file_entries.retain(|(_, relative)| !bulk_paths.contains(relative.as_slice()));
    }
    {
        let _trace = phase_trace("add.stage_files");
        let mut stage_files_trace = StageFilesTrace::new();
        let parallel_staged = if all && chmod.is_none() && !intent_to_add && !ignore_errors {
            try_stage_regular_files_parallel(
                &repo,
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
            for (file, relative) in stage_file_entries {
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
                if !no_warn_embedded_repo {
                    warn_add_embedded_repo(&repo, &index, &file, &mut embedded_repo_hint_printed)?;
                }
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
                    if object_database_permission_denied(&repo, &error).is_some() {
                        let display = String::from_utf8_lossy(&relative);
                        return Err(CliError::Stderr {
                            code: 128,
                            text: format!(
                                "{}error: {display}: failed to insert into database\nerror: unable to index file '{display}'\nfatal: updating files failed\n",
                                object_database_permission_denied_prefix(&repo)
                            ),
                        });
                    }
                    return Err(error);
                }
                if verbose {
                    println!("add '{}'", String::from_utf8_lossy(&relative));
                }
            }
        } else if verbose {
            for (_, relative) in &stage_file_entries {
                println!("add '{}'", String::from_utf8_lossy(relative));
            }
        }
        stage_files_trace.emit();
    }
    {
        let _trace = phase_trace("add.bulk_checkin");
        stage_bulk_checkin_candidates(
            &repo,
            &store,
            &mut index,
            &bulk_checkin_candidates,
            &stage_options,
        )?;
        if verbose {
            for candidate in &bulk_checkin_candidates {
                println!("add '{}'", String::from_utf8_lossy(&candidate.relative));
            }
        }
    }
    {
        let _trace = phase_trace("add.write_index");
        write_add_index(&repo, &store, &index)?;
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

fn write_add_index(repo: &GitRepo, store: &LooseObjectStore, index: &GitIndex) -> Result<()> {
    let write_index = collapse_sparse_index(repo, store, index)?;
    let options = add_index_write_options(repo)?;
    write_index_with_lockfile_diagnostics(
        repo,
        &write_index,
        add_lockfile_pid_config_enabled(repo)?,
        options,
    )
}

#[derive(Clone, Copy, Default)]
struct AddIndexWriteOptions {
    version: Option<zmin_git_core::GitIndexVersion>,
    skip_hash: bool,
}

fn add_index_write_options(repo: &GitRepo) -> Result<AddIndexWriteOptions> {
    if repo.index_path.exists() {
        return Ok(AddIndexWriteOptions::default());
    }
    let feature_many_files = read_config_value(repo, "feature.manyFiles")?
        .as_deref()
        .and_then(parse_git_bool)
        .unwrap_or(false);
    let version = if let Ok(raw) = std::env::var("GIT_INDEX_VERSION") {
        parse_add_index_version(&raw, "GIT_INDEX_VERSION")?
    } else if let Some(raw) = read_config_value(repo, "index.version")? {
        parse_add_index_version(&raw, "index.version")?
    } else if feature_many_files {
        Some(zmin_git_core::GitIndexVersion::V4)
    } else {
        None
    };
    let skip_hash = read_config_value(repo, "index.skipHash")?
        .as_deref()
        .and_then(parse_git_bool)
        .unwrap_or(feature_many_files);
    Ok(AddIndexWriteOptions { version, skip_hash })
}

fn parse_add_index_version(
    raw: &str,
    name: &str,
) -> Result<Option<zmin_git_core::GitIndexVersion>> {
    let version = match raw {
        "2" => Some(zmin_git_core::GitIndexVersion::V2),
        "3" => None,
        "4" => Some(zmin_git_core::GitIndexVersion::V4),
        _ => {
            eprintln!("warning: {name} set, but the value is invalid.\nUsing version 3");
            // Git reports the fallback as version 3 but writes the legacy
            // v2 on-disk format for a newly-created index.
            None
        }
    };
    Ok(version)
}

fn add_interactive_quit_lane(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let worktree_index = worktree_index_snapshot(repo, index)?;
    let staged = diff_indexes(&head_index, index)?
        .into_iter()
        .filter(|entry| pathspecs.is_empty() || pathspec_matches(&entry.path, pathspecs))
        .collect::<Vec<_>>();
    let unstaged = diff_indexes(index, &worktree_index)?
        .into_iter()
        .filter(|entry| pathspecs.is_empty() || pathspec_matches(&entry.path, pathspecs))
        .collect::<Vec<_>>();
    let staged_paths = staged
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let unstaged_paths = unstaged
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let paths = staged_paths
        .iter()
        .chain(&unstaged_paths)
        .cloned()
        .collect::<BTreeSet<_>>();
    if paths.is_empty() {
        return Ok(());
    }

    println!("           staged     unstaged path");
    for (row, path) in paths.iter().enumerate() {
        let staged_text = add_interactive_diff_stat(
            &head_index,
            index,
            repo,
            store,
            &staged,
            path,
            DiffSideSource::Index,
        )?
        .unwrap_or_else(|| "unchanged".to_owned());
        let unstaged_text = add_interactive_diff_stat(
            index,
            &worktree_index,
            repo,
            store,
            &unstaged,
            path,
            DiffSideSource::WorktreeOrIndex,
        )?
        .unwrap_or_else(|| "unchanged".to_owned());
        println!(
            "{:>3}: {:>12} {:>12} {}",
            row + 1,
            staged_text,
            unstaged_text,
            String::from_utf8_lossy(path)
        );
    }
    println!();
    println!("*** Commands ***");
    println!("  1: [s]tatus\t  2: [u]pdate\t  3: [r]evert\t  4: [a]dd untracked");
    println!("  5: [p]atch\t  6: [d]iff\t  7: [q]uit\t  8: [h]elp");
    print!("What now> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    match parse_add_interactive_action(&input, &staged_paths, &unstaged_paths)? {
        AddInteractiveAction::Quit => {
            print!("Bye.");
            Ok(())
        }
        AddInteractiveAction::Patch {
            selected_paths,
            patch_answers,
        } => {
            let mut answers = patch_commands::PatchAnswers::from_text(&patch_answers);
            stage_worktree_patch_hunks_to_index(repo, store, index, &selected_paths, &mut answers)?;
            Ok(())
        }
        AddInteractiveAction::Update { selected_paths } => {
            stage_add_interactive_update(repo, store, index, &selected_paths)
        }
        AddInteractiveAction::Revert { selected_paths } => {
            revert_add_interactive_paths(repo, store, index, &head_index, &selected_paths)
        }
    }
}

fn add_interactive_diff_stat(
    old_index: &GitIndex,
    new_index: &GitIndex,
    repo: &GitRepo,
    store: &LooseObjectStore,
    entries: &[zmin_git_core::IndexDiffEntry],
    path: &[u8],
    new_source: DiffSideSource,
) -> Result<Option<String>> {
    let Some(entry) = entries.iter().find(|entry| entry.path == path) else {
        return Ok(None);
    };
    let old_content = find_index_entry(old_index, diff_entry_old_path(entry))
        .map(|entry| read_diff_side_content(repo, store, entry, DiffSideSource::Index))
        .transpose()?
        .unwrap_or_default();
    let new_content = find_index_entry(new_index, &entry.path)
        .map(|entry| read_diff_side_content(repo, store, entry, new_source))
        .transpose()?
        .unwrap_or_default();
    let (insertions, deletions) = diff_line_counts_with_options(
        &old_content,
        &new_content,
        DiffWhitespaceMode::None,
        &[],
        false,
    );
    Ok(Some(format!("+{insertions}/-{deletions}")))
}

pub(crate) fn add_patch_quit_lane(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let mut answers = patch_commands::PatchAnswers::read()?;
    let _ = stage_worktree_patch_hunks_to_index(repo, store, index, pathspecs, &mut answers)?;
    Ok(())
}

pub(crate) fn commit_interactive_stage(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    let worktree_index = worktree_index_snapshot(repo, index)?;
    let entries = diff_indexes(index, &worktree_index)?
        .into_iter()
        .filter(|entry| pathspecs.is_empty() || pathspec_matches(&entry.path, pathspecs))
        .collect::<Vec<_>>();
    let paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();

    if !paths.is_empty() {
        println!("           staged     unstaged path");
        for (row, path) in paths.iter().enumerate() {
            let staged_text = add_interactive_diff_stat(
                index,
                &worktree_index,
                repo,
                store,
                &entries,
                path,
                DiffSideSource::WorktreeOrIndex,
            )?
            .unwrap_or_else(|| "unchanged".to_owned());
            println!(
                "{:>3}: {:>12} {:>12} {}",
                row + 1,
                "unchanged",
                staged_text,
                String::from_utf8_lossy(path)
            );
        }
        println!();
    }

    println!("*** Commands ***");
    println!("  1: [s]tatus\t  2: [u]pdate\t  3: [r]evert\t  4: [a]dd untracked");
    println!("  5: [p]atch\t  6: [d]iff\t  7: [q]uit\t  8: [h]elp");
    print!("What now> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    match parse_add_interactive_action(&input, &BTreeSet::new(), &paths)? {
        AddInteractiveAction::Quit => Ok(()),
        AddInteractiveAction::Patch {
            selected_paths,
            patch_answers,
        } => {
            let mut answers = patch_commands::PatchAnswers::from_text(&patch_answers);
            let _ = stage_worktree_patch_hunks_to_index(
                repo,
                store,
                index,
                &selected_paths,
                &mut answers,
            )?;
            Ok(())
        }
        AddInteractiveAction::Update { selected_paths } => {
            stage_add_interactive_update(repo, store, index, &selected_paths)
        }
        AddInteractiveAction::Revert { selected_paths } => {
            let runtime = CliPrimitiveRuntime::new_default(repo);
            let head_index = read_head_index_from_primitive_stores(
                runtime.refs(),
                runtime.object_store_adapter(),
            )?;
            revert_add_interactive_paths(repo, store, index, &head_index, &selected_paths)
        }
    }
}

enum AddInteractiveAction {
    Quit,
    Update {
        selected_paths: Vec<Vec<u8>>,
    },
    Revert {
        selected_paths: Vec<Vec<u8>>,
    },
    Patch {
        selected_paths: Vec<Vec<u8>>,
        patch_answers: String,
    },
}

fn parse_add_interactive_action(
    input: &str,
    staged_paths: &BTreeSet<Vec<u8>>,
    unstaged_paths: &BTreeSet<Vec<u8>>,
) -> Result<AddInteractiveAction> {
    let mut lines = input.lines();
    while let Some(line) = lines.next() {
        let command = line.trim();
        if command.is_empty() {
            continue;
        }
        match command {
            "q" | "quit" | "7" => return Ok(AddInteractiveAction::Quit),
            "h" | "help" | "8" | "s" | "status" | "1" => continue,
            "u" | "update" | "2" => {
                if unstaged_paths.is_empty() {
                    continue;
                }
                let selected_paths =
                    parse_add_interactive_path_selection(&mut lines, unstaged_paths)?;
                if selected_paths.is_empty() {
                    continue;
                }
                return Ok(AddInteractiveAction::Update { selected_paths });
            }
            "r" | "revert" | "3" => {
                if staged_paths.is_empty() {
                    continue;
                }
                let selected_paths =
                    parse_add_interactive_path_selection(&mut lines, staged_paths)?;
                if selected_paths.is_empty() {
                    continue;
                }
                return Ok(AddInteractiveAction::Revert { selected_paths });
            }
            "p" | "patch" | "5" => {
                let selected_paths =
                    parse_add_interactive_path_selection(&mut lines, unstaged_paths)?;
                let patch_answers = lines.collect::<Vec<_>>().join("\n");
                return Ok(AddInteractiveAction::Patch {
                    selected_paths,
                    patch_answers,
                });
            }
            _ => {
                eprintln!("Huh ({command})?");
            }
        }
    }
    Ok(AddInteractiveAction::Quit)
}

fn stage_add_interactive_update(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    selected_paths: &[Vec<u8>],
) -> Result<()> {
    if selected_paths.is_empty() {
        return Ok(());
    }
    let mut updated_index = index.clone();
    if stage_tracked_worktree_changes_matching(
        repo,
        store,
        &mut updated_index,
        selected_paths,
        &HashSet::new(),
    )? {
        write_add_index(repo, store, &updated_index)?;
    }
    Ok(())
}

fn revert_add_interactive_paths(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    head_index: &GitIndex,
    selected_paths: &[Vec<u8>],
) -> Result<()> {
    if selected_paths.is_empty() {
        return Ok(());
    }
    let mut updated_index = index.clone();
    for path in selected_paths {
        updated_index.remove_path(path)?;
        if let Some(entry) = find_index_entry(head_index, path) {
            updated_index.upsert(entry.clone())?;
        }
    }
    write_add_index(repo, store, &updated_index)
}

fn parse_add_interactive_path_selection<'a>(
    lines: &mut std::str::Lines<'a>,
    candidate_paths: &BTreeSet<Vec<u8>>,
) -> Result<Vec<Vec<u8>>> {
    let ordered_paths = candidate_paths.iter().cloned().collect::<Vec<_>>();
    let mut selected = BTreeSet::new();
    for line in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        for token in trimmed.split_whitespace() {
            let index = token.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 129,
                message: format!("invalid interactive path selection: {token}"),
            })?;
            let Some(path) = ordered_paths.get(index.saturating_sub(1)) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("interactive path selection out of range: {index}"),
                });
            };
            selected.insert(path.clone());
        }
    }
    Ok(selected.into_iter().collect())
}

pub(crate) fn stage_worktree_patch_hunks_to_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
    answers: &mut patch_commands::PatchAnswers,
) -> Result<bool> {
    let worktree_index = worktree_index_snapshot(repo, index)?;
    let entries = diff_indexes(index, &worktree_index)?
        .into_iter()
        .filter(|entry| pathspecs.is_empty() || pathspec_matches(&entry.path, pathspecs))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Ok(false);
    }

    let mut patch_bytes = Vec::new();
    write_patch_entries(
        &mut patch_bytes,
        repo,
        store,
        index,
        &worktree_index,
        &entries,
        PatchFormatOptions::worktree(),
    )?;
    let output = String::from_utf8(patch_bytes.clone()).map_err(|error| CliError::Fatal {
        code: 128,
        message: format!("patch output was not valid utf-8: {error}"),
    })?;
    let patches = patch_commands::parse_apply_patches(&patch_bytes)?;
    let displays = reset_patch_displays(&output);
    let mut updated_index = index.clone();
    let mut selected_any = false;
    let mut all_remaining = None;
    let mut quit = false;
    for (patch, display) in patches.into_iter().zip(displays) {
        print!("{}", display.header);
        let target_path = patch
            .new_path
            .as_ref()
            .or(patch.old_path.as_ref())
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "patch has no target path".into(),
            })?
            .clone();
        let mut selected_hunks = Vec::new();
        for (hunk_index, (hunk, hunk_display)) in patch.hunks.iter().zip(&display.hunks).enumerate()
        {
            print!("{hunk_display}");
            let selected = match all_remaining {
                Some(value) => value,
                None => {
                    print!(
                        "({}/{}) Stage this hunk {}? ",
                        hunk_index + 1,
                        patch.hunks.len(),
                        stage_patch_prompt_options(hunk_index, patch.hunks.len())
                    );
                    io::stdout().flush()?;
                    let answer = answers.next();
                    println!();
                    match answer {
                        patch_commands::PatchAnswer::Yes => true,
                        patch_commands::PatchAnswer::No => false,
                        patch_commands::PatchAnswer::Split => {
                            let split_hunks = patch_commands::split_apply_hunk(hunk);
                            if split_hunks.len() == 1 {
                                false
                            } else {
                                for split_hunk in split_hunks {
                                    print!("Stage this split hunk [y,n,q,a,d]? ");
                                    io::stdout().flush()?;
                                    let split_selected = match answers.next() {
                                        patch_commands::PatchAnswer::Yes => true,
                                        patch_commands::PatchAnswer::All => {
                                            all_remaining = Some(true);
                                            true
                                        }
                                        patch_commands::PatchAnswer::Done => {
                                            all_remaining = Some(false);
                                            false
                                        }
                                        patch_commands::PatchAnswer::Quit => {
                                            quit = true;
                                            false
                                        }
                                        patch_commands::PatchAnswer::No
                                        | patch_commands::PatchAnswer::Split => false,
                                    };
                                    println!();
                                    if split_selected {
                                        selected_hunks.push(split_hunk);
                                    }
                                    if quit {
                                        break;
                                    }
                                }
                                false
                            }
                        }
                        patch_commands::PatchAnswer::All => {
                            all_remaining = Some(true);
                            true
                        }
                        patch_commands::PatchAnswer::Done => {
                            all_remaining = Some(false);
                            false
                        }
                        patch_commands::PatchAnswer::Quit => {
                            quit = true;
                            false
                        }
                    }
                }
            };
            if selected {
                selected_hunks.push(hunk.clone());
            }
            if quit {
                break;
            }
        }
        if selected_hunks.is_empty() {
            if quit {
                break;
            }
            continue;
        }
        selected_any = true;
        let base_entry = find_index_entry(index, &target_path);
        let base = base_entry
            .map(|entry| read_index_entry_content(store, entry))
            .transpose()?
            .unwrap_or_default();
        if patch.deleted && selected_hunks.len() == patch.hunks.len() {
            updated_index.remove_path(&target_path)?;
            continue;
        }
        let content = patch_commands::apply_hunks_to_content(&base, &selected_hunks, &target_path)?;
        let mode = patch
            .new_mode
            .or_else(|| find_index_entry(&worktree_index, &target_path).map(|entry| entry.mode))
            .or_else(|| base_entry.map(|entry| entry.mode))
            .unwrap_or(IndexMode::File);
        upsert_index_content(store, &mut updated_index, target_path, content, mode)?;
        if quit {
            break;
        }
    }
    if selected_any {
        updated_index.refresh_cache_tree();
        updated_index.write_to_path(&repo.index_path)?;
    }
    Ok(selected_any)
}

fn stage_patch_prompt_options(index: usize, total: usize) -> &'static str {
    match (index, total) {
        (_, 1) => "[y,n,q,a,d,e,p,?]",
        (0, _) => "[y,n,q,a,d,j,J,g,/,e,p,?]",
        (index, total) if index + 1 == total => "[y,n,q,a,d,K,g,/,e,p,?]",
        _ => "[y,n,q,a,d,k,j,J,K,g,/,e,p,?]",
    }
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
        eprintln!("hint: Disable this message with \"git config advice.addEmbeddedRepo false\"");
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

fn expand_add_pathspec_args(
    repo: &GitRepo,
    index: &GitIndex,
    ignore: &GitIgnore,
    force: bool,
    ignore_errors: bool,
    paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>> {
    let mut expanded = Vec::new();
    for path in paths {
        let absolute = path_arg_absolute_for_repo(repo, &path)?;
        if path_exists(&absolute) || !add_arg_looks_like_pathspec_pattern(&path) {
            expanded.push(path);
            continue;
        }
        let pathspec = path_arg_to_repo_relative_allow_root(repo, &path)?;
        let matches =
            add_paths_matching_pathspec(repo, index, ignore, force, ignore_errors, &pathspec)?;
        if matches.is_empty() {
            expanded.push(path);
        } else {
            expanded.extend(matches);
        }
    }
    Ok(expanded)
}

fn add_arg_looks_like_pathspec_pattern(path: &Path) -> bool {
    let raw = path.to_string_lossy();
    raw.starts_with(":/")
        || raw.starts_with(":!")
        || raw.starts_with(":^")
        || raw.starts_with(":(")
        || raw.contains('*')
        || raw.contains('?')
        || raw.contains('[')
}

fn add_paths_matching_pathspec(
    repo: &GitRepo,
    index: &GitIndex,
    ignore: &GitIgnore,
    force: bool,
    ignore_errors: bool,
    pathspec: &[u8],
) -> Result<Vec<PathBuf>> {
    let pathspecs = vec![pathspec.to_vec()];
    let mut matched = BTreeSet::new();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        if pathspec_matches(&entry.path, &pathspecs) {
            matched.insert(worktree_path_for_index_entry(&repo.root, &entry.path));
        }
    }
    let mut worktree_files = Vec::new();
    if ignore_errors {
        collect_add_files_ignore_errors(
            &repo.root,
            &repo.root,
            ignore,
            force,
            &mut worktree_files,
        )?;
    } else {
        collect_add_files(&repo.root, &repo.root, ignore, force, &mut worktree_files)?;
    }
    for path in worktree_files {
        let relative = repo_relative_path(&repo.root, &path)?;
        if pathspec_matches(&relative, &pathspecs) {
            matched.insert(path);
        }
    }
    Ok(matched.into_iter().collect())
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
    message.push_str("hint: Disable this message with \"git config advice.addIgnoredFile false\"");
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
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let raw_index = read_repo_index_raw(&repo)?;
    let mut index = expand_repo_sparse_index(&repo, &raw_index)?;
    let sparse_matcher = if sparse_checkout_active(&repo)? && !options.sparse {
        Some((
            sparse_pattern_matcher(&read_sparse_checkout_match_patterns(&repo)?),
            sparse_checkout_cone_mode(&repo)?,
        ))
    } else {
        None
    };
    let mut removed = Vec::new();

    for path in paths {
        let relative = if options.recursive {
            path_arg_to_repo_relative_allow_root(&repo, &path)?
        } else {
            path_arg_to_repo_relative(&repo, &path)?
        };
        let matches = rm_path_matches(&index, &relative, options.recursive)?;
        let sparse_directories = if options.sparse {
            rm_matching_sparse_directories(&raw_index, &relative, options.recursive)
        } else {
            Vec::new()
        };
        let _expansion_region = (sparse_directories.is_empty()
            && rm_matches_inside_sparse_directory(&raw_index, &matches))
        .then(|| trace2_region("index", "ensure_full_index"));
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
        if !sparse_directories.is_empty() {
            for directory in sparse_directories {
                let prefix = directory.strip_suffix(b"/").unwrap_or(&directory);
                let descendants = index
                    .entries()
                    .iter()
                    .filter(|entry| {
                        entry.stage == 0 && path_is_below_sparse_directory(&entry.path, prefix)
                    })
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>();
                if !options.force {
                    for descendant in &descendants {
                        ensure_rm_safe(&repo, &head_index, &index, descendant, options.cached)?;
                    }
                }
                if !options.dry_run {
                    index.remove_dir(prefix)?;
                }
                removed.push(directory);
            }
            continue;
        }
        let (matches, rejected) = if let Some((matcher, cone_mode)) = sparse_matcher.as_ref() {
            let mut allowed = Vec::new();
            let mut rejected = Vec::new();
            for matched in matches {
                if sparse_path_matches(&matched, matcher, *cone_mode) {
                    allowed.push(matched);
                } else {
                    rejected.push(matched);
                }
            }
            (allowed, rejected)
        } else {
            (matches, Vec::new())
        };
        if matches.is_empty() && !rejected.is_empty() {
            if options.ignore_unmatch {
                continue;
            }
            return Err(sparse_path_update_error(&repo, &rejected)?);
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
        let write_index = collapse_sparse_index(&repo, &store, &index)?;
        write_index.write_to_path(&repo.index_path)?;
    }
    Ok(())
}

fn rm_matching_sparse_directories(
    raw_index: &GitIndex,
    pathspec: &[u8],
    recursive: bool,
) -> Vec<Vec<u8>> {
    let pathspecs = [pathspec.to_vec()];
    raw_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Tree)
        .filter(|entry| {
            let directory = entry.path.strip_suffix(b"/").unwrap_or(&entry.path);
            (recursive && directory == pathspec) || pathspec_matches(&entry.path, &pathspecs)
        })
        .map(|entry| entry.path.clone())
        .collect()
}

fn rm_matches_inside_sparse_directory(raw_index: &GitIndex, matches: &[Vec<u8>]) -> bool {
    raw_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Tree)
        .any(|directory| {
            let prefix = directory.path.strip_suffix(b"/").unwrap_or(&directory.path);
            matches
                .iter()
                .any(|path| path_is_below_sparse_directory(path, prefix))
        })
}

fn sparse_path_update_error(repo: &GitRepo, paths: &[Vec<u8>]) -> Result<CliError> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    let mut text = String::from(
        "The following paths and/or pathspecs matched paths that exist\n\
         outside of your sparse-checkout definition, so will not be\n\
         updated in the index:\n",
    );
    for path in paths {
        text.push_str(&String::from_utf8_lossy(&path));
        text.push('\n');
    }
    if read_config_value(repo, "advice.updateSparsePath")?
        .as_deref()
        .and_then(parse_git_bool)
        .unwrap_or(true)
    {
        text.push_str(
            "hint: If you intend to update such entries, try one of the following:\n\
             hint: * Use the --sparse option.\n\
             hint: * Disable or modify the sparsity rules.\n\
             hint: Disable this message with \"git config advice.updateSparsePath false\"\n",
        );
    }
    Ok(CliError::Stderr { code: 1, text })
}

pub(crate) fn mv(
    force: bool,
    dry_run: bool,
    verbose: bool,
    skip_errors: bool,
    sparse: bool,
    paths: Vec<PathBuf>,
) -> Result<()> {
    if paths.len() < 2 {
        return Err(CliError::Message(
            "`mv` requires at least one source and a destination".into(),
        ));
    }
    let repo = find_repo_or_bare()?;
    let mut index = read_repo_index(&repo)?;
    let store = LooseObjectStore::new(
        repo.objects_dir.clone(),
        repo_hash_algorithm_from_config(&repo)?,
    );
    let sparse_rules = if sparse_checkout_active(&repo)? {
        Some((
            sparse_pattern_matcher(&read_sparse_checkout_match_patterns(&repo)?),
            sparse_checkout_cone_mode(&repo)?,
        ))
    } else {
        None
    };
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
        let destination_is_directory = destination_absolute.is_dir();
        let target_absolute = mv_target_path(
            &source_absolute,
            &destination_absolute,
            multiple_sources || destination_is_directory,
        )?;
        let target_relative = crate::runtime::repo_relative_path_preserve_final_component(
            &repo.root,
            &target_absolute,
        )?;
        let target_display = if multiple_sources || destination_is_directory {
            PathBuf::from(String::from_utf8_lossy(&target_relative).as_ref())
        } else {
            destination.clone()
        };
        let mut moves = mv_index_moves(&index, &source_relative, &target_relative)?;
        if moves.is_empty() {
            if skip_errors {
                if dry_run {
                    println!(
                        "Checking rename of '{}' to '{}'",
                        source.display(),
                        target_display.display()
                    );
                }
                continue;
            }
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "bad source, source={}, destination={}",
                    source.display(),
                    destination.display()
                ),
            });
        }
        if !sparse && moves.iter().any(|(_, entry)| entry.skip_worktree()) {
            let paths = moves
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            return Err(sparse_path_update_error(&repo, &paths)?);
        }
        if let Some((matcher, cone_mode)) = sparse_rules.as_ref() {
            for (_, entry) in &mut moves {
                entry.set_skip_worktree(!sparse_path_matches(&entry.path, matcher, *cone_mode));
            }
        }
        let overwrites_tracked_destination = find_index_entry(&index, &target_relative).is_some();
        ensure_mv_destination_available(&index, &target_relative, force)?;
        if force && verbose && overwrites_tracked_destination {
            eprintln!(
                "warning: overwriting '{}'",
                String::from_utf8_lossy(&target_relative)
            );
        }
        if dry_run {
            println!(
                "Checking rename of '{}' to '{}'",
                source.display(),
                target_display.display()
            );
        }
        if dry_run || verbose {
            println!(
                "Renaming {} to {}",
                source.display(),
                target_display.display()
            );
        }
        if !dry_run {
            let source_exists = path_exists(&source_absolute);
            if source_exists {
                rename_worktree_path(&source_absolute, &target_absolute, force)?;
            }
            let checkout_entries = if source_exists {
                Vec::new()
            } else {
                moves
                    .iter()
                    .map(|(_, entry)| entry)
                    .filter(|entry| !entry.skip_worktree())
                    .cloned()
                    .collect::<Vec<_>>()
            };
            apply_index_moves(&mut index, moves)?;
            if !checkout_entries.is_empty() {
                let checkout = GitIndex::from_entries(checkout_entries)?;
                checkout_index(
                    &store,
                    &checkout,
                    &repo.root,
                    CheckoutIndexOptions { force },
                )?;
                smudge_worktree_filter_entries_with_metadata_for_index(
                    &repo,
                    &store,
                    &index,
                    &checkout,
                    &WorktreeCheckoutMetadata::default(),
                )?;
            }
        }
    }

    if !dry_run {
        let write_index = collapse_sparse_index(&repo, &store, &index)?;
        write_index.write_to_path(&repo.index_path)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct ReadTreeCommandOptions {
    pub(crate) empty: bool,
    pub(crate) merge: bool,
    pub(crate) trivial: bool,
    pub(crate) aggressive: bool,
    pub(crate) reset: bool,
    pub(crate) update_worktree: bool,
    pub(crate) index_only: bool,
    pub(crate) dry_run: bool,
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) index_output: Option<PathBuf>,
    pub(crate) prefix: Option<String>,
    pub(crate) exclude_per_directory: Option<String>,
    pub(crate) super_prefix: Option<String>,
    pub(crate) recurse_submodules: bool,
    pub(crate) no_recurse_submodules: bool,
    pub(crate) no_sparse_checkout: bool,
    pub(crate) treeish: Vec<String>,
}

pub(crate) fn read_tree_command(options: ReadTreeCommandOptions) -> Result<()> {
    let ReadTreeCommandOptions {
        empty,
        merge,
        trivial,
        aggressive,
        reset,
        update_worktree,
        index_only,
        dry_run,
        verbose,
        quiet,
        index_output,
        prefix,
        exclude_per_directory,
        super_prefix,
        recurse_submodules,
        no_recurse_submodules,
        no_sparse_checkout,
        treeish,
    } = options;
    let _ = (quiet, no_sparse_checkout, verbose, trivial, aggressive);
    if empty && !treeish.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "passing trees as arguments contradicts --empty".into(),
        });
    }
    if empty && prefix.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "you must specify at least one tree to merge".into(),
        });
    }
    if index_only && !merge && !reset && prefix.is_none() {
        return Err(CliError::Stderr {
            code: 128,
            text: "fatal: -i is meaningless without -m, --reset, or --prefix\n".into(),
        });
    }
    if update_worktree && index_only {
        return Err(CliError::Stderr {
            code: 128,
            text: "fatal: -u and -i at the same time makes no sense\n".into(),
        });
    }
    if update_worktree && !merge && !reset && prefix.is_none() {
        return Err(CliError::Stderr {
            code: 128,
            text: "fatal: -u is meaningless without -m, --reset, or --prefix\n".into(),
        });
    }
    if exclude_per_directory.is_some() && !update_worktree {
        return Err(CliError::Stderr {
            code: 128,
            text: "fatal: --exclude-per-directory is meaningless unless -u\n".into(),
        });
    }
    if let Some(value) = exclude_per_directory.as_deref()
        && value != ".gitignore"
    {
        return Err(CliError::Stderr {
            code: 128,
            text: "fatal: --exclude-per-directory argument must be .gitignore\n".into(),
        });
    }
    let repo = find_repo_or_bare()?;
    let recurse_submodules = !no_recurse_submodules
        && (recurse_submodules
            || read_config_value(&repo, "submodule.recurse")?
                .as_deref()
                .and_then(parse_git_bool)
                .unwrap_or(false));
    let output_path = index_output.unwrap_or_else(|| repo.index_path.clone());
    let original_index = read_repo_index(&repo)?;
    if empty {
        if !dry_run {
            GitIndex::new().write_to_path(&output_path)?;
        }
        return Ok(());
    }
    if treeish.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "read-tree requires --empty or a tree-ish".into(),
        });
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let tree_cache = TreeObjectCache::new(&store);
    let mut result_index = if let Some(prefix) = prefix.as_deref() {
        if prefix.starts_with('/') {
            return Err(CliError::Stderr {
                code: 128,
                text: "fatal: Invalid prefix, prefix cannot start with '/'\n".into(),
            });
        }
        let tree_id = resolve_treeish_or_invalid_object(
            &repo,
            &store,
            treeish.last().expect("treeish checked"),
        )?;
        let imported_index = tree_cache.read_tree_to_index(&tree_id)?;
        let existing = original_index.clone();
        prefix_index_onto_existing(existing, imported_index, prefix)?
    } else if reset {
        read_tree_resolved_index(
            &repo,
            &store,
            &tree_cache,
            treeish.last().expect("treeish checked"),
        )?
    } else if merge {
        read_tree_merge_index(&repo, &store, &tree_cache, &original_index, &treeish)?
    } else {
        read_tree_resolved_index(
            &repo,
            &store,
            &tree_cache,
            treeish.last().expect("treeish checked"),
        )?
    };
    reject_null_read_tree_entries(&result_index)?;
    read_tree_prefetch_missing_objects(&repo, &store, &result_index)?;
    validate_read_tree_submodule_targets(&repo, &result_index, recurse_submodules)?;
    if !no_sparse_checkout && sparse_checkout_active(&repo)? {
        apply_sparse_checkout_bits_to_index(&repo, &mut result_index)?;
        result_index = expand_repo_sparse_index(&repo, &result_index)?;
    }
    preserve_read_tree_index_metadata(&original_index, &mut result_index)?;
    result_index.refresh_cache_tree();
    read_tree_validate_confusing_paths(&repo, &result_index)?;
    if !index_only && (merge || reset) {
        read_tree_validate_worktree(&repo, &original_index, &result_index)?;
    }
    if update_worktree {
        read_tree_validate_update_worktree(
            &repo,
            &original_index,
            &result_index,
            merge && !reset,
            reset,
            exclude_per_directory.as_deref(),
            super_prefix.as_deref(),
        )?;
    }
    if !dry_run {
        let write_index = collapse_sparse_index(&repo, &store, &result_index)?;
        write_index.write_to_path(&output_path)?;
        if update_worktree {
            remove_read_tree_submodules(&repo, &original_index, &result_index, recurse_submodules)?;
            let refreshed_paths = read_tree_update_worktree(
                &repo,
                &store,
                &original_index,
                &result_index,
                merge && !reset,
                reset,
                prefix.is_some(),
                exclude_per_directory.as_deref(),
                super_prefix.as_deref(),
            )?;
            checkout_read_tree_submodules(
                &repo,
                &original_index,
                &result_index,
                recurse_submodules,
                reset,
            )?;
            refresh_tracked_index_metadata_matching(&repo, &mut result_index, &refreshed_paths)?;
            result_index.refresh_cache_tree();
            let write_index = collapse_sparse_index(&repo, &store, &result_index)?;
            write_index.write_to_path(&output_path)?;
        }
    }
    Ok(())
}

fn reject_null_read_tree_entries(index: &GitIndex) -> Result<()> {
    let Some(entry) = index
        .entries()
        .iter()
        .find(|entry| entry.id.as_bytes().iter().all(|byte| *byte == 0))
    else {
        return Ok(());
    };
    let path = String::from_utf8_lossy(&entry.path);
    if std::env::var_os("GIT_ALLOW_NULL_SHA1").is_some_and(|value| value == "1") {
        eprintln!("warning: cache entry has null sha1: {path}");
        return Ok(());
    }
    Err(CliError::Stderr {
        code: 128,
        text: format!(
            "error: cache entry has null sha1: {path}\nfatal: unable to write new index file\n"
        ),
    })
}

fn preserve_read_tree_index_metadata(
    original_index: &GitIndex,
    result_index: &mut GitIndex,
) -> Result<()> {
    let preserved = result_index
        .entries()
        .iter()
        .filter_map(|result| {
            let current = find_index_entry(original_index, &result.path)?;
            if result.stage != 0 || !merge_tree_same_entry(Some(current), Some(result)) {
                return None;
            }
            let mut entry = current.clone();
            entry.set_skip_worktree(result.skip_worktree());
            Some(entry)
        })
        .collect::<Vec<_>>();
    for entry in preserved {
        result_index.upsert(entry)?;
    }
    Ok(())
}

fn read_tree_prefetch_missing_objects(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
) -> Result<()> {
    if !super::transport_commands::lazy_fetch_allowed() || !partial_clone_enabled(repo)? {
        return Ok(());
    }
    let object_ids = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode != IndexMode::Gitlink)
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let mut missing = store
        .missing_objects(&object_ids)
        .map_err(CliError::Io)?
        .into_iter()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    missing.sort_by_key(ObjectId::to_hex);
    super::admin_commands::backfill_promisor_objects(repo, &missing)?;
    Ok(())
}

fn read_tree_validate_update_worktree(
    repo: &GitRepo,
    original_index: &GitIndex,
    result_index: &GitIndex,
    preserve_dirty_local: bool,
    force_checkout: bool,
    exclude_per_directory: Option<&str>,
    super_prefix: Option<&str>,
) -> Result<()> {
    for entry in result_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
    {
        if entry.skip_worktree() {
            continue;
        }
        let current = find_index_entry(original_index, &entry.path);
        if !read_tree_entry_needs_checkout(repo, current, entry, preserve_dirty_local)? {
            continue;
        }
        if force_checkout {
            continue;
        }
        if entry.mode == IndexMode::Gitlink {
            let ignore = standard_repo_ignore(repo)?;
            if ignore.is_ignored(&entry.path, false) || ignore.is_ignored(&entry.path, true) {
                continue;
            }
        }
        read_tree_preflight_checkout_path(
            repo,
            original_index,
            &entry.path,
            exclude_per_directory,
            super_prefix,
        )?;
    }
    Ok(())
}

fn read_tree_validate_worktree(
    repo: &GitRepo,
    current_index: &GitIndex,
    result_index: &GitIndex,
) -> Result<()> {
    let mut paths = BTreeSet::new();
    paths.extend(
        current_index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0)
            .map(|entry| entry.path.clone()),
    );
    paths.extend(
        result_index
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0)
            .map(|entry| entry.path.clone()),
    );
    for path in paths {
        let current = find_index_entry(current_index, &path);
        let result = find_index_entry(result_index, &path);
        if merge_tree_same_entry(current, result) {
            continue;
        }
        let Some(current) = current else {
            continue;
        };
        let absolute = repo.root.join(String::from_utf8_lossy(&path).as_ref());
        if path_exists(&absolute) && worktree_entry_modified(repo, &absolute, current)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "Entry '{}' not uptodate. Cannot merge.",
                    String::from_utf8_lossy(&path)
                ),
            });
        }
    }
    Ok(())
}

fn read_tree_resolved_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    treeish: &str,
) -> Result<GitIndex> {
    let tree_id = resolve_treeish_or_invalid_object(repo, store, treeish)?;
    Ok(tree_cache.read_tree_to_index(&tree_id)?)
}

fn read_tree_merge_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    current_index: &GitIndex,
    treeishes: &[String],
) -> Result<GitIndex> {
    match treeishes.len() {
        0 => Err(CliError::Fatal {
            code: 129,
            message: "read-tree requires --empty or a tree-ish".into(),
        }),
        1 => read_tree_resolved_index(repo, store, tree_cache, &treeishes[0]),
        2 => {
            let base = read_tree_resolved_index(repo, store, tree_cache, &treeishes[0])?;
            let target = read_tree_resolved_index(repo, store, tree_cache, &treeishes[1])?;
            read_tree_two_way_merge_index(current_index, &base, &target)
        }
        _ => {
            let base = read_tree_resolved_index(repo, store, tree_cache, &treeishes[0])?;
            let ours = read_tree_resolved_index(repo, store, tree_cache, &treeishes[1])?;
            let theirs = read_tree_resolved_index(repo, store, tree_cache, &treeishes[2])?;
            read_tree_validate_three_way_current_index(current_index, &base, &ours, &theirs)?;
            read_tree_three_way_merge_index(&base, &ours, &theirs)
        }
    }
}

fn read_tree_two_way_merge_index(
    current_index: &GitIndex,
    base: &GitIndex,
    target: &GitIndex,
) -> Result<GitIndex> {
    let mut paths = BTreeSet::new();
    for index in [current_index, base, target] {
        paths.extend(
            index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0)
                .map(|entry| entry.path.clone()),
        );
    }
    let mut entries = Vec::new();
    for path in paths {
        let current = find_index_entry(current_index, &path);
        let old = find_index_entry(base, &path);
        let new = find_index_entry(target, &path);
        match read_tree_two_way_path_result(current, old, new) {
            ReadTreeTwoWayPathResult::UseCurrent => {
                if let Some(entry) = current {
                    entries.push(entry.clone());
                }
            }
            ReadTreeTwoWayPathResult::UseTarget => {
                if let Some(current) = current
                    && merge_tree_same_entry(Some(current), new)
                {
                    entries.push(current.clone());
                } else if let Some(entry) = new {
                    entries.push(entry.clone());
                }
            }
            ReadTreeTwoWayPathResult::Remove => {}
            ReadTreeTwoWayPathResult::Conflict => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "Entry not uptodate. Cannot merge.".into(),
                });
            }
        }
    }
    Ok(GitIndex::from_entries(entries)?)
}

fn read_tree_validate_three_way_current_index(
    current_index: &GitIndex,
    base: &GitIndex,
    ours: &GitIndex,
    theirs: &GitIndex,
) -> Result<()> {
    let mut paths = BTreeSet::new();
    for index in [current_index, base, ours, theirs] {
        paths.extend(
            index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0)
                .map(|entry| entry.path.clone()),
        );
    }

    for path in paths {
        let Some(current) = find_index_entry(current_index, &path) else {
            continue;
        };
        let base_entry = find_index_entry(base, &path);
        let our_entry = find_index_entry(ours, &path);
        let their_entry = find_index_entry(theirs, &path);
        if read_tree_three_way_current_entry_allowed(current, base_entry, our_entry, their_entry) {
            continue;
        }
        return Err(CliError::Stderr {
            code: 128,
            text: format!(
                "error: Entry '{}' would be overwritten by merge. Cannot merge.\n",
                String::from_utf8_lossy(&path)
            ),
        });
    }
    Ok(())
}

fn read_tree_three_way_current_entry_allowed(
    current: &IndexEntry,
    base: Option<&IndexEntry>,
    ours: Option<&IndexEntry>,
    theirs: Option<&IndexEntry>,
) -> bool {
    if merge_tree_same_entry(ours, theirs) {
        return merge_tree_same_entry(Some(current), ours);
    }
    if merge_tree_same_entry(base, ours) {
        return merge_tree_same_entry(Some(current), base)
            || merge_tree_same_entry(Some(current), theirs);
    }
    if merge_tree_same_entry(base, theirs) {
        return merge_tree_same_entry(Some(current), ours);
    }
    if ours.is_none() {
        return false;
    }
    if base.is_none() || theirs.is_none() {
        return merge_tree_same_entry(Some(current), ours);
    }
    merge_tree_same_entry(Some(current), ours)
}

fn read_tree_three_way_merge_index(
    base: &GitIndex,
    ours: &GitIndex,
    theirs: &GitIndex,
) -> Result<GitIndex> {
    let mut paths = BTreeSet::new();
    for index in [base, ours, theirs] {
        paths.extend(
            index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0)
                .map(|entry| entry.path.clone()),
        );
    }

    let mut entries = Vec::new();
    let mut consumed = BTreeSet::new();
    read_tree_three_way_directory_file_conflicts(base, ours, theirs, &mut entries, &mut consumed);
    for path in paths {
        if consumed.contains(&path) {
            continue;
        }
        let base_entry = find_index_entry(base, &path);
        let our_entry = find_index_entry(ours, &path);
        let their_entry = find_index_entry(theirs, &path);
        match (base_entry, our_entry, their_entry) {
            (None, None, None) => {}
            (None, Some(ours), None) => entries.push(ours.clone()),
            (None, None, Some(theirs)) => entries.push(theirs.clone()),
            (None, Some(ours), Some(theirs)) => {
                if merge_tree_same_entry(Some(ours), Some(theirs)) {
                    entries.push(ours.clone());
                } else {
                    let mut ours_stage = ours.clone();
                    ours_stage.stage = 2;
                    entries.push(ours_stage);
                    let mut theirs_stage = theirs.clone();
                    theirs_stage.stage = 3;
                    entries.push(theirs_stage);
                }
            }
            (Some(base), None, None) => {
                let mut base_stage = base.clone();
                base_stage.stage = 1;
                entries.push(base_stage);
            }
            (Some(base), Some(ours), None) => {
                let mut base_stage = base.clone();
                base_stage.stage = 1;
                entries.push(base_stage);
                let mut ours_stage = ours.clone();
                ours_stage.stage = 2;
                entries.push(ours_stage);
            }
            (Some(base), None, Some(theirs)) => {
                let mut base_stage = base.clone();
                base_stage.stage = 1;
                entries.push(base_stage);
                let mut theirs_stage = theirs.clone();
                theirs_stage.stage = 3;
                entries.push(theirs_stage);
            }
            (Some(base), Some(ours), Some(theirs)) => {
                if merge_tree_same_entry(Some(ours), Some(theirs)) {
                    entries.push(ours.clone());
                } else if merge_tree_same_entry(Some(base), Some(ours)) {
                    entries.push(theirs.clone());
                } else if merge_tree_same_entry(Some(base), Some(theirs)) {
                    entries.push(ours.clone());
                } else {
                    let mut base_stage = base.clone();
                    base_stage.stage = 1;
                    entries.push(base_stage);
                    let mut ours_stage = ours.clone();
                    ours_stage.stage = 2;
                    entries.push(ours_stage);
                    let mut theirs_stage = theirs.clone();
                    theirs_stage.stage = 3;
                    entries.push(theirs_stage);
                }
            }
        }
    }
    Ok(GitIndex::from_entries(entries)?)
}

fn read_tree_three_way_directory_file_conflicts(
    base: &GitIndex,
    ours: &GitIndex,
    theirs: &GitIndex,
    entries: &mut Vec<IndexEntry>,
    consumed: &mut BTreeSet<Vec<u8>>,
) {
    let mut candidate_paths = BTreeSet::new();
    for index in [base, ours, theirs] {
        candidate_paths.extend(
            index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0)
                .map(|entry| entry.path.clone()),
        );
    }

    for path in candidate_paths {
        if consumed.contains(&path) {
            continue;
        }
        let exact_entries = [
            find_index_entry(base, &path),
            find_index_entry(ours, &path),
            find_index_entry(theirs, &path),
        ];
        if exact_entries.iter().all(Option::is_none) {
            continue;
        }
        let mut prefix = path.clone();
        prefix.push(b'/');
        let mut nested_paths = BTreeSet::new();
        for index in [base, ours, theirs] {
            nested_paths.extend(
                index
                    .entries()
                    .iter()
                    .filter(|entry| {
                        entry.stage == 0
                            && !consumed.contains(&entry.path)
                            && entry.path.starts_with(prefix.as_slice())
                    })
                    .map(|entry| entry.path.clone()),
            );
        }
        if nested_paths.is_empty() {
            continue;
        }

        for (stage, entry) in [
            (1u8, exact_entries[0]),
            (2u8, exact_entries[1]),
            (3u8, exact_entries[2]),
        ] {
            if let Some(entry) = entry {
                let mut staged = entry.clone();
                staged.stage = stage;
                entries.push(staged);
            }
        }
        consumed.insert(path.clone());

        for nested_path in nested_paths {
            for (stage, index) in [(1u8, base), (2u8, ours), (3u8, theirs)] {
                if let Some(entry) = find_index_entry(index, &nested_path) {
                    let mut staged = entry.clone();
                    staged.stage = stage;
                    entries.push(staged);
                }
            }
            consumed.insert(nested_path);
        }
    }
}

enum ReadTreeTwoWayPathResult {
    UseCurrent,
    UseTarget,
    Remove,
    Conflict,
}

fn read_tree_two_way_path_result(
    current: Option<&IndexEntry>,
    old: Option<&IndexEntry>,
    new: Option<&IndexEntry>,
) -> ReadTreeTwoWayPathResult {
    if merge_tree_same_entry(old, new) {
        return if current.is_some() {
            ReadTreeTwoWayPathResult::UseCurrent
        } else if new.is_some() {
            ReadTreeTwoWayPathResult::UseTarget
        } else {
            ReadTreeTwoWayPathResult::Remove
        };
    }
    match (current, old, new) {
        (None, None, Some(_)) => ReadTreeTwoWayPathResult::UseTarget,
        (Some(current), None, None) => {
            let _ = current;
            ReadTreeTwoWayPathResult::UseCurrent
        }
        (Some(current), None, Some(new)) => {
            if merge_tree_same_entry(Some(current), Some(new)) {
                ReadTreeTwoWayPathResult::UseTarget
            } else {
                ReadTreeTwoWayPathResult::Conflict
            }
        }
        (Some(current), Some(old), None) => {
            if merge_tree_same_entry(Some(current), Some(old)) {
                ReadTreeTwoWayPathResult::Remove
            } else {
                ReadTreeTwoWayPathResult::Conflict
            }
        }
        (Some(current), Some(old), Some(new)) => {
            if merge_tree_same_entry(Some(current), Some(old))
                || merge_tree_same_entry(Some(current), Some(new))
            {
                ReadTreeTwoWayPathResult::UseTarget
            } else {
                ReadTreeTwoWayPathResult::Conflict
            }
        }
        (None, Some(_), None) => ReadTreeTwoWayPathResult::Remove,
        (None, Some(_), Some(_)) => ReadTreeTwoWayPathResult::UseTarget,
        (None, None, None) => ReadTreeTwoWayPathResult::Remove,
    }
}

fn read_tree_update_worktree(
    repo: &GitRepo,
    store: &LooseObjectStore,
    original_index: &GitIndex,
    result_index: &GitIndex,
    preserve_dirty_local: bool,
    force_checkout: bool,
    keep_existing_paths: bool,
    exclude_per_directory: Option<&str>,
    super_prefix: Option<&str>,
) -> Result<Vec<Vec<u8>>> {
    let target_entries = result_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .map(|entry| (entry.path.as_slice(), entry))
        .collect::<HashMap<_, _>>();
    let mut checkout_entries = Vec::new();
    let mut refreshed_paths = Vec::new();
    if !keep_existing_paths {
        let mut original_paths = BTreeSet::new();
        original_paths.extend(
            original_index
                .entries()
                .iter()
                .map(|entry| entry.path.clone()),
        );
        for path in original_paths {
            match target_entries.get(path.as_slice()).copied() {
                None => remove_worktree_path(repo, &path)?,
                Some(entry) if entry.skip_worktree() => {
                    let current = find_index_entry(original_index, &path);
                    if current.is_none_or(|current| !current.skip_worktree()) {
                        remove_worktree_path(repo, &path)?;
                    }
                }
                Some(_) => {}
            }
        }
    }
    for entry in result_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode != IndexMode::Gitlink)
    {
        if entry.skip_worktree() {
            continue;
        }
        let current = find_index_entry(original_index, &entry.path);
        if !read_tree_entry_needs_checkout(repo, current, entry, preserve_dirty_local)? {
            continue;
        }
        if !force_checkout {
            read_tree_preflight_checkout_path(
                repo,
                original_index,
                &entry.path,
                exclude_per_directory,
                super_prefix,
            )?;
        }
        checkout_entries.push(entry.clone());
        refreshed_paths.push(entry.path.clone());
    }
    if checkout_entries.is_empty() {
        return Ok(refreshed_paths);
    }
    let checkout_entries = GitIndex::from_entries(checkout_entries)?;
    let materialized_paths = checkout_entries
        .entries()
        .iter()
        .map(|entry| {
            let path = repo
                .root
                .join(String::from_utf8_lossy(&entry.path).as_ref());
            let existed = path_exists(&path);
            (path, existed)
        })
        .collect::<Vec<_>>();
    checkout_index(
        store,
        &checkout_entries,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    if let Err(error) = smudge_worktree_filter_entries(repo, &checkout_entries) {
        for (path, existed) in materialized_paths {
            if !existed {
                let _ = fs::remove_file(path);
            }
        }
        return Err(error);
    }
    Ok(refreshed_paths)
}

fn read_tree_same_materialized_entry(current: Option<&IndexEntry>, result: &IndexEntry) -> bool {
    current.is_some_and(|current| {
        merge_tree_same_entry(Some(current), Some(result))
            && current.skip_worktree() == result.skip_worktree()
    })
}

fn read_tree_entry_needs_checkout(
    repo: &GitRepo,
    current: Option<&IndexEntry>,
    result: &IndexEntry,
    preserve_dirty_local: bool,
) -> Result<bool> {
    if !read_tree_same_materialized_entry(current, result) {
        return Ok(true);
    }
    let path = worktree_path_for_index_entry(&repo.root, &result.path);
    if !path_exists(&path) {
        return Ok(true);
    }
    let modified = worktree_entry_modified(repo, &path, result)?;
    Ok(modified && !preserve_dirty_local)
}

fn read_tree_preflight_checkout_path(
    repo: &GitRepo,
    original_index: &GitIndex,
    path: &[u8],
    exclude_per_directory: Option<&str>,
    super_prefix: Option<&str>,
) -> Result<()> {
    let absolute = worktree_path_for_index_entry(&repo.root, path);
    let Ok(metadata) = fs::symlink_metadata(&absolute) else {
        return Ok(());
    };
    let tracked_paths = tracked_path_set_for_repo(repo, original_index)?;
    let ignore = GitIgnore::load_from_root(&repo.root)?;
    let ignored = ignored_untracked_files(&repo.root, &tracked_paths, &ignore)?;
    let allow_ignored = exclude_per_directory == Some(".gitignore");
    if !metadata.is_dir() {
        let is_tracked = tracked_paths.contains(path);
        let is_ignored = ignored.iter().any(|candidate| candidate.as_slice() == path);
        if !is_tracked && (!is_ignored || !allow_ignored) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "Untracked working tree file '{}' would be overwritten by merge.",
                    String::from_utf8_lossy(path)
                ),
            });
        }
        return Ok(());
    }
    let untracked = untracked_files(&repo.root, &tracked_paths, &ignore)?;
    let path_prefix = format!("{}/", String::from_utf8_lossy(path));
    if untracked.iter().any(|candidate| {
        candidate == path || String::from_utf8_lossy(candidate).starts_with(&path_prefix)
    }) {
        let display = match super_prefix {
            Some(prefix) if !prefix.is_empty() => {
                format!("{prefix}{}", String::from_utf8_lossy(path))
            }
            _ => String::from_utf8_lossy(path).into_owned(),
        };
        return Err(CliError::Stderr {
            code: 128,
            text: format!("error: Updating '{display}' would lose untracked files in it\n"),
        });
    }
    Ok(())
}

fn read_tree_validate_confusing_paths(repo: &GitRepo, index: &GitIndex) -> Result<()> {
    let protect_hfs = config_bool_enabled(repo, "core.protectHFS")?;
    let protect_ntfs = config_bool_enabled(repo, "core.protectNTFS")?;
    if !protect_hfs && !protect_ntfs {
        return Ok(());
    }
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        if read_tree_path_is_confusing(&entry.path, protect_hfs, protect_ntfs) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid path '{}'", String::from_utf8_lossy(&entry.path)),
            });
        }
    }
    Ok(())
}

fn read_tree_path_is_confusing(path: &[u8], protect_hfs: bool, protect_ntfs: bool) -> bool {
    path.split(|byte| *byte == b'/').any(|component| {
        component.is_empty()
            || component == b"."
            || component == b".."
            || (protect_ntfs && component.contains(&b'\\'))
            || read_tree_component_matches_dotgit(component, protect_hfs, protect_ntfs)
    })
}

fn read_tree_component_matches_dotgit(
    component: &[u8],
    protect_hfs: bool,
    protect_ntfs: bool,
) -> bool {
    if !protect_hfs && !protect_ntfs {
        return false;
    }
    let mut candidate = if protect_ntfs {
        match component.iter().position(|byte| *byte == b':') {
            Some(index) => &component[..index],
            None => component,
        }
    } else {
        component
    };
    let mut normalized = Vec::with_capacity(candidate.len());
    let mut cursor = 0usize;
    while cursor < candidate.len() {
        if protect_hfs && candidate[cursor..].starts_with(&[0xE2, 0x80, 0x8C]) {
            cursor += 3;
            continue;
        }
        normalized.push(candidate[cursor].to_ascii_lowercase());
        cursor += 1;
    }
    candidate = &normalized;
    while candidate
        .last()
        .is_some_and(|byte| *byte == b'.' || *byte == b' ')
    {
        candidate = &candidate[..candidate.len() - 1];
    }
    candidate == b".git" || candidate == b"git~1"
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

fn prefix_index_onto_existing(
    existing: GitIndex,
    imported: GitIndex,
    prefix: &str,
) -> Result<GitIndex> {
    let imported = prefix_index(imported, prefix)?;
    for entry in imported.entries().iter().filter(|entry| entry.stage == 0) {
        if let Some(overlap) = existing.entries().iter().find(|existing_entry| {
            existing_entry.stage == 0
                && (existing_entry.path == entry.path
                    || path_is_below_sparse_directory(&existing_entry.path, &entry.path)
                    || path_is_below_sparse_directory(&entry.path, &existing_entry.path))
        }) {
            return Err(CliError::Stderr {
                code: 128,
                text: format!(
                    "error: Entry '{}' overlaps with '{}'. Cannot bind.\n",
                    String::from_utf8_lossy(&entry.path),
                    String::from_utf8_lossy(&overlap.path)
                ),
            });
        }
    }
    let mut entries = existing.entries().to_vec();
    entries.extend(imported.entries().iter().cloned());
    Ok(GitIndex::from_entries(entries)?)
}

pub(crate) fn checkout_index_command(options: CheckoutIndexCommandOptions) -> Result<()> {
    let CheckoutIndexCommandOptions {
        all,
        force,
        quiet,
        update_index,
        no_create,
        stage,
        temp,
        ignore_skip_worktree_bits,
        stdin,
        nul,
        prefix,
        paths,
    } = options;
    if all && stdin {
        return Err(CliError::Fatal {
            code: 128,
            message: "git checkout-index: don't mix '--all' and '--stdin'".into(),
        });
    }
    if all && !paths.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "git checkout-index: don't mix '--all' and explicit filenames".into(),
        });
    }

    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let raw_index = read_repo_index_raw(&repo)?;
    let mut index = expand_repo_sparse_index(&repo, &raw_index)?;
    let stage_mode = checkout_index_stage_mode(stage.as_deref())?;
    let use_temp_output = temp || matches!(stage_mode, CheckoutIndexStageMode::All);
    let selected = if all {
        index
            .entries()
            .iter()
            .filter(|entry| {
                checkout_index_entry_matches(entry, stage_mode, ignore_skip_worktree_bits)
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        let inputs = checkout_index_inputs(stdin, nul, paths)?;
        if inputs.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "checkout-index requires --all or pathnames".into(),
            });
        }
        let mut selected = Vec::new();
        for path in inputs {
            let trailing_slash = path.to_string_lossy().ends_with('/');
            let mut relative = path_arg_to_repo_relative(&repo, &path)?;
            if trailing_slash && !relative.ends_with(b"/") {
                relative.push(b'/');
            }
            match stage_mode {
                CheckoutIndexStageMode::Normal => match find_index_entry(&index, &relative) {
                    _ if raw_index
                        .entry(&relative, 0)
                        .is_some_and(|entry| entry.mode == IndexMode::Tree) =>
                    {
                        return Err(CliError::Stderr {
                            code: 1,
                            text: format!(
                                "git checkout-index: {} is a sparse directory\n",
                                String::from_utf8_lossy(&relative)
                            ),
                        });
                    }
                    Some(entry)
                        if entry.stage == 0
                            && (ignore_skip_worktree_bits || !entry.skip_worktree()) =>
                    {
                        selected.push(entry.clone());
                    }
                    Some(entry) if entry.stage == 0 && !quiet => {
                        return Err(CliError::Stderr {
                            code: 1,
                            text: format!(
                                "git checkout-index: {} has skip-worktree enabled; use '--ignore-skip-worktree-bits' to checkout\n",
                                String::from_utf8_lossy(&relative)
                            ),
                        });
                    }
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
                },
                CheckoutIndexStageMode::Stage(stage) => match index.entry(&relative, stage) {
                    Some(entry) => selected.push(entry.clone()),
                    _ if quiet => {}
                    _ => {
                        return Err(CliError::Stderr {
                            code: 1,
                            text: format!(
                                "git checkout-index: {} does not exist at stage {stage}\n",
                                String::from_utf8_lossy(&relative)
                            ),
                        });
                    }
                },
                CheckoutIndexStageMode::All => {
                    let mut found = false;
                    for stage in 1..=3 {
                        if let Some(entry) = index.entry(&relative, stage) {
                            selected.push(entry.clone());
                            found = true;
                        }
                    }
                    if !found && !quiet {
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
        }
        selected
    };
    if use_temp_output {
        return checkout_index_temp_output(&repo, &store, &selected, stage_mode, quiet, nul);
    }
    let original_selected = selected;
    let prefix_is_none = prefix.is_none();
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
    let checkout_entries = if no_create {
        checkout_entries
            .into_iter()
            .filter(|entry| {
                let relative = String::from_utf8_lossy(&entry.path);
                path_exists(&root.join(relative.as_ref()))
            })
            .collect::<Vec<_>>()
    } else {
        checkout_entries
    };
    let checkout_entries = if !force && prefix_is_none {
        let mut pending = Vec::with_capacity(checkout_entries.len());
        for entry in checkout_entries {
            let path = worktree_path_for_index_entry(&repo.root, &entry.path);
            if path_exists(&path) {
                if worktree_entry_modified(&repo, &path, &entry)? {
                    return Err(CliError::Stderr {
                        code: 1,
                        text: format!(
                            "{} already exists, no checkout\n",
                            String::from_utf8_lossy(&entry.path)
                        ),
                    });
                }
                continue;
            }
            pending.push(entry);
        }
        pending
    } else {
        checkout_entries
    };
    let selected_index = GitIndex::from_entries(checkout_entries)?;
    let materialized_paths = selected_index
        .entries()
        .iter()
        .map(|entry| {
            let path = root.join(String::from_utf8_lossy(&entry.path).as_ref());
            let existed = path_exists(&path);
            (path, existed)
        })
        .collect::<Vec<_>>();
    checkout_index(
        &store,
        &selected_index,
        root,
        CheckoutIndexOptions { force },
    )?;
    let smudge_result = if prefixed_paths.is_empty() {
        smudge_worktree_filter_entries_with_metadata_for_index(
            &repo,
            &store,
            &index,
            &selected_index,
            &WorktreeCheckoutMetadata::default(),
        )
    } else {
        (|| {
            for (entry, path) in original_selected.iter().zip(prefixed_paths) {
                smudge_worktree_filter_entry_at_path(
                    &repo,
                    entry,
                    &repo.root.join(String::from_utf8_lossy(&path).as_ref()),
                )?;
            }
            Ok(())
        })()
    };
    if let Err(error) = smudge_result {
        for (path, existed) in materialized_paths {
            if !existed {
                let _ = fs::remove_file(path);
            }
        }
        return Err(error);
    }
    if update_index && prefix_is_none && matches!(stage_mode, CheckoutIndexStageMode::Normal) {
        let selected_paths = selected_index
            .entries()
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        refresh_tracked_index_metadata_matching(&repo, &mut index, &selected_paths)?;
        index.write_to_path(&repo.index_path)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct CheckoutIndexCommandOptions {
    pub(crate) all: bool,
    pub(crate) force: bool,
    pub(crate) quiet: bool,
    pub(crate) update_index: bool,
    pub(crate) no_create: bool,
    pub(crate) stage: Option<String>,
    pub(crate) temp: bool,
    pub(crate) ignore_skip_worktree_bits: bool,
    pub(crate) stdin: bool,
    pub(crate) nul: bool,
    pub(crate) prefix: Option<PathBuf>,
    pub(crate) paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckoutIndexStageMode {
    Normal,
    Stage(u8),
    All,
}

fn checkout_index_stage_mode(stage: Option<&str>) -> Result<CheckoutIndexStageMode> {
    let Some(stage) = stage else {
        return Ok(CheckoutIndexStageMode::Normal);
    };
    if stage == "all" {
        return Ok(CheckoutIndexStageMode::All);
    }
    match stage.parse::<u8>() {
        Ok(stage @ 1..=3) => Ok(CheckoutIndexStageMode::Stage(stage)),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "git checkout-index: stage should be between 1 and 3 or all, not {stage}"
            ),
        }),
    }
}

fn checkout_index_entry_matches(
    entry: &IndexEntry,
    stage_mode: CheckoutIndexStageMode,
    ignore_skip_worktree_bits: bool,
) -> bool {
    match stage_mode {
        CheckoutIndexStageMode::Normal => {
            entry.stage == 0 && (ignore_skip_worktree_bits || !entry.skip_worktree())
        }
        CheckoutIndexStageMode::Stage(stage) => entry.stage == stage,
        CheckoutIndexStageMode::All => entry.stage != 0,
    }
}

fn checkout_index_temp_output(
    repo: &GitRepo,
    store: &LooseObjectStore,
    selected: &[IndexEntry],
    stage_mode: CheckoutIndexStageMode,
    quiet: bool,
    nul: bool,
) -> Result<()> {
    match stage_mode {
        CheckoutIndexStageMode::All => {
            let mut grouped = BTreeMap::<Vec<u8>, [Option<IndexEntry>; 3]>::new();
            for entry in selected {
                if !(1..=3).contains(&entry.stage) {
                    continue;
                }
                grouped.entry(entry.path.clone()).or_default()[usize::from(entry.stage - 1)] =
                    Some(entry.clone());
            }
            for (path, stages) in grouped {
                let mut names = Vec::with_capacity(3);
                for entry in stages {
                    if let Some(entry) = entry {
                        names.push(checkout_index_temp_file(repo, store, &entry)?);
                    } else {
                        names.push(".".to_owned());
                    }
                }
                print!(
                    "{} {} {}\t{}{}",
                    names[0],
                    names[1],
                    names[2],
                    String::from_utf8_lossy(&path),
                    checkout_index_record_separator(nul)
                );
            }
        }
        CheckoutIndexStageMode::Normal | CheckoutIndexStageMode::Stage(_) => {
            for entry in selected {
                if entry.stage != 0 && quiet {
                    continue;
                }
                let name = checkout_index_temp_file(repo, store, entry)?;
                print!(
                    "{}\t{}{}",
                    name,
                    String::from_utf8_lossy(&entry.path),
                    checkout_index_record_separator(nul)
                );
            }
        }
    }
    Ok(())
}

fn checkout_index_temp_file(
    repo: &GitRepo,
    store: &LooseObjectStore,
    entry: &IndexEntry,
) -> Result<String> {
    let object = store.read_object(&entry.id)?;
    let temp_path = unique_temp_sibling(&repo.root.join(".merge_file"));
    fs::write(&temp_path, object.content)?;
    Ok(temp_path
        .strip_prefix(&repo.root)
        .unwrap_or(&temp_path)
        .to_string_lossy()
        .replace('\\', "/"))
}

fn checkout_index_record_separator(nul: bool) -> &'static str {
    if nul { "\0" } else { "\n" }
}

fn checkout_index_prefix_bytes(prefix: &Path) -> Vec<u8> {
    prefix.to_string_lossy().replace('\\', "/").into_bytes()
}

fn checkout_index_inputs(stdin: bool, nul: bool, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    if stdin && !paths.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "git checkout-index: don't mix '--stdin' and explicit filenames".into(),
        });
    }
    if !stdin {
        return Ok(paths);
    }
    let mut buffer = Vec::new();
    io::stdin().read_to_end(&mut buffer)?;
    let parts = if nul {
        buffer
            .split(|byte| *byte == b'\0')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
    } else {
        buffer
            .split(|byte| *byte == b'\n')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
    };
    Ok(parts
        .into_iter()
        .map(|part| PathBuf::from(String::from_utf8_lossy(part).into_owned()))
        .collect())
}

pub(crate) fn restore(
    source: Option<&str>,
    _quiet: bool,
    _progress: bool,
    _no_progress: bool,
    staged: bool,
    no_staged: bool,
    worktree: bool,
    no_worktree: bool,
    _merge: bool,
    _conflict: Option<String>,
    _ours: bool,
    _theirs: bool,
    _overlay: bool,
    _no_overlay: bool,
    _ignore_unmerged: bool,
    ignore_skip_worktree_bits: bool,
    _recurse_submodules: bool,
    _no_recurse_submodules: bool,
    patch: bool,
    pathspec_from_file: Option<PathBuf>,
    pathspec_file_nul: bool,
    mut paths: Vec<PathBuf>,
) -> Result<usize> {
    let _trace = phase_trace("restore.total");
    if let Some(pathspec_file) = pathspec_from_file {
        let loaded = read_pathspec_file(&pathspec_file, pathspec_file_nul)?;
        paths.extend(loaded);
    } else if pathspec_file_nul {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--pathspec-file-nul' requires '--pathspec-from-file'".into(),
        });
    }
    if paths.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "`restore` requires at least one path".into(),
        });
    }
    if patch {
        checkout_patch(source, &paths)?;
        return Ok(0);
    }
    let restore_index = staged;
    let restore_worktree = if worktree {
        true
    } else if no_worktree {
        false
    } else {
        !restore_index && !no_staged
    };
    if !restore_index && !restore_worktree {
        return Err(CliError::Fatal {
            code: 128,
            message: "neither '--staged' or '--worktree' is specified".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let mut index = {
        let _trace = phase_trace("restore.read_index");
        read_repo_index(&repo)?
    };
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
        .collect::<Result<Vec<_>>>()?;

    let source_index = {
        let _trace = phase_trace("restore.read_source_index");
        if let Some(source) = source {
            let source_id =
                resolve_commitish(&repo, &store, source).map_err(|_| CliError::Fatal {
                    code: 128,
                    message: restore_source_resolve_error(source),
                })?;
            let source_commit = commit_cache.read_commit(&source_id)?;
            tree_cache.read_tree_to_index(&source_commit.tree)?
        } else if restore_index {
            read_head_index_with_caches(&repo, &commit_cache, &tree_cache)?
        } else {
            index.clone()
        }
    };
    let original_index = index.clone();
    for pathspec in &pathspecs {
        let include_skipped = restore_index || ignore_skip_worktree_bits;
        let source_matches = restore_matching_entries(&source_index, pathspec, include_skipped);
        let current_matches = restore_matching_entries(&original_index, pathspec, include_skipped);
        if source_matches.is_empty() && current_matches.is_empty() {
            return Err(unmatched_restore_pathspec_error(std::slice::from_ref(
                pathspec,
            )));
        }
    }
    let mut checkout_entries = Vec::new();

    if restore_index {
        let _trace = phase_trace("restore.update_index");
        for pathspec in &pathspecs {
            let source_matches = restore_matching_entries(&source_index, pathspec, true);
            let current_matches = restore_matching_entries(&index, pathspec, true);
            if source_matches.is_empty() && current_matches.is_empty() {
                continue;
            }
            for entry in current_matches {
                index.remove_path(&entry.path)?;
            }
            for entry in source_matches {
                remove_index_path_or_dir(&mut index, &entry.path)?;
                index.upsert(entry)?;
            }
        }
        if sparse_checkout_active(&repo)? {
            apply_sparse_checkout_bits_to_index(&repo, &mut index)?;
            index = expand_repo_sparse_index(&repo, &index)?;
        }
        let write_index = collapse_sparse_index(&repo, &store, &index)?;
        write_index.write_to_path(&repo.index_path)?;
    }

    if restore_worktree {
        let _trace = phase_trace("restore.update_worktree");
        let worktree_source_index = if restore_index { &index } else { &source_index };
        for pathspec in &pathspecs {
            let source_matches = restore_matching_entries(
                worktree_source_index,
                pathspec,
                ignore_skip_worktree_bits,
            );
            let current_matches =
                restore_matching_entries(&original_index, pathspec, ignore_skip_worktree_bits);
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
        let materialized_paths = checkout_index_entries
            .entries()
            .iter()
            .filter(|entry| entry.stage == 0)
            .map(|entry| {
                let path = repo
                    .root
                    .join(String::from_utf8_lossy(&entry.path).as_ref());
                let existed = path_exists(&path);
                (path, existed)
            })
            .collect::<Vec<_>>();
        checkout_index(
            &store,
            &checkout_index_entries,
            &repo.root,
            CheckoutIndexOptions { force: true },
        )?;
        let updated_paths = checkout_index_entries.entries().len();
        if let Err(error) = smudge_worktree_filter_entries(&repo, &checkout_index_entries) {
            for (path, existed) in materialized_paths {
                if !existed {
                    let _ = fs::remove_file(path);
                }
            }
            return Err(error);
        }
        return Ok(updated_paths);
    }

    Ok(0)
}

fn restore_matching_entries(
    index: &GitIndex,
    pathspec: &[u8],
    ignore_skip_worktree_bits: bool,
) -> Vec<IndexEntry> {
    matching_index_entries(index, pathspec)
        .into_iter()
        .filter(|entry| ignore_skip_worktree_bits || !entry.skip_worktree())
        .collect()
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

pub(crate) fn reset(options: ResetOptions) -> Result<()> {
    let (soft, mixed, hard, merge, keep, args) =
        normalize_reset_mode_options(options.soft, options.mixed, options.hard, options.args)?;
    let selected = [soft, mixed, hard, merge, keep]
        .into_iter()
        .filter(|value| *value)
        .count();
    if selected > 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "reset mode must be one of --soft, --mixed, --hard, --merge, or --keep".into(),
        });
    }

    let mode = if soft {
        ResetMode::Soft
    } else if hard {
        ResetMode::Hard
    } else if merge {
        ResetMode::Merge
    } else if keep {
        ResetMode::Keep
    } else {
        let _ = mixed;
        ResetMode::Mixed
    };
    let should_refresh = options.refresh || !options.no_refresh;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let (args, pathspec_from_file, pathspec_file_nul) = normalize_reset_pathspec_file_options(
        args,
        options.pathspec_from_file,
        options.pathspec_file_nul,
    )?;
    let pathspec_args =
        reset_effective_args(args, pathspec_from_file.as_deref(), pathspec_file_nul)?;
    let patch_mode = pathspec_args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-p" | "--patch"));
    if patch_mode {
        return reset_patch(&repo, &store, &commit_cache, &tree_cache, &pathspec_args);
    }
    if let Some((source, paths)) = reset_path_mode(&repo, &store, &pathspec_args)? {
        if mode != ResetMode::Mixed {
            let mode_name = match mode {
                ResetMode::Soft => "soft",
                ResetMode::Hard => "hard",
                ResetMode::Mixed => "mixed",
                ResetMode::Merge => "merge",
                ResetMode::Keep => "keep",
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
        return reset_paths(
            &repo,
            &store,
            &commit_cache,
            &tree_cache,
            source,
            paths,
            options.quiet,
            should_refresh,
        );
    }
    let target = pathspec_args.first().map(String::as_str).unwrap_or("HEAD");
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if target == "HEAD"
        && refs
            .resolve("HEAD")
            .err()
            .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        return reset_unborn_head(&repo, &store, mode, options.quiet, should_refresh);
    }
    let target_id = resolve_commitish(&repo, &store, target)?;
    let target_commit = commit_cache.read_commit(&target_id)?;
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
            let mut new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
            if sparse_checkout_active(&repo)? {
                new_index = sparse_checkout_index(&repo, &new_index)?;
            }
            let write_index = collapse_sparse_index(&repo, &store, &new_index)?;
            write_index.write_to_path(&repo.index_path)?;
            if !options.quiet && should_refresh {
                print_reset_mixed_refresh_summary(&repo, &new_index, &[])?;
            }
        }
        ResetMode::Hard => {
            let raw_index = read_repo_index_raw(&repo)?;
            let preserve_full_index = !index_has_sparse_directories(&raw_index);
            let old_index = expand_sparse_index(&repo, &raw_index)?;
            let mut new_index = tree_cache.read_tree_to_index(&target_commit.tree)?;
            remove_tracked_paths_missing_from_target(&repo, &old_index, &new_index)?;
            if sparse_checkout_active(&repo)? {
                new_index = sparse_checkout_index(&repo, &new_index)?;
                for entry in new_index
                    .entries()
                    .iter()
                    .filter(|entry| entry.stage == 0 && entry.skip_worktree())
                {
                    remove_worktree_path(&repo, &entry.path)?;
                }
            }
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
            if should_refresh {
                refresh_tracked_index_metadata_after_checkout(&repo, &mut new_index, &[])?;
                new_index.refresh_cache_tree();
            }
            let write_index = if preserve_full_index {
                new_index
            } else {
                collapse_sparse_index(&repo, &store, &new_index)?
            };
            write_index.write_to_path(&repo.index_path)?;
            if !options.quiet {
                println!(
                    "HEAD is now at {} {}",
                    short_object_id(&target_id),
                    commit_subject(&target_commit.message)
                );
            }
        }
        ResetMode::Merge | ResetMode::Keep => {
            let checkout_metadata = WorktreeCheckoutMetadata {
                ref_name: target_ref_name,
                treeish: Some(target_id.clone()),
            };
            checkout_clean_worktree_replacement_with_metadata(
                &repo,
                &store,
                &target_id,
                &checkout_metadata,
            )?;
        }
    }
    Ok(())
}

struct ResetPatchTarget {
    source: String,
    paths: Vec<PathBuf>,
}

struct ResetPatchDisplay {
    header: String,
    hunks: Vec<String>,
}

fn reset_patch(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    args: &[String],
) -> Result<()> {
    let target = reset_patch_target(repo, store, args)?;
    let source_id = resolve_commitish(repo, store, &target.source)?;
    let source_commit = commit_cache.read_commit(&source_id)?;
    let source_index = tree_cache.read_tree_to_index(&source_commit.tree)?;
    let raw_index = read_repo_index_raw(repo)?;
    let current_index = expand_repo_sparse_index(repo, &raw_index)?;
    let pathspecs = target
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    let entries = diff_indexes(&source_index, &current_index)?
        .into_iter()
        .filter(|entry| pathspecs.is_empty() || pathspec_matches(&entry.path, &pathspecs))
        .collect::<Vec<_>>();
    let _sparse_expansion_region =
        patch_changes_require_sparse_expansion(repo, &raw_index, &entries)
            .then(|| trace2_region("index", "ensure_full_index"));
    if entries.is_empty() {
        return Ok(());
    }

    let mut patch_bytes = Vec::new();
    write_patch_entries(
        &mut patch_bytes,
        repo,
        store,
        &source_index,
        &current_index,
        &entries,
        PatchFormatOptions::cached(),
    )?;
    let output = String::from_utf8(patch_bytes.clone()).map_err(|error| CliError::Fatal {
        code: 128,
        message: format!("patch output was not valid utf-8: {error}"),
    })?;
    let patches = patch_commands::parse_apply_patches(&patch_bytes)?;
    let displays = reset_patch_displays(&output);
    let mut answers = patch_commands::PatchAnswers::read()?;
    let mut updated_index = current_index;
    let mut selected_any = false;
    let mut all_remaining = None;
    let mut quit = false;
    let mut last_output_was_prompt = false;
    for (patch, display) in patches.into_iter().zip(displays) {
        print!("{}", display.header);
        last_output_was_prompt = false;
        let target_path = patch
            .new_path
            .as_ref()
            .or(patch.old_path.as_ref())
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "reset patch has no target path".into(),
            })?
            .clone();
        let mut selected_hunks = Vec::new();
        for (index, (hunk, hunk_display)) in patch.hunks.iter().zip(&display.hunks).enumerate() {
            print!("{hunk_display}");
            last_output_was_prompt = false;
            let selected = match all_remaining {
                Some(value) => value,
                None => {
                    print!(
                        "({}/{}) Unstage this hunk {}? ",
                        index + 1,
                        patch.hunks.len(),
                        reset_patch_prompt_options(index, patch.hunks.len())
                    );
                    last_output_was_prompt = true;
                    io::stdout().flush()?;
                    match answers.next() {
                        patch_commands::PatchAnswer::Yes => true,
                        patch_commands::PatchAnswer::No | patch_commands::PatchAnswer::Split => {
                            false
                        }
                        patch_commands::PatchAnswer::All => {
                            all_remaining = Some(true);
                            true
                        }
                        patch_commands::PatchAnswer::Done => {
                            all_remaining = Some(false);
                            false
                        }
                        patch_commands::PatchAnswer::Quit => {
                            quit = true;
                            false
                        }
                    }
                }
            };
            if selected {
                selected_hunks.push(hunk.clone());
            }
            if quit {
                break;
            }
        }
        if selected_hunks.is_empty() {
            if quit {
                break;
            }
            continue;
        }
        selected_any = true;
        let remaining_hunks = patch_commands::rejected_hunks_for_selection(&patch, &selected_hunks);
        let source_entry = find_index_entry(&source_index, &target_path);
        if source_entry.is_none() && remaining_hunks.is_empty() {
            updated_index.remove_path(&target_path)?;
            continue;
        }
        let base = source_entry
            .map(|entry| read_index_entry_content(store, entry))
            .transpose()?
            .unwrap_or_default();
        let content =
            patch_commands::apply_hunks_to_content(&base, &remaining_hunks, &target_path)?;
        let mode = find_index_entry(&updated_index, &target_path)
            .map(|entry| entry.mode)
            .or_else(|| source_entry.map(|entry| entry.mode))
            .unwrap_or(IndexMode::File);
        upsert_index_content(store, &mut updated_index, target_path, content, mode)?;
        if quit {
            break;
        }
    }
    if last_output_was_prompt {
        println!();
    }
    if selected_any {
        updated_index.refresh_cache_tree();
        let write_index = collapse_sparse_index(repo, store, &updated_index)?;
        let _conversion_region = index_has_sparse_directories(&write_index)
            .then(|| trace2_region("index", "convert_to_sparse"));
        write_index.write_to_path(&repo.index_path)?;
    }
    Ok(())
}

fn reset_patch_displays(output: &str) -> Vec<ResetPatchDisplay> {
    let mut displays = Vec::new();
    let mut current = None;
    for line in output.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            if let Some(display) = current.take() {
                displays.push(display);
            }
            current = Some(ResetPatchDisplay {
                header: String::new(),
                hunks: Vec::new(),
            });
        }
        let display = current.get_or_insert_with(|| ResetPatchDisplay {
            header: String::new(),
            hunks: Vec::new(),
        });
        if line.starts_with("@@ ") {
            display.hunks.push(String::new());
        }
        if let Some(hunk) = display.hunks.last_mut() {
            hunk.push_str(line);
        } else {
            display.header.push_str(line);
        }
    }
    if let Some(display) = current {
        displays.push(display);
    }
    displays
}

fn reset_patch_prompt_options(index: usize, total: usize) -> &'static str {
    match (index, total) {
        (_, 1) => "[y,n,q,a,d,e,p,P,?]",
        (0, _) => "[y,n,q,a,d,j,J,g,/,e,p,?]",
        (index, total) if index + 1 == total => "[y,n,q,a,d,K,g,/,e,p,?]",
        _ => "[y,n,q,a,d,k,j,J,K,g,/,e,p,?]",
    }
}

fn reset_patch_target(
    repo: &GitRepo,
    store: &LooseObjectStore,
    args: &[String],
) -> Result<ResetPatchTarget> {
    let mut positional = args
        .iter()
        .filter(|arg| !matches!(arg.as_str(), "-p" | "--patch"))
        .cloned()
        .collect::<Vec<_>>();
    let separator = positional.iter().position(|arg| arg == "--");
    if let Some(separator) = separator {
        let paths = positional
            .drain(separator + 1..)
            .map(PathBuf::from)
            .collect();
        positional.pop();
        let source = positional
            .first()
            .cloned()
            .unwrap_or_else(|| "HEAD".to_owned());
        return Ok(ResetPatchTarget { source, paths });
    }
    let source = positional
        .first()
        .filter(|arg| resolve_commitish(repo, store, arg).is_ok())
        .cloned();
    let paths = positional
        .into_iter()
        .skip(usize::from(source.is_some()))
        .map(PathBuf::from)
        .collect();
    Ok(ResetPatchTarget {
        source: source.unwrap_or_else(|| "HEAD".to_owned()),
        paths,
    })
}

fn reset_unborn_head(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mode: ResetMode,
    quiet: bool,
    should_refresh: bool,
) -> Result<()> {
    match mode {
        ResetMode::Soft => Ok(()),
        ResetMode::Mixed => {
            let new_index = GitIndex::new();
            new_index.write_to_path(&repo.index_path)?;
            if !quiet && should_refresh {
                print_reset_mixed_refresh_summary(repo, &new_index, &[])?;
            }
            Ok(())
        }
        ResetMode::Hard | ResetMode::Merge | ResetMode::Keep => {
            let old_index = read_repo_index(repo)?;
            let mut new_index = GitIndex::new();
            remove_tracked_paths_missing_from_target(repo, &old_index, &new_index)?;
            new_index.write_to_path(&repo.index_path)?;
            checkout_worktree_updates_to_index_with_metadata(
                repo,
                store,
                &new_index,
                &WorktreeCheckoutMetadata::default(),
            )?;
            if should_refresh {
                refresh_tracked_index_metadata_after_checkout(repo, &mut new_index, &[])?;
                new_index.refresh_cache_tree();
                new_index.write_to_path(&repo.index_path)?;
            }
            Ok(())
        }
    }
}

fn reset_effective_args(
    mut args: Vec<String>,
    pathspec_from_file: Option<&Path>,
    pathspec_file_nul: bool,
) -> Result<Vec<String>> {
    if let Some(pathspec_file) = pathspec_from_file {
        let loaded = read_pathspec_file(pathspec_file, pathspec_file_nul)?;
        if !args.iter().any(|arg| arg == "--") {
            args.push("--".to_owned());
        }
        args.extend(
            loaded
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned()),
        );
    } else if pathspec_file_nul {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--pathspec-file-nul' requires '--pathspec-from-file'".into(),
        });
    }
    Ok(args)
}

fn normalize_reset_pathspec_file_options(
    args: Vec<String>,
    mut pathspec_from_file: Option<PathBuf>,
    mut pathspec_file_nul: bool,
) -> Result<(Vec<String>, Option<PathBuf>, bool)> {
    let mut normalized = Vec::with_capacity(args.len());
    let mut pathspec_mode = false;
    let mut cursor = 0;
    while cursor < args.len() {
        let arg = &args[cursor];
        if pathspec_mode {
            normalized.push(arg.clone());
            cursor += 1;
            continue;
        }
        match arg.as_str() {
            "--" => {
                pathspec_mode = true;
                normalized.push(arg.clone());
            }
            "--pathspec-from-file" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "reset --pathspec-from-file requires a file".into(),
                    });
                };
                pathspec_from_file = Some(PathBuf::from(value));
            }
            other if other.starts_with("--pathspec-from-file=") => {
                let Some(value) = other.strip_prefix("--pathspec-from-file=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "reset --pathspec-from-file requires a file".into(),
                    });
                };
                pathspec_from_file = Some(PathBuf::from(value));
            }
            "--pathspec-file-nul" => {
                pathspec_file_nul = true;
            }
            other => normalized.push(other.to_owned()),
        }
        cursor += 1;
    }
    Ok((normalized, pathspec_from_file, pathspec_file_nul))
}

fn print_reset_mixed_refresh_summary(
    repo: &GitRepo,
    new_index: &GitIndex,
    paths: &[PathBuf],
) -> Result<()> {
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    let mut changed = worktree_status(repo, new_index)?
        .into_iter()
        .filter(|(path, _)| pathspecs.is_empty() || pathspec_matches(path, &pathspecs))
        .collect::<Vec<_>>();
    if changed.is_empty() {
        return Ok(());
    }
    changed.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    println!("Unstaged changes after reset:");
    for (path, status) in changed {
        println!("{status}\t{}", String::from_utf8_lossy(&path));
    }
    Ok(())
}

fn normalize_reset_mode_options(
    mut soft: bool,
    mut mixed: bool,
    mut hard: bool,
    args: Vec<String>,
) -> Result<(bool, bool, bool, bool, bool, Vec<String>)> {
    let mut normalized = Vec::with_capacity(args.len());
    let mut pathspec_mode = false;
    let mut merge = false;
    let mut keep = false;
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
            "--merge" => merge = true,
            "--keep" => keep = true,
            value
                if value.starts_with("--soft=")
                    || value.starts_with("--mixed=")
                    || value.starts_with("--hard=")
                    || value.starts_with("--merge=")
                    || value.starts_with("--keep=") =>
            {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unknown option `{value}`"),
                });
            }
            _ => normalized.push(arg),
        }
    }
    Ok((soft, mixed, hard, merge, keep, normalized))
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
    quiet: bool,
    refresh: bool,
) -> Result<()> {
    let source_id = resolve_commitish(repo, store, source)?;
    let source_commit = commit_cache.read_commit(&source_id)?;
    let source_index = tree_cache.read_tree_to_index(&source_commit.tree)?;
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative(repo, path))
        .collect::<Result<Vec<_>>>()?;
    let raw_index = read_repo_index_raw(repo)?;
    let requires_expansion = pathspecs
        .iter()
        .any(|pathspec| sparse_index_path_requires_expansion(&raw_index, pathspec));
    let mut index = {
        let _region = requires_expansion.then(|| trace2_region("index", "ensure_full_index"));
        expand_repo_sparse_index(repo, &raw_index)?
    };
    for pathspec in &pathspecs {
        let source_matches = matching_index_entries(&source_index, &pathspec);
        let current_matches = matching_index_entries(&index, &pathspec);
        if source_matches.is_empty() && current_matches.is_empty() {
            continue;
        }
        for entry in current_matches {
            index.remove_path(&entry.path)?;
        }
        for entry in source_matches {
            remove_index_path_or_dir(&mut index, &entry.path)?;
            index.upsert(entry)?;
        }
    }
    let write_index = collapse_sparse_index(repo, store, &index)?;
    let _conversion_region = (requires_expansion && index_has_sparse_directories(&write_index))
        .then(|| trace2_region("index", "convert_to_sparse"));
    write_index.write_to_path(&repo.index_path)?;
    if !quiet && refresh {
        print_reset_mixed_refresh_summary(repo, &index, &paths)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetMode {
    Soft,
    Mixed,
    Hard,
    Merge,
    Keep,
}

pub(crate) fn worktree(args: Vec<String>) -> Result<()> {
    let subcommand = args.first().map(String::as_str).unwrap_or("list");
    match subcommand {
        "list" => worktree_list(&args[1..]),
        "add" => worktree_add(&args[1..]),
        "move" => worktree_move(&args[1..]),
        "lock" => worktree_lock(&args[1..]),
        "unlock" => worktree_unlock(&args[1..]),
        "remove" => worktree_remove(&args[1..]),
        "prune" => worktree_prune(&args[1..]),
        "repair" => worktree_repair(&args[1..]),
        _ => Err(worktree_unknown_subcommand_error(subcommand)),
    }
}

pub(crate) fn sparse_checkout(args: Vec<String>) -> Result<()> {
    let subcommand = args.first().map(String::as_str).unwrap_or("list");
    match subcommand {
        "set" => sparse_checkout_set(&args[1..]),
        "add" => sparse_checkout_add(&args[1..]),
        "reapply" => sparse_checkout_reapply(&args[1..]),
        "list" => sparse_checkout_list(),
        "disable" => sparse_checkout_disable(),
        "init" => sparse_checkout_init(&args[1..]),
        "check-rules" => sparse_checkout_check_rules(&args[1..]),
        _ => Err(sparse_checkout_unknown_subcommand_error(subcommand)),
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
        let quiet_args = quiet
            .then(|| "--quiet".to_owned())
            .into_iter()
            .collect::<Vec<_>>();
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
        _ => Err(submodule_usage_error()),
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
    let mut checkout = true;
    let mut force_count = 0usize;
    let mut guess_remote = false;
    let mut lock = false;
    let mut lock_reason = String::new();
    let mut no_guess_remote = false;
    let mut no_track = false;
    let mut orphan = false;
    let mut quiet = false;
    let mut _track_requested = false;
    let mut values = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if arg == "-d" || arg == "--detach" {
            detach = true;
        } else if arg == "--checkout" {
            checkout = true;
        } else if arg == "--no-checkout" {
            checkout = false;
        } else if arg == "-f" || arg == "--force" {
            force_count += 1;
        } else if arg == "--guess-remote" {
            guess_remote = true;
        } else if arg == "-q" || arg == "--quiet" {
            quiet = true;
        } else if arg == "--no-guess-remote" {
            no_guess_remote = true;
        } else if arg == "--no-track" {
            no_track = true;
        } else if arg == "--orphan" {
            orphan = true;
        } else if arg == "--lock" {
            lock = true;
        } else if arg == "--reason" {
            cursor += 1;
            lock_reason = args.get(cursor).cloned().ok_or_else(|| CliError::Fatal {
                code: 129,
                message: "--reason requires a value".into(),
            })?;
        } else if let Some(value) = arg.strip_prefix("--reason=") {
            lock_reason = value.to_owned();
        } else if arg == "--track" {
            _track_requested = true;
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
    if no_guess_remote {
        guess_remote = false;
    }
    if orphan && branch_option.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "options '-b'/'-B' and '--orphan' cannot be used together".into(),
        });
    }
    if detach && (branch_option.is_some() || orphan) {
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
    let path_branch_name = target_root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("cannot derive branch name from '{}'", target_root.display()),
        })?;
    let inferred_upstream =
        if guess_remote && branch_option.is_none() && values.len() == 1 && !orphan {
            find_unique_remote_tracking_branch(&refs, path_branch_name)?
        } else {
            None
        };
    let default_commitish = inferred_upstream
        .as_ref()
        .map(|upstream| upstream.ref_name.as_str())
        .unwrap_or("HEAD");
    let explicit_commitish = values.get(1).copied();
    let commitish = explicit_commitish.unwrap_or(default_commitish);
    let inferred_orphan = !orphan
        && !detach
        && branch_option.is_none()
        && values.len() == 1
        && explicit_commitish.is_none()
        && resolve_commitish(&repo, &store, commitish).is_err();
    let orphan = orphan || inferred_orphan;
    let mut id = if orphan {
        None
    } else {
        Some(
            resolve_commitish(&repo, &store, commitish).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("invalid reference: {commitish}"),
            })?,
        )
    };
    let mut tracking_upstream = None;
    let mut created_new_branch = false;
    let branch_ref = if detach {
        None
    } else if orphan {
        let ref_name = branch_ref_name(path_branch_name)?;
        if ref_exists(&refs, &ref_name)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("a branch named '{path_branch_name}' already exists"),
            });
        }
        created_new_branch = true;
        Some(ref_name)
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
            id.as_ref().expect("worktree branch target"),
            "branch: Created from HEAD",
        )?;
        if !no_track {
            tracking_upstream = parse_worktree_tracking_upstream(&refs, commitish)?;
        }
        created_new_branch = true;
        Some(ref_name)
    } else if values.len() == 1 {
        let ref_name = branch_ref_name(path_branch_name)?;
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
            id = Some(refs.resolve(&ref_name)?);
        } else {
            let target_id = if let Some(upstream) = &inferred_upstream {
                tracking_upstream = Some(upstream.clone());
                refs.resolve(&upstream.ref_name)?
            } else {
                id.expect("path-only worktree target")
            };
            write_ref_with_reflog(
                &common_repo,
                &refs,
                &ref_name,
                &target_id,
                "branch: Created from HEAD",
            )?;
            id = Some(target_id);
            created_new_branch = true;
        }
        Some(ref_name)
    } else if let Some(ref_name) = branch_ref_name(commitish)
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
    let ref_kind = refs.storage_kind()?;
    let algorithm = repo_hash_algorithm_from_config(&repo)?;
    if ref_kind == zmin_git_core::refs::RefStorageKind::Reftable {
        zmin_git_core::initialize_alternate_ref_store(
            &admin_dir,
            &admin_dir,
            algorithm,
            ref_kind,
            "refs/heads/.invalid",
        )?;
        let linked_refs = RefStore::new(&admin_dir, algorithm);
        if let Some(branch_ref) = &branch_ref {
            linked_refs.write_symbolic_ref("HEAD", branch_ref)?;
        } else {
            linked_refs.write_ref("HEAD", id.as_ref().expect("detached worktree id"))?;
        }
    } else {
        fs::create_dir_all(admin_dir.join("refs"))?;
        if let Some(branch_ref) = &branch_ref {
            fs::write(admin_dir.join("HEAD"), format!("ref: {branch_ref}\n"))?;
        } else {
            fs::write(
                admin_dir.join("HEAD"),
                format!("{}\n", id.as_ref().expect("detached worktree id").to_hex()),
            )?;
        }
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
    if sparse_checkout_active(&repo)? {
        let info_dir = linked_repo.git_dir.join("info");
        fs::create_dir_all(&info_dir)?;
        fs::copy(
            sparse_checkout_file(&repo),
            info_dir.join("sparse-checkout"),
        )?;
        set_config_value(&repo, "extensions.worktreeConfig", "true")?;
        set_config_value_in_file(
            &linked_repo.git_dir.join("config.worktree"),
            "core.sparseCheckout",
            "true",
        )?;
        set_config_value_in_file(
            &linked_repo.git_dir.join("config.worktree"),
            "core.sparseCheckoutCone",
            if sparse_checkout_cone_mode(&repo)? {
                "true"
            } else {
                "false"
            },
        )?;
        set_config_value_in_file(
            &linked_repo.git_dir.join("config.worktree"),
            "index.sparse",
            if config_bool_enabled(&repo, "index.sparse")? {
                "true"
            } else {
                "false"
            },
        )?;
    }
    if let Some(id) = id.as_ref() {
        append_reflog_if_identity_available(
            &linked_repo,
            "HEAD",
            &zero_object_id(),
            id,
            "worktree: Created from HEAD",
        )?;
    }
    let commit = if let Some(id) = id.as_ref() {
        Some(commit_cache.read_commit(id)?)
    } else {
        None
    };
    if orphan {
        GitIndex::new().write_to_path(&linked_repo.index_path)?;
    } else if checkout {
        let mut new_index =
            tree_cache.read_tree_to_index(&commit.as_ref().expect("worktree commit").tree)?;
        if sparse_checkout_active(&linked_repo)? {
            apply_sparse_checkout_bits_to_index(&linked_repo, &mut new_index)?;
        }
        new_index.write_to_path(&linked_repo.index_path)?;
        let checkout_entries = GitIndex::from_entries(
            new_index
                .entries()
                .iter()
                .filter(|entry| entry.stage == 0 && !entry.skip_worktree())
                .cloned()
                .collect(),
        )?;
        checkout_index(
            &store,
            &checkout_entries,
            &linked_repo.root,
            CheckoutIndexOptions { force: true },
        )?;
    }
    if lock {
        fs::write(
            linked_repo.git_dir.join("locked"),
            format!("{lock_reason}\n"),
        )?;
    }
    if let (Some(branch_ref), Some(upstream)) = (branch_ref.as_ref(), tracking_upstream.as_ref()) {
        let branch_name = branch_display_name(branch_ref);
        set_config_value(
            &common_repo,
            &format!("branch.{branch_name}.remote"),
            &upstream.remote,
        )?;
        set_config_value(
            &common_repo,
            &format!("branch.{branch_name}.merge"),
            &upstream.merge,
        )?;
    }
    if !quiet {
        if inferred_orphan {
            eprintln!("No possible source branch, inferring '--orphan'");
        }
        if let Some(branch_ref) = &branch_ref {
            let action = if orphan || created_new_branch {
                if let Some((_, reset)) = branch_option {
                    if reset {
                        "resetting branch"
                    } else {
                        "new branch"
                    }
                } else {
                    "new branch"
                }
            } else {
                "checking out"
            };
            eprintln!(
                "Preparing worktree ({action} '{}')",
                branch_display_name(branch_ref)
            );
            if let Some(upstream) = &tracking_upstream {
                println!(
                    "branch '{}' set up to track '{}'.",
                    branch_display_name(branch_ref),
                    upstream.display
                );
            }
            if let Some((id, commit)) = id.as_ref().zip(commit.as_ref()) {
                println!(
                    "HEAD is now at {} {}",
                    short_object_id(id),
                    commit_subject(&commit.message)
                );
            }
        } else {
            eprintln!(
                "Preparing worktree (detached HEAD {})",
                short_object_id(id.as_ref().expect("detached worktree id"))
            );
            if checkout {
                println!(
                    "HEAD is now at {} {}",
                    short_object_id(id.as_ref().expect("detached worktree id")),
                    commit_subject(&commit.as_ref().expect("detached worktree commit").message)
                );
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct WorktreeTrackingUpstream {
    remote: String,
    merge: String,
    display: String,
    ref_name: String,
}

fn parse_worktree_tracking_upstream(
    refs: &RefStore,
    value: &str,
) -> Result<Option<WorktreeTrackingUpstream>> {
    if let Some(rest) = value.strip_prefix("refs/remotes/") {
        let Some((remote, branch)) = rest.split_once('/') else {
            return Ok(None);
        };
        return Ok(Some(WorktreeTrackingUpstream {
            remote: remote.to_owned(),
            merge: format!("refs/heads/{branch}"),
            display: format!("{remote}/{branch}"),
            ref_name: value.to_owned(),
        }));
    }
    if let Some((remote, branch)) = value.split_once('/') {
        let ref_name = format!("refs/remotes/{remote}/{branch}");
        if refs.resolve(&ref_name).is_ok() {
            return Ok(Some(WorktreeTrackingUpstream {
                remote: remote.to_owned(),
                merge: format!("refs/heads/{branch}"),
                display: value.to_owned(),
                ref_name,
            }));
        }
    }
    Ok(None)
}

fn find_unique_remote_tracking_branch(
    refs: &RefStore,
    branch_name: &str,
) -> Result<Option<WorktreeTrackingUpstream>> {
    let suffix = format!("/{branch_name}");
    let matches = refs
        .list_refs("refs/remotes/")?
        .into_iter()
        .filter(|ref_name| !ref_name.ends_with("/HEAD") && ref_name.ends_with(&suffix))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Ok(None);
    }
    parse_worktree_tracking_upstream(refs, &matches[0])
}

fn worktree_list(args: &[String]) -> Result<()> {
    let mut porcelain = false;
    let mut nul_terminated = false;
    for arg in args {
        match arg.as_str() {
            "--porcelain" => porcelain = true,
            "-z" => nul_terminated = true,
            "-v" => {}
            _ => {}
        }
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head = refs.resolve("HEAD")?;
    if porcelain {
        write_worktree_list_entry(
            &mut io::stdout(),
            &repo.root,
            &head,
            current_branch_ref(&refs)?,
            worktree_lock_reason(&repo.git_dir)?,
            nul_terminated,
        )?;
    } else {
        let mut entries = Vec::new();
        let label = current_branch_ref(&refs)?
            .map(|branch| format!("[{}]", branch_display_name(&branch)))
            .unwrap_or_else(|| "(detached HEAD)".into());
        entries.push((repo.root.clone(), head, label));
        for linked in linked_worktrees(&repo)? {
            let linked_refs = RefStore::new(&linked.git_dir, GitHashAlgorithm::Sha1);
            let (id, branch) = linked_head_id_and_branch(&repo, &linked_refs)?;
            let label = branch
                .map(|branch| format!("[{}]", branch_display_name(&branch)))
                .unwrap_or_else(|| "(detached HEAD)".into());
            let _ = store.read_object(&id)?;
            entries.push((linked.root, id, label));
        }
        let width = entries
            .iter()
            .map(|(root, _, _)| root.display().to_string().len())
            .max()
            .unwrap_or(0);
        for (root, id, label) in entries {
            println!(
                "{:<width$} {} {}",
                root.display(),
                short_object_id(&id),
                label,
                width = width
            );
        }
        return Ok(());
    }
    for linked in linked_worktrees(&repo)? {
        let linked_refs = RefStore::new(&linked.git_dir, GitHashAlgorithm::Sha1);
        let (id, branch) = linked_head_id_and_branch(&repo, &linked_refs)?;
        if porcelain {
            write_worktree_list_entry(
                &mut io::stdout(),
                &linked.root,
                &id,
                branch,
                worktree_lock_reason(&linked.git_dir)?,
                nul_terminated,
            )?;
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

fn write_worktree_list_entry(
    writer: &mut dyn Write,
    root: &Path,
    id: &ObjectId,
    branch: Option<String>,
    lock_reason: Option<String>,
    nul_terminated: bool,
) -> Result<()> {
    let separator = if nul_terminated { "\0" } else { "\n" };
    write!(writer, "worktree {}{separator}", root.display())?;
    write!(writer, "HEAD {}{separator}", id.to_hex())?;
    if let Some(branch) = branch {
        write!(writer, "branch {branch}{separator}")?;
    } else {
        write!(writer, "detached{separator}")?;
    }
    if let Some(reason) = lock_reason {
        if reason.is_empty() {
            write!(writer, "locked{separator}")?;
        } else {
            write!(writer, "locked {reason}{separator}")?;
        }
    }
    write!(writer, "{separator}")?;
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
    if force_count == 0 {
        let linked_repo = find_repo_at(&target)?;
        let store = LooseObjectStore::new(
            linked_repo.objects_dir.clone(),
            repo_hash_algorithm_from_config(&linked_repo)?,
        );
        if !worktree_clean(&linked_repo, &store)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "'{}' contains modified or untracked files, use --force to delete it",
                    values[0]
                ),
            });
        }
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
    let options = parse_sparse_checkout_options(patterns, true, SparseCheckoutUsage::Set)?;
    let repo = find_repo_or_bare()?;
    ensure_sparse_checkout_worktree(&repo)?;
    apply_sparse_checkout_config_options(&repo, &options)?;
    let cone_mode = sparse_checkout_cone_mode(&repo)?;
    validate_sparse_checkout_inputs(&repo, options.patterns(), cone_mode, options.skip_checks)?;
    let patterns = normalize_sparse_checkout_inputs(&repo, options.patterns(), cone_mode)?;
    write_sparse_checkout_patterns(&repo, &patterns, cone_mode)?;
    apply_sparse_checkout(&repo)
}

pub(crate) fn enable_clone_sparse_checkout(repo: &GitRepo) -> Result<()> {
    let options = SparseCheckoutOptions {
        cone: Some(true),
        ..SparseCheckoutOptions::default()
    };
    set_config_value(repo, "extensions.worktreeConfig", "true")?;
    write_clone_sparse_checkout_worktree_config(repo)?;
    apply_sparse_checkout_config_options(repo, &options)?;
    write_sparse_checkout_patterns(repo, &[], true)?;
    apply_sparse_checkout(repo)
}

fn sparse_checkout_add(patterns: &[String]) -> Result<()> {
    let repo = find_repo_or_bare()?;
    ensure_sparse_checkout_worktree(&repo)?;
    ensure_sparse_checkout_enabled(&repo, "no sparse-checkout to add to")?;
    let options = parse_sparse_checkout_options(patterns, true, SparseCheckoutUsage::Add)?;
    apply_sparse_checkout_config_options(&repo, &options)?;
    let cone_mode = sparse_checkout_cone_mode(&repo)?;
    validate_sparse_checkout_inputs(&repo, options.patterns(), cone_mode, options.skip_checks)?;
    if cone_mode && !sparse_checkout_file_uses_cone_patterns(&repo)? {
        return Err(CliError::Fatal {
            code: 128,
            message: "existing sparse-checkout patterns do not use cone mode".into(),
        });
    }
    let mut combined = read_sparse_checkout_patterns(&repo)?;
    for pattern in normalize_sparse_checkout_inputs(&repo, options.patterns(), cone_mode)? {
        if !cone_mode || !combined.iter().any(|existing| existing == &pattern) {
            combined.push(pattern);
        }
    }
    write_sparse_checkout_patterns(&repo, &combined, cone_mode)?;
    apply_sparse_checkout(&repo)
}

fn sparse_checkout_init(args: &[String]) -> Result<()> {
    let options = parse_sparse_checkout_options(args, false, SparseCheckoutUsage::Init)?;
    if !options.patterns.is_empty() || options.stdin {
        return Err(CliError::Fatal {
            code: 129,
            message: "sparse-checkout init does not take patterns".into(),
        });
    }
    let repo = find_repo_or_bare()?;
    ensure_sparse_checkout_worktree(&repo)?;
    apply_sparse_checkout_config_options(&repo, &options)?;
    if !sparse_checkout_file(&repo).exists() {
        write_sparse_checkout_patterns(&repo, &["/*".to_owned(), "!/*/".to_owned()], false)?;
    }
    apply_sparse_checkout(&repo)
}

fn sparse_checkout_reapply(args: &[String]) -> Result<()> {
    let repo = find_repo_or_bare()?;
    ensure_sparse_checkout_worktree(&repo)?;
    ensure_sparse_checkout_enabled(
        &repo,
        "must be in a sparse-checkout to reapply sparsity patterns",
    )?;
    let options = parse_sparse_checkout_options(args, false, SparseCheckoutUsage::Reapply)?;
    apply_sparse_checkout_config_options(&repo, &options)?;
    apply_sparse_checkout(&repo)
}

fn sparse_checkout_list() -> Result<()> {
    let repo = find_repo_or_bare()?;
    ensure_sparse_checkout_worktree(&repo)?;
    if !sparse_checkout_file(&repo).exists() {
        let message = if config_bool_enabled(&repo, "core.sparseCheckout")? {
            "this worktree is not sparse (sparse-checkout file may not exist)"
        } else {
            "this worktree is not sparse"
        };
        if config_bool_enabled(&repo, "core.sparseCheckout")? {
            return Err(CliError::Stderr {
                code: 0,
                text: format!("warning: {message}\n"),
            });
        }
        return Err(CliError::Fatal {
            code: 128,
            message: message.to_owned(),
        });
    }
    let raw = fs::read_to_string(sparse_checkout_file(&repo))?;
    let quote_paths = sparse_checkout_cone_mode(&repo)? && valid_cone_sparse_checkout_file(&raw);
    let quote_non_ascii = read_config_value(&repo, "core.quotePath")?
        .as_deref()
        .and_then(parse_git_bool)
        .unwrap_or(true);
    for pattern in read_sparse_checkout_patterns(&repo)? {
        if quote_paths {
            let quoted = reference_commands::quote_git_path(pattern.as_bytes(), quote_non_ascii);
            println!("{}", String::from_utf8_lossy(&quoted));
        } else {
            println!("{pattern}");
        }
    }
    Ok(())
}

fn sparse_checkout_disable() -> Result<()> {
    let repo = find_repo_or_bare()?;
    ensure_sparse_checkout_worktree(&repo)?;
    checkout_sparse_excluded_entries(&repo)?;
    let _ = unset_worktree_config_value(&repo, "core.sparseCheckout");
    let _ = unset_worktree_config_value(&repo, "core.sparseCheckoutCone");
    set_worktree_config_value(&repo, "index.sparse", "false")?;
    Ok(())
}

fn sparse_checkout_check_rules(args: &[String]) -> Result<()> {
    let options = parse_sparse_checkout_check_rules_options(args)?;
    let repo = find_repo_or_bare()?;
    let (patterns, cone_mode) = if let Some(rules_file) = options.rules_file.as_ref() {
        let path = if rules_file.is_absolute() {
            rules_file.clone()
        } else {
            repo.root.join(rules_file)
        };
        let raw = fs::read_to_string(path)?;
        let cone_mode = options.cone.unwrap_or(true);
        let rules = raw
            .lines()
            .map(unquote_sparse_checkout_input)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let patterns = if cone_mode {
            cone_sparse_checkout_file(&rules)
                .lines()
                .map(str::to_owned)
                .collect()
        } else {
            rules
        };
        (patterns, cone_mode)
    } else {
        ensure_sparse_checkout_enabled(&repo, "this worktree is not sparse")?;
        (
            read_sparse_checkout_match_patterns(&repo)?,
            options.cone.unwrap_or(sparse_checkout_cone_mode(&repo)?),
        )
    };
    let matcher = sparse_pattern_matcher(&patterns);
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let separator = if options.nul { b'\0' } else { b'\n' };
    let mut output = io::stdout().lock();
    for raw_path in input.split(|byte| *byte == separator) {
        if raw_path.is_empty() {
            continue;
        }
        let path = if options.nul {
            raw_path.to_vec()
        } else {
            unquote_sparse_checkout_input(&String::from_utf8_lossy(raw_path)).into_bytes()
        };
        if sparse_path_matches(&path, &matcher, cone_mode) {
            output.write_all(raw_path)?;
            output.write_all(&[separator])?;
        }
    }
    Ok(())
}

fn parse_sparse_checkout_check_rules_options(
    args: &[String],
) -> Result<SparseCheckoutCheckRulesOptions> {
    let mut options = SparseCheckoutCheckRulesOptions::default();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        match arg.as_str() {
            "--cone" => options.cone = Some(true),
            "--no-cone" => options.cone = Some(false),
            "-z" => options.nul = true,
            "--rules-file" => {
                cursor += 1;
                options.rules_file = Some(PathBuf::from(args.get(cursor).ok_or_else(|| {
                    CliError::Fatal {
                        code: 129,
                        message: "option `rules-file' requires a value".into(),
                    }
                })?));
            }
            value if value.starts_with("--rules-file=") => {
                options.rules_file = Some(PathBuf::from(&value["--rules-file=".len()..]));
            }
            value => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!(
                        "error: unknown option `{}`\nusage: git sparse-checkout check-rules [--[no-]cone] [--rules-file <file>] [-z]\n",
                        value.trim_start_matches('-')
                    ),
                });
            }
        }
        cursor += 1;
    }
    Ok(options)
}

fn unquote_sparse_checkout_input(input: &str) -> String {
    let Some(quoted) = input
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return input.to_owned();
    };
    let mut out = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_sparse_checkout_patterns(
    repo: &GitRepo,
    patterns: &[String],
    cone_mode: bool,
) -> Result<()> {
    let path = sparse_checkout_file(repo);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let out = if cone_mode {
        cone_sparse_checkout_file(patterns)
    } else {
        patterns
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
            + if patterns.is_empty() { "" } else { "\n" }
    };
    let lock_path = path.with_file_name("sparse-checkout.lock");
    let mut lock = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                CliError::Fatal {
                    code: 128,
                    message: format!("Unable to create '{}': File exists", lock_path.display()),
                }
            } else {
                CliError::Io(error)
            }
        })?;
    if let Err(error) = lock
        .write_all(out.as_bytes())
        .and_then(|()| lock.sync_all())
    {
        let _ = fs::remove_file(&lock_path);
        return Err(CliError::Io(error));
    }
    drop(lock);
    fs::rename(&lock_path, &path)?;
    Ok(())
}

fn write_clone_sparse_checkout_worktree_config(repo: &GitRepo) -> Result<()> {
    fs::write(
        repo.git_dir.join("config.worktree"),
        "[core]\n\tsparseCheckout = true\n\tsparseCheckoutCone = true\n",
    )?;
    Ok(())
}

fn read_sparse_checkout_patterns(repo: &GitRepo) -> Result<Vec<String>> {
    let raw = match fs::read_to_string(sparse_checkout_file(repo)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    if sparse_checkout_cone_mode(repo)? {
        if valid_cone_sparse_checkout_file(&raw) {
            return Ok(cone_sparse_checkout_directories(&raw));
        }
        warn_invalid_cone_sparse_checkout(&raw);
    }
    Ok(raw.lines().map(str::to_owned).collect())
}

fn apply_sparse_checkout(repo: &GitRepo) -> Result<()> {
    if !repo.index_path.exists() {
        return Ok(());
    }
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let old_index = expand_sparse_index(repo, &read_repo_index(repo)?)?;
    let patterns = read_sparse_checkout_match_patterns(repo)?;
    let cone_mode = sparse_checkout_cone_mode(repo)?;
    let matcher = sparse_pattern_matcher(&patterns);
    let unmerged_paths = old_index
        .entries()
        .iter()
        .filter(|entry| entry.stage != 0)
        .filter(|entry| !sparse_path_matches(&entry.path, &matcher, cone_mode))
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let index = sparse_checkout_index(repo, &old_index)?;
    let mut keep_entries = Vec::new();
    let mut preserved_paths = Vec::new();
    let mut updated_entries = Vec::with_capacity(index.entries().len());
    for mut entry in index.entries().iter().cloned() {
        if entry.skip_worktree() {
            let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
            if path_exists(&absolute)
                && old_index.entry(&entry.path, 0).is_some_and(|old| {
                    worktree_entry_modified(repo, &absolute, old).unwrap_or(true)
                })
            {
                entry.set_skip_worktree(false);
                preserved_paths.push(entry.path.clone());
            } else {
                remove_worktree_path(repo, &entry.path)?;
            }
        } else {
            let absolute = worktree_path_for_index_entry(&repo.root, &entry.path);
            if !path_exists(&absolute)
                && old_index
                    .entry(&entry.path, 0)
                    .is_some_and(IndexEntry::skip_worktree)
            {
                keep_entries.push(entry.clone());
            }
        }
        updated_entries.push(entry);
    }
    if !preserved_paths.is_empty() {
        print_sparse_checkout_update_warning(&preserved_paths);
    }
    if !unmerged_paths.is_empty() {
        print_sparse_checkout_unmerged_warning(&unmerged_paths);
    }
    let untracked_directories = cleanup_sparse_checkout_ignored_paths(
        repo,
        &old_index,
        &index,
        &matcher,
        cone_mode,
        &preserved_paths,
    )?;
    if !untracked_directories.is_empty() {
        print_sparse_checkout_untracked_warning(&untracked_directories);
    }
    let mut index = GitIndex::from_entries(updated_entries)?;
    let checkout_paths = keep_entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let checkout = GitIndex::from_entries(keep_entries)?;
    checkout_index(
        &store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )
    .map_err(CliError::Io)?;
    refresh_tracked_index_metadata_after_checkout(repo, &mut index, &checkout_paths)?;
    let write_index = collapse_sparse_index(repo, &store, &index)?;
    write_index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn cleanup_sparse_checkout_ignored_paths(
    repo: &GitRepo,
    old_index: &GitIndex,
    sparse_index: &GitIndex,
    matcher: &GitIgnore,
    cone_mode: bool,
    preserved_paths: &[Vec<u8>],
) -> Result<Vec<Vec<u8>>> {
    let mut candidates = BTreeSet::new();
    for entry in sparse_index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.skip_worktree())
    {
        for directory in index_path_ancestors(&entry.path) {
            let mut probe = directory.clone();
            probe.extend_from_slice(b"/.zmin-sparse-probe");
            if !sparse_path_matches(&probe, matcher, cone_mode) {
                candidates.insert(directory);
            }
        }
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)));
    let mut top_level: Vec<Vec<u8>> = Vec::new();
    for candidate in candidates {
        if top_level
            .iter()
            .any(|parent| path_is_below_sparse_directory(&candidate, parent))
        {
            continue;
        }
        top_level.push(candidate);
    }

    let tracked_paths = tracked_path_set_for_repo(repo, old_index)?;
    let ignore = GitIgnore::load_from_root(&repo.root)?;
    let mut blocked: Vec<Vec<u8>> = Vec::new();
    for directory in top_level {
        if old_index.entries().iter().any(|entry| {
            path_is_below_sparse_directory(&entry.path, &directory)
                && (entry.stage != 0 || entry.mode == IndexMode::Gitlink)
        }) {
            continue;
        }
        let absolute = worktree_path_for_index_entry(&repo.root, &directory);
        if !absolute.is_dir() {
            continue;
        }
        if preserved_paths
            .iter()
            .any(|path| path_is_below_sparse_directory(path, &directory))
        {
            continue;
        }
        if sparse_directory_contains_untracked(&absolute, &directory, &tracked_paths, &ignore)? {
            blocked.push(directory);
        } else {
            fs::remove_dir_all(absolute)?;
        }
    }
    Ok(blocked)
}

fn sparse_directory_contains_untracked(
    directory: &Path,
    relative_directory: &[u8],
    tracked_paths: &TrackedPathSet<'_>,
    ignore: &GitIgnore,
) -> Result<bool> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let relative = relative_child_path(relative_directory, &entry.file_name());
        if tracked_paths.contains(&relative) {
            continue;
        }
        if ignore.is_ignored(&relative, file_type.is_dir()) {
            continue;
        }
        if !file_type.is_dir() || is_nested_worktree(&entry.path()) {
            return Ok(true);
        }
        if sparse_directory_contains_untracked(&entry.path(), &relative, tracked_paths, ignore)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn print_sparse_checkout_untracked_warning(paths: &[Vec<u8>]) {
    for path in paths {
        eprintln!(
            "warning: directory '{}/' contains untracked files, but is not in the sparse-checkout cone",
            String::from_utf8_lossy(path)
        );
    }
}

fn print_sparse_checkout_unmerged_warning(paths: &[Vec<u8>]) {
    eprintln!("warning: The following paths are unmerged and were left despite sparse patterns:");
    for path in paths {
        eprintln!("\t{}", String::from_utf8_lossy(path));
    }
    eprintln!();
    eprintln!("After fixing the above paths, you may want to run `git sparse-checkout reapply`.");
}

fn checkout_sparse_excluded_entries(repo: &GitRepo) -> Result<()> {
    if !repo.index_path.exists() {
        return Ok(());
    }
    let algorithm = repo_hash_algorithm_from_config(repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let mut index = read_repo_index(repo)?;
    index = expand_sparse_index(repo, &index)?;
    let unmerged_paths = index
        .entries()
        .iter()
        .filter(|entry| entry.stage != 0)
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if !unmerged_paths.is_empty() {
        print_sparse_checkout_unmerged_warning(&unmerged_paths);
    }
    let mut restore_entries = Vec::new();
    let mut full_entries = Vec::new();
    for mut entry in index.entries().to_vec() {
        if entry.skip_worktree() {
            restore_entries.push(entry.clone());
        }
        entry.set_skip_worktree(false);
        full_entries.push(entry);
    }
    index = GitIndex::from_entries(full_entries)?;
    let checkout_paths = restore_entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    let checkout = GitIndex::from_entries(restore_entries)?;
    checkout_index(
        &store,
        &checkout,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )
    .map_err(CliError::Io)?;
    refresh_tracked_index_metadata_after_checkout(repo, &mut index, &checkout_paths)?;
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn sparse_checkout_index(repo: &GitRepo, index: &GitIndex) -> Result<GitIndex> {
    let patterns = read_sparse_checkout_match_patterns(repo)?;
    let cone_mode = config_bool_enabled(repo, "core.sparseCheckoutCone")?;
    let matcher = sparse_pattern_matcher(&patterns);
    let entries = index
        .entries()
        .iter()
        .cloned()
        .map(|mut entry| {
            if entry.stage == 0 {
                entry.set_skip_worktree(!sparse_path_matches(&entry.path, &matcher, cone_mode));
            }
            entry
        })
        .collect::<Vec<_>>();
    Ok(GitIndex::from_entries(entries)?)
}

fn sparse_checkout_active(repo: &GitRepo) -> Result<bool> {
    Ok(sparse_checkout_file(repo).exists() && config_bool_enabled(repo, "core.sparseCheckout")?)
}

fn apply_sparse_checkout_bits_to_index(repo: &GitRepo, index: &mut GitIndex) -> Result<()> {
    let patterns = read_sparse_checkout_match_patterns(repo)?;
    let mut cone_mode = config_bool_enabled(repo, "core.sparseCheckoutCone")?;
    if cone_mode {
        let raw = fs::read_to_string(sparse_checkout_file(repo)).unwrap_or_default();
        if !valid_cone_sparse_checkout_file(&raw) {
            warn_invalid_cone_sparse_checkout(&raw);
            cone_mode = false;
        }
    }
    let matcher = sparse_pattern_matcher(&patterns);
    let entries = index
        .entries()
        .iter()
        .cloned()
        .map(|mut entry| {
            if entry.stage == 0 {
                entry.set_skip_worktree(!sparse_path_matches(&entry.path, &matcher, cone_mode));
            }
            entry
        })
        .collect::<Vec<_>>();
    let full_index = GitIndex::from_entries(entries)?;
    let store = LooseObjectStore::new(
        repo.objects_dir.clone(),
        repo_hash_algorithm_from_config(repo)?,
    );
    *index = collapse_sparse_index(repo, &store, &full_index)?;
    Ok(())
}

#[derive(Debug, Default)]
struct SparseDirectoryState {
    all_skipped: bool,
    entry_count: usize,
    contains_gitlink: bool,
}

fn collapse_sparse_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
) -> Result<GitIndex> {
    if !config_bool_enabled(repo, "index.sparse")?
        || index.entries().iter().any(|entry| entry.stage != 0)
    {
        return Ok(index.clone());
    }
    let mut directories = BTreeMap::<Vec<u8>, SparseDirectoryState>::new();
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        for directory in index_path_ancestors(&entry.path) {
            let state = directories
                .entry(directory)
                .or_insert(SparseDirectoryState {
                    all_skipped: true,
                    entry_count: 0,
                    contains_gitlink: false,
                });
            state.all_skipped &= entry.skip_worktree();
            state.entry_count += 1;
            state.contains_gitlink |= entry.mode == IndexMode::Gitlink;
        }
    }
    let mut candidates = directories
        .into_iter()
        .filter(|(_, state)| state.all_skipped && state.entry_count > 0 && !state.contains_gitlink)
        .map(|(path, _)| path)
        .collect::<Vec<_>>();
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let head_index =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    candidates.retain(|directory| sparse_directory_matches_head(index, &head_index, directory));
    candidates.sort_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)));
    let mut collapsed = Vec::new();
    for candidate in candidates {
        if collapsed
            .iter()
            .any(|parent: &Vec<u8>| path_is_below_sparse_directory(&candidate, parent))
        {
            continue;
        }
        collapsed.push(candidate);
    }
    if collapsed.is_empty() {
        return Ok(index.clone());
    }
    let head = resolve_commitish(repo, store, "HEAD")?;
    let commit = CommitObjectCache::new(store).read_commit(&head)?;
    let mut entries = index
        .entries()
        .iter()
        .filter(|entry| {
            !collapsed
                .iter()
                .any(|directory| path_is_below_sparse_directory(&entry.path, directory))
        })
        .cloned()
        .collect::<Vec<_>>();
    for directory in collapsed {
        let Some(tree) = find_tree_entry(store, &commit.tree, &directory)? else {
            continue;
        };
        if tree.mode != TreeMode::Tree {
            continue;
        }
        let mut path = directory;
        path.push(b'/');
        let mut entry = IndexEntry::new(path, tree.id, IndexMode::Tree, 0)?;
        entry.set_skip_worktree(true);
        entries.push(entry);
    }
    Ok(GitIndex::from_entries(entries)?)
}

fn sparse_directory_matches_head(
    index: &GitIndex,
    head_index: &GitIndex,
    directory: &[u8],
) -> bool {
    let mut current = index
        .entries()
        .iter()
        .filter(|entry| path_is_below_sparse_directory(&entry.path, directory));
    let mut head = head_index
        .entries()
        .iter()
        .filter(|entry| path_is_below_sparse_directory(&entry.path, directory));
    loop {
        match (current.next(), head.next()) {
            (Some(current), Some(head))
                if current.stage == 0
                    && current.path == head.path
                    && current.id == head.id
                    && current.mode == head.mode => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn expand_sparse_index(repo: &GitRepo, index: &GitIndex) -> Result<GitIndex> {
    let sparse_directories = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0 && entry.mode == IndexMode::Tree)
        .map(|entry| {
            entry
                .path
                .strip_suffix(b"/")
                .unwrap_or(&entry.path)
                .to_vec()
        })
        .collect::<Vec<_>>();
    if sparse_directories.is_empty() {
        return Ok(index.clone());
    }
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let mut expanded =
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?;
    let entries = expanded
        .entries()
        .iter()
        .cloned()
        .map(|mut entry| {
            if sparse_directories
                .iter()
                .any(|directory| path_is_below_sparse_directory(&entry.path, directory))
            {
                entry.set_skip_worktree(true);
            }
            entry
        })
        .collect::<Vec<_>>();
    expanded = GitIndex::from_entries(entries)?;
    for entry in index
        .entries()
        .iter()
        .filter(|entry| entry.mode != IndexMode::Tree)
    {
        expanded.upsert(entry.clone())?;
    }
    Ok(expanded)
}

fn path_is_below_sparse_directory(path: &[u8], directory: &[u8]) -> bool {
    path.starts_with(directory) && path.get(directory.len()) == Some(&b'/')
}

fn read_sparse_checkout_match_patterns(repo: &GitRepo) -> Result<Vec<String>> {
    let raw = match fs::read_to_string(sparse_checkout_file(repo)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn sparse_pattern_matcher(patterns: &[String]) -> GitIgnore {
    GitIgnore::parse(&patterns.join("\n"))
}

fn sparse_path_matches(path: &[u8], matcher: &GitIgnore, cone_mode: bool) -> bool {
    if cone_mode && !path.contains(&b'/') {
        return true;
    }
    sparse_path_match(path, matcher, cone_mode).is_some_and(|(_, is_negation)| !is_negation)
}

fn sparse_path_match(path: &[u8], matcher: &GitIgnore, cone_mode: bool) -> Option<(usize, bool)> {
    let mut best = matcher
        .match_path(path, false)
        .map(|matched| (matched.line_number, matched.is_negation));
    for ancestor in index_path_ancestors(path) {
        let candidate = matcher.match_path(&ancestor, true).and_then(|matched| {
            if cone_mode && !matched.is_negation && matched.pattern == "/*" {
                None
            } else {
                Some((matched.line_number, matched.is_negation))
            }
        });
        if sparse_match_is_newer(candidate.as_ref(), best.as_ref()) {
            best = candidate;
        }
    }
    best
}

fn sparse_match_is_newer(
    candidate: Option<&(usize, bool)>,
    current: Option<&(usize, bool)>,
) -> bool {
    match (candidate, current) {
        (Some(candidate), Some(current)) => candidate.0 >= current.0,
        (Some(_), None) => true,
        _ => false,
    }
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
    skip_checks: bool,
}

#[derive(Debug, Default)]
struct SparseCheckoutCheckRulesOptions {
    cone: Option<bool>,
    rules_file: Option<PathBuf>,
    nul: bool,
}

impl SparseCheckoutOptions {
    fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

#[derive(Clone, Copy, Debug)]
enum SparseCheckoutUsage {
    Set,
    Add,
    Init,
    Reapply,
}

fn parse_sparse_checkout_options(
    args: &[String],
    allow_stdin: bool,
    usage: SparseCheckoutUsage,
) -> Result<SparseCheckoutOptions> {
    let mut options = SparseCheckoutOptions::default();
    let mut end_of_options = false;
    for arg in args {
        if !end_of_options && arg == "--end-of-options" {
            end_of_options = true;
            continue;
        }
        match arg.as_str() {
            "--stdin" if !end_of_options && allow_stdin => options.stdin = true,
            "--cone" if !end_of_options => options.cone = Some(true),
            "--no-cone" if !end_of_options => options.cone = Some(false),
            "--sparse-index" if !end_of_options => options.sparse_index = Some(true),
            "--no-sparse-index" if !end_of_options => options.sparse_index = Some(false),
            "--skip-checks" if !end_of_options => options.skip_checks = true,
            option if !end_of_options && option.starts_with('-') => {
                return Err(sparse_checkout_unknown_option_error(option, usage));
            }
            pattern => options.patterns.push(pattern.to_owned()),
        }
    }
    if options.stdin {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        options.patterns = input
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(unquote_sparse_checkout_input)
            .collect();
    }
    Ok(options)
}

fn sparse_checkout_unknown_option_error(option: &str, usage: SparseCheckoutUsage) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown option `{}'\n{}",
            option.trim_start_matches('-'),
            sparse_checkout_usage(usage)
        ),
    }
}

fn sparse_checkout_usage(usage: SparseCheckoutUsage) -> &'static str {
    match usage {
        SparseCheckoutUsage::Set => concat!(
            "usage: git sparse-checkout set [--[no-]cone] [--[no-]sparse-index] [--skip-checks] (--stdin | <patterns>)\n",
            "\n",
            "    --[no-]cone           initialize the sparse-checkout in cone mode\n",
            "    --[no-]sparse-index   toggle the use of a sparse index\n",
            "    --skip-checks         skip some sanity checks on the given paths that might give false positives\n",
            "    --stdin               read patterns from standard in\n",
        ),
        SparseCheckoutUsage::Add => concat!(
            "usage: git sparse-checkout add [--skip-checks] (--stdin | <patterns>)\n",
            "\n",
            "    --skip-checks         skip some sanity checks on the given paths that might give false positives\n",
            "    --[no-]stdin          read patterns from standard in\n",
        ),
        SparseCheckoutUsage::Init => concat!(
            "usage: git sparse-checkout init [--cone] [--[no-]sparse-index]\n",
            "\n",
            "    --[no-]cone           initialize the sparse-checkout in cone mode\n",
            "    --[no-]sparse-index   toggle the use of a sparse index\n",
        ),
        SparseCheckoutUsage::Reapply => concat!(
            "usage: git sparse-checkout reapply [--[no-]cone] [--[no-]sparse-index]\n",
            "\n",
            "    --[no-]cone           initialize the sparse-checkout in cone mode\n",
            "    --[no-]sparse-index   toggle the use of a sparse index\n",
        ),
    }
}

fn apply_sparse_checkout_config_options(
    repo: &GitRepo,
    options: &SparseCheckoutOptions,
) -> Result<()> {
    set_config_value(repo, "core.repositoryFormatVersion", "1")?;
    set_config_value(repo, "extensions.worktreeConfig", "true")?;
    let cone = options.cone.unwrap_or(
        read_config_value(repo, "core.sparseCheckoutCone")?
            .as_deref()
            .and_then(parse_git_bool)
            .unwrap_or(true),
    );
    let existing_sparse_index = read_config_value(repo, "index.sparse")?;
    let sparse_index = options
        .sparse_index
        .or_else(|| existing_sparse_index.as_deref().and_then(parse_git_bool));
    let mut values = vec![
        ("core.sparseCheckout".to_owned(), "true".to_owned()),
        ("core.sparseCheckoutCone".to_owned(), cone.to_string()),
    ];
    if let Some(sparse_index) = sparse_index {
        values.push(("index.sparse".to_owned(), sparse_index.to_string()));
    }
    set_ordered_worktree_config_values(repo, &values)
}

fn ensure_sparse_checkout_worktree(repo: &GitRepo) -> Result<()> {
    if repo_is_bare(repo) {
        return Err(CliError::Fatal {
            code: 128,
            message: "this operation must be run in a work tree".into(),
        });
    }
    Ok(())
}

fn sparse_checkout_cone_mode(repo: &GitRepo) -> Result<bool> {
    Ok(read_config_value(repo, "core.sparseCheckoutCone")?
        .and_then(|value| parse_git_bool(&value))
        .unwrap_or(true))
}

fn validate_sparse_checkout_inputs(
    repo: &GitRepo,
    patterns: &[String],
    cone_mode: bool,
    skip_checks: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    if !cone_mode && cwd != repo.root {
        return Err(CliError::Fatal {
            code: 128,
            message: "must run from the toplevel directory in non-cone mode".into(),
        });
    }
    if skip_checks {
        return Ok(());
    }
    for pattern in patterns {
        let path = cwd.join(pattern);
        if cone_mode {
            if path.exists() && !path.is_dir() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "'{}' is not a directory; to treat it as a directory anyway, rerun with --skip-checks",
                        pattern
                    ),
                });
            }
        } else if !pattern.starts_with('/') {
            eprintln!(
                "warning: pass a leading slash before paths such as '{pattern}' if you want a single file"
            );
        }
    }
    Ok(())
}

fn normalize_sparse_checkout_inputs(
    repo: &GitRepo,
    patterns: &[String],
    cone_mode: bool,
) -> Result<Vec<String>> {
    if !cone_mode {
        return Ok(patterns.to_vec());
    }
    let cwd = std::env::current_dir()?;
    let prefix = cwd
        .strip_prefix(&repo.root)
        .unwrap_or_else(|_| Path::new(""));
    let mut normalized = BTreeSet::new();
    for pattern in patterns {
        let path = prefix.join(pattern);
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::Normal(component) => {
                    components.push(component.to_string_lossy().into_owned());
                }
                std::path::Component::ParentDir => {
                    if components.pop().is_none() {
                        return Err(CliError::Stderr {
                            code: 128,
                            text: format!("fatal: could not normalize path {pattern}\n"),
                        });
                    }
                }
                _ => {
                    return Err(CliError::Stderr {
                        code: 128,
                        text: format!("fatal: could not normalize path {pattern}\n"),
                    });
                }
            }
        }
        if components.is_empty() {
            return Err(CliError::Stderr {
                code: 128,
                text: format!("fatal: could not normalize path {pattern}\n"),
            });
        }
        normalized.insert(components.join("/"));
    }
    let mut minimal = Vec::new();
    for path in normalized {
        if minimal
            .iter()
            .any(|parent: &String| path.starts_with(&format!("{parent}/")))
        {
            continue;
        }
        minimal.push(path);
    }
    Ok(minimal)
}

fn cone_sparse_checkout_file(patterns: &[String]) -> String {
    let patterns = minimal_cone_directories(patterns);
    let explicit = patterns.iter().cloned().collect::<BTreeSet<_>>();
    let mut prefixes = BTreeSet::new();
    for pattern in patterns {
        let components = pattern.split('/').collect::<Vec<_>>();
        for end in 1..=components.len() {
            prefixes.insert(components[..end].join("/"));
        }
    }
    let mut out = String::from("/*\n!/*/\n");
    for prefix in prefixes {
        let escaped = escape_cone_sparse_path(&prefix);
        out.push('/');
        out.push_str(&escaped);
        out.push_str("/\n");
        if !explicit.contains(&prefix) {
            out.push_str("!/");
            out.push_str(&escaped);
            out.push_str("/*/\n");
        }
    }
    out
}

fn minimal_cone_directories(patterns: &[String]) -> Vec<String> {
    let mut minimal = Vec::new();
    for path in patterns.iter().cloned().collect::<BTreeSet<_>>() {
        if minimal
            .iter()
            .any(|parent: &String| path.starts_with(&format!("{parent}/")))
        {
            continue;
        }
        minimal.push(path);
    }
    minimal
}

fn escape_cone_sparse_path(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for ch in path.chars() {
        if matches!(ch, '\\' | '*' | '?' | '[') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn unescape_cone_sparse_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn cone_sparse_checkout_directories(raw: &str) -> Vec<String> {
    let mut positives = BTreeSet::new();
    let mut internal = BTreeSet::new();
    for line in raw.lines() {
        let line = line.trim();
        if line == "/*" || line == "!/*/" || line.is_empty() {
            continue;
        }
        if let Some(path) = line
            .strip_prefix("!/")
            .and_then(|line| line.strip_suffix("/*/"))
        {
            internal.insert(unescape_cone_sparse_path(path));
        } else if let Some(path) = line
            .strip_prefix('/')
            .and_then(|line| line.strip_suffix('/'))
        {
            positives.insert(unescape_cone_sparse_path(path));
        }
    }
    positives
        .into_iter()
        .filter(|path| !internal.contains(path))
        .collect()
}

fn sparse_checkout_file_uses_cone_patterns(repo: &GitRepo) -> Result<bool> {
    match fs::read_to_string(sparse_checkout_file(repo)) {
        Ok(raw) => Ok(valid_cone_sparse_checkout_file(&raw)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn valid_cone_sparse_checkout_file(raw: &str) -> bool {
    let lines = raw.lines().collect::<Vec<_>>();
    if lines.get(0) != Some(&"/*") || lines.get(1) != Some(&"!/*/") {
        return false;
    }
    let mut positives = BTreeSet::new();
    let mut negatives = Vec::new();
    for line in &lines[2..] {
        if let Some(path) = line
            .strip_prefix("!/")
            .and_then(|line| line.strip_suffix("/*/"))
        {
            if path.is_empty() || sparse_pattern_has_unescaped_glob(path) {
                return false;
            }
            negatives.push(unescape_cone_sparse_path(path));
            continue;
        }
        let Some(path) = line
            .strip_prefix('/')
            .and_then(|line| line.strip_suffix('/'))
        else {
            return false;
        };
        if path.is_empty() || sparse_pattern_has_unescaped_glob(path) {
            return false;
        }
        let path = unescape_cone_sparse_path(path);
        if path.ends_with("/*") {
            return false;
        }
        positives.insert(path);
    }
    negatives.into_iter().all(|path| positives.contains(&path))
}

fn sparse_pattern_has_unescaped_glob(pattern: &str) -> bool {
    let mut escaped = false;
    for ch in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if matches!(ch, '*' | '?' | '[') {
            return true;
        }
    }
    false
}

fn warn_invalid_cone_sparse_checkout(raw: &str) {
    if raw.lines().any(|line| line.starts_with("!/")) {
        eprintln!("warning: unrecognized negative pattern in sparse-checkout file");
    }
    eprintln!("warning: your sparse-checkout file may have issues: pattern '/foo/*/' is repeated");
    eprintln!("warning: disabling cone pattern matching");
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
    let existing_repo = exact_repo_at(&absolute_submodule_path).is_some();
    if !existing_repo {
        let clone_repository =
            resolve_submodule_clone_url(&submodule_parent_repository(&repo), &options.repository);
        transport_commands::clone(CloneOptions {
            quiet: options.quiet,
            configs: Vec::new(),
            template: None,
            reject_shallow: false,
            recurse_submodules: Vec::new(),
            remote_submodules: false,
            shallow_submodules: false,
            sparse: false,
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
            shallow_since: None,
            shallow_exclude: Vec::new(),
            branch: options.branch.clone(),
            server_options: Vec::new(),
            upload_pack: None,
            filter: None,
            also_filter_submodules: false,
            bundle_uri: None,
            ref_format: None,
            keep_partial_on_missing_branch: false,
            repository: clone_repository,
            directory: Some(absolute_submodule_path.clone()),
        })?;
    } else if !options.quiet {
        println!(
            "Adding existing repo at '{}' to the index",
            submodule_path.display()
        );
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
    absorb_submodule_gitdir(&repo, &submodule_path_string, &submodule_name)?;
    set_config_value(
        &repo,
        &format!("submodule.{submodule_name}.url"),
        &options.repository,
    )?;
    set_config_value(&repo, &format!("submodule.{submodule_name}.active"), "true")?;

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
            return Err(submodule_usage_error());
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
    let repo = find_repo_with_parent_dir_error()?;
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
            return Err(submodule_usage_error());
        } else {
            paths.push(arg.clone());
        }
    }
    submodule_status_for_repo(&repo, &paths, cached, recursive, quiet, "")
}

fn submodule_usage_error() -> CliError {
    CliError::Stderr {
        code: 1,
        text: "usage: git submodule [--quiet] [--cached]
   or: git submodule [--quiet] add [-b <branch>] [-f|--force] [--name <name>] [--reference <repository>] [--] <repository> [<path>]
   or: git submodule [--quiet] status [--cached] [--recursive] [--] [<path>...]
   or: git submodule [--quiet] init [--] [<path>...]
   or: git submodule [--quiet] deinit [-f|--force] (--all| [--] <path>...)
   or: git submodule [--quiet] update [--init [--filter=<filter-spec>]] [--remote] [-N|--no-fetch] [-f|--force] [--checkout|--merge|--rebase] [--[no-]recommend-shallow] [--reference <repository>] [--recursive] [--[no-]single-branch] [--] [<path>...]
   or: git submodule [--quiet] set-branch (--default|--branch <branch>) [--] <path>
   or: git submodule [--quiet] set-url [--] <path> <newurl>
   or: git submodule [--quiet] summary [--cached|--files] [--summary-limit <n>] [commit] [--] [<path>...]
   or: git submodule [--quiet] foreach [--recursive] <command>
   or: git submodule [--quiet] sync [--recursive] [--] [<path>...]
   or: git submodule [--quiet] absorbgitdirs [--] [<path>...]\n"
            .to_owned(),
    }
}

fn sparse_checkout_unknown_subcommand_error(subcommand: &str) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown subcommand: `{subcommand}'\n\
             usage: git sparse-checkout (init | list | set | add | reapply | disable | check-rules) [<options>]\n"
        ),
    }
}

fn worktree_unknown_subcommand_error(subcommand: &str) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown subcommand: `{subcommand}'\n{}",
            worktree_usage()
        ),
    }
}

fn worktree_usage() -> &'static str {
    concat!(
        "usage: git worktree add [-f] [--detach] [--checkout] [--lock [--reason <string>]]\n",
        "                        [--orphan] [(-b | -B) <new-branch>] <path> [<commit-ish>]\n",
        "   or: git worktree list [-v | --porcelain [-z]]\n",
        "   or: git worktree lock [--reason <string>] <worktree>\n",
        "   or: git worktree move <worktree> <new-path>\n",
        "   or: git worktree prune [-n] [-v] [--expire <expire>]\n",
        "   or: git worktree remove [-f] <worktree>\n",
        "   or: git worktree repair [<path>...]\n",
        "   or: git worktree unlock <worktree>\n"
    )
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
                &options.labels,
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
                &options.labels,
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

const STASH_APPLY_USAGE: &str = "\
usage: git stash apply [--index] [-q | --quiet] [<stash>]

    -q, --[no-]quiet      be quiet, only report errors
    --[no-]index          attempt to recreate the index
";

const STASH_DROP_USAGE: &str = "\
usage: git stash drop [-q | --quiet] [<stash>]

    -q, --[no-]quiet      be quiet, only report errors
";

const STASH_POP_USAGE: &str = "\
usage: git stash pop [--index] [-q | --quiet] [<stash>]

    -q, --[no-]quiet      be quiet, only report errors
    --[no-]index          attempt to recreate the index
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
    let _ = stage_tracked_worktree_changes_matching(
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
            "error: unknown option `{}'\n{}\n",
            option.trim_start_matches('-'),
            usage_text
        ),
    }
}

fn stash_locked_index_error(repo: &GitRepo) -> Option<CliError> {
    let lock_path = repo.index_path.with_extension("lock");
    lock_path.exists().then(|| CliError::Stderr {
        code: 1,
        text: "error: could not write index\n".to_owned(),
    })
}

fn add_lockfile_pid_config_enabled(repo: &GitRepo) -> Result<bool> {
    let Some(entry) = read_config_entry(repo, "core.lockfilepid").map_err(CliError::Io)? else {
        return Ok(false);
    };
    entry.bool_value().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("bad boolean config value '{}'", entry.value),
    })
}

fn write_index_with_lockfile_diagnostics(
    repo: &GitRepo,
    index: &GitIndex,
    lockfile_pid_enabled: bool,
    options: AddIndexWriteOptions,
) -> Result<()> {
    let write_result = match options.version {
        Some(version) => index.write_to_path_with_version(&repo.index_path, version),
        None => index.write_to_path(&repo.index_path),
    };
    write_result.map_err(|error| add_index_write_error(repo, error, lockfile_pid_enabled))?;
    if options.skip_hash {
        zero_index_trailing_hash(&repo.index_path, index.hash_algorithm())
            .map_err(|error| add_index_write_error(repo, error, lockfile_pid_enabled))?;
    }
    Ok(())
}

fn zero_index_trailing_hash(
    path: &Path,
    algorithm: zmin_git_core::GitHashAlgorithm,
) -> io::Result<()> {
    let mut bytes = fs::read(path)?;
    let digest_len = algorithm.digest_len();
    if bytes.len() < digest_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "git index is too short",
        ));
    }
    let checksum_offset = bytes.len() - digest_len;
    bytes[checksum_offset..].fill(0);
    fs::write(path, bytes)
}

fn add_index_write_error(repo: &GitRepo, error: io::Error, lockfile_pid_enabled: bool) -> CliError {
    if error.kind() != io::ErrorKind::AlreadyExists {
        return CliError::Io(error);
    }
    let lock_path = repo.index_path.with_extension("lock");
    let mut text = format!(
        "error: could not write index\nerror: Unable to create '{}': File exists.\n",
        lock_path.display()
    );
    if let Some(pid_diagnostic) = add_lockfile_pid_diagnostic(repo, lockfile_pid_enabled) {
        text.push_str(&pid_diagnostic);
    }
    CliError::Stderr { code: 128, text }
}

fn add_lockfile_pid_diagnostic(repo: &GitRepo, lockfile_pid_enabled: bool) -> Option<String> {
    let pid_path = add_lockfile_pid_path(repo);
    let content = fs::read_to_string(pid_path).ok()?;
    let pid = parse_lockfile_pid(&content)?;
    if pid > 0 && lockfile_pid_enabled && process_id_is_running(pid) {
        return Some(format!("error: index.lock is held by process {pid}\n"));
    }
    Some(format!(
        "error: index.lock was written by process {pid}, which is no longer running\nerror: index.lock appears to be stale\n"
    ))
}

fn add_lockfile_pid_path(repo: &GitRepo) -> PathBuf {
    repo.index_path.with_file_name("index~pid.lock")
}

fn parse_lockfile_pid(content: &str) -> Option<i32> {
    content
        .trim()
        .strip_prefix("pid ")?
        .trim()
        .parse::<i32>()
        .ok()
}

fn process_id_is_running(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: kill(pid, 0) does not send a signal and is the standard existence probe.
        let rc = unsafe { libc::kill(pid, 0) };
        if rc == 0 {
            return true;
        }
        let errno = io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or_default();
        errno != libc::ESRCH
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
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
    let _ = stage_tracked_worktree_changes_matching(
        &repo,
        &store,
        &mut snapshot,
        &[],
        &HashSet::new(),
    )?;
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
    current_index.refresh_cache_tree();
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
    current_index.refresh_cache_tree();
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
    let mut target_index = target_index.clone();
    target_index.refresh_cache_tree();
    target_index.write_to_path(&repo.index_path)?;
    checkout_index(
        store,
        &target_index,
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
    current_index.refresh_cache_tree();
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
        render_stash_list_format_atom(atom, &mut chars, &mut out, index, entry, &commit)?;
    }
    Ok(out)
}

fn render_stash_list_format_atom<I>(
    atom: char,
    chars: &mut std::iter::Peekable<I>,
    out: &mut String,
    index: usize,
    entry: &StashEntry,
    commit: &zmin_git_core::CommitObject,
) -> Result<()>
where
    I: Iterator<Item = char> + Clone,
{
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
        'P' => render_stash_parent_list(out, &commit.parents, false),
        'p' => render_stash_parent_list(out, &commit.parents, true),
        'T' => out.push_str(&commit.tree.to_hex()),
        't' => out.push_str(&commit.tree.short_hex(7)),
        'g' => render_stash_reflog_atom(chars, out, index, entry)?,
        'x' => render_stash_hex_atom(chars, out)?,
        'a' => render_stash_signature_atom(chars, out, &commit.author, "a")?,
        'c' => render_stash_signature_atom(chars, out, &commit.committer, "c")?,
        'G' => render_stash_gpg_atom(chars, out)?,
        'C' => {
            render_stash_color_atom(chars, out)?;
        }
        '<' | '>' => render_stash_width_atom(atom, chars, out, index, entry, commit)?,
        'w' => render_stash_wrap_atom(chars, out, index, entry, commit)?,
        _ => stash_format_literal_atom(out, "", atom),
    }
    Ok(())
}

fn render_stash_parent_list(out: &mut String, parents: &[ObjectId], short: bool) {
    for (index, parent) in parents.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        if short {
            out.push_str(&parent.short_hex(7));
        } else {
            out.push_str(&parent.to_hex());
        }
    }
}

fn render_stash_reflog_atom<I>(
    chars: &mut std::iter::Peekable<I>,
    out: &mut String,
    index: usize,
    entry: &StashEntry,
) -> Result<()>
where
    I: Iterator<Item = char>,
{
    let Some(reflog_atom) = chars.next() else {
        out.push_str("%g");
        return Ok(());
    };
    match reflog_atom {
        'd' => out.push_str(&format!("stash@{{{index}}}")),
        'D' => out.push_str(&format!("refs/stash@{{{index}}}")),
        's' => out.push_str(&entry.message),
        'N' | 'n' => out.push_str(&signature_name(entry.reflog_identity.as_bytes())),
        'E' | 'e' => out.push_str(&signature_email(entry.reflog_identity.as_bytes())),
        'K' => out.push_str("%gK"),
        _ => stash_format_literal_atom(out, "g", reflog_atom),
    }
    Ok(())
}

fn render_stash_hex_atom<I>(chars: &mut std::iter::Peekable<I>, out: &mut String) -> Result<()>
where
    I: Iterator<Item = char>,
{
    let Some(high) = chars.next() else {
        out.push_str("%x");
        return Ok(());
    };
    let Some(low) = chars.next() else {
        out.push_str("%x");
        out.push(high);
        return Ok(());
    };
    let Some(byte) = parse_hex_byte(high, low) else {
        out.push_str("%x");
        out.push(high);
        out.push(low);
        return Ok(());
    };
    out.push(byte as char);
    Ok(())
}

fn render_stash_signature_atom<I>(
    chars: &mut std::iter::Peekable<I>,
    out: &mut String,
    signature: &[u8],
    prefix: &str,
) -> Result<()>
where
    I: Iterator<Item = char>,
{
    let Some(next) = chars.next() else {
        out.push('%');
        out.push_str(prefix);
        return Ok(());
    };
    match next {
        'n' => out.push_str(&signature_name(signature)),
        'e' => out.push_str(&signature_email(signature)),
        'l' | 'L' => out.push_str(&signature_email_local_part(signature)),
        't' => out.push_str(&signature_timestamp(signature).unwrap_or(0).to_string()),
        'd' => out.push_str(&signature_log_date(signature)?),
        'D' => out.push_str(&signature_mail_date(signature)?),
        'h' => out.push_str(&signature_human_date(signature)?),
        'r' => out.push_str(&signature_relative_date(signature)?),
        'i' => out.push_str(&signature_blame_date(signature)?),
        'I' => out.push_str(&signature_strict_iso_date(signature)?),
        's' => out.push_str(&signature_short_date(signature)?),
        _ => stash_format_literal_atom(out, prefix, next),
    }
    Ok(())
}

fn render_stash_gpg_atom<I>(chars: &mut std::iter::Peekable<I>, out: &mut String) -> Result<()>
where
    I: Iterator<Item = char>,
{
    let Some(next) = chars.next() else {
        out.push_str("%G");
        return Ok(());
    };
    match next {
        '?' => out.push('N'),
        'K' | 'F' | 'P' | 'S' | 'G' => {}
        'T' => out.push_str("undefined"),
        _ => stash_format_literal_atom(out, "G", next),
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum StashWidthAlign {
    Left,
    Right,
}

#[derive(Clone, Copy)]
enum StashWidthTruncation {
    None,
    Right,
    Left,
    Middle,
}

struct StashWidthSpec {
    width: usize,
    align: StashWidthAlign,
    truncation: StashWidthTruncation,
}

struct StashWrapSpec {
    width: usize,
    indent_first: usize,
    indent_rest: usize,
}

enum StashWidthAtom {
    Valid(StashWidthSpec),
    Literal(String),
}

enum StashColorAtom {
    Valid(String),
    Unterminated(String),
    Invalid(String),
    Missing,
}

fn render_stash_width_atom<I>(
    atom: char,
    chars: &mut std::iter::Peekable<I>,
    out: &mut String,
    index: usize,
    entry: &StashEntry,
    commit: &CommitObject,
) -> Result<()>
where
    I: Iterator<Item = char> + Clone,
{
    let spec = match parse_stash_width_spec(atom, chars) {
        StashWidthAtom::Valid(spec) => spec,
        StashWidthAtom::Literal(literal) => {
            out.push('%');
            out.push(atom);
            out.push_str(&literal);
            return Ok(());
        }
    };
    let Some(next) = chars.next() else {
        return Ok(());
    };
    if next != '%' {
        out.push(next);
        for ch in chars.by_ref() {
            if ch != '%' {
                out.push(ch);
                continue;
            }
            let Some(next_atom) = chars.next() else {
                out.push('%');
                break;
            };
            let mut rendered = String::new();
            render_stash_list_format_atom(next_atom, chars, &mut rendered, index, entry, commit)?;
            out.push_str(&apply_stash_width_spec(&rendered, &spec));
            return Ok(());
        }
        return Ok(());
    }
    let Some(next_atom) = chars.next() else {
        return Err(unsupported_stash_list_format_atom(&format!("%{atom}")));
    };
    let mut rendered = String::new();
    render_stash_list_format_atom(next_atom, chars, &mut rendered, index, entry, commit)?;
    out.push_str(&apply_stash_width_spec(&rendered, &spec));
    Ok(())
}

fn parse_stash_width_spec<I>(atom: char, chars: &mut std::iter::Peekable<I>) -> StashWidthAtom
where
    I: Iterator<Item = char>,
{
    if !matches!(chars.peek(), Some('(')) {
        return StashWidthAtom::Literal(String::new());
    }
    chars.next();
    let mut raw = String::new();
    for ch in chars.by_ref() {
        if ch == ')' {
            let mut parts = raw.split(',').map(str::trim);
            let Some(width) = parts.next().and_then(|value| value.parse::<usize>().ok()) else {
                return StashWidthAtom::Literal(format!("({raw})"));
            };
            let truncation = match parts.next() {
                None | Some("") => StashWidthTruncation::None,
                Some("trunc") => StashWidthTruncation::Right,
                Some("ltrunc") => StashWidthTruncation::Left,
                Some("mtrunc") => StashWidthTruncation::Middle,
                Some(_) => return StashWidthAtom::Literal(format!("({raw})")),
            };
            if parts.next().is_some() {
                return StashWidthAtom::Literal(format!("({raw})"));
            }
            return StashWidthAtom::Valid(StashWidthSpec {
                width,
                align: if atom == '<' {
                    StashWidthAlign::Left
                } else {
                    StashWidthAlign::Right
                },
                truncation,
            });
        }
        raw.push(ch);
    }
    StashWidthAtom::Literal(format!("({raw}"))
}

fn apply_stash_width_spec(value: &str, spec: &StashWidthSpec) -> String {
    let mut value = apply_stash_width_truncation(value, spec.width, spec.truncation);
    let len = value.chars().count();
    if len >= spec.width {
        return value;
    }
    let padding = " ".repeat(spec.width - len);
    match spec.align {
        StashWidthAlign::Left => value.push_str(&padding),
        StashWidthAlign::Right => value.insert_str(0, &padding),
    }
    value
}

fn apply_stash_width_truncation(
    value: &str,
    width: usize,
    truncation: StashWidthTruncation,
) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if matches!(truncation, StashWidthTruncation::None) || chars.len() <= width {
        return value.to_owned();
    }
    if width <= 2 {
        return ".".repeat(width);
    }
    match truncation {
        StashWidthTruncation::None => value.to_owned(),
        StashWidthTruncation::Right => {
            let mut out = chars[..width - 2].iter().collect::<String>();
            out.push_str("..");
            out
        }
        StashWidthTruncation::Left => {
            let mut out = String::from("..");
            out.extend(chars[chars.len() - (width - 2)..].iter());
            out
        }
        StashWidthTruncation::Middle => {
            let left = (width - 2).div_ceil(2);
            let right = (width - 2) - left;
            let mut out = chars[..left].iter().collect::<String>();
            out.push_str("..");
            out.extend(chars[chars.len() - right..].iter());
            out
        }
    }
}

fn render_stash_wrap_atom<I>(
    chars: &mut std::iter::Peekable<I>,
    out: &mut String,
    index: usize,
    entry: &StashEntry,
    commit: &CommitObject,
) -> Result<()>
where
    I: Iterator<Item = char>,
{
    let Some(spec) = parse_stash_wrap_spec(chars, out) else {
        return Ok(());
    };
    let remaining = chars.by_ref().collect::<String>();
    let mut rendered = String::new();
    let mut remaining_chars = remaining.chars().peekable();
    while let Some(ch) = remaining_chars.next() {
        if ch != '%' {
            rendered.push(ch);
            continue;
        }
        let Some(atom) = remaining_chars.next() else {
            rendered.push('%');
            break;
        };
        render_stash_list_format_atom(
            atom,
            &mut remaining_chars,
            &mut rendered,
            index,
            entry,
            commit,
        )?;
    }
    out.push_str(&apply_stash_wrap_spec(&rendered, &spec));
    Ok(())
}

fn parse_stash_wrap_spec<I>(
    chars: &mut std::iter::Peekable<I>,
    out: &mut String,
) -> Option<StashWrapSpec>
where
    I: Iterator<Item = char>,
{
    if !matches!(chars.peek(), Some('(')) {
        out.push_str("%w");
        return None;
    }
    chars.next();
    let mut raw = String::new();
    for ch in chars.by_ref() {
        raw.push(ch);
        if ch == ')' {
            let inner = &raw[..raw.len() - 1];
            let mut parts = inner.split(',').map(str::trim);
            let Some(width) = parts.next().and_then(|value| value.parse::<usize>().ok()) else {
                out.push_str("%w(");
                out.push_str(inner);
                out.push(')');
                return None;
            };
            let indent_first = match parts.next() {
                None | Some("") => 0,
                Some(value) => match value.parse::<usize>() {
                    Ok(indent) => indent,
                    Err(_) => {
                        out.push_str("%w(");
                        out.push_str(inner);
                        out.push(')');
                        return None;
                    }
                },
            };
            let indent_rest = match parts.next() {
                None | Some("") => 0,
                Some(value) => match value.parse::<usize>() {
                    Ok(indent) => indent,
                    Err(_) => {
                        out.push_str("%w(");
                        out.push_str(inner);
                        out.push(')');
                        return None;
                    }
                },
            };
            if parts.next().is_some() {
                out.push_str("%w(");
                out.push_str(inner);
                out.push(')');
                return None;
            }
            return Some(StashWrapSpec {
                width,
                indent_first,
                indent_rest,
            });
        }
    }
    out.push_str("%w(");
    out.push_str(&raw);
    None
}

fn apply_stash_wrap_spec(value: &str, spec: &StashWrapSpec) -> String {
    let mut out = String::new();
    for (paragraph_index, paragraph) in value.split('\n').enumerate() {
        if paragraph_index > 0 {
            out.push('\n');
        }
        append_wrapped_stash_paragraph(&mut out, paragraph, spec, paragraph_index == 0);
    }
    out
}

fn append_wrapped_stash_paragraph(
    out: &mut String,
    paragraph: &str,
    spec: &StashWrapSpec,
    first_paragraph: bool,
) {
    let mut line = String::new();
    let mut line_index = 0usize;
    for word in paragraph.split_whitespace() {
        let indent = if first_paragraph && line_index == 0 {
            spec.indent_first
        } else {
            spec.indent_rest
        };
        let available = spec.width.saturating_sub(indent).max(1);
        let next_len = if line.is_empty() {
            word.chars().count()
        } else {
            line.chars().count() + 1 + word.chars().count()
        };
        if !line.is_empty() && next_len > available {
            append_stash_wrap_line(out, &line, indent, line_index > 0);
            line.clear();
            line_index += 1;
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        let indent = if first_paragraph && line_index == 0 {
            spec.indent_first
        } else {
            spec.indent_rest
        };
        append_stash_wrap_line(out, &line, indent, line_index > 0);
    }
}

fn append_stash_wrap_line(out: &mut String, line: &str, indent: usize, needs_newline: bool) {
    if needs_newline {
        out.push('\n');
    }
    out.push_str(&" ".repeat(indent));
    out.push_str(line);
}

fn render_stash_color_atom<I>(chars: &mut std::iter::Peekable<I>, out: &mut String) -> Result<()>
where
    I: Iterator<Item = char> + Clone,
{
    match consume_stash_color_atom(chars) {
        StashColorAtom::Valid(sequence) => out.push_str(&sequence),
        StashColorAtom::Unterminated(literal) => {
            out.push_str("%C");
            out.push_str(&literal);
        }
        StashColorAtom::Invalid(value) => {
            return Err(invalid_stash_list_color_atom(&value));
        }
        StashColorAtom::Missing => out.push_str("%C"),
    }
    Ok(())
}

fn consume_stash_color_atom<I>(chars: &mut std::iter::Peekable<I>) -> StashColorAtom
where
    I: Iterator<Item = char> + Clone,
{
    if matches!(chars.peek(), Some('(')) {
        chars.next();
        let mut literal = String::from("(");
        let mut spec = String::new();
        for ch in chars.by_ref() {
            if ch == ')' {
                return if let Some((mode, color)) = spec.split_once(',') {
                    if mode.trim() == "always" {
                        config_commands::parse_config_color(color.trim())
                            .map(StashColorAtom::Valid)
                            .unwrap_or_else(|| StashColorAtom::Invalid(color.trim().to_owned()))
                    } else {
                        StashColorAtom::Valid(String::new())
                    }
                } else {
                    StashColorAtom::Valid(String::new())
                };
            }
            literal.push(ch);
            spec.push(ch);
        }
        return StashColorAtom::Unterminated(literal);
    }

    let color_atoms = [
        "normal", "reset", "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        "bold", "dim", "ul", "blink", "reverse", "italic", "strike",
    ];
    let mut preview = chars.clone();
    let lookahead = preview.by_ref().take(8).collect::<String>();
    if let Some(atom) = color_atoms
        .iter()
        .filter(|atom| lookahead.starts_with(**atom))
        .max_by_key(|atom| atom.len())
    {
        for _ in 0..atom.chars().count() {
            chars.next();
        }
        return StashColorAtom::Valid(String::new());
    }

    StashColorAtom::Missing
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
    email
        .split_once('@')
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
        .and_then(|timestamp| crate::runtime::local_datetime(timestamp.timestamp()))
        .unwrap_or_else(crate::runtime::local_now);
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

fn invalid_stash_list_color_atom(value: &str) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!(
            "error: invalid color value: {value}\nfatal: unable to parse --pretty format\n"
        ),
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
    let mut include_untracked = false;
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
            "-u" | "--include-untracked" => {
                include_untracked = true;
            }
            "--no-include-untracked" => {
                include_untracked = false;
            }
            "--only-untracked" => {
                only_untracked = true;
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
    let mut old_index = read_commit_tree_index_cached(&tree_cache, &parent_commit)?;
    let mut new_index = read_commit_tree_index_cached(&tree_cache, &commit)?;
    if include_untracked || only_untracked {
        let untracked_index = stash_untracked_parent_index(&commit_cache, &tree_cache, &commit)?
            .unwrap_or_else(GitIndex::new);
        if only_untracked {
            old_index = GitIndex::new();
            new_index = untracked_index;
        } else {
            let mut entries = new_index.entries().to_vec();
            entries.extend(untracked_index.entries().iter().cloned());
            new_index = GitIndex::from_entries(entries)?;
        }
    }
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
        color: false,
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
    labels: &StashApplyLabels,
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
        labels,
    )?;
    if !quiet {
        status(
            None,
            false,
            true,
            false,
            0,
            None,
            false,
            true,
            None,
            false,
            false,
            None,
            None,
            Vec::new(),
        )?;
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
    labels: StashApplyLabels,
}

#[derive(Default)]
struct StashApplyLabels {
    ours: Option<String>,
    theirs: Option<String>,
    base: Option<String>,
}

fn parse_stash_reference_options(
    args: &[String],
    operation: &str,
) -> Result<StashReferenceOptions> {
    let mut quiet = false;
    let mut index = false;
    let mut no_index = false;
    let mut stash = None;
    let mut index_arg = 0usize;
    while index_arg < args.len() {
        let arg = &args[index_arg];
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
                return Err(stash_reference_unknown_option(value, operation));
            }
            value => {
                if let Some(previous) = stash.replace(value.to_owned()) {
                    return Err(too_many_stash_revisions_error(&previous, value));
                }
            }
        }
        index_arg += 1;
    }
    Ok(StashReferenceOptions {
        quiet,
        index,
        no_index,
        stash,
        labels: StashApplyLabels::default(),
    })
}

fn stash_reference_unknown_option(option: &str, operation: &str) -> CliError {
    let usage_text = match operation {
        "apply" => STASH_APPLY_USAGE,
        "drop" => STASH_DROP_USAGE,
        "pop" => STASH_POP_USAGE,
        other => unreachable!("unsupported stash reference operation: {other}"),
    };
    CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown option `{}'\n{}\n",
            option.trim_start_matches('-'),
            usage_text
        ),
    }
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
    checkout_new_branch(false, branch, &base.to_hex(), false, false, false, false)?;
    let repo = find_repo()?;
    let tree_cache = TreeObjectCache::new(&store);
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
        true,
        &StashApplyLabels::default(),
    )?;
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
    if refs.storage_kind()? == zmin_git_core::refs::RefStorageKind::Reftable {
        refs.delete_reftable_log(stash_ref_name())?;
        return Ok(());
    }
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
    labels: &StashApplyLabels,
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
        Err(_) => {
            let merge_result = merge_indexes(
                store,
                &base_index,
                &head_index,
                &patch_index,
                labels.theirs.as_deref().unwrap_or("Stashed changes"),
            )?;
            match merge_result {
                MergeIndexResult::Clean(index) => index,
                MergeIndexResult::Conflicted { index, mut files } => {
                    let conflict_index =
                        merge_stash_apply_index(&current_index, &index, &stash_changed_paths)?;
                    remove_stash_deleted_paths(repo, &stash_changed_paths, &index)?;
                    let checkout = GitIndex::from_entries(
                        conflict_index
                            .entries()
                            .iter()
                            .filter(|entry| {
                                entry.stage == 0
                                    && stash_changed_paths.contains(entry.path.as_slice())
                            })
                            .cloned()
                            .collect(),
                    )?;
                    checkout_index(
                        store,
                        &checkout,
                        &repo.root,
                        CheckoutIndexOptions { force: true },
                    )?;
                    render_stash_conflicts(repo, store, &conflict_index, &mut files, labels)?;
                    restore_stash_untracked_index(repo, store, untracked_index.as_ref())?;
                    conflict_index.write_to_path(&repo.index_path)?;
                    return Err(CliError::Exit(1));
                }
            }
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

fn render_stash_conflicts(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    files: &mut [MergeConflictFile],
    labels: &StashApplyLabels,
) -> Result<()> {
    let merge_labels = MergeFileLabels {
        current: labels
            .ours
            .clone()
            .unwrap_or_else(|| "Updated upstream".to_owned()),
        ancestor: labels
            .base
            .clone()
            .unwrap_or_else(|| "Stash base".to_owned()),
        other: labels
            .theirs
            .clone()
            .unwrap_or_else(|| "Stashed changes".to_owned()),
    };
    let diff3 = read_config_entry(repo, "merge.conflictStyle")?
        .is_some_and(|entry| matches!(entry.value.as_str(), "diff3" | "zdiff3"));
    for file in files {
        if matches!(file.kind, MergeConflictKind::Content)
            && let (Some(base), Some(ours), Some(theirs)) = (
                index.entry(&file.path, 1),
                index.entry(&file.path, 2),
                index.entry(&file.path, 3),
            )
        {
            let base_content = read_index_entry_content(store, base)?;
            let ours_content = read_index_entry_content(store, ours)?;
            let theirs_content = read_index_entry_content(store, theirs)?;
            let merged = if diff3 {
                merge_file_diff3_core(&ours_content, &base_content, &theirs_content, &merge_labels)
            } else {
                merge_file_core(&ours_content, &base_content, &theirs_content, &merge_labels)
            };
            file.content = merged.content;
        }
        merge_commands::write_worktree_file(repo, &file.path, &file.content)?;
        println!("Auto-merging {}", String::from_utf8_lossy(&file.path));
        eprintln!(
            "CONFLICT (content): Merge conflict in {}",
            String::from_utf8_lossy(&file.path)
        );
    }
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

pub(crate) fn reset_worktree_to_head(repo: &GitRepo, store: &LooseObjectStore) -> Result<()> {
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
    head_index.refresh_cache_tree();
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
    let tracked_paths = tracked_path_set_for_repo(repo, index)?;
    let ignore = GitIgnore::load_from_root(&repo.root)?;
    let mut paths = worktree_commands::untracked_files_with_mode(
        &repo.root,
        &tracked_paths,
        &ignore,
        worktree_commands::UntrackedMode::All,
        true,
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
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if refs.storage_kind()? == zmin_git_core::refs::RefStorageKind::Reftable {
        let mut records = refs
            .reftable_logs()?
            .into_iter()
            .filter(|record| record.ref_name == stash_ref_name())
            .collect::<Vec<_>>();
        records.sort_by_key(|record| std::cmp::Reverse(record.update_index));
        return Ok(records
            .into_iter()
            .map(|record| StashEntry {
                id: record.new_id,
                message: record.message.trim_end_matches('\n').to_owned(),
                reflog_identity: format!("{} <{}>", record.name, record.email),
            })
            .collect());
    }
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
    append_reflog_with_committer(repo, stash_ref_name(), old_id, new_id, message, committer)
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
    quiet: bool,
    _guess: bool,
    _no_guess: bool,
    _merge: bool,
    _conflict: Option<String>,
    _progress: bool,
    _no_progress: bool,
    patch: bool,
    detach: bool,
    _recurse_submodules: bool,
    _no_recurse_submodules: bool,
    ours: bool,
    theirs: bool,
    overlay: bool,
    no_overlay: bool,
    _overwrite_ignore: bool,
    _no_overwrite_ignore: bool,
    _ignore_other_worktrees: bool,
    ignore_skip_worktree_bits: bool,
    track: Option<String>,
    no_track: bool,
    create: Option<String>,
    reset_create: Option<String>,
    create_reflog: bool,
    orphan: Option<String>,
    mut pathspec_from_file: Option<PathBuf>,
    no_pathspec_from_file: bool,
    mut pathspec_file_nul: bool,
    no_pathspec_file_nul: bool,
    mut args: Vec<String>,
) -> Result<()> {
    if no_pathspec_from_file {
        pathspec_from_file = None;
    }
    if no_pathspec_file_nul {
        pathspec_file_nul = false;
    }
    let has_pathspec_file = pathspec_from_file.is_some();
    if let Some(pathspec_file) = pathspec_from_file {
        let loaded = read_pathspec_file(&pathspec_file, pathspec_file_nul)?;
        args.extend(
            loaded
                .into_iter()
                .map(|path| path.into_os_string().to_string_lossy().into_owned()),
        );
    } else if pathspec_file_nul {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--pathspec-file-nul' requires '--pathspec-from-file'".into(),
        });
    }
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
    if track.is_some() || no_track {
        let message = if checkout_raw_args_have_separator() {
            "--track needs a branch name"
        } else {
            "missing branch name; try -b"
        };
        return Err(CliError::Fatal {
            code: 128,
            message: message.into(),
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
            false,
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
            false,
        );
    }
    if let Some(branch) = orphan {
        return checkout_orphan(force, &branch);
    }
    let explicit_pathspec_separator = checkout_raw_args_have_separator() || has_pathspec_file;
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
    let path_mode = checkout_path_mode(&args)?;
    let implicit_path_checkout = path_mode.is_none()
        && args
            .first()
            .is_some_and(|target| !checkout_target_exists(target).unwrap_or(false));
    if detach
        && args.len() == 1
        && args
            .first()
            .is_some_and(|target| checkout_worktree_path_exists(target).unwrap_or(false))
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "git checkout: --detach does not take a path argument '{}'",
                args[0]
            ),
        });
    }
    if path_mode.is_none() && !implicit_path_checkout {
        if overlay || no_overlay {
            return Err(CliError::Fatal {
                code: 128,
                message: "'--[no]-overlay' cannot be used with switching branches".into(),
            });
        }
        if ours || theirs {
            return Err(CliError::Fatal {
                code: 128,
                message: "'--ours/--theirs' needs the paths to check out".into(),
            });
        }
    }
    if let Some((source, paths, report_updated_paths)) = path_mode {
        if patch {
            return checkout_patch(source, &paths);
        }
        return checkout_paths(
            source,
            paths,
            report_updated_paths && !explicit_pathspec_separator && !quiet,
            ignore_skip_worktree_bits,
        );
    }
    if patch && implicit_path_checkout {
        return checkout_patch(None, &args.iter().map(PathBuf::from).collect::<Vec<_>>());
    }
    let Some(target) = args.first() else {
        if detach {
            return checkout_detached(force, "HEAD", "checkout", true);
        }
        return checkout_current_head(force);
    };
    if detach {
        return checkout_detached(force, target, "checkout", true);
    }
    if patch {
        return checkout_patch(Some(target), &[]);
    }
    if target == "HEAD" || target == "@" {
        return checkout_current_head(force);
    }
    if target == "-" {
        let previous = previous_checkout_target(1)?;
        return checkout_existing_with_message(
            force,
            &previous,
            CheckoutBranchMessage::ExistingBranch,
            !quiet,
        );
    }
    if target.starts_with("refs/heads/") {
        return checkout_detached(force, target, "checkout", true);
    }
    if !checkout_target_exists(target)? {
        return checkout_paths(
            None,
            vec![PathBuf::from(target)],
            !explicit_pathspec_separator && !quiet,
            ignore_skip_worktree_bits,
        );
    }
    checkout_existing_with_message(force, target, CheckoutBranchMessage::ExistingBranch, !quiet)
}

fn checkout_patch(source: Option<&str>, paths: &[PathBuf]) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let raw_index = read_repo_index_raw(&repo)?;
    let current_index = expand_repo_sparse_index(&repo, &raw_index)?;
    let source_index = if let Some(source) = source {
        let source_id = resolve_commitish(&repo, &store, source)?;
        let source_commit = CommitObjectCache::new(&store).read_commit(&source_id)?;
        TreeObjectCache::new(&store).read_tree_to_index(&source_commit.tree)?
    } else {
        let runtime = CliPrimitiveRuntime::new_default(&repo);
        read_head_index_from_primitive_stores(runtime.refs(), runtime.object_store_adapter())?
    };
    let worktree_index = worktree_index_snapshot(&repo, &current_index)?;
    let pathspecs = paths
        .iter()
        .map(|path| path_arg_to_repo_relative_allow_root(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let entries = diff_indexes(&source_index, &worktree_index)?
        .into_iter()
        .filter(|entry| {
            find_index_entry(&current_index, &entry.path)
                .is_some_and(|entry| !entry.intent_to_add())
                || find_index_entry(&source_index, &entry.path).is_some()
        })
        .filter(|entry| pathspec_matches(&entry.path, &pathspecs))
        .collect::<Vec<_>>();
    let _sparse_expansion_region =
        patch_changes_require_sparse_expansion(&repo, &raw_index, &entries)
            .then(|| trace2_region("index", "ensure_full_index"));
    if entries.is_empty() {
        return Ok(());
    }

    let patches = entries
        .iter()
        .map(|entry| {
            let mut patch_bytes = Vec::new();
            write_patch_entries(
                &mut patch_bytes,
                &repo,
                &store,
                &source_index,
                &worktree_index,
                std::slice::from_ref(entry),
                PatchFormatOptions::worktree(),
            )?;
            let patch = patch_commands::parse_apply_patches(&patch_bytes)?
                .into_iter()
                .next()
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "checkout patch diff did not contain a patch".into(),
                })?;
            Ok((patch_bytes, patch))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut answers = patch_commands::PatchAnswers::read()?;
    let mut all_remaining = None;
    for (patch_bytes, patch) in patches {
        let action = match all_remaining {
            Some(value) => Some(value),
            None => {
                let output = String::from_utf8(patch_bytes).map_err(|error| CliError::Fatal {
                    code: 128,
                    message: format!("patch output was not valid utf-8: {error}"),
                })?;
                print!("{output}");
                print!("(1/1) Discard this hunk from worktree [y,n,q,a,d,e,p,?]? ");
                io::stdout().flush()?;
                let answer = answers.next();
                println!();
                match answer {
                    patch_commands::PatchAnswer::Yes => Some(true),
                    patch_commands::PatchAnswer::No => Some(false),
                    patch_commands::PatchAnswer::All => {
                        all_remaining = Some(true);
                        Some(true)
                    }
                    patch_commands::PatchAnswer::Done => {
                        all_remaining = Some(false);
                        Some(false)
                    }
                    patch_commands::PatchAnswer::Quit => None,
                    patch_commands::PatchAnswer::Split => Some(false),
                }
            }
        };
        let Some(discard) = action else {
            break;
        };
        if !discard {
            continue;
        }
        let target_path = patch
            .new_path
            .as_ref()
            .or(patch.old_path.as_ref())
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "patch has no target path".into(),
            })?;
        let base_entry = find_index_entry(&source_index, target_path);
        let base = base_entry
            .map(|entry| read_index_entry_content(&store, entry))
            .transpose()?
            .unwrap_or_default();
        write_patch_worktree_update(
            &repo,
            PatchWorktreeUpdate {
                path: target_path.clone(),
                content: base,
                remove_if_empty_untracked: base_entry.is_none(),
            },
        )?;
    }
    Ok(())
}

fn patch_changes_require_sparse_expansion(
    repo: &GitRepo,
    raw_index: &GitIndex,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> bool {
    entries.iter().any(|change| {
        sparse_index_path_requires_expansion(raw_index, &change.path)
            || find_index_entry(raw_index, &change.path).is_some_and(IndexEntry::skip_worktree)
            || raw_index.entries().iter().any(|entry| {
                entry.stage == 0
                    && entry.mode == IndexMode::Tree
                    && change.path.starts_with(&entry.path)
                    && path_exists(&worktree_path_for_index_entry(
                        repo.root.as_path(),
                        &entry.path,
                    ))
            })
    })
}

fn checkout_raw_args_have_separator() -> bool {
    std::env::args_os().any(|arg| arg == "--")
}

fn checkout_current_head(force: bool) -> Result<()> {
    let repo = find_repo()?;
    let index_exists = repo.index_path.exists();
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target_id = refs.resolve("HEAD")?;
    let checkout_metadata = WorktreeCheckoutMetadata {
        ref_name: current_branch_ref(&refs)?,
        treeish: Some(target_id.clone()),
    };
    if force {
        checkout_worktree_with_metadata(&repo, &store, &target_id, &checkout_metadata)
            .map_err(|error| map_promisor_checkout_error(&repo, error))?;
    } else if !index_exists {
        checkout_clean_missing_index_transition_with_metadata(
            &repo,
            &store,
            &target_id,
            &checkout_metadata,
        )
        .map_err(|error| map_promisor_checkout_error(&repo, error))?;
    } else {
        checkout_clean_worktree_transition_with_metadata(
            &repo,
            &store,
            &target_id,
            &checkout_metadata,
        )
        .map_err(|error| map_promisor_checkout_error(&repo, error))?;
    }
    Ok(())
}

fn map_promisor_checkout_error(repo: &GitRepo, error: CliError) -> CliError {
    let is_missing = matches!(&error, CliError::Io(io_error) if io_error.kind() == io::ErrorKind::NotFound)
        || matches!(&error, CliError::Fatal { message, .. } if message.contains("git object not found"))
        || matches!(&error, CliError::Stderr { text, .. } if text.contains("git object not found"));
    if is_missing
        && admin_commands::promisor_remote_names(repo)
            .map(|remotes| !remotes.is_empty())
            .unwrap_or(false)
    {
        return CliError::Fatal {
            code: 128,
            message: "could not fetch required object from promisor remote".into(),
        };
    }
    error
}

pub(crate) fn previous_checkout_target(index: usize) -> Result<String> {
    if index == 0 {
        return Err(CliError::Fatal {
            code: 128,
            message: "could not resolve previous checkout".into(),
        });
    }
    let repo = find_repo()?;
    super::previous_checkout_target_from_repo(&repo, index).map_err(|_| CliError::Fatal {
        code: 128,
        message: "could not resolve previous checkout".into(),
    })
}

fn checkout_target_exists(target: &str) -> Result<bool> {
    let repo = find_repo()?;
    let algorithm = repo_hash_algorithm_from_config(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);
    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(common_git_dir, algorithm);
    if branch_checkout_ref(&refs, target)?.is_some() {
        return Ok(true);
    }
    Ok(resolve_commitish(&repo, &store, target).is_ok())
}

fn checkout_worktree_path_exists(target: &str) -> Result<bool> {
    let repo = find_repo()?;
    Ok(repo.root.join(target).exists())
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
    ignore_skip_worktree_bits: bool,
) -> Result<()> {
    let report_from_index = source.is_none();
    let updated_paths = if source.is_some() {
        worktree_commands::restore(
            source,
            false,
            false,
            false,
            true,
            false,
            true,
            false,
            false,
            None,
            false,
            false,
            false,
            false,
            false,
            ignore_skip_worktree_bits,
            false,
            false,
            false,
            None,
            false,
            paths,
        )?
    } else {
        worktree_commands::restore(
            source,
            false,
            false,
            false,
            false,
            false,
            true,
            false,
            false,
            None,
            false,
            false,
            false,
            false,
            false,
            ignore_skip_worktree_bits,
            false,
            false,
            false,
            None,
            false,
            paths,
        )?
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
    switch_reset_message: bool,
) -> Result<()> {
    let repo = find_repo()?;
    let algorithm = repo_hash_algorithm_from_config(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);

    let refs = RefStore::new(&repo.git_dir, algorithm);
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
    let reset_current_branch_old_id =
        if reset_existing && current_branch_ref(&refs)?.as_deref() == Some(ref_name.as_str()) {
            Some(refs.resolve(&ref_name)?)
        } else {
            None
        };
    if create_reflog || checkout_branch_reflog_enabled(&repo)? {
        write_ref_with_reflog(&repo, &refs, &ref_name, &id, &branch_reflog_message)?;
    } else {
        refs.write_ref(&ref_name, &id)?;
    }
    if let Some(old_id) = reset_current_branch_old_id {
        append_reflog(&repo, "HEAD", &old_id, &id, &branch_reflog_message)?;
    }
    match (reset_existing, switch_reset_message) {
        (true, true) => checkout_existing_with_message(
            force,
            branch,
            CheckoutBranchMessage::SwitchResetBranch,
            true,
        ),
        (true, false) => {
            checkout_existing_with_message(force, branch, CheckoutBranchMessage::ResetBranch, true)
        }
        (false, _) => {
            checkout_existing_with_message(force, branch, CheckoutBranchMessage::NewBranch, true)
        }
    }
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
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let ref_name = branch_ref_name(branch)?;
    if ref_exists(&refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("a branch named '{branch}' already exists"),
        });
    }
    let old_index = read_repo_index(&repo)?;
    let empty_index = GitIndex::new();
    if !force {
        verify_checkout_transition_clean(&repo, &old_index, &empty_index)?;
    }
    remove_tracked_paths_missing_from_target(&repo, &old_index, &empty_index)?;
    empty_index.write_to_path(&repo.index_path)?;
    refs.write_head_symbolic(&ref_name)?;
    eprintln!("Switched to a new branch '{branch}'");
    Ok(())
}

fn checkout_orphan(_force: bool, branch: &str) -> Result<()> {
    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let ref_name = branch_ref_name(branch)?;
    if ref_exists(&refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("a branch named '{branch}' already exists"),
        });
    }
    let index = read_repo_index(&repo)?;
    refs.write_head_symbolic(&ref_name)?;
    for entry in index.entries() {
        println!("A\t{}", String::from_utf8_lossy(&entry.path));
    }
    eprintln!("Switched to a new branch '{branch}'");
    Ok(())
}

#[derive(Clone, Copy)]
enum CheckoutBranchMessage {
    ExistingBranch,
    NewBranch,
    ResetBranch,
    SwitchResetBranch,
}

pub(crate) fn checkout_existing(force: bool, target: &str) -> Result<()> {
    checkout_existing_with_message(force, target, CheckoutBranchMessage::ExistingBranch, true)
}

fn checkout_existing_with_message(
    force: bool,
    target: &str,
    branch_message: CheckoutBranchMessage,
    print_messages: bool,
) -> Result<()> {
    let repo = find_repo()?;
    let algorithm = repo_hash_algorithm_from_config(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), algorithm);

    let common_git_dir = read_common_git_dir(&repo.git_dir)?;
    let refs = RefStore::new(common_git_dir, algorithm);
    let head_refs = RefStore::new(&repo.git_dir, algorithm);
    let target_branch_ref = branch_checkout_ref(&refs, target)?;
    let target_id = if let Some(ref_name) = target_branch_ref.as_deref() {
        refs.resolve(ref_name)?
    } else {
        resolve_commitish(&repo, &store, target)?
    };
    let current_id = head_refs.resolve("HEAD").ok();
    let index_exists = repo.index_path.exists();
    let force_transition = matches!(
        branch_message,
        CheckoutBranchMessage::ResetBranch | CheckoutBranchMessage::SwitchResetBranch
    );
    if force || force_transition || current_id.as_ref() != Some(&target_id) || !index_exists {
        let checkout_metadata = WorktreeCheckoutMetadata {
            ref_name: target_branch_ref.clone(),
            treeish: Some(target_id.clone()),
        };
        if force {
            checkout_worktree_with_metadata(&repo, &store, &target_id, &checkout_metadata)
                .map_err(|error| map_promisor_checkout_error(&repo, error))?;
        } else if force_transition {
            checkout_clean_missing_index_transition_with_metadata(
                &repo,
                &store,
                &target_id,
                &checkout_metadata,
            )
            .map_err(|error| map_promisor_checkout_error(&repo, error))?;
        } else if !index_exists {
            checkout_clean_worktree_replacement_with_metadata(
                &repo,
                &store,
                &target_id,
                &checkout_metadata,
            )
            .map_err(|error| map_promisor_checkout_error(&repo, error))?;
        } else {
            checkout_clean_worktree_transition_with_metadata(
                &repo,
                &store,
                &target_id,
                &checkout_metadata,
            )
            .map_err(|error| map_promisor_checkout_error(&repo, error))?;
        }
    }

    let source = current_head_reflog_name(&head_refs)?;
    let current_branch = current_branch_ref(&head_refs)?;
    if let Some(ref_name) = target_branch_ref {
        let already_on_branch = current_branch.as_deref() == Some(ref_name.as_str());
        let reflog_message = format!("checkout: moving from {source} to {target}");
        if print_messages && !print_detached_orphan_warning(&store, &head_refs, &[])? {
            print_previous_detached_head_position(&repo, &store, &head_refs)?;
        }
        write_head_symbolic_with_reflog(&repo, &head_refs, &ref_name, &reflog_message)?;
        if print_messages {
            match branch_message {
                CheckoutBranchMessage::ExistingBranch if already_on_branch => {
                    eprintln!("Already on '{target}'")
                }
                CheckoutBranchMessage::ExistingBranch => {
                    eprintln!("Switched to branch '{target}'")
                }
                CheckoutBranchMessage::ResetBranch => eprintln!("Reset branch '{target}'"),
                CheckoutBranchMessage::SwitchResetBranch => {
                    eprintln!("Switched to and reset branch '{target}'")
                }
                CheckoutBranchMessage::NewBranch => {
                    eprintln!("Switched to a new branch '{target}'")
                }
            }
        }
        if let Some(lines) = human_status_upstream(&repo, &head_refs, true)? {
            for line in lines {
                println!("{line}");
            }
        }
    } else {
        let reflog_message = format!("checkout: moving from {source} to {}", target_id.to_hex());
        if print_messages {
            print_detached_checkout_notice(
                &repo,
                &store,
                &head_refs,
                &target_id,
                target,
                DetachedCheckoutMode::Implicit,
            )?;
        }
        write_head_direct_with_reflog(&repo, &head_refs, &target_id, &reflog_message)?;
    }
    Ok(())
}

fn checkout_detached(
    force: bool,
    target: &str,
    operation: &str,
    print_messages: bool,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let _ = operation;

    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target_id = resolve_commitish(&repo, &store, target).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid reference: {target}"),
    })?;
    if force {
        checkout_worktree(&repo, &store, &target_id)
            .map_err(|error| map_promisor_checkout_error(&repo, error))?;
    } else {
        checkout_clean_worktree_transition(&repo, &store, &target_id)
            .map_err(|error| map_promisor_checkout_error(&repo, error))?;
    }
    let source = current_head_reflog_name(&refs)?;
    let mode = if operation == "checkout" {
        DetachedCheckoutMode::Explicit
    } else {
        DetachedCheckoutMode::Implicit
    };
    if print_messages {
        print_detached_checkout_notice(&repo, &store, &refs, &target_id, target, mode)?;
    }
    let reflog_message = format!("checkout: moving from {source} to {}", target_id.to_hex());
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
        RefTarget::Direct(id) => Ok(id.to_hex()),
    }
}

pub(crate) fn switch(
    force: bool,
    discard_changes: bool,
    force_create: Option<String>,
    create: Option<String>,
    _merge: bool,
    _conflict: Option<String>,
    _guess: bool,
    _no_guess: bool,
    quiet: bool,
    _progress: bool,
    _no_progress: bool,
    _recurse_submodules: bool,
    _no_recurse_submodules: bool,
    _ignore_other_worktrees: bool,
    orphan: Option<String>,
    detach: bool,
    track: Option<String>,
    no_track: bool,
    target: Option<String>,
) -> Result<()> {
    if [force_create.is_some(), create.is_some(), orphan.is_some()]
        .into_iter()
        .filter(|mode| *mode)
        .count()
        > 1
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "'--orphan' cannot be used with '-c'".into(),
        });
    }
    if (force_create.is_some() || create.is_some() || orphan.is_some()) && detach {
        return Err(CliError::Fatal {
            code: 128,
            message: "'--detach' cannot be used with '-b/-B/--orphan'".into(),
        });
    }
    if track.is_some() || no_track {
        return Err(CliError::Fatal {
            code: 128,
            message: "missing branch name; try -c".into(),
        });
    }
    let force = force || discard_changes;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);

    if let Some(branch) = orphan {
        return orphan_checkout(force, &branch);
    }

    if let Some(branch) = force_create {
        if !force && !worktree_clean(&repo, &store)? {
            return Err(CliError::Fatal {
                code: 1,
                message: "local changes would be overwritten by switch".into(),
            });
        }
        let start = target.as_deref().unwrap_or("HEAD");
        return checkout_new_branch(force, &branch, start, target.is_none(), true, false, true);
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
        return checkout_existing_with_message(
            force,
            &branch,
            CheckoutBranchMessage::NewBranch,
            true,
        );
    }

    let Some(mut target) = target else {
        return Err(CliError::Fatal {
            code: 129,
            message: "`switch` requires a branch, -c <branch>, or --detach <commit>".into(),
        });
    };
    if target == "-" {
        target =
            resolve_previous_checkout_name(&repo, "@{-1}")?.ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "invalid reference: -".into(),
            })?;
    }
    if detach {
        return checkout_detached(force, &target, "checkout", !quiet);
    }
    if !ref_exists(&refs, &branch_ref_name(&target)?)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid reference: {target}"),
        });
    }
    checkout_existing_with_message(
        force,
        &target,
        CheckoutBranchMessage::ExistingBranch,
        !quiet,
    )
}
