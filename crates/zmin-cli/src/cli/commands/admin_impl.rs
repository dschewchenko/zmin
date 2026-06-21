use super::*;

pub(crate) fn not_ready_current_git_command(name: &str, _args: Vec<String>) -> Result<()> {
    Err(CliError::Stderr {
        code: 1,
        text: format!("git: '{name}' is not a git command. See 'git --help'.\n"),
    })
}

pub(crate) fn hook(command: HookCommand) -> Result<()> {
    match command {
        HookCommand::Run {
            ignore_missing,
            to_stdin,
            hook_name,
            args,
        } => hook_run(ignore_missing, to_stdin, &hook_name, &args),
    }
}

pub(crate) fn managed_hooks(command: ManagedHooksCommand) -> Result<()> {
    match command {
        ManagedHooksCommand::Init => managed_hooks_init(),
        ManagedHooksCommand::Add {
            force,
            hook_name,
            command,
        } => managed_hooks_add(force, &hook_name, &command),
        ManagedHooksCommand::List => managed_hooks_list(),
        ManagedHooksCommand::Remove { hook_name } => managed_hooks_remove(&hook_name),
    }
}

pub(crate) fn for_each_repo_command(
    config: &str,
    keep_going: bool,
    arguments: Vec<String>,
) -> Result<()> {
    for_each_repo(config, keep_going, arguments)
}

pub(crate) struct UpdateIndexCommandOptions {
    pub(crate) add: bool,
    pub(crate) remove: bool,
    pub(crate) force_remove: bool,
    pub(crate) replace: bool,
    pub(crate) refresh: bool,
    pub(crate) cacheinfo: Vec<String>,
    pub(crate) index_info: bool,
    pub(crate) chmod: Option<String>,
    pub(crate) assume_unchanged: bool,
    pub(crate) no_assume_unchanged: bool,
    pub(crate) skip_worktree: bool,
    pub(crate) no_skip_worktree: bool,
    pub(crate) stdin: bool,
    pub(crate) nul_terminated: bool,
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) fn update_index_command(options: UpdateIndexCommandOptions) -> Result<()> {
    update_index(options)
}

pub(crate) fn bugreport_command(
    output_directory: Option<PathBuf>,
    suffix: Option<&str>,
    no_suffix: bool,
    diagnose: Option<&str>,
) -> Result<()> {
    bugreport(output_directory, suffix, no_suffix, diagnose)
}

pub(crate) fn diagnose_command_entry(
    output_directory: Option<PathBuf>,
    suffix: Option<&str>,
    mode: &str,
) -> Result<()> {
    diagnose(output_directory, suffix, mode)
}

pub(crate) fn backfill_command(
    min_batch_size: Option<usize>,
    sparse: bool,
    no_sparse: bool,
    revs: Vec<String>,
) -> Result<()> {
    backfill(min_batch_size, sparse, no_sparse, revs)
}

pub(crate) fn sh_i18n_command(args: Vec<String>) -> Result<()> {
    sh_i18n(args)
}

pub(crate) fn sh_setup_command(args: Vec<String>) -> Result<()> {
    sh_setup(args)
}

pub(crate) fn cvsserver_command(args: Vec<String>) -> Result<()> {
    cvsserver(args)
}

pub(crate) fn cvsexportcommit_command(args: Vec<String>) -> Result<()> {
    cvsexportcommit(args)
}

pub(crate) fn cvsimport_command(args: Vec<String>) -> Result<()> {
    cvsimport(args)
}

pub(crate) fn archimport_command(args: Vec<String>) -> Result<()> {
    archimport(args)
}

pub(crate) fn p4_command(args: Vec<String>) -> Result<()> {
    p4(args)
}

pub(crate) fn svn_command(args: Vec<String>) -> Result<()> {
    svn(args)
}

pub(crate) struct InstawebCommandOptions {
    pub(crate) start: bool,
    pub(crate) stop: bool,
    pub(crate) restart: bool,
    pub(crate) local: bool,
    pub(crate) port: u16,
    pub(crate) httpd: Option<String>,
    pub(crate) browser: Option<String>,
    pub(crate) daemon_internal: bool,
    pub(crate) git_dir: Option<PathBuf>,
    pub(crate) work_tree: Option<PathBuf>,
}

pub(crate) fn instaweb_command(options: InstawebCommandOptions) -> Result<()> {
    instaweb(options)
}

fn hook_run(
    ignore_missing: bool,
    to_stdin: Option<PathBuf>,
    hook_name: &str,
    args: &[String],
) -> Result<()> {
    let repo = find_repo()?;
    let hook_path = repo.git_dir.join("hooks").join(hook_name);
    if !hook_path.is_file() || !hook_is_executable(&hook_path)? {
        if ignore_missing {
            return Ok(());
        }
        let mut text = String::new();
        if hook_path.is_file() {
            text.push_str(&format!(
                "hint: The '{}' hook was ignored because it's not set as executable.\n",
                hook_path.display()
            ));
            text.push_str(
                "hint: You can disable this warning with `git config set advice.ignoredHook false`.\n",
            );
        }
        text.push_str(&format!("error: cannot find a hook named {hook_name}\n"));
        return Err(CliError::Stderr { code: 1, text });
    }

    let stdin = if let Some(path) = to_stdin {
        Stdio::from(fs::File::open(path)?)
    } else {
        Stdio::null()
    };
    let mut command = git_hook_command(&hook_path);
    let output = command
        .args(args)
        .current_dir(&repo.root)
        .stdin(stdin)
        .output()?;
    io::stderr().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(CliError::Exit(code)),
        None => Err(CliError::Exit(1)),
    }
}

pub(crate) fn hook_is_executable(path: &Path) -> io::Result<bool> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        Ok(true)
    }
}

const MANAGED_HOOK_NAMES: &[&str] = &[
    "pre-commit",
    "commit-msg",
    "pre-push",
    "post-checkout",
    "post-merge",
];
const MANAGED_HOOK_MARKER: &str = "# zmin-managed-hook";

fn managed_hooks_init() -> Result<()> {
    let repo = find_repo()?;
    fs::create_dir_all(repo.git_dir.join("hooks"))?;
    fs::create_dir_all(repo.git_dir.join("zmin"))?;
    for entry in read_config_entries(&repo)? {
        if entry.section == "zmin"
            && entry.subsection == "hooks"
            && managed_hook_name_is_supported(&entry.key)
        {
            let commands = managed_hook_commands(&repo, &entry.key)?;
            write_managed_hook_file(&repo, &entry.key, &commands)?;
        }
    }
    Ok(())
}

fn managed_hooks_add(force: bool, hook_name: &str, command: &str) -> Result<()> {
    let repo = find_repo()?;
    let hook_name = normalize_managed_hook_name(hook_name)?;
    if command.trim().is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "hook command cannot be empty".into(),
        });
    }
    fs::create_dir_all(repo.git_dir.join("hooks"))?;
    fs::create_dir_all(repo.git_dir.join("zmin"))?;
    reject_unmanaged_hook_file(&repo, hook_name, force)?;
    append_config_value(&repo, &managed_hook_config_key(hook_name), command)?;
    let commands = managed_hook_commands(&repo, hook_name)?;
    write_managed_hook_file(&repo, hook_name, &commands)?;
    Ok(())
}

fn managed_hooks_list() -> Result<()> {
    let repo = find_repo()?;
    let mut entries = read_config_entries(&repo)?
        .into_iter()
        .enumerate()
        .filter(|(_, entry)| managed_hook_entry_is_supported(entry))
        .collect::<Vec<_>>();
    entries.sort_by(|(left_index, left), (right_index, right)| {
        left.key
            .cmp(&right.key)
            .then_with(|| left_index.cmp(right_index))
    });
    for (_, entry) in entries {
        println!("{}\t{}", entry.key, entry.value);
    }
    Ok(())
}

fn managed_hooks_remove(hook_name: &str) -> Result<()> {
    let repo = find_repo()?;
    let hook_name = normalize_managed_hook_name(hook_name)?;
    unset_config_value(&repo, &managed_hook_config_key(hook_name))?;
    let hook_path = repo.git_dir.join("hooks").join(hook_name);
    if hook_path.is_file() && managed_hook_file_is_owned(&hook_path)? {
        fs::remove_file(hook_path)?;
    }
    Ok(())
}

fn normalize_managed_hook_name(hook_name: &str) -> Result<&str> {
    if managed_hook_name_is_supported(hook_name) {
        Ok(hook_name)
    } else {
        Err(CliError::Fatal {
            code: 1,
            message: format!("unsupported hook '{hook_name}'"),
        })
    }
}

fn managed_hook_name_is_supported(hook_name: &str) -> bool {
    MANAGED_HOOK_NAMES.contains(&hook_name)
}

fn managed_hook_config_key(hook_name: &str) -> String {
    format!("zmin.hooks.{hook_name}")
}

fn managed_hook_entry_is_supported(entry: &ConfigEntry) -> bool {
    entry.section == "zmin"
        && entry.subsection == "hooks"
        && managed_hook_name_is_supported(&entry.key)
}

fn managed_hook_commands(repo: &GitRepo, hook_name: &str) -> Result<Vec<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .filter(|entry| managed_hook_entry_is_supported(entry) && entry.key == hook_name)
        .map(|entry| entry.value)
        .collect())
}

fn reject_unmanaged_hook_file(repo: &GitRepo, hook_name: &str, force: bool) -> Result<()> {
    let hook_path = repo.git_dir.join("hooks").join(hook_name);
    if hook_path.exists() && !managed_hook_file_is_owned(&hook_path)? && !force {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "refusing to overwrite existing hook '{}'",
                hook_path.display()
            ),
        });
    }
    Ok(())
}

fn managed_hook_file_is_owned(path: &Path) -> io::Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let contents = fs::read_to_string(path)?;
    Ok(contents.lines().any(|line| line == MANAGED_HOOK_MARKER))
}

fn write_managed_hook_file(repo: &GitRepo, hook_name: &str, commands: &[String]) -> Result<()> {
    let hook_path = repo.git_dir.join("hooks").join(hook_name);
    let mut script = format!("#!/bin/sh\n{MANAGED_HOOK_MARKER}\n# hook: {hook_name}\n");
    for command in commands {
        script.push_str(&format!(
            "sh -c {} zmin-managed-hook \"$@\" || exit $?\n",
            shell_quote_single(command)
        ));
    }
    fs::write(&hook_path, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Clone)]
struct CvsExportCommitOptions {
    update: bool,
    verbose: bool,
    commit: bool,
    force_parent: bool,
    pedantic: bool,
    add_author: bool,
    cvsroot: Option<String>,
    cvsworkdir: Option<PathBuf>,
    same_worktree: bool,
    force: bool,
    msgprefix: Option<String>,
    keyword_reverse: bool,
    parent: Option<String>,
    commit_id: String,
}

#[derive(Debug, Clone)]
struct CvsImportOptions {
    head_branch: String,
    verbose: bool,
    cvsroot: Option<String>,
    target_dir: PathBuf,
    cvsps_file: Option<PathBuf>,
    import_only: bool,
    module: Option<String>,
    remote: Option<String>,
    track_revisions: bool,
}

#[derive(Debug, Clone)]
struct CvsPatchSet {
    number: usize,
    date: i64,
    author_name: String,
    author_email: String,
    branch: String,
    tag: Option<String>,
    log: String,
    members: Vec<CvsPatchMember>,
}

#[derive(Debug, Clone)]
struct CvsPatchMember {
    path: String,
    new_rev: String,
}

#[derive(Debug, Clone)]
struct P4SyncOptions {
    depot_path: String,
    target_dir: PathBuf,
    branch: String,
    checkout: bool,
    local_master: bool,
    verbose: bool,
}

#[derive(Debug, Clone)]
struct P4File {
    depot_path: String,
    revision: String,
    action: String,
}

#[derive(Debug, Clone)]
struct SvnSyncOptions {
    url: String,
    target_dir: PathBuf,
    ref_name: String,
    checkout: bool,
    local_master: bool,
    verbose: bool,
}

#[derive(Debug, Clone)]
struct P4SubmitOptions {
    branch: String,
    dry_run: bool,
    verbose: bool,
}

#[derive(Debug, Clone)]
struct SvnDcommitOptions {
    ref_name: String,
    dry_run: bool,
    verbose: bool,
}

#[derive(Debug, Clone)]
struct ArchImportOptions {
    roots: Vec<ArchImportRoot>,
    temp_dir: Option<PathBuf>,
    verbose: bool,
}

