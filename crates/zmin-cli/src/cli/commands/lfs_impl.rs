use super::*;

const LFS_POINTER_VERSION: &str = "version https://git-lfs.github.com/spec/v1";
const LFS_ATTR_SUFFIX: &str = " filter=lfs diff=lfs merge=lfs -text";
const LFS_PRE_PUSH_MARKER: &str = "# zmin-lfs-pre-push";
const LFS_POST_CHECKOUT_MARKER: &str = "# zmin-lfs-post-checkout";
const LFS_POST_COMMIT_MARKER: &str = "# zmin-lfs-post-commit";
const LFS_POST_MERGE_MARKER: &str = "# zmin-lfs-post-merge";

pub(crate) fn lfs_command(args: Vec<String>) -> Result<()> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print!("{}", lfs_usage());
        return Ok(());
    };
    match subcommand {
        "version" => lfs_version(),
        "env" => lfs_env(),
        "pull" => lfs_pull(&args[1..]),
        "install" => lfs_install(&args[1..]),
        "update" => lfs_update(&args[1..]),
        "checkout" => lfs_checkout(&args[1..]),
        "track" => lfs_track(&args[1..]),
        "untrack" => lfs_untrack(&args[1..]),
        "ls-files" => lfs_ls_files(&args[1..]),
        "pre-push" => lfs_pre_push(&args[1..]),
        "post-checkout" => lfs_post_checkout(&args[1..]),
        "post-commit" => lfs_post_commit(&args[1..]),
        "post-merge" => lfs_post_merge(&args[1..]),
        _ => Err(CliError::Stderr {
            code: 1,
            text: format!(
                "Error: unknown command \"{subcommand}\" for \"git-lfs\"\nRun 'git lfs --help' for usage.\n"
            ),
        }),
    }
}

fn lfs_usage() -> String {
    "git lfs <command> [<args>]\n\nBuilt-in Zmin Git LFS local foundation commands:\n\n\
git lfs env\n\
git lfs checkout [<path>...]\n\
git lfs install [--local|--worktree] [--force] [--skip-repo] [--skip-smudge]\n\
git lfs ls-files [--name-only] [--long] [--size] [<ref> [<ref>]]\n\
git lfs pull [<remote>]\n\
git lfs post-checkout [old] [new] [flag]\n\
git lfs post-commit\n\
git lfs post-merge [flag]\n\
git lfs pre-push <remote> [remoteurl]\n\
git lfs track <pattern>...\n\
git lfs untrack <pattern>...\n\
git lfs update [--manual | --force]\n\
git lfs version\n"
        .into()
}

