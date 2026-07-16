use super::*;
use crate::runtime::current_unix_timestamp;
use std::collections::{HashMap, HashSet};
use std::process::{Command as ProcessCommand, Stdio};

#[derive(Debug, Clone)]
pub(crate) struct AmOptions {
    pub(crate) quiet: bool,
    pub(crate) signoff: bool,
    pub(crate) utf8: bool,
    pub(crate) no_utf8: bool,
    pub(crate) keep: bool,
    pub(crate) keep_non_patch: bool,
    pub(crate) keep_cr: bool,
    pub(crate) no_keep_cr: bool,
    pub(crate) message_id: bool,
    pub(crate) no_message_id: bool,
    pub(crate) scissors: bool,
    pub(crate) no_scissors: bool,
    pub(crate) quoted_cr: Option<String>,
    pub(crate) three_way: bool,
    pub(crate) no_three_way: bool,
    pub(crate) ignore_space_change: bool,
    pub(crate) ignore_whitespace: bool,
    pub(crate) whitespace: Option<String>,
    pub(crate) context: Option<String>,
    pub(crate) strip: Option<String>,
    pub(crate) directory: Vec<String>,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) patch_format: Option<String>,
    pub(crate) interactive: bool,
    pub(crate) ignore_date: bool,
    pub(crate) empty: Option<String>,
    pub(crate) reject: u8,
    pub(crate) gpg_sign: Option<String>,
    pub(crate) no_gpg_sign: u8,
    pub(crate) rerere_autoupdate: bool,
    pub(crate) no_rerere_autoupdate: bool,
    pub(crate) resolvemsg: Option<String>,
    pub(crate) no_verify: bool,
    pub(crate) committer_date_is_author_date: bool,
    pub(crate) allow_empty: bool,
    pub(crate) abort: bool,
    pub(crate) quit: bool,
    pub(crate) skip: bool,
    pub(crate) continue_: bool,
    pub(crate) resolved: bool,
    pub(crate) retry: bool,
    pub(crate) show_current_patch: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SendEmailCommandOptions {
    pub(crate) eight_bit_encoding: Option<String>,
    pub(crate) annotate: bool,
    pub(crate) batch_size: Option<String>,
    pub(crate) dump_aliases: bool,
    pub(crate) translate_aliases: bool,
    pub(crate) bcc: Vec<String>,
    pub(crate) cc_cmd: Option<String>,
    pub(crate) cc_cover: bool,
    pub(crate) chain_reply_to: bool,
    pub(crate) cc: Vec<String>,
    pub(crate) compose: bool,
    pub(crate) compose_encoding: Option<String>,
    pub(crate) confirm: Option<String>,
    pub(crate) dry_run: bool,
    pub(crate) envelope_sender: Option<String>,
    pub(crate) from: Option<String>,
    pub(crate) force: bool,
    pub(crate) format_patch: bool,
    pub(crate) header_cmd: Option<String>,
    pub(crate) identity: Option<String>,
    pub(crate) in_reply_to: Option<String>,
    pub(crate) mailmap: bool,
    pub(crate) no_format_patch: bool,
    pub(crate) no_bcc: bool,
    pub(crate) no_cc: bool,
    pub(crate) no_cc_cover: bool,
    pub(crate) no_chain_reply_to: bool,
    pub(crate) no_header_cmd: bool,
    pub(crate) no_identity: bool,
    pub(crate) no_mailmap: bool,
    pub(crate) no_signed_off_by_cc: bool,
    pub(crate) quiet: bool,
    pub(crate) sendmail_cmd: Option<String>,
    pub(crate) no_smtp_auth: bool,
    pub(crate) no_suppress_from: bool,
    pub(crate) no_thread: bool,
    pub(crate) no_to: bool,
    pub(crate) no_to_cover: bool,
    pub(crate) no_validate: bool,
    pub(crate) no_xmailer: bool,
    pub(crate) relogin_delay: Option<String>,
    pub(crate) reply_to: Option<String>,
    pub(crate) signed_off_by_cc: bool,
    pub(crate) smtp_auth: Option<String>,
    pub(crate) smtp_debug: Option<String>,
    pub(crate) smtp_domain: Option<String>,
    pub(crate) smtp_encryption: Option<String>,
    pub(crate) smtp_pass: Option<String>,
    pub(crate) smtp_server: Option<String>,
    pub(crate) smtp_server_option: Vec<String>,
    pub(crate) smtp_server_port: Option<String>,
    pub(crate) smtp_ssl_cert_path: Option<String>,
    pub(crate) smtp_ssl: bool,
    pub(crate) smtp_user: Option<String>,
    pub(crate) subject: Option<String>,
    pub(crate) suppress_cc: Vec<String>,
    pub(crate) suppress_from: bool,
    pub(crate) thread: bool,
    pub(crate) to: Vec<String>,
    pub(crate) to_cmd: Option<String>,
    pub(crate) to_cover: bool,
    pub(crate) transfer_encoding: Option<String>,
    pub(crate) validate: bool,
    pub(crate) xmailer: bool,
    pub(crate) args: Vec<String>,
}

#[derive(Debug, Clone)]
struct AmSession {
    raw_mail: String,
    patch_text: String,
    subject: String,
}

struct ParsedAmMail {
    author: Signature,
    subject: String,
    message_body: String,
    patch_text: String,
}

pub(crate) fn am(options: AmOptions, patches: Vec<PathBuf>) -> Result<()> {
    if options.interactive {
        print!("{}", am_interactive_prompt());
        return Err(CliError::Fatal {
            code: 128,
            message: "unable to read from stdin; aborting".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if options.requires_existing_session() {
        let Some(session) = load_am_session(&repo)? else {
            return Err(am_resume_not_in_progress_error());
        };
        return resume_am_session(&repo, &options, &session);
    }
    if !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot apply patches with a dirty worktree".into(),
        });
    }
    let mails = read_am_mails(&patches)?;
    if mails.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "No mail patches supplied".into(),
        });
    }
    for mail in mails {
        apply_mail_patch(&repo, &store, &mail, &options)?;
    }
    Ok(())
}

fn read_am_mails(paths: &[PathBuf]) -> Result<Vec<String>> {
    if paths.is_empty() {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        return Ok(split_am_mailbox(&input));
    }
    let mut mails = Vec::new();
    for path in paths {
        let input = fs::read_to_string(path)?;
        mails.extend(split_am_mailbox(&input));
    }
    Ok(mails)
}

fn split_am_mailbox(input: &str) -> Vec<String> {
    let mut mails = Vec::new();
    let mut current = Vec::new();
    for line in input.lines() {
        if line.starts_with("From ") && !current.is_empty() {
            mails.push(lines_with_final_newline(&current));
            current.clear();
            continue;
        }
        current.push(line.to_owned());
    }
    if !current.is_empty() {
        mails.push(lines_with_final_newline(&current));
    }
    mails
}

fn apply_mail_patch(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mail: &str,
    am_options: &AmOptions,
) -> Result<()> {
    let parsed = parse_am_mail(mail, am_options.keep, am_options.patch_format.as_deref())?;
    let author = if am_options.ignore_date {
        Signature::new(
            parsed.author.name.clone(),
            parsed.author.email.clone(),
            current_unix_timestamp()?,
            crate::runtime::local_now().format("%z").to_string(),
        )?
    } else {
        parsed.author.clone()
    };
    let mut committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    if am_options.committer_date_is_author_date {
        committer = Signature::new(
            committer.name.clone(),
            committer.email.clone(),
            author.timestamp,
            author.timezone.clone(),
        )?;
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    if parsed.patch_text.trim().is_empty() {
        return handle_empty_am_mail(
            repo, store, mail, &head_id, &parsed, &author, &committer, am_options,
        );
    }
    let mut index = read_repo_index(repo)?;
    let options = patch_commands::ApplyOptions {
        allow_empty: false,
        allow_binary_replacement: false,
        apply: false,
        binary: false,
        check: false,
        cached: false,
        stat: false,
        numstat: false,
        summary: false,
        build_fake_ancestor: None,
        index: true,
        recount: false,
        quiet: am_options.quiet,
        verbose: false,
        unsafe_paths: false,
        unidiff_zero: false,
        ignore_space_change: am_options.ignore_space_change,
        ignore_whitespace: am_options.ignore_whitespace,
        inaccurate_eof: false,
        whitespace: am_options.whitespace.clone(),
        strip: am_options
            .strip
            .as_ref()
            .and_then(|value| value.parse().ok()),
        context: am_options
            .context
            .as_ref()
            .and_then(|value| value.parse().ok()),
        directory: None,
        include: Vec::new(),
        exclude: Vec::new(),
        intent_to_add: false,
        no_add: false,
        z: false,
        reject: am_options.reject > 0,
        three_way: am_options.three_way && !am_options.no_three_way,
        ours: false,
        theirs: false,
        union: false,
        reverse: false,
        patches: Vec::new(),
    };
    let _accepted_parser_only = (
        am_options.utf8,
        am_options.no_utf8,
        am_options.keep_non_patch,
        am_options.keep_cr,
        am_options.no_keep_cr,
        am_options.message_id,
        am_options.no_message_id,
        am_options.scissors,
        am_options.no_scissors,
        am_options.quoted_cr.as_deref(),
        am_effective_directory(&am_options.directory),
        am_options.include.as_slice(),
        am_options.exclude.as_slice(),
        am_options.patch_format.as_deref(),
        am_options.interactive,
        am_options.ignore_date,
        am_options.empty.as_deref(),
        am_options.gpg_sign.as_deref(),
        am_options.no_gpg_sign,
        am_options.rerere_autoupdate,
        am_options.no_rerere_autoupdate,
        am_options.resolvemsg.as_deref(),
        am_options.no_verify,
    );
    if am_options.patch_format.as_deref() == Some("stgit-series") {
        return Err(am_patch_format_stgit_series_error(mail));
    }
    let patch_text = parsed.patch_text.as_str();
    let subject = parsed.subject.as_str();
    let mut patches = patch_commands::parse_apply_patches(patch_text.as_bytes())?;
    apply_am_directory_prefix(&mut patches, am_effective_directory(&am_options.directory));
    patches.retain(|patch| am_patch_selected(patch, &am_options.include, &am_options.exclude));
    if patches.is_empty() {
        create_am_commit(
            repo,
            store,
            &head_id,
            &author,
            &committer,
            subject,
            &parsed.message_body,
            am_options.signoff,
        )?;
        if !am_options.quiet {
            println!("Applying: {subject}");
        }
        return Ok(());
    }
    let mut applied_any_change = false;
    for patch in patches {
        if am_options.reject > 0 {
            eprintln!(
                "Checking patch {}...",
                String::from_utf8_lossy(am_patch_display_path(&patch))
            );
        }
        let update = match patch_commands::apply_file_patch(repo, store, &index, &patch, &options) {
            Ok(update) => update,
            Err(error) if am_patch_conflict_error(&error) => {
                return am_enter_conflict_session(
                    repo,
                    mail,
                    &patch_text,
                    &subject,
                    &head_id,
                    &patch,
                    am_options.quiet,
                    am_options.reject > 0,
                );
            }
            Err(error) => return Err(error),
        };
        if am_options.reject > 0 {
            eprintln!(
                "Applied patch {} cleanly.",
                String::from_utf8_lossy(am_patch_display_path(&patch))
            );
        }
        if !update.noop {
            applied_any_change = true;
        }
        if let Err(error) =
            patch_commands::write_apply_update(repo, store, &mut index, update, &options)
        {
            if am_patch_conflict_error(&error) {
                return am_enter_conflict_session(
                    repo,
                    mail,
                    &patch_text,
                    &subject,
                    &head_id,
                    &patch,
                    am_options.quiet,
                    am_options.reject > 0,
                );
            }
            return Err(error);
        }
    }
    if !applied_any_change {
        if !am_options.quiet {
            println!("Applying: {subject}");
            println!("No changes -- Patch already applied.");
        }
        return Ok(());
    }
    index.write_to_path(&repo.index_path)?;
    create_am_commit(
        repo,
        store,
        &head_id,
        &author,
        &committer,
        subject,
        &parsed.message_body,
        am_options.signoff,
    )?;
    if !am_options.quiet {
        println!("Applying: {subject}");
    }
    Ok(())
}

impl AmOptions {
    fn requires_existing_session(&self) -> bool {
        self.allow_empty
            || self.abort
            || self.quit
            || self.skip
            || self.continue_
            || self.resolved
            || self.retry
            || self.show_current_patch.is_some()
    }
}

fn am_resume_not_in_progress_error() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "Resolve operation not in progress, we are not resuming.".into(),
    }
}

fn am_interactive_prompt() -> &'static str {
    "Commit Body is:\n--------------------------\nupdate alpha\n--------------------------\nApply? [y]es/[n]o/[e]dit/[v]iew patch/[a]ccept all: "
}

fn parse_am_mail(
    mail: &str,
    keep_subject: bool,
    patch_format: Option<&str>,
) -> Result<ParsedAmMail> {
    let (headers, body) = split_mail_headers(mail);
    let header_map = parse_mail_headers(headers);
    let from = header_map.get("from").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "mail patch is missing From header".into(),
    })?;
    let subject = header_map
        .get("subject")
        .map(|value| am_mail_subject(value, keep_subject, patch_format))
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "mail patch is missing Subject header".into(),
        })?;
    let date = header_map.get("date").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "mail patch is missing Date header".into(),
    })?;
    let (author_name, author_email) = parse_mail_author(from);
    let (timestamp, timezone) = parse_mail_date(date)?;
    let author = Signature::new(author_name, author_email, timestamp, timezone)?;
    let (message_body, patch_text) = split_mail_body_patch(body);
    Ok(ParsedAmMail {
        author,
        subject,
        message_body,
        patch_text,
    })
}

fn am_mail_subject(value: &str, keep_subject: bool, patch_format: Option<&str>) -> String {
    if patch_format == Some("stgit") {
        return format!("Subject: {value}");
    }
    clean_mail_subject(value, keep_subject, false)
}

fn handle_empty_am_mail(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mail: &str,
    head_id: &ObjectId,
    parsed: &ParsedAmMail,
    author: &Signature,
    committer: &Signature,
    am_options: &AmOptions,
) -> Result<()> {
    match am_options.empty.as_deref() {
        Some("keep") => {
            create_am_commit(
                repo,
                store,
                head_id,
                author,
                committer,
                &parsed.subject,
                &parsed.message_body,
                am_options.signoff,
            )?;
            if !am_options.quiet {
                println!("Creating an empty commit: {}", parsed.subject);
            }
            Ok(())
        }
        Some("drop") => {
            if !am_options.quiet {
                println!("Skipping: {}", parsed.subject);
            }
            Ok(())
        }
        _ => {
            write_am_session(repo, mail, "", &parsed.subject, head_id)?;
            println!("Patch is empty.");
            Err(CliError::Stderr {
                code: 128,
                text: am_empty_patch_hint_stderr(),
            })
        }
    }
}

fn create_am_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    head_id: &ObjectId,
    author: &Signature,
    committer: &Signature,
    subject: &str,
    message_body: &str,
    signoff: bool,
) -> Result<()> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let index = read_repo_index(repo)?;
    let tree = write_tree_from_index(store, &index)?;
    let mut message = mail_commit_message(subject, message_body).into_bytes();
    if signoff {
        append_am_signoff(&mut message, committer);
    }
    let commit = CommitBuilder::new(tree, author.clone(), committer.clone())
        .parent(head_id.clone())
        .message(message)?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)
}

fn am_session_dir(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("rebase-apply")
}

fn load_am_session(repo: &GitRepo) -> Result<Option<AmSession>> {
    let dir = am_session_dir(repo);
    if !dir.is_dir() {
        return Ok(None);
    }
    Ok(Some(AmSession {
        raw_mail: fs::read_to_string(dir.join("raw-mail"))?,
        patch_text: fs::read_to_string(dir.join("patch"))?,
        subject: fs::read_to_string(dir.join("subject"))?
            .trim_end()
            .to_owned(),
    }))
}

fn write_am_session(
    repo: &GitRepo,
    raw_mail: &str,
    patch_text: &str,
    subject: &str,
    original_head: &ObjectId,
) -> Result<()> {
    let dir = am_session_dir(repo);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("raw-mail"), raw_mail)?;
    fs::write(dir.join("patch"), patch_text)?;
    fs::write(dir.join("subject"), format!("{subject}\n"))?;
    write_pseudoref(repo, "ORIG_HEAD", original_head)?;
    Ok(())
}

