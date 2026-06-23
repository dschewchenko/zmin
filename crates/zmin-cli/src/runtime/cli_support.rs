use std::sync::atomic::{AtomicBool, Ordering};

use clap::{CommandFactory, Parser};

use super::*;

pub(crate) const GIT_COMPAT_VERSION: &str = "2.47.1.zmin";

static BROKEN_PIPE_PANIC: AtomicBool = AtomicBool::new(false);

pub(crate) fn command_definition() -> clap::Command {
    Args::command()
}

pub(crate) fn git_compatible_version_line() -> String {
    format!(
        "git version {} (zmin {})",
        GIT_COMPAT_VERSION,
        env!("CARGO_PKG_VERSION")
    )
}

pub(crate) fn write_git_compatible_version(
    mut writer: impl std::io::Write,
    build_options: bool,
) -> io::Result<()> {
    writeln!(writer, "{}", git_compatible_version_line())?;
    if build_options {
        writeln!(writer, "cpu: {}", std::env::consts::ARCH)?;
        writeln!(writer, "no commit associated with this build")?;
        writeln!(
            writer,
            "sizeof-long: {}",
            std::mem::size_of::<std::os::raw::c_long>()
        )?;
        writeln!(writer, "sizeof-size_t: {}", std::mem::size_of::<usize>())?;
        writeln!(writer, "shell-path: {}", git_shell_path())?;
        writeln!(writer, "default-ref-format: files")?;
        writeln!(writer, "zmin-version: {}", env!("CARGO_PKG_VERSION"))?;
        writeln!(writer, "zlib: miniz_oxide")?;
        writeln!(writer, "SHA-1: zmin-git-core")?;
        writeln!(writer, "SHA-256: zmin-git-core")?;
    }
    Ok(())
}

pub(crate) fn install_broken_pipe_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if panic_info_is_broken_pipe(info) {
            BROKEN_PIPE_PANIC.store(true, Ordering::Relaxed);
            return;
        }
        default_hook(info);
    }));
}

fn panic_info_is_broken_pipe(info: &std::panic::PanicHookInfo<'_>) -> bool {
    panic_payload_is_broken_pipe(info.payload()) || broken_pipe_message(&info.to_string())
}

pub(crate) fn panic_payload_is_broken_pipe(payload: &(dyn std::any::Any + Send)) -> bool {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied());
    message.is_some_and(broken_pipe_message)
}

fn broken_pipe_message(message: &str) -> bool {
    (message.contains("failed printing to stdout")
        || message.contains("failed printing to stderr")
        || message.contains("Broken pipe"))
        && message.contains("Broken pipe")
}

pub(crate) fn broken_pipe_panic_triggered() -> bool {
    BROKEN_PIPE_PANIC.load(Ordering::Relaxed)
}

pub(crate) const EMPTY_INIT_TEMPLATE_SENTINEL: &str = "__ZMIN_EMPTY_INIT_TEMPLATE__";

pub(crate) fn parse_cli_invocation(
    program: String,
    raw_args: &[String],
) -> Result<(Args, Vec<String>)> {
    if raw_args.is_empty() {
        let mut command = Args::command();
        command.set_bin_name(program);
        print!("{}", command.render_long_help());
        return Err(CliError::Exit(1));
    }

    let (command_args, global_configs, global_repo_options, pathspec_options) =
        apply_leading_global_options(raw_args)?;
    set_global_config_entries(global_configs);
    set_global_repo_options(global_repo_options);
    set_global_pathspec_options(pathspec_options);
    let command_args = apply_command_alias(command_args)?;
    if let Some(build_options) = root_version_invocation(&command_args) {
        write_git_compatible_version(io::stdout().lock(), build_options).map_err(CliError::Io)?;
        return Err(CliError::Exit(0));
    }
    let command_args = normalize_empty_init_template(command_args);
    let command_args = normalize_history_count_shorthand(command_args);
    let command_args = normalize_log_date_hyphen_value(command_args);
    validate_version_invocation_before_clap(&command_args)?;
    validate_var_invocation_before_clap(&command_args)?;
    validate_sh_helper_invocation_before_clap(&command_args)?;
    validate_scalar_invocation_before_clap(&command_args)?;
    validate_diff_invocation_before_clap(&command_args)?;
    validate_fetch_invocation_before_clap(&command_args)?;
    validate_maintenance_invocation_before_clap(&command_args)?;
    validate_hash_object_invocation_before_clap(&command_args)?;
    let args = Args::try_parse_from(std::iter::once(program).chain(command_args.iter().cloned()))
        .unwrap_or_else(|error| error.exit());
    Ok((args, command_args))
}