fn lfs_version() -> Result<()> {
    println!(
        "git-lfs/zmin (zmin {}; built-in local foundation)",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

fn lfs_env() -> Result<()> {
    let repo = find_repo()?;
    println!(
        "git-lfs/zmin (zmin {}; built-in local foundation)",
        env!("CARGO_PKG_VERSION")
    );
    println!("git version {}", crate::runtime::GIT_COMPAT_VERSION);
    println!();
    let root = repo.root.display();
    let git_dir = repo.git_dir.display();
    let lfs_dir = repo.git_dir.join("lfs");
    let media_dir = lfs_dir.join("objects");
    let temp_dir = lfs_dir.join("tmp");
    println!("LocalWorkingDir={root}");
    println!("LocalGitDir={git_dir}");
    println!("LocalGitStorageDir={git_dir}");
    println!("LocalMediaDir={}", media_dir.display());
    println!("TempDir={}", temp_dir.display());
    for name in [
        "lfs.repositoryformatversion",
        "filter.lfs.process",
        "filter.lfs.smudge",
        "filter.lfs.clean",
        "filter.lfs.required",
    ] {
        if let Some(value) = read_config_value(&repo, name).map_err(CliError::Io)? {
            println!("git config {name} = {value}");
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum LfsInstallScope {
    Local,
    Worktree,
}

#[derive(Default)]
struct LfsInstallOptions {
    force: bool,
    manual: bool,
    skip_repo: bool,
    skip_smudge: bool,
    scope: Option<LfsInstallScope>,
}

fn lfs_install(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let options = parse_lfs_install_options(args)?;
    if options.manual {
        print!("{}", lfs_install_manual_text(&repo)?);
        println!("Git LFS initialized.");
        return Ok(());
    }
    let scope = options.scope.unwrap_or(LfsInstallScope::Local);
    lfs_install_filter_config(&repo, scope, options.skip_smudge, options.force)?;
    if options.skip_repo {
        println!("Git LFS initialized.");
        return Ok(());
    }
    lfs_install_standard_hooks(&repo, options.force)?;
    println!("Updated Git hooks.");
    println!("Git LFS initialized.");
    Ok(())
}

fn parse_lfs_install_options(args: &[String]) -> Result<LfsInstallOptions> {
    let mut options = LfsInstallOptions::default();
    for arg in args {
        match arg.as_str() {
            "--force" => options.force = true,
            "--manual" => options.manual = true,
            "--skip-repo" => options.skip_repo = true,
            "--skip-smudge" => options.skip_smudge = true,
            "--local" => options.scope = Some(LfsInstallScope::Local),
            "--worktree" => options.scope = Some(LfsInstallScope::Worktree),
            "--system" => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("built-in zmin lfs install does not yet support {arg}"),
                });
            }
            value if value.starts_with('-') => {
                print!("{}", lfs_install_usage());
                eprintln!("Error: unknown flag: {value}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
            _other => {}
        }
    }
    Ok(options)
}

fn lfs_install_manual_text(repo: &GitRepo) -> Result<String> {
    lfs_update_manual_text(repo)
}

fn lfs_install_usage() -> &'static str {
    concat!(
        "git lfs install [options]\n\n",
        "Perform the following actions to ensure that Git LFS is setup properly:\n\n",
        "* Set up the clean and smudge filters under the name \"lfs\" in the global\n",
        "  Git config.\n",
        "* Install a pre-push hook to run git lfs pre-push for the current\n",
        "  repository, if run from inside one. If \"core.hooksPath\" is configured in\n",
        "  any Git configuration (and supported, i.e., the installed Git version is\n",
        "  at least 2.9.0), then the pre-push hook will be installed to that\n",
        "  directory instead.\n\n",
        "Options:\n\n",
        "Without any options, git lfs install will only setup the \"lfs\" smudge\n",
        "and clean filters if they are not already set.\n\n",
        "--force:\n",
        "  Sets the \"lfs\" smudge and clean filters, overwriting existing values.\n",
        "--local:\n",
        "  Sets the \"lfs\" smudge and clean filters in the local repository's git config,\n",
        "  instead of the global git config (~/.gitconfig).\n",
        "--worktree:\n",
        "  Sets the \"lfs\" smudge and clean filters in the current working tree's git\n",
        "  config, instead of the global git config (~/.gitconfig) or local repository's\n",
        "  git config ($GIT_DIR/config). If multiple working trees are in use, the Git\n",
        "  config extension worktreeConfig must be enabled to use this option. If only\n",
        "  one working tree is in use, --worktree has the same effect as --local.\n",
        "  This option is only available if the installed Git version is at least 2.20.0\n",
        "  and therefore supports the \"worktreeConfig\" extension.\n",
        "--manual:\n",
        "  Print instructions for manually updating your hooks to include git-lfs\n",
        "  functionality. Use this option if git lfs install fails because of existing\n",
        "  hooks and you want to retain their functionality.\n",
        "--system:\n",
        "  Sets the \"lfs\" smudge and clean filters in the system git config, e.g.\n",
        "  /etc/gitconfig instead of the global git config (~/.gitconfig).\n",
        "--skip-smudge:\n",
        "  Skips automatic downloading of objects on clone or pull. This requires a\n",
        "  manual \"git lfs pull\" every time a new commit is checked out on your\n",
        "  repository.\n",
        "--skip-repo:\n",
        "  Skips installation of hooks into the local repository; use if you want to\n",
        "  install the LFS filters but not make changes to the hooks.  It is valid to use\n",
        "  --local, --global, or --system in conjunction with this option.\n",
    )
}

#[derive(Default)]
struct LfsUpdateOptions {
    force: bool,
    manual: bool,
}

fn lfs_update(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let options = parse_lfs_update_options(args)?;
    if options.manual {
        print!("{}", lfs_update_manual_text(&repo)?);
        return Ok(());
    }
    lfs_install_standard_hooks(&repo, options.force)?;
    println!("Updated Git hooks.");
    Ok(())
}

fn parse_lfs_update_options(args: &[String]) -> Result<LfsUpdateOptions> {
    let mut options = LfsUpdateOptions::default();
    for arg in args {
        match arg.as_str() {
            "--manual" | "-m" => options.manual = true,
            "--force" | "-f" => options.force = true,
            value if value.starts_with('-') => {
                print!("{}", lfs_update_usage());
                eprintln!("Error: unknown flag: {value}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
            other => {
                print!("{}", lfs_update_usage());
                eprintln!("Error: unknown argument: {other}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
        }
    }
    Ok(options)
}

fn lfs_update_usage() -> String {
    concat!(
        "git lfs update [--manual | --force]\n\n",
        "Updates the Git hooks used by Git LFS. Silently upgrades known hook\n",
        "contents. If you have your own custom hooks you may need to use one of\n",
        "the extended options below.\n\n",
        "Options:\n\n",
        "--manual:\n",
        "-m:\n",
        "  Print instructions for manually updating your hooks to include git-lfs\n",
        "  functionality. Use this option if git lfs update fails because of existing\n",
        "  hooks and you want to retain their functionality.\n",
        "--force:\n",
        "-f:\n",
        "  Forcibly overwrite any existing hooks with git-lfs hooks. Use this option if\n",
        "  git lfs update fails because of existing hooks but you don't care about\n",
        "  their current contents.\n"
    )
    .into()
}

fn lfs_update_manual_text(repo: &GitRepo) -> Result<String> {
    let hooks_dir = lfs_hooks_dir(repo)?;
    let hooks_root = hooks_dir
        .strip_prefix(&repo.root)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| hooks_dir.display().to_string());
    let mut text = String::new();
    for (index, (hook_name, subcommand)) in [
        ("pre-push", "pre-push"),
        ("post-checkout", "post-checkout"),
        ("post-commit", "post-commit"),
        ("post-merge", "post-merge"),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&format!(
            "Add the following to '{hooks_root}/{hook_name}':\n\n\t#!/bin/sh\n\tcommand -v git-lfs >/dev/null 2>&1 || {{ echo >&2 \"\\nThis repository is configured for Git LFS but 'git-lfs' was not found on your path. If you no longer wish to use Git LFS, remove this hook by deleting the '{hook_name}' file in the hooks directory (set by 'core.hookspath'; usually '.git/hooks').\\n\"; exit 2; }}\n\tgit lfs {subcommand} \"$@\"\n",
        ));
    }
    Ok(text)
}

fn lfs_install_filter_config(
    repo: &GitRepo,
    scope: LfsInstallScope,
    skip_smudge: bool,
    force: bool,
) -> Result<()> {
    lfs_set_config_value(repo, scope, "lfs.repositoryformatversion", "0", force)?;
    lfs_set_config_value(
        repo,
        scope,
        "filter.lfs.clean",
        "git-lfs clean -- %f",
        force,
    )?;
    let smudge = if skip_smudge {
        "git-lfs smudge --skip -- %f"
    } else {
        "git-lfs smudge -- %f"
    };
    lfs_set_config_value(repo, scope, "filter.lfs.smudge", smudge, force)?;
    let process = if skip_smudge {
        "git-lfs filter-process --skip"
    } else {
        "git-lfs filter-process"
    };
    lfs_set_config_value(repo, scope, "filter.lfs.process", process, force)?;
    lfs_set_config_value(repo, scope, "filter.lfs.required", "true", force)?;
    Ok(())
}

fn lfs_set_config_value(
    repo: &GitRepo,
    scope: LfsInstallScope,
    name: &str,
    value: &str,
    force: bool,
) -> Result<()> {
    let existing = lfs_scope_config_entry(repo, scope, name)?;
    if !force && existing.is_some() {
        return Ok(());
    }
    match scope {
        LfsInstallScope::Local => set_config_value(repo, name, value),
        LfsInstallScope::Worktree => set_worktree_config_value(repo, name, value),
    }
}

fn lfs_scope_config_entry(
    repo: &GitRepo,
    scope: LfsInstallScope,
    name: &str,
) -> Result<Option<ConfigEntry>> {
    let path = match scope {
        LfsInstallScope::Local => local_config_path(repo)?,
        LfsInstallScope::Worktree => worktree_config_path_for_scope(repo)?,
    };
    let (section, subsection, key) = parse_config_name(name).map_err(CliError::Io)?;
    Ok(read_config_file(&path)?.into_iter().rev().find(|entry| {
        entry.section == section && entry.subsection == subsection && entry.key == key
    }))
}

fn lfs_install_standard_hooks(repo: &GitRepo, force: bool) -> Result<()> {
    let hooks_dir = lfs_hooks_dir(repo)?;
    fs::create_dir_all(&hooks_dir)?;
    lfs_install_hook(
        &hooks_dir,
        "pre-push",
        LFS_PRE_PUSH_MARKER,
        "pre-push",
        force,
    )?;
    lfs_install_hook(
        &hooks_dir,
        "post-checkout",
        LFS_POST_CHECKOUT_MARKER,
        "post-checkout",
        force,
    )?;
    lfs_install_hook(
        &hooks_dir,
        "post-commit",
        LFS_POST_COMMIT_MARKER,
        "post-commit",
        force,
    )?;
    lfs_install_hook(
        &hooks_dir,
        "post-merge",
        LFS_POST_MERGE_MARKER,
        "post-merge",
        force,
    )?;
    Ok(())
}

fn lfs_install_hook(
    hooks_dir: &Path,
    hook_name: &str,
    marker: &str,
    lfs_subcommand: &str,
    force: bool,
) -> Result<()> {
    let hook_path = hooks_dir.join(hook_name);
    if hook_path.exists() && !lfs_hook_is_owned(&hook_path, marker, lfs_subcommand)? && !force {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "refusing to overwrite existing hook '{}'",
                hook_path.display()
            ),
        });
    }
    let current_exe = std::env::current_exe().map_err(CliError::Io)?;
    let script = format!(
        "#!/bin/sh\n{marker}\nexec {} lfs {lfs_subcommand} \"$@\"\n",
        lfs_shell_quote_single(&current_exe.display().to_string())
    );
    fs::write(&hook_path, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn lfs_hooks_dir(repo: &GitRepo) -> Result<PathBuf> {
    Ok(
        match read_config_value(repo, "core.hooksPath").map_err(CliError::Io)? {
            Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
            Some(path) => repo.root.join(path),
            None => repo.git_dir.join("hooks"),
        },
    )
}

fn lfs_hook_is_owned(path: &Path, marker: &str, lfs_subcommand: &str) -> io::Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let contents = fs::read_to_string(path)?;
    if contents.lines().any(|line| line == marker) {
        return Ok(true);
    }
    Ok(lfs_hook_matches_stock_git_lfs_script(
        &contents,
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default(),
        lfs_subcommand,
    ))
}

fn lfs_hook_matches_stock_git_lfs_script(
    contents: &str,
    hook_name: &str,
    lfs_subcommand: &str,
) -> bool {
    contents.starts_with("#!/bin/sh\n")
        && contents.contains(
            "This repository is configured for Git LFS but 'git-lfs' was not found on your path.",
        )
        && contents.contains(&format!(
            "deleting the '{hook_name}' file in the hooks directory"
        ))
        && contents.contains(&format!("git lfs {lfs_subcommand} \"$@\""))
}

fn lfs_track(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    if args.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "git lfs track requires at least one pattern".into(),
        });
    }
    let attributes_path = repo.root.join(".gitattributes");
    let mut lines = read_attributes_lines(&attributes_path)?;
    for pattern in args {
        let entry = lfs_attribute_line(pattern);
        if lines.iter().any(|line| line == &entry) {
            println!("\"{pattern}\" already supported");
            continue;
        }
        lines.push(entry);
        println!("Tracking \"{pattern}\"");
    }
    write_attributes_lines(&attributes_path, &lines)?;
    Ok(())
}