#[derive(Debug, Clone)]
struct ArchImportRoot {
    revision: String,
    branch: String,
}

fn for_each_repo(config: &str, keep_going: bool, arguments: Vec<String>) -> Result<()> {
    if arguments.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "for-each-repo requires command arguments".into(),
        });
    }
    let repos = read_multi_config_values(config)?
        .into_iter()
        .map(normalize_for_each_repo_config_path)
        .collect::<Vec<_>>();
    let executable = std::env::current_exe()?;
    let mut failed = None;
    for repo in repos {
        let status = match std::process::Command::new(&executable)
            .args(&arguments)
            .current_dir(&repo)
            .status()
        {
            Ok(status) => status,
            Err(error) => {
                if for_each_repo_missing_dir_error(&error) {
                    eprintln!("fatal: cannot change to '{repo}': No such file or directory");
                } else {
                    eprintln!("fatal: cannot change to '{repo}': {error}");
                }
                if !keep_going {
                    return Err(CliError::Exit(128));
                }
                failed.get_or_insert(1);
                continue;
            }
        };
        if !status.success() {
            let code = status.code().unwrap_or(1);
            if !keep_going {
                return Err(CliError::Exit(code));
            }
            failed.get_or_insert(code);
        }
    }
    if let Some(code) = failed {
        return Err(CliError::Exit(code));
    }
    Ok(())
}

fn for_each_repo_missing_dir_error(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::NotFound {
        return true;
    }
    #[cfg(windows)]
    if error.raw_os_error() == Some(267) {
        return true;
    }
    false
}

fn normalize_for_each_repo_config_path(path: String) -> String {
    #[cfg(windows)]
    {
        return path.replace("\\\\", "\\");
    }
    #[cfg(not(windows))]
    {
        path
    }
}

fn update_index(mut options: UpdateIndexCommandOptions) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut index = read_repo_index(&repo)?;
    normalize_update_index_cacheinfo_args(&mut options)?;
    if options.index_info {
        update_index_index_info(&store, &mut index)?;
    }
    let paths = if options.index_info {
        options.paths.clone()
    } else {
        update_index_paths(&options, &repo)?
    };

    for cacheinfo in &options.cacheinfo {
        update_index_cacheinfo(
            &repo,
            &store,
            &mut index,
            cacheinfo,
            options.add,
            options.replace,
        )?;
    }
    if options.force_remove {
        for path in &paths {
            let relative = path_arg_to_repo_relative(&repo, path)?;
            index.remove_path(&relative)?;
        }
    } else if !update_index_has_only_flag_changes(&options) {
        for path in &paths {
            update_index_path(&repo, &store, &mut index, path, &options)?;
        }
    }
    if let Some(chmod) = options.chmod.as_deref() {
        update_index_chmod(&mut index, &paths, chmod)?;
    }
    update_index_entry_flags(&repo, &mut index, &paths, &options)?;
    if options.refresh && paths.is_empty() {
        update_index_refresh_tracked(&repo, &store, &mut index)?;
    }
    index.write_to_path(&repo.index_path)?;
    Ok(())
}

fn normalize_update_index_cacheinfo_args(options: &mut UpdateIndexCommandOptions) -> Result<()> {
    if options.cacheinfo.iter().all(|value| value.contains(',')) {
        return Ok(());
    }
    let mut normalized = Vec::with_capacity(options.cacheinfo.len());
    let mut remaining_paths = Vec::new();
    let mut path_iter = options.paths.drain(..);
    for value in &options.cacheinfo {
        if value.contains(',') {
            normalized.push(value.clone());
            continue;
        }
        let Some(id) = path_iter.next() else {
            return Err(update_index_cacheinfo_usage_error());
        };
        let Some(path) = path_iter.next() else {
            return Err(update_index_cacheinfo_usage_error());
        };
        normalized.push(format!(
            "{},{},{}",
            value,
            id.to_string_lossy(),
            path.to_string_lossy()
        ));
    }
    remaining_paths.extend(path_iter);
    options.cacheinfo = normalized;
    options.paths = remaining_paths;
    Ok(())
}

fn update_index_paths(options: &UpdateIndexCommandOptions, repo: &GitRepo) -> Result<Vec<PathBuf>> {
    let mut paths = options.paths.clone();
    if options.stdin {
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input)?;
        let separator = if options.nul_terminated { 0 } else { b'\n' };
        for item in input.split(|byte| *byte == separator) {
            if item.is_empty() {
                continue;
            }
            let text = std::str::from_utf8(item).map_err(|_| CliError::Fatal {
                code: 128,
                message: "update-index --stdin path is not valid UTF-8".into(),
            })?;
            paths.push(PathBuf::from(text));
        }
    }
    let _ = repo;
    Ok(paths)
}

fn update_index_has_only_flag_changes(options: &UpdateIndexCommandOptions) -> bool {
    (options.assume_unchanged
        || options.no_assume_unchanged
        || options.skip_worktree
        || options.no_skip_worktree)
        && !options.add
        && !options.remove
        && !options.force_remove
        && !options.replace
        && !options.refresh
        && options.cacheinfo.is_empty()
        && !options.index_info
        && options.chmod.is_none()
}

fn update_index_path(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    path: &std::path::Path,
    options: &UpdateIndexCommandOptions,
) -> Result<()> {
    let relative = path_arg_to_repo_relative(repo, path)?;
    let absolute = repo.root.join(String::from_utf8_lossy(&relative).as_ref());
    if path_exists(&absolute) {
        if options.add || find_index_entry(index, &relative).is_some() {
            if options.replace {
                index.remove_dir(&relative)?;
            }
            stage_file(repo, store, index, &absolute)?;
            return Ok(());
        }
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "{}: cannot add to the index - missing --add option?",
                String::from_utf8_lossy(&relative)
            ),
        });
    }
    if options.remove {
        index.remove_path(&relative)?;
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!(
            "{}: does not exist and --remove not passed",
            String::from_utf8_lossy(&relative)
        ),
    })
}

fn update_index_cacheinfo(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
    value: &str,
    add: bool,
    replace: bool,
) -> Result<()> {
    let (mode, rest) = value
        .split_once(',')
        .ok_or_else(update_index_cacheinfo_usage_error)?;
    let (id, path) = rest
        .split_once(',')
        .ok_or_else(update_index_cacheinfo_usage_error)?;
    let path_bytes = normalize_git_path(path)?.into_bytes();
    if !add && find_index_entry(index, &path_bytes).is_none() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("{path}: cannot add to the index - missing --add option?"),
        });
    }
    update_index_prepare_cacheinfo_path(index, &path_bytes, path, replace)?;
    let mode = parse_index_mode(mode)?;
    let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?;
    let size = match mode {
        IndexMode::Gitlink => 0,
        _ => store.read_object(&id)?.content.len().min(u32::MAX as usize) as u32,
    };
    let mut entry = IndexEntry::new(path_bytes, id, mode, size)?;
    let absolute = repo.root.join(path);
    if let Ok(metadata) = fs::symlink_metadata(&absolute) {
        apply_index_entry_metadata(&mut entry, &metadata);
    }
    index.upsert(entry)?;
    Ok(())
}

fn update_index_prepare_cacheinfo_path(
    index: &mut GitIndex,
    path: &[u8],
    display: &str,
    replace: bool,
) -> Result<()> {
    if replace {
        update_index_remove_parent_file_entries(index, path)?;
        index.remove_dir(path)?;
        return Ok(());
    }
    if update_index_has_parent_file_entry(index, path)
        || update_index_has_child_entries(index, path)
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("git update-index: --cacheinfo cannot add {display}"),
        });
    }
    Ok(())
}

fn update_index_remove_parent_file_entries(index: &mut GitIndex, path: &[u8]) -> Result<()> {
    for (idx, byte) in path.iter().enumerate() {
        if *byte == b'/' {
            index.remove_path(&path[..idx])?;
        }
    }
    Ok(())
}

fn update_index_has_parent_file_entry(index: &GitIndex, path: &[u8]) -> bool {
    path.iter()
        .enumerate()
        .any(|(idx, byte)| *byte == b'/' && find_index_entry(index, &path[..idx]).is_some())
}

fn update_index_has_child_entries(index: &GitIndex, path: &[u8]) -> bool {
    let mut prefix = path.to_vec();
    prefix.push(b'/');
    index
        .entries()
        .iter()
        .any(|entry| entry.path.starts_with(&prefix))
}

fn update_index_index_info(_store: &LooseObjectStore, index: &mut GitIndex) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        update_index_index_info_line(index, line)?;
    }
    Ok(())
}

fn update_index_index_info_line(index: &mut GitIndex, line: &str) -> Result<()> {
    let (header, path) = line
        .split_once('\t')
        .ok_or_else(update_index_index_info_error)?;
    let mut header = header.split_whitespace();
    let mode = header.next().ok_or_else(update_index_index_info_error)?;
    let second = header.next().ok_or_else(update_index_index_info_error)?;
    let third = header.next().ok_or_else(update_index_index_info_error)?;
    let fourth = header.next();
    if fourth.is_some() || header.next().is_some() {
        return Err(update_index_index_info_error());
    }
    let (id, stage) = if update_index_index_info_object_type(second).is_some() {
        (third, 0)
    } else {
        let stage = third
            .parse::<u8>()
            .map_err(|_| update_index_index_info_error())?;
        (second, stage)
    };
    let mode = parse_index_mode(mode)?;
    let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?;
    if stage > 3 {
        return Err(update_index_index_info_error());
    }
    let mut entry = IndexEntry::new(normalize_git_path(path)?.into_bytes(), id, mode, 0)?;
    entry.stage = stage;
    index.upsert(entry)?;
    Ok(())
}

fn update_index_index_info_object_type(value: &str) -> Option<()> {
    matches!(value, "blob" | "tree" | "commit").then_some(())
}

fn update_index_index_info_error() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "malformed index info".into(),
    }
}

fn update_index_cacheinfo_usage_error() -> CliError {
    CliError::Fatal {
        code: 129,
        message: "update-index --cacheinfo expects <mode>,<object>,<path>".into(),
    }
}

fn update_index_chmod(index: &mut GitIndex, paths: &[PathBuf], chmod: &str) -> Result<()> {
    let executable = match chmod {
        "+x" => true,
        "-x" => false,
        _ => {
            return Err(CliError::Fatal {
                code: 129,
                message: "update-index --chmod expects +x or -x".into(),
            });
        }
    };
    for path in paths {
        let path = normalize_git_path(&path.to_string_lossy())?.into_bytes();
        let Some(existing) = find_index_entry(index, &path).cloned() else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "{}: cannot chmod missing index entry",
                    String::from_utf8_lossy(&path)
                ),
            });
        };
        let mut entry = existing;
        entry.mode = if executable {
            IndexMode::Executable
        } else {
            IndexMode::File
        };
        index.upsert(entry)?;
    }
    Ok(())
}

fn update_index_entry_flags(
    repo: &GitRepo,
    index: &mut GitIndex,
    paths: &[PathBuf],
    options: &UpdateIndexCommandOptions,
) -> Result<()> {
    if options.assume_unchanged && options.no_assume_unchanged {
        return Err(update_index_flag_conflict_error(
            "--assume-unchanged",
            "--no-assume-unchanged",
        ));
    }
    if options.skip_worktree && options.no_skip_worktree {
        return Err(update_index_flag_conflict_error(
            "--skip-worktree",
            "--no-skip-worktree",
        ));
    }
    if !options.assume_unchanged
        && !options.no_assume_unchanged
        && !options.skip_worktree
        && !options.no_skip_worktree
    {
        return Ok(());
    }
    for path in paths {
        let relative = path_arg_to_repo_relative(repo, path)?;
        let Some(existing) = find_index_entry(index, &relative).cloned() else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("Unable to mark file {}", String::from_utf8_lossy(&relative)),
            });
        };
        let mut entry = existing;
        if options.assume_unchanged {
            entry.set_assume_valid(true);
        }
        if options.no_assume_unchanged {
            entry.set_assume_valid(false);
        }
        if options.skip_worktree {
            entry.set_skip_worktree(true);
        }
        if options.no_skip_worktree {
            entry.set_skip_worktree(false);
        }
        index.upsert(entry)?;
    }
    Ok(())
}

fn update_index_flag_conflict_error(left: &str, right: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("options '{left}' and '{right}' cannot be used together"),
    }
}

