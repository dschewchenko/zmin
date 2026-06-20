use super::*;

struct NotesArgs {
    notes_ref: Option<String>,
    operation: Option<String>,
    args: Vec<String>,
}

pub(crate) fn notes(args: Vec<String>) -> Result<()> {
    let args = parse_notes_args(args)?;
    let repo = find_repo()?;
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let store = runtime.object_store_adapter();
    let refs = runtime.refs_store_adapter();
    let ref_name = notes_ref_name(&repo, args.notes_ref.as_deref())?;
    match args.operation.as_deref().unwrap_or("list") {
        "list" => notes_list(&repo, &store, &refs, &ref_name, args.args),
        "show" => notes_show(&repo, &store, &refs, &ref_name, args.args),
        "add" => notes_add(&repo, &store, &refs, &ref_name, args.args),
        "append" => notes_append(&repo, &store, &refs, &ref_name, args.args),
        "copy" => notes_copy(&repo, &store, &refs, &ref_name, args.args),
        "edit" => notes_edit(&repo, &store, &refs, &ref_name, args.args),
        "remove" => notes_remove(&repo, &store, &refs, &ref_name, args.args),
        "prune" => notes_prune(&repo, &store, &refs, &ref_name, args.args),
        "merge" => notes_merge(&repo, &store, &refs, &ref_name, args.args),
        "get-ref" => {
            if !args.args.is_empty() {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "notes get-ref accepts no arguments".into(),
                });
            }
            println!("{ref_name}");
            Ok(())
        }
        command => Err(CliError::Stderr {
            code: 129,
            text: format!("error: unknown subcommand: `{command}'\n{}", notes_usage()),
        }),
    }
}

fn notes_usage() -> &'static str {
    "usage: git notes [--ref <notes-ref>] [list [<object>]]
   or: git notes [--ref <notes-ref>] add [-f] [--allow-empty] [--[no-]separator|--separator=<paragraph-break>] [--[no-]stripspace] [-m <msg> | -F <file> | (-c | -C) <object>] [<object>] [-e]
   or: git notes [--ref <notes-ref>] copy [-f] <from-object> <to-object>
   or: git notes [--ref <notes-ref>] append [--allow-empty] [--[no-]separator|--separator=<paragraph-break>] [--[no-]stripspace] [-m <msg> | -F <file> | (-c | -C) <object>] [<object>] [-e]
   or: git notes [--ref <notes-ref>] edit [--allow-empty] [<object>]
   or: git notes [--ref <notes-ref>] show [<object>]
   or: git notes [--ref <notes-ref>] merge [-v | -q] [-s <strategy>] <notes-ref>
   or: git notes merge --commit [-v | -q]
   or: git notes merge --abort [-v | -q]
   or: git notes [--ref <notes-ref>] remove [<object>...]
   or: git notes [--ref <notes-ref>] prune [-n] [-v]
   or: git notes [--ref <notes-ref>] get-ref

    --[no-]ref <notes-ref>
                          use notes from <notes-ref>

"
}

fn notes_copy_usage() -> &'static str {
    "usage: git notes copy [<options>] <from-object> <to-object>
   or: git notes copy --stdin [<from-object> <to-object>]...

    -f, --[no-]force      replace existing notes
    --[no-]stdin          read objects from stdin
    --[no-]for-rewrite <command>
                          load rewriting config for <command> (implies --stdin)

"
}

fn notes_add_usage() -> &'static str {
    "usage: git notes add [<options>] [<object>]

    -m, --message <message>
                          note contents as a string
    -F, --file <file>     note contents in a file
    -c, --reedit-message <object>
                          reuse and edit specified note object
    -e, --[no-]edit       edit note message in editor
    -C, --reuse-message <object>
                          reuse specified note object
    --[no-]allow-empty    allow storing empty note
    -f, --[no-]force      replace existing notes
    --[no-]separator[=<paragraph-break>]
                          insert <paragraph-break> between paragraphs
    --[no-]stripspace     remove unnecessary whitespace

"
}

fn notes_edit_usage() -> &'static str {
    "usage: git notes edit [<object>]

    -m, --message <message>
                          note contents as a string
    -F, --file <file>     note contents in a file
    -c, --reedit-message <object>
                          reuse and edit specified note object
    -C, --reuse-message <object>
                          reuse specified note object
    -e, --[no-]edit       edit note message in editor
    --[no-]allow-empty    allow storing empty note
    --[no-]separator[=<paragraph-break>]
                          insert <paragraph-break> between paragraphs
    --[no-]stripspace     remove unnecessary whitespace

"
}

fn notes_remove_usage() -> &'static str {
    "usage: git notes remove [<object>]

    --[no-]ignore-missing attempt to remove non-existent note is not an error
    --[no-]stdin          read object names from the standard input

"
}

fn notes_prune_usage() -> &'static str {
    "usage: git notes prune [<options>]

    -n, --[no-]dry-run    do not remove, show only
    -v, --[no-]verbose    report pruned notes

"
}

fn notes_merge_usage() -> &'static str {
    "usage: git notes merge [<options>] <notes-ref>
   or: git notes merge --commit [<options>]
   or: git notes merge --abort [<options>]

General options
    -v, --[no-]verbose    be more verbose
    -q, --[no-]quiet      be more quiet

Merge options
    -s, --[no-]strategy <strategy>
                          resolve notes conflicts using the given strategy (manual/ours/theirs/union/cat_sort_uniq)

Committing unmerged notes
    --commit              finalize notes merge by committing unmerged notes

Aborting notes merge resolution
    --abort               abort notes merge

"
}

fn notes_unknown_option(option: &str, usage: &str) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!(
            "error: unknown option `{}'\n{}",
            option.trim_start_matches('-'),
            usage
        ),
    }
}

fn parse_notes_args(args: Vec<String>) -> Result<NotesArgs> {
    let mut notes_ref = None;
    let mut cursor = 0usize;
    while cursor < args.len() {
        match args[cursor].as_str() {
            "--ref" => {
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "option `ref' requires a value".into(),
                    });
                };
                notes_ref = Some(value.clone());
            }
            value if value.starts_with("--ref=") => {
                notes_ref = Some(value["--ref=".len()..].to_owned());
            }
            "--no-ref" => {
                notes_ref = None;
            }
            _ => {
                let operation = Some(args[cursor].clone());
                let args = args[cursor + 1..].to_vec();
                return Ok(NotesArgs {
                    notes_ref,
                    operation,
                    args,
                });
            }
        }
        cursor += 1;
    }
    Ok(NotesArgs {
        notes_ref,
        operation: None,
        args: Vec::new(),
    })
}

