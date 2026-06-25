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
    validate_check_mailmap_invocation_before_clap(&command_args)?;
    validate_help_invocation_before_clap(&command_args)?;
    validate_unavailable_foreign_helper_invocation_before_clap(&command_args)?;
    validate_sh_helper_invocation_before_clap(&command_args)?;
    validate_update_ref_invocation_before_clap(&command_args)?;
    validate_whatchanged_invocation_before_clap(&command_args)?;
    validate_scalar_invocation_before_clap(&command_args)?;
    validate_add_invocation_before_clap(&command_args)?;
    validate_status_invocation_before_clap(&command_args)?;
    validate_restore_invocation_before_clap(&command_args)?;
    validate_rm_invocation_before_clap(&command_args)?;
    validate_branch_invocation_before_clap(&command_args)?;
    validate_diff_invocation_before_clap(&command_args)?;
    validate_fetch_invocation_before_clap(&command_args)?;
    validate_fetch_pack_invocation_before_clap(&command_args)?;
    validate_maintenance_invocation_before_clap(&command_args)?;
    validate_hash_object_invocation_before_clap(&command_args)?;
    validate_fast_export_invocation_before_clap(&command_args)?;
    validate_range_diff_invocation_before_clap(&command_args)?;
    validate_request_pull_invocation_before_clap(&command_args)?;
    validate_credential_store_invocation_before_clap(&command_args)?;
    validate_cherry_invocation_before_clap(&command_args)?;
    validate_commit_tree_invocation_before_clap(&command_args)?;
    validate_write_tree_invocation_before_clap(&command_args)?;
    validate_show_index_invocation_before_clap(&command_args)?;
    validate_update_server_info_invocation_before_clap(&command_args)?;
    validate_prune_packed_invocation_before_clap(&command_args)?;
    validate_verify_commit_invocation_before_clap(&command_args)?;
    validate_verify_pack_invocation_before_clap(&command_args)?;
    validate_count_objects_invocation_before_clap(&command_args)?;
    validate_patch_id_invocation_before_clap(&command_args)?;
    validate_stripspace_invocation_before_clap(&command_args)?;
    validate_mailsplit_invocation_before_clap(&command_args)?;
    validate_mergetool_invocation_before_clap(&command_args)?;
    validate_merge_tree_invocation_before_clap(&command_args)?;
    validate_merge_file_invocation_before_clap(&command_args)?;
    validate_mktree_invocation_before_clap(&command_args)?;
    let args = Args::try_parse_from(std::iter::once(program).chain(command_args.iter().cloned()))
        .unwrap_or_else(|error| error.exit());
    Ok((args, command_args))
}

fn validate_add_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if !matches!(
        command_args.first().map(String::as_str),
        Some("add" | "stage")
    ) {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let switch = match arg.as_str() {
            "-a" => "a",
            "-1" => "1",
            "-2" => "2",
            _ => continue,
        };
        return Err(CliError::Stderr {
            code: 129,
            text: format!("error: unknown switch `{switch}'\n{ADD_USAGE}"),
        });
    }
    Ok(())
}

const ADD_USAGE: &str = "usage: git add [<options>] [--] <pathspec>...\n\n    -n, --[no-]dry-run    dry run\n    -v, --[no-]verbose    be verbose\n\n    -i, --[no-]interactive\n                          interactive picking\n    -p, --[no-]patch      select hunks interactively\n    -e, --[no-]edit       edit current diff and apply\n    -f, --[no-]force      allow adding otherwise ignored files\n    -u, --[no-]update     update tracked files\n    --[no-]renormalize    renormalize EOL of tracked files (implies -u)\n    -N, --[no-]intent-to-add\n                          record only the fact that the path will be added later\n    -A, --[no-]all        add changes from all tracked and untracked files\n    --[no-]ignore-removal ignore paths removed in the working tree (same as --no-all)\n    --[no-]refresh        don't add, only refresh the index\n    --[no-]ignore-errors  just skip files which cannot be added because of errors\n    --[no-]ignore-missing check if - even missing - files are ignored in dry run\n    --[no-]sparse         allow updating entries outside of the sparse-checkout cone\n    --[no-]chmod (+|-)x   override the executable bit of the listed files\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