fn update_index_refresh_tracked(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &mut GitIndex,
) -> Result<()> {
    let paths = index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .map(|entry| PathBuf::from(String::from_utf8_lossy(&entry.path).to_string()))
        .collect::<Vec<_>>();
    for path in paths {
        update_index_path(
            repo,
            store,
            index,
            &path,
            &UpdateIndexCommandOptions {
                add: false,
                remove: true,
                force_remove: false,
                replace: false,
                refresh: true,
                cacheinfo: Vec::new(),
                index_info: false,
                chmod: None,
                assume_unchanged: false,
                no_assume_unchanged: false,
                skip_worktree: false,
                no_skip_worktree: false,
                stdin: false,
                nul_terminated: false,
                paths: Vec::new(),
            },
        )?;
    }
    Ok(())
}

fn bugreport(
    output_directory: Option<PathBuf>,
    suffix: Option<&str>,
    no_suffix: bool,
    diagnose: Option<&str>,
) -> Result<()> {
    if no_suffix && suffix.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "bugreport --suffix cannot be combined with --no-suffix".into(),
        });
    }
    if let Some(mode) = diagnose {
        diagnose_command(
            output_directory.clone(),
            if no_suffix { None } else { suffix },
            mode,
        )?;
    }
    let directory = output_directory.unwrap_or(std::env::current_dir()?);
    fs::create_dir_all(&directory)?;
    let filename = bugreport_filename(suffix, no_suffix)?;
    let path = directory.join(filename);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut report = String::new();
    report.push_str("Thank you for filling out a Git bug report!\n\n");
    report.push_str("[System Info]\n");
    report.push_str(&format!(
        "zmin version: {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    report.push_str(&format!("os: {}\n", std::env::consts::OS));
    report.push_str(&format!("arch: {}\n", std::env::consts::ARCH));
    if let Ok(repo) = find_repo() {
        report.push_str("\n[Repository]\n");
        report.push_str(&format!("worktree: {}\n", repo.root.display()));
        report.push_str(&format!("gitdir: {}\n", repo.git_dir.display()));
        if let Ok(head) = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1).read_head() {
            report.push_str(&format!("HEAD: {head:?}\n"));
        }
    }
    fs::write(&path, report)?;
    eprintln!("Created new report at '{}'.", path.display());
    Ok(())
}

fn diagnose(output_directory: Option<PathBuf>, suffix: Option<&str>, mode: &str) -> Result<()> {
    diagnose_command(output_directory, suffix, mode)
}

fn diagnose_command(
    output_directory: Option<PathBuf>,
    suffix: Option<&str>,
    mode: &str,
) -> Result<()> {
    if !matches!(mode, "stats" | "all") {
        return Err(CliError::Fatal {
            code: 129,
            message: "diagnose --mode expects 'stats' or 'all'".into(),
        });
    }
    let repo = find_repo()?;
    let directory = output_directory.unwrap_or(std::env::current_dir()?);
    fs::create_dir_all(&directory)?;
    let suffix = match suffix {
        Some(format) => format_bugreport_suffix(format)?,
        None => format_bugreport_suffix("%Y-%m-%d-%H%M")?,
    };
    let path = directory.join(format!("git-diagnostics-{suffix}.zip"));

    println!("Collecting diagnostic info\n");
    let diagnostics = diagnose_log(&repo)?;
    print!("{diagnostics}");

    let mut entries = vec![
        ("diagnostics.log".to_owned(), diagnostics.into_bytes()),
        ("packs-local.txt".to_owned(), diagnose_packs_local(&repo)?),
        (
            "objects-local.txt".to_owned(),
            diagnose_objects_local(&repo)?,
        ),
    ];
    if mode == "all" {
        diagnose_collect_git_files(&repo.git_dir, &repo.git_dir, &mut entries)?;
    }
    write_stored_zip(&path, &entries)?;
    eprintln!("\nDiagnostics complete.");
    eprintln!(
        "All of the gathered info is captured in '{}'",
        path.display()
    );
    Ok(())
}

fn backfill(
    min_batch_size: Option<usize>,
    sparse: bool,
    _no_sparse: bool,
    revs: Vec<String>,
) -> Result<()> {
    let _ = min_batch_size;
    let repo = find_repo()?;
    if sparse
        && let Err(CliError::Fatal { .. }) = worktree_commands::ensure_sparse_checkout_enabled(
            &repo,
            "problem loading sparse-checkout",
        )
    {
        return Err(CliError::Stderr {
            code: 255,
            text: "error: problem loading sparse-checkout\n".into(),
        });
    }
    let promisor_remotes = promisor_remote_names(&repo)?;
    if !revs.is_empty() {
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        let revs = collect_rev_list_revs(&repo, &store, false, revs)?;
        if !promisor_remotes.is_empty() {
            backfill_from_promisor_remotes(&repo, &store, &revs, &promisor_remotes)?;
        }
        let _ = collect_commits_with_exclusions(&repo, &store, &revs, None)?;
    }
    Ok(())
}

fn promisor_remote_names(repo: &GitRepo) -> Result<Vec<String>> {
    let mut remotes = BTreeSet::new();
    for entry in read_config_entries(repo)? {
        if entry.section.eq_ignore_ascii_case("remote")
            && entry.key.eq_ignore_ascii_case("promisor")
            && entry.bool_value().unwrap_or(false)
        {
            remotes.insert(entry.subsection);
        }
    }
    Ok(remotes.into_iter().collect())
}

fn backfill_from_promisor_remotes(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    remotes: &[String],
) -> Result<()> {
    let roots = backfill_root_ids(repo, store, revs)?;
    backfill_promisor_objects_with_remotes(repo, &roots, remotes).map(|_| ())
}

pub(crate) fn backfill_promisor_objects(repo: &GitRepo, roots: &[ObjectId]) -> Result<bool> {
    let remotes = promisor_remote_names(repo)?;
    backfill_promisor_objects_with_remotes(repo, roots, &remotes)
}

fn backfill_promisor_objects_with_remotes(
    repo: &GitRepo,
    roots: &[ObjectId],
    remotes: &[String],
) -> Result<bool> {
    if roots.is_empty() {
        return Ok(!remotes.is_empty());
    }
    if remotes.is_empty() {
        return Ok(false);
    }
    for remote in remotes {
        let url = remote_url(repo, remote)?;
        if transport_commands::is_http_transport_url(&url) {
            backfill_http_promisor_remote(repo, &url, roots)?;
            continue;
        }
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        let haves = transport_commands::collect_upload_pack_haves(&store, &refs)?;
        if transport_commands::is_git_daemon_transport_url(&url) {
            transport_commands::daemon_fetch_pack_with_haves(
                &url,
                &repo.objects_dir,
                roots,
                &haves,
            )?;
            continue;
        }
        if transport_commands::is_ssh_transport_url(&url) {
            transport_commands::ssh_fetch_pack_with_haves(&url, &repo.objects_dir, roots, &haves)?;
            continue;
        }
        let Some(source_path) = local_repository_path_from_location(&url)? else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "backfill for promisor remote '{remote}' is only implemented for local, HTTP, SSH, and git daemon transports"
                ),
            });
        };
        backfill_local_promisor_remote(repo, &source_path, roots)?;
    }
    Ok(true)
}

fn backfill_root_ids(
    repo: &GitRepo,
    _store: &LooseObjectStore,
    revs: &RevListRevs,
) -> Result<Vec<ObjectId>> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for rev in &revs.include {
        let id = resolve_objectish(repo, rev).map_err(|_| ambiguous_revision_error(rev))?;
        if seen.insert(id.to_hex()) {
            roots.push(id);
        }
    }
    for (id, _) in &revs.extra_objects {
        if seen.insert(id.to_hex()) {
            roots.push(id.clone());
        }
    }
    Ok(roots)
}

fn backfill_local_promisor_remote(
    repo: &GitRepo,
    source_path: &Path,
    roots: &[ObjectId],
) -> Result<()> {
    let source = local_clone_source(source_path)?;
    let source_root = if source_path.join(".git").is_dir() {
        source_path.to_path_buf()
    } else {
        source.git_dir.clone()
    };
    let source_repo = GitRepo {
        root: source_root,
        objects_dir: source.git_dir.join("objects"),
        index_path: source.git_dir.join("index"),
        git_dir: source.git_dir,
    };
    let source_store =
        LooseObjectStore::new(source_repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let destination_store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    for id in roots {
        let _ = transport_commands::copy_reachable_objects(
            &source_repo,
            &source_store,
            &destination_store,
            id,
        )?;
    }
    Ok(())
}

fn backfill_http_promisor_remote(repo: &GitRepo, url: &str, roots: &[ObjectId]) -> Result<()> {
    let parsed_url = transport_commands::ParsedHttpUrl::parse(url)?;
    let mut helper = transport_commands::RemoteHttpHelperSession::spawn_for_url(url)?;
    let fetch_options = transport_commands::HttpFetchOptions {
        commit: false,
        tags: false,
        all: true,
        verbose: false,
        recover: false,
        write_ref: Vec::new(),
        stdin: false,
        packfile: None,
        index_pack_args: Vec::new(),
        args: Vec::new(),
    };
    let pack_fetched = transport_commands::http_fetch_smart_pack_with_helper(
        &parsed_url,
        &mut helper,
        &repo.objects_dir,
        roots,
        &[],
    )?;
    if pack_fetched {
        return Ok(());
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let mut seen = HashSet::new();
    let mut fetch_context = transport_commands::HttpFetchObjectContext::new(
        &parsed_url,
        &mut helper,
        &store,
        &commit_cache,
        &tree_cache,
        &fetch_options,
        &mut seen,
    );
    for id in roots {
        transport_commands::http_fetch_object_recursive(&mut fetch_context, id)?;
    }
    Ok(())
}

fn diagnose_log(repo: &GitRepo) -> Result<String> {
    let mut report = String::new();
    report.push_str(&format!("{}\n", git_compatible_version_line()));
    report.push_str(&format!("cpu: {}\n", std::env::consts::ARCH));
    report.push_str("no commit associated with this build\n");
    report.push_str(&format!(
        "sizeof-long: {}\n",
        std::mem::size_of::<std::os::raw::c_long>()
    ));
    report.push_str(&format!(
        "sizeof-size_t: {}\n",
        std::mem::size_of::<usize>()
    ));
    report.push_str(&format!("shell-path: {}\n", git_shell_path()));
    report.push_str("zlib: miniz_oxide\n");
    report.push_str("SHA-1: zmin-git-core\n");
    report.push_str("SHA-256: zmin-git-core\n");
    report.push_str(&format!("Repository root: {}\n", repo.root.display()));
    Ok(report)
}

fn diagnose_packs_local(repo: &GitRepo) -> Result<Vec<u8>> {
    let stats = collect_pack_object_stats(&repo.objects_dir)?;
    Ok(format!(
        "packs: {}\nobjects: {}\nsize: {}\n",
        stats.packs, stats.objects, stats.size_bytes
    )
    .into_bytes())
}

fn diagnose_objects_local(repo: &GitRepo) -> Result<Vec<u8>> {
    let stats = collect_loose_object_stats(&repo.objects_dir, GitHashAlgorithm::Sha1, false)?;
    Ok(format!(
        "loose objects: {}\nloose size KiB: {}\ngarbage: {}\ngarbage size KiB: {}\n",
        stats.count, stats.size_kib, stats.garbage, stats.garbage_size_kib
    )
    .into_bytes())
}

fn diagnose_collect_git_files(
    git_dir: &std::path::Path,
    dir: &std::path::Path,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        if name == "objects" {
            continue;
        }
        if file_type.is_dir() {
            diagnose_collect_git_files(git_dir, &path, entries)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(git_dir).map_err(|_| CliError::Fatal {
                code: 128,
                message: "diagnose path escaped git directory".into(),
            })?;
            let name = format!(".git/{}", relative.to_string_lossy());
            entries.push((name, fs::read(path)?));
        }
    }
    Ok(())
}

fn write_stored_zip(path: &std::path::Path, entries: &[(String, Vec<u8>)]) -> Result<()> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, content) in entries {
        let name_bytes = name.as_bytes();
        let offset = out.len() as u32;
        let crc = crc32(content);
        write_zip_local_header(&mut out, name_bytes, content.len() as u32, crc);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(content);
        write_zip_central_header(&mut central, name_bytes, content.len() as u32, crc, offset);
    }
    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    out.extend_from_slice(&central);
    write_zip_eocd(&mut out, entries.len() as u16, central_size, central_offset);
    fs::write(path, out)?;
    Ok(())
}