fn notes_ref_name(repo: &GitRepo, notes_ref: Option<&str>) -> Result<String> {
    match notes_ref {
        None => Ok(std::env::var("GIT_NOTES_REF")
            .ok()
            .or_else(|| read_config_value(repo, "core.notesRef").ok().flatten())
            .unwrap_or_else(|| "refs/notes/commits".to_owned())),
        Some(name) if name.starts_with("refs/notes/") => Ok(name.to_owned()),
        Some(name) => Ok(format!("refs/notes/{name}")),
    }
}

fn ensure_mutable_notes_ref(ref_name: &str, operation: &str) -> Result<()> {
    if ref_name.starts_with("refs/notes/") {
        return Ok(());
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("refusing to {operation} notes in {ref_name} (outside of refs/notes/)"),
    })
}

fn notes_list(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    if args.len() > 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "notes list accepts at most one object".into(),
        });
    }
    let notes = read_notes_map(store, refs, ref_name)?;
    if let Some(object) = args.first() {
        let object_id = resolve_notes_objectish(repo, object, NotesResolveMode::Fatal)?;
        let key = object_id.to_hex();
        let Some(note_id) = notes.get(&key) else {
            eprintln!("error: no note found for object {key}.");
            return Err(CliError::Exit(1));
        };
        println!("{} {}", note_id.to_hex(), key);
        return Ok(());
    }
    let mut rows = notes.into_iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    for (object, note) in rows {
        println!("{} {}", note.to_hex(), object);
    }
    Ok(())
}

fn notes_show(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    if args.len() > 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "notes show accepts at most one object".into(),
        });
    }
    let object = args.first().map(String::as_str).unwrap_or("HEAD");
    let object_id = resolve_notes_objectish(repo, object, NotesResolveMode::Fatal)?;
    let key = object_id.to_hex();
    let notes = read_notes_map(store, refs, ref_name)?;
    let Some(note_id) = notes.get(&key) else {
        eprintln!("error: no note found for object {key}.");
        return Err(CliError::Exit(1));
    };
    let note = store.read_object(note_id)?;
    if note.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: "note object is not a blob".into(),
        });
    }
    io::stdout().write_all(&note.content)?;
    Ok(())
}

fn notes_add(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    ensure_mutable_notes_ref(ref_name, "add")?;
    let NotesAddArgs {
        force,
        edit,
        separator,
        stripspace,
        sources,
        object,
    } = parse_notes_add_args(args)?;
    let object = object.as_deref().unwrap_or("HEAD");
    let object_id = resolve_notes_objectish(repo, object, NotesResolveMode::Fatal)?;
    let key = object_id.to_hex();
    let mut notes = read_notes_map(store, refs, ref_name)?;
    if !force && notes.contains_key(&key) {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("Cannot add notes. Found existing notes for object {key}"),
        });
    }
    let mut message = notes_message_from_sources(repo, store, sources, &separator, stripspace)?;
    if edit {
        message = strip_note_editor_comments(edit_history_message(repo, &message)?);
    }
    let note_id = store.write_object(GitObjectKind::Blob, &message)?;
    notes.insert(key, note_id);
    write_notes_ref(
        repo,
        store,
        refs,
        ref_name,
        &notes,
        "Notes added by 'git notes add'",
    )?;
    Ok(())
}

fn notes_append(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    ensure_mutable_notes_ref(ref_name, "append")?;
    let NotesAddArgs {
        force: _,
        edit,
        separator,
        stripspace,
        sources,
        object,
    } = parse_notes_add_args(args)?;
    let object = object.as_deref().unwrap_or("HEAD");
    let object_id = resolve_notes_objectish(repo, object, NotesResolveMode::Fatal)?;
    let key = object_id.to_hex();
    let mut notes = read_notes_map(store, refs, ref_name)?;
    let mut message = notes_message_from_sources(repo, store, sources, &separator, stripspace)?;
    if edit {
        message = strip_note_editor_comments(edit_history_message(repo, &message)?);
    }
    let content = if let Some(existing_id) = notes.get(&key) {
        let existing = store.read_object(existing_id)?;
        if existing.kind != GitObjectKind::Blob {
            return Err(CliError::Fatal {
                code: 128,
                message: "note object is not a blob".into(),
            });
        }
        if message.is_empty() {
            existing.content
        } else {
            let mut content = existing.content;
            while content.ends_with(b"\n") {
                content.pop();
            }
            if !content.is_empty() {
                content.extend_from_slice(&separator);
            }
            content.extend_from_slice(&message);
            content
        }
    } else {
        message
    };
    let note_id = store.write_object(GitObjectKind::Blob, &content)?;
    notes.insert(key, note_id);
    write_notes_ref(
        repo,
        store,
        refs,
        ref_name,
        &notes,
        "Notes added by 'git notes append'",
    )?;
    Ok(())
}

fn notes_copy(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    ensure_mutable_notes_ref(ref_name, "copy")?;
    let args = parse_notes_copy_args(args)?;
    if let Some(command) = args.for_rewrite.as_deref()
        && !notes_copy_for_rewrite_enabled(repo, ref_name, command)?
    {
        return Ok(());
    }
    let mut notes = read_notes_map(store, refs, ref_name)?;
    let pairs = if args.stdin {
        read_notes_copy_stdin_pairs()?
    } else {
        args.pairs
    };
    for (from, to) in pairs {
        let from_id = resolve_notes_objectish(repo, &from, NotesResolveMode::Fatal)?;
        let to_id = resolve_notes_objectish(repo, &to, NotesResolveMode::Fatal)?;
        let from_key = from_id.to_hex();
        let to_key = to_id.to_hex();
        let Some(note_id) = notes.get(&from_key).cloned() else {
            eprintln!("error: no note found for object {from_key}.");
            return Err(CliError::Exit(1));
        };
        if notes.contains_key(&to_key) {
            if !args.force {
                eprintln!(
                    "error: Cannot copy notes. Found existing notes for object {to_key}. Use '-f' to overwrite existing notes"
                );
                return Err(CliError::Exit(1));
            }
            eprintln!("Overwriting existing notes for object {to_key}");
        }
        notes.insert(to_key, note_id);
    }
    write_notes_ref(
        repo,
        store,
        refs,
        ref_name,
        &notes,
        "Notes added by 'git notes copy'",
    )?;
    Ok(())
}

#[derive(Debug)]
struct NotesCopyArgs {
    force: bool,
    stdin: bool,
    for_rewrite: Option<String>,
    pairs: Vec<(String, String)>,
}