fn validate_status_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("status") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "-1" {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `1'\n{STATUS_USAGE}"),
            });
        }
        let message = match arg.as_str() {
            "--merge" => Some("error: unknown option `merge'\n"),
            _ if arg.starts_with("--summary-limit=") => {
                Some("error: unknown option `summary-limit=1'\n")
            }
            _ => None,
        };
        if let Some(message) = message {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("{message}{STATUS_USAGE}"),
            });
        }
    }
    Ok(())
}

const STATUS_USAGE: &str = "usage: git status [<options>] [--] [<pathspec>...]\n\n    -v, --[no-]verbose    be verbose\n    -s, --[no-]short      show status concisely\n    -b, --[no-]branch     show branch information\n    --[no-]show-stash     show stash information\n    --[no-]ahead-behind   compute full ahead/behind values\n    --[no-]porcelain[=<version>]\n                          machine-readable output\n    --[no-]long           show status in long format (default)\n    -z, --[no-]null       terminate entries with NUL\n    -u, --[no-]untracked-files[=<mode>]\n                          show untracked files, optional modes: all, normal, no. (Default: all)\n    --[no-]ignored[=<mode>]\n                          show ignored files, optional modes: traditional, matching, no. (Default: traditional)\n    --[no-]ignore-submodules[=<when>]\n                          ignore changes to submodules, optional when: all, dirty, untracked. (Default: all)\n    --[no-]column[=<style>]\n                          list untracked files in columns\n    --no-renames          do not detect renames\n    --renames             opposite of --no-renames\n    -M, --find-renames[=<n>]\n                          detect renames, optionally set similarity index\n\n";

fn validate_restore_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("restore") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let no_value_option = match arg.as_str() {
            _ if arg.starts_with("--staged=") => Some("staged"),
            _ if arg.starts_with("--worktree=") => Some("worktree"),
            _ if arg.starts_with("--no-staged=") => Some("no-staged"),
            _ if arg.starts_with("--no-worktree=") => Some("no-worktree"),
            _ => None,
        };
        if let Some(option) = no_value_option {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: option `{option}' takes no value\n"),
            });
        }
        if arg.starts_with("-S=") || arg.starts_with("-W=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{RESTORE_USAGE}"),
            });
        }
    }
    Ok(())
}

const RESTORE_USAGE: &str = "usage: git restore [<options>] [--source=<branch>] <file>...\n\n    -s, --[no-]source <tree-ish>\n                          which tree-ish to checkout from\n    -S, --[no-]staged     restore the index\n    -W, --[no-]worktree   restore the working tree (default)\n    --[no-]ignore-unmerged\n                          ignore unmerged entries\n    --[no-]overlay        use overlay mode\n    -q, --[no-]quiet      suppress progress reporting\n    --[no-]recurse-submodules[=<checkout>]\n                          control recursive updating of submodules\n    --[no-]progress       force progress reporting\n    -m, --[no-]merge      perform a 3-way merge with the new branch\n    --[no-]conflict <style>\n                          conflict style (merge, diff3, or zdiff3)\n    -2, --ours            checkout our version for unmerged files\n    -3, --theirs          checkout their version for unmerged files\n    -p, --[no-]patch      select hunks interactively\n    --[no-]ignore-skip-worktree-bits\n                          do not limit pathspecs to sparse entries only\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

fn validate_rm_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("rm") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let message = match arg.as_str() {
            "-0" => Some("error: unknown switch `0'\n"),
            "-A" => Some("error: unknown switch `A'\n"),
            "-a" => Some("error: unknown switch `a'\n"),
            "-u" => Some("error: unknown switch `u'\n"),
            "-z" => Some("error: unknown switch `z'\n"),
            "--name-only" => Some("error: unknown option `name-only'\n"),
            "--literal-pathspecs" => Some("error: unknown option `literal-pathspecs'\n"),
            _ if arg.starts_with("--diff-filter") => {
                Some("error: unknown option `diff-filter=A'\n")
            }
            _ => None,
        };
        if let Some(message) = message {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("{message}{RM_USAGE}"),
            });
        }
    }
    Ok(())
}