fn write_zip_local_header(out: &mut Vec<u8>, name: &[u8], size: u32, crc: u32) {
    push_le_u32(out, 0x0403_4b50);
    push_le_u16(out, 10);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u32(out, crc);
    push_le_u32(out, size);
    push_le_u32(out, size);
    push_le_u16(out, name.len() as u16);
    push_le_u16(out, 0);
}

fn write_zip_central_header(out: &mut Vec<u8>, name: &[u8], size: u32, crc: u32, offset: u32) {
    push_le_u32(out, 0x0201_4b50);
    push_le_u16(out, 10);
    push_le_u16(out, 10);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u32(out, crc);
    push_le_u32(out, size);
    push_le_u32(out, size);
    push_le_u16(out, name.len() as u16);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u32(out, 0);
    push_le_u32(out, offset);
    out.extend_from_slice(name);
}

fn write_zip_eocd(out: &mut Vec<u8>, count: u16, central_size: u32, central_offset: u32) {
    push_le_u32(out, 0x0605_4b50);
    push_le_u16(out, 0);
    push_le_u16(out, 0);
    push_le_u16(out, count);
    push_le_u16(out, count);
    push_le_u32(out, central_size);
    push_le_u32(out, central_offset);
    push_le_u16(out, 0);
}

fn push_le_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_le_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn bugreport_filename(suffix: Option<&str>, no_suffix: bool) -> Result<String> {
    if no_suffix {
        return Ok("git-bugreport.txt".to_owned());
    }
    let suffix = match suffix {
        Some(format) => format_bugreport_suffix(format)?,
        None => format_bugreport_suffix("%Y-%m-%d-%H%M")?,
    };
    if suffix.contains('\0') {
        return Err(CliError::Fatal {
            code: 128,
            message: "bugreport suffix must be a filename suffix".into(),
        });
    }
    Ok(format!("git-bugreport-{suffix}.txt"))
}

fn format_bugreport_suffix(format: &str) -> Result<String> {
    let timestamp = current_unix_timestamp()?;
    let datetime =
        chrono::DateTime::from_timestamp(timestamp, 0).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "system clock timestamp is out of range".into(),
        })?;
    Ok(datetime.format(format).to_string())
}

fn sh_i18n(_args: Vec<String>) -> Result<()> {
    Ok(())
}

fn sh_setup(args: Vec<String>) -> Result<()> {
    if args.first().is_some_and(|arg| arg == "-h") {
        println!("usage: git sh-setup ");
    }
    Ok(())
}

fn cvsserver(args: Vec<String>) -> Result<()> {
    if args
        .first()
        .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("git-cvsserver version {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "-h" || arg == "-H") {
        return Ok(());
    }

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    for line in input.lines() {
        match line.trim_end_matches('\r') {
            "" => {}
            "valid-requests" => {
                println!(
                    "Valid-requests Argument Argumentx Directory Entry Global_option Modified Questionable Root Sticky Unchanged Valid-responses add admin annotate ci co diff editors expand-modules history log noop remove rlog status tag update valid-requests watchers"
                );
                println!("ok");
            }
            "noop" => {
                println!("ok");
            }
            other => {
                println!("error  unrecognized request `{other}'");
                return Ok(());
            }
        }
    }
    Ok(())
}

fn cvsexportcommit(args: Vec<String>) -> Result<()> {
    let options = parse_cvsexportcommit_args(args)?;
    let _ = options.pedantic;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let commit_id = resolve_commitish(&repo, &store, &options.commit_id)?;
    let commit = commit_cache.read_commit(&commit_id)?;
    let parent_id = cvsexportcommit_parent(&repo, &store, &commit, &options)?;
    let cvs_dir = cvsexportcommit_workdir(&repo, &options)?;
    if !cvs_dir.join("CVS").is_dir() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("{} is not a CVS checkout", cvs_dir.display()),
        });
    }

    let old_index = match parent_id.as_ref() {
        Some(parent_id) => read_treeish_index(&repo, &store, &parent_id.to_hex())?,
        None => GitIndex::new(),
    };
    let new_index = read_treeish_index(&repo, &store, &commit_id.to_hex())?;
    let entries = diff_indexes(&old_index, &new_index)?;
    let files = entries
        .iter()
        .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
        .collect::<Vec<_>>();

    if options.verbose {
        let parent = parent_id
            .as_ref()
            .map(ObjectId::to_hex)
            .unwrap_or_else(|| "0".repeat(GitHashAlgorithm::Sha1.digest_len() * 2));
        println!(
            "Applying to CVS commit {} from parent {}",
            commit_id.to_hex(),
            parent
        );
    }

    let mut message = Vec::new();
    if let Some(prefix) = options.msgprefix.as_deref() {
        message.extend_from_slice(prefix.as_bytes());
    }
    message.extend_from_slice(&commit.message);
    if options.add_author {
        message.extend_from_slice(b"\n\nAuthor: ");
        message.extend_from_slice(cvsexportcommit_identity(&commit.author).as_bytes());
        let author = cvsexportcommit_identity(&commit.author);
        let committer = cvsexportcommit_identity(&commit.committer);
        if author != committer {
            message.extend_from_slice(b"\nCommitter: ");
            message.extend_from_slice(committer.as_bytes());
        }
        message.push(b'\n');
    }
    fs::write(cvs_dir.join(".msg"), message)?;

    let mut patch = Vec::new();
    write_patch_entries(
        &mut patch,
        &repo,
        &store,
        &old_index,
        &new_index,
        &entries,
        PatchFormatOptions::cached(),
    )?;
    fs::write(cvs_dir.join(".cvsexportcommit.diff"), &patch)?;

    println!("Checking if patch will apply");
    if options.update && !files.is_empty() {
        run_cvs_command(&cvs_dir, &options, "update", &files)?;
    }
    if !options.force {
        cvsexportcommit_check_cvs_status(&cvs_dir, &options, &entries)?;
    }
    if options.keyword_reverse {
        cvsexportcommit_reverse_keywords(&cvs_dir, &entries)?;
    }

    println!("Applying");
    if options.same_worktree {
        checkout_worktree(&repo, &store, &commit_id)?;
    } else {
        let work_repo = GitRepo {
            root: cvs_dir.clone(),
            git_dir: repo.git_dir.clone(),
            objects_dir: repo.objects_dir.clone(),
            index_path: repo.index_path.clone(),
        };
        let apply_options = patch_commands::ApplyOptions {
            check: false,
            cached: false,
            index: false,
            reverse: false,
            patches: Vec::new(),
        };
        for patch in patch_commands::parse_apply_patches(&patch)? {
            let update = patch_commands::apply_file_patch(
                &work_repo,
                &store,
                &old_index,
                &patch,
                &apply_options,
            )?;
            let mut ignored_index = GitIndex::new();
            patch_commands::write_apply_update(
                &work_repo,
                &store,
                &mut ignored_index,
                update,
                &apply_options,
            )?;
        }
    }

    println!("Patch applied successfully. Adding new files and directories to CVS");
    cvsexportcommit_add_remove_cvs_paths(&cvs_dir, &options, &entries)?;
    println!("Commit to CVS");
    let title = String::from_utf8_lossy(&commit.message)
        .lines()
        .next()
        .unwrap_or("")
        .to_owned();
    println!("Patch title (first comment line): {title}");
    if options.commit {
        println!("Autocommit");
        run_cvs_command(
            &cvs_dir,
            &options,
            "commit",
            &cvsexportcommit_commit_args(&files),
        )?;
        remove_file_if_exists(&cvs_dir.join(".msg"))?;
        println!("Committed successfully to CVS");
    } else {
        println!("Ready for you to commit, just run:");
        println!();
        println!("   {}", cvsexportcommit_commit_command(&options, &files));
    }
    remove_file_if_exists(&cvs_dir.join(".cvsexportcommit.diff"))?;
    Ok(())
}

fn parse_cvsexportcommit_args(args: Vec<String>) -> Result<CvsExportCommitOptions> {
    let mut update = false;
    let mut verbose = false;
    let mut commit = false;
    let mut force_parent = false;
    let mut pedantic = false;
    let mut add_author = false;
    let mut cvsroot = None;
    let mut cvsworkdir = None;
    let mut same_worktree = false;
    let mut force = false;
    let mut msgprefix = None;
    let mut keyword_reverse = false;
    let mut values = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "usage: GIT_DIR=/path/to/.git git cvsexportcommit [-h] [-p] [-v] [-c] [-f] [-u] [-k] [-w cvsworkdir] [-m msgprefix] [ parent ] commit".into(),
                });
            }
            "-u" => update = true,
            "-v" => verbose = true,
            "-c" => commit = true,
            "-P" => force_parent = true,
            "-p" => pedantic = true,
            "-a" => add_author = true,
            "-W" => same_worktree = true,
            "-f" => force = true,
            "-k" => keyword_reverse = true,
            "-d" => cvsroot = Some(next_option_value(&mut iter, "-d")?),
            "-w" => cvsworkdir = Some(PathBuf::from(next_option_value(&mut iter, "-w")?)),
            "-m" => msgprefix = Some(next_option_value(&mut iter, "-m")?),
            _ if arg.starts_with("-d") && arg.len() > 2 => cvsroot = Some(arg[2..].to_owned()),
            _ if arg.starts_with("-w") && arg.len() > 2 => {
                cvsworkdir = Some(PathBuf::from(&arg[2..]))
            }
            _ if arg.starts_with("-m") && arg.len() > 2 => msgprefix = Some(arg[2..].to_owned()),
            _ if arg.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unknown cvsexportcommit option '{arg}'"),
                });
            }
            _ => values.push(arg),
        }
    }
    let commit_id = values.pop().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "Need at least one commit identifier!".into(),
    })?;
    let parent = match values.as_slice() {
        [] => None,
        [parent] => Some(parent.clone()),
        _ => {
            return Err(CliError::Fatal {
                code: 129,
                message: "cvsexportcommit accepts at most one parent and one commit".into(),
            });
        }
    };
    Ok(CvsExportCommitOptions {
        update,
        verbose,
        commit,
        force_parent,
        pedantic,
        add_author,
        cvsroot,
        cvsworkdir,
        same_worktree,
        force,
        msgprefix,
        keyword_reverse,
        parent,
        commit_id,
    })
}

fn next_option_value(iter: &mut impl Iterator<Item = String>, option: &str) -> Result<String> {
    iter.next().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: format!("{option} requires a value"),
    })
}

fn cvsexportcommit_parent(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: &zmin_git_core::CommitObject,
    options: &CvsExportCommitOptions,
) -> Result<Option<ObjectId>> {
    if let Some(parent) = options.parent.as_deref() {
        let parent = resolve_commitish(repo, store, parent)?;
        if !options.force_parent && !commit.parents.iter().any(|candidate| candidate == &parent) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "Did not find {} in the parents for this commit!",
                    parent.to_hex()
                ),
            });
        }
        return Ok(Some(parent));
    }
    match commit.parents.as_slice() {
        [parent] => Ok(Some(parent.clone())),
        [] => Ok(None),
        _ => Err(CliError::Fatal {
            code: 128,
            message:
                "This commit has more than one parent -- please name the parent you want to use explicitly"
                    .into(),
        }),
    }
}

fn cvsexportcommit_workdir(repo: &GitRepo, options: &CvsExportCommitOptions) -> Result<PathBuf> {
    if options.same_worktree {
        return Ok(repo.root.clone());
    }
    if let Some(path) = options.cvsworkdir.as_deref() {
        return absolute_path_from_arg(path);
    }
    if let Some(path) = read_config_value(repo, "cvsexportcommit.cvsdir")? {
        return absolute_path_from_arg(std::path::Path::new(&path));
    }
    Ok(std::env::current_dir()?)
}

fn cvsexportcommit_identity(raw: &[u8]) -> String {
    let value = String::from_utf8_lossy(raw);
    value
        .rsplit_once(' ')
        .and_then(|(left, _)| left.rsplit_once(' ').map(|(identity, _)| identity))
        .unwrap_or(&value)
        .to_owned()
}

fn cvsexportcommit_check_cvs_status(
    cvs_dir: &std::path::Path,
    options: &CvsExportCommitOptions,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<()> {
    let files = entries
        .iter()
        .filter(|entry| entry.status != IndexDiffStatus::Added)
        .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Ok(());
    }
    let output = run_cvs_command(cvs_dir, options, "status", &files)?;
    for line in output
        .lines()
        .filter(|line| line.trim_start().starts_with("File:"))
    {
        if !line.contains("Status: Up-to-date") {
            return Err(CliError::Fatal {
                code: 1,
                message: "Exiting: your CVS tree is not clean for this merge.".into(),
            });
        }
    }
    Ok(())
}