fn parse_notes_copy_args(args: Vec<String>) -> Result<NotesCopyArgs> {
    let mut force = false;
    let mut stdin = false;
    let mut for_rewrite = None;
    let mut objects = Vec::new();
    let mut cursor = 0usize;
    while cursor < args.len() {
        let arg = &args[cursor];
        match arg.as_str() {
            "-f" | "--force" => force = true,
            "--no-force" => force = false,
            "--stdin" => stdin = true,
            "--no-stdin" | "--no-for-rewrite" => {
                stdin = false;
                for_rewrite = None;
            }
            "--for-rewrite" => {
                cursor += 1;
                let Some(command) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "option `for-rewrite' requires a value".into(),
                    });
                };
                stdin = true;
                for_rewrite = Some(command.clone());
            }
            value if value.starts_with("--for-rewrite=") => {
                stdin = true;
                for_rewrite = Some(value["--for-rewrite=".len()..].to_owned());
            }
            value if value.starts_with("--no-for-rewrite=") => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "option `no-for-rewrite' takes no value".into(),
                });
            }
            value if value.starts_with('-') => {
                return Err(notes_unknown_option(value, notes_copy_usage()));
            }
            value => objects.push(value.to_owned()),
        }
        cursor += 1;
    }
    if stdin && !objects.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "notes copy --stdin does not accept positional objects".into(),
        });
    }
    if !stdin && objects.len() != 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "notes copy requires <from-object> <to-object>".into(),
        });
    }
    let pairs = if stdin {
        Vec::new()
    } else {
        vec![(objects.remove(0), objects.remove(0))]
    };
    Ok(NotesCopyArgs {
        force,
        stdin,
        for_rewrite,
        pairs,
    })
}

fn notes_copy_for_rewrite_enabled(repo: &GitRepo, ref_name: &str, command: &str) -> Result<bool> {
    if let Some(value) = read_config_value(repo, &format!("notes.rewrite.{command}"))?
        && parse_git_bool(&value) == Some(false)
    {
        return Ok(false);
    }
    let Some(rewrite_ref) = read_config_value(repo, "notes.rewriteRef")? else {
        return Ok(false);
    };
    Ok(notes_rewrite_ref_matches(&rewrite_ref, ref_name))
}

fn notes_rewrite_ref_matches(pattern: &str, ref_name: &str) -> bool {
    pattern == ref_name
        || pattern == "*"
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| ref_name.starts_with(prefix))
}

fn read_notes_copy_stdin_pairs() -> Result<Vec<(String, String)>> {
    let mut pairs = Vec::new();
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(from) = parts.next() else {
            continue;
        };
        let Some(to) = parts.next() else {
            return Err(CliError::Fatal {
                code: 129,
                message: "notes copy --stdin requires <from-object> <to-object> pairs".into(),
            });
        };
        if parts.next().is_some() {
            return Err(CliError::Fatal {
                code: 129,
                message: "notes copy --stdin accepts exactly two object names per line".into(),
            });
        }
        pairs.push((from.to_owned(), to.to_owned()));
    }
    Ok(pairs)
}

fn notes_edit(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    ensure_mutable_notes_ref(ref_name, "edit")?;
    let NotesEditArgs {
        allow_empty,
        edit,
        separator,
        stripspace,
        sources,
        object,
        deprecated_message_sources,
    } = parse_notes_edit_args(args)?;
    let object = object.as_deref().unwrap_or("HEAD");
    let object_id = resolve_notes_objectish(repo, object, NotesResolveMode::Fatal)?;
    let key = object_id.to_hex();
    let mut notes = read_notes_map(store, refs, ref_name)?;
    let existing = notes
        .get(&key)
        .map(|note_id| store.read_object(note_id))
        .transpose()?;
    let initial = match existing {
        Some(note) if note.kind == GitObjectKind::Blob => note.content,
        Some(_) => {
            return Err(CliError::Fatal {
                code: 128,
                message: "note object is not a blob".into(),
            });
        }
        None => Vec::new(),
    };
    let edited = if sources.is_empty() {
        strip_note_editor_comments(edit_history_message(repo, &initial)?)
    } else {
        if deprecated_message_sources {
            eprintln!(
                "The -m/-F/-c/-C options have been deprecated for the 'edit' subcommand.\nPlease use 'git notes add -f -m/-F/-c/-C' instead."
            );
        }
        let mut message = notes_message_from_sources(repo, store, sources, &separator, stripspace)?;
        if edit {
            message = strip_note_editor_comments(edit_history_message(repo, &message)?);
        }
        message
    };
    if edited.is_empty() && !allow_empty {
        eprintln!("Removing note for object {key}");
        if notes.remove(&key).is_some() {
            write_notes_ref(
                repo,
                store,
                refs,
                ref_name,
                &notes,
                "Notes removed by 'git notes edit'",
            )?;
        }
        return Ok(());
    }
    let note_id = store.write_object(GitObjectKind::Blob, &edited)?;
    notes.insert(key, note_id);
    write_notes_ref(
        repo,
        store,
        refs,
        ref_name,
        &notes,
        "Notes added by 'git notes edit'",
    )?;
    Ok(())
}

#[derive(Debug)]
struct NotesEditArgs {
    allow_empty: bool,
    edit: bool,
    separator: Vec<u8>,
    stripspace: NotesStripspaceMode,
    sources: Vec<NotesMessageSource>,
    object: Option<String>,
    deprecated_message_sources: bool,
}

fn parse_notes_edit_args(args: Vec<String>) -> Result<NotesEditArgs> {
    let mut allow_empty = false;
    let mut edit = false;
    let mut separator = b"\n\n".to_vec();
    let mut stripspace = NotesStripspaceMode::Default;
    let mut sources = Vec::new();
    let mut object = None;
    let mut deprecated_message_sources = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        match args[cursor].as_str() {
            "-e" | "--edit" => edit = true,
            "--no-edit" => edit = false,
            "--separator" => separator = b"\n\n".to_vec(),
            "--no-separator" => separator = b"\n".to_vec(),
            "--stripspace" => stripspace = NotesStripspaceMode::Strip,
            "--no-stripspace" => stripspace = NotesStripspaceMode::NoStrip,
            value if value.starts_with("--separator=") => {
                let Some(value) = value.strip_prefix("--separator=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes edit invalid option '{value}'"),
                    });
                };
                separator = format!("\n{value}\n").into_bytes();
            }
            "--allow-empty" => allow_empty = true,
            "--no-allow-empty" => allow_empty = false,
            "-m" | "--message" => {
                cursor += 1;
                let Some(message) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes edit -m requires a message".into(),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Literal(message.clone()));
            }
            value if value.starts_with("-m") && value.len() > 2 => {
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Literal(value[2..].to_owned()));
            }
            value if value.starts_with("--message=") => {
                let Some(message) = value.strip_prefix("--message=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes edit invalid option '{value}'"),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Literal(message.to_owned()));
            }
            "-F" | "--file" => {
                cursor += 1;
                let Some(path) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes edit -F requires a file".into(),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::File(path.clone()));
            }
            value if value.starts_with("-F") && value.len() > 2 => {
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::File(value[2..].to_owned()));
            }
            value if value.starts_with("--file=") => {
                let Some(path) = value.strip_prefix("--file=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes edit invalid option '{value}'"),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::File(path.to_owned()));
            }
            "-C" | "--reuse-message" => {
                cursor += 1;
                let Some(objectish) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes edit -C requires an object".into(),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Reuse(objectish.clone()));
            }
            value if value.starts_with("-C") && value.len() > 2 => {
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Reuse(value[2..].to_owned()));
            }
            value if value.starts_with("--reuse-message=") => {
                let Some(objectish) = value.strip_prefix("--reuse-message=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes edit invalid option '{value}'"),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Reuse(objectish.to_owned()));
            }
            "-c" | "--reedit-message" => {
                cursor += 1;
                let Some(objectish) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes edit -c requires an object".into(),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Reedit(objectish.clone()));
            }
            value if value.starts_with("-c") && value.len() > 2 => {
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Reedit(value[2..].to_owned()));
            }
            value if value.starts_with("--reedit-message=") => {
                let Some(objectish) = value.strip_prefix("--reedit-message=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes edit invalid option '{value}'"),
                    });
                };
                deprecated_message_sources = true;
                sources.push(NotesMessageSource::Reedit(objectish.to_owned()));
            }
            value if value.starts_with('-') => {
                return Err(notes_unknown_option(value, notes_edit_usage()));
            }
            value => {
                if object.replace(value.to_owned()).is_some() {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes edit accepts at most one object".into(),
                    });
                }
            }
        }
        cursor += 1;
    }
    Ok(NotesEditArgs {
        allow_empty,
        edit,
        separator,
        stripspace,
        sources,
        object,
        deprecated_message_sources,
    })
}

