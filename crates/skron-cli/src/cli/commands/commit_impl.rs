use super::*;

pub(crate) struct CommitCommandOptions<'a> {
    pub(crate) all: bool,
    pub(crate) only: bool,
    pub(crate) allow_empty: bool,
    pub(crate) amend: bool,
    pub(crate) edit: bool,
    pub(crate) no_edit: bool,
    pub(crate) signoff: bool,
    pub(crate) quiet: bool,
    pub(crate) verbose: u8,
    pub(crate) no_verify: bool,
    pub(crate) status: bool,
    pub(crate) no_status: bool,
    pub(crate) cleanup: Option<&'a str>,
    pub(crate) no_cleanup: bool,
    pub(crate) allow_empty_message: bool,
    pub(crate) author_override: Option<&'a str>,
    pub(crate) date_override: Option<&'a str>,
    pub(crate) squash: Option<&'a str>,
    pub(crate) template: Option<&'a Path>,
    pub(crate) reset_author: bool,
    pub(crate) reuse_message: Option<&'a str>,
    pub(crate) reedit_message: Option<&'a str>,
    pub(crate) fixup: Option<&'a str>,
    pub(crate) message_file: Option<&'a Path>,
    pub(crate) messages: Vec<String>,
    pub(crate) trailers: Vec<String>,
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) fn commit_command(options: CommitCommandOptions<'_>) -> Result<()> {
    commit(options)
}

pub(crate) fn citool_command(
    amend: bool,
    nocommit: bool,
    message_file: Option<&Path>,
    messages: Vec<String>,
) -> Result<()> {
    citool(amend, nocommit, message_file, messages)
}

pub(crate) fn gui_command(args: Vec<String>) -> Result<()> {
    gui(args)
}

pub(crate) fn write_tree_command_entry() -> Result<()> {
    write_tree_command()
}

pub(crate) fn commit_tree_command(
    tree: &str,
    parents: Vec<String>,
    messages: Vec<String>,
) -> Result<()> {
    commit_tree(tree, parents, messages)
}

pub(crate) fn mktree_command(nul_terminated: bool, missing: bool, batch: bool) -> Result<()> {
    mktree(nul_terminated, missing, batch)
}