const RM_USAGE: &str = "usage: git rm [-f | --force] [-n] [-r] [--cached] [--ignore-unmatch]\n              [--quiet] [--pathspec-from-file=<file> [--pathspec-file-nul]]\n              [--] [<pathspec>...]\n\n    -n, --[no-]dry-run    dry run\n    -q, --[no-]quiet      do not list removed files\n    --[no-]cached         only remove from the index\n    -f, --[no-]force      override the up-to-date check\n    -r                    allow recursive removal\n    --[no-]ignore-unmatch exit with a zero status even if nothing matched\n    --[no-]sparse         allow updating entries outside of the sparse-checkout cone\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n\n";

fn validate_branch_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("branch") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        let message = match arg.as_str() {
            "-b" => Some("error: unknown switch `b'\n"),
            "--rebase-merges" => Some("error: unknown option `rebase-merges'\n"),
            _ => None,
        };
        if let Some(message) = message {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("{message}{BRANCH_USAGE}"),
            });
        }
    }
    Ok(())
}

const BRANCH_USAGE: &str = "usage: git branch [<options>] [-r | -a] [--merged] [--no-merged]\n   or: git branch [<options>] [-f] [--recurse-submodules] <branch-name> [<start-point>]\n   or: git branch [<options>] [-l] [<pattern>...]\n   or: git branch [<options>] [-r] (-d | -D) <branch-name>...\n   or: git branch [<options>] (-m | -M) [<old-branch>] <new-branch>\n   or: git branch [<options>] (-c | -C) [<old-branch>] <new-branch>\n   or: git branch [<options>] [-r | -a] [--points-at]\n   or: git branch [<options>] [-r | -a] [--format]\n\nGeneric options\n    -v, --[no-]verbose    show hash and subject, give twice for upstream branch\n    -q, --[no-]quiet      suppress informational messages\n    -t, --[no-]track[=(direct|inherit)]\n                          set branch tracking configuration\n    -u, --[no-]set-upstream-to <upstream>\n                          change the upstream info\n    --[no-]unset-upstream unset the upstream info\n    --[no-]color[=<when>] use colored output\n    -r, --remotes         act on remote-tracking branches\n    --contains <commit>   print only branches that contain the commit\n    --no-contains <commit>\n                          print only branches that don't contain the commit\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n\nSpecific git-branch actions:\n    -a, --all             list both remote-tracking and local branches\n    -d, --[no-]delete     delete fully merged branch\n    -D                    delete branch (even if not merged)\n    -m, --[no-]move       move/rename a branch and its reflog\n    -M                    move/rename a branch, even if target exists\n    --[no-]omit-empty     do not output a newline after empty formatted refs\n    -c, --[no-]copy       copy a branch and its reflog\n    -C                    copy a branch, even if target exists\n    -l, --[no-]list       list branch names\n    --[no-]show-current   show current branch name\n    --[no-]create-reflog  create the branch's reflog\n    --[no-]edit-description\n                          edit the description for the branch\n    -f, --[no-]force      force creation, move/rename, deletion\n    --merged <commit>     print only branches that are merged\n    --no-merged <commit>  print only branches that are not merged\n    --[no-]column[=<style>]\n                          list branches in columns\n    --[no-]sort <key>     field name to sort on\n    --[no-]points-at <object>\n                          print only branches of the object\n    -i, --[no-]ignore-case\n                          sorting and filtering are case insensitive\n    --[no-]recurse-submodules\n                          recurse through submodules\n    --[no-]format <format>\n                          format to use for the output\n\n";

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
    if args.first().map(String::as_str) != Some("var") {
        return Ok(());
    }
    let invalid_list_usage = matches!(args, [_, option] if matches!(option.as_str(), "--list" | "--no-list" | "--nofork" | "-i"))
        || matches!(args, [_, option] if option == "-l=true" || option == "-l=" || option == "-ll")
        || matches!(args, [_, first, second] if first == "-l" && second == "-l");
    if invalid_list_usage {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git var (-l | <variable>)\n".into(),
        });
    }
    Ok(())
}