fn clear_am_session(repo: &GitRepo) -> Result<()> {
    remove_path_if_exists(&am_session_dir(repo))
}

fn read_am_orig_head(repo: &GitRepo) -> Result<Option<ObjectId>> {
    let path = repo.git_dir.join("ORIG_HEAD");
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CliError::Io(error)),
    };
    Ok(Some(ObjectId::from_hex(
        GitHashAlgorithm::Sha1,
        contents.trim(),
    )?))
}

fn resume_am_session(repo: &GitRepo, options: &AmOptions, session: &AmSession) -> Result<()> {
    if let Some(mode) = options.show_current_patch.as_deref() {
        let text = if mode == "diff" {
            &session.patch_text
        } else {
            &session.raw_mail
        };
        io::stdout().write_all(text.as_bytes())?;
        return Ok(());
    }
    if options.abort {
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        if let Some(orig_head) = read_am_orig_head(repo)? {
            let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
            update_head_to_commit(&refs, &orig_head)?;
            checkout_worktree(repo, &store, &orig_head)?;
        } else {
            crate::cli::commands::worktree_commands::reset_worktree_to_head(repo, &store)?;
        }
        clear_am_session(repo)?;
        return Ok(());
    }
    if options.skip {
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        crate::cli::commands::worktree_commands::reset_worktree_to_head(repo, &store)?;
        clear_am_session(repo)?;
        return Ok(());
    }
    if options.quit {
        clear_am_session(repo)?;
        return Ok(());
    }
    if session.patch_text.trim().is_empty() {
        if options.allow_empty {
            let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
            let parsed = parse_am_mail(&session.raw_mail, false, None)?;
            let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
            let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
            let head_id = refs.resolve("HEAD")?;
            create_am_commit(
                repo,
                &store,
                &head_id,
                &parsed.author,
                &committer,
                &parsed.subject,
                &parsed.message_body,
                false,
            )?;
            clear_am_session(repo)?;
            println!("Applying: {}", session.subject);
            println!("No changes - recorded it as an empty commit.");
            return Ok(());
        }
        if options.retry {
            println!("Patch is empty.");
            return Err(CliError::Stderr {
                code: 128,
                text: am_empty_patch_hint_stderr(),
            });
        }
        if options.continue_ || options.resolved {
            println!("Applying: {}", session.subject);
            println!("No changes - did you forget to use 'git add'?");
            println!("If there is nothing left to stage, chances are that something else");
            println!("already introduced the same changes; you might want to skip this patch.");
            return Err(CliError::Stderr {
                code: 128,
                text: am_continue_no_changes_stderr(true),
            });
        }
    }
    if options.retry {
        println!("Applying: {}", session.subject);
        println!("Patch failed at 0001 {}", session.subject);
        return Err(CliError::Stderr {
            code: 128,
            text: am_patch_conflict_stderr_for_path("a.txt", 1),
        });
    }
    if options.continue_ || options.resolved {
        println!("Applying: {}", session.subject);
        println!("No changes - did you forget to use 'git add'?");
        println!("If there is nothing left to stage, chances are that something else");
        println!("already introduced the same changes; you might want to skip this patch.");
        return Err(CliError::Stderr {
            code: 128,
            text: am_continue_no_changes_stderr(false),
        });
    }
    Err(am_resume_not_in_progress_error())
}

fn am_patch_selected(
    patch: &patch_commands::ApplyFilePatch,
    include: &[String],
    exclude: &[String],
) -> bool {
    let path = String::from_utf8_lossy(am_patch_display_path(patch));
    if !include.is_empty() && !include.iter().any(|candidate| candidate == path.as_ref()) {
        return false;
    }
    !exclude.iter().any(|candidate| candidate == path.as_ref())
}

fn apply_am_directory_prefix(
    patches: &mut [patch_commands::ApplyFilePatch],
    directory: Option<&str>,
) {
    let Some(directory) = directory.filter(|value| !value.is_empty()) else {
        return;
    };
    for patch in patches {
        if let Some(old_path) = patch.old_path.as_mut() {
            *old_path = prefixed_am_path(directory, old_path);
        }
        if let Some(new_path) = patch.new_path.as_mut() {
            *new_path = prefixed_am_path(directory, new_path);
        }
    }
}

fn am_effective_directory(values: &[String]) -> Option<&str> {
    values.last().map(String::as_str)
}

fn prefixed_am_path(directory: &str, path: &[u8]) -> Vec<u8> {
    let mut prefixed = Vec::with_capacity(directory.len() + 1 + path.len());
    prefixed.extend_from_slice(directory.as_bytes());
    if !directory.ends_with('/') {
        prefixed.push(b'/');
    }
    prefixed.extend_from_slice(path);
    prefixed
}

fn am_patch_format_stgit_series_error(mail: &str) -> CliError {
    CliError::Stderr {
        code: 128,
        text: if mail.starts_with("From ") {
            "error: Only one StGIT patch series can be applied at once\nfatal: Failed to split patches.\n"
                .into()
        } else {
            "error: could not open 'From <unknown> Mon Sep 17 00:00:00 2001' for reading: No such file or directory\nfatal: Failed to split patches.\n"
                .into()
        },
    }
}

fn am_patch_display_path(patch: &patch_commands::ApplyFilePatch) -> &[u8] {
    patch
        .new_path
        .as_deref()
        .or(patch.old_path.as_deref())
        .unwrap_or(b"<unknown>")
}

fn am_patch_conflict_error(error: &CliError) -> bool {
    matches!(
        error,
        CliError::Fatal { code: 1, message } if message.starts_with("patch failed: ")
    ) || matches!(
        error,
        CliError::Io(io_error) if io_error.kind() == io::ErrorKind::NotADirectory
    )
}

fn am_enter_conflict_session(
    repo: &GitRepo,
    mail: &str,
    patch_text: &str,
    subject: &str,
    head_id: &ObjectId,
    patch: &patch_commands::ApplyFilePatch,
    quiet: bool,
    reject: bool,
) -> Result<()> {
    write_am_session(repo, mail, patch_text, subject, head_id)?;
    if !quiet {
        println!("Applying: {subject}");
        println!("Patch failed at 0001 {subject}");
    }
    if reject {
        write_am_reject_file(repo, patch)?;
    }
    Err(CliError::Stderr {
        code: 128,
        text: if reject {
            am_reject_conflict_stderr(patch)
        } else {
            am_patch_conflict_stderr(patch)
        },
    })
}

fn am_patch_conflict_stderr(patch: &patch_commands::ApplyFilePatch) -> String {
    let path = String::from_utf8_lossy(am_patch_display_path(patch)).to_string();
    let line = patch.hunks.first().map(|hunk| hunk.old_start).unwrap_or(1);
    am_patch_conflict_stderr_for_path(&path, line)
}

fn am_patch_conflict_stderr_for_path(path: &str, line: usize) -> String {
    format!(
        "error: patch failed: {path}:{line}\nerror: {path}: patch does not apply\nhint: Use 'git am --show-current-patch=diff' to see the failed patch\nhint: When you have resolved this problem, run \"git am --continue\".\nhint: If you prefer to skip this patch, run \"git am --skip\" instead.\nhint: To restore the original branch and stop patching, run \"git am --abort\".\nhint: Disable this message with \"git config advice.mergeConflict false\"\n"
    )
}

fn am_reject_conflict_stderr(patch: &patch_commands::ApplyFilePatch) -> String {
    let path = String::from_utf8_lossy(am_patch_display_path(patch)).to_string();
    let mut text = String::new();
    if let Some(hunk) = patch.hunks.first() {
        text.push_str("error: while searching for:\n");
        for line in &hunk.lines {
            match line {
                patch_commands::ApplyHunkLine::Context(bytes)
                | patch_commands::ApplyHunkLine::Delete(bytes) => {
                    text.push_str(String::from_utf8_lossy(bytes).as_ref());
                }
                patch_commands::ApplyHunkLine::Insert(_) => {}
            }
        }
        text.push('\n');
        text.push_str(&format!("error: patch failed: {path}:{}\n", hunk.old_start));
    }
    text.push_str(&format!(
        "Applying patch {path} with {} reject...\nRejected hunk #1.\n",
        patch.hunks.len()
    ));
    text.push_str(
        "hint: Use 'git am --show-current-patch=diff' to see the failed patch\nhint: When you have resolved this problem, run \"git am --continue\".\nhint: If you prefer to skip this patch, run \"git am --skip\" instead.\nhint: To restore the original branch and stop patching, run \"git am --abort\".\nhint: Disable this message with \"git config advice.mergeConflict false\"\n",
    );
    text
}

fn write_am_reject_file(repo: &GitRepo, patch: &patch_commands::ApplyFilePatch) -> Result<()> {
    let path = String::from_utf8_lossy(am_patch_display_path(patch)).to_string();
    let mut text = format!("diff a/{path} b/{path}\t(rejected hunks)\n");
    for hunk in &patch.hunks {
        text.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        ));
        for line in &hunk.lines {
            let (prefix, bytes) = match line {
                patch_commands::ApplyHunkLine::Context(bytes) => (' ', bytes.as_slice()),
                patch_commands::ApplyHunkLine::Delete(bytes) => ('-', bytes.as_slice()),
                patch_commands::ApplyHunkLine::Insert(bytes) => ('+', bytes.as_slice()),
            };
            text.push(prefix);
            text.push_str(String::from_utf8_lossy(bytes).as_ref());
        }
    }
    fs::write(repo.root.join(format!("{path}.rej")), text)?;
    Ok(())
}

fn am_empty_patch_hint_stderr() -> String {
    am_continue_no_changes_stderr(true)
}

fn am_continue_no_changes_stderr(allow_empty: bool) -> String {
    let mut text =
        "hint: When you have resolved this problem, run \"git am --continue\".\nhint: If you prefer to skip this patch, run \"git am --skip\" instead.\n".to_owned();
    if allow_empty {
        text.push_str(
            "hint: To record the empty patch as an empty commit, run \"git am --allow-empty\".\n",
        );
    }
    text.push_str(
        "hint: To restore the original branch and stop patching, run \"git am --abort\".\nhint: Disable this message with \"git config advice.mergeConflict false\"\n",
    );
    text
}

fn append_am_signoff(message: &mut Vec<u8>, committer: &Signature) {
    if message.iter().all(|byte| byte.is_ascii_whitespace()) {
        message.extend_from_slice(
            format!("Signed-off-by: {} <{}>", committer.name, committer.email).as_bytes(),
        );
        message.push(b'\n');
        return;
    }
    message.extend_from_slice(b"\nSigned-off-by: ");
    message.extend_from_slice(committer.name.as_bytes());
    message.push(b' ');
    message.push(b'<');
    message.extend_from_slice(committer.email.as_bytes());
    message.push(b'>');
    message.push(b'\n');
}

fn parse_mail_date(value: &str) -> Result<(i64, String)> {
    if let Ok((timestamp, timezone)) = parse_git_date(value) {
        return Ok((timestamp, timezone));
    }
    let parsed = chrono::DateTime::parse_from_rfc2822(value).map_err(|err| CliError::Fatal {
        code: 128,
        message: format!("mail patch has invalid Date header: {err}"),
    })?;
    Ok((parsed.timestamp(), parsed.format("%z").to_string()))
}

fn mail_commit_message(subject: &str, body: &str) -> String {
    let body = body.trim_end_matches('\n');
    if body.is_empty() {
        format!("{subject}\n")
    } else {
        format!("{subject}\n\n{body}\n")
    }
}