fn commit(options: CommitCommandOptions<'_>) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut index = read_repo_index(&repo)?;
    if options.all && (!options.paths.is_empty() || options.only) {
        return Err(CliError::Fatal {
            code: 128,
            message: "paths cannot be used with -a".into(),
        });
    }
    if options.reuse_message.is_some() && !options.messages.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-m' and '-C' cannot be used together".into(),
        });
    }
    if options.reedit_message.is_some() && !options.messages.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-m' and '-c' cannot be used together".into(),
        });
    }
    if options.reuse_message.is_some() && options.reedit_message.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-C' and '-c' cannot be used together".into(),
        });
    }
    let fixup_options = options.fixup.map(parse_commit_fixup_option).transpose()?;
    if fixup_options.is_some() && options.reuse_message.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--fixup' and '-C' cannot be used together".into(),
        });
    }
    if fixup_options.is_some() && options.reedit_message.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-c' and '--fixup' cannot be used together".into(),
        });
    }
    if let Some(fixup) = fixup_options.as_ref()
        && !options.messages.is_empty()
        && matches!(fixup.mode, CommitFixupMode::Amend | CommitFixupMode::Reword)
    {
        let mode = match fixup.mode {
            CommitFixupMode::Amend => "amend",
            CommitFixupMode::Reword => "reword",
            CommitFixupMode::Fixup => "fixup",
        };
        return Err(CliError::Fatal {
            code: 128,
            message: format!("options '-m' and '--fixup:{mode}' cannot be used together"),
        });
    }
    if fixup_options.is_some() && options.squash.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--squash' and '--fixup' cannot be used together".into(),
        });
    }
    if options.message_file.is_some() && !options.messages.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-m' and '-F' cannot be used together".into(),
        });
    }
    if options.reuse_message.is_some() && options.message_file.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-C' and '-F' cannot be used together".into(),
        });
    }
    if options.reedit_message.is_some() && options.message_file.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '-c' and '-F' cannot be used together".into(),
        });
    }
    if options.reset_author && options.author_override.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--reset-author' and '--author' cannot be used together".into(),
        });
    }
    if options.reset_author
        && !options.amend
        && options.reuse_message.is_none()
        && options.reedit_message.is_none()
    {
        return Err(CliError::Fatal {
            code: 128,
            message: "--reset-author can be used only with -C, -c or --amend.".into(),
        });
    }
    if options.all {
        stage_tracked_worktree_changes(&repo, &store, &mut index)?;
        index.write_to_path(&repo.index_path)?;
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let reused_commit = match options.reuse_message.or(options.reedit_message) {
        Some(rev) => {
            let id = resolve_commitish(&repo, &store, rev).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("could not lookup commit '{rev}'"),
            })?;
            Some(commit_cache.read_commit(&id)?)
        }
        None => None,
    };
    let squashed_commit = match options.squash {
        Some(rev) => {
            let id = resolve_commitish(&repo, &store, rev).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("could not lookup commit '{rev}'"),
            })?;
            Some(commit_cache.read_commit(&id)?)
        }
        None => None,
    };
    let fixup_commit = match fixup_options.as_ref() {
        Some(fixup) => {
            let rev = fixup.rev;
            let id = resolve_commitish(&repo, &store, rev).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("could not lookup commit '{rev}'"),
            })?;
            Some(commit_cache.read_commit(&id)?)
        }
        None => None,
    };
    let pathspec_commit = (!options.paths.is_empty())
        .then(|| commit_pathspec_indexes(&repo, &store, &index, &options.paths))
        .transpose()?;
    let fixup_reword_index;
    let commit_index = if matches!(
        fixup_options.as_ref().map(|fixup| fixup.mode),
        Some(CommitFixupMode::Reword)
    ) {
        fixup_reword_index = read_head_index(&repo)?;
        &fixup_reword_index
    } else {
        pathspec_commit
            .as_ref()
            .map(|indexes| &indexes.commit_index)
            .unwrap_or(&index)
    };
    let tree = write_tree_from_index(&store, commit_index)?;
    let reused_author = reused_commit
        .as_ref()
        .map(|commit| signature_from_commit_bytes(&commit.author))
        .transpose()?;
    let explicit_reused_message = options
        .reuse_message
        .and_then(|_| reused_commit.as_ref().map(|commit| commit.message.clone()));
    let reedit_message = options
        .reedit_message
        .and_then(|_| reused_commit.as_ref().map(|commit| commit.message.clone()));
    let mut parents = Vec::new();
    let mut summary_parent_tree = None;
    let mut amended_head = None;
    let mut reused_message = None;
    let committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
    let author = if options.amend {
        let head = refs.resolve("HEAD").map_err(|_| CliError::Fatal {
            code: 128,
            message: "You have nothing to amend.".into(),
        })?;
        let head_commit = commit_cache.read_commit(&head)?;
        amended_head = Some(head);
        summary_parent_tree = head_commit
            .parents
            .first()
            .cloned()
            .map(|parent_id| {
                commit_cache
                    .read_commit(&parent_id)
                    .map(|parent| parent.tree.clone())
            })
            .transpose()?;
        if options.no_edit
            && options.messages.is_empty()
            && options.message_file.is_none()
            && options.reedit_message.is_none()
        {
            reused_message = Some(head_commit.message.clone());
        }
        parents = head_commit.parents.clone();
        let previous_author = signature_from_commit_bytes(&head_commit.author)?;
        let base_author = if options.reset_author {
            None
        } else {
            Some(reused_author.as_ref().unwrap_or(&previous_author))
        };
        if options.reset_author
            || options.author_override.is_some()
            || options.date_override.is_some()
        {
            signature_from_author_options(
                &repo,
                base_author,
                options.author_override,
                options.date_override,
            )?
        } else if let Some(author) = reused_author {
            author
        } else {
            previous_author
        }
    } else if let Ok(parent) = refs.resolve("HEAD") {
        let parent_commit = commit_cache.read_commit(&parent)?;
        summary_parent_tree = Some(parent_commit.tree.clone());
        if parent_commit.tree == tree
            && !options.allow_empty
            && !matches!(
                fixup_options.as_ref().map(|fixup| fixup.mode),
                Some(CommitFixupMode::Reword)
            )
        {
            return Err(CliError::Message(
                "nothing to commit, working tree clean".into(),
            ));
        }
        parents.push(parent);
        let base_author = if options.reset_author {
            None
        } else {
            reused_author.as_ref()
        };
        signature_from_author_options(
            &repo,
            base_author,
            options.author_override,
            options.date_override,
        )?
    } else if index.entries().is_empty() && !options.allow_empty {
        return Err(CliError::Message("nothing to commit".into()));
    } else {
        let base_author = if options.reset_author {
            None
        } else {
            reused_author.as_ref()
        };
        signature_from_author_options(
            &repo,
            base_author,
            options.author_override,
            options.date_override,
        )?
    };
    let cleanup_mode = commit_cleanup_mode(options.cleanup, options.no_cleanup)?;
    let template_message = if let Some(path) = options.template {
        Some(read_commit_message_file(path)?)
    } else {
        read_commit_template_config(&repo)?
    };
    let force_edit = options.edit || (options.reedit_message.is_some() && !options.no_edit);
    let has_direct_message = !options.messages.is_empty()
        || options.message_file.is_some()
        || explicit_reused_message.is_some()
        || reused_message.is_some()
        || (fixup_commit.is_some()
            && !matches!(
                fixup_options.as_ref().map(|fixup| fixup.mode),
                Some(CommitFixupMode::Amend | CommitFixupMode::Reword)
            ));
    let uses_editor = force_edit
        || (!options.no_edit
            && !has_direct_message
            && (template_message.is_some() || !options.allow_empty_message));
    let editor_date = if options.reedit_message.is_some()
        || (options.edit && options.reuse_message.is_some())
        || options.date_override.is_some()
        || (options.amend && !options.reset_author)
    {
        Some(&author)
    } else {
        None
    };
    let editor_status = commit_status_enabled(&repo, options.status, options.no_status)?;
    let editor_status_index = if matches!(
        fixup_options.as_ref().map(|fixup| fixup.mode),
        Some(CommitFixupMode::Reword)
    ) {
        commit_index
    } else {
        &index
    };
    let editor_message = if uses_editor {
        Some(if editor_status {
            commit_editor_message(
                &repo,
                &store,
                summary_parent_tree.as_ref(),
                commit_index,
                editor_status_index,
                options.verbose,
                editor_date,
            )?
        } else {
            Vec::new()
        })
    } else {
        None
    };
    let fixup_direct_message = fixup_commit.as_ref().and_then(|commit| {
        fixup_options
            .as_ref()
            .and_then(|fixup| fixup_direct_message(commit, fixup.mode, &options.messages))
    });
    let commit_messages = if fixup_direct_message.is_some() {
        Vec::new()
    } else {
        options.messages
    };
    let mut message = commit_message_bytes(CommitMessageInput {
        repo: &repo,
        messages: commit_messages,
        file_message: options
            .message_file
            .map(read_commit_message_file)
            .transpose()?,
        reused_message: explicit_reused_message
            .or(reused_message)
            .or(fixup_direct_message),
        reedit_message: fixup_commit
            .as_ref()
            .and_then(|commit| {
                fixup_options
                    .as_ref()
                    .and_then(|fixup| fixup_reedit_message(commit, fixup.mode))
            })
            .or(reedit_message),
        template_message,
        trailers: &options.trailers,
        editor_message,
        force_edit,
        allow_empty_message: options.allow_empty_message,
        cleanup: cleanup_mode,
    })?;
    if let Some(squashed) = squashed_commit.as_ref() {
        message = squash_commit_message(&commit_subject(&squashed.message), message);
    }
    if options.signoff {
        append_commit_signoff(&mut message, &committer)?;
    }
    if !options.trailers.is_empty() && !uses_editor {
        message = append_commit_trailers(message, &options.trailers)?;
    }
    if !options.no_verify {
        run_commit_hook(&repo, "pre-commit", &[], None, false)?;
    }
    write_commit_editmsg(&repo, &message)?;
    run_commit_hook(
        &repo,
        "prepare-commit-msg",
        &[".git/COMMIT_EDITMSG", "message"],
        None,
        false,
    )?;
    if !options.no_verify {
        run_commit_hook(&repo, "commit-msg", &[".git/COMMIT_EDITMSG"], None, false)?;
    }
    message = fs::read(repo.git_dir.join("COMMIT_EDITMSG"))?;
    if !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    message = if uses_editor {
        let has_scissors = editor_message_has_scissors(&message);
        cleanup_edited_commit_message(message, cleanup_mode, has_scissors)
    } else {
        cleanup_commit_message(message, cleanup_mode)
    };
    let mut builder = CommitBuilder::new(tree.clone(), author.clone(), committer);
    for parent in parents {
        builder = builder.parent(parent);
    }
    let commit = builder.message(message.clone())?.encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    if amended_head.as_ref() == Some(&id) && !options.allow_empty {
        return Err(CliError::Fatal {
            code: 128,
            message: "You have nothing to amend.".into(),
        });
    }
    let reflog_subject = commit_subject(&message);
    let reflog_message = if options.amend {
        format!("commit (amend): {reflog_subject}")
    } else {
        format!("commit: {reflog_subject}")
    };
    update_head_to_commit_with_reflog(&repo, &refs, &id, &reflog_message)?;
    if let Some(pathspec_commit) = pathspec_commit
        && !matches!(
            fixup_options.as_ref().map(|fixup| fixup.mode),
            Some(CommitFixupMode::Reword)
        )
    {
        pathspec_commit.real_index.write_to_path(&repo.index_path)?;
    }
    let summary_date_author = if options.amend
        || options.reuse_message.is_some()
        || options.reedit_message.is_some()
        || options.date_override.is_some()
    {
        Some(&author)
    } else {
        None
    };
    if !options.quiet {
        print_commit_summary(
            &repo,
            &store,
            &id,
            &message,
            summary_parent_tree.as_ref(),
            &tree,
            summary_date_author,
        )?;
    }
    run_commit_hook(&repo, "post-commit", &[], None, true)?;
    if let Some(old_id) = amended_head {
        let post_rewrite_stdin = format!("{} {}\n", old_id.to_hex(), id.to_hex());
        run_commit_hook(
            &repo,
            "post-rewrite",
            &["amend"],
            Some(post_rewrite_stdin.as_bytes()),
            true,
        )?;
    }
    Ok(())
}