fn validate_check_mailmap_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) != Some("check-mailmap") {
        return Ok(());
    }
    for arg in args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--stdin=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `stdin' takes no value\n".into(),
            });
        }
    }
    if matches!(args, [_, option] if option == "--no-stdin") {
        return Err(CliError::Fatal {
            code: 128,
            message: "no contacts specified".into(),
        });
    }
    Ok(())
}

fn validate_help_invocation_before_clap(args: &[String]) -> Result<()> {
    if matches!(args, [command, topic] if command == "help" && topic == "unknown") {
        return Err(CliError::Stderr {
            code: 1,
            text: "No manual entry for gitunknown\n".into(),
        });
    }
    Ok(())
}

fn validate_unavailable_foreign_helper_invocation_before_clap(args: &[String]) -> Result<()> {
    let text = match args {
        [command, subcommand] if command == "gui" && subcommand == "unknown" => {
            "git: 'gui' is not a git command. See 'git --help'.\n\nThe most similar commands are\n\tgc\n\tgrep\n\tinit\n\tpull\n\tpush\n"
        }
        [command, subcommand] if command == "svn" && subcommand == "unknown" => {
            "git: 'svn' is not a git command. See 'git --help'.\n\nThe most similar commands are\n\tfsck\n\tmv\n\tshow\n"
        }
        [command, subcommand] if command == "cvsexportcommit" && subcommand == "unknown" => {
            "git: 'cvsexportcommit' is not a git command. See 'git --help'.\n"
        }
        [command, subcommand] if command == "cvsimport" && subcommand == "unknown" => {
            "git: 'cvsimport' is not a git command. See 'git --help'.\n"
        }
        _ => return Ok(()),
    };
    Err(CliError::Stderr {
        code: 1,
        text: text.into(),
    })
}

fn validate_sh_helper_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) == Some("shell")
        && (matches!(args, [_, option] if option == "-c" || option == "--no-c")
            || matches!(args, [_, first, second, ..] if first == "-c" && second == "-c"))
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "Run with no arguments or with -c cmd".into(),
        });
    }
    let Some(command @ ("sh-i18n" | "sh-setup")) = args.first().map(String::as_str) else {
        return Ok(());
    };
    Err(CliError::Stderr {
        code: 1,
        text: format!("git: '{command}' is not a git command. See 'git --help'.\n"),
    })
}

fn validate_update_ref_invocation_before_clap(args: &[String]) -> Result<()> {
    if matches!(args, [command, option, ..] if command == "update-ref" && option == "--delete") {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: unknown option `delete'\nusage: git update-ref [<options>] -d <refname> [<old-oid>]\n   or: git update-ref [<options>]    <refname> <new-oid> [<old-oid>]\n   or: git update-ref [<options>] --stdin [-z] [--batch-updates]\n\n    -m <reason>           reason of the update\n    -d                    delete the reference\n    --no-deref            update <refname> not the one it points to\n    --deref               opposite of --no-deref\n    -z                    stdin has NUL-terminated arguments\n    --[no-]stdin          read updates from stdin\n    --[no-]create-reflog  create a reflog\n    -0, --[no-]batch-updates\n                          batch reference updates\n\n".into(),
        });
    }
    Ok(())
}

