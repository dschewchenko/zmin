use crate::runtime;

fn format_patch_relative(raw_args: &[String]) -> Option<String> {
    let mut relative = crate::runtime::pending_format_patch_relative_arg();
    for arg in raw_args {
        if arg == "--no-relative" {
            relative = None;
        } else if arg == "--relative" && relative.is_none() {
            relative = Some(String::new());
        }
    }
    relative
}

fn format_patch_attach_boundary(raw_args: &[String]) -> Option<String> {
    let mut boundary = crate::runtime::pending_format_patch_attach_arg();
    for arg in raw_args {
        if arg == "--no-attach" {
            boundary = None;
        } else if arg == "--attach" && boundary.is_none() {
            boundary = Some(String::new());
        }
    }
    boundary
}

fn format_patch_inline_boundary(raw_args: &[String]) -> Option<String> {
    let mut boundary = crate::runtime::pending_format_patch_inline_arg();
    for arg in raw_args {
        if arg == "--inline" && boundary.is_none() {
            boundary = Some(String::new());
        }
    }
    boundary
}

fn format_patch_similarity_option(
    raw_args: &[String],
    long_name: &str,
    short_name: &str,
) -> Option<String> {
    let long_prefix = format!("--{long_name}=");
    let short_prefix = format!("-{short_name}");
    let mut iter = raw_args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == &format!("--{long_name}") {
            return Some(String::new());
        }
        if let Some(value) = arg.strip_prefix(&long_prefix) {
            return Some(value.to_owned());
        }
        if let Some(value) = arg.strip_prefix(&short_prefix) {
            if value.is_empty() {
                return Some(String::new());
            }
            return Some(value.to_owned());
        }
    }
    None
}