fn lfs_checkout(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let index = read_index(&repo.index_path).map_err(CliError::Io)?;
    let head_index = read_head_index(&repo)?;
    let path_filters = parse_lfs_checkout_paths(args)?;
    lfs_checkout_index_entries(
        &repo,
        index.entries().iter().filter(|entry| entry.stage == 0),
        &path_filters,
        Some(&head_index),
        true,
    )
}

fn lfs_checkout_index_entries<'a, I>(
    repo: &GitRepo,
    entries: I,
    path_filters: &[String],
    head_index: Option<&GitIndex>,
    print_progress: bool,
) -> Result<()>
where
    I: Iterator<Item = &'a IndexEntry>,
{
    let mut candidate_count = 0_usize;
    let mut total_size = 0_u64;
    let mut missing = Vec::new();

    for entry in entries {
        let path = String::from_utf8_lossy(&entry.path).into_owned();
        if !path_filters.is_empty() && !path_filters.iter().any(|candidate| candidate == &path) {
            continue;
        }
        let worktree_path = repo.root.join(&path);
        let Ok(content) = fs::read(&worktree_path) else {
            continue;
        };
        let Some(pointer) = parse_lfs_pointer(&content) else {
            continue;
        };
        let count_progress = head_index.is_none_or(|head| head.entry(&entry.path, 0).is_some());
        if count_progress {
            candidate_count += 1;
            total_size = total_size.saturating_add(pointer.size);
        }
        let media_path = lfs_local_media_path(&repo, &pointer.oid);
        if media_path.is_file() {
            let object_content = fs::read(&media_path)?;
            fs::write(&worktree_path, object_content)?;
        } else {
            missing.push(path);
        }
    }

    if print_progress && candidate_count > 0 {
        println!(
            "Checking out LFS objects: 100% ({candidate_count}/{candidate_count}), {total_size} B | 0 B/s, done."
        );
    }
    for path in missing {
        eprintln!("Skipped checkout for \"{path}\", content not local. Use fetch to download.");
    }
    Ok(())
}

