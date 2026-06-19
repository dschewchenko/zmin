use super::*;

#[derive(Debug, Clone)]
struct ScalarEnlistment {
    enlistment: PathBuf,
    repo_root: PathBuf,
}

pub(crate) fn scalar_command(
    directories: Vec<PathBuf>,
    configs: Vec<String>,
    help: bool,
    command: Option<ScalarCommand>,
) -> Result<()> {
    let cwd = scalar_effective_cwd(directories)?;
    std::env::set_current_dir(&cwd).map_err(|error| CliError::Fatal {
        code: 128,
        message: format!("could not change to '{}': {error}", cwd.display()),
    })?;
    let mut config_entries = Vec::new();
    for config in &configs {
        match parse_global_config_entry(config) {
            Ok(entry) => config_entries.push(entry),
            Err(CliError::Stderr { text, .. }) => {
                return Err(CliError::Stderr {
                    code: 0,
                    text: format!("{text}fatal: unable to parse command-line config\n"),
                });
            }
            Err(error) => return Err(error),
        }
    }
    if !config_entries.is_empty() {
        set_global_config_entries(config_entries);
    }

    if help {
        return Err(CliError::Stderr {
            code: 129,
            text: scalar_usage(),
        });
    }

    match command {
        None => Err(CliError::Stderr {
            code: 129,
            text: scalar_usage(),
        }),
        Some(ScalarCommand::List { help, extra }) => {
            if help || !extra.is_empty() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "`scalar list` does not take arguments".into(),
                });
            }
            scalar_list()
        }
        Some(ScalarCommand::Register {
            help,
            maintenance,
            no_maintenance,
            enlistment,
            extra,
        }) => {
            if help {
                return scalar_subcommand_help("register");
            }
            scalar_reject_no_value_option("maintenance", maintenance.as_deref())?;
            scalar_reject_no_value_option("no-maintenance", no_maintenance.as_deref())?;
            if !extra.is_empty() {
                return scalar_subcommand_usage("register");
            }
            scalar_register(enlistment, maintenance, no_maintenance)
        }
        Some(ScalarCommand::Unregister {
            help,
            enlistment,
            extra,
        }) => {
            if help {
                return scalar_subcommand_help("unregister");
            }
            if let Some(option) = scalar_first_unknown_option(
                enlistment
                    .as_ref()
                    .and_then(|path| path.to_str())
                    .into_iter()
                    .chain(extra.iter().map(String::as_str)),
            ) {
                return scalar_unknown_option_usage("unregister", option);
            }
            if !extra.is_empty() {
                return scalar_subcommand_usage("unregister");
            }
            scalar_unregister(enlistment)
        }
        Some(ScalarCommand::Run {
            help,
            task,
            enlistment,
            extra,
        }) => {
            if help {
                return scalar_subcommand_help("run");
            }
            if let Some(option) = scalar_first_unknown_option(
                task.as_deref()
                    .into_iter()
                    .chain(enlistment.as_ref().and_then(|path| path.to_str()))
                    .chain(extra.iter().map(String::as_str)),
            ) {
                return scalar_unknown_option_usage("run", option);
            }
            if task.is_none() || !extra.is_empty() {
                return scalar_subcommand_usage("run");
            }
            let task = task.expect("validated scalar run task");
            scalar_run(&task, enlistment)
        }
        Some(ScalarCommand::Reconfigure {
            help,
            maintenance,
            all,
            no_all,
            enlistment,
        }) => {
            if help {
                return scalar_subcommand_help("reconfigure");
            }
            scalar_reject_no_value_option("all", all.as_deref())?;
            scalar_reject_no_value_option("no-all", no_all.as_deref())?;
            let all = all.is_some() && no_all.is_none();
            scalar_reconfigure(maintenance.last().map(String::as_str), all, enlistment)
        }
        Some(ScalarCommand::Clone(args)) => {
            if args.help {
                return scalar_subcommand_help("clone");
            }
            scalar_reject_no_value_option("single-branch", args.single_branch.as_deref())?;
            scalar_reject_no_value_option("no-single-branch", args.no_single_branch.as_deref())?;
            scalar_reject_no_value_option("no-branch", args.no_branch.as_deref())?;
            scalar_reject_no_value_option("full-clone", args.full_clone.as_deref())?;
            scalar_reject_no_value_option("no-full-clone", args.no_full_clone.as_deref())?;
            scalar_reject_no_value_option("src", args.src.as_deref())?;
            scalar_reject_no_value_option("no-src", args.no_src.as_deref())?;
            scalar_reject_no_value_option("tags", args.tags.as_deref())?;
            scalar_reject_no_value_option("no-tags", args.no_tags.as_deref())?;
            scalar_reject_no_value_option("maintenance", args.maintenance.as_deref())?;
            scalar_reject_no_value_option("no-maintenance", args.no_maintenance.as_deref())?;
            let Some(url) = args.url else {
                return scalar_clone_missing_url_usage();
            };
            if args.branch.as_deref() == Some("") {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "invalid branch name: init.defaultBranch = ".into(),
                });
            }
            scalar_clone(ScalarCloneOptions {
                single_branch: args.single_branch.is_some(),
                no_single_branch: args.no_single_branch.is_some(),
                branch: args.branch,
                no_branch: args.no_branch.is_some(),
                full_clone: args.full_clone.is_some(),
                no_full_clone: args.no_full_clone.is_some(),
                src: args.src.is_some(),
                no_src: args.no_src.is_some(),
                tags: args.tags.is_some(),
                no_tags: args.no_tags.is_some(),
                maintenance: args.maintenance.is_some(),
                no_maintenance: args.no_maintenance.is_some(),
                url,
                enlistment: args.enlistment,
            })
        }
        Some(ScalarCommand::Diagnose {
            help,
            mode,
            enlistment,
            extra,
        }) => {
            if help {
                return scalar_subcommand_help("diagnose");
            }
            if let Some(mode) = mode {
                let option = if mode.is_empty() {
                    "--mode".to_owned()
                } else {
                    format!("--mode={mode}")
                };
                return scalar_unknown_option_usage("diagnose", &option);
            }
            if let Some(option) = scalar_first_unknown_option(
                enlistment
                    .as_ref()
                    .and_then(|path| path.to_str())
                    .into_iter()
                    .chain(extra.iter().map(String::as_str)),
            ) {
                return scalar_unknown_option_usage("diagnose", option);
            }
            if !extra.is_empty() {
                return scalar_subcommand_usage("diagnose");
            }
            scalar_diagnose(enlistment)
        }
        Some(ScalarCommand::Delete {
            help,
            enlistment,
            extra,
        }) => {
            if help {
                return scalar_subcommand_help("delete");
            }
            if let Some(option) = scalar_first_unknown_option(
                enlistment
                    .as_ref()
                    .and_then(|path| path.to_str())
                    .into_iter()
                    .chain(extra.iter().map(String::as_str)),
            ) {
                return scalar_unknown_option_usage("delete", option);
            }
            if enlistment.is_none() || !extra.is_empty() {
                return scalar_subcommand_usage("delete");
            }
            scalar_delete(enlistment.expect("validated scalar delete enlistment"))
        }
        Some(ScalarCommand::Help { args }) => {
            if !args.is_empty() {
                return scalar_subcommand_usage("help");
            }
            scalar_help()
        }
        Some(ScalarCommand::Version { args }) => {
            let Some(build_options) = parse_scalar_version_args(&args)? else {
                print!("{}", scalar_subcommand_usage_text("version"));
                return Err(CliError::Exit(129));
            };
            scalar_version(build_options)
        }
        Some(ScalarCommand::Unknown(args)) => {
            let _ = args;
            Err(CliError::Stderr {
                code: 129,
                text: scalar_usage(),
            })
        }
    }
}