fn cvsexportcommit_reverse_keywords(
    cvs_dir: &std::path::Path,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<()> {
    for entry in entries
        .iter()
        .filter(|entry| entry.status != IndexDiffStatus::Added)
    {
        let path = cvs_dir.join(String::from_utf8_lossy(&entry.path).as_ref());
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        fs::write(path, reverse_cvs_keywords(&content))?;
    }
    Ok(())
}

fn reverse_cvs_keywords(input: &str) -> String {
    let keyword = match regex::Regex::new(r"\$([A-Z][a-z]+):[^\$]+\$") {
        Ok(keyword) => keyword,
        Err(_) => return input.to_owned(),
    };
    keyword.replace_all(input, "$$$1$$").into_owned()
}

fn cvsexportcommit_add_remove_cvs_paths(
    cvs_dir: &std::path::Path,
    options: &CvsExportCommitOptions,
    entries: &[zmin_git_core::IndexDiffEntry],
) -> Result<()> {
    let mut dirs = BTreeSet::new();
    for entry in entries
        .iter()
        .filter(|entry| entry.status == IndexDiffStatus::Added)
    {
        let mut path = PathBuf::from(String::from_utf8_lossy(&entry.path).as_ref());
        path.pop();
        while !path.as_os_str().is_empty() {
            if !cvs_dir.join(&path).join("CVS").is_dir() {
                dirs.insert(path.to_string_lossy().replace('\\', "/"));
            }
            if !path.pop() {
                break;
            }
        }
    }
    for dir in dirs {
        run_cvs_command(cvs_dir, options, "add", &[dir])?;
    }
    for entry in entries {
        let path = String::from_utf8_lossy(&entry.path).into_owned();
        match entry.status {
            IndexDiffStatus::Added => {
                run_cvs_command(cvs_dir, options, "add", &[path])?;
            }
            IndexDiffStatus::Deleted => {
                run_cvs_command(cvs_dir, options, "rm", &["-f".into(), path])?;
            }
            IndexDiffStatus::Modified | IndexDiffStatus::Renamed | IndexDiffStatus::Copied => {}
        }
    }
    Ok(())
}

fn cvsexportcommit_commit_args(files: &[String]) -> Vec<String> {
    let mut args = vec!["-F".to_owned(), ".msg".to_owned()];
    args.extend(files.iter().cloned());
    args
}

fn cvsexportcommit_commit_command(options: &CvsExportCommitOptions, files: &[String]) -> String {
    let mut parts = vec!["cvs".to_owned()];
    if let Some(root) = options.cvsroot.as_deref() {
        parts.push("-d".to_owned());
        parts.push(root.to_owned());
    }
    parts.push("commit".to_owned());
    parts.extend(cvsexportcommit_commit_args(files));
    parts.join(" ")
}

fn run_cvs_command(
    cwd: &std::path::Path,
    options: &CvsExportCommitOptions,
    subcommand: &str,
    args: &[String],
) -> Result<String> {
    let mut command = foreign_scm_command("cvs");
    if let Some(root) = options.cvsroot.as_deref() {
        command.arg("-d").arg(root);
    }
    command.arg(subcommand).args(args).current_dir(cwd);
    let output = command.output().map_err(CliError::Io)?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!(
                "cvs {subcommand} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !stdout.is_empty() {
        print!("{stdout}");
    }
    Ok(stdout)
}

fn cvsimport(args: Vec<String>) -> Result<()> {
    let options = parse_cvsimport_args(args)?;
    let repo = open_or_init_cvsimport_repo(&options.target_dir)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let cvsps_output = read_or_generate_cvsps_output(&options)?;
    let patchsets = parse_cvsps_patchsets(&cvsps_output)?;
    let mut branch_indexes = HashMap::<String, BTreeMap<Vec<u8>, IndexEntry>>::new();
    for patchset in patchsets {
        let branch = if patchset.branch == "HEAD" {
            options.head_branch.clone()
        } else {
            patchset.branch.clone()
        };
        let ref_name = cvsimport_ref_name(&options, &branch);
        let index = branch_indexes
            .entry(branch.clone())
            .or_insert_with(|| read_ref_index(&store, &refs, &ref_name).unwrap_or_default());
        for member in &patchset.members {
            let path = normalize_git_path(&member.path)?.into_bytes();
            if member.new_rev.eq_ignore_ascii_case("dead") {
                index.remove(&path);
                continue;
            }
            let content = cvsimport_fetch_revision(&options, &member.path, &member.new_rev)?;
            let id = store.write_object(GitObjectKind::Blob, &content)?;
            let entry = IndexEntry::new(
                path.clone(),
                id,
                IndexMode::File,
                content.len().min(u32::MAX as usize) as u32,
            )?;
            index.insert(path, entry);
        }
        let git_index = GitIndex::from_entries(index.values().cloned().collect::<Vec<_>>())?;
        let tree = write_tree_from_index(&store, &git_index)?;
        let mut builder = CommitBuilder::new(
            tree,
            Signature::new(
                patchset.author_name.clone(),
                patchset.author_email.clone(),
                patchset.date,
                "+0000",
            )?,
            Signature::new(
                patchset.author_name.clone(),
                patchset.author_email.clone(),
                patchset.date,
                "+0000",
            )?,
        );
        if let Ok(parent) = refs.resolve(&ref_name) {
            builder = builder.parent(parent);
        }
        let id = store.write_object(
            GitObjectKind::Commit,
            &builder
                .message(cvsimport_log_message(&patchset))?
                .encode()?,
        )?;
        refs.write_ref(&ref_name, &id)?;
        if let Some(tag) = patchset.tag.as_deref().filter(|tag| *tag != "(none)") {
            refs.write_ref(&tag_ref_name(&sanitize_cvs_symbol(tag))?, &id)?;
        }
        if options.track_revisions {
            append_cvs_revision_map(&repo, &patchset, &id)?;
        }
        if options.verbose {
            println!("Committed patch {} ({} +0000)", patchset.number, branch);
        }
    }
    let head_ref = cvsimport_ref_name(&options, &options.head_branch);
    if options.remote.is_none() {
        refs.write_symbolic_ref("HEAD", &head_ref)?;
    }
    if !options.import_only
        && let Ok(id) = refs.resolve(&head_ref)
    {
        checkout_worktree(&repo, &store, &id)?;
    }
    Ok(())
}

fn parse_cvsimport_args(args: Vec<String>) -> Result<CvsImportOptions> {
    let mut head_branch = None;
    let mut verbose = false;
    let mut cvsroot = None;
    let mut target_dir = None;
    let mut cvsps_file = None;
    let mut import_only = false;
    let mut remote = None;
    let mut track_revisions = false;
    let mut module = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "usage: git cvsimport [-o branch-for-HEAD] [-h] [-v] [-d CVSROOT] [-P file] [-C GIT_repository] [-i] [-r remote] [-R] [CVS_module]".into(),
                });
            }
            "-v" => verbose = true,
            "-i" => import_only = true,
            "-R" => track_revisions = true,
            "-o" => head_branch = Some(next_option_value(&mut iter, "-o")?),
            "-d" => cvsroot = Some(next_option_value(&mut iter, "-d")?),
            "-C" => target_dir = Some(PathBuf::from(next_option_value(&mut iter, "-C")?)),
            "-P" => cvsps_file = Some(PathBuf::from(next_option_value(&mut iter, "-P")?)),
            "-r" => remote = Some(next_option_value(&mut iter, "-r")?),
            "-a" | "-k" | "-u" | "-m" => {}
            "-p" | "-z" | "-A" | "-s" | "-M" | "-S" | "-L" => {
                let _ = next_option_value(&mut iter, arg.as_str())?;
            }
            _ if arg.starts_with("-o") && arg.len() > 2 => head_branch = Some(arg[2..].to_owned()),
            _ if arg.starts_with("-d") && arg.len() > 2 => cvsroot = Some(arg[2..].to_owned()),
            _ if arg.starts_with("-C") && arg.len() > 2 => {
                target_dir = Some(PathBuf::from(&arg[2..]))
            }
            _ if arg.starts_with("-P") && arg.len() > 2 => {
                cvsps_file = Some(PathBuf::from(&arg[2..]))
            }
            _ if arg.starts_with("-r") && arg.len() > 2 => remote = Some(arg[2..].to_owned()),
            _ if arg.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unknown cvsimport option '{arg}'"),
                });
            }
            _ if module.is_none() => module = Some(arg),
            _ => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "You can't specify more than one CVS module".into(),
                });
            }
        }
    }
    Ok(CvsImportOptions {
        head_branch: head_branch.unwrap_or_else(|| {
            if remote.is_some() {
                "master".to_owned()
            } else {
                "origin".to_owned()
            }
        }),
        verbose,
        cvsroot,
        target_dir: target_dir.unwrap_or_else(|| PathBuf::from(".")),
        cvsps_file,
        import_only,
        module,
        remote,
        track_revisions,
    })
}

fn read_or_generate_cvsps_output(options: &CvsImportOptions) -> Result<String> {
    if let Some(path) = options.cvsps_file.as_deref() {
        return Ok(fs::read_to_string(path)?);
    }
    let mut command = foreign_scm_command("cvsps");
    if let Some(root) = options.cvsroot.as_deref() {
        command.arg("-d").arg(root);
    }
    if let Some(module) = options.module.as_deref() {
        command.arg(module);
    }
    command.current_dir(&options.target_dir);
    let output = command.output().map_err(CliError::Io)?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!(
                "cvsps failed: {}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| CliError::Fatal {
        code: 128,
        message: "cvsps output contains non-utf8 data".into(),
    })
}

fn open_or_init_cvsimport_repo(path: &std::path::Path) -> Result<GitRepo> {
    let root = absolute_path_from_arg(path)?;
    if !root.join(".git").is_dir() {
        init_repository(
            root.clone(),
            InitRepositoryOptions {
                bare: false,
                initial_branch: "master".to_owned(),
            },
        )?;
    }
    Ok(GitRepo {
        index_path: root.join(".git/index"),
        objects_dir: root.join(".git/objects"),
        git_dir: root.join(".git"),
        root,
    })
}

fn parse_cvsps_patchsets(input: &str) -> Result<Vec<CvsPatchSet>> {
    let mut patchsets = Vec::new();
    for block in input.split("---------------------") {
        let mut lines = block.lines().peekable();
        let Some(first) = lines.find(|line| line.starts_with("PatchSet ")) else {
            continue;
        };
        let number = first["PatchSet ".len()..]
            .trim()
            .parse::<usize>()
            .map_err(|_| CliError::Fatal {
                code: 128,
                message: "cvsps PatchSet number is invalid".into(),
            })?;
        let mut date = None;
        let mut author = None;
        let mut branch = None;
        let mut tag = None;
        let mut log = String::new();
        let mut members = Vec::new();
        while let Some(line) = lines.next() {
            if let Some(value) = line.strip_prefix("Date:") {
                date = Some(parse_cvsps_date(value.trim())?);
            } else if let Some(value) = line.strip_prefix("Author:") {
                author = Some(parse_cvsps_author(value.trim()));
            } else if let Some(value) = line.strip_prefix("Branch:") {
                branch = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("Tag:") {
                tag = Some(value.trim().to_owned());
            } else if line == "Log:" {
                while let Some(next) = lines.peek().copied() {
                    if next == "Members:" {
                        break;
                    }
                    log.push_str(lines.next().unwrap_or_default());
                    log.push('\n');
                }
            } else if line == "Members:" {
                for member in lines.by_ref() {
                    let member = member.trim();
                    if member.is_empty() {
                        continue;
                    }
                    members.push(parse_cvsps_member(member)?);
                }
            }
        }
        let (author_name, author_email) = author.ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "cvsps patchset is missing Author".into(),
        })?;
        patchsets.push(CvsPatchSet {
            number,
            date: date.ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "cvsps patchset is missing Date".into(),
            })?,
            author_name,
            author_email,
            branch: branch.unwrap_or_else(|| "HEAD".to_owned()),
            tag,
            log,
            members,
        });
    }
    Ok(patchsets)
}

fn parse_cvsps_date(value: &str) -> Result<i64> {
    let parsed =
        chrono::NaiveDateTime::parse_from_str(value, "%Y/%m/%d %H:%M:%S").map_err(|err| {
            CliError::Fatal {
                code: 128,
                message: format!("cvsps date is invalid: {err}"),
            }
        })?;
    Ok(parsed.and_utc().timestamp())
}

