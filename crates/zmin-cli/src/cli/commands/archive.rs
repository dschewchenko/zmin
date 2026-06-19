use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::GetTarCommitId => run_get_tar_commit_id(),
        runtime::Command::Archive {
            format,
            prefix,
            output,
            add_files,
            add_virtual_files,
            mtime,
            list,
            verbose,
            treeish,
            paths,
        } => run_archive(super::archive_commands::ArchiveOptions {
            format,
            prefix,
            output,
            add_files,
            add_virtual_files,
            mtime,
            list,
            verbose,
            treeish,
            paths,
        }),
        runtime::Command::UploadArchive { repository } => {
            super::archive_commands::upload_archive(repository)
        }
        _ => unreachable!("non-archive command dispatched to archive"),
    }
}

pub(crate) fn run_get_tar_commit_id() -> std::result::Result<(), runtime::CliError> {
    super::archive_commands::get_tar_commit_id()
}

pub(crate) fn run_archive(
    options: super::archive_commands::ArchiveOptions,
) -> std::result::Result<(), runtime::CliError> {
    super::archive_commands::archive(options)
}