#[derive(Debug, Clone)]
struct ScalarCloneOptions {
    single_branch: bool,
    no_single_branch: bool,
    branch: Option<String>,
    no_branch: bool,
    full_clone: bool,
    no_full_clone: bool,
    src: bool,
    no_src: bool,
    tags: bool,
    no_tags: bool,
    maintenance: bool,
    no_maintenance: bool,
    url: String,
    enlistment: Option<PathBuf>,
}

fn scalar_usage() -> String {
    "usage: scalar [-C <directory>] [-c <key>=<value>] <command> [<options>]\n\n\
Commands:\n\tclone\n\tlist\n\tregister\n\tunregister\n\trun\n\treconfigure\n\tdelete\n\thelp\n\tversion\n\tdiagnose\n\n"
        .into()
}

fn scalar_help() -> Result<()> {
    print!(
        "SCALAR(1)                         Git Manual                         SCALAR(1)\n\n\
NAME\n\
       scalar - A tool for managing large Git repositories\n\n\
SYNOPSIS\n\
       scalar clone [--single-branch] [--branch <main-branch>] [--full-clone]\n\
               [--[no-]src] [--[no-]tags] [--[no-]maintenance] <url> [<enlistment>]\n\
       scalar list\n\
       scalar register [--[no-]maintenance] [<enlistment>]\n\
       scalar unregister [<enlistment>]\n\
       scalar run ( all | config | commit-graph | fetch | loose-objects | pack-files ) [<enlistment>]\n\
       scalar reconfigure [--maintenance=(enable|disable|keep)] [ --all | <enlistment> ]\n\
       scalar diagnose [<enlistment>]\n\
       scalar delete <enlistment>\n\n\
DESCRIPTION\n\
       Scalar is a repository management tool that optimizes Git for use in\n\
       large repositories. Scalar improves performance by configuring advanced\n\
       Git settings, maintaining repositories in the background, and helping\n\
       to reduce data sent across the network.\n\n\
       An important Scalar concept is the enlistment: this is the top-level\n\
       directory of the project. It usually contains the subdirectory src/\n\
       which is a Git worktree. This encourages separation between tracked\n\
       files inside src/ and untracked files, such as build artifacts, outside\n\
       src/. When registering an existing Git worktree whose name is not src,\n\
       the enlistment is identical to the worktree.\n\n\
       The scalar command implements subcommands with different options. With\n\
       the exception of clone, list, and reconfigure --all, subcommands expect\n\
       to be run in an enlistment.\n\n\
       The following options can be specified before the subcommand:\n\n\
       -C <directory>\n\
           Before running the subcommand, change the working directory. This\n\
           option imitates the same option of git(1).\n\n\
       -c <key>=<value>\n\
           For the duration of running the specified subcommand, configure\n\
           this setting. This option imitates the same option of git(1).\n\n\
COMMANDS\n\
   Clone\n\
       clone [<options>] <url> [<enlistment>]\n\
           Clones the specified repository, similar to git-clone(1). By\n\
           default, only commit and tree objects are cloned. Once finished,\n\
           the worktree is located at <enlistment>/src.\n\n\
       -b <name>, --branch <name>\n\
           Instead of checking out the branch pointed to by the cloned\n\
           repository's HEAD, check out the <name> branch instead.\n\n\
       --[no-]single-branch\n\
           Clone only the history leading to the tip of a single branch,\n\
           either specified by --branch or by the remote HEAD.\n\n\
       --[no-]src\n\
           By default, scalar clone places the cloned repository within an\n\
           <enlistment>/src directory. Use --no-src to place the cloned\n\
           repository directly in the <enlistment> directory.\n\n\
       --[no-]tags\n\
           By default, scalar clone fetches tag objects advertised by the\n\
           remote and future git fetch commands do the same. Use --no-tags to\n\
           avoid fetching tags during clone and to configure future fetches to\n\
           avoid tags.\n\n\
       --[no-]full-clone\n\
           A partial clone and sparse checkout are initialized by default.\n\
           This behavior can be turned off via --full-clone.\n\n\
       --[no-]maintenance\n\
           By default, scalar clone configures the enlistment to use Git's\n\
           background maintenance feature. Use --no-maintenance to skip this\n\
           configuration.\n\n\
   List\n\
       list\n\
           List enlistments that are currently registered by Scalar. This\n\
           subcommand does not need to be run inside an enlistment.\n\n\
   Register\n\
       register [--[no-]maintenance] [<enlistment>]\n\
           Add the enlistment's repository to the list of registered\n\
           repositories and start background maintenance unless maintenance is\n\
           disabled. If <enlistment> is not provided, register the enlistment\n\
           associated with the current working directory.\n\n\
   Unregister\n\
       unregister [<enlistment>]\n\
           Remove the enlistment's repository from the registered Scalar repo\n\
           list and stop Scalar-managed background maintenance.\n\n\
   Run\n\
       run <task> [<enlistment>]\n\
           Run one Scalar maintenance task. Valid tasks are all, config,\n\
           commit-graph, fetch, loose-objects, and pack-files.\n\n\
   Reconfigure\n\
       reconfigure [--maintenance=(enable|disable|keep)] [ --all | <enlistment> ]\n\
           Reapply Scalar's recommended Git configuration to one registered\n\
           enlistment, or all registered enlistments when --all is provided.\n\n\
   Diagnose\n\
       diagnose [<enlistment>]\n\
           Gather diagnostics into a git-diagnostics-*.zip archive under the\n\
           enlistment's .scalarDiagnostics directory.\n\n\
   Delete\n\
       delete <enlistment>\n\
           Unregister and remove the enlistment from disk.\n\n\
SEE ALSO\n\
       git(1), git-clone(1), git-maintenance(1), git-sparse-checkout(1)\n"
    );
    Ok(())
}

