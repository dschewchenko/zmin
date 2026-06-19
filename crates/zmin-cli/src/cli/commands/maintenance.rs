use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Maintenance {
            operation,
            auto,
            schedule,
            scheduler,
            config_file,
            force,
            quiet,
            tasks,
        } => super::maintenance_commands::maintenance(
            super::maintenance_commands::MaintenanceOptions {
                operation: &operation,
                auto,
                schedule: schedule.as_deref(),
                scheduler: scheduler.as_deref(),
                config_file: config_file.as_deref(),
                force,
                quiet,
                tasks,
            },
        ),
        runtime::Command::PrunePacked { dry_run, quiet } => {
            super::maintenance_commands::prune_packed_command(dry_run, quiet)
        }
        runtime::Command::Repack {
            all,
            all_and_loosen_unreachable,
            delete_redundant,
            quiet,
            no_update_server_info,
            no_reuse_delta,
            no_reuse_object,
            local,
            write_bitmap_index,
            no_write_bitmap_index,
            write_midx,
            no_write_midx,
            window,
            depth,
            threads,
            keep_pack,
        } => super::maintenance_commands::repack_command(
            all,
            all_and_loosen_unreachable,
            delete_redundant,
            quiet,
            no_update_server_info,
            no_reuse_delta,
            no_reuse_object,
            local,
            write_bitmap_index,
            no_write_bitmap_index,
            write_midx,
            no_write_midx,
            window,
            depth,
            threads,
            keep_pack,
        ),
        runtime::Command::Gc {
            prune,
            no_prune,
            auto,
            aggressive,
            quiet,
        } => super::maintenance_commands::gc_command(prune, no_prune, auto, aggressive, quiet),
        runtime::Command::Prune { args } => super::maintenance_commands::prune_command(args),
        command => {
            unreachable!("non-maintenance command routed to maintenance dispatcher: {command:?}")
        }
    }
}
