use super::*;

#[derive(Clone, Debug)]
struct GitmodulesEntry {
    name: String,
    path: String,
    url: String,
    branch: Option<String>,
}

pub(crate) fn clone_submodules(
    repo: &GitRepo,
    parent_repository: &str,
    active_specs: &[String],
    remote_submodules: bool,
    shallow_submodules: bool,
) -> Result<()> {
    let modules = read_gitmodules(repo)?;
    if modules.is_empty() {
        return Ok(());
    }
    set_config_value(
        repo,
        "submodule.active",
        &submodule_active_value(active_specs),
    )?;
    let index = read_repo_index(repo)?;
    for module in modules {
        if !submodule_selected(&module.path, active_specs) {
            continue;
        }
        let path_bytes = module.path.as_bytes();
        let Some(entry) = index
            .entries()
            .iter()
            .find(|entry| entry.mode == IndexMode::Gitlink && entry.path.as_slice() == path_bytes)
        else {
            continue;
        };
        let url = resolve_submodule_clone_url(parent_repository, &module.url);
        set_config_value(repo, &format!("submodule.{}.url", module.name), &url)?;
        let destination = repo.root.join(&module.path);
        run_clone_service(CloneOptions {
            quiet: false,
            configs: Vec::new(),
            template: None,
            reject_shallow: false,
            recurse_submodules: nested_submodule_specs(&module.path, active_specs),
            remote_submodules,
            shallow_submodules,
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
            references: Vec::new(),
            reference_if_able: Vec::new(),
            shared: false,
            dissociate: false,
            no_hardlinks: false,
            no_local: false,
            depth: shallow_submodules.then(|| "1".to_owned()),
            branch: None,
            keep_partial_on_missing_branch: false,
            repository: url,
            directory: Some(destination.clone()),
        })?;
        if !remote_submodules {
            checkout_submodule_gitlink(&destination, &entry.id)?;
        }
    }
    Ok(())
}

pub(crate) fn fetch_submodules_on_demand(repo: &GitRepo, remote: &str) -> Result<()> {
    let modules = read_gitmodules(repo)?;
    if modules.is_empty() {
        return Ok(());
    }

    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let tree_cache = TreeObjectCache::new(&store);
    let mut targets = BTreeMap::<String, ObjectId>::new();
    let prefix = format!("refs/remotes/{remote}/");
    refs.for_each_resolved_ref(&prefix, |ref_name, id| {
        if ref_name == format!("{prefix}HEAD") {
            return Ok::<(), CliError>(());
        }
        let tree = read_commit_tree_uncached(&store, id)?;
        let index = tree_cache.read_tree_to_index(&tree)?;
        for module in &modules {
            let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
                continue;
            };
            targets
                .entry(module.path.clone())
                .or_insert_with(|| entry.id.clone());
        }
        Ok::<(), CliError>(())
    })?;
    if targets.is_empty() {
        return Ok(());
    }

    let parent_repository = submodule_parent_repository(repo);
    for module in modules {
        let Some(target) = targets.get(&module.path) else {
            continue;
        };
        let path = repo.root.join(&module.path);
        if exact_repo_at(&path).is_none() {
            continue;
        }
        fetch_submodule_target(repo, &module, &path, &parent_repository, target)?;
        let submodule_repo = find_repo_at(&path)?;
        fetch_submodules_on_demand(&submodule_repo, "origin")?;
    }
    Ok(())
}

pub(crate) fn init_submodules(args: &[String]) -> Result<()> {
    let (quiet, paths) = parse_submodule_quiet_paths(args);
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let parent_repository = submodule_parent_repository(&repo);
    for module in modules {
        let url_key = format!("submodule.{}.url", module.name);
        if read_config_value(&repo, &url_key)?.is_some() {
            continue;
        }
        let url = resolve_submodule_clone_url(&parent_repository, &module.url);
        set_config_value(&repo, &url_key, &url)?;
        set_config_value(&repo, &format!("submodule.{}.active", module.name), "true")?;
        if !quiet {
            eprintln!(
                "Submodule '{}' ({}) registered for path '{}'",
                module.name, url, module.path
            );
        }
    }
    Ok(())
}