struct CommitPathspecIndexes {
    commit_index: GitIndex,
    real_index: GitIndex,
}

fn commit_pathspec_indexes(
    repo: &GitRepo,
    store: &LooseObjectStore,
    real_index: &GitIndex,
    paths: &[PathBuf],
) -> Result<CommitPathspecIndexes> {
    let mut commit_index = read_head_index(repo)?;
    let mut updated_real_index = real_index.clone();
    for path in paths {
        let pathspec = path_arg_to_repo_relative(repo, path)?;
        let mut matches = matching_index_entries(&commit_index, &pathspec);
        matches.extend(matching_index_entries(real_index, &pathspec));
        matches.sort_by(|left, right| left.path.cmp(&right.path));
        matches.dedup_by(|left, right| left.path == right.path);
        if matches.is_empty() {
            return Err(worktree_commands::unmatched_restore_pathspec_error(
                std::slice::from_ref(&pathspec),
            ));
        }
        for entry in matches {
            let absolute = repo
                .root
                .join(String::from_utf8_lossy(&entry.path).as_ref());
            if path_exists(&absolute) {
                stage_file(repo, store, &mut commit_index, &absolute)?;
                stage_file(repo, store, &mut updated_real_index, &absolute)?;
            } else {
                commit_index.remove_path(&entry.path)?;
                updated_real_index.remove_path(&entry.path)?;
            }
        }
    }
    Ok(CommitPathspecIndexes {
        commit_index,
        real_index: updated_real_index,
    })
}

fn print_commit_summary(
    repo: &GitRepo,
    store: &LooseObjectStore,
    id: &ObjectId,
    message: &[u8],
    parent_tree: Option<&ObjectId>,
    tree: &ObjectId,
    amend_author: Option<&Signature>,
) -> Result<()> {
    let branch = commit_summary_branch(repo)?;
    let root = if parent_tree.is_none() && amend_author.is_none() {
        " (root-commit)"
    } else {
        ""
    };
    println!(
        "[{branch}{root} {}] {}",
        short_object_id(id),
        commit_summary_subject(message)
    );
    if let Some(author) = amend_author {
        println!(" Date: {}", signature_summary_date(author)?);
    }
    let tree_cache = TreeObjectCache::new(store);
    let old_index = match parent_tree {
        Some(parent_tree) => tree_cache.read_tree_to_index(parent_tree)?,
        None => GitIndex::new(),
    };
    let new_index = tree_cache.read_tree_to_index(tree)?;
    let entries = diff_indexes(&old_index, &new_index)?;
    print_commit_shortstat(repo, store, &old_index, &new_index, &entries)?;
    print_summary_entries(&old_index, &new_index, &entries, None)?;
    Ok(())
}