fn parse_lfs_checkout_paths(args: &[String]) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    for arg in args {
        if arg.starts_with('-') {
            return Err(CliError::Stderr {
                code: 127,
                text: format!("Error: unknown flag: {arg}\n\n{}\n", lfs_checkout_usage()),
            });
        }
        paths.push(arg.clone());
    }
    Ok(paths)
}

fn lfs_checkout_usage() -> &'static str {
    concat!(
        "git lfs checkout [<path>...]\n\n",
        "Try to replace file pointers in the working tree with their local object content.\n",
        "Only content already present in the local Git LFS storage is checked out.\n"
    )
}

fn lfs_pull(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let index = read_index(&repo.index_path).map_err(CliError::Io)?;
    let mut remote = if args.is_empty() {
        None
    } else {
        parse_lfs_pull_remote(&repo, args)?
    };
    let mut missing = Vec::new();

    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        let path = String::from_utf8_lossy(&entry.path).into_owned();
        let worktree_path = repo.root.join(&path);
        let Ok(content) = fs::read(&worktree_path) else {
            continue;
        };
        let Some(pointer) = parse_lfs_pointer(&content) else {
            continue;
        };
        if lfs_local_media_path(&repo, &pointer.oid).is_file() {
            continue;
        }
        if remote.is_none() {
            remote = parse_lfs_pull_remote(&repo, args)?;
        }
        match remote
            .as_ref()
            .and_then(|remote| lfs_remote_media_path(remote, &pointer.oid))
        {
            Some(remote_media_path) if remote_media_path.is_file() => {
                let local_media_path = lfs_local_media_path(&repo, &pointer.oid);
                if let Some(parent) = local_media_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(remote_media_path, local_media_path)?;
            }
            _ => missing.push(pointer.oid),
        }
    }

    if !missing.is_empty() {
        return Err(lfs_pull_missing_objects_error(remote.as_ref(), &missing));
    }

    lfs_checkout_index_entries(
        &repo,
        index.entries().iter().filter(|entry| entry.stage == 0),
        &[],
        None,
        false,
    )?;
    Ok(())
}