pub(crate) fn sync_submodules(args: &[String]) -> Result<()> {
    let (quiet, paths) = parse_submodule_quiet_paths(args);
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let parent_repository = submodule_parent_repository(&repo);
    for module in modules {
        let url = resolve_submodule_clone_url(&parent_repository, &module.url);
        set_config_value(&repo, &format!("submodule.{}.url", module.name), &url)?;
        if !quiet {
            println!("Synchronizing submodule url for '{}'", module.path);
        }
    }
    Ok(())
}

pub(crate) fn update_submodules(args: &[String]) -> Result<()> {
    let mut init = false;
    let mut recursive = false;
    let mut quiet = false;
    let mut depth = None;
    let mut single_branch = false;
    let mut no_single_branch = false;
    let mut remote = false;
    let mut no_fetch = false;
    let mut references = Vec::new();
    let mut paths = Vec::new();
    let mut path_args = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && arg == "--init" {
            init = true;
        } else if !path_args && arg == "--recursive" {
            recursive = true;
        } else if !path_args && arg == "--remote" {
            remote = true;
        } else if !path_args && (arg == "-N" || arg == "--no-fetch") {
            no_fetch = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet") {
            quiet = true;
        } else if !path_args && arg == "--no-quiet" {
            quiet = false;
        } else if !path_args && (arg == "--progress" || arg == "--no-progress") {
        } else if !path_args && arg == "--single-branch" {
            single_branch = true;
            no_single_branch = false;
        } else if !path_args && arg == "--no-single-branch" {
            single_branch = false;
            no_single_branch = true;
        } else if !path_args && arg == "--depth" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "--depth requires a value".into(),
                });
            };
            depth = Some(value.clone());
        } else if !path_args && arg.starts_with("--depth=") {
            depth = Some(arg["--depth=".len()..].to_owned());
        } else if !path_args
            && matches!(
                arg.as_str(),
                "--checkout" | "--force" | "-f" | "--recommend-shallow" | "--no-recommend-shallow"
            )
        {
        } else if !path_args && arg == "--reference" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "--reference requires a value".into(),
                });
            };
            references.push(PathBuf::from(value));
        } else if !path_args && arg.starts_with("--reference=") {
            references.push(PathBuf::from(arg["--reference=".len()..].to_owned()));
        } else if !path_args && (arg == "--jobs" || arg == "-j") {
            cursor += 1;
            if cursor >= args.len() {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("{arg} requires a value"),
                });
            }
        } else if !path_args && (arg.starts_with("--jobs=") || arg.starts_with("-j")) {
        } else if !path_args && arg.starts_with('-') {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("unsupported submodule update option '{arg}'"),
            });
        } else {
            paths.push(arg.clone());
        }
        cursor += 1;
    }
    if init {
        init_submodules(&paths)?;
    }
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let index = read_repo_index(&repo)?;
    let parent_repository = submodule_parent_repository(&repo);
    for module in modules {
        let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
            continue;
        };
        let path = repo.root.join(&module.path);
        if exact_repo_at(&path).is_none() {
            let url = read_config_value(&repo, &format!("submodule.{}.url", module.name))?
                .unwrap_or_else(|| resolve_submodule_clone_url(&parent_repository, &module.url));
            run_clone_service(CloneOptions {
                quiet,
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
                single_branch,
                no_single_branch,
                separate_git_dir: None,
                references: references.clone(),
                reference_if_able: Vec::new(),
                shared: false,
                dissociate: false,
                no_hardlinks: false,
                no_local: false,
                depth: depth.clone(),
                branch: None,
                keep_partial_on_missing_branch: false,
                repository: url,
                directory: Some(path.clone()),
            })?;
        }
        let checkout_id = if remote {
            update_submodule_remote_head(&repo, &module, &path, &parent_repository, no_fetch)?
        } else {
            entry.id.clone()
        };
        checkout_submodule_gitlink(&path, &checkout_id)?;
        absorb_submodule_gitdir(&repo, &module.path)?;
        if !quiet {
            println!(
                "Submodule path '{}': checked out '{}'",
                module.path,
                checkout_id.to_hex()
            );
        }
        if recursive {
            let submodule_repo = find_repo_at(&path)?;
            clone_submodules(
                &submodule_repo,
                &module.url,
                &[".".to_owned()],
                false,
                false,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn foreach_submodules(args: &[String]) -> Result<()> {
    let mut quiet = false;
    let mut recursive = false;
    let mut command = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if command.is_empty() && arg == "--quiet" {
            quiet = true;
        } else if command.is_empty() && arg == "--recursive" {
            recursive = true;
        } else {
            command.extend(args[cursor..].iter().cloned());
            break;
        }
        cursor += 1;
    }
    if command.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule foreach requires a command".into(),
        });
    }
    let repo = find_repo()?;
    foreach_submodules_for_repo(&repo, &command.join(" "), quiet, recursive, "")
}

