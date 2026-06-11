use std::sync::atomic::{AtomicBool, Ordering};

use clap::{CommandFactory, Parser};

use super::*;

static BROKEN_PIPE_PANIC: AtomicBool = AtomicBool::new(false);

pub(crate) fn command_definition() -> clap::Command {
    Args::command()
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

pub(crate) fn parse_cli_invocation(
    program: String,
    raw_args: &[String],
) -> Result<(Args, Vec<String>)> {
    let (command_args, global_configs, global_repo_options, pathspec_options) =
        apply_leading_global_options(raw_args)?;
    set_global_config_entries(global_configs);
    set_global_repo_options(global_repo_options);
    set_global_pathspec_options(pathspec_options);
    validate_scalar_invocation_before_clap(&command_args)?;
    let args = Args::try_parse_from(std::iter::once(program).chain(command_args.iter().cloned()))
        .unwrap_or_else(|error| error.exit());
    Ok((args, command_args))
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

fn literal_pathspec_incompatible_error() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "global 'literal' pathspec setting is incompatible with all other global pathspec settings".into(),
    }
}