fn parse_lfs_pull_remote(repo: &GitRepo, args: &[String]) -> Result<Option<LfsPullRemote>> {
    if args.len() > 1 {
        return Err(CliError::Fatal {
            code: 2,
            message: "usage: git lfs pull [<remote>]".into(),
        });
    }
    if let Some(arg) = args.first() {
        if arg.starts_with('-') {
            print!("{}", lfs_pull_usage());
            eprintln!("Error: unknown flag: {arg}");
            eprintln!();
            return Err(CliError::Exit(127));
        }
        return Ok(Some(resolve_lfs_pull_remote(repo, arg)?));
    }

    if let Some(remote_name) = read_config_value(repo, "branch.main.remote")
        .map_err(CliError::Io)?
        .or_else(|| {
            read_config_value(repo, "remote.origin.url")
                .ok()
                .map(|_| "origin".to_owned())
        })
    {
        return Ok(Some(resolve_lfs_pull_remote(repo, &remote_name)?));
    }
    Ok(None)
}

fn lfs_pull_usage() -> &'static str {
    concat!(
        "git lfs pull [options] [<remote>]\n\n",
        "Download Git LFS objects for the currently checked out ref, and update\n",
        "the working copy with the downloaded content if required.\n"
    )
}

struct LfsPullRemote {
    display_url: String,
    git_dir: PathBuf,
}

fn resolve_lfs_pull_remote(repo: &GitRepo, remote_name: &str) -> Result<LfsPullRemote> {
    let url_key = format!("remote.{remote_name}.url");
    let Some(url) = read_config_value(repo, &url_key).map_err(CliError::Io)? else {
        return Err(CliError::Fatal {
            code: 2,
            message: format!("remote {remote_name} not found"),
        });
    };
    let git_dir = lfs_remote_git_dir_from_url(&url).ok_or_else(|| CliError::Fatal {
        code: 2,
        message: format!("unsupported built-in zmin lfs pull remote: {url}"),
    })?;
    let display_url = if url.starts_with("file://") {
        url
    } else {
        format!("file://{}", git_dir.display())
    };
    Ok(LfsPullRemote {
        display_url,
        git_dir,
    })
}

fn lfs_remote_git_dir_from_url(url: &str) -> Option<PathBuf> {
    let path = if let Some(rest) = url.strip_prefix("file://") {
        PathBuf::from(rest)
    } else {
        PathBuf::from(url)
    };
    if path.join("objects").is_dir() && path.join("refs").exists() {
        return Some(path);
    }
    let dot_git = path.join(".git");
    if dot_git.join("objects").is_dir() && dot_git.join("refs").exists() {
        return Some(dot_git);
    }
    None
}

fn lfs_remote_media_path(remote: &LfsPullRemote, oid: &str) -> Option<PathBuf> {
    if oid.len() < 4 {
        return None;
    }
    Some(
        remote
            .git_dir
            .join("lfs")
            .join("objects")
            .join(&oid[..2])
            .join(&oid[2..4])
            .join(oid),
    )
}

fn lfs_pull_missing_objects_error(remote: Option<&LfsPullRemote>, missing: &[String]) -> CliError {
    let detail = if let Some(remote) = remote {
        format!(
            "error transferring \"{}\": [0] remote missing object {}\nFailed to fetch some objects from '{}'",
            missing[0], missing[0], remote.display_url
        )
    } else {
        "batch request: missing protocol: \"\"\nFailed to fetch some objects from ''".to_owned()
    };
    CliError::Stderr {
        code: 2,
        text: detail,
    }
}

