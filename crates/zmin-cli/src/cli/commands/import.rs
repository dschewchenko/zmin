use crate::runtime;

pub(crate) fn dispatch(command: runtime::Command) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::Quiltimport {
            dry_run,
            author,
            patches,
            series,
            keep_non_patch,
        } => super::import_commands::quiltimport(
            dry_run,
            author.as_deref(),
            patches,
            series,
            keep_non_patch,
        ),
        runtime::Command::FastExport { all, refs } => {
            super::import_commands::fast_export(all, refs)
        }
        runtime::Command::FastImport { date_format } => {
            super::import_commands::fast_import(date_format.as_deref())
        }
        _ => unreachable!("non-import command dispatched to import"),
    }
}