pub(crate) fn dispatch(
    command: runtime::Command,
    raw_args: &[String],
) -> std::result::Result<(), runtime::CliError> {
    match command {
        runtime::Command::InterpretTrailers {
            in_place,
            trim_empty,
            where_,
            no_where: _,
            if_exists,
            no_if_exists: _,
            if_missing,
            no_if_missing: _,
            only_trailers,
            only_input,
            unfold,
            parse,
            no_divider,
            divider,
            trailers,
            files,
        } => super::mail_commands::interpret_trailers(runtime::InterpretTrailersOptions {
            in_place,
            trim_empty,
            where_: where_.as_deref(),
            if_exists: if_exists.as_deref(),
            if_missing: if_missing.as_deref(),
            only_trailers: only_trailers || parse,
            only_input: only_input || parse,
            unfold: unfold || parse,
            no_divider: no_divider && !divider,
            trailers,
            files,
        }),
        runtime::Command::Mailsplit {
            precision,
            first,
            keep_from,
            keep_cr,
            mboxrd: _,
            output,
            paths,
        } => super::mail_commands::mailsplit(
            precision,
            first,
            keep_from > 0,
            keep_cr > 0,
            output,
            paths,
        ),
        runtime::Command::Mailinfo {
            keep_subject,
            keep_non_patch_brackets,
            message_id,
            recode: _,
            no_recode: _,
            encoding: _,
            scissors: _,
            no_scissors: _,
            quoted_cr: _,
            msg,
            patch,
        } => super::mail_commands::mailinfo(
            keep_subject,
            keep_non_patch_brackets,
            message_id,
            msg,
            patch,
        ),
        runtime::Command::FmtMergeMsg {
            log,
            no_log,
            summary,
            no_summary,
            message,
            into_name,
            file,
        } => {
            let (log, no_log) =
                resolve_fmt_merge_msg_summary_aliases(raw_args, log, no_log, summary, no_summary);
            super::mail_commands::fmt_merge_msg(
                log,
                no_log,
                message.as_deref(),
                into_name.as_deref(),
                file,
            )
        }
        runtime::Command::Am {
            quiet,
            signoff,
            utf8,
            no_utf8,
            keep,
            keep_non_patch,
            keep_cr,
            no_keep_cr,
            message_id,
            no_message_id,
            scissors,
            no_scissors,
            quoted_cr,
            three_way,
            no_three_way,
            ignore_space_change,
            ignore_whitespace,
            whitespace,
            context,
            strip,
            directory,
            include,
            exclude,
            patch_format,
            interactive,
            ignore_date,
            empty,
            reject,
            gpg_sign,
            no_gpg_sign,
            rerere_autoupdate,
            no_rerere_autoupdate,
            resolvemsg,
            no_verify,
            committer_date_is_author_date,
            allow_empty,
            abort,
            quit,
            skip,
            continue_,
            resolved,
            retry,
            show_current_patch,
            patches,
        } => super::mail_commands::am(
            super::mail_commands::AmOptions {
                quiet,
                signoff,
                utf8,
                no_utf8,
                keep,
                keep_non_patch,
                keep_cr,
                no_keep_cr,
                message_id,
                no_message_id,
                scissors,
                no_scissors,
                quoted_cr,
                three_way,
                no_three_way,
                ignore_space_change,
                ignore_whitespace,
                whitespace,
                context,
                strip,
                directory,
                include,
                exclude,
                patch_format,
                interactive,
                ignore_date,
                empty,
                reject,
                gpg_sign,
                no_gpg_sign,
                rerere_autoupdate,
                no_rerere_autoupdate,
                resolvemsg,
                no_verify,
                committer_date_is_author_date,
                allow_empty,
                abort,
                quit,
                skip,
                continue_,
                resolved,
                retry,
                show_current_patch,
            },
            patches,
        ),
        runtime::Command::FormatPatch {
            output_directory,
            output,
            stdout,
            abbrev: _,
            always: _,
            anchored: _,
            binary: _,
            break_rewrites: _,
            color: _,
            color_moved: _,
            color_moved_ws: _,
            compact_summary: _,
            check,
            default_prefix,
            diff_algorithm: _,
            diff_filter: _,
            dst_prefix: _,
            exit_code: _,
            ext_diff: _,
            find_copies: _,
            find_copies_harder,
            find_renames: _,
            function_context: _,
            histogram: _,
            ignore_all_space: _,
            ignore_blank_lines: _,
            ignore_cr_at_eol: _,
            ignore_space_at_eol: _,
            ignore_space_change: _,
            ignore_matching_lines: _,
            ignore_submodules: _,
            indent_heuristic: _,
            inter_hunk_context: _,
            irreversible_delete: _,
            ita_invisible_in_index: _,
            pickaxe_string: _,
            pickaxe_regex: _,
            pickaxe_regex_mode: _,
            pickaxe_all: _,
            find_object: _,
            rename_limit_short: _,
            nul_terminated,
            reverse,
            submodule,
            order_file,
            skip_to,
            rotate_to,
            word_diff,
            color_words,
            word_diff_regex,
            line_prefix: _,
            minimal: _,
            no_attach,
            no_binary: _,
            no_cover_letter,
            cover_from_description,
            description_file,
            no_ext_diff: _,
            no_textconv: _,
            no_color: _,
            no_color_moved: _,
            no_color_moved_ws: _,
            no_indent_heuristic: _,
            name_only,
            name_status,
            no_notes,
            no_prefix,
            no_relative: _,
            no_rename_empty: _,
            no_renames,
            no_thread,
            thread,
            notes,
            to,
            no_to,
            cc,
            no_cc,
            add_header,
            no_add_header,
            in_reply_to,
            from,
            no_from,
            force_in_body_from,
            no_force_in_body_from,
            output_indicator_context: _,
            output_indicator_new: _,
            output_indicator_old: _,
            base,
            no_base,
            patience: _,
            text: _,
            textconv: _,
            stat: _,
            patch: _,
            patch_with_raw,
            patch_with_stat: _,
            no_stat,
            no_patch,
            numstat,
            dirstat,
            dirstat_short,
            cumulative,
            dirstat_by_file,
            shortstat,
            raw,
            summary,
            filename_max_length,
            ignore_if_in_upstream,
            interdiff,
            range_diff,
            creation_factor,
            separate_merges,
            combined_merges,
            tree_in_diff,
            first_parent_diff,
            diff_merges,
            no_diff_merges,
            combined_all_paths,
            remerge_diff,
            progress: _,
            quiet: _,
            relative: _,
            rename_empty: _,
            root: _,
            src_prefix: _,
            full_index,
            unified,
            ws_error_highlight: _,
            break_rewrites_short: _,
            find_copies_short: _,
            irreversible_delete_short: _,
            find_renames_short: _,
            intent_to_add_short: _,
            function_context_short: _,
            attach,
            inline,
            suffix,
            subject_prefix,
            keep_subject,
            no_numbered,
            numbered,
            numbered_files,
            start_number,
            commit_list_format,
            cover_letter,
            signoff,
            signature,
            signature_file,
            no_signature,
            encode_email_headers,
            no_encode_email_headers,
            reroll_count,
            max_count,
            rfc,
            no_rfc,
            zero_commit,
            one,
            revs,
        } => {
            let (effective_rfc, effective_no_rfc) =
                resolve_format_patch_rfc(raw_args, &rfc, no_rfc);
            let reroll_count = resolve_format_patch_reroll_count(raw_args, reroll_count.as_deref());
            let pathspecs = format_patch_pathspecs(raw_args);
            let relative = format_patch_relative(raw_args);
            let attach_boundary = format_patch_attach_boundary(raw_args);
            let inline_boundary = format_patch_inline_boundary(raw_args);
            let no_relative = raw_args.iter().any(|arg| arg == "--no-relative");
            let find_renames = format_patch_similarity_option(raw_args, "find-renames", "M");
            let find_copies = format_patch_similarity_option(raw_args, "find-copies", "C");
            let revs = format_patch_revs_without_pathspecs(revs, &pathspecs);
            super::mail_commands::format_patch(
                output_directory,
                output,
                stdout,
                check,
                name_only,
                name_status,
                format_patch_uses_short_patch_alias(raw_args),
                patch_with_raw,
                no_stat,
                no_patch,
                numstat,
                dirstat.as_deref(),
                dirstat_short.as_deref(),
                cumulative,
                dirstat_by_file.as_deref(),
                shortstat,
                raw,
                summary,
                separate_merges,
                combined_merges,
                tree_in_diff,
                first_parent_diff,
                diff_merges.as_deref(),
                no_diff_merges,
                combined_all_paths,
                remerge_diff,
                full_index,
                unified.as_deref(),
                nul_terminated,
                no_prefix,
                default_prefix,
                relative.as_deref(),
                no_relative,
                find_renames.as_deref(),
                find_copies.as_deref(),
                find_copies_harder,
                no_renames,
                reverse,
                submodule.as_deref(),
                order_file.as_deref(),
                skip_to.as_deref(),
                rotate_to.as_deref(),
                word_diff.as_deref(),
                color_words.as_deref(),
                word_diff_regex.as_deref(),
                attach,
                attach_boundary.as_deref(),
                inline,
                inline_boundary.as_deref(),
                no_attach,
                suffix.as_deref(),
                subject_prefix.as_deref(),
                keep_subject,
                no_numbered,
                numbered,
                numbered_files,
                start_number.as_deref(),
                commit_list_format.as_deref(),
                cover_letter,
                no_cover_letter,
                no_thread,
                thread.as_deref(),
                resolve_format_patch_notes(raw_args, &notes, no_notes),
                no_notes,
                to,
                no_to,
                cc,
                no_cc,
                add_header,
                no_add_header,
                in_reply_to.as_deref(),
                from.as_deref(),
                no_from,
                force_in_body_from,
                no_force_in_body_from,
                cover_from_description.as_deref(),
                description_file.as_deref(),
                signoff,
                signature.as_deref(),
                signature_file.as_deref(),
                no_signature,
                encode_email_headers,
                no_encode_email_headers,
                reroll_count.as_deref(),
                max_count.as_deref(),
                effective_rfc.as_deref(),
                effective_no_rfc,
                base.as_deref(),
                no_base,
                filename_max_length.as_deref(),
                ignore_if_in_upstream,
                interdiff.as_deref(),
                range_diff.as_deref(),
                creation_factor.as_deref(),
                zero_commit,
                one,
                pathspecs,
                revs,
            )
        }
        runtime::Command::SendEmail {
            eight_bit_encoding,
            annotate,
            batch_size,
            dump_aliases,
            translate_aliases,
            bcc,
            cc_cmd,
            cc_cover,
            chain_reply_to,
            cc,
            compose,
            compose_encoding,
            confirm,
            dry_run,
            envelope_sender,
            from,
            reply_to,
            header_cmd,
            identity,
            in_reply_to,
            mailmap,
            no_bcc,
            no_cc,
            no_cc_cover,
            no_chain_reply_to,
            no_header_cmd,
            no_identity,
            no_mailmap,
            no_signed_off_by_cc,
            quiet,
            sendmail_cmd,
            smtp_auth,
            smtp_debug,
            smtp_domain,
            smtp_encryption,
            smtp_pass,
            smtp_server,
            smtp_server_option,
            smtp_server_port,
            smtp_ssl_cert_path,
            smtp_ssl,
            smtp_user,
            subject,
            suppress_cc,
            suppress_from,
            thread,
            to,
            to_cmd,
            to_cover,
            force,
            format_patch,
            no_format_patch,
            no_smtp_auth,
            no_suppress_from,
            no_thread,
            no_to,
            no_to_cover,
            no_validate,
            no_xmailer,
            relogin_delay,
            signed_off_by_cc,
            transfer_encoding,
            validate,
            xmailer,
            args,
        } => super::mail_commands::send_email(super::mail_commands::SendEmailCommandOptions {
            eight_bit_encoding,
            annotate,
            batch_size,
            dump_aliases,
            translate_aliases,
            bcc,
            cc_cmd,
            cc_cover,
            chain_reply_to,
            cc,
            compose,
            compose_encoding,
            confirm,
            dry_run,
            envelope_sender,
            from,
            reply_to,
            header_cmd,
            identity,
            in_reply_to,
            mailmap,
            no_bcc,
            no_cc,
            no_cc_cover,
            no_chain_reply_to,
            no_header_cmd,
            no_identity,
            no_mailmap,
            no_signed_off_by_cc,
            quiet,
            sendmail_cmd,
            smtp_auth,
            smtp_debug,
            smtp_domain,
            smtp_encryption,
            smtp_pass,
            smtp_server,
            smtp_server_option,
            smtp_server_port,
            smtp_ssl_cert_path,
            smtp_ssl,
            smtp_user,
            subject,
            suppress_cc,
            suppress_from,
            thread,
            to,
            to_cmd,
            to_cover,
            force,
            format_patch,
            no_format_patch,
            no_smtp_auth,
            no_suppress_from,
            no_thread,
            no_to,
            no_to_cover,
            no_validate,
            no_xmailer,
            relogin_delay,
            signed_off_by_cc,
            transfer_encoding,
            validate,
            xmailer,
            args,
        }),
        runtime::Command::ImapSend {
            verbose,
            quiet,
            folder,
            list,
            curl,
            no_curl,
        } => super::mail_commands::imap_send(super::mail_commands::ImapSendOptions {
            verbose,
            quiet,
            folder,
            list,
            curl,
            no_curl,
        }),
        _ => unreachable!("non-mail command dispatched to mail"),
    }
}