fn scalar_subcommand_help(command: &str) -> Result<()> {
    print!("{}", scalar_subcommand_usage_text(command));
    Err(CliError::Exit(129))
}

fn scalar_subcommand_usage(command: &str) -> Result<()> {
    Err(CliError::Stderr {
        code: 129,
        text: scalar_subcommand_usage_text(command).to_owned(),
    })
}

fn scalar_unknown_option_usage(command: &str, option: &str) -> Result<()> {
    Err(CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown option `{}'\n{}",
            option.trim_start_matches('-'),
            scalar_subcommand_usage_text(command)
        ),
    })
}

fn scalar_clone_missing_url_usage() -> Result<()> {
    Err(CliError::Stderr {
        code: 129,
        text: format!(
            "fatal: You must specify a repository to clone.\n\n{}",
            scalar_subcommand_usage_text("clone")
        ),
    })
}

fn scalar_first_unknown_option<'a>(args: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    args.into_iter().find(|arg| arg.starts_with('-'))
}

fn scalar_reject_no_value_option(name: &str, value: Option<&str>) -> Result<()> {
    if value.is_some_and(|value| !value.is_empty()) {
        return Err(CliError::Stderr {
            code: 129,
            text: format!("error: option `{name}' takes no value\n"),
        });
    }
    Ok(())
}