fn strip_note_editor_comments(message: Vec<u8>) -> Vec<u8> {
    let mut lines = message
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| !line.starts_with(b"#"))
        .flat_map(|line| line.iter().copied())
        .collect::<Vec<_>>();
    while lines.ends_with(b"\n") {
        lines.pop();
    }
    if !lines.is_empty() {
        lines.push(b'\n');
    }
    lines
}

#[derive(Debug)]
enum NotesMessageSource {
    Literal(String),
    File(String),
    Reuse(String),
    Reedit(String),
    Empty,
}

#[derive(Debug)]
struct NotesAddArgs {
    force: bool,
    edit: bool,
    separator: Vec<u8>,
    stripspace: NotesStripspaceMode,
    sources: Vec<NotesMessageSource>,
    object: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotesStripspaceMode {
    Default,
    Strip,
    NoStrip,
}

fn notes_message_from_sources(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    sources: Vec<NotesMessageSource>,
    separator: &[u8],
    stripspace: NotesStripspaceMode,
) -> Result<Vec<u8>> {
    if matches!(sources.as_slice(), [NotesMessageSource::Empty]) {
        return Ok(Vec::new());
    }
    let last_is_text_source = sources.last().is_some_and(|source| {
        matches!(
            source,
            NotesMessageSource::Literal(_) | NotesMessageSource::File(_)
        )
    });
    let mut messages = Vec::new();
    for source in sources {
        match source {
            NotesMessageSource::Literal(message) => {
                let message = normalize_note_text_source(message.into_bytes(), stripspace)?;
                if !message.is_empty() {
                    messages.push(message);
                }
            }
            NotesMessageSource::File(path) => {
                let message = if path == "-" {
                    let mut message = Vec::new();
                    io::stdin().read_to_end(&mut message)?;
                    message
                } else {
                    fs::read(path)?
                };
                let message = normalize_note_text_source(message, stripspace)?;
                if !message.is_empty() {
                    messages.push(message);
                }
            }
            NotesMessageSource::Reuse(objectish) => {
                messages.push(read_note_message_blob(repo, store, &objectish)?);
            }
            NotesMessageSource::Reedit(objectish) => {
                let message = read_note_message_blob(repo, store, &objectish)?;
                messages.push(strip_note_editor_comments(edit_history_message(
                    repo, &message,
                )?));
            }
            NotesMessageSource::Empty => messages.push(Vec::new()),
        }
    }
    let mut message = Vec::new();
    for (index, part) in messages.into_iter().enumerate() {
        if index > 0 {
            message.extend_from_slice(separator);
        }
        message.extend_from_slice(&part);
    }
    match stripspace {
        NotesStripspaceMode::Strip => stripspace_note_message(message),
        NotesStripspaceMode::Default => {
            if last_is_text_source && !message.ends_with(b"\n") {
                message.push(b'\n');
            }
            Ok(message)
        }
        NotesStripspaceMode::NoStrip => Ok(message),
    }
}

fn normalize_note_text_source(
    message: Vec<u8>,
    stripspace: NotesStripspaceMode,
) -> Result<Vec<u8>> {
    if stripspace != NotesStripspaceMode::Default {
        return Ok(message);
    }
    let mut stripped = stripspace_note_message(message)?;
    if stripped.ends_with(b"\n") {
        stripped.pop();
    }
    Ok(stripped)
}

fn stripspace_note_message(message: Vec<u8>) -> Result<Vec<u8>> {
    let text = String::from_utf8(message).map_err(|_| CliError::Fatal {
        code: 128,
        message: "note data must be UTF-8 when stripspace is enabled".into(),
    })?;
    let mut lines = Vec::new();
    let mut previous_blank = true;
    for line in text.lines() {
        let stripped = line.trim_end_matches([' ', '\t', '\r']);
        if stripped.is_empty() {
            if !previous_blank {
                lines.push(String::new());
                previous_blank = true;
            }
        } else {
            lines.push(stripped.to_owned());
            previous_blank = false;
        }
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return Ok(Vec::new());
    }
    let mut stripped = lines.join("\n").into_bytes();
    stripped.push(b'\n');
    Ok(stripped)
}

fn read_note_message_blob(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    objectish: &str,
) -> Result<Vec<u8>> {
    let id = resolve_objectish(repo, objectish)?;
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("cannot read note data from non-blob object {}", id.to_hex()),
        });
    }
    Ok(object.content)
}

