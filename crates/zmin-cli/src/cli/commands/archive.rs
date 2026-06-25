use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::GetTarCommitId => run_get_tar_commit_id(),
        runtime::Command::Archive { args } => {
            run_archive(super::archive_commands::parse_archive_args(args)?)
        }
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