fn scalar_subcommand_usage_text(command: &str) -> &'static str {
    match command {
        "clone" => {
            concat!(
                "usage: scalar clone [--single-branch] [--branch <main-branch>] [--full-clone]\n",
                "       \t[--[no-]src] [--[no-]tags] [--[no-]maintenance] <url> [<enlistment>]\n\n",
                "    -b, --[no-]branch <branch>\n",
                "                          branch to checkout after clone\n",
                "    --[no-]full-clone     when cloning, create full working directory\n",
                "    --[no-]single-branch  only download metadata for the branch that will be checked out\n",
                "    --[no-]src            create repository within 'src' directory\n",
                "    --[no-]tags           specify if tags should be fetched during clone\n",
                "    --[no-]maintenance    specify if background maintenance should be enabled\n",
            )
        }
        "register" => concat!(
            "usage: scalar register [--[no-]maintenance] [<enlistment>]\n\n",
            "    --[no-]maintenance    specify if background maintenance should be enabled\n"
        ),
        "unregister" => "usage: scalar unregister [<enlistment>]\n",
        "run" => concat!(
            "usage: scalar run <task> [<enlistment>]\n",
            "       Tasks:\n",
            "       \tconfig\n",
            "       \tcommit-graph\n",
            "       \tfetch\n",
            "       \tloose-objects\n",
            "       \tpack-files\n",
            "       \n",
        ),
        "reconfigure" => {
            "usage: scalar reconfigure [--maintenance=(enable|disable|keep)] [--all | <enlistment>]\n\n\
    -a, --[no-]all        reconfigure all registered enlistments\n\
    --[no-]maintenance (enable|disable|keep)\n\
                          signal how to adjust background maintenance\n"
        }
        "diagnose" => "usage: scalar diagnose [<enlistment>]\n",
        "delete" => "usage: scalar delete <enlistment>\n",
        "help" => "usage: scalar help\n",
        "version" => concat!(
            "usage: scalar verbose [-v | --verbose] [--build-options]\n\n",
            "    -v, --[no-]verbose    include Git version\n",
            "    --[no-]build-options  include Git's build options\n",
        ),
        _ => unreachable!("validated scalar help command"),
    }
}

fn parse_scalar_version_args(args: &[String]) -> Result<Option<bool>> {
    let mut build_options = false;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "-v" | "--verbose" | "--no-verbose" => {}
            "--build-options" => build_options = true,
            "--no-build-options" => build_options = false,
            _ if arg.starts_with('-') => {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!(
                        "error: unknown option `{}'\n{}",
                        arg.trim_start_matches('-'),
                        scalar_subcommand_usage_text("version")
                    ),
                });
            }
            _ => return scalar_subcommand_usage("version").map(|_| Some(build_options)),
        }
    }
    Ok(Some(build_options))
}