pub(crate) fn deinit_submodules(args: &[String]) -> Result<()> {
    let mut force = false;
    let mut all = false;
    let mut quiet = false;
    let mut paths = Vec::new();
    let mut path_args = false;
    for arg in args {
        match arg.as_str() {
            "--" if !path_args => path_args = true,
            "-f" | "--force" if !path_args => force = true,
            "-q" | "--quiet" if !path_args => quiet = true,
            "--no-quiet" if !path_args => quiet = false,
            "--all" if !path_args => all = true,
            option if !path_args && option.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported submodule deinit option '{option}'"),
                });
            }
            path => paths.push(path.to_owned()),
        }
    }
    let repo = find_repo()?;
    let modules = if all {
        selected_gitmodules(&repo, &[])?
    } else {
        if paths.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "submodule deinit requires a path or --all".into(),
            });
        }
        selected_gitmodules(&repo, &paths)?
    };
    for module in modules {
        let path = repo.root.join(&module.path);
        if path.exists() {
            if !force && path.read_dir()?.next().is_some() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "Submodule work tree '{}' contains local modifications; use '-f' to discard them",
                        module.path
                    ),
                });
            }
            fs::remove_dir_all(&path)?;
            fs::create_dir_all(&path)?;
            if !quiet {
                println!("Cleared directory '{}'", module.path);
            }
        }
        let _ = unset_config_value(&repo, &format!("submodule.{}.url", module.name));
        let _ = unset_config_value(&repo, &format!("submodule.{}.active", module.name));
        if !quiet {
            println!(
                "Submodule '{}' ({}) unregistered for path '{}'",
                module.name, module.url, module.path
            );
        }
    }
    Ok(())
}

pub(crate) fn set_submodule_branch(args: &[String]) -> Result<()> {
    let mut default = false;
    let mut branch = None;
    let mut paths = Vec::new();
    let mut path_args = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet" || arg == "--no-quiet") {
        } else if !path_args && arg == "--default" {
            default = true;
        } else if !path_args && (arg == "-b" || arg == "--branch") {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("{arg} requires a value"),
                });
            };
            branch = Some(value.clone());
        } else if !path_args && arg.starts_with("--branch=") {
            branch = Some(arg["--branch=".len()..].to_owned());
        } else if !path_args && arg.starts_with('-') {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("unsupported submodule set-branch option '{arg}'"),
            });
        } else {
            paths.push(arg.clone());
        }
        cursor += 1;
    }
    if default == branch.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule set-branch requires exactly one of --default or --branch".into(),
        });
    }
    if paths.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule set-branch requires a path".into(),
        });
    }
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &paths)?;
    let module = modules.first().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!(
            "no submodule mapping found in .gitmodules for path '{}'",
            paths[0]
        ),
    })?;
    let gitmodules = repo.root.join(".gitmodules");
    let key = format!("submodule.{}.branch", module.name);
    if let Some(branch) = branch {
        set_config_value_in_file(&gitmodules, &key, &branch)?;
    } else {
        let _ = unset_config_value_in_file(&gitmodules, &key);
    }
    Ok(())
}