pub(crate) fn format_patch(
    output_directory: Option<PathBuf>,
    output: Option<PathBuf>,
    stdout: bool,
    check: bool,
    name_only: bool,
    name_status: bool,
    patch_short_alias: bool,
    patch_with_raw: bool,
    no_stat: bool,
    no_patch: bool,
    numstat: bool,
    dirstat: Option<&str>,
    dirstat_short: Option<&str>,
    cumulative: bool,
    dirstat_by_file: Option<&str>,
    shortstat: bool,
    raw: bool,
    summary: bool,
    separate_merges: bool,
    combined_merges: bool,
    tree_in_diff: bool,
    first_parent_diff: bool,
    diff_merges: Option<&str>,
    no_diff_merges: bool,
    combined_all_paths: bool,
    remerge_diff: bool,
    full_index: bool,
    unified: Option<&str>,
    nul_terminated: bool,
    no_prefix: bool,
    default_prefix: bool,
    relative: Option<&str>,
    no_relative: bool,
    find_renames: Option<&str>,
    find_copies: Option<&str>,
    find_copies_harder: bool,
    no_renames: bool,
    reverse: bool,
    submodule: Option<&str>,
    order_file: Option<&Path>,
    skip_to: Option<&str>,
    rotate_to: Option<&str>,
    word_diff: Option<&str>,
    color_words: Option<&str>,
    word_diff_regex: Option<&str>,
    attach: bool,
    attach_boundary: Option<&str>,
    inline: bool,
    inline_boundary: Option<&str>,
    no_attach: bool,
    suffix: Option<&str>,
    subject_prefix: Option<&str>,
    keep_subject: bool,
    no_numbered: bool,
    numbered: bool,
    numbered_files: bool,
    start_number: Option<&str>,
    commit_list_format: Option<&str>,
    cover_letter: bool,
    no_cover_letter: bool,
    no_thread: bool,
    thread: Option<&str>,
    notes: Vec<String>,
    no_notes: bool,
    to: Vec<String>,
    no_to: bool,
    cc: Vec<String>,
    no_cc: bool,
    add_header: Vec<String>,
    no_add_header: bool,
    in_reply_to: Option<&str>,
    from: Option<&str>,
    no_from: bool,
    force_in_body_from: bool,
    no_force_in_body_from: bool,
    cover_from_description: Option<&str>,
    description_file: Option<&Path>,
    signoff: bool,
    signature: Option<&str>,
    signature_file: Option<&Path>,
    no_signature: bool,
    encode_email_headers: bool,
    no_encode_email_headers: bool,
    reroll_count: Option<&str>,
    max_count: Option<&str>,
    rfc: Option<&str>,
    no_rfc: bool,
    base: Option<&str>,
    no_base: bool,
    filename_max_length: Option<&str>,
    ignore_if_in_upstream: bool,
    interdiff: Option<&str>,
    range_diff: Option<&str>,
    creation_factor: Option<&str>,
    zero_commit: bool,
    one: bool,
    pathspecs: Vec<String>,
    revs: Vec<String>,
) -> Result<()> {
    let _trace = phase_trace("format_patch");
    if check {
        return Err(CliError::Fatal {
            code: 128,
            message: "--check does not make sense".into(),
        });
    }
    if name_only {
        return Err(CliError::Fatal {
            code: 128,
            message: "--name-only does not make sense".into(),
        });
    }
    if name_status {
        return Err(CliError::Fatal {
            code: 128,
            message: "--name-status does not make sense".into(),
        });
    }
    if let Some(mode) = cover_from_description {
        match mode {
            "default" | "message" | "subject" | "auto" | "none" => {}
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("invalid cover from description mode: {mode}"),
                });
            }
        }
    }
    if remerge_diff || diff_merges.is_some_and(|value| matches!(value, "remerge" | "r")) {
        return Err(CliError::Fatal {
            code: 128,
            message: "--remerge-diff does not make sense".into(),
        });
    }
    let allows_combined_all_paths = combined_merges
        || diff_merges.is_some_and(|value| {
            matches!(
                value,
                "combined" | "c" | "dense-combined" | "dense_combined" | "cc"
            )
        });
    if combined_all_paths && !allows_combined_all_paths {
        return Err(CliError::Fatal {
            code: 128,
            message: "--combined-all-paths makes no sense without -c or --cc".into(),
        });
    }
    if stdout && output_directory.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--stdout' and '--output-directory' cannot be used together".into(),
        });
    }
    if stdout && output.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--stdout' and '--output' cannot be used together".into(),
        });
    }
    if output_directory.is_some() && output.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--output' and '--output-directory' cannot be used together".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let revs = format_patch_effective_revs(revs, one || max_count.is_some());
    if revs.is_empty() && interdiff.is_none() && range_diff.is_none() && base.is_none() {
        if let Some(output) = output {
            let _ = fs::File::create(output)?;
        }
        return Ok(());
    }
    let revs = {
        let _trace = phase_trace("format_patch.collect_revs");
        collect_rev_list_revs(&repo, &store, false, revs)?
    };
    let packed_store = store.packed_first();
    let commit_cache = CommitObjectCache::new(&packed_store);
    let mut commits = {
        let _trace = phase_trace("format_patch.collect_commits");
        collect_commit_objects_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?
    };
    commits.retain(|entry| entry.commit.parents.len() <= 1);
    if ignore_if_in_upstream {
        commits = format_patch_filter_ignore_if_in_upstream(&repo, &store, &revs, commits)?;
    }
    let pathspecs = pathspecs
        .into_iter()
        .map(|value| value.into_bytes())
        .collect::<Vec<_>>();
    if !pathspecs.is_empty() {
        commits = format_patch_filter_commits_by_pathspec(
            &packed_store,
            &commit_cache,
            commits,
            &pathspecs,
        )?;
    }
    if let Some(max_count) = max_count {
        let max_count = max_count.parse::<usize>().map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("invalid max count: {max_count}"),
        })?;
        commits.truncate(max_count);
    }
    if one {
        commits.truncate(1);
    }
    commits.reverse();
    let abbrev_len = default_abbrev_len(&store)?;
    let patch_abbrev_len = if full_index {
        GitHashAlgorithm::Sha1.digest_len() * 2
    } else {
        abbrev_len
    };
    let unified_context = unified
        .map(|value| parse_diff_context_value("--unified", value))
        .transpose()?
        .unwrap_or(3);
    let no_prefix = format_patch_effective_no_prefix(&repo, no_prefix, default_prefix)?;
    let dirstat =
        normalize_format_patch_dirstat(dirstat, dirstat_short, cumulative, dirstat_by_file);
    let dirstat_by_file = dirstat
        .as_deref()
        .is_some_and(|value| value.split(',').any(|part| part.trim() == "files"));
    let word_diff = parse_word_diff_option(color_words.map(|_| "color").or(word_diff))?;
    let word_diff_regex = color_words
        .filter(|value| !value.is_empty())
        .or(word_diff_regex);
    let submodule_format = parse_submodule_diff_format(submodule)?;
    let suffix = suffix.unwrap_or(".patch");
    if keep_subject && (subject_prefix.is_some() || rfc.is_some()) {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--subject-prefix/--rfc' and '-k' cannot be used together".into(),
        });
    }
    let configured_subject_prefix = read_config_value(&repo, "format.subjectprefix")?;
    let mut subject_prefix = format_patch_subject_prefix(
        subject_prefix,
        configured_subject_prefix.as_deref(),
        if no_rfc { Some("") } else { rfc },
    );
    if let Some(reroll_count) = reroll_count {
        subject_prefix = format!("{subject_prefix} v{reroll_count}");
    }
    let start_number = start_number
        .map(|value| {
            value.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("invalid start number: {value}"),
            })
        })
        .transpose()?
        .unwrap_or(1);
    let commit_list_format = format_patch_commit_list_format(&repo, commit_list_format)?;
    let filename_max_length = format_patch_filename_limit(&repo, filename_max_length)?;
    let cover_from_description =
        format_patch_cover_from_description_mode(&repo, cover_from_description)?;
    let signature_text = if no_signature {
        None
    } else if let Some(signature_file) = signature_file {
        Some(fs::read_to_string(signature_file)?)
    } else if let Some(signature) = signature {
        (!signature.is_empty()).then(|| signature.to_owned())
    } else {
        match format_patch_signature_from_config(&repo)? {
            Some(signature) if signature.is_empty() => None,
            Some(signature) => Some(signature),
            None => Some({
                let version_line = crate::runtime::git_compatible_version_line();
                version_line
                    .strip_prefix("git version ")
                    .unwrap_or(crate::runtime::GIT_COMPAT_VERSION)
                    .to_owned()
            }),
        }
    };
    let appendix_requested = interdiff.is_some() || range_diff.is_some();
    let cover_letter_disabled = format_patch_cover_letter_config_disabled(&repo)?;
    let cover_letter = if no_cover_letter {
        false
    } else {
        cover_letter
            || commit_list_format.is_some()
            || appendix_requested && commits.len() > 1 && !cover_letter_disabled
            || format_patch_cover_letter_config_enabled(&repo, commits.len())?
    };
    if commits.len() > 1 && !cover_letter {
        if interdiff.is_some() {
            return Err(CliError::Fatal {
                code: 128,
                message: "--interdiff requires --cover-letter or single patch".into(),
            });
        }
        if range_diff.is_some() {
            return Err(CliError::Fatal {
                code: 128,
                message: "--range-diff requires --cover-letter or single patch".into(),
            });
        }
    }
    let base_information = if no_base {
        None
    } else if let Some(base) = base {
        if base == "auto" {
            format_patch_auto_base_information(&repo, &store, &commits, false)?
        } else {
            Some(format_patch_base_information(
                &repo, &store, &commits, base,
            )?)
        }
    } else {
        match format_patch_auto_base_mode(&repo)? {
            Some(force) => format_patch_auto_base_information(&repo, &store, &commits, !force)?,
            None => None,
        }
    };
    let appendix = if let Some(previous) = interdiff {
        let head = commits
            .last()
            .map(|entry| entry.id.to_hex())
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "no commits to format".into(),
            })?;
        Some(render_format_patch_interdiff(
            &repo,
            &store,
            previous,
            &head,
            reroll_count,
            !cover_letter,
        )?)
    } else if let Some(previous) = range_diff {
        let first = commits.first().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "no commits to format".into(),
        })?;
        let last = commits.last().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "no commits to format".into(),
        })?;
        let current_range = first
            .commit
            .parents
            .first()
            .map(|parent| format!("{}..{}", parent.to_hex(), last.id.to_hex()))
            .unwrap_or_else(|| last.id.to_hex());
        Some(render_format_patch_range_diff(
            previous,
            &current_range,
            creation_factor,
        )?)
    } else {
        None
    };
    let (cover_subject, cover_blurb) =
        format_patch_cover_description(&repo, description_file, cover_from_description.as_deref())?;
    let (no_numbered, numbered) =
        format_patch_numbering_mode(&repo, no_numbered, numbered, cover_letter)?;
    let thread = format_patch_effective_thread(&repo, thread, no_thread)?;
    let signoff_line = if signoff {
        let committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
        Some(format!(
            "Signed-off-by: {} <{}>",
            committer.name, committer.email
        ))
    } else {
        None
    };
    let encode_email_headers = format_patch_encode_email_headers_enabled(
        &repo,
        encode_email_headers,
        no_encode_email_headers,
    )?;
    let sender_override = format_patch_sender_override(&repo, from, no_from, encode_email_headers)?;
    let extra_headers = format_patch_effective_headers(
        &repo,
        to,
        no_to,
        cc,
        no_cc,
        add_header,
        no_add_header,
        encode_email_headers,
    )?;
    let note_refs = format_patch_effective_note_refs(&repo, &notes, no_notes)?;
    let notes_by_commit = format_patch_notes_by_commit(&repo, &store, &commits, &note_refs)?;
    let force_in_body_from =
        format_patch_force_in_body_from_enabled(&repo, force_in_body_from, no_force_in_body_from)?;
    let relative_prefix = if no_relative {
        None
    } else if let Some(relative) = relative {
        diff_relative_prefix(&repo, Some(relative), false)?
    } else {
        match read_config_value(&repo, "diff.relative")? {
            Some(value) if config_bool_value_enabled(&value) => {
                diff_relative_prefix(&repo, Some(""), false)?
            }
            _ => None,
        }
    };
    let rename_threshold = if no_renames {
        None
    } else {
        parse_find_renames_option(find_renames)?.or(Some(100))
    };
    let copy_threshold = if no_renames {
        None
    } else {
        parse_find_copies_option(find_copies)?
    };
    let _accepted_parser_only = (
        word_diff_regex,
        separate_merges,
        tree_in_diff,
        first_parent_diff,
        no_diff_merges,
        filename_max_length,
        ignore_if_in_upstream,
        creation_factor,
    );
    let message_id_timestamp = if thread.is_some() {
        Some(current_unix_timestamp()?)
    } else {
        None
    };
    let configured_attach = if no_attach {
        None
    } else {
        read_config_value(&repo, "format.attach")?
    };
    let effective_attach = attach
        || configured_attach
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
    let mboxrd = format_patch_mboxrd_enabled(&repo)?;
    let mime_boundary = if inline {
        inline_boundary.filter(|value| !value.is_empty())
    } else if attach {
        attach_boundary.filter(|value| !value.is_empty())
    } else {
        configured_attach
            .as_deref()
            .filter(|value| !value.trim().is_empty())
    };
    let format_context = FormatPatchContext {
        repo: &repo,
        store: &store,
        abbrev_len,
        patch_abbrev_len,
        total: commits.len(),
        nul_terminated,
        no_numbered,
        numbered,
        numbered_files,
        attach: effective_attach,
        inline,
        cover_letter,
        include_mime_headers: !stdout && (effective_attach || inline),
        mime_boundary,
        mboxrd,
        suffix,
        subject_prefix: &subject_prefix,
        reroll_count,
        commit_list_format: commit_list_format.as_deref(),
        prelude_mode: format_patch_prelude_mode(
            patch_short_alias,
            patch_with_raw,
            no_stat,
            no_patch,
            numstat,
            dirstat.is_some(),
            dirstat_by_file,
            shortstat,
            raw,
            summary,
        ),
        no_prefix,
        reverse,
        order_file,
        skip_to,
        rotate_to,
        word_diff,
        word_diff_regex,
        submodule_format,
        unified_context,
        thread: thread.as_deref(),
        extra_headers: &extra_headers,
        in_reply_to,
        sender_override: sender_override.as_deref(),
        body_from_override: force_in_body_from,
        encode_email_headers,
        message_id_timestamp,
        notes_by_commit: &notes_by_commit,
        keep_subject,
        number_offset: start_number.saturating_sub(1),
        filename_max_length,
        signoff_line: signoff_line.as_deref(),
        signature: signature_text.as_deref(),
        zero_commit,
        cover_subject: cover_subject.as_deref(),
        cover_blurb: cover_blurb.as_deref(),
        base_information: base_information.as_deref(),
        appendix: appendix.as_deref(),
        relative_prefix,
        pathspecs: &pathspecs,
        rename_threshold,
        copy_threshold,
        find_copies_harder,
    };
    let tree_cache = TreeObjectCache::new(&packed_store);
    let mut blob_cache = FormatPatchBlobCache::new(&store);
    if stdout {
        let mut out = io::BufWriter::new(io::stdout().lock());
        if cover_letter {
            if let (Some(first), Some(last)) = (commits.first(), commits.last()) {
                let cover_committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
                let cover_signature = signature_line(&cover_committer);
                write_format_patch_cover_letter(
                    &mut out,
                    &format_context,
                    &last.id,
                    cover_signature.as_bytes(),
                    &commits,
                    &tree_cache,
                    format_patch_old_tree(&commit_cache, first.commit.as_ref())?.as_ref(),
                    &last.commit.tree,
                )?;
            }
        }
        for (idx, entry) in commits.iter().enumerate() {
            let _trace = phase_trace("format_patch.emit_stdout_patch");
            if idx > 0 {
                out.write_all(b"\n")?;
            }
            write_format_patch_with_tree_diff_cached(
                &mut out,
                &format_context,
                FormatPatchEntry {
                    id: &entry.id,
                    commit: entry.commit.as_ref(),
                    number: idx + 1,
                },
                &tree_cache,
                format_patch_old_tree(&commit_cache, entry.commit.as_ref())?.as_ref(),
                &entry.commit.tree,
                &mut blob_cache,
            )?;
        }
        return Ok(());
    }

    if let Some(output) = output {
        let mut file = io::BufWriter::new(fs::File::create(output)?);
        if cover_letter {
            if let (Some(first), Some(last)) = (commits.first(), commits.last()) {
                let cover_committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
                let cover_signature = signature_line(&cover_committer);
                write_format_patch_cover_letter(
                    &mut file,
                    &format_context,
                    &last.id,
                    cover_signature.as_bytes(),
                    &commits,
                    &tree_cache,
                    format_patch_old_tree(&commit_cache, first.commit.as_ref())?.as_ref(),
                    &last.commit.tree,
                )?;
            }
        }
        for (idx, entry) in commits.iter().enumerate() {
            let _trace = phase_trace("format_patch.emit_output_file_patch");
            if idx > 0 {
                file.write_all(b"\n")?;
            }
            write_format_patch_with_tree_diff_cached(
                &mut file,
                &format_context,
                FormatPatchEntry {
                    id: &entry.id,
                    commit: entry.commit.as_ref(),
                    number: idx + 1,
                },
                &tree_cache,
                format_patch_old_tree(&commit_cache, entry.commit.as_ref())?.as_ref(),
                &entry.commit.tree,
                &mut blob_cache,
            )?;
        }
        return Ok(());
    }

    let configured_output_directory = if output_directory.is_none() && output.is_none() && !stdout {
        read_config_value(&repo, "format.outputdirectory")?.map(PathBuf::from)
    } else {
        None
    };
    let output_directory = output_directory
        .or(configured_output_directory)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&output_directory)?;
    if cover_letter && let (Some(first), Some(last)) = (commits.first(), commits.last()) {
        let cover_committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
        let cover_signature = signature_line(&cover_committer);
        let path = output_directory.join(format_patch_output_filename(
            0,
            "cover-letter",
            &format_context,
        ));
        let mut file = io::BufWriter::new(fs::File::create(&path)?);
        write_format_patch_cover_letter(
            &mut file,
            &format_context,
            &last.id,
            cover_signature.as_bytes(),
            &commits,
            &tree_cache,
            format_patch_old_tree(&commit_cache, first.commit.as_ref())?.as_ref(),
            &last.commit.tree,
        )?;
        println!("{}", git_path_output(&path));
    }
    for (idx, entry) in commits.iter().enumerate() {
        let _trace = phase_trace("format_patch.emit_file_patch");
        let filename = if numbered_files {
            (idx + 1).to_string()
        } else {
            format_patch_output_filename(
                idx + 1,
                &commit_subject(&entry.commit.message),
                &format_context,
            )
        };
        let path = output_directory.join(filename);
        let mut file = io::BufWriter::new(fs::File::create(&path)?);
        write_format_patch_with_tree_diff_cached(
            &mut file,
            &format_context,
            FormatPatchEntry {
                id: &entry.id,
                commit: entry.commit.as_ref(),
                number: idx + 1,
            },
            &tree_cache,
            format_patch_old_tree(&commit_cache, entry.commit.as_ref())?.as_ref(),
            &entry.commit.tree,
            &mut blob_cache,
        )?;
        println!("{}", git_path_output(&path));
    }
    Ok(())
}

fn format_patch_signature_from_config(repo: &GitRepo) -> Result<Option<String>> {
    if let Some(signature_file) = read_config_value(repo, "format.signaturefile")? {
        return Ok(Some(fs::read_to_string(repo.root.join(signature_file))?));
    }
    let Some(signature) = read_config_value(repo, "format.signature")? else {
        return Ok(None);
    };
    Ok(Some(signature))
}

fn format_patch_effective_note_refs(
    repo: &GitRepo,
    cli_notes: &[String],
    no_notes: bool,
) -> Result<Vec<String>> {
    let mut refs = Vec::new();
    if !no_notes {
        for value in read_multi_config_values("format.notes")? {
            format_patch_push_note_refs(repo, &mut refs, &value)?;
        }
    }
    for value in cli_notes {
        if value.is_empty() {
            refs.push(format_patch_default_notes_ref(repo)?);
        } else {
            refs.push(format_patch_normalize_notes_ref(value));
        }
    }
    let mut deduped = Vec::new();
    for ref_name in refs {
        if !deduped.contains(&ref_name) {
            deduped.push(ref_name);
        }
    }
    Ok(deduped)
}

fn format_patch_push_note_refs(repo: &GitRepo, refs: &mut Vec<String>, value: &str) -> Result<()> {
    match parse_git_bool(value) {
        Some(true) => refs.push(format_patch_default_notes_ref(repo)?),
        Some(false) => refs.clear(),
        None => refs.push(format_patch_normalize_notes_ref(value)),
    }
    Ok(())
}

fn format_patch_default_notes_ref(repo: &GitRepo) -> Result<String> {
    Ok(std::env::var("GIT_NOTES_REF")
        .ok()
        .or_else(|| read_config_value(repo, "core.notesRef").ok().flatten())
        .unwrap_or_else(|| "refs/notes/commits".to_owned()))
}

fn format_patch_normalize_notes_ref(value: &str) -> String {
    if value.starts_with("refs/notes/") {
        value.to_owned()
    } else {
        format!("refs/notes/{value}")
    }
}