fn parse_notes_add_args(args: Vec<String>) -> Result<NotesAddArgs> {
    let mut force = false;
    let mut edit = false;
    let mut separator = b"\n\n".to_vec();
    let mut stripspace = NotesStripspaceMode::Default;
    let mut allow_empty = false;
    let mut sources = Vec::new();
    let mut object = None;
    let mut cursor = 0usize;
    while cursor < args.len() {
        match args[cursor].as_str() {
            "-f" | "--force" => force = true,
            "--no-force" => force = false,
            "-e" | "--edit" => edit = true,
            "--no-edit" => edit = false,
            "--separator" => separator = b"\n\n".to_vec(),
            "--no-separator" => separator = b"\n".to_vec(),
            "--stripspace" => stripspace = NotesStripspaceMode::Strip,
            "--no-stripspace" => stripspace = NotesStripspaceMode::NoStrip,
            value if value.starts_with("--separator=") => {
                let Some(value) = value.strip_prefix("--separator=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes add invalid option '{value}'"),
                    });
                };
                separator = format!("\n{value}\n").into_bytes();
            }
            "--allow-empty" => allow_empty = true,
            "--no-allow-empty" => allow_empty = false,
            "-m" => {
                cursor += 1;
                let Some(message) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes add -m requires a message".into(),
                    });
                };
                sources.push(NotesMessageSource::Literal(message.clone()));
            }
            value if value.starts_with("-m") && value.len() > 2 => {
                sources.push(NotesMessageSource::Literal(value[2..].to_owned()));
            }
            "--message" => {
                cursor += 1;
                let Some(message) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes add --message requires a message".into(),
                    });
                };
                sources.push(NotesMessageSource::Literal(message.clone()));
            }
            value if value.starts_with("--message=") => {
                let Some(message) = value.strip_prefix("--message=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes add invalid option '{value}'"),
                    });
                };
                sources.push(NotesMessageSource::Literal(message.to_owned()));
            }
            "-F" | "--file" => {
                cursor += 1;
                let Some(path) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes add -F requires a file".into(),
                    });
                };
                sources.push(NotesMessageSource::File(path.clone()));
            }
            value if value.starts_with("-F") && value.len() > 2 => {
                sources.push(NotesMessageSource::File(value[2..].to_owned()));
            }
            value if value.starts_with("--file=") => {
                let Some(path) = value.strip_prefix("--file=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes add invalid option '{value}'"),
                    });
                };
                sources.push(NotesMessageSource::File(path.to_owned()));
            }
            "-C" | "--reuse-message" => {
                cursor += 1;
                let Some(objectish) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes add -C requires an object".into(),
                    });
                };
                sources.push(NotesMessageSource::Reuse(objectish.clone()));
            }
            value if value.starts_with("-C") && value.len() > 2 => {
                sources.push(NotesMessageSource::Reuse(value[2..].to_owned()));
            }
            value if value.starts_with("--reuse-message=") => {
                let Some(message) = value.strip_prefix("--reuse-message=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes add invalid option '{value}'"),
                    });
                };
                sources.push(NotesMessageSource::Reuse(message.to_owned()));
            }
            "-c" | "--reedit-message" => {
                cursor += 1;
                let Some(objectish) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes add -c requires an object".into(),
                    });
                };
                sources.push(NotesMessageSource::Reedit(objectish.clone()));
            }
            value if value.starts_with("-c") && value.len() > 2 => {
                sources.push(NotesMessageSource::Reedit(value[2..].to_owned()));
            }
            value if value.starts_with("--reedit-message=") => {
                let Some(message) = value.strip_prefix("--reedit-message=") else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("notes add invalid option '{value}'"),
                    });
                };
                sources.push(NotesMessageSource::Reedit(message.to_owned()));
            }
            value if value.starts_with('-') => {
                return Err(notes_unknown_option(value, notes_add_usage()));
            }
            value => {
                if object.replace(value.to_owned()).is_some() {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes add accepts at most one object".into(),
                    });
                }
            }
        }
        cursor += 1;
    }
    if sources.is_empty() {
        if allow_empty || edit {
            return Ok(NotesAddArgs {
                force,
                edit: true,
                separator,
                stripspace,
                sources: vec![NotesMessageSource::Empty],
                object,
            });
        }
        return Err(CliError::Fatal {
            code: 129,
            message: "notes add requires -m or -F".into(),
        });
    }
    Ok(NotesAddArgs {
        force,
        edit,
        separator,
        stripspace,
        sources,
        object,
    })
}

fn notes_remove(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    ensure_mutable_notes_ref(ref_name, "remove")?;
    let (ignore_missing, stdin, mut objects) = parse_notes_remove_args(args)?;
    if stdin {
        for line in io::stdin().lock().lines() {
            let line = line?;
            if !line.is_empty() {
                objects.push(line);
            }
        }
    }
    let objects = if objects.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        objects
    };
    let mut notes = read_notes_map(store, refs, ref_name)?;
    let mut changed = false;
    for object in objects {
        let object_id = resolve_notes_objectish(repo, &object, NotesResolveMode::Error)?;
        let key = object_id.to_hex();
        if notes.remove(&key).is_some() {
            eprintln!("Removing note for object {object}");
            changed = true;
        } else {
            eprintln!("Object {object} has no note");
            if !ignore_missing {
                return Err(CliError::Exit(1));
            }
        }
    }
    if changed {
        write_notes_ref(
            repo,
            store,
            refs,
            ref_name,
            &notes,
            "Notes removed by 'git notes remove'",
        )?;
    }
    Ok(())
}

fn parse_notes_remove_args(args: Vec<String>) -> Result<(bool, bool, Vec<String>)> {
    let mut ignore_missing = false;
    let mut stdin = false;
    let mut objects = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--ignore-missing" => ignore_missing = true,
            "--no-ignore-missing" => ignore_missing = false,
            "--stdin" => stdin = true,
            "--no-stdin" => stdin = false,
            value if value.starts_with('-') => {
                return Err(notes_unknown_option(value, notes_remove_usage()));
            }
            value => objects.push(value.to_owned()),
        }
    }
    Ok((ignore_missing, stdin, objects))
}

fn notes_prune(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    ensure_mutable_notes_ref(ref_name, "prune")?;
    let (dry_run, _verbose) = parse_notes_prune_args(args)?;
    let mut notes = read_notes_map(store, refs, ref_name)?;
    let mut stale = notes
        .keys()
        .filter_map(|key| {
            ObjectId::from_hex(GitHashAlgorithm::Sha1, key)
                .ok()
                .filter(|id| store.read_object(id).is_err())
                .map(|_| key.clone())
        })
        .collect::<Vec<_>>();
    stale.sort();
    if dry_run || _verbose {
        for key in &stale {
            println!("{key}");
        }
    }
    if !dry_run && !stale.is_empty() {
        for key in stale {
            notes.remove(&key);
        }
        write_notes_ref(
            repo,
            store,
            refs,
            ref_name,
            &notes,
            "Notes removed by 'git notes prune'",
        )?;
    }
    Ok(())
}

fn parse_notes_prune_args(args: Vec<String>) -> Result<(bool, bool)> {
    let mut dry_run = false;
    let mut verbose = false;
    for arg in args {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "--no-dry-run" => dry_run = false,
            "-v" | "--verbose" => verbose = true,
            "--no-verbose" => verbose = false,
            "-nv" | "-vn" => {
                dry_run = true;
                verbose = true;
            }
            value => {
                return Err(notes_unknown_option(value, notes_prune_usage()));
            }
        }
    }
    Ok((dry_run, verbose))
}