pub(crate) fn set_submodule_url(args: &[String]) -> Result<()> {
    let (quiet, values) = parse_submodule_quiet_paths(args);
    if values.len() != 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "submodule set-url requires <path> <newurl>".into(),
        });
    }
    let repo = find_repo()?;
    let modules = selected_gitmodules(&repo, &[values[0].clone()])?;
    let module = modules.first().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!(
            "no submodule mapping found in .gitmodules for path '{}'",
            values[0]
        ),
    })?;
    let resolved_url = resolve_submodule_set_url(&repo, &values[1])?;
    set_config_value_in_file(
        &repo.root.join(".gitmodules"),
        &format!("submodule.{}.url", module.name),
        &values[1],
    )?;
    set_config_value(
        &repo,
        &format!("submodule.{}.url", module.name),
        &resolved_url,
    )?;
    if !quiet {
        println!("Synchronizing submodule url for '{}'", module.path);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmoduleSummaryMode {
    Worktree,
    Cached,
    Files,
}

struct SubmoduleSummaryOptions {
    mode: SubmoduleSummaryMode,
    summary_limit: usize,
    positionals: Vec<String>,
    paths: Vec<String>,
}

pub(crate) fn summary_submodules(args: &[String]) -> Result<()> {
    let options = parse_submodule_summary_options(args)?;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let (commit, paths) = submodule_summary_commit_and_paths(&repo, &store, &options)?;
    let index = read_repo_index(&repo)?;
    let base_index = if options.mode != SubmoduleSummaryMode::Files {
        Some(submodule_summary_base_index(
            &repo,
            &store,
            commit.as_deref(),
        )?)
    } else {
        None
    };
    let modules = selected_gitmodules(&repo, &paths)?;
    for module in modules {
        let path = repo.root.join(&module.path);
        let path_bytes = module.path.as_bytes();
        let old_id = if let Some(base_index) = base_index.as_ref() {
            find_index_entry(base_index, path_bytes)
                .map(|entry| entry.id.clone())
                .unwrap_or_else(zero_object_id)
        } else {
            let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
                continue;
            };
            entry.id
        };
        let new_id = if options.mode == SubmoduleSummaryMode::Cached {
            find_index_entry(&index, path_bytes)
                .map(|entry| entry.id.clone())
                .unwrap_or_else(zero_object_id)
        } else {
            let Some(state) = submodule_head_state(&path, &old_id, false) else {
                continue;
            };
            state.id
        };
        if old_id == new_id {
            continue;
        }
        print_submodule_summary(&path, &module.path, &old_id, &new_id, options.summary_limit)?;
    }
    Ok(())
}

fn submodule_summary_commit_and_paths(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &SubmoduleSummaryOptions,
) -> Result<(Option<String>, Vec<String>)> {
    let Some(first) = options.positionals.first() else {
        return Ok((None, options.paths.clone()));
    };
    if submodule_summary_resolves_treeish(repo, store, first) {
        let mut paths = options.positionals[1..].to_vec();
        paths.extend(options.paths.iter().cloned());
        Ok((Some(first.clone()), paths))
    } else {
        let mut paths = options.positionals.clone();
        paths.extend(options.paths.iter().cloned());
        Ok((None, paths))
    }
}

fn submodule_summary_resolves_treeish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    value: &str,
) -> bool {
    let tree_cache = TreeObjectCache::new(store);
    read_treeish_index_cached(repo, store, &tree_cache, value).is_ok()
}

fn submodule_summary_base_index(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit: Option<&str>,
) -> Result<GitIndex> {
    let Some(commit) = commit else {
        return read_head_index(repo);
    };
    let tree_cache = TreeObjectCache::new(store);
    read_treeish_index_cached(repo, store, &tree_cache, commit)
}

fn parse_submodule_summary_options(args: &[String]) -> Result<SubmoduleSummaryOptions> {
    let mut mode = SubmoduleSummaryMode::Worktree;
    let mut summary_limit = 10usize;
    let mut positionals = Vec::new();
    let mut paths = Vec::new();
    let mut path_args = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet" || arg == "--no-quiet") {
        } else if !path_args && arg == "--cached" {
            mode = SubmoduleSummaryMode::Cached;
        } else if !path_args && arg == "--files" {
            mode = SubmoduleSummaryMode::Files;
        } else if !path_args && arg == "--summary-limit" {
            cursor += 1;
            let Some(value) = args.get(cursor) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "--summary-limit requires a value".into(),
                });
            };
            summary_limit = parse_submodule_summary_limit(value)?;
        } else if !path_args && arg.starts_with("--summary-limit=") {
            summary_limit = parse_submodule_summary_limit(&arg["--summary-limit=".len()..])?;
        } else if !path_args && arg.starts_with('-') {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("unsupported submodule summary option '{arg}'"),
            });
        } else if path_args {
            paths.push(arg.clone());
        } else {
            positionals.push(arg.clone());
        }
        cursor += 1;
    }
    Ok(SubmoduleSummaryOptions {
        mode,
        summary_limit,
        positionals,
        paths,
    })
}

fn parse_submodule_summary_limit(value: &str) -> Result<usize> {
    value.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 129,
        message: format!("invalid summary-limit '{value}'"),
    })
}