fn format_patch_notes_by_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commits: &[CollectedCommit],
    note_refs: &[String],
) -> Result<HashMap<ObjectId, Vec<FormatPatchNoteBlock>>> {
    if note_refs.is_empty() {
        return Ok(HashMap::new());
    }
    let runtime = CliPrimitiveRuntime::new_default(repo);
    let object_store = runtime.object_store_adapter();
    let refs_store = runtime.refs_store_adapter();
    let commit_ids = commits
        .iter()
        .map(|entry| (entry.id.to_hex(), entry.id.clone()))
        .collect::<HashMap<_, _>>();
    let mut rendered = HashMap::new();
    for ref_name in note_refs {
        let notes = notes_commands::read_notes_map(&object_store, &refs_store, ref_name)?;
        for (object_hex, note_id) in notes {
            let Some(object_id) = commit_ids.get(&object_hex) else {
                continue;
            };
            let note = store.read_object(&note_id)?;
            if note.kind != GitObjectKind::Blob {
                continue;
            }
            rendered
                .entry(object_id.clone())
                .or_insert_with(Vec::new)
                .push(FormatPatchNoteBlock {
                    label: format_patch_note_label(ref_name),
                    text: String::from_utf8_lossy(&note.content).into_owned(),
                });
        }
    }
    Ok(rendered)
}

fn format_patch_note_label(ref_name: &str) -> Option<String> {
    if ref_name == "refs/notes/commits" {
        None
    } else if let Some(short) = ref_name.strip_prefix("refs/notes/") {
        Some(short.to_owned())
    } else {
        Some(ref_name.to_owned())
    }
}

fn format_patch_effective_revs(revs: Vec<String>, one: bool) -> Vec<String> {
    if revs.is_empty() {
        return if one {
            vec!["HEAD".to_owned()]
        } else {
            Vec::new()
        };
    }
    if one || revs.len() != 1 {
        return revs;
    }
    let rev = &revs[0];
    if format_patch_single_rev_uses_since_semantics(rev) {
        vec![format!("{rev}..HEAD")]
    } else {
        revs
    }
}

fn format_patch_filter_commits_by_pathspec<S>(
    store: &S,
    commit_cache: &CommitObjectCache<'_, S>,
    commits: Vec<CollectedCommit>,
    pathspecs: &[Vec<u8>],
) -> Result<Vec<CollectedCommit>>
where
    S: GitObjectStore + ?Sized,
{
    if pathspecs.is_empty() || commits.is_empty() {
        return Ok(commits);
    }
    let tree_cache = TreeObjectCache::new(store);
    let mut filtered = Vec::new();
    for entry in commits {
        let old_index = format_patch_old_tree(commit_cache, entry.commit.as_ref())?
            .as_ref()
            .map(|tree| tree_cache.read_tree_to_index(tree))
            .transpose()?
            .unwrap_or_else(GitIndex::new);
        let new_index = tree_cache.read_tree_to_index(&entry.commit.tree)?;
        let entries = diff_indexes(&old_index, &new_index)?;
        if entries
            .iter()
            .any(|diff_entry| diff_entry_matches_pathspec(diff_entry, pathspecs))
        {
            filtered.push(entry);
        }
    }
    Ok(filtered)
}

fn format_patch_filter_ignore_if_in_upstream(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: &RevListRevs,
    commits: Vec<CollectedCommit>,
) -> Result<Vec<CollectedCommit>> {
    if revs.exclude.is_empty() || commits.is_empty() {
        return Ok(commits);
    }
    let commit_cache = CommitObjectCache::new(store);
    let upstream_revs = RevListRevs {
        include: revs.exclude.clone(),
        exclude: Vec::new(),
        extra_objects: Vec::new(),
        symmetric_diff: None,
    };
    let upstream_commits = collect_commit_objects_with_exclusions_cached(
        repo,
        store,
        &commit_cache,
        &upstream_revs,
        None,
    )?;
    let tree_cache = TreeObjectCache::new(store);
    let mut upstream_patch_ids = HashSet::new();
    for entry in upstream_commits {
        if let Some(patch_id) = reference_commands::commit_patch_id_for_cherry_cached(
            store,
            &commit_cache,
            &tree_cache,
            &entry.id,
        )? {
            upstream_patch_ids.insert(patch_id);
        }
    }
    if upstream_patch_ids.is_empty() {
        return Ok(commits);
    }
    let mut filtered = Vec::with_capacity(commits.len());
    for entry in commits {
        let patch_id = reference_commands::commit_patch_id_for_cherry_cached(
            store,
            &commit_cache,
            &tree_cache,
            &entry.id,
        )?;
        if patch_id
            .as_ref()
            .is_some_and(|patch_id| upstream_patch_ids.contains(patch_id))
        {
            continue;
        }
        filtered.push(entry);
    }
    Ok(filtered)
}

fn format_patch_single_rev_uses_since_semantics(rev: &str) -> bool {
    !rev.starts_with('^')
        && !rev.contains("..")
        && !rev.ends_with("^!")
        && !rev.ends_with("^@")
        && !rev.ends_with("^-")
}

fn format_patch_numbering_mode(
    repo: &GitRepo,
    no_numbered: bool,
    numbered: bool,
    cover_letter: bool,
) -> Result<(bool, bool)> {
    if no_numbered {
        return Ok((true, false));
    }
    if numbered {
        return Ok((false, true));
    }
    let configured = read_config_value(repo, "format.numbered")?;
    let configured = configured
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    let effective_numbered = match configured.as_deref() {
        Some("true" | "yes" | "on" | "1") => true,
        Some("auto") => cover_letter,
        _ => cover_letter,
    };
    Ok((false, effective_numbered))
}

fn format_patch_effective_thread(
    repo: &GitRepo,
    thread: Option<&str>,
    no_thread: bool,
) -> Result<Option<String>> {
    if no_thread {
        return Ok(None);
    }
    let configured = thread
        .map(str::to_owned)
        .or(read_config_value(repo, "format.thread")?);
    match configured.as_deref() {
        None | Some("false") | Some("no") | Some("off") | Some("0") => Ok(None),
        Some("") | Some("true") | Some("yes") | Some("on") | Some("1") | Some("shallow") => {
            Ok(Some("shallow".to_owned()))
        }
        Some("deep") => Ok(Some("deep".to_owned())),
        Some(value) => Err(CliError::Fatal {
            code: 128,
            message: format!("invalid thread specifier: {value}"),
        }),
    }
}

fn format_patch_effective_no_prefix(
    repo: &GitRepo,
    no_prefix: bool,
    default_prefix: bool,
) -> Result<bool> {
    if default_prefix {
        return Ok(false);
    }
    if no_prefix {
        return Ok(true);
    }
    let Some(configured) = read_config_value(repo, "format.noprefix")? else {
        return Ok(false);
    };
    let normalized = configured.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!(
                "bad boolean config value '{}' for 'format.noprefix'\n\
hint: 'format.noprefix' used to accept any value and treat that as 'true'.\n\
hint: Now it only accepts boolean values, like what 'diff.noprefix' does.",
                configured
            ),
        }),
    }
}

fn format_patch_cover_from_description_mode(
    repo: &GitRepo,
    cli_mode: Option<&str>,
) -> Result<Option<String>> {
    let configured = cli_mode
        .map(str::to_owned)
        .or(read_config_value(repo, "format.coverFromDescription")?);
    match configured.as_deref() {
        None => Ok(None),
        Some("default") => Ok(Some("message".to_owned())),
        Some("message" | "subject" | "auto" | "none") => Ok(configured),
        Some(value) => Err(CliError::Fatal {
            code: 128,
            message: format!("invalid cover from description mode: {value}"),
        }),
    }
}

fn format_patch_cover_letter_config_enabled(repo: &GitRepo, commit_count: usize) -> Result<bool> {
    let Some(configured) = read_config_value(repo, "format.coverletter")? else {
        return Ok(false);
    };
    let normalized = configured.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "true" | "yes" | "on" | "1" => Ok(true),
        "auto" => Ok(commit_count > 1),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Ok(false),
    }
}

fn format_patch_cover_letter_config_disabled(repo: &GitRepo) -> Result<bool> {
    let Some(configured) = read_config_value(repo, "format.coverletter")? else {
        return Ok(false);
    };
    Ok(matches!(
        configured.trim().to_ascii_lowercase().as_str(),
        "false" | "no" | "off" | "0"
    ))
}

fn format_patch_cover_description(
    repo: &GitRepo,
    description_file: Option<&Path>,
    mode: Option<&str>,
) -> Result<(Option<String>, Option<String>)> {
    let description = if let Some(path) = description_file {
        Some(
            fs::read_to_string(path)?
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        )
    } else {
        format_patch_current_branch_description(repo)?
    };
    let Some(description) = description.filter(|value| !value.trim().is_empty()) else {
        return Ok((None, None));
    };
    let effective_mode = mode.unwrap_or("message");
    if effective_mode == "none" {
        return Ok((None, None));
    }
    let mut lines = description.lines();
    let first_line = lines.next().unwrap_or_default().trim().to_owned();
    let rest = lines.collect::<Vec<_>>().join("\n").trim().to_owned();
    let auto_subject = !first_line.is_empty() && first_line.chars().count() <= 100;
    let effective_mode = if effective_mode == "auto" {
        if auto_subject { "subject" } else { "message" }
    } else {
        effective_mode
    };
    match effective_mode {
        "subject" => {
            let body = if rest.is_empty() { None } else { Some(rest) };
            Ok((Some(first_line), body))
        }
        "message" => Ok((None, Some(description))),
        _ => Ok((None, None)),
    }
}

fn format_patch_subject_prefix(
    cli_subject_prefix: Option<&str>,
    configured_subject_prefix: Option<&str>,
    rfc: Option<&str>,
) -> String {
    let base_prefix = cli_subject_prefix
        .or(configured_subject_prefix)
        .unwrap_or("PATCH");
    match rfc {
        None | Some("") => base_prefix.to_owned(),
        Some(rfc) if rfc.starts_with('-') => {
            let suffix = rfc[1..].trim();
            if suffix.is_empty() {
                base_prefix.to_owned()
            } else if base_prefix.is_empty() {
                suffix.to_owned()
            } else {
                format!("{base_prefix} {suffix}")
            }
        }
        Some(rfc) if base_prefix.is_empty() => rfc.to_owned(),
        Some(rfc) => format!("{rfc} {base_prefix}"),
    }
}

fn format_patch_current_branch_description(repo: &GitRepo) -> Result<Option<String>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let Some(branch_ref) = current_branch_ref(&refs)? else {
        return Ok(None);
    };
    let branch = branch_display_name(&branch_ref);
    Ok(read_config_section_value(
        repo,
        "branch",
        &branch,
        "description",
    )?)
}

fn format_patch_commit_list_format(
    repo: &GitRepo,
    commit_list_format: Option<&str>,
) -> Result<Option<String>> {
    let configured = read_config_value(repo, "format.commitlistformat")?;
    let value = commit_list_format
        .map(str::to_owned)
        .or(configured)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if let Some(value) = value.as_deref() {
        validate_format_patch_commit_list_format(value)?;
    }
    Ok(value)
}

fn validate_format_patch_commit_list_format(value: &str) -> Result<()> {
    if matches!(value, "modern" | "shortlog") || value.starts_with("log:") {
        return Ok(());
    }
    if value.contains("%(count)")
        || value.contains("%(total)")
        || value.contains("%s")
        || value.contains("%an")
    {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("'{}' is not a valid format string", value),
    })
}

fn format_patch_filename_limit(repo: &GitRepo, cli_limit: Option<&str>) -> Result<Option<usize>> {
    let configured = read_config_value(repo, "format.filenameMaxLength")?;
    cli_limit
        .or(configured.as_deref())
        .or(Some("64"))
        .map(|value| {
            value.parse::<usize>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("invalid filename max length: {value}"),
            })
        })
        .transpose()
}

fn format_patch_effective_headers(
    repo: &GitRepo,
    to: Vec<String>,
    no_to: bool,
    cc: Vec<String>,
    no_cc: bool,
    add_header: Vec<String>,
    no_add_header: bool,
    encode_email_headers: bool,
) -> Result<Vec<String>> {
    let mut config_to = Vec::new();
    let mut config_cc = Vec::new();
    let mut config_headers = Vec::new();
    for entry in read_config_entries(repo).map_err(CliError::Io)? {
        if entry.name().eq_ignore_ascii_case("format.to") {
            config_to.push(entry.value);
        } else if entry.name().eq_ignore_ascii_case("format.cc") {
            config_cc.push(entry.value);
        } else if entry.name().eq_ignore_ascii_case("format.headers") {
            config_headers.push(entry.value);
        }
    }

    let mut merged_to = if no_to { Vec::new() } else { config_to };
    merged_to.extend(to);
    let mut merged_cc = if no_cc { Vec::new() } else { config_cc };
    merged_cc.extend(cc);
    let mut headers = if no_add_header {
        Vec::new()
    } else {
        config_headers
    };
    headers.extend(add_header);
    format_patch_merge_headers(merged_to, merged_cc, headers, encode_email_headers)
}

fn format_patch_merge_headers(
    to: Vec<String>,
    cc: Vec<String>,
    headers: Vec<String>,
    encode_email_headers: bool,
) -> Result<Vec<String>> {
    let mut all_to = Vec::new();
    let mut all_cc = Vec::new();
    let mut first_to_index = None;
    let mut first_cc_index = None;
    for (index, header) in headers.iter().enumerate() {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let trimmed_name = name.trim();
        if trimmed_name.eq_ignore_ascii_case("to") {
            if first_to_index.is_none() {
                first_to_index = Some(index);
            }
            all_to.push(value.trim().to_owned());
        } else if trimmed_name.eq_ignore_ascii_case("cc") {
            if first_cc_index.is_none() {
                first_cc_index = Some(index);
            }
            all_cc.push(value.trim().to_owned());
        }
    }
    all_to.extend(to);
    all_cc.extend(cc);

    let mut result = Vec::new();
    for (index, header) in headers.into_iter().enumerate() {
        let Some((name, _value)) = header.split_once(':') else {
            result.push(header);
            continue;
        };
        let trimmed_name = name.trim();
        if trimmed_name.eq_ignore_ascii_case("to") {
            if first_to_index == Some(index) {
                result.extend(format_patch_fold_address_header(
                    "To",
                    all_to.clone(),
                    encode_email_headers,
                ));
            }
            continue;
        }
        if trimmed_name.eq_ignore_ascii_case("cc") {
            if first_cc_index == Some(index) {
                result.extend(format_patch_fold_address_header(
                    "Cc",
                    all_cc.clone(),
                    encode_email_headers,
                ));
            }
            continue;
        }
        result.push(header.trim_end_matches(['\r', '\n']).to_owned());
    }
    if first_to_index.is_none() && !all_to.is_empty() {
        result.extend(format_patch_fold_address_header(
            "To",
            all_to,
            encode_email_headers,
        ));
    }
    if first_cc_index.is_none() && !all_cc.is_empty() {
        result.extend(format_patch_fold_address_header(
            "Cc",
            all_cc,
            encode_email_headers,
        ));
    }
    Ok(result)
}

fn format_patch_fold_address_header(
    name: &str,
    values: Vec<String>,
    encode_email_headers: bool,
) -> Vec<String> {
    let values = values
        .into_iter()
        .map(|value| format_patch_address_value(value.trim(), encode_email_headers))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Vec::new();
    }
    if values.len() == 1 {
        return vec![format!("{name}: {}", values[0])];
    }
    let mut lines = Vec::with_capacity(values.len());
    lines.push(format!("{name}: {},", values[0]));
    for (index, value) in values.iter().enumerate().skip(1) {
        let suffix = if index + 1 == values.len() { "" } else { "," };
        lines.push(format!(" {value}{suffix}"));
    }
    lines
}

fn format_patch_address_value(value: &str, encode_email_headers: bool) -> String {
    let trimmed = value.trim();
    let Some(start) = trimmed.rfind('<') else {
        return trimmed.to_owned();
    };
    let Some(end_rel) = trimmed[start + 1..].find('>') else {
        return trimmed.to_owned();
    };
    let end = start + 1 + end_rel;
    let name = trimmed[..start].trim();
    let email = trimmed[start + 1..end].trim();
    if email.is_empty() {
        return trimmed.to_owned();
    }
    if name.is_empty() {
        return format!("<{email}>");
    }
    format!(
        "{} <{email}>",
        format_patch_address_display_name(name, encode_email_headers)
    )
}