fn lfs_untrack(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    if args.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "git lfs untrack requires at least one pattern".into(),
        });
    }
    let attributes_path = repo.root.join(".gitattributes");
    if !attributes_path.exists() {
        return Ok(());
    }
    let mut lines = read_attributes_lines(&attributes_path)?;
    let mut changed = false;
    for pattern in args {
        let entry = lfs_attribute_line(pattern);
        let before = lines.len();
        lines.retain(|line| line != &entry);
        if lines.len() != before {
            changed = true;
            println!("Untracking \"{pattern}\"");
        }
    }
    if changed {
        write_attributes_lines(&attributes_path, &lines)?;
    }
    Ok(())
}

fn lfs_ls_files(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let options = parse_lfs_ls_files_options(args)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let records = lfs_ls_files_records(&repo, &store, &options)?;
    if options.json {
        print!("{}", lfs_ls_files_json(&records));
        return Ok(());
    }
    for record in &records {
        if options.debug {
            print!("{}", lfs_ls_files_debug_record(record));
            continue;
        }
        if options.name_only {
            println!("{}", record.path);
            continue;
        }
        let oid = if options.long {
            record.oid.clone()
        } else {
            record.oid[..10].to_owned()
        };
        let marker = if record.downloaded { '*' } else { '-' };
        if options.size {
            println!("{oid} {marker} {} ({} B)", record.path, record.size);
        } else {
            println!("{oid} {marker} {}", record.path);
        }
    }
    Ok(())
}

#[derive(Default)]
struct LfsLsFilesOptions {
    all: bool,
    debug: bool,
    deleted: bool,
    json: bool,
    long: bool,
    name_only: bool,
    ref_args: Vec<String>,
    size: bool,
}

fn parse_lfs_ls_files_options(args: &[String]) -> Result<LfsLsFilesOptions> {
    let mut options = LfsLsFilesOptions::default();
    for arg in args {
        match arg.as_str() {
            "-a" | "--all" => options.all = true,
            "-d" | "--debug" => options.debug = true,
            "-l" | "--long" => options.long = true,
            "-n" | "--name-only" => options.name_only = true,
            "-s" | "--size" => options.size = true,
            "--deleted" => options.deleted = true,
            "--json" => options.json = true,
            value if value.starts_with('-') => {
                print!("{}", lfs_ls_files_usage());
                eprintln!("Error: unknown flag: {value}");
                eprintln!();
                return Err(CliError::Exit(127));
            }
            other => {
                if options.ref_args.len() >= 2 {
                    options.ref_args.push(other.to_owned());
                    break;
                }
                options.ref_args.push(other.to_owned());
            }
        }
    }
    if options.all && !options.ref_args.is_empty() {
        return Err(CliError::Stderr {
            code: 2,
            text: "Cannot use --all with explicit reference\n".into(),
        });
    }
    if options.deleted && options.ref_args.len() == 2 {
        return Err(CliError::Stderr {
            code: 2,
            text: "Cannot use --deleted with reference range\n".into(),
        });
    }
    Ok(options)
}

fn lfs_ls_files_usage() -> &'static str {
    concat!(
        "git lfs ls-files [<ref>]\n",
        "git lfs ls-files <ref> <ref>\n\n",
        "Display paths of Git LFS files that are found in the tree at the given\n",
        "reference. If no reference is given, scan the currently checked-out\n",
        "branch. If two references are given, the LFS files that are modified\n",
        "between the two references are shown; deletions are not listed.\n\n",
        "An asterisk (*) after the OID indicates a full object, a minus (-)\n",
        "indicates an LFS pointer.\n\n",
        "Options:\n\n",
        "-l:\n",
        "--long:\n",
        "   Show the entire 64 character OID, instead of just first 10.\n",
        "-s:\n",
        "--size:\n",
        "   Show the size of the LFS object between parenthesis at the end of a line.\n",
        "-d:\n",
        "--debug:\n",
        "   Show as much information as possible about a LFS file. This is intended for\n",
        "   manual inspection; the exact format may change at any time.\n",
        "-a:\n",
        "--all:\n",
        "   Inspects the full history of the repository, not the current HEAD (or other\n",
        "   provided reference). This will include previous versions of LFS objects that\n",
        "   are no longer found in the current tree.\n",
        "--deleted:\n",
        "  Shows the full history of the given reference, including objects that have\n",
        "  been deleted.\n",
        "-I <paths>:\n",
        "--include=<paths>:\n",
        "   Include paths matching only these patterns; see \"Fetch settings\".\n",
        "-X <paths>:\n",
        "--exclude=<paths>:\n",
        "   Exclude paths matching any of these patterns; see \"Fetch settings\".\n",
        "-n:\n",
        "--name-only:\n",
        "   Show only the lfs tracked file names.\n"
    )
}

