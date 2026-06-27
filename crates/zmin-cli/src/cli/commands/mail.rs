use crate::runtime;

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
        } => {
            super::mail_commands::mailsplit(precision, first, keep_from > 0, keep_cr > 0, output, paths)
        }
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
            stdout,
            binary: _,
            default_prefix: _,
            no_ext_diff: _,
            no_textconv: _,
            no_color: _,
            no_color_moved: _,
            no_color_moved_ws: _,
            stat: _,
            patch: _,
            attach,
            inline,
            suffix,
            subject_prefix,
            no_numbered,
            numbered,
            numbered_files,
            cover_letter,
            one,
            revs,
        } => super::mail_commands::format_patch(
            output_directory,
            stdout,
            attach,
            inline,
            suffix.as_deref(),
            subject_prefix.as_deref(),
            no_numbered,
            numbered,
            numbered_files,
            cover_letter,
            one,
            revs,
        ),
        runtime::Command::SendEmail {
            dump_aliases,
            translate_aliases,
            args,
        } => super::mail_commands::send_email(dump_aliases, translate_aliases, args),
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