fn scalar_version(build_options: bool) -> Result<()> {
    write_git_compatible_version(std::io::stderr().lock(), build_options).map_err(CliError::Io)
}

fn scalar_effective_cwd(directories: Vec<PathBuf>) -> Result<PathBuf> {
    let mut cwd = std::env::current_dir()?;
    for directory in directories {
        cwd = if directory.is_absolute() {
            directory
        } else {
            cwd.join(directory)
        };
    }
    Ok(cwd)
}

fn scalar_global_config_path() -> Result<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        return Err(CliError::Fatal {
            code: 128,
            message: "$HOME is unset".into(),
        });
    };
    Ok(PathBuf::from(home).join(".gitconfig"))
}

fn scalar_registered_repos() -> Result<Vec<String>> {
    let mut repos = Vec::new();
    for path in git_config_global_paths()? {
        repos.extend(
            read_config_file(&path)?
                .into_iter()
                .filter(|entry| {
                    entry.section == "scalar" && entry.subsection.is_empty() && entry.key == "repo"
                })
                .map(|entry| entry.value),
        );
    }
    Ok(repos)
}

fn scalar_list() -> Result<()> {
    for repo in scalar_registered_repos()? {
        println!("{repo}");
    }
    Ok(())
}

fn scalar_clone(options: ScalarCloneOptions) -> Result<()> {
    let ScalarCloneOptions {
        single_branch,
        no_single_branch,
        branch,
        no_branch,
        full_clone,
        no_full_clone,
        src: _src,
        no_src,
        tags: _tags,
        no_tags,
        maintenance,
        no_maintenance,
        url,
        enlistment,
    } = options;
    scalar_validate_clone_destination(enlistment.as_deref(), no_src)?;
    let enlistment = scalar_clone_enlistment(&url, enlistment)?;
    let clone_directory = if no_src {
        enlistment.clone()
    } else {
        enlistment.join("src")
    };
    let branch = if no_branch { None } else { branch };
    let clone_result = transport_commands::clone(CloneOptions {
        quiet: false,
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
        no_tags,
        single_branch,
        no_single_branch,
        separate_git_dir: None,
        references: Vec::new(),
        reference_if_able: Vec::new(),
        shared: false,
        dissociate: false,
        no_hardlinks: false,
        no_local: false,
        depth: None,
        branch,
        keep_partial_on_missing_branch: true,
        repository: url,
        directory: Some(clone_directory.clone()),
    });
    let clone_error = match clone_result {
        Ok(()) => None,
        Err(CliError::Stderr { code: 1, text })
            if text.contains("is not a commit and a branch")
                && text.contains("cannot be created from it") =>
        {
            Some(CliError::Stderr { code: 1, text })
        }
        Err(error) => return Err(error),
    };
    let cloned = scalar_enlistment(Some(if no_src {
        clone_directory
    } else {
        enlistment.clone()
    }))?;
    scalar_configure_repo(&cloned, maintenance || !no_maintenance)?;
    if no_full_clone || !full_clone {
        with_current_dir(&cloned.repo_root, || {
            set_config_value_in_file(
                &cloned.repo_root.join(".git/config"),
                "extensions.worktreeConfig",
                "true",
            )?;
            set_config_value_in_file(
                &cloned.repo_root.join(".git/config"),
                "remote.origin.promisor",
                "true",
            )?;
            set_config_value_in_file(
                &cloned.repo_root.join(".git/config"),
                "remote.origin.partialCloneFilter",
                "blob:none",
            )?;
            set_worktree_config_value(&find_repo()?, "core.sparseCheckout", "true")?;
            Ok(())
        })?;
    }
    if let Some(error) = clone_error {
        return Err(error);
    }
    let path = scalar_global_config_path()?;
    add_config_value_in_file_if_missing(
        &path,
        "scalar.repo",
        &scalar_config_path(&cloned.repo_root),
    )?;
    if maintenance || !no_maintenance {
        scalar_enable_background_maintenance(&cloned.repo_root)?;
    }
    Ok(())
}