fn signature_summary_date(signature: &Signature) -> Result<String> {
    let offset = parse_timezone_offset(&signature.timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let utc = chrono::DateTime::from_timestamp(signature.timestamp, 0).ok_or_else(|| {
        CliError::Fatal {
            code: 128,
            message: "commit author timestamp is out of range".into(),
        }
    })?;
    Ok(utc
        .with_timezone(&offset)
        .format("%a %b %-d %H:%M:%S %Y %z")
        .to_string())
}

fn commit_summary_branch(repo: &GitRepo) -> Result<String> {
    let raw = fs::read_to_string(repo.git_dir.join("HEAD")).unwrap_or_default();
    if let Some(name) = raw
        .trim_end_matches('\n')
        .strip_prefix("ref: ")
        .map(str::to_owned)
    {
        return Ok(name
            .strip_prefix("refs/heads/")
            .unwrap_or(name.as_str())
            .to_owned());
    }
    if !raw.trim().is_empty() {
        Ok("detached HEAD".to_owned())
    } else {
        Ok("HEAD".to_owned())
    }
}

fn print_commit_shortstat(
    repo: &GitRepo,
    store: &LooseObjectStore,
    old_index: &GitIndex,
    new_index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
) -> Result<()> {
    let context = DiffIndexContext {
        repo,
        store,
        old_index,
        new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let rows = diff_stat_rows_with_whitespace(
        &context,
        entries,
        DiffStatOptions {
            whitespace_mode: DiffWhitespaceMode::None,
            relative_prefix: None,
            ignore_matching_lines: &[],
        },
    )?;
    if !rows.is_empty() {
        print_diff_stat_summary(&rows);
    }
    Ok(())
}

fn commit_summary_subject(message: &[u8]) -> String {
    let text = String::from_utf8_lossy(message);
    text.lines()
        .take_while(|line| !line.trim().is_empty())
        .flat_map(|line| line.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_commit_editmsg(repo: &GitRepo, message: &[u8]) -> Result<()> {
    let path = repo.git_dir.join("COMMIT_EDITMSG");
    fs::write(path, message)?;
    Ok(())
}

fn run_commit_hook(
    repo: &GitRepo,
    hook_name: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
    ignore_failure: bool,
) -> Result<()> {
    let Some(hook_path) = commit_hook_path(repo, hook_name)? else {
        return Ok(());
    };
    let mut child = ProcessCommand::new(&hook_path)
        .args(args)
        .current_dir(&repo.root)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = stdin {
        child
            .stdin
            .as_mut()
            .ok_or_else(|| CliError::Fatal {
                code: 1,
                message: format!("failed to capture stdin for hook '{}'", hook_path.display()),
            })?
            .write_all(stdin)?;
    }
    let output = child.wait_with_output()?;
    io::stderr().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() && !ignore_failure {
        return Err(CliError::Exit(1));
    }
    Ok(())
}

fn commit_hook_path(repo: &GitRepo, hook_name: &str) -> Result<Option<PathBuf>> {
    let hooks_dir = match read_config_value(repo, "core.hooksPath")? {
        Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
        Some(path) => repo.root.join(path),
        None => repo.git_dir.join("hooks"),
    };
    let hook_path = hooks_dir.join(hook_name);
    if hook_path.is_file() && admin_commands::hook_is_executable(&hook_path)? {
        Ok(Some(hook_path))
    } else {
        Ok(None)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommitFixupMode {
    Fixup,
    Amend,
    Reword,
}

struct CommitFixupOption<'a> {
    mode: CommitFixupMode,
    rev: &'a str,
}

fn parse_commit_fixup_option(value: &str) -> Result<CommitFixupOption<'_>> {
    if let Some(rev) = value.strip_prefix("amend:") {
        if rev.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "could not lookup commit ''".into(),
            });
        }
        return Ok(CommitFixupOption {
            mode: CommitFixupMode::Amend,
            rev,
        });
    }
    if let Some(rev) = value.strip_prefix("reword:") {
        if rev.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "could not lookup commit ''".into(),
            });
        }
        return Ok(CommitFixupOption {
            mode: CommitFixupMode::Reword,
            rev,
        });
    }
    if let Some((mode, _)) = value.split_once(':') {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "unknown option: --fixup={mode}:{}",
                &value[mode.len() + 1..]
            ),
        });
    }
    Ok(CommitFixupOption {
        mode: CommitFixupMode::Fixup,
        rev: value,
    })
}

fn fixup_direct_message(
    commit: &skron_git_core::CommitObject,
    mode: CommitFixupMode,
    messages: &[String],
) -> Option<Vec<u8>> {
    if mode != CommitFixupMode::Fixup {
        return None;
    }
    let mut message = format!("fixup! {}\n", commit_subject(&commit.message)).into_bytes();
    for extra in messages {
        message.push(b'\n');
        message.extend_from_slice(extra.as_bytes());
        message.push(b'\n');
    }
    message.push(b'\n');
    Some(message)
}

fn fixup_reedit_message(
    commit: &skron_git_core::CommitObject,
    mode: CommitFixupMode,
) -> Option<Vec<u8>> {
    if !matches!(mode, CommitFixupMode::Amend | CommitFixupMode::Reword) {
        return None;
    }
    let mut message = format!("amend! {}\n", commit_subject(&commit.message)).into_bytes();
    if commit
        .message
        .iter()
        .any(|byte| !matches!(byte, b' ' | b'\n' | b'\t' | b'\r'))
    {
        message.push(b'\n');
        message.extend_from_slice(&commit.message);
        if !message.ends_with(b"\n") {
            message.push(b'\n');
        }
    }
    Some(message)
}

