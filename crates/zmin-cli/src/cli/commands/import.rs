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
        runtime::Command::FastExport {
            all,
            progress,
            signed_tags,
            tag_of_filtered_object,
            reencode,
            export_marks,
            import_marks,
            import_marks_if_exists,
            fake_missing_tagger,
            full_tree,
            use_done_feature,
            no_data,
            refspec,
            reference_excluded_parents,
            show_original_ids,
            mark_tags,
            detect_copies,
            detect_renames,
            refs,
        } => {
            super::import_commands::fast_export(super::import_commands::FastExportOptions {
                all: all > 0,
                progress,
                signed_tags,
                tag_of_filtered_object,
                reencode,
                export_marks,
                import_marks,
                import_marks_if_exists,
                fake_missing_tagger,
                full_tree,
                use_done_feature,
                no_data,
                refspec,
                reference_excluded_parents,
                show_original_ids,
                mark_tags,
                detect_copies,
                detect_renames,
                refs,
            })
        }
        runtime::Command::FastImport { date_format } => {
            super::import_commands::fast_import(date_format.as_deref())
        }
        _ => unreachable!("non-import command dispatched to import"),
    }
}
