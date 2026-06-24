use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Commit {
            all,
            only,
            allow_empty,
            amend,
            edit,
            no_edit,
            signoff,
            quiet,
            verbose,
            no_verify,
            status,
            no_status,
            cleanup,
            no_cleanup,
            allow_empty_message,
            author,
            date,
            squash,
            template,
            reset_author,
            reuse_message,
            reedit_message,
            fixup,
            message_file,
            messages,
            trailers,
            paths,
        } => super::commit_commands::commit_command(super::commit_commands::CommitCommandOptions {
            all,
            only,
            allow_empty,
            amend,
            edit,
            no_edit,
            signoff,
            quiet,
            verbose,
            no_verify,
            status,
            no_status,
            cleanup: cleanup.as_deref(),
            no_cleanup,
            allow_empty_message,
            author_override: author.as_deref(),
            date_override: date.as_deref(),
            squash: squash.as_deref(),
            template: template.as_deref(),
            reset_author,
            reuse_message: reuse_message.as_deref(),
            reedit_message: reedit_message.as_deref(),
            fixup: fixup.as_deref(),
            message_file: message_file.as_deref(),
            messages,
            trailers,
            paths,
        }),
        runtime::Command::Citool {
            amend,
            nocommit,
            message_file,
            messages,
        } => super::commit_commands::citool_command(
            amend,
            nocommit,
            message_file.as_deref(),
            messages,
        ),
        runtime::Command::Gui { args } => super::commit_commands::gui_command(args),
        runtime::Command::WriteTree {
            prefix,
            no_prefix,
            missing_ok,
            no_missing_ok,
        } => {
            let _no_missing_ok = no_missing_ok;
            let prefix = effective_write_tree_prefix(prefix, no_prefix > 0);
            super::commit_commands::write_tree_command_entry(prefix.as_deref(), missing_ok > 0)
        }
        runtime::Command::CommitTree {
            tree,
            parents,
            messages,
            message_files,
            no_gpg_sign,
        } => super::commit_commands::commit_tree_command(
            &tree,
            parents,
            ordered_commit_tree_message_sources(messages, message_files),
            no_gpg_sign,
        ),
        runtime::Command::Mktree {
            nul_terminated,
            missing,
            no_missing,
            batch,
            no_batch,
        } => super::commit_commands::mktree_command(
            nul_terminated > 0,
            missing > 0 && no_missing == 0,
            batch > 0 && no_batch == 0,
        ),
        _ => unreachable!("non-commit command dispatched to commit"),
    }
}

fn effective_write_tree_prefix(
    prefixes: Vec<String>,
    no_prefix: bool,
) -> Option<String> {
    let raw_args: Vec<String> = std::env::args().skip_while(|arg| arg != "write-tree").collect();
    let mut prefix_iter = prefixes.into_iter();
    let mut effective = None;
    let mut index = 0usize;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--prefix" => {
                if index + 1 < raw_args.len() {
                    effective = prefix_iter.next().or_else(|| Some(raw_args[index + 1].clone()));
                    index += 2;
                    continue;
                }
            }
            "--no-prefix" => {
                effective = None;
            }
            arg if arg.starts_with("--prefix=") => {
                effective = prefix_iter
                    .next()
                    .or_else(|| Some(arg.trim_start_matches("--prefix=").to_owned()));
            }
            _ => {}
        }
        index += 1;
    }
    if effective.is_none() && !no_prefix {
        prefix_iter.next()
    } else {
        effective
    }
}

fn ordered_commit_tree_message_sources(
    messages: Vec<String>,
    message_files: Vec<std::path::PathBuf>,
) -> Vec<super::commit_commands::CommitTreeMessageSource> {
    let args = std::env::args_os().collect::<Vec<_>>();
    let Some(command_index) = args.iter().position(|arg| arg == "commit-tree") else {
        return grouped_commit_tree_message_sources(messages, message_files);
    };
    let mut sources = Vec::new();
    let mut index = command_index + 2;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        match arg.as_ref() {
            "-m" => {
                if let Some(value) = args.get(index + 1) {
                    sources.push(super::commit_commands::CommitTreeMessageSource::Message(
                        value.to_string_lossy().into_owned(),
                    ));
                    index += 2;
                    continue;
                }
            }
            "-F" => {
                if let Some(value) = args.get(index + 1) {
                    sources.push(super::commit_commands::CommitTreeMessageSource::File(
                        value.into(),
                    ));
                    index += 2;
                    continue;
                }
            }
            _ if arg.starts_with("-m") && arg.len() > 2 => {
                sources.push(super::commit_commands::CommitTreeMessageSource::Message(
                    arg[2..].to_owned(),
                ));
            }
            _ if arg.starts_with("-F") && arg.len() > 2 => {
                sources.push(super::commit_commands::CommitTreeMessageSource::File(
                    std::path::PathBuf::from(&arg[2..]),
                ));
            }
            _ => {}
        }
        index += 1;
    }
    if sources.len() == messages.len() + message_files.len() {
        sources
    } else {
        grouped_commit_tree_message_sources(messages, message_files)
    }
}

fn grouped_commit_tree_message_sources(
    messages: Vec<String>,
    message_files: Vec<std::path::PathBuf>,
) -> Vec<super::commit_commands::CommitTreeMessageSource> {
    messages
        .into_iter()
        .map(super::commit_commands::CommitTreeMessageSource::Message)
        .chain(
            message_files
                .into_iter()
                .map(super::commit_commands::CommitTreeMessageSource::File),
        )
        .collect()
}