fn resolve_format_patch_rfc(
    raw_args: &[String],
    parsed_rfc: &[String],
    parsed_no_rfc: bool,
) -> (Option<String>, bool) {
    let mut effective_rfc = parsed_rfc.last().cloned();
    let mut effective_no_rfc = false;
    let mut in_format_patch = false;
    for arg in raw_args {
        if !in_format_patch {
            if arg == "format-patch" {
                in_format_patch = true;
            }
            continue;
        }
        if arg == "--" {
            break;
        }
        if arg == "--no-rfc" {
            effective_rfc = None;
            effective_no_rfc = true;
            continue;
        }
        if arg == "--rfc" {
            effective_rfc = Some("RFC".to_owned());
            effective_no_rfc = false;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--rfc=") {
            if value.is_empty() {
                effective_rfc = None;
                effective_no_rfc = true;
            } else {
                effective_rfc = Some(value.to_owned());
                effective_no_rfc = false;
            }
        }
    }
    if !effective_no_rfc && parsed_no_rfc && effective_rfc.is_none() {
        effective_no_rfc = true;
    }
    (effective_rfc, effective_no_rfc)
}

fn resolve_format_patch_reroll_count(
    raw_args: &[String],
    parsed_reroll_count: Option<&str>,
) -> Option<String> {
    let mut effective = parsed_reroll_count.map(str::to_owned);
    let mut in_format_patch = false;
    let mut index = 0usize;
    while index < raw_args.len() {
        let arg = &raw_args[index];
        if !in_format_patch {
            if arg == "format-patch" {
                in_format_patch = true;
            }
            index += 1;
            continue;
        }
        if arg == "--" {
            break;
        }
        if arg == "--reroll-count" || arg == "-v" {
            let next = raw_args.get(index + 1);
            if let Some(value) =
                next.filter(|value| !value.starts_with('-') && value.as_str() != "--")
            {
                effective = Some(value.clone());
                index += 2;
                continue;
            }
            effective = Some(String::new());
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--reroll-count=") {
            effective = Some(value.to_owned());
        } else if let Some(value) = arg.strip_prefix("-v")
            && !value.is_empty()
        {
            effective = Some(value.to_owned());
        }
        index += 1;
    }
    effective
}