fn scalar_validate_clone_destination(enlistment: Option<&Path>, no_src: bool) -> Result<()> {
    let Some(enlistment) = enlistment else {
        return Ok(());
    };
    if enlistment.is_dir() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("directory '{}' exists already", enlistment.display()),
        });
    }
    if enlistment.exists() {
        let target = if no_src {
            enlistment.to_path_buf()
        } else {
            enlistment.join("src")
        };
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "fatal: cannot mkdir {}: File exists\n",
                scalar_config_path(&target)
            ),
        });
    }
    Ok(())
}

fn scalar_clone_enlistment(url: &str, enlistment: Option<PathBuf>) -> Result<PathBuf> {
    let path = if let Some(path) = enlistment {
        path
    } else {
        PathBuf::from(default_clone_dir_name(url))
    };
    absolute_path_from_arg(&path)
}

fn default_clone_dir_name(url: &str) -> String {
    let trimmed = url.trim_end_matches(['/', '\\']);
    let name = trimmed
        .rsplit(['/', '\\', ':'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("repo");
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

fn scalar_register(
    enlistment: Option<PathBuf>,
    maintenance: Option<String>,
    no_maintenance: Option<String>,
) -> Result<()> {
    let enlistment = scalar_enlistment(enlistment)?;
    let enable_maintenance = maintenance.is_some() || no_maintenance.is_none();
    scalar_configure_repo(&enlistment, enable_maintenance)?;
    let path = scalar_global_config_path()?;
    let registered = scalar_config_path(&enlistment.enlistment);
    if scalar_registered_repos()?
        .iter()
        .any(|repo| repo == &registered)
    {
        println!("{registered}");
        return Ok(());
    }
    add_config_value_in_file_if_missing(&path, "scalar.repo", &registered)?;
    if enable_maintenance {
        scalar_enable_background_maintenance(&enlistment.repo_root)?;
    }
    Ok(())
}

fn scalar_unregister(enlistment: Option<PathBuf>) -> Result<()> {
    let enlistment = scalar_enlistment(enlistment)?;
    if !scalar_unregister_config(&enlistment, false)? {
        return Ok(());
    }
    with_current_dir(&enlistment.repo_root, || {
        scalar_maintenance_command("unregister", true, Vec::new())
    })?;
    println!("{}", scalar_config_path(&enlistment.enlistment));
    Ok(())
}

fn scalar_diagnose(enlistment: Option<PathBuf>) -> Result<()> {
    let enlistment = scalar_enlistment(enlistment)?;
    let output_directory = enlistment.enlistment.join(".scalarDiagnostics");
    with_current_dir(&enlistment.repo_root, || {
        admin_commands::diagnose_command_entry(Some(output_directory), Some("%Y%m%d_%H%M%S"), "all")
    })
}

fn scalar_enable_background_maintenance(repo_root: &Path) -> Result<()> {
    with_current_dir(repo_root, || {
        scalar_maintenance_command(scalar_background_maintenance_operation(), false, Vec::new())
    })
}

fn scalar_maintenance_command(operation: &str, force: bool, tasks: Vec<String>) -> Result<()> {
    maintenance_commands::maintenance(maintenance_commands::MaintenanceOptions {
        operation,
        auto: false,
        schedule: None,
        scheduler: None,
        config_file: None,
        force,
        quiet: false,
        tasks,
    })
}

#[cfg(target_os = "linux")]
fn scalar_background_maintenance_operation() -> &'static str {
    "start"
}

#[cfg(not(target_os = "linux"))]
fn scalar_background_maintenance_operation() -> &'static str {
    "register"
}

fn scalar_delete(enlistment: PathBuf) -> Result<()> {
    let enlistment = scalar_enlistment(Some(enlistment))?;
    scalar_unregister_config(&enlistment, true)?;
    with_current_dir(&enlistment.repo_root, || {
        scalar_maintenance_command("unregister", true, Vec::new())
    })?;
    let path = enlistment.enlistment.clone();
    if path.parent().is_none() || path == Path::new("/") {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "refusing to delete unsafe scalar enlistment '{}'",
                path.display()
            ),
        });
    }
    fs::remove_dir_all(&path)?;
    println!("{}", scalar_config_path(&path));
    Ok(())
}