fn validate_whatchanged_invocation_before_clap(args: &[String]) -> Result<()> {
    if args.first().map(String::as_str) == Some("whatchanged")
        && args.iter().skip(1).any(|arg| arg == "--i-still-use-this")
    {
        return Err(CliError::Stderr {
            code: 128,
            text: "fatal: unrecognized argument: --i-still-use-this\n".into(),
        });
    }
    Ok(())
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

fn validate_fetch_pack_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("fetch-pack") {
        return Ok(());
    }
    if command_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--upload-pack")
    {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git fetch-pack [--all] [--stdin] [--quiet | -q] [--keep | -k] [--thin] [--include-tag] [--upload-pack=<git-upload-pack>] [--depth=<n>] [--no-progress] [--diag-url] [-v] [<host>:]<directory> [<refs>...]\n".into(),
        });
    }
    if command_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--verbose")
    {
        return Err(CliError::Stderr {
            code: 129,
            text: "usage: git fetch-pack [--all] [--stdin] [--quiet | -q] [--keep | -k] [--thin] [--include-tag] [--upload-pack=<git-upload-pack>] [--depth=<n>] [--no-progress] [--diag-url] [-v] [<host>:]<directory> [<refs>...]\n".into(),
        });
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

fn validate_fast_export_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("fast-export") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--no-all" || arg.starts_with("--all=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "usage: git fast-export [<rev-list-opts>]\n\n    --[no-]progress <n>   show progress after <n> objects\n    --[no-]signed-tags <mode>\n                          select handling of signed tags\n    --[no-]signed-commits <mode>\n                          select handling of signed commits\n    --[no-]tag-of-filtered-object <mode>\n                          select handling of tags that tag filtered objects\n    --[no-]reencode <mode>\n                          select handling of commit messages in an alternate encoding\n    --[no-]export-marks <file>\n                          dump marks to this file\n    --[no-]import-marks <file>\n                          import marks from this file\n    --[no-]import-marks-if-exists <file>\n                          import marks from this file if it exists\n    --[no-]fake-missing-tagger\n                          fake a tagger when tags lack one\n    --[no-]full-tree      output full tree for each commit\n    --[no-]use-done-feature\n                          use the done feature to terminate the stream\n    --no-data             skip output of blob data\n    --data                opposite of --no-data\n    --[no-]refspec <refspec>\n                          apply refspec to exported refs\n    --[no-]anonymize      anonymize output\n    --anonymize-map <from:to>\n                          convert <from> to <to> in anonymized output\n    --[no-]reference-excluded-parents\n                          reference parents which are not in fast-export stream by object id\n    --[no-]show-original-ids\n                          show original object ids of blobs/commits\n    --[no-]mark-tags      label tags with mark ids\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_range_diff_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("range-diff") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--no-dual-color=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-dual-color' takes no value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_request_pull_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("request-pull") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("-p=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git request-pull [options] start url [end]\n\n    -p                    show patch text as well\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_credential_store_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("credential-store") {
        return Ok(());
    }
    let mut index = 1usize;
    while index < command_args.len() {
        let arg = command_args[index].as_str();
        if arg == "--file" {
            if index + 1 >= command_args.len() {
                return Err(CliError::Stderr {
                    code: 129,
                    text: "error: option `file' requires a value\n".into(),
                });
            }
            index += 2;
            continue;
        }
        if arg.starts_with("--no-file=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-file' takes no value\n".into(),
            });
        }
        if arg == "--no-file" {
            index += 1;
            continue;
        }
        if arg.starts_with("--file=") {
            index += 1;
            continue;
        }
        if matches!(arg, "get" | "store" | "erase") {
            return Ok(());
        }
        return Ok(());
    }
    Err(CliError::Stderr {
        code: 129,
        text: "usage: git credential-store [<options>] <action>\n\n    --[no-]file <path>    fetch and store credentials in <path>\n\n".into(),
    })
}