fn parse_cvsps_author(value: &str) -> (String, String) {
    if let Some((name, email)) = value
        .rsplit_once(" <")
        .and_then(|(name, email)| email.strip_suffix('>').map(|email| (name, email)))
    {
        (name.to_owned(), email.to_owned())
    } else {
        (value.to_owned(), value.to_owned())
    }
}

fn parse_cvsps_member(value: &str) -> Result<CvsPatchMember> {
    let (path, revs) = value.split_once(':').ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("cvsps member is malformed: {value}"),
    })?;
    let (_, new_rev) = revs.split_once("->").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("cvsps member revisions are malformed: {value}"),
    })?;
    Ok(CvsPatchMember {
        path: path.trim().trim_start_matches('/').to_owned(),
        new_rev: new_rev.trim().to_owned(),
    })
}

fn cvsimport_ref_name(options: &CvsImportOptions, branch: &str) -> String {
    if let Some(remote) = options.remote.as_deref() {
        format!("refs/remotes/{remote}/{branch}")
    } else {
        format!("refs/heads/{branch}")
    }
}

fn read_ref_index(
    store: &LooseObjectStore,
    refs: &RefStore,
    ref_name: &str,
) -> Result<BTreeMap<Vec<u8>, IndexEntry>> {
    let id = refs.resolve(ref_name)?;
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let commit = commit_cache.read_commit(&id)?;
    Ok(tree_cache
        .read_tree_to_index(&commit.tree)?
        .entries()
        .iter()
        .cloned()
        .map(|entry| (entry.path.to_vec(), entry))
        .collect())
}

fn cvsimport_fetch_revision(
    options: &CvsImportOptions,
    path: &str,
    revision: &str,
) -> Result<Vec<u8>> {
    let module_path = options
        .module
        .as_ref()
        .map(|module| format!("{module}/{path}"))
        .unwrap_or_else(|| path.to_owned());
    let mut command = foreign_scm_command("cvs");
    if let Some(root) = options.cvsroot.as_deref() {
        command.arg("-d").arg(root);
    }
    command
        .args(["-Q", "co", "-p", "-r", revision, &module_path])
        .current_dir(&options.target_dir);
    let output = command.output().map_err(CliError::Io)?;
    if !output.status.success() {
        return Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!(
                "cvs checkout failed for {module_path} {revision}: {}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        });
    }
    Ok(output.stdout)
}

fn cvsimport_log_message(patchset: &CvsPatchSet) -> Vec<u8> {
    let message = patchset.log.trim_end();
    if message.is_empty() {
        format!("PatchSet {}\n", patchset.number).into_bytes()
    } else {
        format!("{message}\n").into_bytes()
    }
}

fn sanitize_cvs_symbol(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '~' | '^' | ':' | '\\' | '*' | '?' | '['))
        .collect::<String>()
        .trim_matches('.')
        .trim_start_matches('-')
        .to_owned()
}

fn append_cvs_revision_map(repo: &GitRepo, patchset: &CvsPatchSet, id: &ObjectId) -> Result<()> {
    use std::fs::OpenOptions;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(repo.git_dir.join("cvs-revisions"))?;
    for member in &patchset.members {
        writeln!(file, "{} {} {}", member.path, member.new_rev, id.to_hex())?;
    }
    Ok(())
}

fn archimport(args: Vec<String>) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!(
            "usage: git archimport [-v] [-f] [-T] [-D depth] [-t tempdir] <archive/branch[:git-branch]>..."
        );
        return Ok(());
    }
    let options = parse_archimport_args(&args)?;
    let repo = open_or_init_cvsimport_repo(Path::new("."))?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let temp_root = match &options.temp_dir {
        Some(path) => {
            fs::create_dir_all(path)?;
            absolute_path_from_arg(path)?
        }
        None => create_cli_temp_root("zmin-archimport")?,
    };

    for (idx, root) in options.roots.iter().enumerate() {
        let ref_name = archimport_branch_ref(&root.branch)?;
        let checkout_dir = temp_root.join(format!("tree-{idx}"));
        if checkout_dir.exists() {
            fs::remove_dir_all(&checkout_dir)?;
        }
        let arch_client = std::env::var("ARCH_CLIENT").unwrap_or_else(|_| "tla".to_owned());
        run_arch_command(
            &arch_client,
            &["get", "--no-pristine", &root.revision],
            &checkout_dir,
        )?;
        let index = archimport_index_from_tree(&store, &checkout_dir)?;
        let tree = write_tree_from_index(&store, &index)?;
        let signature = Signature::new(
            "GNU Arch",
            "archimport@example.invalid",
            current_unix_timestamp()?,
            "+0000",
        )?;
        let mut builder = CommitBuilder::new(tree, signature.clone(), signature);
        if let Ok(parent) = refs.resolve(&ref_name) {
            builder = builder.parent(parent);
        }
        let message = format!(
            "Import from GNU Arch {}\n\n\
             git-archimport-id: {}\n",
            root.revision, root.revision
        );
        let id = store.write_object(
            GitObjectKind::Commit,
            &builder.message(message.into_bytes())?.encode()?,
        )?;
        refs.write_ref(&ref_name, &id)?;
        fs::create_dir_all(repo.git_dir.join("archimport/tags"))?;
        fs::write(
            repo.git_dir
                .join("archimport/tags")
                .join(archimport_private_tag_name(&root.revision)),
            format!("{}\n", id.to_hex()),
        )?;
        if idx == 0 {
            refs.write_symbolic_ref("HEAD", &ref_name)?;
            checkout_worktree(&repo, &store, &id)?;
        }
        if options.verbose {
            println!("Imported {} as {}", root.revision, id.to_hex());
        }
    }
    Ok(())
}

fn parse_archimport_args(args: &[String]) -> Result<ArchImportOptions> {
    let mut roots = Vec::new();
    let mut temp_dir = None;
    let mut verbose = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--" => {
                roots.extend(iter.map(|value| parse_archimport_root(value.as_str())));
                break;
            }
            "-v" | "--verbose" => verbose = true,
            "-f" | "-T" | "-a" => {}
            "-o" => {
                return Err(CliError::Fatal {
                    code: 129,
                    message:
                        "git archimport -o old-style branch names are intentionally unsupported"
                            .into(),
                });
            }
            "-D" => {
                let _ = next_borrowed_option_value(&mut iter, "-D")?;
            }
            "-t" => {
                temp_dir = Some(PathBuf::from(next_borrowed_option_value(&mut iter, "-t")?));
            }
            _ if arg.starts_with("-D") && arg.len() > 2 => {}
            _ if arg.starts_with("-t") && arg.len() > 2 => {
                temp_dir = Some(PathBuf::from(&arg[2..]));
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported archimport option '{arg}'"),
                });
            }
            _ => roots.push(parse_archimport_root(arg)),
        }
    }
    if roots.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "git archimport requires at least one archive/branch".into(),
        });
    }
    Ok(ArchImportOptions {
        roots,
        temp_dir,
        verbose,
    })
}

fn parse_archimport_root(value: &str) -> ArchImportRoot {
    let (revision, branch) = match value.rsplit_once(':') {
        Some((revision, branch)) if !revision.is_empty() && !branch.is_empty() => {
            (revision.to_owned(), branch.to_owned())
        }
        _ => (value.to_owned(), archimport_default_branch_name(value)),
    };
    ArchImportRoot { revision, branch }
}

fn archimport_default_branch_name(revision: &str) -> String {
    let version = revision
        .rsplit_once("--patch-")
        .or_else(|| revision.rsplit_once("--version-"))
        .or_else(|| revision.rsplit_once("--versionfix-"))
        .or_else(|| revision.rsplit_once("--base-"))
        .map(|(prefix, _)| prefix)
        .unwrap_or(revision);
    version.replace('/', ",")
}

fn archimport_branch_ref(branch: &str) -> Result<String> {
    let ref_name = if branch.starts_with("refs/") {
        branch.to_owned()
    } else {
        format!("refs/heads/{branch}")
    };
    if !check_ref_format(&ref_name, false) {
        return Err(CliError::Message(format!("invalid refname: {ref_name}")));
    }
    Ok(ref_name)
}

fn archimport_private_tag_name(revision: &str) -> String {
    revision.replace('/', ",")
}

fn run_arch_command(client: &str, args: &[&str], checkout_dir: &Path) -> Result<()> {
    let output = arch_command(client, args, checkout_dir)
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!(
                "{} {} failed: {}",
                client,
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        })
    }
}

#[cfg(windows)]
fn foreign_scm_command(program: &str) -> ProcessCommand {
    let resolved = resolve_windows_program(program).unwrap_or_else(|| PathBuf::from(program));
    let extension = resolved
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd") {
        let shell =
            std::env::var_os("COMSPEC").unwrap_or_else(|| std::ffi::OsString::from("cmd.exe"));
        let mut command = ProcessCommand::new(shell);
        command.arg("/C").arg("call").arg(resolved);
        command
    } else {
        ProcessCommand::new(resolved)
    }
}

#[cfg(not(windows))]
fn foreign_scm_command(program: &str) -> ProcessCommand {
    ProcessCommand::new(program)
}

#[cfg(windows)]
fn resolve_windows_program(program: &str) -> Option<PathBuf> {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return resolve_windows_program_path(program_path);
    }

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| resolve_windows_program_path(&dir.join(program)))
}

#[cfg(windows)]
fn resolve_windows_program_path(path: &Path) -> Option<PathBuf> {
    if path.extension().is_some() && path.is_file() {
        return Some(path.to_path_buf());
    }
    let pathext = std::env::var_os("PATHEXT")
        .unwrap_or_else(|| std::ffi::OsString::from(".COM;.EXE;.BAT;.CMD"));
    pathext
        .to_string_lossy()
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| path.with_extension(extension.trim_start_matches('.')))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn arch_command(client: &str, args: &[&str], checkout_dir: &Path) -> ProcessCommand {
    let mut command = foreign_scm_command(client);
    command.args(args).arg(checkout_dir);
    command
}

#[cfg(not(windows))]
fn arch_command(client: &str, args: &[&str], checkout_dir: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(client);
    command.args(args).arg(checkout_dir);
    command
}

fn archimport_index_from_tree(store: &LooseObjectStore, root: &Path) -> Result<GitIndex> {
    let mut entries = BTreeMap::new();
    archimport_collect_entries(store, root, root, &mut entries)?;
    Ok(GitIndex::from_entries(
        entries.values().cloned().collect::<Vec<_>>(),
    )?)
}

fn archimport_collect_entries(
    store: &LooseObjectStore,
    root: &Path,
    path: &Path,
    entries: &mut BTreeMap<Vec<u8>, IndexEntry>,
) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if archimport_ignored_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            archimport_collect_entries(store, root, &path, entries)?;
        } else if metadata.is_file() || metadata.file_type().is_symlink() {
            let relative = repo_relative_path(root, &path)?;
            let mode = if metadata.file_type().is_symlink() {
                IndexMode::Symlink
            } else {
                index_mode_for_metadata(&metadata)
            };
            let content = if metadata.file_type().is_symlink() {
                read_symlink_content(&path)?
            } else {
                fs::read(&path)?
            };
            let id = store.write_object(GitObjectKind::Blob, &content)?;
            let mut index_entry = IndexEntry::new(
                relative.clone(),
                id,
                mode,
                content.len().min(u32::MAX as usize) as u32,
            )?;
            apply_index_entry_metadata(&mut index_entry, &metadata);
            entries.insert(relative, index_entry);
        }
    }
    Ok(())
}

fn archimport_ignored_name(name: &str) -> bool {
    name == ".git"
        || name == ".arch-ids"
        || name == ".arch-inventory"
        || name == "{arch}"
        || name.starts_with('+')
        || name.starts_with(',')
}

fn create_cli_temp_root(prefix: &str) -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir();
    for attempt in 0..1024_u32 {
        let path = unique_temp_sibling(&temp_dir.join(format!("{}-{}", prefix, attempt)));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Err(CliError::Fatal {
        code: 128,
        message: "unable to create temporary directory".into(),
    })
}

