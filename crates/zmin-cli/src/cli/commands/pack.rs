use crate::runtime;
use std::path::PathBuf;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::PackObjects {
            quiet,
            compression,
            stdout,
            revs,
            all,
            stdin_packs,
            progress,
            all_progress_implied,
            no_progress,
            index_version,
            honor_pack_keep,
            include_tag,
            incremental,
            keep_true_parents,
            delta_islands,
            keep_unreachable,
            cruft,
            cruft_expiration,
            local,
            non_empty,
            no_reuse_delta,
            no_reuse_object,
            sparse,
            no_sparse,
            shallow,
            delta_base_offset,
            threads,
            unpacked,
            max_pack_size,
            unpack_unreachable,
            keep_pack,
            pack_loose_unreachable,
            window,
            window_memory,
            depth,
            base_name,
        } => run_pack_objects(runtime::PackObjectsOptions {
            quiet,
            compression,
            stdout,
            revs,
            all,
            stdin_packs,
            progress,
            all_progress_implied,
            no_progress,
            index_version,
            honor_pack_keep,
            include_tag,
            incremental,
            keep_true_parents,
            delta_islands,
            keep_unreachable,
            cruft,
            cruft_expiration,
            local,
            non_empty,
            no_reuse_delta,
            no_reuse_object,
            sparse,
            no_sparse,
            shallow,
            delta_base_offset,
            threads,
            unpacked,
            max_pack_size,
            unpack_unreachable,
            keep_pack,
            pack_loose_unreachable,
            window,
            window_memory,
            depth,
            base_name,
        }),
        runtime::Command::Bundle {
            quiet,
            no_quiet,
            progress,
            no_progress,
            operation,
            version,
            file,
            args,
        } => {
            let quiet = resolve_bundle_quiet(raw_args, quiet, no_quiet);
            let no_quiet = resolve_bundle_no_quiet(raw_args, quiet, no_quiet);
            let progress = resolve_bundle_progress(raw_args, progress, no_progress)
                || (operation == "create" && no_quiet);
            run_bundle(quiet, progress, false, operation, version, file, args)
        }
        runtime::Command::IndexPack {
            stdin,
            output,
            keep,
            rev_index,
            no_rev_index,
            verify,
            strict,
            fsck_objects,
            check_self_contained_and_connected,
            fix_thin,
            progress_title,
            verbose,
            index_version,
            threads,
            max_input_size,
            object_format,
            promisor,
            pack_file,
        } => run_index_pack(runtime::IndexPackOptions {
            stdin,
            output,
            keep,
            rev_index,
            no_rev_index,
            verify,
            strict,
            fsck_objects,
            check_self_contained_and_connected,
            fix_thin,
            progress_title,
            verbose,
            index_version,
            threads,
            max_input_size,
            object_format,
            promisor,
            pack_file,
        }),
        runtime::Command::Fsck {
            unreachable,
            dangling,
            no_dangling,
            strict,
            full,
            connectivity_only,
            no_reflogs,
            cache,
            tags,
            root,
            verbose,
            lost_found,
            progress,
            no_progress,
            name_objects,
            references,
            no_references,
            objects,
        } => super::pack_commands::fsck(
            unreachable,
            dangling,
            no_dangling,
            strict,
            full,
            connectivity_only,
            no_reflogs,
            cache,
            tags,
            root,
            verbose,
            lost_found,
            progress,
            no_progress,
            name_objects,
            references,
            no_references,
            objects,
        ),
        runtime::Command::VerifyPack {
            verbose,
            no_verbose,
            stat_only,
            no_stat_only,
            object_format,
            packs,
        } => super::pack_commands::verify_pack(
            verbose > 0 && no_verbose == 0,
            stat_only > 0 && no_stat_only == 0,
            object_format.as_deref(),
            packs,
        ),
        runtime::Command::PackRedundant {
            verbose,
            alt_odb,
            all,
            i_still_use_this: _,
            packs,
        } => super::pack_commands::pack_redundant(verbose, alt_odb, all, packs),
        runtime::Command::VerifyCommit {
            verbose,
            raw,
            commits,
        } => super::pack_commands::verify_commit(verbose, raw, commits),
        runtime::Command::VerifyTag {
            verbose,
            raw,
            format,
            tags,
        } => super::pack_commands::verify_tag(verbose, raw, format.as_deref(), tags),
        runtime::Command::Mktag {
            strict: _,
            no_strict: _,
        } => super::pack_commands::mktag_command(),
        runtime::Command::CommitGraph { command } => super::pack_commands::commit_graph_command(
            resolve_commit_graph_command_toggles(command, raw_args),
        ),
        runtime::Command::MultiPackIndex {
            object_dir,
            command,
        } => super::pack_commands::multi_pack_index_command(
            object_dir,
            resolve_multi_pack_index_command_toggles(command, raw_args),
        ),
        _ => unreachable!("non-pack command dispatched to pack"),
    }
}