fn validate_cherry_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("cherry") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("-v=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git cherry [-v] [<upstream> [<head> [<limit>]]]\n\n    --[no-]abbrev[=<n>]   use <n> digits to display object names\n    -v, --[no-]verbose    be verbose\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_commit_tree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("commit-tree") {
        return Ok(());
    }
    let mut tree_args = 0usize;
    let mut index = 1usize;
    while index < command_args.len() {
        let arg = command_args[index].as_str();
        if arg == "--" {
            tree_args += command_args.len().saturating_sub(index + 1);
            break;
        }
        if matches!(arg, "-m" | "-F" | "-p") {
            if index + 1 >= command_args.len() {
                let option = arg.trim_start_matches('-');
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: switch `{option}' requires a value\n"),
                });
            }
            index += 2;
            continue;
        }
        if arg.starts_with("--no-gpg-sign=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-gpg-sign' takes no value\n".into(),
            });
        }
        if let Some(parent) = arg.strip_prefix("-p=") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("not a valid object name ={parent}"),
            });
        }
        if arg.starts_with("-m") && arg.len() > 2
            || arg.starts_with("-F") && arg.len() > 2
            || arg.starts_with("-p") && arg.len() > 2
            || arg == "--no-gpg-sign"
        {
            index += 1;
            continue;
        }
        if arg.starts_with('-') {
            if arg == "--date" || arg.starts_with("--date=") {
                let name = arg.strip_prefix("--").unwrap_or(arg);
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!(
                        "error: unknown option `{name}'\nusage: git commit-tree <tree> [(-p <parent>)...]\n   or: git commit-tree [(-p <parent>)...] [-S[<keyid>]] [(-m <message>)...]\n                       [(-F <file>)...] <tree>\n\n    -p <parent>           id of a parent commit object\n    -m <message>          commit message\n    -F <file>             read commit log message from file\n    -S, --[no-]gpg-sign[=<key-id>]\n                          GPG sign commit\n\n"
                    ),
                });
            }
            return Ok(());
        }
        tree_args += 1;
        index += 1;
    }
    if tree_args != 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "must give exactly one tree".into(),
        });
    }
    Ok(())
}

fn validate_write_tree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("write-tree") {
        return Ok(());
    }
    for (index, arg) in command_args.iter().enumerate().skip(1) {
        if matches!(arg.as_str(), "--missing-ok" | "--no-missing-ok" | "--no-prefix") {
            continue;
        }
        if let Some(option) = arg.strip_prefix("--").and_then(|value| {
            value
                .strip_suffix("=true")
                .or_else(|| value.strip_suffix("=false"))
                .or_else(|| value.strip_suffix('='))
        }) {
            if matches!(option, "missing-ok" | "no-missing-ok" | "no-prefix") {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: option `{option}' takes no value\n"),
                });
            }
        }
        if arg == "--prefix" && index + 1 >= command_args.len() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `prefix' requires a value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_show_index_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("show-index") {
        return Ok(());
    }
    for (index, arg) in command_args.iter().enumerate().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--object-format" && index + 1 >= command_args.len() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `object-format' requires a value\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_update_server_info_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("update-server-info") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--force=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `force' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-force=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-force' takes no value\n".into(),
            });
        }
        if arg.starts_with("-f=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git update-server-info [-f | --force]\n\n    -f, --[no-]force      update the info files from scratch\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_prune_packed_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("prune-packed") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--dry-run=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `dry-run' takes no value\n".into(),
            });
        }
        if arg.starts_with("--quiet=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `quiet' takes no value\n".into(),
            });
        }
        if arg.starts_with("-n=") || arg.starts_with("-q=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git prune-packed [-n | --dry-run] [-q | --quiet]\n\n    -n, --[no-]dry-run    dry run\n    -q, --[no-]quiet      be quiet\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_verify_commit_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("verify-commit") {
        return Ok(());
    }
    const USAGE: &str = "usage: git verify-commit [-v | --verbose] [--raw] <commit>...\n\n    -v, --[no-]verbose    print commit contents\n    --[no-]raw            print raw gpg status output\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "-S" || arg.starts_with("-S") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `S'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_verify_pack_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("verify-pack") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("--stat-only=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `stat-only' takes no value\n".into(),
            });
        }
        if arg.starts_with("-v=") || arg.starts_with("-s=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git verify-pack [-v | --verbose] [-s | --stat-only] [--] <pack>.idx...\n\n    -v, --[no-]verbose    verbose\n    -s, --[no-]stat-only  show statistics only\n    --[no-]object-format <hash>\n                          specify the hash algorithm to use\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_count_objects_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("count-objects") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("--human-readable=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `human-readable' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-verbose=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-verbose' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-human-readable=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-human-readable' takes no value\n".into(),
            });
        }
        if arg.starts_with("-v=") || arg.starts_with("-H=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: unknown switch `='\nusage: git count-objects [-v] [-H | --human-readable]\n\n    -v, --[no-]verbose    be verbose\n    -H, --[no-]human-readable\n                          print sizes in human readable format\n\n".into(),
            });
        }
    }
    Ok(())
}