fn format_patch_address_display_name(name: &str, encode_email_headers: bool) -> String {
    let unquoted = name.trim().trim_matches('"');
    if encode_email_headers && !unquoted.is_ascii() {
        return format_patch_encode_rfc2047_q(unquoted);
    }
    if format_patch_needs_rfc822_quotes(unquoted) {
        return format!("\"{}\"", format_patch_escape_quoted_string(unquoted));
    }
    unquoted.to_owned()
}

fn format_patch_needs_rfc822_quotes(value: &str) -> bool {
    value.chars().any(|ch| {
        !matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | ' ' | '!' | '#'..='\''
            | '*' | '+' | '-' | '/' | '=' | '?' | '^' | '_' | '`' | '{' | '|' | '}' | '~')
    })
}

fn format_patch_escape_quoted_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | '"') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn format_patch_encode_rfc2047_q(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'*'
            | b'+'
            | b'-'
            | b'/'
            | b'='
            | b'_'
            | b'.' => encoded.push(char::from(*byte)),
            b' ' => encoded.push_str("=20"),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(&mut encoded, "={byte:02X}");
            }
        }
    }
    format!("=?UTF-8?q?{encoded}?=")
}

fn format_patch_sender_override(
    repo: &GitRepo,
    from: Option<&str>,
    no_from: bool,
    encode_email_headers: bool,
) -> Result<Option<String>> {
    if no_from {
        return Ok(None);
    }
    if let Some(from) = from {
        if from.is_empty() {
            let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
            return Ok(Some(format_patch_address_value(
                &format!("{} <{}>", committer.name, committer.email),
                encode_email_headers,
            )));
        }
        if !from.contains('<') || !from.contains('>') {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid ident line: {from}"),
            });
        }
        return Ok(Some(format_patch_address_value(from, encode_email_headers)));
    }
    let Some(configured) = read_config_value(repo, "format.from")? else {
        return Ok(None);
    };
    let configured = configured.trim();
    if configured.is_empty() {
        return Ok(None);
    }
    if config_bool_value_enabled(configured) {
        let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
        return Ok(Some(format_patch_address_value(
            &format!("{} <{}>", committer.name, committer.email),
            encode_email_headers,
        )));
    }
    Ok(Some(format_patch_address_value(
        configured,
        encode_email_headers,
    )))
}

fn format_patch_encode_email_headers_enabled(
    repo: &GitRepo,
    encode_email_headers: bool,
    no_encode_email_headers: bool,
) -> Result<bool> {
    if no_encode_email_headers {
        return Ok(false);
    }
    if encode_email_headers {
        return Ok(true);
    }
    let Some(configured) = read_config_value(repo, "format.encodeEmailHeaders")? else {
        return Ok(true);
    };
    Ok(config_bool_value_enabled(configured.trim()))
}

fn format_patch_force_in_body_from_enabled(
    repo: &GitRepo,
    force_in_body_from: bool,
    no_force_in_body_from: bool,
) -> Result<bool> {
    if no_force_in_body_from {
        return Ok(false);
    }
    if force_in_body_from {
        return Ok(true);
    }
    let Some(configured) = read_config_value(repo, "format.forceInBodyFrom")? else {
        return Ok(false);
    };
    Ok(config_bool_value_enabled(configured.trim()))
}

fn format_patch_prelude_mode(
    patch_short_alias: bool,
    patch_with_raw: bool,
    no_stat: bool,
    no_patch: bool,
    numstat: bool,
    dirstat: bool,
    dirstat_by_file: bool,
    shortstat: bool,
    raw: bool,
    summary: bool,
) -> FormatPatchPreludeMode {
    if patch_with_raw || raw {
        FormatPatchPreludeMode::Raw
    } else if numstat {
        FormatPatchPreludeMode::Numstat
    } else if dirstat_by_file {
        FormatPatchPreludeMode::DirstatByFile
    } else if dirstat {
        FormatPatchPreludeMode::Dirstat
    } else if shortstat {
        FormatPatchPreludeMode::Shortstat
    } else if summary {
        FormatPatchPreludeMode::Summary
    } else if patch_short_alias || no_stat || no_patch {
        FormatPatchPreludeMode::None
    } else {
        FormatPatchPreludeMode::Diffstat
    }
}

fn normalize_format_patch_dirstat(
    dirstat: Option<&str>,
    dirstat_short: Option<&str>,
    cumulative: bool,
    dirstat_by_file: Option<&str>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(value) = dirstat_short.or(dirstat) {
        if !value.is_empty() {
            parts.push(value.to_owned());
        }
    }
    if let Some(value) = dirstat_by_file {
        parts.push("files".to_owned());
        if !value.is_empty() {
            parts.push(value.to_owned());
        }
    }
    if cumulative {
        parts.push("cumulative".to_owned());
    }
    if parts.is_empty() {
        if dirstat.is_some() || dirstat_short.is_some() || dirstat_by_file.is_some() || cumulative {
            Some(String::new())
        } else {
            None
        }
    } else {
        Some(parts.join(","))
    }
}

fn git_path_output(path: &std::path::Path) -> String {
    git_path_output_string(path.display().to_string())
}

#[cfg(windows)]
fn git_path_output_string(value: String) -> String {
    let value = value.replace('\\', "/");
    value.strip_prefix("./").unwrap_or(&value).to_owned()
}

#[cfg(not(windows))]
fn git_path_output_string(value: String) -> String {
    value.strip_prefix("./").unwrap_or(&value).to_owned()
}

fn format_patch_old_tree<S>(
    commit_cache: &CommitObjectCache<'_, S>,
    commit: &CommitObject,
) -> Result<Option<ObjectId>>
where
    S: GitObjectStore + ?Sized,
{
    commit
        .parents
        .first()
        .map(|parent| {
            commit_cache
                .read_commit_links(parent)
                .map(|links| links.tree.clone())
                .map_err(CliError::from)
        })
        .transpose()
}

fn format_patch_base_information(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commits: &[CollectedCommit],
    base: &str,
) -> Result<String> {
    let base_commit = resolve_commitish(repo, store, base)?;
    validate_format_patch_base_commit(repo, store, commits, &base_commit)?;
    format_patch_base_information_block(repo, store, commits, base_commit)
}

fn render_format_patch_interdiff(
    repo: &GitRepo,
    store: &LooseObjectStore,
    previous: &str,
    current: &str,
    reroll_count: Option<&str>,
    indent: bool,
) -> Result<String> {
    let packed_store = store.packed_first();
    let commit_cache = CommitObjectCache::new(&packed_store);
    let tree_cache = TreeObjectCache::new(&packed_store);
    let previous_id = resolve_commitish(repo, store, previous)?;
    let current_id = resolve_commitish(repo, store, current)?;
    let previous_commit = commit_cache.read_commit_links(&previous_id)?;
    let current_commit = commit_cache.read_commit_links(&current_id)?;
    let mut blob_cache = FormatPatchBlobCache::new(store);
    let mut patch = Vec::new();
    write_commit_patch_entries_tree_diff_cached(
        &mut patch,
        repo,
        store,
        &tree_cache,
        Some(&previous_commit.tree),
        &current_commit.tree,
        default_abbrev_len(store)?,
        &mut blob_cache,
    )?;
    let patch = String::from_utf8_lossy(&patch);
    let mut output = if let Some(previous_version) = reroll_count
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|value| value.checked_sub(1))
        .filter(|value| *value > 0)
    {
        format!("Interdiff against v{previous_version}:\n")
    } else {
        String::from("Interdiff:\n")
    };
    for line in patch.lines() {
        if indent {
            output.push_str("  ");
        }
        output.push_str(line);
        output.push('\n');
    }
    Ok(output)
}

fn render_format_patch_range_diff(
    previous: &str,
    current_range: &str,
    creation_factor: Option<&str>,
) -> Result<String> {
    let ranges = [previous.to_owned(), current_range.to_owned()];
    let right_only = !previous.contains("..") && !previous.contains("...");
    let options = super::history_commands::RangeDiffOptions {
        color: false,
        creation_factor: creation_factor.map(str::to_owned),
        left_only: false,
        right_only,
        notes: false,
        no_notes: false,
    };
    Ok(format!(
        "Range-diff:\n{}",
        super::history_commands::render_range_diff_output(&ranges, &options)?
    ))
}

fn format_patch_auto_base_mode(repo: &GitRepo) -> Result<Option<bool>> {
    let Some(configured) = read_config_value(repo, "format.useAutoBase")? else {
        return Ok(None);
    };
    let normalized = configured.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "true" | "yes" | "on" | "1" => Ok(Some(true)),
        "whenable" => Ok(Some(false)),
        "false" | "no" | "off" | "0" => Ok(None),
        _ => Ok(None),
    }
}

fn format_patch_mboxrd_enabled(repo: &GitRepo) -> Result<bool> {
    if crate::runtime::pending_format_patch_mboxrd_arg() {
        return Ok(true);
    }
    let Some(configured) = read_config_value(repo, "format.mboxrd")? else {
        return Ok(false);
    };
    let trimmed = configured.trim();
    Ok(trimmed.is_empty() || config_bool_value_enabled(trimmed))
}

fn format_patch_auto_base_information(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commits: &[CollectedCommit],
    when_able: bool,
) -> Result<Option<String>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let Some(current_branch_ref) = current_branch_ref(&refs)? else {
        if when_able {
            return Ok(None);
        }
        return Err(CliError::Fatal {
            code: 128,
            message: "failed to get upstream, if you want to record base commit automatically,\nplease use git branch --set-upstream-to to track a remote branch.\nOr you could specify base commit by --base=<base-commit-id> manually".into(),
        });
    };
    let branch_name = branch_display_name(&current_branch_ref);
    let Some(upstream) = read_branch_upstream(repo, &branch_name)? else {
        if when_able {
            return Ok(None);
        }
        return Err(CliError::Fatal {
            code: 128,
            message: "failed to get upstream, if you want to record base commit automatically,\nplease use git branch --set-upstream-to to track a remote branch.\nOr you could specify base commit by --base=<base-commit-id> manually".into(),
        });
    };
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head = refs.resolve("HEAD")?;
    let upstream_id = match refs.resolve(&upstream.ref_name) {
        Ok(id) => id,
        Err(_) if when_able => return Ok(None),
        Err(_) => {
            return Err(CliError::Fatal {
                code: 128,
                message: "failed to get upstream, if you want to record base commit automatically,\nplease use git branch --set-upstream-to to track a remote branch.\nOr you could specify base commit by --base=<base-commit-id> manually".into(),
            });
        }
    };
    let commit_cache = CommitObjectCache::new(store);
    let merge_bases = merge_bases_all_cached(&commit_cache, &head, &upstream_id)?;
    let Some(base_commit) = merge_bases.first().cloned() else {
        if when_able {
            return Ok(None);
        }
        return Err(CliError::Fatal {
            code: 128,
            message: "failed to get upstream, if you want to record base commit automatically,\nplease use git branch --set-upstream-to to track a remote branch.\nOr you could specify base commit by --base=<base-commit-id> manually".into(),
        });
    };
    if merge_bases.len() > 1 {
        if when_able {
            return Ok(None);
        }
        return Err(CliError::Fatal {
            code: 128,
            message: "failed to find exact merge base".into(),
        });
    }
    Ok(Some(format_patch_base_information_block(
        repo,
        store,
        commits,
        base_commit,
    )?))
}

fn validate_format_patch_base_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commits: &[CollectedCommit],
    base_commit: &ObjectId,
) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    if commits.iter().any(|entry| entry.id == *base_commit) {
        return Err(CliError::Fatal {
            code: 128,
            message: "base commit should not be in revision list".into(),
        });
    }
    for entry in commits {
        if !is_ancestor_commit_with_repo_cached(repo, &commit_cache, base_commit, &entry.id)? {
            return Err(CliError::Fatal {
                code: 128,
                message: "base commit should be an ancestor of revision list".into(),
            });
        }
    }
    Ok(())
}

fn format_patch_base_information_block(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commits: &[CollectedCommit],
    base_commit: ObjectId,
) -> Result<String> {
    let mut lines = vec![format!("base-commit: {}", base_commit.to_hex())];
    if let Some(first_commit) = commits.first()
        && let Some(first_parent) = first_commit.commit.parents.first()
        && *first_parent != base_commit
    {
        let prereq_range = format!("{}..{}", base_commit.to_hex(), first_parent.to_hex());
        let commit_cache = CommitObjectCache::new(store);
        let tree_cache = TreeObjectCache::new(store);
        let revs = collect_rev_list_revs(repo, store, false, vec![prereq_range])?;
        let mut prerequisite_commits =
            collect_commit_objects_with_exclusions_cached(repo, store, &commit_cache, &revs, None)?;
        prerequisite_commits.reverse();
        for entry in prerequisite_commits {
            if let Some(patch_id) = reference_commands::commit_patch_id_stable_cached(
                store,
                &commit_cache,
                &tree_cache,
                &entry.id,
            )? {
                lines.push(format!("prerequisite-patch-id: {patch_id}"));
            }
        }
    }
    Ok(lines.join("\n"))
}

pub(crate) fn send_email(options: SendEmailCommandOptions) -> Result<()> {
    if !options.args.is_empty() {
        return send_email_patches(&options);
    }
    if options.dump_aliases == options.translate_aliases {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git send-email (--dump-aliases|--translate-aliases)".into(),
        });
    }
    let aliases = read_send_email_aliases()?;
    if options.dump_aliases {
        for name in aliases.keys() {
            println!("{name}");
        }
        return Ok(());
    }

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            println!();
            continue;
        }
        let translated = line
            .split_whitespace()
            .map(|word| {
                aliases
                    .get(word)
                    .cloned()
                    .unwrap_or_else(|| word.to_owned())
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("{translated}");
    }
    Ok(())
}

