use crate::runtime;
use std::path::PathBuf;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::PackObjects {
            stdout,
            revs,
            all,
            progress,
            no_progress,
            index_version,
            no_reuse_delta,
            no_reuse_object,
            delta_base_offset,
            window,
            depth,
            base_name,
        } => run_pack_objects(runtime::PackObjectsOptions {
            stdout,
            revs,
            all,
            progress,
            no_progress,
            index_version,
            no_reuse_delta,
            no_reuse_object,
            delta_base_offset,
            window,
            depth,
            base_name,
        }),
        runtime::Command::Bundle {
            operation,
            file,
            args,
        } => run_bundle(operation, file, args),
        runtime::Command::IndexPack {
            stdin,
            output,
            keep,
            rev_index,
            no_rev_index,
            verify,
            strict,
            fsck_objects,
            fix_thin,
            verbose,
            index_version,
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
            fix_thin,
            verbose,
            index_version,
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
        } => runtime::pack_commands::fsck(
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
            stat_only,
            object_format,
            packs,
        } => {
            runtime::pack_commands::verify_pack(verbose, stat_only, object_format.as_deref(), packs)
        }
        runtime::Command::PackRedundant {
            verbose,
            alt_odb,
            all,
            i_still_use_this: _,
            packs,
        } => runtime::pack_commands::pack_redundant(verbose, alt_odb, all, packs),
        runtime::Command::VerifyCommit {
            verbose,
            raw,
            commits,
        } => runtime::pack_commands::verify_commit(verbose, raw, commits),
        runtime::Command::VerifyTag { verbose, raw, tags } => {
            runtime::pack_commands::verify_tag(verbose, raw, tags)
        }
        runtime::Command::Mktag { strict: _ } => runtime::pack_commands::mktag_command(),
        runtime::Command::CommitGraph { command } => {
            runtime::pack_commands::commit_graph_command(command)
        }
        runtime::Command::MultiPackIndex {
            object_dir,
            command,
        } => runtime::pack_commands::multi_pack_index_command(object_dir, command),
        _ => unreachable!("non-pack command dispatched to pack"),
    }
}

pub(crate) fn run_pack_objects(
    options: runtime::PackObjectsOptions,
) -> std::result::Result<(), runtime::CliError> {
    runtime::pack_commands::pack_objects(options)
}

pub(crate) fn run_bundle(
    operation: String,
    file: PathBuf,
    args: Vec<String>,
) -> std::result::Result<(), runtime::CliError> {
    runtime::pack_commands::bundle(&operation, file, args)
}

pub(crate) fn run_index_pack(
    options: runtime::IndexPackOptions,
) -> std::result::Result<(), runtime::CliError> {
    runtime::pack_commands::index_pack(options)
}