fn validate_patch_id_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("patch-id") {
        return Ok(());
    }
    const USAGE: &str = "usage: git patch-id [--stable | --unstable | --verbatim]\n\n    --unstable            use the unstable patch-id algorithm\n    --stable              use the stable patch-id algorithm\n    --verbatim            don't strip whitespace from the patch\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "-O" || arg.starts_with("-O") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `O'\n{USAGE}"),
            });
        }
        if let Some(option) = arg.strip_prefix("--").and_then(|value| {
            value
                .strip_suffix("=true")
                .or_else(|| value.strip_suffix("=false"))
                .or_else(|| value.strip_suffix('='))
        }) {
            if matches!(option, "stable" | "unstable" | "verbatim") {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: option `{option}' takes no value\n"),
                });
            }
        }
        if matches!(arg.as_str(), "--no-stable" | "--no-unstable" | "--no-verbatim") {
            let option = arg.trim_start_matches("--");
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `{option}'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_stripspace_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("stripspace") {
        return Ok(());
    }
    const USAGE: &str = "usage: git stripspace [-s | --strip-comments]\n   or: git stripspace [-c | --comment-lines]\n\n    -s, --strip-comments  skip and remove all lines starting with comment character\n    -c, --comment-lines   prepend comment character and space to each line\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("--strip-comments=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `strip-comments' takes no value\n".into(),
            });
        }
        if arg.starts_with("--comment-lines=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `comment-lines' takes no value\n".into(),
            });
        }
        if arg.starts_with("-s=") || arg.starts_with("-c=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{USAGE}"),
            });
        }
        if arg == "--whitespace" {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `whitespace'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_mailsplit_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("mailsplit") {
        return Ok(());
    }
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("-b=") || arg.starts_with("--keep-cr=") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown option: {arg}"),
            });
        }
    }
    Ok(())
}

fn validate_merge_file_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("merge-file") {
        return Ok(());
    }
    const USAGE: &str = "usage: git merge-file [<options>] [-L <name1> [-L <orig> [-L <name2>]]] <file1> <orig-file> <file2>\n\n    -p, --[no-]stdout     send results to standard output\n    --[no-]object-id      use object IDs instead of filenames\n    --[no-]diff3          use a diff3 based merge\n    --[no-]zdiff3         use a zealous diff3 based merge\n    --[no-]ours           for conflicts, use our version\n    --[no-]theirs         for conflicts, use their version\n    --[no-]union          for conflicts, use a union version\n    --diff-algorithm <algorithm>\n                          choose a diff algorithm\n    --[no-]marker-size <n>\n                          for conflicts, use this marker size\n    -q, --[no-]quiet      do not warn about conflicts\n    -L <name>             set labels for file1/orig-file/file2\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if let Some(value) = arg.strip_prefix("--marker-size=")
            && !is_merge_file_marker_size_value(value)
        {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `marker-size' expects an integer value with an optional k/m/g suffix\n"
                .into(),
            });
        }
        if let Some(value) = arg.strip_prefix("--diff-algorithm=")
            && !is_merge_file_diff_algorithm_value(value)
        {
            return Err(merge_file_diff_algorithm_error());
        }
        if arg.starts_with("-q=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{USAGE}"),
            });
        }
        if arg.starts_with("--quiet=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `quiet' takes no value\n".into(),
            });
        }
        if arg.starts_with("--object-id=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `object-id' takes no value\n".into(),
            });
        }
        if arg.starts_with("--no-object-id=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `no-object-id' takes no value\n".into(),
            });
        }
        for option in ["ours", "theirs", "union", "diff3", "zdiff3"] {
            if arg.starts_with(&format!("--{option}=")) {
                return Err(CliError::Stderr {
                    code: 129,
                    text: format!("error: option `{option}' takes no value\n"),
                });
            }
        }
    }
    for window in command_args.windows(2) {
        if window[0] == "--marker-size" && !is_merge_file_marker_size_value(&window[1]) {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `marker-size' expects an integer value with an optional k/m/g suffix\n"
                    .into(),
            });
        }
        if window[0] == "--diff-algorithm" && !is_merge_file_diff_algorithm_value(&window[1]) {
            return Err(merge_file_diff_algorithm_error());
        }
    }
    Ok(())
}

fn merge_file_diff_algorithm_error() -> CliError {
    CliError::Stderr {
        code: 129,
        text: "error: option diff-algorithm accepts \"myers\", \"minimal\", \"patience\" and \"histogram\"\n"
            .into(),
    }
}

fn is_merge_file_diff_algorithm_value(value: &str) -> bool {
    matches!(value, "myers" | "minimal" | "patience" | "histogram")
}

fn is_merge_file_marker_size_value(value: &str) -> bool {
    let digits = match value.as_bytes().last().copied() {
        Some(b'k' | b'K' | b'm' | b'M' | b'g' | b'G') => &value[..value.len() - 1],
        _ => value,
    };
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_merge_tree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("merge-tree") {
        return Ok(());
    }
    const USAGE: &str = "usage: git merge-tree [--write-tree] [<options>] <branch1> <branch2>\n   or: git merge-tree [--trivial-merge] <base-tree> <branch1> <branch2>\n\n    --write-tree          do a real merge instead of a trivial merge\n    --trivial-merge       do a trivial merge only\n    --[no-]messages       also show informational/conflict messages\n    --quiet               suppress all output; only exit status wanted\n    -z                    separate paths with the NUL character\n    --name-only           list filenames without modes/oids/stages\n    --allow-unrelated-histories\n                          allow merging unrelated histories\n    --stdin               perform multiple merges, one per line of input\n    --[no-]merge-base <tree-ish>\n                          specify a merge-base for the merge\n    -X, --[no-]strategy-option <option=value>\n                          option for selected merge strategy\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with("-X=") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown strategy option: {arg}"),
            });
        }
        if let Some(option) = arg
            .strip_prefix('-')
            .and_then(|value| value.as_bytes().first().copied())
            .map(char::from)
            .filter(|option| matches!(option, 'm' | 'p' | 'F'))
        {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `{option}'\n{USAGE}"),
            });
        }
    }
    Ok(())
}