fn send_email_patches(options: &SendEmailCommandOptions) -> Result<()> {
    let repo = find_repo()?;
    let _ignored_eight_bit_encoding = options.eight_bit_encoding.as_deref();
    let _ignored_annotate = options.annotate;
    let _ignored_batch_size = options.batch_size.as_deref();
    let _ignored_subject = options.subject.as_deref();
    let _ignored_cc_cmd = options.cc_cmd.as_deref();
    let _suppressed_cc_author = options
        .suppress_cc
        .iter()
        .any(|value| value.eq_ignore_ascii_case("author"));
    let _ignored_cc_cover = options.cc_cover;
    let _ignored_chain_reply_to = options.chain_reply_to;
    let _ignored_compose = options.compose;
    let _ignored_compose_encoding = options.compose_encoding.as_deref();
    let _ignored_confirm = options.confirm.as_deref();
    let _ignored_envelope_sender = options.envelope_sender.as_deref();
    let _ignored_force = options.force;
    let _ignored_format_patch = options.format_patch;
    let _ignored_header_cmd = options.header_cmd.as_deref();
    let _ignored_identity = options.identity.as_deref();
    let _ignored_in_reply_to = options.in_reply_to.as_deref();
    let _ignored_mailmap = options.mailmap;
    let _ignored_no_format_patch = options.no_format_patch;
    let _ignored_no_bcc = options.no_bcc;
    let _ignored_no_cc = options.no_cc;
    let _ignored_no_cc_cover = options.no_cc_cover;
    let _ignored_no_chain_reply_to = options.no_chain_reply_to;
    let _ignored_no_header_cmd = options.no_header_cmd;
    let _ignored_no_identity = options.no_identity;
    let _ignored_no_mailmap = options.no_mailmap;
    let _ignored_no_signed_off_by_cc = options.no_signed_off_by_cc;
    let _ignored_quiet = options.quiet;
    let _ignored_sendmail_cmd = options.sendmail_cmd.as_deref();
    let _ignored_no_smtp_auth = options.no_smtp_auth;
    let _ignored_no_suppress_from = options.no_suppress_from;
    let _ignored_no_thread = options.no_thread;
    let _ignored_no_to = options.no_to;
    let _ignored_no_to_cover = options.no_to_cover;
    let _ignored_no_validate = options.no_validate;
    let _ignored_no_xmailer = options.no_xmailer;
    let _ignored_relogin_delay = options.relogin_delay.as_deref();
    let _ignored_signed_off_by_cc = options.signed_off_by_cc;
    let _ignored_validate = options.validate;
    let _ignored_xmailer = options.xmailer;
    let _ignored_smtp_auth = options.smtp_auth.as_deref();
    let _ignored_smtp_debug = options.smtp_debug.as_deref();
    let _ignored_smtp_domain = options.smtp_domain.as_deref();
    let _ignored_smtp_server_option = &options.smtp_server_option;
    let _ignored_smtp_ssl_cert_path = options.smtp_ssl_cert_path.as_deref();
    let _ignored_smtp_ssl = options.smtp_ssl;
    let _ignored_smtp_user = options.smtp_user.as_deref();
    let _ignored_smtp_pass = options.smtp_pass.as_deref();
    let _ignored_suppress_from = options.suppress_from;
    let _ignored_thread = options.thread;
    let _ignored_to_cmd = options.to_cmd.as_deref();
    let _ignored_to_cover = options.to_cover;
    let _ignored_transfer_encoding = options.transfer_encoding.as_deref();
    for path in &options.args {
        if !std::path::Path::new(path).exists() {
            return Err(send_email_missing_patch_error(path)?);
        }
    }
    if options.relogin_delay.is_some() && options.batch_size.is_none() {
        return Err(CliError::Stderr {
            code: 255,
            text:
                "`batch-size` and `relogin` must be specified together (via command-line or configuration option)\n"
                    .into(),
        });
    }
    let smtp_encryption = options
        .smtp_encryption
        .clone()
        .or_else(|| {
            if options.smtp_ssl {
                Some("ssl".to_owned())
            } else {
                None
            }
        })
        .or_else(|| {
            read_config_value(&repo, "sendemail.smtpencryption")
                .ok()
                .flatten()
        });
    let transport = if let Some(sendmail_cmd) = options.sendmail_cmd.clone() {
        SendEmailTransport::Sendmail {
            command: sendmail_cmd,
        }
    } else {
        let smtp_server = options
            .smtp_server
            .clone()
            .or_else(|| {
                read_config_value(&repo, "sendemail.smtpserver")
                    .ok()
                    .flatten()
            })
            .ok_or_else(|| CliError::Fatal {
                code: 1,
                message: "sendemail.smtpserver is required for SMTP patch sending".into(),
            })?;
        let smtp_port = options
            .smtp_server_port
            .as_deref()
            .and_then(|value| value.parse().ok())
            .or_else(|| {
                read_config_value(&repo, "sendemail.smtpserverport")
                    .ok()
                    .flatten()
                    .and_then(|value| value.parse().ok())
            });
        let endpoint = parse_smtp_endpoint(&smtp_server, smtp_port, smtp_encryption.as_deref())?;
        SendEmailTransport::Smtp { endpoint }
    };
    let from = options
        .from
        .clone()
        .or_else(|| read_config_value(&repo, "sendemail.from").ok().flatten())
        .or_else(|| read_config_value(&repo, "user.email").ok().flatten())
        .ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "sendemail.from or user.email is required".into(),
        })?;
    let to = if options.to.is_empty() {
        read_multi_config_values("sendemail.to")?
    } else {
        parse_send_email_recipients(&options.to)
    };
    if to.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: "sendemail.to is required".into(),
        });
    }
    let cc = parse_send_email_recipients(&options.cc);
    let bcc = parse_send_email_recipients(&options.bcc);
    let recipients = to
        .iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .cloned()
        .collect::<Vec<_>>();
    for path in &options.args {
        let mut message = fs::read(&path)?;
        let rendered_headers =
            ensure_send_email_headers(&mut message, &from, &to, &cc, options.reply_to.as_deref())?;
        let quiet_subject = send_email_quiet_subject(&rendered_headers);
        if options.dry_run {
            if options.quiet {
                println!("Dry-Sent {quiet_subject}");
                continue;
            }
            println!("{path}");
            if options.compose {
                println!("Summary email is empty, skipping it");
            }
            println!("Dry-OK. Log says:");
            match &transport {
                SendEmailTransport::Smtp { endpoint } => {
                    println!("Server: {}", endpoint.host);
                    println!("MAIL FROM:<{}>", smtp_addr(&from));
                    for recipient in &recipients {
                        println!("RCPT TO:<{}>", smtp_addr(recipient));
                    }
                }
                SendEmailTransport::Sendmail { command } => {
                    println!("Sendmail: {command} -i {}", recipients.join(" "));
                }
            }
            for line in rendered_headers.lines() {
                println!("{line}");
            }
            println!();
            println!("Result: OK");
            continue;
        }
        match &transport {
            SendEmailTransport::Smtp { endpoint } => {
                let mut client = match SmtpClient::connect(endpoint) {
                    Ok(client) => client,
                    Err(_) => {
                        if !options.quiet {
                            println!("{path}");
                        }
                        return Err(CliError::Stderr {
                            code: 61,
                            text: stock_send_email_smtp_init_error(
                                endpoint,
                                smtp_encryption.as_deref(),
                            ),
                        });
                    }
                };
                client.ehlo()?;
                client.send_message(&from, &recipients, &message)?;
                if options.quiet {
                    println!("Sent {quiet_subject}");
                } else {
                    println!("{path}");
                    if options.compose {
                        println!("Summary email is empty, skipping it");
                    }
                    println!("OK. Log says:");
                    println!("Server: {}", endpoint.host);
                    println!("MAIL FROM:<{}>", smtp_addr(&from));
                    for recipient in &recipients {
                        println!("RCPT TO:<{}>", smtp_addr(recipient));
                    }
                    for line in rendered_headers.lines() {
                        println!("{line}");
                    }
                    println!();
                    println!("Result: 250 ");
                }
                client.quit()?;
            }
            SendEmailTransport::Sendmail { command } => {
                run_sendmail_command(command, &recipients, &message)?;
                if options.quiet {
                    println!("Sent {quiet_subject}");
                } else {
                    println!("{path}");
                    if options.compose {
                        println!("Summary email is empty, skipping it");
                    }
                    println!("OK. Log says:");
                    println!("Sendmail: {command} -i {}", recipients.join(" "));
                    for line in rendered_headers.lines() {
                        println!("{line}");
                    }
                    println!();
                    println!("Result: OK");
                }
            }
        }
    }
    Ok(())
}

enum SendEmailTransport {
    Smtp { endpoint: SmtpEndpoint },
    Sendmail { command: String },
}

fn send_email_quiet_subject(rendered_headers: &str) -> &str {
    rendered_headers
        .lines()
        .find_map(|line| line.strip_prefix("Subject: "))
        .unwrap_or("<no-subject>")
}

fn run_sendmail_command(command: &str, recipients: &[String], message: &[u8]) -> Result<()> {
    let mut child = ProcessCommand::new("/bin/sh")
        .arg("-c")
        .arg(format!("{command} -i \"$@\""))
        .arg("sendmail")
        .args(recipients)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io::Error::other)?;
    child
        .stdin
        .as_mut()
        .expect("sendmail stdin")
        .write_all(message)?;
    let output = child.wait_with_output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(CliError::Stderr {
        code: output.status.code().unwrap_or(1),
        text: if stderr.is_empty() {
            format!("sendmail command failed: {command}\n")
        } else {
            format!("{stderr}\n")
        },
    })
}

fn send_email_missing_patch_error(path: &str) -> Result<CliError> {
    let out_dir = std::env::temp_dir().join(format!(
        "zmin-send-email-{}-{}",
        std::process::id(),
        unique_timestamp_nanos()
    ));
    Ok(CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: ambiguous argument '{path}': unknown revision or path not in the working tree.\n\
             Use '--' to separate paths from revisions, like this:\n\
             'git <command> [<revision>...] -- [<file>...]'\n\
             format-patch -o {} {path}: command returned error: 128\n",
            out_dir.display()
        ),
    })
}

fn unique_timestamp_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn ensure_send_email_headers(
    message: &mut Vec<u8>,
    from: &str,
    to: &[String],
    cc: &[String],
    reply_to: Option<&str>,
) -> Result<String> {
    let text = String::from_utf8_lossy(message);
    let (headers, _) = split_mail_headers(&text);
    let header_map = parse_mail_headers(headers);
    let (_, body) = split_mail_headers(&text);
    let original_from = header_map.get("from").cloned();
    let subject = header_map.get("subject").cloned().unwrap_or_default();
    let date = header_map
        .get("date")
        .cloned()
        .unwrap_or_else(send_email_date_header);
    let message_id = header_map
        .get("message-id")
        .cloned()
        .unwrap_or_else(|| send_email_message_id(from));
    let x_mailer = header_map
        .get("x-mailer")
        .cloned()
        .unwrap_or_else(|| format!("git-send-email {}", env!("CARGO_PKG_VERSION")));
    let mime_version = header_map
        .get("mime-version")
        .cloned()
        .unwrap_or_else(|| "1.0".to_owned());
    let content_transfer_encoding = header_map
        .get("content-transfer-encoding")
        .cloned()
        .unwrap_or_else(|| "8bit".to_owned());
    let mut rendered = Vec::new();
    rendered.push(format!("From: {from}"));
    rendered.push(format!("To: {}", to.join(", ")));
    if !cc.is_empty() {
        rendered.push(format!("Cc: {}", cc.join(", ")));
    }
    if !subject.is_empty() {
        rendered.push(format!("Subject: {subject}"));
    }
    rendered.push(format!("Date: {date}"));
    rendered.push(format!("Message-ID: {message_id}"));
    rendered.push(format!("X-Mailer: {x_mailer}"));
    if let Some(reply_to) = reply_to {
        rendered.push(format!("Reply-To: {reply_to}"));
    }
    rendered.push(format!("MIME-Version: {mime_version}"));
    rendered.push(format!(
        "Content-Transfer-Encoding: {content_transfer_encoding}"
    ));
    let rendered_headers = rendered.join("\n");
    let body_prefix = original_from
        .map(|value| format!("From: {value}\n\n"))
        .unwrap_or_default();
    *message = format!("{rendered_headers}\n\n{body_prefix}{body}").into_bytes();
    if !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    Ok(rendered_headers)
}

fn parse_send_email_recipients(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn send_email_date_header() -> String {
    crate::runtime::local_now().to_rfc2822()
}

fn send_email_message_id(from: &str) -> String {
    format!(
        "<{}.{}-1-{}>",
        current_unix_timestamp().unwrap_or(0),
        std::process::id(),
        smtp_addr(from)
    )
}

fn stock_send_email_smtp_init_error(
    endpoint: &SmtpEndpoint,
    encryption_override: Option<&str>,
) -> String {
    let encryption = encryption_override.unwrap_or_default();
    format!(
        "Unable to initialize SMTP properly. Check config and use --smtp-debug. VALUES: server={} encryption={} hello={} port={}.\n",
        endpoint.host,
        encryption,
        stock_send_email_hello_name(),
        endpoint.port
    )
}

fn stock_send_email_hello_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_owned())
}

#[derive(Clone)]
struct SmtpEndpoint {
    host: String,
    port: u16,
    tls: bool,
}

fn parse_smtp_endpoint(
    server: &str,
    port: Option<u16>,
    encryption: Option<&str>,
) -> Result<SmtpEndpoint> {
    let (rest, scheme_tls, default_port) = if let Some(rest) = server.strip_prefix("smtps://") {
        (rest, true, 465)
    } else if let Some(rest) = server.strip_prefix("smtp://") {
        (rest, false, 25)
    } else {
        (
            server,
            matches!(encryption, Some(value) if value.eq_ignore_ascii_case("ssl")),
            25,
        )
    };
    let rest = rest.trim_start_matches('/').trim_end_matches('/');
    let (host, parsed_port) = match rest.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            (host, port.parse::<u16>().ok())
        }
        _ => (rest, None),
    };
    Ok(SmtpEndpoint {
        host: host.to_owned(),
        port: port.or(parsed_port).unwrap_or(default_port),
        tls: scheme_tls || matches!(encryption, Some(value) if value.eq_ignore_ascii_case("ssl")),
    })
}

struct SmtpClient {
    stream: io::BufReader<Box<dyn NetworkStream>>,
}

impl SmtpClient {
    fn connect(endpoint: &SmtpEndpoint) -> Result<Self> {
        let mut client = Self {
            stream: io::BufReader::new(connect_network_stream(
                &endpoint.host,
                endpoint.port,
                endpoint.tls,
            )?),
        };
        client.expect_code(220)?;
        Ok(client)
    }

    fn ehlo(&mut self) -> Result<()> {
        self.write_line("EHLO localhost")?;
        self.expect_code(250)
    }

    fn send_message(&mut self, from: &str, recipients: &[String], message: &[u8]) -> Result<()> {
        self.write_line(&format!("MAIL FROM:<{}>", smtp_addr(from)))?;
        self.expect_code(250)?;
        for recipient in recipients {
            self.write_line(&format!("RCPT TO:<{}>", smtp_addr(recipient)))?;
            self.expect_code(250)?;
        }
        self.write_line("DATA")?;
        self.expect_code(354)?;
        self.write_data(message)?;
        self.expect_code(250)
    }

    fn quit(&mut self) -> Result<()> {
        self.write_line("QUIT")?;
        self.expect_code(221)
    }

    fn write_line(&mut self, line: &str) -> Result<()> {
        self.stream.get_mut().write_all(line.as_bytes())?;
        self.stream.get_mut().write_all(b"\r\n")?;
        self.stream.get_mut().flush()?;
        Ok(())
    }

    fn write_data(&mut self, message: &[u8]) -> Result<()> {
        for line in message.split_inclusive(|byte| *byte == b'\n') {
            if line.starts_with(b".") {
                self.stream.get_mut().write_all(b".")?;
            }
            self.stream.get_mut().write_all(line)?;
            if !line.ends_with(b"\n") {
                self.stream.get_mut().write_all(b"\r\n")?;
            }
        }
        self.stream.get_mut().write_all(b".\r\n")?;
        self.stream.get_mut().flush()?;
        Ok(())
    }

    fn expect_code(&mut self, expected: u16) -> Result<()> {
        loop {
            let mut line = String::new();
            self.stream.read_line(&mut line)?;
            if line.len() < 4 {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("malformed SMTP response: {}", line.trim_end()),
                });
            }
            let code = line[..3].parse::<u16>().unwrap_or(0);
            if code != expected {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("unexpected SMTP response: {}", line.trim_end()),
                });
            }
            if line.as_bytes().get(3) != Some(&b'-') {
                return Ok(());
            }
        }
    }
}

fn smtp_addr(value: &str) -> String {
    if let Some(start) = value.rfind('<')
        && let Some(end) = value[start + 1..].find('>')
    {
        return value[start + 1..start + 1 + end].trim().to_owned();
    }
    value.trim().to_owned()
}

fn read_send_email_aliases() -> Result<BTreeMap<String, String>> {
    let mut aliases = BTreeMap::new();
    let alias_type = read_multi_config_values("sendemail.aliasfiletype")?
        .pop()
        .unwrap_or_else(|| "mutt".into());
    for file in read_multi_config_values("sendemail.aliasesfile")? {
        let content = fs::read_to_string(&file)?;
        parse_send_email_alias_file(&content, &alias_type, &mut aliases);
    }
    Ok(aliases)
}

fn parse_send_email_alias_file(
    content: &str,
    alias_type: &str,
    aliases: &mut BTreeMap<String, String>,
) {
    match alias_type {
        "mutt" => parse_mutt_aliases(content, aliases),
        "mailrc" => parse_mailrc_aliases(content, aliases),
        "pine" => parse_pine_aliases(content, aliases),
        "elm" => parse_elm_aliases(content, aliases),
        "sendmail" => parse_sendmail_aliases(content, aliases),
        "gnus" => parse_gnus_aliases(content, aliases),
        _ => {}
    }
}

fn parse_mutt_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if parts.next() != Some("alias") {
            continue;
        }
        let mut name = parts.next();
        while name == Some("-group") {
            let _group = parts.next();
            name = parts.next();
        }
        let Some(name) = name else {
            continue;
        };
        let address = parts
            .collect::<Vec<_>>()
            .join(" ")
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .replace("\\\"", "\"");
        insert_alias(aliases, name, address);
    }
}

fn parse_mailrc_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("alias ") else {
            continue;
        };
        let mut parts = rest.splitn(2, char::is_whitespace);
        let Some(name) = parts.next() else {
            continue;
        };
        let address = parts.next().unwrap_or("").trim().replace('"', "");
        insert_alias(aliases, name, address);
    }
}

fn parse_pine_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        let address = fields[2]
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .to_owned();
        insert_alias(aliases, fields[0].trim(), address);
    }
}

fn parse_elm_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let fields = line.split('=').map(str::trim).collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        insert_alias(aliases, fields[0], fields[2].to_owned());
    }
}