#[derive(Clone)]
struct LfsLsFilesRecord {
    path: String,
    oid: String,
    size: u64,
    downloaded: bool,
}

fn lfs_ls_files_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &LfsLsFilesOptions,
) -> Result<Vec<LfsLsFilesRecord>> {
    if options.all {
        return lfs_ls_files_all_records(repo, store);
    }
    if options.deleted {
        return lfs_ls_files_deleted_records(
            repo,
            store,
            options.ref_args.first().map(String::as_str),
        );
    }
    if options.ref_args.len() == 2 {
        let revs = collect_rev_list_revs(
            repo,
            store,
            false,
            vec![format!("{}..{}", options.ref_args[0], options.ref_args[1])],
        )?;
        let commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
        let commit_cache = CommitObjectCache::new(store);
        let tree_cache = TreeObjectCache::new(store);
        let mut entries = Vec::new();
        for commit_id in commits {
            let commit = commit_cache.read_commit(&commit_id)?;
            let parent_index = if let Some(parent_id) = commit.parents.first() {
                tree_cache
                    .read_tree_to_index(&commit_cache.read_commit(parent_id)?.tree)
                    .map_err(CliError::Io)?
            } else {
                GitIndex::new()
            };
            let commit_index = tree_cache
                .read_tree_to_index(&commit.tree)
                .map_err(CliError::Io)?;
            let diff = zmin_git_core::diff::diff_indexes(&parent_index, &commit_index)
                .map_err(CliError::Io)?;
            for row in diff {
                if !matches!(
                    row.status,
                    zmin_git_core::diff::IndexDiffStatus::Added
                        | zmin_git_core::diff::IndexDiffStatus::Modified
                ) {
                    continue;
                }
                if let Some(entry) = commit_index.entry(&row.path, 0) {
                    if let Some(record) = lfs_ls_files_record(repo, store, entry) {
                        entries.push(record);
                    }
                }
            }
        }
        return Ok(entries);
    }
    if let Some(treeish) = options.ref_args.first().map(String::as_str) {
        return lfs_ls_files_tree_records(repo, store, treeish);
    }
    if !repo.index_path.exists() {
        return Ok(Vec::new());
    }
    let index = read_index(&repo.index_path).map_err(CliError::Io)?;
    Ok(index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .filter_map(|entry| lfs_ls_files_record(repo, store, entry))
        .collect())
}

fn lfs_ls_files_tree_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
    treeish: &str,
) -> Result<Vec<LfsLsFilesRecord>> {
    let tree = resolve_treeish_or_invalid_object(repo, store, treeish).map_err(|error| {
        let detail = match error {
            CliError::Fatal { message, .. } => message,
            CliError::Stderr { text, .. } => text.trim_end().to_owned(),
            CliError::Message(message) => message,
            CliError::Io(error) => error.to_string(),
            CliError::Exit(code) => format!("exit status {code}"),
        };
        CliError::Stderr {
            code: 2,
            text: format!(
                "Could not scan for Git LFS tree: error in `git ls-tree`: exit status 128 fatal: {detail}"
            ),
        }
    });
    let tree_cache = TreeObjectCache::new(store);
    let index = tree.and_then(|tree| tree_cache.read_tree_to_index(&tree).map_err(CliError::Io))?;
    Ok(index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
        .filter_map(|entry| lfs_ls_files_record(repo, store, entry))
        .collect())
}