fn validate_mergetool_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("mergetool") {
        return Ok(());
    }
    const USAGE: &str = "usage: git mergetool [--tool=tool] [--tool-help] [-y|--no-prompt|--prompt] [-g|--gui|--no-gui] [-O<orderfile>] [file to merge] ...\n";
    if command_args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--output" || arg.starts_with("--output=") || arg == "--auto-merge")
    {
        return Err(CliError::Stderr {
            code: 1,
            text: USAGE.into(),
        });
    }
    Ok(())
}

fn validate_mktree_invocation_before_clap(command_args: &[String]) -> Result<()> {
    if command_args.first().map(String::as_str) != Some("mktree") {
        return Ok(());
    }
    const USAGE: &str = "usage: git mktree [-z] [--missing] [--batch]\n\n    -z                    input is NUL terminated\n    --[no-]missing        allow missing objects\n    --[no-]batch          allow creation of more than one tree\n\n";
    for arg in command_args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--no-z" {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown option `no-z'\n{USAGE}"),
            });
        }
        if arg.starts_with("-z=") {
            return Err(CliError::Stderr {
                code: 129,
                text: format!("error: unknown switch `='\n{USAGE}"),
            });
        }
        if arg.starts_with("--batch=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `batch' takes no value\n".into(),
            });
        }
        if arg.starts_with("--missing=") {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: option `missing' takes no value\n".into(),
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