fn commit_cleanup_mode(cleanup: Option<&str>, no_cleanup: bool) -> Result<CommitCleanupMode> {
    let mode = match cleanup {
        Some(raw_mode) => match raw_mode.to_ascii_lowercase().as_str() {
            "strip" => CommitCleanupMode::Strip,
            "whitespace" => CommitCleanupMode::Whitespace,
            "verbatim" => CommitCleanupMode::Verbatim,
            "scissors" => CommitCleanupMode::Scissors,
            "default" => CommitCleanupMode::Default,
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("Invalid cleanup mode {raw_mode}"),
                });
            }
        },
        None if no_cleanup => CommitCleanupMode::Whitespace,
        None => CommitCleanupMode::Default,
    };
    Ok(mode)
}

pub(crate) fn strip_commit_message_line_whitespace(line: &[u8]) -> &[u8] {
    let new_len = line
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|idx| idx + 1)
        .unwrap_or(0);
    &line[..new_len]
}

pub(crate) fn is_commit_message_line_blank(line: &[u8]) -> bool {
    line.iter().all(|byte| byte.is_ascii_whitespace())
}

fn append_commit_signoff(message: &mut Vec<u8>, committer: &Signature) -> Result<()> {
    if message.iter().all(|byte| byte.is_ascii_whitespace()) {
        message.extend_from_slice(
            format!("Signed-off-by: {} <{}>", committer.name, committer.email).as_bytes(),
        );
        message.push(b'\n');
        return Ok(());
    }
    message.extend_from_slice(b"\nSigned-off-by: ");
    message.extend_from_slice(committer.name.as_bytes());
    message.push(b' ');
    message.push(b'<');
    message.extend_from_slice(committer.email.as_bytes());
    message.push(b'>');
    message.push(b'\n');
    Ok(())
}

fn append_commit_trailers(message: Vec<u8>, trailers: &[String]) -> Result<Vec<u8>> {
    let input = String::from_utf8_lossy(&message);
    Ok(interpret_trailers_content(
        &input,
        &InterpretTrailersOptions {
            in_place: false,
            trim_empty: false,
            where_: None,
            if_exists: None,
            if_missing: None,
            only_trailers: false,
            only_input: false,
            unfold: false,
            no_divider: false,
            trailers: trailers.to_vec(),
            files: Vec::new(),
        },
    )?
    .into_bytes())
}

fn squash_commit_message(squash_subject: &str, mut message: Vec<u8>) -> Vec<u8> {
    let mut squashed_message = format!("squash! {squash_subject}\n").into_bytes();
    if message
        .iter()
        .any(|byte| !matches!(byte, b' ' | b'\n' | b'\t' | b'\r'))
    {
        squashed_message.push(b'\n');
        if !message.is_empty() {
            squashed_message.append(&mut message);
        }
    }
    squashed_message
}

fn citool(
    amend: bool,
    nocommit: bool,
    message_file: Option<&std::path::Path>,
    messages: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let index = read_repo_index(&repo)?;
    let unmerged = merge_index_unmerged_paths(&index);
    if !unmerged.is_empty() {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "cannot commit because unmerged files exist: {}",
                unmerged
                    .iter()
                    .map(|path| String::from_utf8_lossy(path).into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if nocommit {
        return Ok(());
    }
    commit(CommitCommandOptions {
        all: false,
        only: false,
        amend,
        allow_empty: false,
        edit: false,
        no_edit: amend && messages.is_empty() && message_file.is_none(),
        signoff: false,
        quiet: false,
        verbose: 0,
        no_verify: false,
        status: false,
        no_status: false,
        cleanup: None,
        no_cleanup: false,
        allow_empty_message: false,
        author_override: None,
        date_override: None,
        reset_author: false,
        squash: None,
        template: None,
        reuse_message: None,
        reedit_message: None,
        fixup: None,
        message_file,
        messages,
        trailers: Vec::new(),
        paths: Vec::new(),
    })
}

fn gui(args: Vec<String>) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("citool");
    match command {
        "version" | "--version" => {
            println!("git-gui version {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "citool" => gui_citool(&args[1..]),
        "browser" => {
            let treeish = args.get(1).map(String::as_str).unwrap_or("HEAD");
            reference_commands::ls_tree_command(true, true, treeish, Vec::new())
        }
        "blame" => blame(false, false, false, args[1..].to_vec()),
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported gui command '{command}'"),
        }),
    }
}

fn gui_citool(args: &[String]) -> Result<()> {
    let mut amend = false;
    let mut nocommit = false;
    let mut message_file = None;
    let mut messages = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--amend" => amend = true,
            "--nocommit" => nocommit = true,
            "-F" | "--file" => {
                message_file = Some(PathBuf::from(next_borrowed_option_value(&mut iter, arg)?))
            }
            "-m" => messages.push(next_borrowed_option_value(&mut iter, "-m")?.to_owned()),
            _ if arg.starts_with("-m") && arg.len() > 2 => messages.push(arg[2..].to_owned()),
            _ if arg.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported gui citool option '{arg}'"),
                });
            }
            _ => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported gui citool argument '{arg}'"),
                });
            }
        }
    }
    citool(amend, nocommit, message_file.as_deref(), messages)
}

struct CommitMessageInput<'a> {
    repo: &'a GitRepo,
    messages: Vec<String>,
    file_message: Option<Vec<u8>>,
    reused_message: Option<Vec<u8>>,
    reedit_message: Option<Vec<u8>>,
    template_message: Option<Vec<u8>>,
    trailers: &'a [String],
    editor_message: Option<Vec<u8>>,
    force_edit: bool,
    allow_empty_message: bool,
    cleanup: CommitCleanupMode,
}