fn p4(args: Vec<String>) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("sync");
    match command {
        "clone" => {
            let options = parse_p4_clone_args(&args[1..])?;
            p4_sync_impl(&options)
        }
        "sync" => {
            let options = parse_p4_sync_args(&args[1..])?;
            p4_sync_impl(&options)
        }
        "rebase" => {
            let mut options = parse_p4_sync_args(&args[1..])?;
            options.checkout = true;
            p4_sync_impl(&options)
        }
        "submit" => {
            let options = parse_p4_submit_args(&args[1..])?;
            p4_submit_impl(&options)
        }
        "--version" | "version" => {
            println!("git-p4 version {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => p4_unknown_command(command),
    }
}

fn p4_unknown_command(command: &str) -> Result<()> {
    let text = format!(
        "unknown command {command}\n\n\
         usage: git-p4 <command> [options]\n\n\
         valid commands: submit, commit, sync, rebase, clone, branches, unshelve\n\n\
         Try git-p4 <command> --help for command specific help.\n\n"
    );
    io::stdout().write_all(text.as_bytes())?;
    Err(CliError::Exit(2))
}

fn parse_p4_submit_args(args: &[String]) -> Result<P4SubmitOptions> {
    let mut branch = "refs/remotes/p4/master".to_owned();
    let mut dry_run = false;
    let mut verbose = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "-v" | "--verbose" => verbose = true,
            "--branch" => {
                branch = p4_branch_ref(next_borrowed_option_value(&mut iter, "--branch")?)
            }
            _ if arg.starts_with("--branch=") => branch = p4_branch_ref(&arg["--branch=".len()..]),
            _ if arg.starts_with('-') => {}
            _ => {}
        }
    }
    Ok(P4SubmitOptions {
        branch,
        dry_run,
        verbose,
    })
}

fn parse_p4_clone_args(args: &[String]) -> Result<P4SyncOptions> {
    let mut branch = "refs/remotes/p4/master".to_owned();
    let mut verbose = false;
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            "--branch" => {
                branch = p4_branch_ref(next_borrowed_option_value(&mut iter, "--branch")?)
            }
            _ if arg.starts_with("--branch=") => branch = p4_branch_ref(&arg["--branch=".len()..]),
            _ if arg.starts_with('-') => {}
            _ => values.push(arg.clone()),
        }
    }
    let depot_path = values.first().cloned().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "git p4 clone requires a depot path".into(),
    })?;
    let target_dir = values
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_p4_clone_dir(&depot_path)));
    Ok(P4SyncOptions {
        depot_path,
        target_dir,
        branch,
        checkout: true,
        local_master: true,
        verbose,
    })
}

fn parse_p4_sync_args(args: &[String]) -> Result<P4SyncOptions> {
    let repo = find_repo()?;
    let mut branch = "refs/remotes/p4/master".to_owned();
    let mut verbose = false;
    let mut depot_path = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            "--branch" => {
                branch = p4_branch_ref(next_borrowed_option_value(&mut iter, "--branch")?)
            }
            _ if arg.starts_with("--branch=") => branch = p4_branch_ref(&arg["--branch=".len()..]),
            _ if arg.starts_with('-') => {}
            _ => depot_path = Some(arg.clone()),
        }
    }
    let depot_path = depot_path
        .or_else(|| read_config_value(&repo, "git-p4.depotpath").ok().flatten())
        .ok_or_else(|| CliError::Fatal {
            code: 129,
            message: "git p4 sync requires a depot path or git-p4.depotpath config".into(),
        })?;
    Ok(P4SyncOptions {
        depot_path,
        target_dir: repo.root,
        branch,
        checkout: false,
        local_master: false,
        verbose,
    })
}

fn p4_branch_ref(branch: &str) -> String {
    if branch.starts_with("refs/") {
        branch.to_owned()
    } else if branch.starts_with("p4/") {
        format!("refs/remotes/{branch}")
    } else {
        format!("refs/remotes/p4/{branch}")
    }
}

fn default_p4_clone_dir(depot_path: &str) -> String {
    depot_path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty() && !value.contains('@'))
        .unwrap_or("p4-import")
        .to_owned()
}

fn p4_sync_impl(options: &P4SyncOptions) -> Result<()> {
    let repo = open_or_init_cvsimport_repo(&options.target_dir)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let files = p4_list_files(&options.depot_path)?;
    let mut entries = match refs.resolve(&options.branch) {
        Ok(id) => tree_cache
            .read_tree_to_index(&commit_cache.read_commit(&id)?.tree)?
            .entries()
            .iter()
            .cloned()
            .map(|entry| (entry.path.to_vec(), entry))
            .collect::<BTreeMap<_, _>>(),
        Err(_) => BTreeMap::new(),
    };
    for file in files {
        let relative = p4_relative_path(&options.depot_path, &file.depot_path)?;
        let path = normalize_git_path(&relative)?.into_bytes();
        if file.action == "delete" {
            entries.remove(&path);
            continue;
        }
        let content = p4_print_file(&file.depot_path, &file.revision)?;
        let id = store.write_object(GitObjectKind::Blob, &content)?;
        let entry = IndexEntry::new(
            path.clone(),
            id,
            IndexMode::File,
            content.len().min(u32::MAX as usize) as u32,
        )?;
        entries.insert(path, entry);
    }
    let index = GitIndex::from_entries(entries.values().cloned().collect::<Vec<_>>())?;
    let tree = write_tree_from_index(&store, &index)?;
    let signature = Signature::new(
        "Perforce",
        "p4@example.invalid",
        current_unix_timestamp()?,
        "+0000",
    )?;
    let mut builder = CommitBuilder::new(tree, signature.clone(), signature);
    if let Ok(parent) = refs.resolve(&options.branch) {
        builder = builder.parent(parent);
    }
    let message = format!("Import from Perforce {}\n", options.depot_path);
    let id = store.write_object(
        GitObjectKind::Commit,
        &builder.message(message.as_bytes().to_vec())?.encode()?,
    )?;
    refs.write_ref(&options.branch, &id)?;
    set_config_value(&repo, "git-p4.depotpath", &options.depot_path)?;
    if options.local_master {
        refs.write_ref("refs/heads/master", &id)?;
        refs.write_symbolic_ref("HEAD", "refs/heads/master")?;
    }
    if options.checkout {
        checkout_worktree(&repo, &store, &id)?;
    }
    if options.verbose {
        println!("Imported {} as {}", options.depot_path, id.to_hex());
    }
    Ok(())
}

fn p4_submit_impl(options: &P4SubmitOptions) -> Result<()> {
    let repo = find_repo()?;
    let depot_path =
        read_config_value(&repo, "git-p4.depotpath")?.ok_or_else(|| CliError::Fatal {
            code: 129,
            message: "git p4 submit requires git-p4.depotpath config".into(),
        })?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let base_id = refs.resolve(&options.branch)?;
    let head_id = refs.resolve("HEAD")?;
    let base_commit = commit_cache.read_commit(&base_id)?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let base_entries = tree_cache
        .read_tree_to_index(&base_commit.tree)?
        .entries()
        .iter()
        .cloned()
        .map(|entry| (entry.path.to_vec(), entry))
        .collect::<BTreeMap<_, _>>();
    let head_entries = tree_cache
        .read_tree_to_index(&head_commit.tree)?
        .entries()
        .iter()
        .cloned()
        .map(|entry| (entry.path.to_vec(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut opened = Vec::new();

    for (path, head_entry) in &head_entries {
        match base_entries.get(path) {
            None => opened.push(("add", path.clone())),
            Some(base_entry)
                if base_entry.id != head_entry.id || base_entry.mode != head_entry.mode =>
            {
                opened.push(("edit", path.clone()))
            }
            Some(_) => {}
        }
    }
    for path in base_entries.keys() {
        if !head_entries.contains_key(path) {
            opened.push(("delete", path.clone()));
        }
    }

    if opened.is_empty() {
        if options.verbose {
            println!("No changes to submit");
        }
        return Ok(());
    }

    for (action, path) in &opened {
        let path = p4_submit_path(path)?;
        if options.dry_run {
            println!("p4 {action} {path}");
        } else {
            run_p4_command_in(&repo.root, &[*action, &path])?;
        }
    }
    let description = admin_commit_subject(&head_commit.message);
    if options.dry_run {
        println!("p4 submit -d {description}");
    } else {
        run_p4_command_in(&repo.root, &["submit", "-d", &description])?;
        refs.write_ref(&options.branch, &head_id)?;
        if options.verbose {
            println!("Submitted {} to {}", head_id.to_hex(), depot_path);
        }
    }
    Ok(())
}

fn p4_submit_path(path: &[u8]) -> Result<String> {
    String::from_utf8(path.to_vec()).map_err(|_| CliError::Fatal {
        code: 128,
        message: "p4 submit path contains non-utf8 bytes".into(),
    })
}

fn admin_commit_subject(message: &[u8]) -> String {
    String::from_utf8_lossy(message)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("git p4 submit")
        .trim()
        .to_owned()
}

fn p4_list_files(depot_path: &str) -> Result<Vec<P4File>> {
    let spec = format!("{}/...", depot_path.trim_end_matches('/'));
    let output = run_p4_command(&["files", &spec])?;
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_p4_file_line)
        .collect()
}

fn parse_p4_file_line(line: &str) -> Result<P4File> {
    let (path_rev, rest) = line.split_once(" - ").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("p4 files output is malformed: {line}"),
    })?;
    let (path, revision) = path_rev.rsplit_once('#').ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("p4 file revision is malformed: {line}"),
    })?;
    let action = rest.split_whitespace().next().unwrap_or("").to_owned();
    Ok(P4File {
        depot_path: path.to_owned(),
        revision: revision.to_owned(),
        action,
    })
}

fn p4_print_file(depot_path: &str, revision: &str) -> Result<Vec<u8>> {
    let spec = format!("{depot_path}#{revision}");
    let output = foreign_scm_command("p4")
        .args(["print", "-q", &spec])
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!(
                "p4 print failed for {spec}: {}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        })
    }
}

fn run_p4_command(args: &[&str]) -> Result<String> {
    run_p4_command_in(Path::new("."), args)
}

fn run_p4_command_in(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = foreign_scm_command("p4")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!("p4 {} failed", args.join(" ")),
        })
    }
}

fn p4_relative_path(depot_root: &str, depot_file: &str) -> Result<String> {
    let root = depot_root.trim_end_matches('/').trim_end_matches("...");
    let relative = depot_file
        .strip_prefix(root)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("{depot_file} is outside {depot_root}"),
        })?
        .trim_start_matches('/');
    Ok(relative.to_owned())
}

fn svn(args: Vec<String>) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("fetch");
    match command {
        "clone" => {
            let options = parse_svn_clone_args(&args[1..])?;
            svn_sync_impl(&options)
        }
        "init" => {
            let options = parse_svn_clone_args(&args[1..])?;
            let repo = open_or_init_cvsimport_repo(&options.target_dir)?;
            set_config_value(&repo, "svn-remote.svn.url", &options.url)?;
            set_config_value(&repo, "svn-remote.svn.fetch", "refs/remotes/git-svn")?;
            Ok(())
        }
        "fetch" => {
            let options = parse_svn_fetch_args(&args[1..])?;
            svn_sync_impl(&options)
        }
        "rebase" => {
            let mut options = parse_svn_fetch_args(&args[1..])?;
            options.checkout = true;
            svn_sync_impl(&options)
        }
        "dcommit" => {
            let options = parse_svn_dcommit_args(&args[1..])?;
            svn_dcommit_impl(&options)
        }
        "--version" | "version" => {
            println!("git-svn version {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported svn command '{command}'"),
        }),
    }
}

fn parse_svn_clone_args(args: &[String]) -> Result<SvnSyncOptions> {
    let mut verbose = false;
    let mut stdlayout = false;
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            "-s" | "--stdlayout" => stdlayout = true,
            "--prefix" | "--trunk" | "-T" | "--tags" | "-t" | "--branches" | "-b" => {
                let _ = next_borrowed_option_value(&mut iter, arg)?;
            }
            _ if arg.starts_with('-') => {}
            _ => values.push(arg.clone()),
        }
    }
    let mut url = values.first().cloned().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "git svn clone requires an SVN URL".into(),
    })?;
    if stdlayout {
        url = format!("{}/trunk", url.trim_end_matches('/'));
    }
    let target_dir = values
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_svn_clone_dir(&url)));
    Ok(SvnSyncOptions {
        url,
        target_dir,
        ref_name: "refs/remotes/git-svn".to_owned(),
        checkout: true,
        local_master: true,
        verbose,
    })
}