fn print_submodule_summary(
    path: &std::path::Path,
    display_path: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    summary_limit: usize,
) -> Result<()> {
    let submodule_repo = find_repo_at(path)?;
    let store = LooseObjectStore::new(submodule_repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commits = submodule_commit_range(&submodule_repo, &store, old_id, new_id)?;
    println!(
        "* {display_path} {}...{} ({}):",
        old_id.short_hex(7),
        new_id.short_hex(7),
        commits.len()
    );
    let commit_cache = CommitObjectCache::new(&store);
    for id in commits.iter().rev().take(summary_limit) {
        let commit = commit_cache.read_commit(id)?;
        println!("  > {}", commit_subject(&commit.message));
    }
    println!();
    Ok(())
}

fn resolve_submodule_set_url(repo: &GitRepo, url: &str) -> Result<String> {
    if !(url.starts_with("./") || url.starts_with("../")) {
        return Ok(url.to_owned());
    }
    let resolved = lexical_normalize_path(&repo.root.join(url));
    #[cfg(windows)]
    {
        return Ok(resolved.to_string_lossy().replace('\\', "/"));
    }
    #[cfg(not(windows))]
    {
        Ok(resolved.display().to_string())
    }
}

fn lexical_normalize_path(path: &std::path::Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

pub(crate) fn absorb_submodule_gitdirs(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let (_, paths) = parse_submodule_quiet_paths(args);
    for module in selected_gitmodules(&repo, &paths)? {
        absorb_submodule_gitdir(&repo, &module.path)?;
    }
    Ok(())
}

fn submodule_active_value(active_specs: &[String]) -> String {
    active_specs
        .first()
        .cloned()
        .unwrap_or_else(|| ".".to_owned())
}

fn parse_submodule_quiet_paths(args: &[String]) -> (bool, Vec<String>) {
    let mut quiet = false;
    let mut paths = Vec::new();
    let mut path_args = false;
    for arg in args {
        if !path_args && arg == "--" {
            path_args = true;
        } else if !path_args && (arg == "-q" || arg == "--quiet") {
            quiet = true;
        } else if !path_args && arg == "--no-quiet" {
            quiet = false;
        } else {
            paths.push(arg.clone());
        }
    }
    (quiet, paths)
}

fn selected_gitmodules(repo: &GitRepo, paths: &[String]) -> Result<Vec<GitmodulesEntry>> {
    let pathspecs = paths
        .iter()
        .map(|path| path.as_bytes().to_vec())
        .collect::<Vec<_>>();
    let mut modules = read_gitmodules(repo)?
        .into_iter()
        .filter(|module| pathspec_matches(module.path.as_bytes(), &pathspecs))
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.path.cmp(&right.path));
    if !paths.is_empty() && modules.is_empty() {
        return Err(CliError::Message(format!(
            "pathspec '{}' did not match any file(s) known to git",
            paths[0]
        )));
    }
    Ok(modules)
}

fn submodule_parent_repository(repo: &GitRepo) -> String {
    read_config_value(repo, "remote.origin.url")
        .ok()
        .flatten()
        .unwrap_or_else(|| repo.root.display().to_string())
}

fn submodule_gitlink_entry(index: &GitIndex, path: &str) -> Option<IndexEntry> {
    index
        .entries()
        .iter()
        .find(|entry| entry.mode == IndexMode::Gitlink && entry.path.as_slice() == path.as_bytes())
        .cloned()
}

fn submodule_selected(path: &str, active_specs: &[String]) -> bool {
    active_specs
        .iter()
        .any(|spec| spec == "." || pathspec_matches(path.as_bytes(), &[spec.as_bytes().to_vec()]))
}

fn nested_submodule_specs(path: &str, active_specs: &[String]) -> Vec<String> {
    if active_specs
        .iter()
        .any(|spec| spec == "." || pathspec_matches(path.as_bytes(), &[spec.as_bytes().to_vec()]))
    {
        vec![".".to_owned()]
    } else {
        Vec::new()
    }
}

fn read_gitmodules(repo: &GitRepo) -> Result<Vec<GitmodulesEntry>> {
    let entries = read_config_file(&repo.root.join(".gitmodules"))?;
    let mut by_name = BTreeMap::<String, (Option<String>, Option<String>, Option<String>)>::new();
    for entry in entries {
        if entry.section != "submodule" || entry.subsection.is_empty() {
            continue;
        }
        let values = by_name.entry(entry.subsection.clone()).or_default();
        match entry.key.as_str() {
            "path" => values.0 = Some(entry.value),
            "url" => values.1 = Some(entry.value),
            "branch" => values.2 = Some(entry.value),
            _ => {}
        }
    }
    Ok(by_name
        .into_iter()
        .filter_map(|(name, (path, url, branch))| {
            Some(GitmodulesEntry {
                name,
                path: path?,
                url: url?,
                branch,
            })
        })
        .collect())
}

fn resolve_submodule_clone_url(parent_repository: &str, url: &str) -> String {
    if !(url.starts_with("./") || url.starts_with("../")) {
        return url.to_owned();
    }
    let Ok(Some(parent)) = local_repository_path_from_location(parent_repository) else {
        return url.to_owned();
    };
    let base = if url.starts_with("./") {
        parent.as_path()
    } else {
        parent.parent().unwrap_or(parent.as_path())
    };
    let resolved = canonical_or_absolute(base.join(url));
    #[cfg(windows)]
    {
        return resolved.to_string_lossy().replace('\\', "/");
    }
    #[cfg(not(windows))]
    {
        resolved.display().to_string()
    }
}

fn checkout_submodule_gitlink(path: &std::path::Path, id: &ObjectId) -> Result<()> {
    let repo = find_repo_at(path)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if refs.resolve("HEAD").is_ok_and(|head| head == *id) {
        return Ok(());
    }
    checkout_worktree(&repo, &store, id)?;
    refs.write_head_direct(id)?;
    Ok(())
}

fn update_submodule_remote_head(
    repo: &GitRepo,
    module: &GitmodulesEntry,
    path: &std::path::Path,
    parent_repository: &str,
    no_fetch: bool,
) -> Result<ObjectId> {
    let submodule_repo = find_repo_at(path)?;
    let submodule_refs = RefStore::new(&submodule_repo.git_dir, GitHashAlgorithm::Sha1);
    let mut source_refs = None;
    if !no_fetch {
        let remote_url = submodule_remote_url(repo, &submodule_repo, module, parent_repository)?;
        let Some(remote_path) = local_repository_path_from_location(&remote_url)? else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "submodule update --remote cannot fetch non-local remote '{remote_url}' yet"
                ),
            });
        };
        let source = local_clone_source(&remote_path)?;
        copy_dir_contents(
            &source.common_dir.join("objects"),
            &submodule_repo.objects_dir,
        )?;
        source_refs = Some(RefStore::new(&source.git_dir, GitHashAlgorithm::Sha1));
    }
    let branch = match configured_submodule_remote_branch(repo, module)? {
        Some(branch) => branch,
        None => match source_refs.as_ref() {
            Some(refs) => default_submodule_remote_branch(refs)?,
            None => default_submodule_remote_tracking_branch(&submodule_refs)?,
        },
    };
    if branch.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "Unable to find current remote branch for submodule path '{}'",
                module.path
            ),
        });
    }
    if let Some(source_refs) = source_refs.as_ref() {
        copy_remote_refs(source_refs, &submodule_refs, "origin", Some(&branch), true)?;
    }
    submodule_refs
        .resolve(&format!("refs/remotes/origin/{branch}"))
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: format!(
                "Unable to find refs/remotes/origin/{branch} revision in submodule path '{}'",
                module.path
            ),
        })
}