fn scalar_unregister_config(enlistment: &ScalarEnlistment, force: bool) -> Result<bool> {
    let path = scalar_global_config_path()?;
    match remove_config_value_from_file(
        &path,
        "scalar.repo",
        &scalar_config_path(&enlistment.enlistment),
    ) {
        Ok(()) => Ok(true),
        Err(CliError::Fatal { code: 128, .. }) if force => Ok(false),
        Err(CliError::Fatal { code: 128, .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

fn scalar_run(task: &str, enlistment: Option<PathBuf>) -> Result<()> {
    let enlistment = scalar_enlistment(enlistment)?;
    match task {
        "config" => {
            scalar_configure_repo(&enlistment, true)?;
            let path = scalar_global_config_path()?;
            add_config_value_in_file_if_missing(
                &path,
                "scalar.repo",
                &scalar_config_path(&enlistment.enlistment),
            )?;
            with_current_dir(&enlistment.repo_root, || {
                scalar_maintenance_command("start", false, Vec::new())
            })
        }
        "all" => {
            scalar_configure_repo(&enlistment, true)?;
            scalar_maintenance_run(&enlistment, "commit-graph")?;
            scalar_maintenance_run(&enlistment, "prefetch")?;
            scalar_maintenance_run(&enlistment, "loose-objects")?;
            scalar_maintenance_run(&enlistment, "incremental-repack")
        }
        "commit-graph" => scalar_maintenance_run(&enlistment, "commit-graph"),
        "fetch" => scalar_maintenance_run(&enlistment, "prefetch"),
        "loose-objects" => scalar_maintenance_run(&enlistment, "loose-objects"),
        "pack-files" => scalar_maintenance_run(&enlistment, "incremental-repack"),
        other => Err(CliError::Stderr {
            code: 129,
            text: format!(
                "error: no such task: '{other}'\nusage: scalar run <task> [<enlistment>]\n"
            ),
        }),
    }
}

fn scalar_reconfigure(
    maintenance: Option<&str>,
    all: bool,
    enlistment: Option<PathBuf>,
) -> Result<()> {
    if maintenance == Some("") {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: option `maintenance' requires a value\n".into(),
        });
    }
    if all && matches!(maintenance, Some(value) if !matches!(value, "enable" | "disable" | "keep"))
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "unknown mode for --maintenance option: {}",
                maintenance.expect("validated maintenance value")
            ),
        });
    }
    if all && enlistment.is_some() {
        return Err(CliError::Stderr {
            code: 129,
            text: "fatal: --all or <enlistment>, but not both\n\n\
usage: scalar reconfigure [--maintenance=(enable|disable|keep)] [--all | <enlistment>]\n\n\
    -a, --[no-]all        reconfigure all registered enlistments\n\
    --[no-]maintenance (enable|disable|keep)\n\
                          signal how to adjust background maintenance\n"
                .into(),
        });
    }
    if all {
        for repo in scalar_registered_repos()? {
            let enlistment = scalar_enlistment(Some(PathBuf::from(repo)))?;
            scalar_configure_repo(&enlistment, false)?;
            scalar_apply_maintenance_option(&enlistment, maintenance.unwrap_or("enable"))?;
        }
        return Ok(());
    }
    let enlistment = scalar_enlistment(enlistment)?;
    let _ = maintenance;
    scalar_configure_repo(&enlistment, false)
}

fn scalar_apply_maintenance_option(enlistment: &ScalarEnlistment, option: &str) -> Result<()> {
    match option {
        "enable" => with_current_dir(&enlistment.repo_root, || {
            scalar_maintenance_command("register", false, Vec::new())
        }),
        "disable" => with_current_dir(&enlistment.repo_root, || {
            scalar_maintenance_command("unregister", true, Vec::new())
        }),
        _ => Ok(()),
    }
}

fn scalar_maintenance_run(enlistment: &ScalarEnlistment, task: &str) -> Result<()> {
    with_current_dir(&enlistment.repo_root, || {
        scalar_maintenance_command("run", false, vec![task.to_owned()])
    })
}

fn scalar_configure_repo(enlistment: &ScalarEnlistment, maintenance: bool) -> Result<()> {
    let repo = find_repo_at(&enlistment.repo_root)?;
    let config_path = repo.git_dir.join("config");
    for (name, value) in SCALAR_CONFIG {
        set_config_value_in_file(&config_path, name, value)?;
    }
    for (name, value) in scalar_platform_config() {
        set_config_value_in_file(&config_path, name, value)?;
    }
    if maintenance {
        for (name, value) in SCALAR_MAINTENANCE_CONFIG {
            set_config_value_in_file(&config_path, name, value)?;
        }
    }
    Ok(())
}

const SCALAR_CONFIG: &[(&str, &str)] = &[
    ("core.autoCRLF", "false"),
    ("core.safeCRLF", "false"),
    ("core.untrackedCache", "true"),
    ("am.keepCR", "true"),
    ("commitGraph.changedPaths", "true"),
    ("commitGraph.generationVersion", "1"),
    ("credential.https://dev.azure.com.useHttpPath", "true"),
    ("feature.experimental", "false"),
    ("feature.manyFiles", "false"),
    ("fetch.showForcedUpdates", "false"),
    ("fetch.unpackLimit", "1"),
    ("fetch.writeCommitGraph", "false"),
    ("gc.auto", "0"),
    ("gui.GCWarning", "false"),
    ("index.threads", "true"),
    ("index.version", "4"),
    ("merge.renames", "true"),
    ("merge.stat", "false"),
    ("pack.useBitmaps", "false"),
    ("receive.autoGC", "false"),
    ("status.aheadBehind", "false"),
    ("log.excludeDecoration", "refs/prefetch/*"),
];

#[cfg(target_os = "linux")]
fn scalar_platform_config() -> &'static [(&'static str, &'static str)] {
    &[
        ("core.fscache", "true"),
        ("core.multiPackIndex", "true"),
        ("core.preloadIndex", "true"),
        ("credential.validate", "false"),
        ("index.skipHash", "false"),
        ("pack.useSparse", "true"),
    ]
}

#[cfg(not(target_os = "linux"))]
fn scalar_platform_config() -> &'static [(&'static str, &'static str)] {
    &[
        ("core.fsmonitor", "true"),
        ("index.skipHash", "true"),
        ("pack.usePathWalk", "true"),
    ]
}

const SCALAR_MAINTENANCE_CONFIG: &[(&str, &str)] = &[
    ("maintenance.auto", "false"),
    ("maintenance.strategy", "incremental"),
];

fn scalar_enlistment(path: Option<PathBuf>) -> Result<ScalarEnlistment> {
    let path = match path {
        Some(path) => absolute_path_from_arg(&path)?,
        None => std::env::current_dir()?,
    };
    if let Some(enlistment) = scalar_enlistment_from_candidate(&path.join("src"), Some(&path))? {
        return Ok(enlistment);
    }
    if let Some(enlistment) = scalar_enlistment_from_candidate(&path, None)? {
        return Ok(enlistment);
    }
    if !path.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{}' does not exist", path.display()),
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: "not a git repository (or any of the parent directories): .git".into(),
    })
}

fn scalar_enlistment_from_candidate(
    repo_candidate: &Path,
    explicit_enlistment: Option<&Path>,
) -> Result<Option<ScalarEnlistment>> {
    let Ok(repo) = find_repo_at(repo_candidate) else {
        return Ok(None);
    };
    if canonical_or_absolute(repo.root.clone())
        != canonical_or_absolute(repo_candidate.to_path_buf())
    {
        return Ok(None);
    }
    let enlistment = if let Some(enlistment) = explicit_enlistment {
        canonicalize_scalar_path(enlistment)?
    } else if repo
        .root
        .file_name()
        .is_some_and(|name| name == std::ffi::OsStr::new("src"))
    {
        canonicalize_scalar_path(repo.root.parent().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!(
                "could not resolve scalar enlistment for '{}'",
                repo.root.display()
            ),
        })?)?
    } else {
        canonicalize_scalar_path(&repo.root)?
    };
    Ok(Some(ScalarEnlistment {
        enlistment,
        repo_root: canonicalize_scalar_path(&repo.root)?,
    }))
}

fn canonicalize_scalar_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|error| CliError::Fatal {
        code: 128,
        message: format!("could not canonicalize '{}': {error}", path.display()),
    })
}

fn scalar_config_path(path: &Path) -> String {
    scalar_config_path_string(path.display().to_string())
}

#[cfg(windows)]
fn scalar_config_path_string(value: String) -> String {
    let value = if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        value
    };
    value.replace('\\', "/")
}

#[cfg(not(windows))]
fn scalar_config_path_string(value: String) -> String {
    value
}

fn with_current_dir<T>(path: &Path, run: impl FnOnce() -> Result<T>) -> Result<T> {
    let previous = std::env::current_dir()?;
    std::env::set_current_dir(path)?;
    let result = run();
    std::env::set_current_dir(previous)?;
    result
}