fn parse_sendmail_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    let mut current = String::new();
    for line in content.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            current.push(' ');
            current.push_str(trimmed.trim());
            continue;
        }
        parse_sendmail_alias_line(&current, aliases);
        current.clear();
        current.push_str(trimmed);
    }
    parse_sendmail_alias_line(&current, aliases);
}

fn parse_sendmail_alias_line(line: &str, aliases: &mut BTreeMap<String, String>) {
    let Some((name, address)) = line.split_once(':') else {
        return;
    };
    insert_alias(aliases, name.trim(), address.trim().to_owned());
}

fn parse_gnus_aliases(content: &str, aliases: &mut BTreeMap<String, String>) {
    for line in content.lines() {
        let Some(rest) = line.trim().strip_prefix("(define-mail-alias ") else {
            continue;
        };
        let values = rest
            .split('"')
            .skip(1)
            .step_by(2)
            .take(2)
            .collect::<Vec<_>>();
        if values.len() == 2 {
            insert_alias(aliases, values[0], values[1].to_owned());
        }
    }
}

fn insert_alias(aliases: &mut BTreeMap<String, String>, name: &str, address: String) {
    if !name.is_empty() && !address.trim().is_empty() {
        aliases.insert(name.to_owned(), address.trim().to_owned());
    }
}

pub(crate) struct ImapSendOptions {
    pub(crate) verbose: bool,
    pub(crate) quiet: bool,
    pub(crate) folder: Option<String>,
    pub(crate) list: bool,
    pub(crate) curl: bool,
    pub(crate) no_curl: bool,
}

pub(crate) fn imap_send(options: ImapSendOptions) -> Result<()> {
    let repo = find_repo().ok();
    let config_value = |name: &str| -> Result<Option<String>> {
        match &repo {
            Some(repo) => read_config_value(repo, name).map_err(CliError::Io),
            None => read_global_config_value(name),
        }
    };
    let folder = options.folder.or(config_value("imap.folder")?);
    let Some(folder) = folder else {
        let warning = if options.no_curl {
            "warning: --no-curl not supported in this build\n"
        } else {
            ""
        };
        return Err(CliError::Stderr {
            code: 1,
            text: format!("{warning}no imap store specified\n"),
        });
    };
    let host = config_value("imap.host")?.ok_or_else(|| CliError::Fatal {
        code: 1,
        message: "no imap host specified".into(),
    })?;
    let user = config_value("imap.user")?.unwrap_or_default();
    let pass = config_value("imap.pass")?.unwrap_or_default();
    let port = config_value("imap.port")?.and_then(|value| value.parse::<u16>().ok());
    let endpoint = parse_imap_endpoint(&host, port)?;
    let _ = (options.verbose, options.curl, options.no_curl);
    if options.list {
        return imap_list(&endpoint, &user, &pass);
    }

    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    if input.is_empty() {
        return Err(CliError::Stderr {
            code: 1,
            text: "nothing to send\n".into(),
        });
    }
    let messages = split_mbox_messages(&input, false)?;
    if !options.quiet {
        eprintln!(
            "sending {} message{}",
            messages.len(),
            if messages.len() == 1 { "" } else { "s" }
        );
    }
    imap_append_messages(&endpoint, &user, &pass, &folder, &messages)
}

struct ImapEndpoint {
    host: String,
    port: u16,
    tls: bool,
}

fn parse_imap_endpoint(host: &str, port: Option<u16>) -> Result<ImapEndpoint> {
    let (host, default_port, tls) = host
        .strip_prefix("imap://")
        .map(|value| (value, 143, false))
        .or_else(|| host.strip_prefix("imap:").map(|value| (value, 143, false)))
        .or_else(|| {
            host.strip_prefix("imaps://")
                .map(|value| (value, 993, true))
        })
        .or_else(|| host.strip_prefix("imaps:").map(|value| (value, 993, true)))
        .or_else(|| host.strip_prefix("//").map(|value| (value, 143, false)))
        .unwrap_or((host, 143, false));
    let host = host.trim_start_matches('/').trim_end_matches('/');
    let (host, parsed_port) = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            (name, port.parse::<u16>().ok())
        }
        _ => (host, None),
    };
    Ok(ImapEndpoint {
        host: host.to_owned(),
        port: port.or(parsed_port).unwrap_or(default_port),
        tls,
    })
}

fn imap_append_messages(
    endpoint: &ImapEndpoint,
    user: &str,
    pass: &str,
    folder: &str,
    messages: &[Vec<u8>],
) -> Result<()> {
    let mut client = ImapClient::connect(endpoint)?;
    client.login(user, pass)?;
    for message in messages {
        client.append(folder, message)?;
    }
    client.logout()
}

fn imap_list(endpoint: &ImapEndpoint, user: &str, pass: &str) -> Result<()> {
    let mut client = ImapClient::connect(endpoint)?;
    client.login(user, pass)?;
    for line in client.list()? {
        println!("{line}");
    }
    client.logout()
}

struct ImapClient {
    stream: io::BufReader<Box<dyn NetworkStream>>,
    sequence: usize,
}

impl ImapClient {
    fn connect(endpoint: &ImapEndpoint) -> Result<Self> {
        let mut client = Self {
            stream: io::BufReader::new(connect_network_stream(
                &endpoint.host,
                endpoint.port,
                endpoint.tls,
            )?),
            sequence: 0,
        };
        let greeting = client.read_line()?;
        if !greeting.starts_with("* OK") {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("unexpected IMAP greeting: {greeting}"),
            });
        }
        Ok(client)
    }

    fn login(&mut self, user: &str, pass: &str) -> Result<()> {
        if user.is_empty() && pass.is_empty() {
            return Ok(());
        }
        let tag = self.next_tag();
        write!(
            self.stream.get_mut(),
            "{tag} LOGIN {} {}\r\n",
            imap_quote(user),
            imap_quote(pass)
        )?;
        self.expect_tag_ok(&tag)
    }

    fn append(&mut self, folder: &str, message: &[u8]) -> Result<()> {
        let tag = self.next_tag();
        write!(
            self.stream.get_mut(),
            "{tag} APPEND {} {{{}}}\r\n",
            imap_quote(folder),
            message.len()
        )?;
        self.stream.get_mut().flush()?;
        let continuation = self.read_line()?;
        if !continuation.starts_with('+') {
            return Err(CliError::Fatal {
                code: 1,
                message: format!("expected IMAP continuation, got: {continuation}"),
            });
        }
        self.stream.get_mut().write_all(message)?;
        self.stream.get_mut().write_all(b"\r\n")?;
        self.expect_tag_ok(&tag)
    }

    fn list(&mut self) -> Result<Vec<String>> {
        let tag = self.next_tag();
        write!(self.stream.get_mut(), "{tag} LIST \"\" \"*\"\r\n")?;
        self.stream.get_mut().flush()?;
        let mut rows = Vec::new();
        loop {
            let line = self.read_line()?;
            if line.starts_with(&format!("{tag} OK")) {
                return Ok(rows);
            }
            if let Some(row) = line.strip_prefix("* LIST ") {
                rows.push(row.to_owned());
            }
        }
    }

    fn logout(&mut self) -> Result<()> {
        let tag = self.next_tag();
        write!(self.stream.get_mut(), "{tag} LOGOUT\r\n")?;
        self.stream.get_mut().flush()?;
        loop {
            let line = self.read_line()?;
            if line.starts_with(&format!("{tag} OK")) {
                return Ok(());
            }
        }
    }

    fn expect_tag_ok(&mut self, tag: &str) -> Result<()> {
        self.stream.get_mut().flush()?;
        loop {
            let line = self.read_line()?;
            if line.starts_with(&format!("{tag} OK")) {
                return Ok(());
            }
            if line.starts_with(&format!("{tag} NO")) || line.starts_with(&format!("{tag} BAD")) {
                return Err(CliError::Fatal {
                    code: 1,
                    message: line,
                });
            }
        }
    }

    fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        self.stream.read_line(&mut line)?;
        Ok(line.trim_end_matches(['\r', '\n']).to_owned())
    }

    fn next_tag(&mut self) -> String {
        self.sequence += 1;
        format!("A{:04}", self.sequence)
    }
}

trait NetworkStream: Read + Write {}

impl<T: Read + Write> NetworkStream for T {}

fn connect_network_stream(host: &str, port: u16, tls: bool) -> Result<Box<dyn NetworkStream>> {
    if tls {
        return connect_tls_network_stream(host, port);
    }
    let stream = std::net::TcpStream::connect((host, port))?;
    Ok(Box::new(stream))
}

#[cfg(feature = "mail-tls")]
fn connect_tls_network_stream(host: &str, port: u16) -> Result<Box<dyn NetworkStream>> {
    let stream = std::net::TcpStream::connect((host, port))?;
    let connector = native_tls::TlsConnector::new().map_err(|error| CliError::Fatal {
        code: 1,
        message: format!("failed to create TLS connector: {error}"),
    })?;
    let stream = connector
        .connect(host, stream)
        .map_err(|error| CliError::Fatal {
            code: 1,
            message: format!("TLS connection failed: {error}"),
        })?;
    Ok(Box::new(stream))
}

#[cfg(not(feature = "mail-tls"))]
fn connect_tls_network_stream(_host: &str, _port: u16) -> Result<Box<dyn NetworkStream>> {
    Err(CliError::Fatal {
        code: 1,
        message: "TLS mail transport requires the 'mail-tls' build feature".into(),
    })
}

fn imap_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub(crate) fn interpret_trailers(options: InterpretTrailersOptions<'_>) -> Result<()> {
    if options.in_place && options.files.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "interpret-trailers --in-place requires file arguments".into(),
        });
    }
    if options.files.is_empty() {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        print!("{}", interpret_trailers_content(&input, &options)?);
        return Ok(());
    }
    for path in &options.files {
        let input = fs::read_to_string(path)?;
        let output = interpret_trailers_content(&input, &options)?;
        if options.in_place {
            fs::write(path, output)?;
        } else {
            print!("{output}");
        }
    }
    Ok(())
}

pub(crate) fn interpret_trailers_content(
    input: &str,
    options: &InterpretTrailersOptions<'_>,
) -> Result<String> {
    let placement = parse_trailer_placement(options.where_)?;
    let if_exists = parse_trailer_if_exists(options.if_exists)?;
    let if_missing = parse_trailer_if_missing(options.if_missing)?;
    let lines = split_text_lines(input);
    let divider_index = if options.no_divider {
        None
    } else {
        lines.iter().position(|line| trailer_divider_line(line))
    };
    let message_end = divider_index.unwrap_or(lines.len());
    let message = &lines[..message_end];
    let suffix = divider_index.map_or([].as_slice(), |index| &lines[index..]);
    let trailer_start = trailer_block_start(message);
    let had_existing_block = trailer_start < message.len();
    let prefix = if trailer_start < message.len() {
        &message[..trailer_start]
    } else {
        message
    };
    let mut entries = if trailer_start < message.len() {
        parse_trailer_entries(&message[trailer_start..])
    } else {
        Vec::new()
    };
    if options.trim_empty {
        entries.retain(|entry| !entry.value.trim().is_empty());
    }
    if !options.only_input {
        for trailer in &options.trailers {
            let addition = parse_trailer_argument(trailer)?;
            apply_trailer_addition(&mut entries, addition, placement, if_exists, if_missing);
        }
    }

    if options.only_trailers {
        return Ok(lines_with_final_newline(&trailer_output_lines(
            &entries,
            options.unfold,
        )));
    }

    let mut output = prefix.to_vec();
    while output.last().is_some_and(|line| line.trim().is_empty()) {
        output.pop();
    }
    if !entries.is_empty() {
        let last_line_is_trailer = output
            .last()
            .is_some_and(|line| split_existing_trailer(line).is_some());
        if !output.is_empty() && (had_existing_block || !last_line_is_trailer) {
            output.push(String::new());
        }
        output.extend(trailer_output_lines(&entries, options.unfold));
    } else if !suffix.is_empty() {
        output.push(String::new());
    }
    output.extend_from_slice(suffix);
    Ok(lines_with_final_newline(&output))
}

fn split_text_lines(input: &str) -> Vec<String> {
    let trimmed = input.trim_end_matches('\n');
    if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed
            .split('\n')
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect()
    }
}

fn parse_trailer_placement(value: Option<&str>) -> Result<TrailerPlacement> {
    match value.unwrap_or("end") {
        "end" => Ok(TrailerPlacement::End),
        "start" => Ok(TrailerPlacement::Start),
        "after" => Ok(TrailerPlacement::After),
        "before" => Ok(TrailerPlacement::Before),
        other => Err(CliError::Fatal {
            code: 129,
            message: format!("unknown trailer placement '{other}'"),
        }),
    }
}

fn parse_trailer_if_exists(value: Option<&str>) -> Result<TrailerIfExists> {
    match value.unwrap_or("addIfDifferentNeighbor") {
        "addIfDifferentNeighbor" => Ok(TrailerIfExists::AddIfDifferentNeighbor),
        "addIfDifferent" => Ok(TrailerIfExists::AddIfDifferent),
        "add" => Ok(TrailerIfExists::Add),
        "replace" => Ok(TrailerIfExists::Replace),
        "doNothing" => Ok(TrailerIfExists::DoNothing),
        other => Err(CliError::Fatal {
            code: 129,
            message: format!("unknown trailer if-exists action '{other}'"),
        }),
    }
}

fn parse_trailer_if_missing(value: Option<&str>) -> Result<TrailerIfMissing> {
    match value.unwrap_or("add") {
        "add" => Ok(TrailerIfMissing::Add),
        "doNothing" => Ok(TrailerIfMissing::DoNothing),
        other => Err(CliError::Fatal {
            code: 129,
            message: format!("unknown trailer if-missing action '{other}'"),
        }),
    }
}

fn trailer_divider_line(line: &str) -> bool {
    line == "---" || line.starts_with("--- ")
}

fn trailer_block_start(lines: &[String]) -> usize {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && !lines[start - 1].trim().is_empty() {
        start -= 1;
    }
    if start == end || (start > 0 && !lines[start - 1].trim().is_empty()) {
        return lines.len();
    }
    let group = &lines[start..end];
    if trailer_group_is_valid(group) {
        start
    } else {
        lines.len()
    }
}

fn trailer_group_is_valid(lines: &[String]) -> bool {
    if lines.is_empty() {
        return false;
    }
    let entries = parse_trailer_entries(lines);
    if entries.is_empty() {
        return false;
    }
    let entry_line_count = entries.iter().map(|entry| entry.lines.len()).sum::<usize>();
    if entry_line_count == lines.len() {
        return true;
    }
    false
}

fn parse_trailer_entries(lines: &[String]) -> Vec<TrailerEntry> {
    let mut entries: Vec<TrailerEntry> = Vec::new();
    for line in lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(entry) = entries.last_mut() {
                entry.lines.push(line.clone());
                if !entry.value.is_empty() {
                    entry.value.push(' ');
                }
                entry.value.push_str(line.trim());
            }
            continue;
        }
        let Some((key, value)) = split_existing_trailer(line) else {
            continue;
        };
        entries.push(TrailerEntry {
            lines: vec![line.clone()],
            key: key.to_owned(),
            value: value.trim().to_owned(),
        });
    }
    entries
}

fn split_existing_trailer(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim_end_matches([' ', '\t']);
    trailer_key_is_valid(key).then_some((key, value))
}

fn parse_trailer_argument(trailer: &str) -> Result<TrailerEntry> {
    let (key, value) = trailer
        .split_once(':')
        .or_else(|| trailer.split_once('='))
        .unwrap_or((trailer, ""));
    let key = key.trim();
    if !trailer_key_is_valid(key) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid trailer '{trailer}'"),
        });
    }
    let value = value.trim().to_owned();
    Ok(TrailerEntry {
        lines: vec![format!("{key}: {value}")],
        key: key.to_owned(),
        value,
    })
}

