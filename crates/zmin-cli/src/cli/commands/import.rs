use crate::runtime;

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
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
            anonymize,
            anonymize_map,
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
                anonymize,
                anonymize_map,
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
        runtime::Command::FastImport {
            date_format,
            quiet,
            stats,
            force,
            done,
            allow_unsafe_features,
            active_branches,
            depth,
            big_file_threshold,
            cat_blob_fd,
            export_marks,
            export_pack_edges,
            import_marks,
            import_marks_if_exists,
            max_pack_size,
            no_relative_marks,
            relative_marks,
            rewrite_submodules_from,
            rewrite_submodules_to,
        } => {
            let date_format = super::import_commands::resolve_fast_import_last_value(&date_format);
            let (quiet, stats) =
                super::import_commands::resolve_fast_import_stats_mode(raw_args, quiet > 0, stats > 0);
            let max_pack_size =
                super::import_commands::resolve_fast_import_last_value(&max_pack_size);
            super::import_commands::fast_import(super::import_commands::FastImportOptions {
                date_format,
                quiet,
                stats,
                force: force > 0,
                done: done > 0,
                allow_unsafe_features: allow_unsafe_features > 0,
                active_branches: super::import_commands::resolve_fast_import_last_value(
                    &active_branches,
                ),
                depth: super::import_commands::resolve_fast_import_last_value(&depth),
                big_file_threshold: super::import_commands::resolve_fast_import_last_value(
                    &big_file_threshold,
                ),
                cat_blob_fd: super::import_commands::resolve_fast_import_last_value(&cat_blob_fd),
                export_marks: super::import_commands::resolve_fast_import_last_value(
                    &export_marks,
                ),
                export_pack_edges,
                import_marks,
                import_marks_if_exists,
                max_pack_size,
                max_pack_size_warning:
                    super::import_commands::resolve_fast_import_max_pack_size_warning(raw_args),
                no_relative_marks,
                relative_marks,
                rewrite_submodules_from,
                rewrite_submodules_to,
            })
        }
        _ => unreachable!("non-import command dispatched to import"),
    }
}