fn parse_svn_fetch_args(args: &[String]) -> Result<SvnSyncOptions> {
    let repo = find_repo()?;
    let mut verbose = false;
    let mut url = None;
    let mut args_iter = args.iter();
    for arg in args_iter.by_ref() {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            _ if arg.starts_with('-') => {}
            _ => url = Some(arg.clone()),
        }
    }
    let url = url
        .or_else(|| {
            read_config_value(&repo, "svn-remote.svn.url")
                .ok()
                .flatten()
        })
        .ok_or_else(|| CliError::Fatal {
            code: 129,
            message: "git svn fetch requires an SVN URL or svn-remote.svn.url config".into(),
        })?;
    Ok(SvnSyncOptions {
        url,
        target_dir: repo.root,
        ref_name: "refs/remotes/git-svn".to_owned(),
        checkout: false,
        local_master: false,
        verbose,
    })
}

fn parse_svn_dcommit_args(args: &[String]) -> Result<SvnDcommitOptions> {
    let mut ref_name = "refs/remotes/git-svn".to_owned();
    let mut dry_run = false;
    let mut verbose = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "-v" | "--verbose" => verbose = true,
            "--id" => {
                let id = next_borrowed_option_value(&mut iter, "--id")?;
                ref_name = format!("refs/remotes/{id}");
            }
            _ if arg.starts_with("--id=") => {
                ref_name = format!("refs/remotes/{}", &arg["--id=".len()..]);
            }
            _ if arg.starts_with('-') => {}
            _ => {}
        }
    }
    Ok(SvnDcommitOptions {
        ref_name,
        dry_run,
        verbose,
    })
}

fn default_svn_clone_dir(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("svn-import")
        .to_owned()
}

fn svn_dcommit_impl(options: &SvnDcommitOptions) -> Result<()> {
    let repo = find_repo()?;
    let url = read_config_value(&repo, "svn-remote.svn.url")?.ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "git svn dcommit requires svn-remote.svn.url config".into(),
    })?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let base_id = refs.resolve(&options.ref_name)?;
    let head_id = refs.resolve("HEAD")?;
    let base_commit = commit_cache.read_commit(&base_id)?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let base_entries = tree_cache
        .read_tree_to_index(&base_commit.tree)?
        .entries()
        .iter()
        .cloned()
        .map(|entry| (entry.path.to_vec(), entry))
        .collect::<BTreeMap<_, _>>();
    let head_entries = tree_cache
        .read_tree_to_index(&head_commit.tree)?
        .entries()
        .iter()
        .cloned()
        .map(|entry| (entry.path.to_vec(), entry))
        .collect::<BTreeMap<_, _>>();

    let mut added = Vec::new();
    let mut deleted = Vec::new();
    let mut changed = false;
    for (path, head_entry) in &head_entries {
        match base_entries.get(path) {
            None => added.push(path.clone()),
            Some(base_entry)
                if base_entry.id != head_entry.id || base_entry.mode != head_entry.mode =>
            {
                changed = true
            }
            Some(_) => {}
        }
    }
    for path in base_entries.keys() {
        if !head_entries.contains_key(path) {
            deleted.push(path.clone());
        }
    }

    if added.is_empty() && deleted.is_empty() && !changed {
        if options.verbose {
            println!("No changes to dcommit");
        }
        return Ok(());
    }

    for path in &added {
        let path = p4_submit_path(path)?;
        if options.dry_run {
            println!("svn add {path}");
        } else {
            run_svn_command_in(&repo.root, &["add", &path])?;
        }
    }
    for path in &deleted {
        let path = p4_submit_path(path)?;
        if options.dry_run {
            println!("svn delete {path}");
        } else {
            run_svn_command_in(&repo.root, &["delete", &path])?;
        }
    }
    let message = admin_commit_subject(&head_commit.message);
    if options.dry_run {
        println!("svn commit -m {message}");
    } else {
        run_svn_command_in(&repo.root, &["commit", "-m", &message])?;
        refs.write_ref(&options.ref_name, &head_id)?;
        if options.verbose {
            println!("Committed {} to {}", head_id.to_hex(), url);
        }
    }
    Ok(())
}

fn svn_sync_impl(options: &SvnSyncOptions) -> Result<()> {
    let repo = open_or_init_cvsimport_repo(&options.target_dir)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut entries = match refs.resolve(&options.ref_name) {
        Ok(id) => tree_cache
            .read_tree_to_index(&commit_cache.read_commit(&id)?.tree)?
            .entries()
            .iter()
            .cloned()
            .map(|entry| (entry.path.to_vec(), entry))
            .collect::<BTreeMap<_, _>>(),
        Err(_) => BTreeMap::new(),
    };
    for path in svn_list_files(&options.url)? {
        let content = svn_cat_file(&options.url, &path)?;
        let id = store.write_object(GitObjectKind::Blob, &content)?;
        let path_bytes = normalize_git_path(&path)?.into_bytes();
        let entry = IndexEntry::new(
            path_bytes.clone(),
            id,
            IndexMode::File,
            content.len().min(u32::MAX as usize) as u32,
        )?;
        entries.insert(path_bytes, entry);
    }
    let index = GitIndex::from_entries(entries.values().cloned().collect::<Vec<_>>())?;
    let tree = write_tree_from_index(&store, &index)?;
    let signature = Signature::new(
        "Subversion",
        "svn@example.invalid",
        current_unix_timestamp()?,
        "+0000",
    )?;
    let mut builder = CommitBuilder::new(tree, signature.clone(), signature);
    if let Ok(parent) = refs.resolve(&options.ref_name) {
        builder = builder.parent(parent);
    }
    let message = format!("Import from Subversion {}\n", options.url);
    let id = store.write_object(
        GitObjectKind::Commit,
        &builder.message(message.as_bytes().to_vec())?.encode()?,
    )?;
    refs.write_ref(&options.ref_name, &id)?;
    set_config_value(&repo, "svn-remote.svn.url", &options.url)?;
    set_config_value(&repo, "svn-remote.svn.fetch", &options.ref_name)?;
    if options.local_master {
        refs.write_ref("refs/heads/master", &id)?;
        refs.write_symbolic_ref("HEAD", "refs/heads/master")?;
    }
    if options.checkout {
        checkout_worktree(&repo, &store, &id)?;
    }
    if options.verbose {
        println!("Imported {} as {}", options.url, id.to_hex());
    }
    Ok(())
}

fn svn_list_files(url: &str) -> Result<Vec<String>> {
    let output = run_svn_command(&["list", "-R", url])?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.ends_with('/'))
        .map(str::to_owned)
        .collect())
}

fn svn_cat_file(url: &str, path: &str) -> Result<Vec<u8>> {
    let full_url = format!("{}/{}", url.trim_end_matches('/'), path);
    let output = foreign_scm_command("svn")
        .args(["cat", &full_url])
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!(
                "svn cat failed for {full_url}: {}",
                String::from_utf8_lossy(&output.stderr).trim_end()
            ),
        })
    }
}

fn run_svn_command(args: &[&str]) -> Result<String> {
    run_svn_command_in(Path::new("."), args)
}

fn run_svn_command_in(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = foreign_scm_command("svn")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(CliError::Io)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(CliError::Fatal {
            code: output.status.code().unwrap_or(1),
            message: format!("svn {} failed", args.join(" ")),
        })
    }
}

fn instaweb(options: InstawebCommandOptions) -> Result<()> {
    if options.daemon_internal {
        let git_dir = options.git_dir.ok_or_else(|| CliError::Fatal {
            code: 129,
            message: "instaweb daemon requires --git-dir".into(),
        })?;
        let work_tree = options.work_tree.ok_or_else(|| CliError::Fatal {
            code: 129,
            message: "instaweb daemon requires --work-tree".into(),
        })?;
        return instaweb_serve(git_dir, work_tree, options.local, options.port);
    }

    let repo = find_repo()?;
    if options.restart {
        instaweb_stop(&repo)?;
        return instaweb_start(&repo, &options);
    }
    if options.stop {
        return instaweb_stop(&repo);
    }
    let _ = options.start;
    instaweb_start(&repo, &options)
}

fn instaweb_start(repo: &GitRepo, options: &InstawebCommandOptions) -> Result<()> {
    let gitweb_dir = repo.git_dir.join("gitweb");
    fs::create_dir_all(&gitweb_dir)?;
    instaweb_stop(repo)?;
    let url = format!(
        "http://{}:{}/",
        if options.local {
            "127.0.0.1"
        } else {
            "0.0.0.0"
        },
        options.port
    );
    let child = match options.httpd.as_deref().unwrap_or("builtin") {
        "zmin" | "builtin" => instaweb_spawn_builtin(repo, options)?,
        httpd => instaweb_spawn_external(repo, options, httpd)?,
    };
    fs::write(instaweb_pid_path(repo), format!("{}\n", child.id()))?;
    println!("Started git instaweb at {url}");
    if let Some(browser) = options.browser.as_deref()
        && !browser.is_empty()
    {
        let _ = ProcessCommand::new(browser).arg(&url).status();
    }
    Ok(())
}

fn instaweb_spawn_builtin(
    repo: &GitRepo,
    options: &InstawebCommandOptions,
) -> Result<std::process::Child> {
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    configure_instaweb_daemon_command(&mut command);
    Ok(command
        .args([
            "instaweb",
            "--daemon-internal",
            "--port",
            &options.port.to_string(),
            "--git-dir",
            repo.git_dir.to_str().unwrap_or_default(),
            "--work-tree",
            repo.root.to_str().unwrap_or_default(),
        ])
        .args(options.local.then_some("--local"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

fn instaweb_spawn_external(
    repo: &GitRepo,
    options: &InstawebCommandOptions,
    httpd: &str,
) -> Result<std::process::Child> {
    let mut command = ProcessCommand::new(httpd);
    configure_instaweb_daemon_command(&mut command);
    Ok(command
        .current_dir(&repo.root)
        .env("GIT_DIR", &repo.git_dir)
        .env("GIT_WORK_TREE", &repo.root)
        .env("GITWEB_PORT", options.port.to_string())
        .env(
            "GITWEB_BIND",
            if options.local {
                "127.0.0.1"
            } else {
                "0.0.0.0"
            },
        )
        .env(
            "GITWEB_CONFIG",
            repo.git_dir.join("gitweb").join("gitweb_config.perl"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?)
}

fn configure_instaweb_daemon_command(_command: &mut ProcessCommand) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;

        _command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
}

fn instaweb_stop(repo: &GitRepo) -> Result<()> {
    let pid_path = instaweb_pid_path(repo);
    let pid = match fs::read_to_string(&pid_path) {
        Ok(raw) => raw.trim().parse::<u32>().ok(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(CliError::Io(error)),
    };
    if let Some(pid) = pid {
        let _ = kill_process(pid);
    }
    remove_file_if_exists(&pid_path)
}

fn instaweb_pid_path(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("gitweb").join("pid")
}

fn kill_process(pid: u32) -> io::Result<()> {
    #[cfg(windows)]
    {
        ProcessCommand::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        ProcessCommand::new("kill")
            .arg(pid.to_string())
            .status()
            .map(|_| ())
    }
}

fn instaweb_serve(git_dir: PathBuf, work_tree: PathBuf, local: bool, port: u16) -> Result<()> {
    let bind = if local { "127.0.0.1" } else { "0.0.0.0" };
    let listener = std::net::TcpListener::bind((bind, port))?;
    let repo = GitRepo {
        root: work_tree,
        objects_dir: git_dir.join("objects"),
        index_path: git_dir.join("index"),
        git_dir,
    };
    for stream in listener.incoming() {
        let mut stream = stream?;
        if let Err(error) = instaweb_handle_connection(&repo, &mut stream) {
            let message = format!("{error:?}");
            let _ = write!(
                stream,
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                message.len(),
                message
            );
        }
    }
    Ok(())
}

fn instaweb_handle_connection(repo: &GitRepo, stream: &mut std::net::TcpStream) -> Result<()> {
    let mut request = [0_u8; 1024];
    let read = stream.read(&mut request)?;
    if read == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&request[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let body = if path == "/" {
        instaweb_index_html(repo)?
    } else {
        b"not found\n".to_vec()
    };
    let status = if path == "/" {
        "200 OK"
    } else {
        "404 Not Found"
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn instaweb_index_html(repo: &GitRepo) -> Result<Vec<u8>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let head = refs.resolve("HEAD")?;
    let commit = commit_cache.read_commit(&head)?;
    let branch = current_branch_ref(&refs)?
        .map(|name| branch_display_name(&name))
        .unwrap_or_else(|| "HEAD".to_owned());
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>git instaweb</title><h1>{}</h1><p>branch: {}</p><p>HEAD: {}</p><p>{}</p>",
        escape_html(
            repo.root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("repository")
        ),
        escape_html(&branch),
        head.to_hex(),
        escape_html(&commit_subject(&commit.message))
    );
    Ok(html.into_bytes())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