fn notes_merge(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    args: Vec<String>,
) -> Result<()> {
    match parse_notes_merge_args(args)? {
        NotesMergeAction::Commit => notes_merge_commit(repo, store, refs),
        NotesMergeAction::Abort => notes_merge_abort(repo),
        NotesMergeAction::Merge {
            strategy,
            source,
            quiet,
        } => {
            ensure_mutable_notes_ref(ref_name, "merge")?;
            notes_merge_refs(repo, store, refs, ref_name, strategy, source, quiet)
        }
    }
}

fn notes_merge_refs(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    strategy: NotesMergeStrategy,
    source: String,
    quiet: bool,
) -> Result<()> {
    let source_ref = notes_ref_name(repo, Some(&source))?;
    let mut current = read_notes_map(store, refs, ref_name)?;
    let incoming = read_notes_map(store, refs, &source_ref)?;
    let mut changed = false;
    let mut conflicts = Vec::new();
    for (object, incoming_note) in incoming {
        let Some(existing_note) = current.get(&object).cloned() else {
            current.insert(object, incoming_note);
            changed = true;
            continue;
        };
        if existing_note == incoming_note {
            continue;
        }
        match strategy {
            NotesMergeStrategy::Manual => {
                if !quiet {
                    println!("Auto-merging notes for {object}");
                    println!("CONFLICT (add/add): Merge conflict in notes for object {object}");
                }
                conflicts.push((object, existing_note, incoming_note));
            }
            NotesMergeStrategy::Ours => {
                if !quiet {
                    println!("Using local notes for {object}");
                }
            }
            NotesMergeStrategy::Theirs => {
                if !quiet {
                    println!("Using remote notes for {object}");
                }
                current.insert(object, incoming_note);
                changed = true;
            }
            NotesMergeStrategy::Union => {
                if !quiet {
                    println!("Concatenating local and remote notes for {object}");
                }
                let merged = concatenate_note_blobs(store, &existing_note, &incoming_note, true)?;
                let merged_note = store.write_object(GitObjectKind::Blob, &merged)?;
                current.insert(object, merged_note);
                changed = true;
            }
            NotesMergeStrategy::CatSortUniq => {
                if !quiet {
                    println!("Concatenating unique lines in local and remote notes for {object}");
                }
                let merged = concatenate_unique_note_lines(store, &existing_note, &incoming_note)?;
                let merged_note = store.write_object(GitObjectKind::Blob, &merged)?;
                current.insert(object, merged_note);
                changed = true;
            }
        }
    }
    if !conflicts.is_empty() {
        write_notes_merge_state(
            repo,
            store,
            refs,
            ref_name,
            &source_ref,
            &current,
            &conflicts,
        )?;
        return Err(CliError::Stderr {
            code: 1,
            text: "Automatic notes merge failed. Fix conflicts in .git/NOTES_MERGE_WORKTREE and commit the result with 'git notes merge --commit', or abort the merge with 'git notes merge --abort'.\n".into(),
        });
    }
    if changed || strategy == NotesMergeStrategy::Ours {
        write_notes_ref(
            repo,
            store,
            refs,
            ref_name,
            &current,
            &format!("Merged notes from {source_ref} into {ref_name}"),
        )?;
    }
    Ok(())
}

enum NotesMergeAction {
    Merge {
        strategy: NotesMergeStrategy,
        source: String,
        quiet: bool,
    },
    Commit,
    Abort,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NotesMergeStrategy {
    Manual,
    Ours,
    Theirs,
    Union,
    CatSortUniq,
}

fn parse_notes_merge_args(args: Vec<String>) -> Result<NotesMergeAction> {
    let mut strategy = NotesMergeStrategy::Manual;
    let mut source = None;
    let mut commit = false;
    let mut abort = false;
    let mut quiet = false;
    let mut strategy_seen = false;
    let mut cursor = 0usize;
    while cursor < args.len() {
        match args[cursor].as_str() {
            "-s" | "--strategy" => {
                strategy_seen = true;
                cursor += 1;
                let Some(value) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes merge -s requires a strategy".into(),
                    });
                };
                strategy = parse_notes_merge_strategy(value)?;
            }
            value if value.starts_with("--strategy=") => {
                strategy_seen = true;
                let Some(strategy_name) = value.strip_prefix("--strategy=") else {
                    return Err(notes_unknown_option(value, notes_merge_usage()));
                };
                strategy = parse_notes_merge_strategy(strategy_name)?;
            }
            "--no-strategy" => {
                strategy_seen = false;
                strategy = NotesMergeStrategy::Manual;
            }
            "--commit" => commit = true,
            "--abort" => abort = true,
            "-q" | "--quiet" => quiet = true,
            "--no-quiet" | "-v" | "--verbose" | "--no-verbose" => quiet = false,
            value if value.starts_with('-') => {
                return Err(notes_unknown_option(value, notes_merge_usage()));
            }
            value => {
                if source.replace(value.to_owned()).is_some() {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "notes merge accepts exactly one notes ref".into(),
                    });
                }
            }
        }
        cursor += 1;
    }
    if commit && abort {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: cannot mix --commit, --abort or -s/--strategy\n".into(),
        });
    }
    if commit || abort {
        if strategy_seen {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: cannot mix --commit, --abort or -s/--strategy\n".into(),
            });
        }
        if source.is_some() {
            return Err(CliError::Stderr {
                code: 129,
                text: "error: too many arguments\n".into(),
            });
        }
        return Ok(if commit {
            NotesMergeAction::Commit
        } else {
            NotesMergeAction::Abort
        });
    }
    let Some(source) = source else {
        return Err(CliError::Stderr {
            code: 129,
            text: "error: must specify a notes ref to merge\n".into(),
        });
    };
    Ok(NotesMergeAction::Merge {
        strategy,
        source,
        quiet,
    })
}

fn parse_notes_merge_strategy(value: &str) -> Result<NotesMergeStrategy> {
    match value {
        "manual" => Ok(NotesMergeStrategy::Manual),
        "ours" => Ok(NotesMergeStrategy::Ours),
        "theirs" => Ok(NotesMergeStrategy::Theirs),
        "union" => Ok(NotesMergeStrategy::Union),
        "cat_sort_uniq" => Ok(NotesMergeStrategy::CatSortUniq),
        _ => Err(CliError::Stderr {
            code: 129,
            text: format!("error: unknown -s/--strategy: {value}\n"),
        }),
    }
}

fn concatenate_note_blobs(
    store: &OwnedCliObjectStoreAdapter,
    left: &ObjectId,
    right: &ObjectId,
    paragraph_break: bool,
) -> Result<Vec<u8>> {
    let mut content = read_note_blob(store, left)?;
    while content.ends_with(b"\n") {
        content.pop();
    }
    if !content.is_empty() {
        content.push(b'\n');
        if paragraph_break {
            content.push(b'\n');
        }
    }
    content.extend_from_slice(&read_note_blob(store, right)?);
    if !content.ends_with(b"\n") {
        content.push(b'\n');
    }
    Ok(content)
}