fn commit_message_bytes(input: CommitMessageInput<'_>) -> Result<Vec<u8>> {
    let CommitMessageInput {
        repo,
        messages,
        file_message,
        reused_message,
        reedit_message,
        template_message,
        trailers,
        editor_message,
        force_edit,
        allow_empty_message,
        cleanup,
    } = input;
    if !messages.is_empty() {
        let mut message = messages.join("\n\n").into_bytes();
        message.push(b'\n');
        if !trailers.is_empty() && force_edit {
            message = append_commit_trailers(message, trailers)?;
        }
        if force_edit {
            return edit_commit_message(
                repo,
                message,
                editor_message,
                false,
                allow_empty_message,
                cleanup,
            );
        }
        return Ok(cleanup_commit_message(message, cleanup));
    }
    if let Some(mut message) = file_message {
        if !message.ends_with(b"\n") {
            message.push(b'\n');
        }
        if !trailers.is_empty() && force_edit {
            message = append_commit_trailers(message, trailers)?;
        }
        if force_edit {
            return edit_commit_message(
                repo,
                message,
                editor_message,
                false,
                allow_empty_message,
                cleanup,
            );
        }
        return Ok(cleanup_commit_message(message, cleanup));
    }
    if let Some(mut message) = reused_message {
        if !message.ends_with(b"\n") {
            message.push(b'\n');
        }
        if !trailers.is_empty() && force_edit {
            message = append_commit_trailers(message, trailers)?;
        }
        if force_edit {
            return edit_commit_message(
                repo,
                message,
                editor_message,
                false,
                allow_empty_message,
                cleanup,
            );
        }
        return Ok(message);
    }
    if let Some(mut message) = reedit_message {
        if !message.ends_with(b"\n") {
            message.push(b'\n');
        }
        if !trailers.is_empty() {
            message = append_commit_trailers(message, trailers)?;
        }
        if editor_message.is_none() {
            return Ok(message);
        }
        return edit_commit_message(
            repo,
            message,
            editor_message,
            false,
            allow_empty_message,
            cleanup,
        );
    }
    if let Some(mut message) = template_message {
        if !message.ends_with(b"\n") {
            message.push(b'\n');
        }
        if !trailers.is_empty() {
            message = append_commit_trailers(message, trailers)?;
        }
        return edit_commit_message(
            repo,
            message,
            editor_message,
            true,
            allow_empty_message,
            cleanup,
        );
    }
    if let Some(editor_message) = editor_message {
        let message = if trailers.is_empty() {
            Vec::new()
        } else {
            editor_only_commit_trailers(trailers)?
        };
        return edit_commit_message(
            repo,
            message,
            Some(editor_message),
            false,
            allow_empty_message,
            cleanup,
        );
    }
    if allow_empty_message {
        return Ok(Vec::new());
    }
    Err(editor_required_message_error())
}

fn editor_only_commit_trailers(trailers: &[String]) -> Result<Vec<u8>> {
    let mut message = append_commit_trailers(Vec::new(), trailers)?;
    if message.ends_with(b"\n\n") {
        message.pop();
    }
    if message.ends_with(b"\n") {
        message.pop();
    }
    message.insert(0, b'\n');
    Ok(message)
}

fn edit_commit_message(
    repo: &GitRepo,
    mut message: Vec<u8>,
    editor_message: Option<Vec<u8>>,
    abort_if_unchanged: bool,
    allow_empty_message: bool,
    cleanup: CommitCleanupMode,
) -> Result<Vec<u8>> {
    let editor_message = editor_message.unwrap_or_default();
    let has_scissors = editor_message_has_scissors(&editor_message);
    message.extend_from_slice(&editor_message);
    let edited = edit_history_message(repo, &message)?;
    if abort_if_unchanged && edited == message {
        return Err(CliError::Stderr {
            code: 1,
            text: "Aborting commit; you did not edit the message.\n".into(),
        });
    }
    let message = cleanup_edited_commit_message(edited, cleanup, has_scissors);
    if message.is_empty() && !allow_empty_message {
        return Err(empty_commit_message_error());
    }
    Ok(message)
}

fn commit_status_enabled(repo: &GitRepo, status: bool, no_status: bool) -> Result<bool> {
    if status {
        return Ok(true);
    }
    if no_status {
        return Ok(false);
    }
    match read_config_entry(repo, "commit.status")? {
        Some(entry) => entry.bool_value().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!(
                "bad boolean config value '{}' for 'commit.status'",
                entry.value
            ),
        }),
        None => Ok(true),
    }
}

fn empty_commit_message_error() -> CliError {
    CliError::Stderr {
        code: 1,
        text: "Aborting commit due to empty commit message.\n".into(),
    }
}

fn editor_message_has_scissors(message: &[u8]) -> bool {
    message
        .windows(b"# ------------------------ >8 ------------------------".len())
        .any(|window| window == b"# ------------------------ >8 ------------------------")
}

fn cleanup_edited_commit_message(
    message: Vec<u8>,
    cleanup: CommitCleanupMode,
    has_verbose_message: bool,
) -> Vec<u8> {
    if has_verbose_message && matches!(cleanup, CommitCleanupMode::Default) {
        return cleanup_commit_message(message, CommitCleanupMode::Scissors);
    }
    if matches!(cleanup, CommitCleanupMode::Default) {
        return cleanup_commit_message(message, CommitCleanupMode::Strip);
    }
    cleanup_commit_message(message, cleanup)
}