fn normalize_empty_init_template(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("init") {
        return args;
    }
    args.into_iter()
        .map(|arg| {
            if arg == "--template=" {
                format!("--template={EMPTY_INIT_TEMPLATE_SENTINEL}")
            } else {
                arg
            }
        })
        .collect()
}

fn normalize_history_count_shorthand(args: Vec<String>) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return args;
    };
    if !matches!(command, "log" | "whatchanged" | "rev-list") {
        return args;
    }
    let mut normalized = Vec::with_capacity(args.len());
    let mut after_separator = false;
    for arg in args {
        if arg == "--" {
            after_separator = true;
            normalized.push(arg);
            continue;
        }
        if !after_separator {
            if let Some(value) = arg.strip_prefix('-').filter(|value| {
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                normalized.push(format!("--max-count={value}"));
                continue;
            }
        }
        normalized.push(arg);
    }
    normalized
}

fn normalize_log_date_hyphen_value(args: Vec<String>) -> Vec<String> {
    if args.first().map(String::as_str) != Some("log") {
        return args;
    }
    let mut normalized = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--date"
            && let Some(value) = args.get(index + 1)
            && value.starts_with('-')
        {
            normalized.push(format!("--date={value}"));
            index += 2;
            continue;
        }
        normalized.push(arg.clone());
        index += 1;
    }
    normalized
}

fn apply_command_alias(command_args: Vec<String>) -> Result<Vec<String>> {
    let Some(command) = command_args.first().map(String::as_str) else {
        return Ok(command_args);
    };
    if is_known_command(command) {
        return Ok(command_args);
    }
    let Some(alias) = read_alias_value(command)? else {
        return Ok(command_args);
    };
    if let Some(shell_command) = alias.strip_prefix('!') {
        let mut process = std::process::Command::new(git_shell_command_path());
        process
            .arg("-c")
            .arg(shell_alias_command(shell_command, &command_args[1..]));
        if let Ok(repo) = find_repo_or_bare() {
            process.current_dir(repo.root);
        }
        let status = process.status().map_err(CliError::Io)?;
        return Err(CliError::Exit(status.code().unwrap_or(1)));
    }
    let mut expanded = split_alias_words(&alias);
    if expanded.is_empty() {
        return Ok(command_args);
    }
    expanded.extend(command_args.into_iter().skip(1));
    Ok(expanded)
}

fn root_version_invocation(args: &[String]) -> Option<bool> {
    match args {
        [arg] if matches!(arg.as_str(), "--version" | "-v") => Some(false),
        [arg, build_options]
            if matches!(arg.as_str(), "--version" | "-v") && build_options == "--build-options" =>
        {
            Some(true)
        }
        _ => None,
    }
}

