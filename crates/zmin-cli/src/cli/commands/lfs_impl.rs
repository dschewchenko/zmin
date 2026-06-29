use super::*;

const LFS_POINTER_VERSION: &str = "version https://git-lfs.github.com/spec/v1";
const LFS_ATTR_SUFFIX: &str = " filter=lfs diff=lfs merge=lfs -text";
const LFS_PRE_PUSH_MARKER: &str = "# zmin-lfs-pre-push";

pub(crate) fn lfs_command(args: Vec<String>) -> Result<()> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print!("{}", lfs_usage());
        return Ok(());
    };
    match subcommand {
        "version" => lfs_version(),
        "env" => lfs_env(),
        "install" => lfs_install(&args[1..]),
        "track" => lfs_track(&args[1..]),
        "untrack" => lfs_untrack(&args[1..]),
        "ls-files" => lfs_ls_files(&args[1..]),
        "pre-push" => lfs_pre_push(&args[1..]),
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
git lfs install [--local|--worktree] [--force] [--skip-repo] [--skip-smudge]\n\
git lfs ls-files [--name-only] [--long] [--size]\n\
git lfs pre-push <remote> [remoteurl]\n\
git lfs track <pattern>...\n\
git lfs untrack <pattern>...\n\
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
    skip_repo: bool,
    skip_smudge: bool,
    scope: Option<LfsInstallScope>,
}

fn lfs_install(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let options = parse_lfs_install_options(args)?;
    let scope = options.scope.unwrap_or(LfsInstallScope::Local);
    lfs_install_filter_config(&repo, scope, options.skip_smudge, options.force)?;
    if options.skip_repo {
        println!("Git LFS initialized.");
        return Ok(());
    }
    lfs_install_pre_push_hook(&repo, options.force)?;
    println!("Updated Git hooks.");
    println!("Git LFS initialized.");
    Ok(())
}

fn parse_lfs_install_options(args: &[String]) -> Result<LfsInstallOptions> {
    let mut options = LfsInstallOptions::default();
    for arg in args {
        match arg.as_str() {
            "--force" => options.force = true,
            "--skip-repo" => options.skip_repo = true,
            "--skip-smudge" => options.skip_smudge = true,
            "--local" => options.scope = Some(LfsInstallScope::Local),
            "--worktree" => options.scope = Some(LfsInstallScope::Worktree),
            "--system" | "--manual" => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("built-in zmin lfs install does not yet support {arg}"),
                });
            }
            value if value.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("unknown lfs install option: {value}"),
                });
            }
            other => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("unexpected lfs install argument: {other}"),
                });
            }
        }
    }
    Ok(options)
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

fn lfs_install_pre_push_hook(repo: &GitRepo, force: bool) -> Result<()> {
    let hooks_dir = lfs_hooks_dir(repo)?;
    fs::create_dir_all(&hooks_dir)?;
    let hook_path = hooks_dir.join("pre-push");
    if hook_path.exists() && !lfs_hook_is_owned(&hook_path)? && !force {
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
        "#!/bin/sh\n{LFS_PRE_PUSH_MARKER}\nexec {} lfs pre-push \"$@\"\n",
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

fn lfs_hook_is_owned(path: &Path) -> io::Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let contents = fs::read_to_string(path)?;
    Ok(contents.lines().any(|line| line == LFS_PRE_PUSH_MARKER))
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
    if !repo.index_path.exists() {
        return Ok(());
    }
    let index = read_index(&repo.index_path).map_err(CliError::Io)?;
    for entry in index.entries().iter().filter(|entry| entry.stage == 0) {
        let object = match store.read_object(&entry.id) {
            Ok(object) => object,
            Err(_) => continue,
        };
        let Some(pointer) = parse_lfs_pointer(&object.content) else {
            continue;
        };
        if options.name_only {
            println!("{}", String::from_utf8_lossy(&entry.path));
            continue;
        }
        let oid = if options.long {
            pointer.oid.clone()
        } else {
            pointer.oid[..10].to_owned()
        };
        let marker = if lfs_local_object_exists(&repo, &pointer.oid) {
            '*'
        } else {
            '-'
        };
        if options.size {
            println!(
                "{oid} {marker} {} ({} B)",
                String::from_utf8_lossy(&entry.path),
                pointer.size
            );
        } else {
            println!("{oid} {marker} {}", String::from_utf8_lossy(&entry.path));
        }
    }
    Ok(())
}

#[derive(Default)]
struct LfsLsFilesOptions {
    long: bool,
    name_only: bool,
    size: bool,
}

fn parse_lfs_ls_files_options(args: &[String]) -> Result<LfsLsFilesOptions> {
    let mut options = LfsLsFilesOptions::default();
    for arg in args {
        match arg.as_str() {
            "-l" | "--long" => options.long = true,
            "-n" | "--name-only" => options.name_only = true,
            "-s" | "--size" => options.size = true,
            value if value.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("built-in zmin lfs ls-files does not yet support {value}"),
                });
            }
            other => {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!(
                        "built-in zmin lfs ls-files does not yet support ref argument {other}"
                    ),
                });
            }
        }
    }
    Ok(options)
}

fn lfs_pre_push(args: &[String]) -> Result<()> {
    if args.is_empty() || args.len() > 2 {
        return Err(CliError::Fatal {
            code: 1,
            message: "usage: git lfs pre-push <remote> [remoteurl]".into(),
        });
    }
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
            return Err(CliError::Fatal {
                code: 1,
                message: format!("invalid lfs pre-push update line: {line}"),
            });
        }
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
    if oid.len() < 4 {
        return false;
    }
    repo.git_dir
        .join("lfs")
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..4])
        .join(oid)
        .is_file()
}

fn lfs_shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