fn lfs_ls_files_all_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
) -> Result<Vec<LfsLsFilesRecord>> {
    let revs = collect_rev_list_revs(repo, store, true, Vec::new())?;
    let commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let mut seen = HashSet::new();
    let mut records = Vec::new();
    for commit_id in commits {
        let commit = commit_cache.read_commit(&commit_id)?;
        let index = tree_cache
            .read_tree_to_index(&commit.tree)
            .map_err(CliError::Io)?;
        for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
            let Some(record) = lfs_ls_files_record(repo, store, entry) else {
                continue;
            };
            let key = (record.path.clone(), record.oid.clone());
            if seen.insert(key) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn lfs_ls_files_deleted_records(
    repo: &GitRepo,
    store: &LooseObjectStore,
    treeish: Option<&str>,
) -> Result<Vec<LfsLsFilesRecord>> {
    let target = treeish.unwrap_or("HEAD");
    let revs = collect_rev_list_revs(repo, store, false, vec![target.to_owned()])?;
    let commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
    let commit_cache = CommitObjectCache::new(store);
    let tree_cache = TreeObjectCache::new(store);
    let mut seen_paths = HashSet::new();
    let mut records = Vec::new();
    for commit_id in commits {
        let commit = commit_cache.read_commit(&commit_id)?;
        let index = tree_cache
            .read_tree_to_index(&commit.tree)
            .map_err(CliError::Io)?;
        for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
            let Some(record) = lfs_ls_files_record(repo, store, entry) else {
                continue;
            };
            if seen_paths.insert(record.path.clone()) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn lfs_ls_files_record(
    repo: &GitRepo,
    store: &LooseObjectStore,
    entry: &IndexEntry,
) -> Option<LfsLsFilesRecord> {
    let object = store.read_object(&entry.id).ok()?;
    let pointer = parse_lfs_pointer(&object.content)?;
    Some(LfsLsFilesRecord {
        path: String::from_utf8_lossy(&entry.path).into_owned(),
        downloaded: lfs_local_object_exists(repo, &pointer.oid),
        oid: pointer.oid,
        size: pointer.size,
    })
}

fn lfs_ls_files_debug_record(record: &LfsLsFilesRecord) -> String {
    format!(
        "filepath: {}\n    size: {}\ncheckout: false\ndownload: {}\n     oid: sha256 {}\n version: https://git-lfs.github.com/spec/v1\n\n",
        record.path, record.size, record.downloaded, record.oid
    )
}

fn lfs_ls_files_json(records: &[LfsLsFilesRecord]) -> String {
    if records.is_empty() {
        return "{\n \"files\": null\n}\n".into();
    }
    let mut out = String::from("{\n \"files\": [\n");
    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            out.push_str(",\n");
        }
        out.push_str("  {\n");
        out.push_str(&format!(
            "   \"name\": \"{}\",\n   \"size\": {},\n   \"checkout\": false,\n   \"downloaded\": {},\n   \"oid_type\": \"sha256\",\n   \"oid\": \"{}\",\n   \"version\": \"https://git-lfs.github.com/spec/v1\"\n",
            lfs_json_escape(&record.path),
            record.size,
            if record.downloaded { "true" } else { "false" },
            record.oid
        ));
        out.push_str("  }");
    }
    out.push_str("\n ]\n}\n");
    out
}

fn lfs_json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn lfs_pre_push(args: &[String]) -> Result<()> {
    if args.is_empty() {
        println!(
            "This should be run through Git's pre-push hook.  Run `git lfs update` to install it."
        );
        return Err(CliError::Exit(1));
    }
    if args.len() > 2 {
        return Err(CliError::Fatal {
            code: 1,
            message: "usage: git lfs pre-push <remote> [remoteurl]".into(),
        });
    }
    let repo = find_repo()?;
    ensure_remote_exists(&repo, &args[0])?;
    let mut stdin = String::new();
    io::stdin()
        .read_to_string(&mut stdin)
        .map_err(CliError::Io)?;
    for line in stdin.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 4 {
            continue;
        }
    }
    Ok(())
}

fn lfs_post_checkout(args: &[String]) -> Result<()> {
    if args.len() != 3 {
        println!(
            "This should be run through Git's post-checkout hook.  Run `git lfs update` to install it."
        );
        return Err(CliError::Exit(1));
    }
    Ok(())
}

fn lfs_post_commit(_args: &[String]) -> Result<()> {
    Ok(())
}

fn lfs_post_merge(args: &[String]) -> Result<()> {
    if args.len() != 1 {
        println!(
            "This should be run through Git's post-merge hook.  Run `git lfs update` to install it."
        );
        return Err(CliError::Exit(1));
    }
    Ok(())
}

fn read_attributes_lines(path: &Path) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)?;
    Ok(content.lines().map(str::to_owned).collect())
}

fn write_attributes_lines(path: &Path, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        fs::write(path, b"")?;
        return Ok(());
    }
    let mut content = lines.join("\n");
    content.push('\n');
    fs::write(path, content)?;
    Ok(())
}

fn lfs_attribute_line(pattern: &str) -> String {
    format!("{pattern}{LFS_ATTR_SUFFIX}")
}

struct LfsPointer {
    oid: String,
    size: u64,
}

fn parse_lfs_pointer(content: &[u8]) -> Option<LfsPointer> {
    let text = std::str::from_utf8(content).ok()?;
    let mut lines = text.lines();
    if lines.next()? != LFS_POINTER_VERSION {
        return None;
    }
    let oid = lines.next()?.strip_prefix("oid sha256:")?.trim().to_owned();
    if oid.len() != 64 || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let size = lines.next()?.strip_prefix("size ")?.trim().parse().ok()?;
    Some(LfsPointer { oid, size })
}

fn lfs_local_object_exists(repo: &GitRepo, oid: &str) -> bool {
    lfs_local_media_path(repo, oid).is_file()
}

fn lfs_local_media_path(repo: &GitRepo, oid: &str) -> PathBuf {
    if oid.len() < 4 {
        return repo.git_dir.join("lfs").join("objects").join(oid);
    }
    repo.git_dir
        .join("lfs")
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(oid)
}

fn lfs_shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