fn commit_editor_message(
    repo: &GitRepo,
    store: &LooseObjectStore,
    parent_tree: Option<&ObjectId>,
    commit_index: &GitIndex,
    real_index: &GitIndex,
    verbose: u8,
    date_author: Option<&Signature>,
) -> Result<Vec<u8>> {
    let tree_cache = TreeObjectCache::new(store);
    let parent_index = match parent_tree {
        Some(tree) => tree_cache.read_tree_to_index(tree)?,
        None => GitIndex::new(),
    };
    let staged_entries = diff_indexes(&parent_index, commit_index)?;
    let worktree_entries = worktree_commit_diff_entries(repo, real_index)?;
    let untracked_entries = commit_untracked_entries(repo, real_index)?;
    let mut message = Vec::new();
    write_commit_verbose_status(
        &mut message,
        parent_tree.is_some(),
        &staged_entries,
        &worktree_entries,
        &untracked_entries,
        refs_head_label(repo),
        date_author,
    )?;
    if verbose == 0 {
        return Ok(message);
    }
    message.extend_from_slice(b"# ------------------------ >8 ------------------------\n");
    message.extend_from_slice(b"# Do not modify or remove the line above.\n");
    message.extend_from_slice(b"# Everything below it will be ignored.\n");
    if verbose > 1 {
        if !staged_entries.is_empty() {
            message.extend_from_slice(b"#\n# Changes to be committed:\n");
            write_patch_entries(
                &mut message,
                repo,
                store,
                &parent_index,
                commit_index,
                &staged_entries,
                PatchFormatOptions::cached().with_prefixes("c/".to_owned(), "i/".to_owned()),
            )?;
        }
        if !worktree_entries.is_empty() {
            message.extend_from_slice(b"# --------------------------------------------------\n");
            message.extend_from_slice(b"# Changes not staged for commit:\n");
            write_worktree_verbose_patch(&mut message, repo, store, real_index, &worktree_entries)?;
        }
    } else if !staged_entries.is_empty() {
        write_patch_entries(
            &mut message,
            repo,
            store,
            &parent_index,
            commit_index,
            &staged_entries,
            PatchFormatOptions::cached(),
        )?;
    }
    Ok(message)
}

fn write_commit_verbose_status<W: Write>(
    out: &mut W,
    head_has_commit: bool,
    staged_entries: &[skron_git_core::IndexDiffEntry],
    worktree_entries: &[skron_git_core::IndexDiffEntry],
    untracked_entries: &[Vec<u8>],
    branch: String,
    date_author: Option<&Signature>,
) -> Result<()> {
    writeln!(
        out,
        "\n# Please enter the commit message for your changes. Lines starting\n\
         # with '#' will be ignored, and an empty message aborts the commit.\n#"
    )?;
    if let Some(author) = date_author {
        writeln!(out, "# Date:      {}", signature_summary_date(author)?)?;
        writeln!(out, "#")?;
    }
    writeln!(out, "# On branch {branch}")?;
    if !head_has_commit {
        writeln!(out, "#")?;
        writeln!(out, "# Initial commit")?;
        writeln!(out, "#")?;
    }
    if staged_entries.is_empty() && worktree_entries.is_empty() && untracked_entries.is_empty() {
        writeln!(out, "#")?;
        writeln!(out, "# No changes")?;
        return Ok(());
    }
    if !staged_entries.is_empty() {
        writeln!(out, "# Changes to be committed:")?;
        for entry in staged_entries {
            writeln!(
                out,
                "#\t{}   {}",
                worktree_commands::human_status_label(status_code(entry.status)),
                commit_verbose_path(&entry.path)
            )?;
        }
        writeln!(out, "#")?;
    }
    if !worktree_entries.is_empty() {
        writeln!(out, "# Changes not staged for commit:")?;
        for entry in worktree_entries {
            writeln!(
                out,
                "#\t{}   {}",
                worktree_commands::human_status_label(status_code(entry.status)),
                commit_verbose_path(&entry.path)
            )?;
        }
        writeln!(out, "#")?;
    }
    if !untracked_entries.is_empty() {
        writeln!(out, "# Untracked files:")?;
        for path in untracked_entries {
            writeln!(out, "#\t{}", commit_verbose_path(path))?;
        }
        writeln!(out, "#")?;
    }
    Ok(())
}

fn refs_head_label(repo: &GitRepo) -> String {
    commit_summary_branch(repo).unwrap_or_else(|_| "HEAD".to_owned())
}

fn commit_verbose_path(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

fn worktree_commit_diff_entries(
    repo: &GitRepo,
    index: &GitIndex,
) -> Result<Vec<skron_git_core::IndexDiffEntry>> {
    let mut entries = worktree_status(repo, index)?
        .into_iter()
        .map(|(path, status)| skron_git_core::IndexDiffEntry {
            status: match status {
                'D' => IndexDiffStatus::Deleted,
                _ => IndexDiffStatus::Modified,
            },
            path,
            old_path: None,
            similarity: None,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn commit_untracked_entries(repo: &GitRepo, index: &GitIndex) -> Result<Vec<Vec<u8>>> {
    let tracked_paths = worktree_commands::tracked_path_set(index);
    let ignore = GitIgnore::load_from_root(&repo.root)?;
    worktree_commands::untracked_files_with_mode(
        &repo.root,
        &tracked_paths,
        &ignore,
        worktree_commands::UntrackedMode::Normal,
    )
}

fn write_worktree_verbose_patch<W: Write>(
    out: &mut W,
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    entries: &[skron_git_core::IndexDiffEntry],
) -> Result<()> {
    let mut worktree_index = index.clone();
    for entry in entries {
        if entry.status == IndexDiffStatus::Deleted {
            worktree_index.remove_path(&entry.path)?;
        } else if let Some(index_entry) = find_index_entry(index, &entry.path) {
            let mut worktree_entry = index_entry.clone();
            let path = repo
                .root
                .join(String::from_utf8_lossy(&entry.path).as_ref());
            let content = fs::read(path)?;
            worktree_entry.id = hash_object(GitHashAlgorithm::Sha1, GitObjectKind::Blob, &content);
            worktree_index.upsert(worktree_entry)?;
        }
    }
    write_patch_entries(
        out,
        repo,
        store,
        index,
        &worktree_index,
        entries,
        PatchFormatOptions::worktree().with_prefixes("i/".to_owned(), "w/".to_owned()),
    )
}

fn read_commit_template_config(repo: &GitRepo) -> Result<Option<Vec<u8>>> {
    let Some(path) = read_config_value(repo, "commit.template")? else {
        return Ok(None);
    };
    Ok(Some(read_commit_message_file(Path::new(&path))?))
}

fn read_commit_message_file(path: &std::path::Path) -> Result<Vec<u8>> {
    if path == std::path::Path::new("-") {
        let mut message = Vec::new();
        io::stdin().read_to_end(&mut message)?;
        Ok(message)
    } else {
        Ok(fs::read(path)?)
    }
}

fn write_tree_command() -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let tree = write_tree_from_index(&store, &index)?;
    println!("{}", tree.to_hex());
    Ok(())
}

fn commit_tree(tree: &str, parents: Vec<String>, messages: Vec<String>) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let tree = resolve_objectish(&repo, tree).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("not a valid object name {tree}"),
    })?;
    let tree_object = store.read_object(&tree)?;
    if tree_object.kind != GitObjectKind::Tree {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("{} is not a valid 'tree' object", tree.to_hex()),
        });
    }
    let author = signature_from_identity(&repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
    let mut builder = CommitBuilder::new(tree, author, committer);
    for parent in parents {
        builder = builder.parent(resolve_commitish(&repo, &store, &parent)?);
    }
    let message = commit_tree_message(messages)?;
    let commit = builder.message(message)?.encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    println!("{}", id.to_hex());
    Ok(())
}