fn validate_version_invocation_before_clap(args: &[String]) -> Result<()> {
    if matches!(args, [command, option] if command == "version" && option == "--version") {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: unknown option `version'\nusage: git version [--build-options]\n\n    --[no-]build-options  also print build options\n\n".into(),
        });
    }
    Ok(())
}

fn validate_var_invocation_before_clap(args: &[String]) -> Result<()> {
    if matches!(args, [command, option] if command == "var" && option == "--list") {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git var (-l | <variable>)\n".into(),
        });
    }
    Ok(())
}

fn validate_sh_helper_invocation_before_clap(args: &[String]) -> Result<()> {
    let Some(command @ ("sh-i18n" | "sh-setup")) = args.first().map(String::as_str) else {
        return Ok(());
    };
    Err(CliError::Stderr {
        code: 1,
        text: format!("git: '{command}' is not a git command. See 'git --help'.\n"),
    })
}

fn is_known_command(command: &str) -> bool {
    Args::command().get_subcommands().any(|subcommand| {
        subcommand.get_name() == command
            || subcommand.get_all_aliases().any(|alias| alias == command)
    })
}

fn read_alias_value(name: &str) -> Result<Option<String>> {
    let mut entries = Vec::new();
    for path in system_config_paths() {
        entries.extend(read_config_file(&path)?);
    }
    for home in global_config_homes() {
        entries.extend(read_config_file(&home.join(".gitconfig"))?);
        entries.extend(read_config_file(
            &xdg_config_home(&home).join("git/config"),
        )?);
    }
    if let Ok(repo) = find_repo_or_bare() {
        entries.extend(read_config_entries(&repo)?);
    }
    entries.extend(read_bare_ancestor_alias_config()?);
    Ok(entries
        .into_iter()
        .rev()
        .find(|entry| entry.section == "alias" && entry.subsection.is_empty() && entry.key == name)
        .map(|entry| entry.value))
}

fn read_bare_ancestor_alias_config() -> Result<Vec<ConfigEntry>> {
    let mut dir = std::env::current_dir()?;
    let mut entries = Vec::new();
    while dir.pop() {
        if is_bare_git_dir(&dir) {
            entries.extend(read_config_file(&dir.join("config"))?);
            break;
        }
    }
    Ok(entries)
}

fn split_alias_words(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}

fn shell_alias_command(command: &str, args: &[String]) -> String {
    std::iter::once(command.to_owned())
        .chain(args.iter().map(|arg| shell_quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn validate_scalar_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) == Some("scalar")
        && command_args.get(1).map(String::as_str) == Some("-C")
        && command_args.get(2).is_none()
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "-C requires a <directory>".into(),
        });
    }
    Ok(())
}

fn validate_diff_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("diff") {
        return Ok(());
    }
    if command_args.iter().skip(1).any(|arg| arg == "--no-rename") {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: invalid option: --no-rename\n".into(),
        });
    }
    Ok(())
}

fn validate_fetch_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("fetch") {
        return Ok(());
    }
    let mut args = command_args.iter().skip(1).peekable();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--server-option" && args.peek().is_none() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `server-option' requires a value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_maintenance_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("maintenance")
        || command_args.get(1).map(String::as_str) != Some("run")
    {
        return Ok(());
    }
    for arg in command_args.iter().skip(2) {
        if arg == "--" {
            break;
        }
        if arg == "-q" {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `q'\nusage: git maintenance run [--auto] [--[no-]quiet] [--task=<task>] [--schedule]\n\n    --[no-]auto           run tasks based on the state of the repository\n    --[no-]detach         perform maintenance in the background\n    --[no-]schedule <frequency>\n                          run tasks based on frequency\n    --[no-]quiet          do not report progress or other information over stderr\n    --task <task>         run a specific task\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_hash_object_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("hash-object") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if let Some(option) = arg
            .strip_prefix("--type")
            .filter(|value| value.is_empty() || value.starts_with('='))
        {
            let name = if option.is_empty() {
                "type".to_owned()
            } else {
                format!("type{option}")
            };
            return Err(CliError::Stderr {
                code: 129,
                text: format!(
                    "error: unknown option `{name}'\nusage: git hash-object [-t <type>] [-w] [--path=<file> | --no-filters]\n                       [--stdin [--literally]] [--] <file>...\n   or: git hash-object [-t <type>] [-w] --stdin-paths [--no-filters]\n\n    -t <type>             object type\n    -w                    write the object into the object database\n    --[no-]stdin          read the object from stdin\n    --[no-]stdin-paths    read file names from stdin\n    --no-filters          store file as is without filters\n    --filters             opposite of --no-filters\n    --[no-]literally      just hash any random garbage to create corrupt objects for debugging Git\n    --[no-]path <file>    process file as it were from this path\n\n"
                ),
            });
        }
        if arg == "--write" {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown option `write'\nusage: git hash-object [-t <type>] [-w] [--path=<file> | --no-filters]\n                       [--stdin [--literally]] [--] <file>...\n   or: git hash-object [-t <type>] [-w] --stdin-paths [--no-filters]\n\n    -t <type>             object type\n    -w                    write the object into the object database\n    --[no-]stdin          read the object from stdin\n    --[no-]stdin-paths    read file names from stdin\n    --no-filters          store file as is without filters\n    --filters             opposite of --no-filters\n    --[no-]literally      just hash any random garbage to create corrupt objects for debugging Git\n    --[no-]path <file>    process file as it were from this path\n\n".into(),
            });
        }
    }
    Ok(())
}

fn apply_leading_global_options(
    args: &[String],
) -> Result<(
    Vec<String>,
    Vec<ConfigEntry>,
    GlobalRepoOptions,
    PathspecOptions,
)> {
    let mut command_args = Vec::new();
    let mut global_configs = Vec::new();
    let mut repo_options = GlobalRepoOptions::default();
    let mut pathspec_options = PathspecOptions::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "-C" {
            let Some(path) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: switch `C' requires a value\n".into(),
                });
            };
            std::env::set_current_dir(path).map_err(|error| CliError::Fatal {
                code: 128,
                message: format!("cannot change to '{path}': {error}"),
            })?;
            index += 2;
        } else if arg == "-c" {
            let Some(config) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "-c expects a configuration string\n".into(),
                });
            };
            global_configs.push(parse_global_config_entry(config)?);
            index += 2;
        } else if let Some(config) = arg.strip_prefix("--config-env=") {
            global_configs.push(parse_global_config_env_entry(config)?);
            index += 1;
        } else if arg == "--config-env" {
            return Err(CliError::Stderr {
                code: 129,
                text: "no config key given for --config-env\n".into(),
            });
        } else if arg == "--exec-path" {
            println!("{}", git_exec_path_output());
            return Err(CliError::Exit(0));
        } else if let Some(path) = arg.strip_prefix("--exec-path=") {
            // SAFETY: CLI startup is single-threaded before any worker threads are spawned.
            unsafe {
                std::env::set_var("GIT_EXEC_PATH", path);
            }
            index += 1;
        } else if matches!(
            arg.as_str(),
            "-P" | "--no-pager"
                | "-p"
                | "--paginate"
                | "--no-replace-objects"
                | "--no-lazy-fetch"
                | "--no-optional-locks"
                | "--no-advice"
        ) {
            index += 1;
        } else if arg == "--literal-pathspecs" {
            if pathspec_options.icase || pathspec_options.glob_explicit {
                return Err(literal_pathspec_incompatible_error());
            }
            pathspec_options.literal = true;
            pathspec_options.glob = false;
            index += 1;
        } else if arg == "--noglob-pathspecs" {
            pathspec_options.glob = false;
            index += 1;
        } else if arg == "--glob-pathspecs" {
            if pathspec_options.literal {
                return Err(literal_pathspec_incompatible_error());
            }
            pathspec_options.glob = true;
            pathspec_options.glob_explicit = true;
            index += 1;
        } else if arg == "--icase-pathspecs" {
            if pathspec_options.literal {
                return Err(literal_pathspec_incompatible_error());
            }
            pathspec_options.icase = true;
            index += 1;
        } else if arg == "--bare" {
            repo_options.bare = true;
            if repo_options.git_dir.is_none() {
                let git_dir = canonical_or_absolute(std::env::current_dir()?);
                repo_options.git_dir_display = Some(git_dir.display().to_string());
                repo_options.git_dir = Some(git_dir);
            }
            index += 1;
        } else if let Some(path) = arg.strip_prefix("--git-dir=") {
            repo_options.git_dir_display = Some(path.to_owned());
            repo_options.git_dir = Some(canonical_or_absolute(absolute_path_from_arg(
                std::path::Path::new(path),
            )?));
            index += 1;
        } else if arg == "--git-dir" {
            let Some(path) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `git-dir' requires a value\n".into(),
                });
            };
            repo_options.git_dir_display = Some(path.clone());
            repo_options.git_dir = Some(canonical_or_absolute(absolute_path_from_arg(
                std::path::Path::new(path),
            )?));
            index += 2;
        } else if let Some(path) = arg.strip_prefix("--work-tree=") {
            repo_options.work_tree = Some(canonical_or_absolute(absolute_path_from_arg(
                std::path::Path::new(path),
            )?));
            index += 1;
        } else if arg == "--work-tree" {
            let Some(path) = args.get(index + 1) else {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `work-tree' requires a value\n".into(),
                });
            };
            repo_options.work_tree = Some(canonical_or_absolute(absolute_path_from_arg(
                std::path::Path::new(path),
            )?));
            index += 2;
        } else {
            command_args.extend_from_slice(&args[index..]);
            break;
        }
    }
    Ok((command_args, global_configs, repo_options, pathspec_options))
}

fn git_exec_path_output() -> String {
    if let Some(path) = std::env::var_os("GIT_EXEC_PATH") {
        return git_var_path_output(std::path::Path::new(&path));
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(git_var_path_output))
        .unwrap_or_default()
}

fn literal_pathspec_incompatible_error() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "global 'literal' pathspec setting is incompatible with all other global pathspec settings".into(),
    }
}