fn concatenate_unique_note_lines(
    store: &OwnedCliObjectStoreAdapter,
    left: &ObjectId,
    right: &ObjectId,
) -> Result<Vec<u8>> {
    let mut lines = BTreeSet::new();
    for blob in [read_note_blob(store, left)?, read_note_blob(store, right)?] {
        for line in String::from_utf8_lossy(&blob).lines() {
            lines.insert(line.to_owned());
        }
    }
    let mut content = lines
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    if !content.is_empty() {
        content.push(b'\n');
    }
    Ok(content)
}

fn read_note_blob(store: &OwnedCliObjectStoreAdapter, id: &ObjectId) -> Result<Vec<u8>> {
    let note = store.read_object(id)?;
    if note.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: "note object is not a blob".into(),
        });
    }
    Ok(note.content)
}

type NotesConflict = (String, ObjectId, ObjectId);

fn write_notes_merge_state(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    source_ref: &str,
    partial_notes: &HashMap<String, ObjectId>,
    conflicts: &[NotesConflict],
) -> Result<()> {
    let mut conflict_objects = conflicts
        .iter()
        .map(|(object, _, _)| object.clone())
        .collect::<Vec<_>>();
    conflict_objects.sort();
    let message = notes_merge_message(source_ref, ref_name, &conflict_objects);
    let mut parents = Vec::new();
    if let Ok(parent) = refs.resolve(ref_name) {
        parents.push(parent);
    }
    if let Ok(parent) = refs.resolve(source_ref) {
        parents.push(parent);
    }
    let partial_id = write_notes_commit(repo, store, partial_notes, parents, message.as_bytes())?;
    fs::write(notes_merge_ref_path(repo), format!("ref: {ref_name}\n"))?;
    fs::write(
        notes_merge_partial_path(repo),
        format!("{}\n", partial_id.to_hex()),
    )?;
    let worktree = notes_merge_worktree_path(repo);
    match fs::remove_dir_all(&worktree) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(&worktree)?;
    for (object, local_note, remote_note) in conflicts {
        fs::write(
            worktree.join(object),
            notes_conflict_file(store, ref_name, source_ref, local_note, remote_note)?,
        )?;
    }
    Ok(())
}

fn notes_merge_commit(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
) -> Result<()> {
    let partial_id = read_notes_merge_partial(repo)?;
    let ref_name = read_notes_merge_ref(repo)?;
    let object_store = store.as_object_store();
    let commit_cache = CommitObjectCache::new(object_store);
    let tree_cache = TreeObjectCache::new(object_store);
    let partial_commit = commit_cache.read_commit(&partial_id)?;
    let mut notes = HashMap::new();
    collect_notes_tree_cached(&tree_cache, &partial_commit.tree, String::new(), &mut notes)?;
    let worktree = notes_merge_worktree_path(repo);
    for entry in fs::read_dir(&worktree)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let object = entry
            .file_name()
            .into_string()
            .map_err(|_| CliError::Fatal {
                code: 128,
                message: "notes merge worktree contains non-UTF-8 path".into(),
            })?;
        let content = fs::read(entry.path())?;
        let note_id = store.write_object(GitObjectKind::Blob, &content)?;
        notes.insert(object, note_id);
    }
    let commit_id = write_notes_commit(
        repo,
        store,
        &notes,
        partial_commit.parents.clone(),
        &partial_commit.message,
    )?;
    refs.write_ref(&ref_name, &commit_id)?;
    clear_notes_merge_state(repo)?;
    Ok(())
}

fn notes_merge_abort(repo: &GitRepo) -> Result<()> {
    if !notes_merge_worktree_path(repo).exists() {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: failed to remove 'git notes merge' worktree\n".into(),
        });
    }
    clear_notes_merge_state(repo)
}

fn write_notes_commit(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    notes: &HashMap<String, ObjectId>,
    parents: Vec<ObjectId>,
    message: &[u8],
) -> Result<ObjectId> {
    let tree_id = write_notes_tree(store, notes)?;
    let author = signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let mut builder = CommitBuilder::new(tree_id, author, committer);
    for parent in parents {
        builder = builder.parent(parent);
    }
    let commit = builder.message(message.to_vec())?.encode()?;
    Ok(store.write_object(GitObjectKind::Commit, &commit)?)
}

fn notes_merge_message(source_ref: &str, ref_name: &str, conflicts: &[String]) -> String {
    let mut message = format!("Merged notes from {source_ref} into {ref_name}\n");
    if !conflicts.is_empty() {
        message.push_str("\nConflicts:\n");
        for conflict in conflicts {
            message.push('\t');
            message.push_str(conflict);
            message.push('\n');
        }
    }
    message
}

fn notes_conflict_file(
    store: &OwnedCliObjectStoreAdapter,
    ref_name: &str,
    source_ref: &str,
    local_note: &ObjectId,
    remote_note: &ObjectId,
) -> Result<Vec<u8>> {
    let mut content = Vec::new();
    content.extend_from_slice(format!("<<<<<<< {ref_name}\n").as_bytes());
    content.extend_from_slice(&read_note_blob(store, local_note)?);
    if !content.ends_with(b"\n") {
        content.push(b'\n');
    }
    content.extend_from_slice(b"=======\n");
    content.extend_from_slice(&read_note_blob(store, remote_note)?);
    if !content.ends_with(b"\n") {
        content.push(b'\n');
    }
    content.extend_from_slice(format!(">>>>>>> {source_ref}\n").as_bytes());
    Ok(content)
}

fn read_notes_merge_ref(repo: &GitRepo) -> Result<String> {
    let content = fs::read_to_string(notes_merge_ref_path(repo)).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            CliError::Fatal {
                code: 128,
                message: "failed to read ref NOTES_MERGE_REF".into(),
            }
        } else {
            CliError::Io(error)
        }
    })?;
    content
        .trim_end()
        .strip_prefix("ref: ")
        .map(str::to_owned)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "invalid NOTES_MERGE_REF".into(),
        })
}

fn read_notes_merge_partial(repo: &GitRepo) -> Result<ObjectId> {
    let content = fs::read_to_string(notes_merge_partial_path(repo)).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            CliError::Fatal {
                code: 128,
                message: "failed to read ref NOTES_MERGE_PARTIAL".into(),
            }
        } else {
            CliError::Io(error)
        }
    })?;
    ObjectId::from_hex(GitHashAlgorithm::Sha1, content.trim()).map_err(|_| CliError::Fatal {
        code: 128,
        message: "invalid NOTES_MERGE_PARTIAL".into(),
    })
}

