use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Clone {
            quiet,
            verbose: _verbose,
            progress: _progress,
            no_progress: _no_progress,
            bare,
            mirror,
            local: _local,
            no_local: _no_local,
            no_hardlinks,
            hardlinks,
            reject_shallow,
            no_reject_shallow,
            template,
            no_template,
            configs,
            no_checkout,
            checkout,
            recurse_submodules,
            recursive,
            no_recurse_submodules,
            jobs,
            shallow_submodules,
            remote_submodules,
            origin,
            no_tags,
            tags,
            single_branch,
            no_single_branch,
            separate_git_dir,
            references,
            reference_if_able,
            shared,
            dissociate,
            depth,
            branch,
            repository,
            directory,
        } => run_clone(
            runtime::transport_commands::CloneCommandInput {
                quiet,
                reject_shallow,
                no_reject_shallow,
                template,
                no_template,
                configs,
                no_checkout,
                checkout,
                recurse_submodules,
                recursive,
                no_recurse_submodules,
                jobs,
                shallow_submodules,
                remote_submodules,
                origin,
                no_tags,
                tags,
                single_branch,
                no_single_branch,
                separate_git_dir,
                references,
                reference_if_able,
                shared,
                dissociate,
                no_hardlinks,
                hardlinks,
                depth,
                branch,
                repository,
                directory,
                bare,
                mirror,
            },
            raw_args,
        ),
        runtime::Command::LsRemote {
            heads,
            tags,
            refs_only,
            repository,
            patterns,
        } => run_ls_remote(heads, tags, refs_only, repository, patterns),
        runtime::Command::Fetch {
            depth,
            remote,
            branch,
        } => run_fetch(depth, remote, branch),
        runtime::Command::Pull {
            ff_only,
            rebase,
            remote,
            branch,
        } => run_pull(ff_only, rebase, remote, branch),
        runtime::Command::Push {
            force,
            set_upstream,
            remote,
            refspecs,
        } => run_push(force, set_upstream, remote, refspecs),
        runtime::Command::Daemon {
            verbose,
            export_all,
            timeout,
            init_timeout,
            max_connections,
            strict_paths,
            base_path,
            base_path_relaxed,
            reuseaddr,
            pid_file,
            inetd,
            listen,
            port,
            directories,
        } => runtime::transport_commands::daemon(runtime::transport_commands::DaemonOptions {
            verbose,
            export_all,
            timeout,
            init_timeout,
            max_connections,
            strict_paths,
            base_path,
            base_path_relaxed,
            reuseaddr,
            pid_file,
            inetd,
            listen,
            port,
            directories,
        }),
        runtime::Command::UploadPack {
            strict,
            no_strict,
            stateless_rpc,
            advertise_refs,
            timeout,
            directory,
        } => runtime::transport_commands::upload_pack(
            runtime::transport_commands::UploadPackOptions {
                strict,
                no_strict,
                stateless_rpc,
                advertise_refs,
                timeout,
                directory,
            },
        ),
        runtime::Command::HttpFetch {
            commit,
            tags,
            all,
            verbose,
            recover,
            write_ref,
            stdin,
            packfile,
            index_pack_args,
            args,
        } => {
            runtime::transport_commands::http_fetch(runtime::transport_commands::HttpFetchOptions {
                commit,
                tags,
                all,
                verbose,
                recover,
                write_ref,
                stdin,
                packfile,
                index_pack_args,
                args,
            })
        }
        runtime::Command::HttpPush {
            all,
            dry_run,
            force,
            verbose,
            remote,
            heads,
        } => runtime::transport_commands::http_push(runtime::transport_commands::HttpPushOptions {
            all,
            dry_run,
            force,
            verbose,
            remote,
            heads,
        }),
        runtime::Command::FetchPack {
            all,
            stdin,
            quiet,
            keep,
            thin,
            include_tag,
            upload_pack,
            depth,
            no_progress,
            diag_url,
            verbose,
            directory,
            refs,
        } => {
            runtime::transport_commands::fetch_pack(runtime::transport_commands::FetchPackOptions {
                all,
                stdin,
                quiet,
                keep,
                thin,
                include_tag,
                upload_pack,
                depth,
                no_progress,
                diag_url,
                verbose,
                directory,
                refs,
            })
        }
        runtime::Command::SendPack {
            mirror,
            dry_run,
            force,
            receive_pack,
            verbose,
            thin,
            atomic,
            all,
            stdin,
            directory,
            refs,
        } => runtime::transport_commands::send_pack(runtime::transport_commands::SendPackOptions {
            mirror,
            dry_run,
            force,
            receive_pack,
            verbose,
            thin,
            atomic,
            all,
            stdin,
            directory,
            refs,
        }),
        runtime::Command::HttpBackend => runtime::transport_commands::http_backend(),
        runtime::Command::ReceivePack { quiet, directory } => {
            runtime::transport_commands::receive_pack(quiet, directory)
        }
        runtime::Command::Shell { command, args } => {
            runtime::transport_commands::shell(command, args)
        }
        _ => unreachable!("non-transport command dispatched to transport"),
    }
}

pub(crate) fn run_clone(
    input: runtime::transport_commands::CloneCommandInput,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    runtime::transport_commands::run_clone(input, raw_args)
}

pub(crate) fn run_ls_remote(
    heads: bool,
    tags: bool,
    refs_only: bool,
    repository: Option<String>,
    patterns: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::transport_commands::run_ls_remote(heads, tags, refs_only, repository, patterns)
}

pub(crate) fn run_fetch(
    depth: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::transport_commands::run_fetch(depth, remote, branch)
}

pub(crate) fn run_pull(
    ff_only: bool,
    rebase_mode: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::transport_commands::run_pull(ff_only, rebase_mode, remote, branch)
}

pub(crate) fn run_push(
    force: bool,
    set_upstream: bool,
    remote: Option<String>,
    refspecs: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::transport_commands::run_push(force, set_upstream, remote, refspecs)
}