pub(crate) fn commit_tree_message(messages: Vec<String>) -> Result<Vec<u8>> {
    if messages.is_empty() {
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input)?;
        return Ok(input);
    }
    let mut message = messages.join("\n\n").into_bytes();
    message.push(b'\n');
    Ok(message)
}

fn mktree(nul_terminated: bool, missing: bool, batch: bool) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let records = split_mktree_records(&input, nul_terminated)?;
    if batch {
        let mut group = Vec::new();
        for record in records {
            if record.is_empty() {
                write_mktree_group(&store, &group, missing)?;
                group.clear();
            } else {
                group.push(record);
            }
        }
        if !group.is_empty() {
            write_mktree_group(&store, &group, missing)?;
        }
    } else {
        let records = records
            .into_iter()
            .filter(|record| !record.is_empty())
            .collect::<Vec<_>>();
        write_mktree_group(&store, &records, missing)?;
    }
    Ok(())
}

pub(crate) fn split_mktree_records(input: &[u8], nul_terminated: bool) -> Result<Vec<String>> {
    let separator = if nul_terminated { 0 } else { b'\n' };
    input
        .split(|byte| *byte == separator)
        .filter(|record| !record.is_empty() || !nul_terminated)
        .map(|record| {
            String::from_utf8(record.trim_ascii_end().to_vec()).map_err(|_| CliError::Fatal {
                code: 128,
                message: "mktree input must be UTF-8 for this implementation".into(),
            })
        })
        .collect()
}

fn write_mktree_group(store: &LooseObjectStore, records: &[String], missing: bool) -> Result<()> {
    let mut entries = records
        .iter()
        .map(|record| parse_mktree_entry(store, record, missing))
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by(compare_mktree_entries);
    let encoded = encode_tree(&entries)?;
    let id = store.write_object(GitObjectKind::Tree, &encoded)?;
    println!("{}", id.to_hex());
    Ok(())
}

pub(crate) fn parse_mktree_entry(
    store: &LooseObjectStore,
    record: &str,
    missing: bool,
) -> Result<TreeEntry> {
    let (header, name) = record.split_once('\t').ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "input format error: expected '<mode> <type> <sha1>\\t<path>'".into(),
    })?;
    let mut parts = header.split_whitespace();
    let mode = parts.next().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "mktree input missing mode".into(),
    })?;
    let object_type = parts.next().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "mktree input missing object type".into(),
    })?;
    let id = parts.next().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "mktree input missing object id".into(),
    })?;
    if parts.next().is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "mktree input has too many header fields".into(),
        });
    }
    let tree_mode = parse_mktree_mode(mode, object_type)?;
    let object_kind = tree_entry_kind(tree_mode);
    let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?;
    if !missing && tree_mode != TreeMode::Gitlink {
        let object = store.read_object(&id)?;
        if object.kind != object_kind {
            return Err(CliError::Fatal {
                code: 128,
                message: "mktree object type does not match mode".into(),
            });
        }
    }
    Ok(TreeEntry::new(tree_mode, name.as_bytes().to_vec(), id)?)
}

fn parse_mktree_mode(mode: &str, object_type: &str) -> Result<TreeMode> {
    match (mode, object_type) {
        ("100644", "blob") => Ok(TreeMode::File),
        ("100755", "blob") => Ok(TreeMode::Executable),
        ("120000", "blob") => Ok(TreeMode::Symlink),
        ("40000" | "040000", "tree") => Ok(TreeMode::Tree),
        ("160000", "commit") => Ok(TreeMode::Gitlink),
        _ => Err(CliError::Fatal {
            code: 128,
            message: "mktree input has invalid mode/type combination".into(),
        }),
    }
}

pub(crate) fn compare_mktree_entries(left: &TreeEntry, right: &TreeEntry) -> std::cmp::Ordering {
    let left_name = mktree_sort_name(left);
    let right_name = mktree_sort_name(right);
    left_name.cmp(&right_name)
}

fn mktree_sort_name(entry: &TreeEntry) -> Vec<u8> {
    let mut name = entry.name.clone();
    if entry.mode == TreeMode::Tree {
        name.push(b'/');
    }
    name
}