fn fetch_submodule_target(
    repo: &GitRepo,
    module: &GitmodulesEntry,
    path: &std::path::Path,
    parent_repository: &str,
    target: &ObjectId,
) -> Result<()> {
    let submodule_repo = find_repo_at(path)?;
    let submodule_store =
        LooseObjectStore::new(submodule_repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if submodule_store.read_object(target).is_ok() {
        return Ok(());
    }
    let remote_url = submodule_remote_url(repo, &submodule_repo, module, parent_repository)?;
    let Some(remote_path) = local_repository_path_from_location(&remote_url)? else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "fetch --recurse-submodules cannot fetch non-local submodule remote '{remote_url}' yet"
            ),
        });
    };
    let source = local_clone_source(&remote_path)?;
    copy_dir_contents(
        &source.common_dir.join("objects"),
        &submodule_repo.objects_dir,
    )?;
    let source_refs = RefStore::new(&source.git_dir, GitHashAlgorithm::Sha1);
    let submodule_refs = RefStore::new(&submodule_repo.git_dir, GitHashAlgorithm::Sha1);
    let branch = configured_submodule_remote_branch(repo, module)?
        .or_else(|| default_submodule_remote_branch(&source_refs).ok());
    copy_remote_refs(
        &source_refs,
        &submodule_refs,
        "origin",
        branch.as_deref(),
        true,
    )?;
    submodule_store
        .read_object(target)
        .map(|_| ())
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: format!(
                "Fetched in submodule path '{}', but it did not contain {}",
                module.path,
                target.to_hex()
            ),
        })
}