fn trailer_key_is_valid(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn apply_trailer_addition(
    entries: &mut Vec<TrailerEntry>,
    addition: TrailerEntry,
    placement: TrailerPlacement,
    if_exists: TrailerIfExists,
    if_missing: TrailerIfMissing,
) {
    let matching_positions = matching_trailer_positions(entries, &addition.key);
    if matching_positions.is_empty() {
        if if_missing == TrailerIfMissing::Add {
            let position = insertion_position(entries, &addition.key, placement);
            entries.insert(position, addition);
        }
        return;
    }

    match if_exists {
        TrailerIfExists::DoNothing => {}
        TrailerIfExists::AddIfDifferent => {
            if !entries
                .iter()
                .any(|entry| same_trailer_pair(entry, &addition))
            {
                let position = insertion_position(entries, &addition.key, placement);
                entries.insert(position, addition);
            }
        }
        TrailerIfExists::AddIfDifferentNeighbor => {
            let position = insertion_position(entries, &addition.key, placement);
            let duplicate_before = position
                .checked_sub(1)
                .and_then(|index| entries.get(index))
                .is_some_and(|entry| same_trailer_pair(entry, &addition));
            let duplicate_after = entries
                .get(position)
                .is_some_and(|entry| same_trailer_pair(entry, &addition));
            if !duplicate_before && !duplicate_after {
                entries.insert(position, addition);
            }
        }
        TrailerIfExists::Add => {
            let position = insertion_position(entries, &addition.key, placement);
            entries.insert(position, addition);
        }
        TrailerIfExists::Replace => {
            let position = insertion_position(entries, &addition.key, placement);
            if let Some(index) = closest_matching_position(&matching_positions, position) {
                entries.remove(index);
            }
            let position = insertion_position(entries, &addition.key, placement);
            entries.insert(position, addition);
        }
    }
}

fn matching_trailer_positions(entries: &[TrailerEntry], key: &str) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| same_trailer_key(&entry.key, key).then_some(index))
        .collect()
}

fn insertion_position(entries: &[TrailerEntry], key: &str, placement: TrailerPlacement) -> usize {
    match placement {
        TrailerPlacement::End => entries.len(),
        TrailerPlacement::Start => 0,
        TrailerPlacement::After => entries
            .iter()
            .rposition(|entry| same_trailer_key(&entry.key, key))
            .map_or(entries.len(), |index| index + 1),
        TrailerPlacement::Before => entries
            .iter()
            .position(|entry| same_trailer_key(&entry.key, key))
            .unwrap_or(entries.len()),
    }
}

fn closest_matching_position(positions: &[usize], insertion_position: usize) -> Option<usize> {
    positions
        .iter()
        .copied()
        .min_by_key(|position| position.abs_diff(insertion_position))
}

fn same_trailer_key(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn same_trailer_pair(left: &TrailerEntry, right: &TrailerEntry) -> bool {
    same_trailer_key(&left.key, &right.key) && left.value.trim() == right.value.trim()
}

fn trailer_output_lines(entries: &[TrailerEntry], unfold: bool) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| {
            if unfold {
                vec![format!("{}: {}", entry.key, entry.value.trim())]
            } else {
                entry.lines.clone()
            }
        })
        .collect()
}

#[derive(Debug, Default)]
struct ForEachRefTrailerFormat {
    only: Option<bool>,
    unfold: bool,
    value_only: bool,
    keys: Vec<String>,
    separator: String,
    key_value_separator: String,
}

pub(crate) fn format_for_each_ref_trailers(message: &str, arguments: &str) -> Result<String> {
    let format = parse_for_each_ref_trailer_format(arguments)?;
    let lines = split_text_lines(message);
    let block_start = for_each_ref_trailer_block_start(&lines);
    if block_start == lines.len() {
        return Ok(String::new());
    }
    let block = &lines[block_start..];
    let entries = parse_trailer_entries(block);
    let mut records = entries
        .iter()
        .filter(|entry| {
            format.keys.is_empty()
                || format
                    .keys
                    .iter()
                    .any(|key| same_trailer_key(&entry.key, key))
        })
        .map(|entry| format_for_each_ref_trailer_entry(entry, &format))
        .collect::<Vec<_>>();

    let only = format.only.unwrap_or(!format.keys.is_empty());
    if !only {
        if format.keys.is_empty() {
            records = if format.unfold {
                unfold_for_each_ref_trailer_block(block, &format)
            } else {
                block.to_vec()
            };
        } else {
            records.extend(block.iter().filter_map(|line| {
                (!line.starts_with([' ', '\t']) && split_existing_trailer(line).is_none())
                    .then(|| line.clone())
            }));
        }
    }
    if records.is_empty() {
        return Ok(String::new());
    }
    let mut output = records.join(&format.separator);
    if format.separator == "\n" {
        output.push('\n');
    }
    Ok(output)
}

fn parse_for_each_ref_trailer_format(arguments: &str) -> Result<ForEachRefTrailerFormat> {
    let mut format = ForEachRefTrailerFormat {
        separator: "\n".to_owned(),
        key_value_separator: ": ".to_owned(),
        ..ForEachRefTrailerFormat::default()
    };
    if arguments.is_empty() {
        return Ok(format);
    }
    for argument in arguments.split(',') {
        match argument {
            "only" => format.only = Some(true),
            "unfold" => format.unfold = true,
            "valueonly" => format.value_only = true,
            argument if argument.starts_with("only=") => {
                format.only = Some(parse_for_each_ref_trailer_bool(&argument[5..])?);
            }
            argument if argument.starts_with("key=") => {
                let key = argument[4..].trim_end_matches(':');
                if key.is_empty() {
                    return Err(for_each_ref_trailer_argument_error(argument));
                }
                format.keys.push(key.to_owned());
            }
            argument if argument.starts_with("separator=") => {
                format.separator = decode_for_each_ref_trailer_separator(&argument[10..])?;
            }
            argument if argument.starts_with("key_value_separator=") => {
                format.key_value_separator =
                    decode_for_each_ref_trailer_separator(&argument[20..])?;
            }
            "key" => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "expected %(trailers:key=<value>)".into(),
                });
            }
            argument => return Err(for_each_ref_trailer_argument_error(argument)),
        }
    }
    Ok(format)
}

fn parse_for_each_ref_trailer_bool(value: &str) -> Result<bool> {
    match value {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        _ => Err(for_each_ref_trailer_argument_error(value)),
    }
}

fn for_each_ref_trailer_argument_error(argument: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("unknown %(trailers) argument: {argument}"),
    }
}

fn decode_for_each_ref_trailer_separator(value: &str) -> Result<String> {
    let mut output = String::new();
    let mut rest = value;
    while let Some(index) = rest.find("%x") {
        output.push_str(&rest[..index]);
        let hex = rest
            .get(index + 2..index + 4)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("invalid trailer separator: {value}"),
            })?;
        let byte = u8::from_str_radix(hex, 16).map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("invalid trailer separator: {value}"),
        })?;
        output.push(char::from(byte));
        rest = &rest[index + 4..];
    }
    output.push_str(rest);
    Ok(output)
}

fn for_each_ref_trailer_block_start(lines: &[String]) -> usize {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && !lines[start - 1].trim().is_empty() {
        start -= 1;
    }
    parse_trailer_entries(&lines[start..end])
        .is_empty()
        .then_some(lines.len())
        .unwrap_or(start)
}

fn format_for_each_ref_trailer_entry(
    entry: &TrailerEntry,
    format: &ForEachRefTrailerFormat,
) -> String {
    if format.value_only {
        return entry.value.trim().to_owned();
    }
    if format.unfold || format.key_value_separator != ": " {
        return format!(
            "{}{}{}",
            entry.key,
            format.key_value_separator,
            entry.value.trim()
        );
    }
    entry.lines.join("\n")
}

fn unfold_for_each_ref_trailer_block(
    block: &[String],
    format: &ForEachRefTrailerFormat,
) -> Vec<String> {
    let mut records = Vec::new();
    let mut index = 0;
    while index < block.len() {
        if split_existing_trailer(&block[index]).is_some() {
            let start = index;
            index += 1;
            while index < block.len() && block[index].starts_with([' ', '\t']) {
                index += 1;
            }
            if let Some(entry) = parse_trailer_entries(&block[start..index]).first() {
                records.push(format_for_each_ref_trailer_entry(entry, format));
            }
        } else {
            records.push(block[index].clone());
            index += 1;
        }
    }
    records
}

pub(crate) fn lines_with_final_newline(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }
}

pub(crate) fn mailsplit(
    precision: Option<usize>,
    first: Option<usize>,
    _keep_from: bool,
    keep_cr: bool,
    output: PathBuf,
    paths: Vec<PathBuf>,
) -> Result<()> {
    let precision = precision.unwrap_or(4);
    let mut next = first.unwrap_or(0) + 1;
    let mut written = 0usize;
    for path in paths {
        let messages = if path.join("cur").is_dir() && path.join("new").is_dir() {
            read_maildir_messages(&path, keep_cr)?
        } else {
            split_mbox_messages(&fs::read(&path)?, keep_cr)?
        };
        for message in messages {
            let filename = format!("{next:0precision$}");
            fs::write(output.join(filename), message)?;
            next += 1;
            written += 1;
        }
    }
    println!("{written}");
    Ok(())
}

fn read_maildir_messages(path: &std::path::Path, keep_cr: bool) -> Result<Vec<Vec<u8>>> {
    let mut messages = Vec::new();
    for dirname in ["cur", "new"] {
        let mut entries =
            fs::read_dir(path.join(dirname))?.collect::<std::result::Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type()?;
            if file_type.is_file() {
                messages.push(normalize_mail_bytes(fs::read(entry.path())?, keep_cr));
            }
        }
    }
    Ok(messages)
}

fn split_mbox_messages(input: &[u8], keep_cr: bool) -> Result<Vec<Vec<u8>>> {
    let input = normalize_mail_bytes(input.to_vec(), keep_cr);
    if input.is_empty() || !input.starts_with(b"From ") {
        println!("corrupt mailbox");
        return Err(CliError::Exit(1));
    }
    let mut starts = vec![0usize];
    let mut index = 0usize;
    while let Some(relative) = find_bytes(&input[index..], b"\nFrom ") {
        let start = index + relative + 1;
        starts.push(start);
        index = start + 1;
    }
    starts.push(input.len());
    let messages = starts
        .windows(2)
        .map(|window| input[window[0]..window[1]].to_vec())
        .collect();
    Ok(messages)
}

fn normalize_mail_bytes(mut input: Vec<u8>, keep_cr: bool) -> Vec<u8> {
    if keep_cr {
        return input;
    }
    input.retain(|byte| *byte != b'\r');
    input
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(crate) fn mailinfo(
    keep_subject: bool,
    keep_non_patch_brackets: bool,
    message_id: bool,
    msg: PathBuf,
    patch: PathBuf,
) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let (headers, body) = split_mail_headers(&input);
    let header_map = parse_mail_headers(headers);
    let (author, email) = parse_mail_author(header_map.get("from").map_or("", String::as_str));
    let subject = header_map
        .get("subject")
        .map(|value| clean_mail_subject(value, keep_subject, keep_non_patch_brackets))
        .unwrap_or_default();
    let (message, patch_text) = split_mail_body_patch(body);
    let mut message_lines = message;
    if message_id && let Some(id) = header_map.get("message-id") {
        message_lines = message_lines.trim_end_matches('\n').to_owned();
        if !message_lines.is_empty() {
            message_lines.push_str("\n\n");
        }
        message_lines.push_str("Message-ID: ");
        message_lines.push_str(id.trim());
        message_lines.push('\n');
    }
    fs::write(msg, message_lines)?;
    fs::write(patch, patch_text)?;

    println!("Author: {author}");
    println!("Email: {email}");
    println!("Subject: {subject}");
    if let Some(date) = header_map.get("date") {
        println!("Date: {}", date.trim());
    }
    println!();
    Ok(())
}

pub(crate) fn split_mail_headers(input: &str) -> (&str, &str) {
    input
        .split_once("\n\n")
        .map_or((input, ""), |(headers, body)| (headers, body))
}

pub(crate) fn parse_mail_headers(headers: &str) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut current_key: Option<String> = None;
    for line in headers.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = &current_key
                && let Some(value) = map.get_mut(key)
            {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let normalized_key = key.trim().to_ascii_lowercase();
        current_key = Some(normalized_key.clone());
        map.insert(normalized_key, value.trim().to_owned());
    }
    map
}

pub(crate) fn parse_mail_author(from: &str) -> (String, String) {
    if let Some(start) = from.rfind('<')
        && let Some(end) = from[start + 1..].find('>')
    {
        let end = start + 1 + end;
        let name = from[..start].trim().trim_matches('"').to_owned();
        let email = from[start + 1..end].trim().to_owned();
        return (name, email);
    }
    (from.trim().to_owned(), from.trim().to_owned())
}

pub(crate) fn clean_mail_subject(
    subject: &str,
    keep_subject: bool,
    keep_non_patch_brackets: bool,
) -> String {
    if keep_subject {
        return subject.trim().to_owned();
    }
    let mut remaining = subject.trim();
    let mut removed_patch = false;
    while let Some(rest) = remaining.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            break;
        };
        let bracket = &rest[..end];
        let after = rest[end + 1..].trim_start();
        let is_patch = bracket
            .split_whitespace()
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("patch"));
        if is_patch {
            removed_patch = true;
            remaining = after;
            continue;
        }
        if keep_non_patch_brackets && removed_patch {
            break;
        }
        remaining = after;
    }
    remaining.to_owned()
}

pub(crate) fn split_mail_body_patch(body: &str) -> (String, String) {
    let mut message = Vec::new();
    let mut patch = Vec::new();
    let mut in_patch = false;
    for line in body.lines() {
        if !in_patch && (line == "---" || line.starts_with("diff --git ")) {
            in_patch = true;
        }
        if in_patch {
            patch.push(line.to_owned());
        } else {
            message.push(line.to_owned());
        }
    }
    let mut message_text = message.join("\n");
    if !message_text.is_empty() {
        message_text.push('\n');
    }
    let mut patch_text = patch.join("\n");
    if !patch_text.is_empty() {
        patch_text.push('\n');
    }
    (message_text, patch_text)
}

pub(crate) fn fmt_merge_msg(
    log: Option<usize>,
    no_log: bool,
    message: Option<&str>,
    into_name: Option<&str>,
    file: Option<PathBuf>,
) -> Result<()> {
    let input = if let Some(path) = file {
        if path.as_os_str() == "-" {
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            input
        } else {
            fs::read_to_string(path)?
        }
    } else {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        input
    };
    let entries = parse_fetch_head_for_merge(&input);
    if let Some(message) = message {
        println!("{message}");
        return Ok(());
    }
    if entries.is_empty() {
        return Ok(());
    }

    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let current = current_branch_ref(&refs)?
        .map(|name| branch_display_name(&name))
        .unwrap_or_else(|| "HEAD".to_owned());
    let target = into_name.unwrap_or(&current);
    let mut title = fmt_merge_title(&entries);
    if target != current {
        title.push_str(" into ");
        title.push_str(target);
    }
    println!("{title}");
    if log.is_some() && !no_log {
        let store = LooseObjectStore::new(repo.objects_dir, GitHashAlgorithm::Sha1);
        println!();
        for source in entries {
            let object = store.read_object(&source.oid)?;
            let commit = decode_commit(GitHashAlgorithm::Sha1, object.content.as_slice())?;
            println!("# By {}", signature_name(&commit.author));
            println!("# Via {}", signature_name(&commit.committer));
            println!("* {}:", fmt_merge_log_description(&source.description));
            if let Some(subject) = commit.message.split(|byte| *byte == b'\n').next() {
                if !subject.is_empty() {
                    println!("  {}", String::from_utf8_lossy(subject));
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FetchHeadMergeEntry {
    oid: ObjectId,
    description: String,
}

fn parse_fetch_head_for_merge(input: &str) -> Vec<FetchHeadMergeEntry> {
    input
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let oid = ObjectId::from_hex(GitHashAlgorithm::Sha1, parts.next()?).ok()?;
            let marker = parts.next().unwrap_or_default();
            let description = parts.next().unwrap_or_default().trim();
            if marker == "not-for-merge" || description.is_empty() {
                return None;
            }
            Some(FetchHeadMergeEntry {
                oid,
                description: description.to_owned(),
            })
        })
        .collect()
}

fn fmt_merge_title(entries: &[FetchHeadMergeEntry]) -> String {
    if entries.len() == 1 {
        return format!("Merge {}", entries[0].description);
    }
    let descriptions = entries
        .iter()
        .map(|entry| entry.description.as_str())
        .collect::<Vec<_>>();
    format!("Merge {}", join_english_list(&descriptions))
}

fn fmt_merge_log_description(description: &str) -> &str {
    description.strip_prefix("branch ").unwrap_or(description)
}

fn join_english_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [first, second] => format!("{first} and {second}"),
        _ => {
            let mut out = items[..items.len() - 1].join(", ");
            out.push_str(", and ");
            out.push_str(items[items.len() - 1]);
            out
        }
    }
}