fn clear_notes_merge_state(repo: &GitRepo) -> Result<()> {
    for path in [notes_merge_ref_path(repo), notes_merge_partial_path(repo)] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let worktree = notes_merge_worktree_path(repo);
    match fs::remove_dir_all(&worktree) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(worktree)?;
    Ok(())
}

fn notes_merge_ref_path(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("NOTES_MERGE_REF")
}

fn notes_merge_partial_path(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("NOTES_MERGE_PARTIAL")
}

fn notes_merge_worktree_path(repo: &GitRepo) -> PathBuf {
    repo.git_dir.join("NOTES_MERGE_WORKTREE")
}

#[derive(Clone, Copy)]
enum NotesResolveMode {
    Fatal,
    Error,
}

fn resolve_notes_objectish(
    repo: &GitRepo,
    object: &str,
    mode: NotesResolveMode,
) -> Result<ObjectId> {
    resolve_objectish(repo, object).map_err(|_| match mode {
        NotesResolveMode::Fatal => CliError::Fatal {
            code: 128,
            message: format!("failed to resolve '{object}' as a valid ref."),
        },
        NotesResolveMode::Error => {
            CliError::Message(format!("Failed to resolve '{object}' as a valid ref."))
        }
    })
}

pub(crate) fn read_notes_map(
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
) -> Result<HashMap<String, ObjectId>> {
    let object_store = store.as_object_store();
    let commit_cache = CommitObjectCache::new(object_store);
    let tree_cache = TreeObjectCache::new(object_store);
    let commit_id = match refs.resolve(ref_name) {
        Ok(id) => id,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let commit = commit_cache
        .read_commit(&commit_id)
        .map_err(|error| match error.kind() {
            io::ErrorKind::InvalidData => CliError::Fatal {
                code: 128,
                message: "notes ref does not point to a commit".into(),
            },
            _ => CliError::Io(error),
        })?;
    let mut notes = HashMap::new();
    collect_notes_tree_cached(&tree_cache, &commit.tree, String::new(), &mut notes)?;
    Ok(notes)
}

fn collect_notes_tree_cached<S: GitObjectStore>(
    tree_cache: &TreeObjectCache<'_, S>,
    tree_id: &ObjectId,
    prefix: String,
    notes: &mut HashMap<String, ObjectId>,
) -> Result<()> {
    for entry in tree_cache.read_tree(tree_id)?.iter() {
        let name = std::str::from_utf8(&entry.name).map_err(|_| CliError::Fatal {
            code: 128,
            message: "notes tree contains non-UTF-8 path".into(),
        })?;
        let path = format!("{prefix}{name}");
        match entry.mode {
            TreeMode::Tree => {
                collect_notes_tree_cached(tree_cache, &entry.id, path, notes)?;
            }
            TreeMode::File if path.len() == GitHashAlgorithm::Sha1.digest_len() * 2 => {
                notes.insert(path, entry.id.clone());
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_notes_ref(
    repo: &GitRepo,
    store: &OwnedCliObjectStoreAdapter,
    refs: &OwnedCliRefsStoreAdapter,
    ref_name: &str,
    notes: &HashMap<String, ObjectId>,
    message: &str,
) -> Result<()> {
    let tree_id = write_notes_tree(store, notes)?;
    let author = signature_from_identity(repo, "GIT_AUTHOR")?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let mut builder = CommitBuilder::new(tree_id, author, committer);
    if let Ok(parent) = refs.resolve(ref_name) {
        builder = builder.parent(parent);
    }
    let mut message = message.as_bytes().to_vec();
    message.push(b'\n');
    let commit = builder.message(message)?.encode()?;
    let commit_id = store.write_object(GitObjectKind::Commit, &commit)?;
    refs.write_ref(ref_name, &commit_id)?;
    Ok(())
}

fn write_notes_tree(
    store: &OwnedCliObjectStoreAdapter,
    notes: &HashMap<String, ObjectId>,
) -> Result<ObjectId> {
    let prefix_counts = notes_prefix_counts(notes);
    let mut root = NotesTreeNode::default();
    for (object, note) in notes {
        let path = notes_tree_path(object, &prefix_counts);
        root.insert(&path, note.clone());
    }
    root.write(store)
}

#[derive(Default)]
struct NotesTreeNode {
    files: BTreeMap<String, ObjectId>,
    dirs: BTreeMap<String, NotesTreeNode>,
}

impl NotesTreeNode {
    fn insert(&mut self, path: &str, note: ObjectId) {
        let mut node = self;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                node.files.insert(part.to_owned(), note);
                break;
            }
            node = node.dirs.entry(part.to_owned()).or_default();
        }
    }

    fn write(&self, store: &OwnedCliObjectStoreAdapter) -> Result<ObjectId> {
        let mut entries = Vec::with_capacity(self.files.len() + self.dirs.len());
        for (name, node) in &self.dirs {
            entries.push(TreeEntry::new(
                TreeMode::Tree,
                name.as_bytes(),
                node.write(store)?,
            )?);
        }
        for (name, note) in &self.files {
            entries.push(TreeEntry::new(
                TreeMode::File,
                name.as_bytes(),
                note.clone(),
            )?);
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let tree_content = encode_tree(&entries)?;
        Ok(store.write_object(GitObjectKind::Tree, &tree_content)?)
    }
}

fn notes_prefix_counts(notes: &HashMap<String, ObjectId>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for object in notes.keys() {
        for length in 1..=object.len() {
            *counts.entry(object[..length].to_owned()).or_insert(0) += 1;
        }
    }
    counts
}

fn notes_tree_path(object: &str, prefix_counts: &HashMap<String, usize>) -> String {
    let mut prefix = String::new();
    let mut depth = 0usize;
    let mut fanout = 0usize;
    while depth < object.len() {
        if notes_should_increase_fanout(&prefix, depth, fanout, prefix_counts) {
            fanout += 1;
        }
        let child = format!("{}{}", prefix, &object[depth..depth + 1]);
        if prefix_counts.get(&child).copied().unwrap_or(0) <= 1 {
            break;
        }
        prefix = child;
        depth += 1;
    }
    if fanout == 0 {
        return object.to_owned();
    }
    let mut path = String::with_capacity(object.len() + fanout);
    for index in 0..fanout {
        let offset = index * 2;
        path.push_str(&object[offset..offset + 2]);
        path.push('/');
    }
    path.push_str(&object[fanout * 2..]);
    path
}

fn notes_should_increase_fanout(
    prefix: &str,
    depth: usize,
    fanout: usize,
    prefix_counts: &HashMap<String, usize>,
) -> bool {
    if !depth.is_multiple_of(2) || depth > 2 * fanout {
        return false;
    }
    for digit in b"0123456789abcdef" {
        let child = format!("{prefix}{}", *digit as char);
        if prefix_counts.get(&child).copied().unwrap_or(0) <= 1 {
            return false;
        }
    }
    true
}