fn resolve_commit_graph_command_toggles(
    command: runtime::CommitGraphCommand,
    raw_args: &[String],
) -> runtime::CommitGraphCommand {
    match command {
        runtime::CommitGraphCommand::Write {
            object_dir,
            reachable,
            progress,
            no_progress,
        } => runtime::CommitGraphCommand::Write {
            object_dir,
            reachable,
            progress: resolve_commit_graph_progress(raw_args, progress, no_progress),
            no_progress: false,
        },
        runtime::CommitGraphCommand::Verify {
            object_dir,
            progress,
            no_progress,
        } => runtime::CommitGraphCommand::Verify {
            object_dir,
            progress: resolve_commit_graph_progress(raw_args, progress, no_progress),
            no_progress: false,
        },
    }
}

fn resolve_commit_graph_progress(raw_args: &[String], progress: bool, no_progress: bool) -> bool {
    if !progress && !no_progress {
        return false;
    }

    let mut resolved = false;
    for arg in raw_args {
        match arg.as_str() {
            "--progress" => resolved = true,
            "--no-progress" => resolved = false,
            _ => {}
        }
    }
    resolved
}

fn resolve_multi_pack_index_command_toggles(
    command: runtime::MultiPackIndexCommand,
    raw_args: &[String],
) -> runtime::MultiPackIndexCommand {
    match command {
        runtime::MultiPackIndexCommand::Write {
            bitmap,
            preferred_pack,
            no_bitmap,
            refs_snapshot,
            incremental,
            stdin_packs,
            progress,
            no_progress,
        } => runtime::MultiPackIndexCommand::Write {
            bitmap,
            preferred_pack,
            no_bitmap,
            refs_snapshot,
            incremental,
            stdin_packs,
            progress: resolve_commit_graph_progress(raw_args, progress, no_progress),
            no_progress: false,
        },
        runtime::MultiPackIndexCommand::Verify {
            progress,
            no_progress,
        } => runtime::MultiPackIndexCommand::Verify {
            progress: resolve_commit_graph_progress(raw_args, progress, no_progress),
            no_progress: false,
        },
        runtime::MultiPackIndexCommand::Expire {
            progress,
            no_progress,
        } => runtime::MultiPackIndexCommand::Expire {
            progress: resolve_commit_graph_progress(raw_args, progress, no_progress),
            no_progress: false,
        },
        runtime::MultiPackIndexCommand::Repack {
            batch_size,
            progress,
            no_progress,
        } => runtime::MultiPackIndexCommand::Repack {
            batch_size,
            progress: resolve_commit_graph_progress(raw_args, progress, no_progress),
            no_progress: false,
        },
    }
}

fn resolve_bundle_progress(raw_args: &[String], progress: bool, no_progress: bool) -> bool {
    if !progress && !no_progress {
        return false;
    }

    let mut resolved = false;
    for arg in raw_args {
        match arg.as_str() {
            "--progress" => resolved = true,
            "--no-progress" => resolved = false,
            _ => {}
        }
    }
    resolved
}

fn resolve_bundle_quiet(raw_args: &[String], quiet: bool, no_quiet: bool) -> bool {
    if !quiet && !no_quiet {
        return false;
    }

    let mut resolved = false;
    for arg in raw_args {
        match arg.as_str() {
            "-q" | "--quiet" => resolved = true,
            "--no-quiet" => resolved = false,
            _ => {}
        }
    }
    resolved
}

fn resolve_bundle_no_quiet(raw_args: &[String], quiet: bool, no_quiet: bool) -> bool {
    if !quiet && !no_quiet {
        return false;
    }

    let mut resolved = false;
    for arg in raw_args {
        match arg.as_str() {
            "-q" | "--quiet" => resolved = false,
            "--no-quiet" => resolved = true,
            _ => {}
        }
    }
    resolved
}

pub(crate) fn run_pack_objects(
    options: runtime::PackObjectsOptions,
) -> std::result::Result<(), runtime::CliError> {
    super::pack_commands::pack_objects(options)
}

pub(crate) fn run_bundle(
    quiet: bool,
    progress: bool,
    no_progress: bool,
    operation: String,
    version: Option<String>,
    file: PathBuf,
    args: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    super::pack_commands::bundle(
        quiet,
        progress,
        no_progress,
        &operation,
        version,
        file,
        args,
    )
}

pub(crate) fn run_index_pack(
    options: runtime::IndexPackOptions,
) -> std::result::Result<(), runtime::CliError> {
    super::pack_commands::index_pack(options)
}