fn submodule_remote_url(
    repo: &GitRepo,
    submodule_repo: &GitRepo,
    module: &GitmodulesEntry,
    parent_repository: &str,
) -> Result<String> {
    let url = read_config_value(submodule_repo, "remote.origin.url")?
        .or(read_config_value(
            repo,
            &format!("submodule.{}.url", module.name),
        )?)
        .unwrap_or_else(|| module.url.clone());
    Ok(resolve_submodule_clone_url(parent_repository, &url))
}

fn configured_submodule_remote_branch(
    repo: &GitRepo,
    module: &GitmodulesEntry,
) -> Result<Option<String>> {
    let branch = read_config_value(repo, &format!("submodule.{}.branch", module.name))?
        .or_else(|| module.branch.clone());
    match branch.as_deref() {
        Some(".") => {
            let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
            current_branch_ref(&refs)?
                .map(|name| branch_display_name(&name))
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!(
                        "submodule '{}' uses branch '.' but the superproject HEAD is detached",
                        module.path
                    ),
                })
                .map(Some)
        }
        Some(value) => Ok(Some(value.to_owned())),
        None => Ok(None),
    }
}

fn default_submodule_remote_branch(source_refs: &RefStore) -> Result<String> {
    if let Some(head) = current_branch_ref(source_refs)? {
        return Ok(branch_display_name(&head));
    }
    let head_id = source_refs.resolve("HEAD").map_err(CliError::Io)?;
    let mut branch = None;
    source_refs.for_each_resolved_ref("refs/heads/", |ref_name, id| {
        if branch.is_none() && id == &head_id {
            branch = ref_name.strip_prefix("refs/heads/").map(str::to_owned);
        }
        Ok::<(), CliError>(())
    })?;
    branch.ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "remote HEAD does not point at a branch".into(),
    })
}

fn default_submodule_remote_tracking_branch(refs: &RefStore) -> Result<String> {
    match refs.read_ref("refs/remotes/origin/HEAD") {
        Ok(RefTarget::Symbolic(target)) => target
            .strip_prefix("refs/remotes/origin/")
            .map(str::to_owned)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "origin/HEAD does not point at an origin branch".into(),
            }),
        Ok(RefTarget::Direct(id)) => {
            let mut branch = None;
            refs.for_each_resolved_ref("refs/remotes/origin/", |ref_name, ref_id| {
                if branch.is_none() && ref_name != "refs/remotes/origin/HEAD" && ref_id == &id {
                    branch = ref_name
                        .strip_prefix("refs/remotes/origin/")
                        .map(str::to_owned);
                }
                Ok::<(), CliError>(())
            })?;
            branch.ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "origin/HEAD does not match an origin branch".into(),
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(CliError::Fatal {
            code: 128,
            message: "origin/HEAD is not available; run without --no-fetch first".into(),
        }),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn absorb_submodule_gitdir(repo: &GitRepo, path: &str) -> Result<()> {
    let worktree = repo.root.join(path);
    let git_path = worktree.join(".git");
    if !git_path.exists() || git_path.is_file() {
        return Ok(());
    }
    let target = repo.git_dir.join("modules").join(path);
    if !target.exists() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&git_path, &target)?;
    } else {
        fs::remove_dir_all(&git_path)?;
    }
    fs::write(&git_path, format!("gitdir: {}\n", target.display()))?;
    set_config_value_in_file(
        &target.join("config"),
        "core.worktree",
        &worktree.display().to_string(),
    )?;
    Ok(())
}

