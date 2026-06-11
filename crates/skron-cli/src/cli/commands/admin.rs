use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::ForEachRepo {
            config,
            keep_going,
            arguments,
        } => runtime::admin_commands::for_each_repo_command(&config, keep_going, arguments),
        runtime::Command::UpdateIndex {
            add,
            remove,
            force_remove,
            refresh,
            really_refresh,
            cacheinfo,
            chmod,
            stdin,
            nul_terminated,
            paths,
        } => runtime::admin_commands::update_index_command(
            runtime::admin_commands::UpdateIndexCommandOptions {
                add,
                remove,
                force_remove,
                refresh: refresh || really_refresh,
                cacheinfo,
                chmod,
                stdin,
                nul_terminated,
                paths,
            },
        ),
        runtime::Command::Bugreport {
            output_directory,
            suffix,
            no_suffix,
            diagnose,
        } => runtime::admin_commands::bugreport_command(
            output_directory,
            suffix.as_deref(),
            no_suffix,
            diagnose.as_deref(),
        ),
        runtime::Command::Diagnose {
            output_directory,
            suffix,
            mode,
        } => runtime::admin_commands::diagnose_command_entry(
            output_directory,
            suffix.as_deref(),
            &mode,
        ),
        runtime::Command::Backfill {
            min_batch_size,
            sparse,
            no_sparse,
            revs,
        } => runtime::admin_commands::backfill_command(min_batch_size, sparse, no_sparse, revs),
        runtime::Command::Gitk { args } => {
            runtime::admin_commands::not_ready_current_git_command("gitk", args)
        }
        runtime::Command::Gitweb { args } => {
            runtime::admin_commands::not_ready_current_git_command("gitweb", args)
        }
        runtime::Command::Scalar {
            directories,
            configs,
            help,
            command,
        } => runtime::scalar_commands::scalar_command(directories, configs, help, command),
        runtime::Command::Hook { command } => run_hook(command),
        runtime::Command::ShI18n { args } => runtime::admin_commands::sh_i18n_command(args),
        runtime::Command::ShSetup { args } => runtime::admin_commands::sh_setup_command(args),
        runtime::Command::Cvsserver { args } => runtime::admin_commands::cvsserver_command(args),
        runtime::Command::Cvsexportcommit { args } => {
            runtime::admin_commands::cvsexportcommit_command(args)
        }
        runtime::Command::Cvsimport { args } => runtime::admin_commands::cvsimport_command(args),
        runtime::Command::Archimport { args } => runtime::admin_commands::archimport_command(args),
        runtime::Command::P4 { args } => runtime::admin_commands::p4_command(args),
        runtime::Command::Svn { args } => runtime::admin_commands::svn_command(args),
        runtime::Command::Instaweb {
            start,
            stop,
            restart,
            local,
            port,
            httpd,
            browser,
            daemon_internal,
            git_dir,
            work_tree,
        } => runtime::admin_commands::instaweb_command(
            runtime::admin_commands::InstawebCommandOptions {
                start,
                stop,
                restart,
                local,
                port,
                httpd,
                browser,
                daemon_internal,
                git_dir,
                work_tree,
            },
        ),
        _ => unreachable!("non-admin command dispatched to admin"),
    }
}

pub(crate) fn run_hook(
    command: runtime::HookCommand,
) -> std::result::Result<(), runtime::CliError> {
    runtime::admin_commands::hook(command)
}