fn resolve_format_patch_notes(
    raw_args: &[String],
    parsed_notes: &[String],
    parsed_no_notes: bool,
) -> Vec<String> {
    let mut resolved = Vec::new();
    let mut in_format_patch = false;
    let mut index = 0usize;
    while index < raw_args.len() {
        let arg = &raw_args[index];
        if !in_format_patch {
            if arg == "format-patch" {
                in_format_patch = true;
            }
            index += 1;
            continue;
        }
        if arg == "--" {
            break;
        }
        if arg == "--no-notes" {
            resolved.clear();
            index += 1;
            continue;
        }
        if arg == "--notes" {
            let next = raw_args.get(index + 1);
            if let Some(value) =
                next.filter(|value| !value.starts_with('-') && value.as_str() != "--")
            {
                resolved.push(value.clone());
                index += 2;
                continue;
            }
            resolved.push(String::new());
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--notes=") {
            resolved.push(value.to_owned());
        }
        index += 1;
    }
    if resolved.is_empty() {
        if parsed_no_notes {
            return Vec::new();
        }
        return parsed_notes.to_vec();
    }
    resolved
}

fn resolve_fmt_merge_msg_summary_aliases(
    raw_args: &[String],
    log: Option<usize>,
    no_log: bool,
    summary: Option<usize>,
    no_summary: bool,
) -> (Option<usize>, bool) {
    if log.is_none() && !no_log && summary.is_none() && !no_summary {
        return (None, false);
    }

    let mut resolved_log = log;
    let mut resolved_no_log = no_log;
    for arg in raw_args {
        if let Some(value) = arg.strip_prefix("--log=") {
            resolved_log = value.parse().ok();
            resolved_no_log = false;
            continue;
        }
        if arg == "--log" {
            resolved_log = log.or(Some(20));
            resolved_no_log = false;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--summary=") {
            resolved_log = value.parse().ok();
            resolved_no_log = false;
            continue;
        }
        if arg == "--summary" {
            resolved_log = summary.or(Some(20));
            resolved_no_log = false;
            continue;
        }
        if arg == "--no-log" || arg == "--no-summary" {
            resolved_log = None;
            resolved_no_log = true;
        }
    }
    (resolved_log, resolved_no_log)
}

fn format_patch_uses_short_patch_alias(raw_args: &[String]) -> bool {
    raw_args.iter().any(|arg| arg == "-p")
}

fn format_patch_pathspecs(raw_args: &[String]) -> Vec<String> {
    let mut pathspecs = Vec::new();
    let mut after_dashdash = false;
    for arg in raw_args {
        if !after_dashdash {
            if arg == "--" {
                after_dashdash = true;
            }
            continue;
        }
        pathspecs.push(arg.clone());
    }
    pathspecs
}

fn format_patch_revs_without_pathspecs(mut revs: Vec<String>, pathspecs: &[String]) -> Vec<String> {
    if pathspecs.is_empty() || revs.len() < pathspecs.len() {
        return revs;
    }
    if revs[revs.len() - pathspecs.len()..] == *pathspecs {
        revs.truncate(revs.len() - pathspecs.len());
    }
    revs
}