fn foreach_submodules_for_repo(
    repo: &GitRepo,
    command: &str,
    quiet: bool,
    recursive: bool,
    prefix: &str,
) -> Result<()> {
    let index = read_repo_index(repo)?;
    let modules = selected_gitmodules(repo, &[])?;
    for module in modules {
        let Some(entry) = submodule_gitlink_entry(&index, &module.path) else {
            continue;
        };
        let path = repo.root.join(&module.path);
        if exact_repo_at(&path).is_none() {
            continue;
        }
        let display_path = format!("{prefix}{}", module.path);
        if !quiet {
            println!("Entering '{}'", display_path);
        }
        let mut shell = ProcessCommand::new("sh");
        shell.arg("-c").arg(foreach_shell_command(
            command,
            &module.name,
            &module.path,
            &display_path,
            &entry.id.to_hex(),
            &repo.root.display().to_string(),
        ));
        #[cfg(not(windows))]
        shell
            .env("name", &module.name)
            .env("sm_path", &module.path)
            .env("path", &module.path)
            .env("displaypath", &display_path)
            .env("sha1", entry.id.to_hex())
            .env("toplevel", repo.root.display().to_string());
        let status = shell.current_dir(&path).status()?;
        if !status.success() {
            return Err(CliError::Exit(status.code().unwrap_or(1)));
        }
        if recursive {
            let submodule_repo = find_repo_at(&path)?;
            foreach_submodules_for_repo(
                &submodule_repo,
                command,
                quiet,
                true,
                &format!("{display_path}/"),
            )?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn foreach_shell_command(
    command: &str,
    name: &str,
    path: &str,
    display_path: &str,
    sha1: &str,
    toplevel: &str,
) -> String {
    format!(
        "name={}; sm_path={}; path={}; displaypath={}; sha1={}; toplevel={}; export name sm_path path displaypath sha1 toplevel; {}",
        shell_quote_single(name),
        shell_quote_single(path),
        shell_quote_single(path),
        shell_quote_single(display_path),
        shell_quote_single(sha1),
        shell_quote_single(toplevel),
        command
    )
}

#[cfg(not(windows))]
fn foreach_shell_command(
    command: &str,
    _name: &str,
    _path: &str,
    _display_path: &str,
    _sha1: &str,
    _toplevel: &str,
) -> String {
    command.to_owned()
}

#[cfg(windows)]
fn shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub(crate) struct SubmoduleHeadState {
    pub(crate) prefix: char,
    pub(crate) id: ObjectId,
    pub(crate) display: String,
}

pub(crate) fn write_gitmodules_named_entry(
    repo: &GitRepo,
    name: &str,
    url: &str,
    path: &str,
    branch: Option<&str>,
) -> Result<()> {
    let gitmodules = repo.root.join(".gitmodules");
    set_config_value_in_file(&gitmodules, &format!("submodule.{name}.path"), path)?;
    set_config_value_in_file(&gitmodules, &format!("submodule.{name}.url"), url)?;
    if let Some(branch) = branch {
        set_config_value_in_file(&gitmodules, &format!("submodule.{name}.branch"), branch)?;
    }
    Ok(())
}

pub(crate) fn submodule_head_state(
    path: &std::path::Path,
    index_id: &ObjectId,
    cached: bool,
) -> Option<SubmoduleHeadState> {
    let repo = exact_repo_at(path)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD").ok()?;
    let prefix = if head_id == *index_id { ' ' } else { '+' };
    let id = if cached {
        index_id.clone()
    } else {
        head_id.clone()
    };
    Some(SubmoduleHeadState {
        prefix,
        display: submodule_head_display(&refs, &id),
        id,
    })
}

fn submodule_head_display(refs: &RefStore, id: &ObjectId) -> String {
    if let Some(branch) = current_branch_ref(refs).ok().flatten()
        && refs
            .resolve(&branch)
            .is_ok_and(|branch_id| branch_id == *id)
    {
        return branch.strip_prefix("refs/").unwrap_or(&branch).to_owned();
    }
    let mut display = None;
    let _ = refs.for_each_resolved_ref("refs/heads/", |branch, branch_id| {
        if display.is_none() && branch_id == id {
            display = Some(branch.strip_prefix("refs/").unwrap_or(branch).to_owned());
        }
        Ok::<(), CliError>(())
    });
    if let Some(display) = display {
        return display;
    }
    let mut display = None;
    let _ = refs.for_each_resolved_ref("refs/remotes/", |remote, remote_id| {
        if display.is_none() && remote_id == id {
            display = Some(remote.strip_prefix("refs/").unwrap_or(remote).to_owned());
        }
        Ok::<(), CliError>(())
    });
    if let Some(display) = display {
        return display;
    }
    short_object_id(id)
}
