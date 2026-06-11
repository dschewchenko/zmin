use super::*;
use skron_primitives::Error as PrimitiveError;
use skron_primitives::git_runtime::{GitObjectStore, GitPrimitiveRuntime, GitRefsStore};

pub(crate) fn pack_refs(all: bool, prune: bool, no_prune: bool) -> Result<()> {
    let repo = find_repo()?;
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    runtime
        .refs()
        .pack_refs(all, prune || !no_prune)
        .map_err(|error| map_primitive_error(error, "pack refs"))?;
    Ok(())
}

pub(crate) struct UpdateRefCommandOptions<'a> {
    pub(crate) delete: bool,
    pub(crate) no_deref: bool,
    pub(crate) stdin: bool,
    pub(crate) nul_terminated: bool,
    pub(crate) message: Option<&'a str>,
    pub(crate) create_reflog: bool,
    pub(crate) batch_updates: bool,
    pub(crate) name: Option<&'a str>,
    pub(crate) newvalue: Option<&'a str>,
}

pub(crate) fn update_ref(options: UpdateRefCommandOptions<'_>) -> Result<()> {
    let repo = find_repo()?;
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let refs = runtime.refs_store_adapter();
    if options.stdin {
        if options.delete || options.name.is_some() || options.newvalue.is_some() {
            return Err(CliError::Stderr {
                code: 129,
                text: update_ref_usage(),
            });
        }
        return update_ref_stdin(
            &repo,
            &refs,
            options.nul_terminated,
            options.no_deref,
            options.message,
            options.create_reflog,
            options.batch_updates,
        );
    }
    if options.nul_terminated || options.batch_updates {
        return Err(CliError::Stderr {
            code: 129,
            text: update_ref_usage(),
        });
    }
    let Some(name) = options.name else {
        return Err(CliError::Stderr {
            code: 129,
            text: update_ref_usage(),
        });
    };
    if options.delete {
        if options.newvalue.is_some() {
            return Err(CliError::Fatal {
                code: 129,
                message: "update-ref -d accepts only a ref name".into(),
            });
        }
        if !update_ref_name_is_valid(name) {
            return Ok(());
        }
        if name == "HEAD" {
            if options.no_deref {
                update_ref_delete(&repo, &refs, "HEAD", true)?;
            } else if let Ok(RefTarget::Symbolic(target)) = refs.read_head() {
                update_ref_delete(&repo, &refs, &target, true)?;
            }
            return Ok(());
        }
        update_ref_delete(&repo, &refs, name, true)?;
        return Ok(());
    }
    let Some(newvalue) = options.newvalue else {
        return Err(CliError::Stderr {
            code: 129,
            text: update_ref_usage(),
        });
    };
    update_ref_validate_cli_name(name)?;
    let id = resolve_objectish(&repo, newvalue).map_err(CliError::Io)?;
    update_ref_write(
        &repo,
        &refs,
        name,
        &id,
        options.no_deref,
        options.create_reflog,
        options.message,
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
enum UpdateRefStdinOp {
    Update {
        name: String,
        new_id: ObjectId,
        old_id: Option<ObjectId>,
        no_deref: bool,
    },
    Create {
        name: String,
        new_id: ObjectId,
        no_deref: bool,
    },
    Delete {
        name: String,
        old_id: Option<ObjectId>,
        no_deref: bool,
    },
    Verify {
        name: String,
        old_id: Option<ObjectId>,
    },
    SymrefUpdate {
        name: String,
        new_target: String,
        old: Option<SymrefOld>,
        no_deref: bool,
    },
    SymrefCreate {
        name: String,
        new_target: String,
    },
    SymrefDelete {
        name: String,
        old_target: Option<String>,
        no_deref: bool,
    },
    SymrefVerify {
        name: String,
        old_target: Option<String>,
        no_deref: bool,
    },
}

#[derive(Debug, Clone)]
enum SymrefOld {
    Target(String),
    Oid(ObjectId),
}

fn update_ref_stdin(
    repo: &GitRepo,
    refs: &RefStore,
    nul_terminated: bool,
    initial_no_deref: bool,
    message: Option<&str>,
    create_reflog: bool,
    batch_updates: bool,
) -> Result<()> {
    if nul_terminated {
        return update_ref_stdin_z(
            repo,
            refs,
            initial_no_deref,
            message,
            create_reflog,
            batch_updates,
        );
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let mut ops = Vec::new();
    let mut in_transaction = false;
    let mut prepared = false;
    let mut no_deref = initial_no_deref;
    for raw_line in input.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() {
            continue;
        }
        match line {
            "start" => {
                in_transaction = true;
                prepared = false;
                println!("start: ok");
            }
            "prepare" => {
                if batch_updates {
                    update_ref_validate_stdin_batch_ops(refs, &ops).map_err(|message| {
                        CliError::Fatal {
                            code: 128,
                            message: format!("prepare: {message}"),
                        }
                    })?;
                } else {
                    update_ref_validate_stdin_ops(refs, &ops).map_err(|message| {
                        CliError::Fatal {
                            code: 128,
                            message: format!("prepare: {message}"),
                        }
                    })?;
                }
                prepared = true;
                println!("prepare: ok");
            }
            "commit" => {
                if batch_updates {
                    if in_transaction && !prepared {
                        update_ref_validate_stdin_batch_ops(refs, &ops).map_err(|message| {
                            CliError::Fatal {
                                code: 128,
                                message: format!("commit: {message}"),
                            }
                        })?;
                    }
                    update_ref_apply_stdin_batch_ops(repo, refs, &ops, create_reflog, message)?;
                } else {
                    if in_transaction && !prepared {
                        update_ref_validate_stdin_ops(refs, &ops).map_err(|message| {
                            CliError::Fatal {
                                code: 128,
                                message: format!("commit: {message}"),
                            }
                        })?;
                    }
                    update_ref_apply_stdin_ops(repo, refs, &ops, create_reflog, message)?;
                }
                ops.clear();
                prepared = false;
                in_transaction = false;
                println!("commit: ok");
            }
            "abort" => {
                ops.clear();
                prepared = false;
                in_transaction = false;
                println!("abort: ok");
            }
            _ => {
                if let Some(option) = line.strip_prefix("option ") {
                    match option {
                        "no-deref" => no_deref = true,
                        _ => {
                            return Err(CliError::Fatal {
                                code: 128,
                                message: format!("option unknown: {option}"),
                            });
                        }
                    }
                } else {
                    ops.push(parse_update_ref_stdin_op(repo, line, no_deref)?);
                    no_deref = initial_no_deref;
                }
            }
        }
    }
    if !in_transaction && !ops.is_empty() {
        if batch_updates {
            update_ref_apply_stdin_batch_ops(repo, refs, &ops, create_reflog, message)?;
        } else {
            update_ref_validate_stdin_ops(refs, &ops)
                .map_err(|message| CliError::Fatal { code: 128, message })?;
            update_ref_apply_stdin_ops(repo, refs, &ops, create_reflog, message)?;
        }
    }
    Ok(())
}

fn update_ref_stdin_z(
    repo: &GitRepo,
    refs: &RefStore,
    initial_no_deref: bool,
    message: Option<&str>,
    create_reflog: bool,
    batch_updates: bool,
) -> Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let tokens = input.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut index = 0usize;
    let mut ops = Vec::new();
    let mut in_transaction = false;
    let mut prepared = false;
    let mut no_deref = initial_no_deref;
    while index < tokens.len() {
        if tokens[index].is_empty() && index + 1 == tokens.len() {
            break;
        }
        let token = update_ref_stdin_z_token(tokens[index])?;
        index += 1;
        if token.is_empty() {
            continue;
        }
        match token.as_str() {
            "start" => {
                in_transaction = true;
                prepared = false;
                println!("start: ok");
            }
            "prepare" => {
                if batch_updates {
                    update_ref_validate_stdin_batch_ops(refs, &ops).map_err(|message| {
                        CliError::Fatal {
                            code: 128,
                            message: format!("prepare: {message}"),
                        }
                    })?;
                } else {
                    update_ref_validate_stdin_ops(refs, &ops).map_err(|message| {
                        CliError::Fatal {
                            code: 128,
                            message: format!("prepare: {message}"),
                        }
                    })?;
                }
                prepared = true;
                println!("prepare: ok");
            }
            "commit" => {
                if batch_updates {
                    if in_transaction && !prepared {
                        update_ref_validate_stdin_batch_ops(refs, &ops).map_err(|message| {
                            CliError::Fatal {
                                code: 128,
                                message: format!("commit: {message}"),
                            }
                        })?;
                    }
                    update_ref_apply_stdin_batch_ops(repo, refs, &ops, create_reflog, message)?;
                } else {
                    if in_transaction && !prepared {
                        update_ref_validate_stdin_ops(refs, &ops).map_err(|message| {
                            CliError::Fatal {
                                code: 128,
                                message: format!("commit: {message}"),
                            }
                        })?;
                    }
                    update_ref_apply_stdin_ops(repo, refs, &ops, create_reflog, message)?;
                }
                ops.clear();
                prepared = false;
                in_transaction = false;
                println!("commit: ok");
            }
            "abort" => {
                ops.clear();
                prepared = false;
                in_transaction = false;
                println!("abort: ok");
            }
            _ => {
                if let Some(option) = token.strip_prefix("option ") {
                    match option {
                        "no-deref" => no_deref = true,
                        _ => {
                            return Err(CliError::Fatal {
                                code: 128,
                                message: format!("option unknown: {option}"),
                            });
                        }
                    }
                } else {
                    ops.push(parse_update_ref_stdin_z_op(
                        repo, &token, &tokens, &mut index, no_deref,
                    )?);
                    no_deref = initial_no_deref;
                }
            }
        }
    }
    if !in_transaction && !ops.is_empty() {
        if batch_updates {
            update_ref_apply_stdin_batch_ops(repo, refs, &ops, create_reflog, message)?;
        } else {
            update_ref_validate_stdin_ops(refs, &ops)
                .map_err(|message| CliError::Fatal { code: 128, message })?;
            update_ref_apply_stdin_ops(repo, refs, &ops, create_reflog, message)?;
        }
    }
    Ok(())
}

fn parse_update_ref_stdin_z_op(
    repo: &GitRepo,
    command: &str,
    tokens: &[&[u8]],
    index: &mut usize,
    no_deref: bool,
) -> Result<UpdateRefStdinOp> {
    let Some((verb, name)) = command.split_once(char::is_whitespace) else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("unknown command: {command}"),
        });
    };
    let name = name.trim_start();
    let parse_id = |value: String| update_ref_parse_stdin_id(repo, &value);
    let parse_optional_id = |value: String| {
        if value.is_empty() {
            Ok(None)
        } else {
            parse_id(value).map(Some)
        }
    };
    match verb {
        "update" => {
            let new = update_ref_stdin_z_next(tokens, index, command, "<new-oid>")?;
            let old = update_ref_stdin_z_next(tokens, index, command, "<old-oid>")?;
            Ok(UpdateRefStdinOp::Update {
                name: name.to_owned(),
                new_id: parse_id(new)?,
                old_id: parse_optional_id(old)?,
                no_deref,
            })
        }
        "create" => {
            let new = update_ref_stdin_z_next(tokens, index, command, "<new-oid>")?;
            Ok(UpdateRefStdinOp::Create {
                name: name.to_owned(),
                new_id: parse_id(new)?,
                no_deref,
            })
        }
        "delete" => {
            let old = update_ref_stdin_z_next(tokens, index, command, "<old-oid>")?;
            Ok(UpdateRefStdinOp::Delete {
                name: name.to_owned(),
                old_id: parse_optional_id(old)?,
                no_deref,
            })
        }
        "verify" => {
            let old = update_ref_stdin_z_next(tokens, index, command, "<old-oid>")?;
            Ok(UpdateRefStdinOp::Verify {
                name: name.to_owned(),
                old_id: parse_optional_id(old)?,
            })
        }
        "symref-update" => {
            let new_target = update_ref_stdin_z_next(tokens, index, command, "<new-target>")?;
            let old_kind = update_ref_stdin_z_next(tokens, index, command, "<old-target>")?;
            let old = match old_kind.as_str() {
                "" => None,
                "ref" => Some(SymrefOld::Target(update_ref_stdin_z_next(
                    tokens,
                    index,
                    command,
                    "<old-target>",
                )?)),
                "oid" => Some(SymrefOld::Oid(parse_id(update_ref_stdin_z_next(
                    tokens,
                    index,
                    command,
                    "<old-oid>",
                )?)?)),
                _ => {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: format!("unknown command: {command}"),
                    });
                }
            };
            Ok(UpdateRefStdinOp::SymrefUpdate {
                name: name.to_owned(),
                new_target,
                old,
                no_deref,
            })
        }
        "symref-create" => {
            let new_target = update_ref_stdin_z_next(tokens, index, command, "<new-target>")?;
            Ok(UpdateRefStdinOp::SymrefCreate {
                name: name.to_owned(),
                new_target,
            })
        }
        "symref-delete" => {
            let old_target = update_ref_stdin_z_next(tokens, index, command, "<old-target>")?;
            Ok(UpdateRefStdinOp::SymrefDelete {
                name: name.to_owned(),
                old_target: (!old_target.is_empty()).then_some(old_target),
                no_deref,
            })
        }
        "symref-verify" => {
            let old_target = update_ref_stdin_z_next(tokens, index, command, "<old-target>")?;
            Ok(UpdateRefStdinOp::SymrefVerify {
                name: name.to_owned(),
                old_target: (!old_target.is_empty()).then_some(old_target),
                no_deref,
            })
        }
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown command: {command}"),
        }),
    }
}

fn update_ref_stdin_z_next(
    tokens: &[&[u8]],
    index: &mut usize,
    command: &str,
    label: &str,
) -> Result<String> {
    if *index >= tokens.len() || (*index + 1 == tokens.len() && tokens[*index].is_empty()) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("{command}: unexpected end of input when reading {label}"),
        });
    }
    let value = update_ref_stdin_z_token(tokens[*index])?;
    *index += 1;
    Ok(value)
}

fn update_ref_stdin_z_token(token: &[u8]) -> Result<String> {
    String::from_utf8(token.to_vec()).map_err(|error| CliError::Fatal {
        code: 128,
        message: format!("invalid UTF-8 in stdin: {error}"),
    })
}

fn parse_update_ref_stdin_op(
    repo: &GitRepo,
    line: &str,
    no_deref: bool,
) -> Result<UpdateRefStdinOp> {
    let parts = parse_update_ref_stdin_line(line)?;
    let parse_id = |value: &str| update_ref_parse_stdin_id(repo, value);
    match parts
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["update", name, new] => Ok(UpdateRefStdinOp::Update {
            name: (*name).to_owned(),
            new_id: parse_id(new)?,
            old_id: None,
            no_deref,
        }),
        ["update", name, new, old] => Ok(UpdateRefStdinOp::Update {
            name: (*name).to_owned(),
            new_id: parse_id(new)?,
            old_id: Some(parse_id(old)?),
            no_deref,
        }),
        ["create", name, new] => Ok(UpdateRefStdinOp::Create {
            name: (*name).to_owned(),
            new_id: parse_id(new)?,
            no_deref,
        }),
        ["delete", name] => Ok(UpdateRefStdinOp::Delete {
            name: (*name).to_owned(),
            old_id: None,
            no_deref,
        }),
        ["delete", name, old] => Ok(UpdateRefStdinOp::Delete {
            name: (*name).to_owned(),
            old_id: Some(parse_id(old)?),
            no_deref,
        }),
        ["verify", name] => Ok(UpdateRefStdinOp::Verify {
            name: (*name).to_owned(),
            old_id: None,
        }),
        ["verify", name, old] => Ok(UpdateRefStdinOp::Verify {
            name: (*name).to_owned(),
            old_id: Some(parse_id(old)?),
        }),
        ["symref-update", name, new_target] => Ok(UpdateRefStdinOp::SymrefUpdate {
            name: (*name).to_owned(),
            new_target: (*new_target).to_owned(),
            old: None,
            no_deref,
        }),
        ["symref-update", name, new_target, "ref", old_target] => {
            Ok(UpdateRefStdinOp::SymrefUpdate {
                name: (*name).to_owned(),
                new_target: (*new_target).to_owned(),
                old: Some(SymrefOld::Target((*old_target).to_owned())),
                no_deref,
            })
        }
        ["symref-update", name, new_target, "oid", old_oid] => Ok(UpdateRefStdinOp::SymrefUpdate {
            name: (*name).to_owned(),
            new_target: (*new_target).to_owned(),
            old: Some(SymrefOld::Oid(parse_id(old_oid)?)),
            no_deref,
        }),
        ["symref-create", name, new_target] => Ok(UpdateRefStdinOp::SymrefCreate {
            name: (*name).to_owned(),
            new_target: (*new_target).to_owned(),
        }),
        ["symref-delete", name] => Ok(UpdateRefStdinOp::SymrefDelete {
            name: (*name).to_owned(),
            old_target: None,
            no_deref,
        }),
        ["symref-delete", name, old_target] => Ok(UpdateRefStdinOp::SymrefDelete {
            name: (*name).to_owned(),
            old_target: Some((*old_target).to_owned()),
            no_deref,
        }),
        ["symref-verify", name] => Ok(UpdateRefStdinOp::SymrefVerify {
            name: (*name).to_owned(),
            old_target: None,
            no_deref,
        }),
        ["symref-verify", name, old_target] => Ok(UpdateRefStdinOp::SymrefVerify {
            name: (*name).to_owned(),
            old_target: Some((*old_target).to_owned()),
            no_deref,
        }),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown command: {line}"),
        }),
    }
}

fn parse_update_ref_stdin_line(line: &str) -> Result<Vec<String>> {
    let bytes = line.as_bytes();
    let mut parts = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        if bytes[index] == b'"' {
            let bad_arg = &line[index..];
            index += 1;
            let mut token = Vec::new();
            let mut closed = false;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    b'\\' => {
                        index += 1;
                        if index >= bytes.len() {
                            return update_ref_badly_quoted(bad_arg);
                        }
                        parse_update_ref_c_escape(bytes, &mut index, &mut token, bad_arg)?;
                    }
                    byte => {
                        token.push(byte);
                        index += 1;
                    }
                }
            }
            if !closed || (index < bytes.len() && !bytes[index].is_ascii_whitespace()) {
                return update_ref_badly_quoted(bad_arg);
            }
            parts.push(String::from_utf8(token).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("badly quoted argument: {bad_arg}"),
            })?);
        } else {
            let start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            parts.push(line[start..index].to_owned());
        }
    }
    Ok(parts)
}

fn parse_update_ref_c_escape(
    bytes: &[u8],
    index: &mut usize,
    token: &mut Vec<u8>,
    line: &str,
) -> Result<()> {
    match bytes[*index] {
        b'a' => {
            token.push(0x07);
            *index += 1;
        }
        b'b' => {
            token.push(0x08);
            *index += 1;
        }
        b'f' => {
            token.push(0x0c);
            *index += 1;
        }
        b'n' => {
            token.push(b'\n');
            *index += 1;
        }
        b'r' => {
            token.push(b'\r');
            *index += 1;
        }
        b't' => {
            token.push(b'\t');
            *index += 1;
        }
        b'v' => {
            token.push(0x0b);
            *index += 1;
        }
        b'\\' | b'"' => {
            token.push(bytes[*index]);
            *index += 1;
        }
        b'0'..=b'7' => {
            let mut value = 0u8;
            let mut count = 0;
            while *index < bytes.len() && count < 3 && bytes[*index].is_ascii_digit() {
                let digit = bytes[*index] - b'0';
                if digit > 7 {
                    break;
                }
                value = (value << 3) + digit;
                *index += 1;
                count += 1;
            }
            token.push(value);
        }
        _ => return update_ref_badly_quoted(line),
    }
    Ok(())
}

fn update_ref_badly_quoted<T>(line: &str) -> Result<T> {
    Err(CliError::Fatal {
        code: 128,
        message: format!("badly quoted argument: {line}"),
    })
}

fn update_ref_parse_stdin_id(repo: &GitRepo, value: &str) -> Result<ObjectId> {
    if value == "0".repeat(40) {
        return ObjectId::from_hex(GitHashAlgorithm::Sha1, value).map_err(CliError::Io);
    }
    resolve_objectish(repo, value).map_err(CliError::Io)
}

fn update_ref_validate_stdin_ops(
    refs: &RefStore,
    ops: &[UpdateRefStdinOp],
) -> std::result::Result<(), String> {
    let mut seen = BTreeSet::new();
    for op in ops {
        let name = update_ref_stdin_op_name(op);
        update_ref_validate_stdin_name(name)?;
        if !seen.insert(name.to_owned()) {
            return Err(format!("multiple updates for ref '{name}' not allowed"));
        }
        match op {
            UpdateRefStdinOp::Update { name, old_id, .. } => {
                if let Some(expected) = old_id {
                    update_ref_verify_current(refs, name, expected)?;
                }
            }
            UpdateRefStdinOp::Create { name, .. } => {
                if refs.resolve(name).is_ok() {
                    return Err(format!(
                        "cannot lock ref '{name}': reference already exists"
                    ));
                }
            }
            UpdateRefStdinOp::Delete { name, old_id, .. }
            | UpdateRefStdinOp::Verify { name, old_id } => {
                if let Some(expected) = old_id {
                    update_ref_verify_current(refs, name, expected)?;
                }
            }
            UpdateRefStdinOp::SymrefUpdate {
                name,
                new_target,
                old,
                no_deref,
                ..
            } => {
                update_ref_validate_stdin_name(new_target)?;
                if let Some(old) = old {
                    if let SymrefOld::Target(target) = old {
                        update_ref_validate_stdin_name(target)?;
                    }
                    update_ref_verify_symref_old(refs, name, old, *no_deref)?;
                }
            }
            UpdateRefStdinOp::SymrefCreate {
                name, new_target, ..
            } => {
                update_ref_validate_stdin_name(new_target)?;
                if update_ref_read_raw(refs, name).is_ok() {
                    return Err(format!(
                        "cannot lock ref '{name}': reference already exists"
                    ));
                }
            }
            UpdateRefStdinOp::SymrefDelete {
                name,
                old_target,
                no_deref,
            }
            | UpdateRefStdinOp::SymrefVerify {
                name,
                old_target,
                no_deref,
            } => {
                if let Some(target) = old_target {
                    update_ref_validate_stdin_name(target)?;
                }
                if !no_deref {
                    let command = match op {
                        UpdateRefStdinOp::SymrefDelete { .. } => "symref-delete",
                        _ => "symref-verify",
                    };
                    return Err(format!("{command}: cannot operate with deref mode"));
                }
                update_ref_verify_symref_target(refs, name, old_target.as_deref())?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct UpdateRefBatchRejection {
    name: String,
    new_id: Option<ObjectId>,
    old_id: Option<ObjectId>,
    reason: &'static str,
}

fn update_ref_validate_stdin_batch_ops(
    refs: &RefStore,
    ops: &[UpdateRefStdinOp],
) -> std::result::Result<(Vec<UpdateRefStdinOp>, Vec<UpdateRefBatchRejection>), String> {
    let mut seen = BTreeSet::new();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for op in ops {
        let name = update_ref_stdin_op_name(op);
        update_ref_validate_stdin_name(name)?;
        if !seen.insert(name.to_owned()) {
            return Err(format!("multiple updates for ref '{name}' not allowed"));
        }
        match op {
            UpdateRefStdinOp::Update {
                name,
                new_id,
                old_id,
                ..
            } => {
                if let Some(expected) = old_id {
                    match update_ref_verify_current_for_batch(refs, name, expected) {
                        Ok(()) => accepted.push(op.clone()),
                        Err(reason) => rejected.push(UpdateRefBatchRejection {
                            name: name.clone(),
                            new_id: Some(new_id.clone()),
                            old_id: Some(expected.clone()),
                            reason,
                        }),
                    }
                } else {
                    accepted.push(op.clone());
                }
            }
            UpdateRefStdinOp::Create { name, .. } => {
                if let Ok(current) = refs.resolve(name) {
                    rejected.push(UpdateRefBatchRejection {
                        name: name.clone(),
                        new_id: Some(current),
                        old_id: Some(update_ref_zero_id()?),
                        reason: "reference already exists",
                    });
                } else {
                    accepted.push(op.clone());
                }
            }
            UpdateRefStdinOp::Delete { name, old_id, .. } => {
                if let Some(expected) = old_id {
                    match update_ref_verify_current_for_batch(refs, name, expected) {
                        Ok(()) => accepted.push(op.clone()),
                        Err(reason) => rejected.push(UpdateRefBatchRejection {
                            name: name.clone(),
                            new_id: Some(update_ref_zero_id()?),
                            old_id: Some(expected.clone()),
                            reason,
                        }),
                    }
                } else {
                    accepted.push(op.clone());
                }
            }
            UpdateRefStdinOp::Verify { name, old_id } => {
                if let Some(expected) = old_id
                    && let Err(reason) = update_ref_verify_current_for_batch(refs, name, expected)
                {
                    rejected.push(UpdateRefBatchRejection {
                        name: name.clone(),
                        new_id: None,
                        old_id: Some(expected.clone()),
                        reason,
                    });
                }
            }
            UpdateRefStdinOp::SymrefUpdate {
                name,
                new_target,
                old,
                no_deref,
            } => {
                update_ref_validate_stdin_name(new_target)?;
                if let Some(old) = old {
                    if let SymrefOld::Target(target) = old {
                        update_ref_validate_stdin_name(target)?;
                    }
                    match update_ref_verify_symref_old(refs, name, old, *no_deref) {
                        Ok(()) => accepted.push(op.clone()),
                        Err(_) => rejected.push(UpdateRefBatchRejection {
                            name: name.clone(),
                            new_id: None,
                            old_id: None,
                            reason: "incorrect old value provided",
                        }),
                    }
                } else {
                    accepted.push(op.clone());
                }
            }
            UpdateRefStdinOp::SymrefCreate {
                name, new_target, ..
            } => {
                update_ref_validate_stdin_name(new_target)?;
                if update_ref_read_raw(refs, name).is_ok() {
                    rejected.push(UpdateRefBatchRejection {
                        name: name.clone(),
                        new_id: None,
                        old_id: None,
                        reason: "reference already exists",
                    });
                } else {
                    accepted.push(op.clone());
                }
            }
            UpdateRefStdinOp::SymrefDelete {
                name,
                old_target,
                no_deref,
            }
            | UpdateRefStdinOp::SymrefVerify {
                name,
                old_target,
                no_deref,
            } => {
                if let Some(target) = old_target {
                    update_ref_validate_stdin_name(target)?;
                }
                if !no_deref {
                    let command = match op {
                        UpdateRefStdinOp::SymrefDelete { .. } => "symref-delete",
                        _ => "symref-verify",
                    };
                    return Err(format!("{command}: cannot operate with deref mode"));
                }
                match update_ref_verify_symref_target(refs, name, old_target.as_deref()) {
                    Ok(()) => {
                        if matches!(op, UpdateRefStdinOp::SymrefDelete { .. }) {
                            accepted.push(op.clone());
                        }
                    }
                    Err(_) => rejected.push(UpdateRefBatchRejection {
                        name: name.clone(),
                        new_id: None,
                        old_id: None,
                        reason: "incorrect old value provided",
                    }),
                }
            }
        }
    }
    Ok((accepted, rejected))
}

fn update_ref_stdin_op_name(op: &UpdateRefStdinOp) -> &str {
    match op {
        UpdateRefStdinOp::Update { name, .. }
        | UpdateRefStdinOp::Create { name, .. }
        | UpdateRefStdinOp::Delete { name, .. }
        | UpdateRefStdinOp::Verify { name, .. }
        | UpdateRefStdinOp::SymrefUpdate { name, .. }
        | UpdateRefStdinOp::SymrefCreate { name, .. }
        | UpdateRefStdinOp::SymrefDelete { name, .. }
        | UpdateRefStdinOp::SymrefVerify { name, .. } => name,
    }
}

fn update_ref_validate_cli_name(name: &str) -> Result<()> {
    if update_ref_name_is_valid(name) {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!(
                "update_ref failed for ref '{name}': refusing to update ref with bad name '{name}'"
            ),
        })
    }
}

fn update_ref_validate_stdin_name(name: &str) -> std::result::Result<(), String> {
    if update_ref_name_is_valid(name) {
        Ok(())
    } else {
        Err(format!("invalid ref format: {name}"))
    }
}

fn update_ref_name_is_valid(name: &str) -> bool {
    name == "HEAD" || check_ref_format(name, false)
}

fn update_ref_verify_current(
    refs: &RefStore,
    name: &str,
    expected: &ObjectId,
) -> std::result::Result<(), String> {
    let current = refs.resolve(name).ok();
    let zero = ObjectId::from_hex(GitHashAlgorithm::Sha1, &"0".repeat(40))
        .map_err(|error| error.to_string())?;
    match (current, expected == &zero) {
        (None, true) => Ok(()),
        (Some(current), false) if &current == expected => Ok(()),
        (Some(current), _) => Err(format!(
            "cannot lock ref '{name}': is at {} but expected {}",
            current.to_hex(),
            expected.to_hex()
        )),
        (None, false) => Err(format!(
            "cannot lock ref '{name}': unable to resolve reference '{name}'"
        )),
    }
}

fn update_ref_verify_symref_old(
    refs: &RefStore,
    name: &str,
    old: &SymrefOld,
    no_deref: bool,
) -> std::result::Result<(), String> {
    match old {
        SymrefOld::Target(target) => update_ref_verify_symref_target(refs, name, Some(target)),
        SymrefOld::Oid(expected) => {
            if no_deref {
                match update_ref_read_raw(refs, name) {
                    Ok(RefTarget::Direct(current)) if &current == expected => Ok(()),
                    Ok(RefTarget::Direct(current)) => Err(format!(
                        "cannot lock ref '{name}': is at {} but expected {}",
                        current.to_hex(),
                        expected.to_hex()
                    )),
                    Ok(RefTarget::Symbolic(target)) => Err(format!(
                        "cannot lock ref '{name}': expected object id but found symref target '{target}'"
                    )),
                    Err(_) if update_ref_is_zero(expected) => Ok(()),
                    Err(_) => Err(format!(
                        "cannot lock ref '{name}': unable to resolve reference '{name}'"
                    )),
                }
            } else {
                update_ref_verify_current(refs, name, expected)
            }
        }
    }
}

fn update_ref_verify_symref_target(
    refs: &RefStore,
    name: &str,
    expected: Option<&str>,
) -> std::result::Result<(), String> {
    match (update_ref_read_raw(refs, name), expected) {
        (Err(_), None) => Ok(()),
        (Ok(RefTarget::Symbolic(current)), Some(expected)) if current == expected => Ok(()),
        (Ok(RefTarget::Symbolic(current)), Some(expected)) => Err(format!(
            "verifying symref target: '{name}': is at {current} but expected {expected}"
        )),
        (Ok(RefTarget::Symbolic(current)), None) => Err(format!(
            "cannot lock ref '{name}': reference already exists with target '{current}'"
        )),
        (Ok(RefTarget::Direct(_)), Some(expected)) => Err(format!(
            "cannot lock ref '{name}': expected symref with target '{expected}': but is a regular ref"
        )),
        (Ok(RefTarget::Direct(_)), None) => Err(format!(
            "cannot lock ref '{name}': reference already exists"
        )),
        (Err(_), Some(expected)) => Err(format!(
            "cannot lock ref '{name}': expected symref with target '{expected}': but the reference is missing"
        )),
    }
}

fn update_ref_read_raw(refs: &RefStore, name: &str) -> io::Result<RefTarget> {
    if name == "HEAD" {
        refs.read_head()
    } else {
        refs.read_ref(name)
    }
}

fn update_ref_is_zero(id: &ObjectId) -> bool {
    id.to_hex() == "0".repeat(40)
}

fn update_ref_zero_id() -> std::result::Result<ObjectId, String> {
    ObjectId::from_hex(GitHashAlgorithm::Sha1, &"0".repeat(40)).map_err(|error| error.to_string())
}

fn update_ref_verify_current_for_batch(
    refs: &RefStore,
    name: &str,
    expected: &ObjectId,
) -> std::result::Result<(), &'static str> {
    match (refs.resolve(name).ok(), update_ref_is_zero(expected)) {
        (None, true) => Ok(()),
        (Some(current), false) if &current == expected => Ok(()),
        (Some(_), _) => Err("incorrect old value provided"),
        (None, false) => Err("reference does not exist"),
    }
}

fn update_ref_apply_stdin_ops(
    repo: &GitRepo,
    refs: &RefStore,
    ops: &[UpdateRefStdinOp],
    create_reflog: bool,
    message: Option<&str>,
) -> Result<()> {
    for op in ops {
        match op {
            UpdateRefStdinOp::Update {
                name,
                new_id,
                no_deref,
                ..
            }
            | UpdateRefStdinOp::Create {
                name,
                new_id,
                no_deref,
            } => update_ref_write(repo, refs, name, new_id, *no_deref, create_reflog, message)?,
            UpdateRefStdinOp::Delete { name, no_deref, .. } => {
                update_ref_delete(repo, refs, name, *no_deref)?;
            }
            UpdateRefStdinOp::Verify { .. } => {}
            UpdateRefStdinOp::SymrefUpdate {
                name, new_target, ..
            }
            | UpdateRefStdinOp::SymrefCreate {
                name, new_target, ..
            } => refs.write_symbolic_ref(name, new_target)?,
            UpdateRefStdinOp::SymrefDelete { name, no_deref, .. } => {
                update_ref_delete(repo, refs, name, *no_deref)?;
            }
            UpdateRefStdinOp::SymrefVerify { .. } => {}
        }
    }
    Ok(())
}

fn update_ref_apply_stdin_batch_ops(
    repo: &GitRepo,
    refs: &RefStore,
    ops: &[UpdateRefStdinOp],
    create_reflog: bool,
    message: Option<&str>,
) -> Result<()> {
    let (accepted, rejected) = update_ref_validate_stdin_batch_ops(refs, ops)
        .map_err(|message| CliError::Fatal { code: 128, message })?;
    update_ref_apply_stdin_ops(repo, refs, &accepted, create_reflog, message)?;
    for rejection in rejected {
        println!(
            "rejected {} {} {} {}",
            rejection.name,
            update_ref_rejection_id(rejection.new_id),
            update_ref_rejection_id(rejection.old_id),
            rejection.reason
        );
    }
    Ok(())
}

fn update_ref_rejection_id(id: Option<ObjectId>) -> String {
    id.map(|id| id.to_hex())
        .unwrap_or_else(|| "(null)".to_owned())
}

fn update_ref_write(
    repo: &GitRepo,
    refs: &RefStore,
    name: &str,
    id: &ObjectId,
    no_deref: bool,
    create_reflog: bool,
    message: Option<&str>,
) -> Result<()> {
    if name == "HEAD" && no_deref {
        let old_id = update_ref_reflog_old_id(refs, name, true)?;
        refs.write_head_direct(id)?;
        update_ref_record_reflogs(repo, refs, name, &old_id, id, create_reflog, message)?;
        return Ok(());
    }
    let effective_name = update_ref_effective_name(refs, name, no_deref);
    let old_id = update_ref_reflog_old_id(refs, &effective_name, true)?;
    refs.write_ref(&effective_name, id)?;
    update_ref_record_reflogs(
        repo,
        refs,
        &effective_name,
        &old_id,
        id,
        create_reflog,
        message,
    )?;
    if name == "HEAD" && effective_name != "HEAD" {
        update_ref_append_reflog(repo, "HEAD", &old_id, id, message)?;
    }
    Ok(())
}

fn update_ref_delete(repo: &GitRepo, refs: &RefStore, name: &str, no_deref: bool) -> Result<()> {
    let effective_name = update_ref_effective_name(refs, name, no_deref);
    if name == "HEAD" && no_deref {
        match fs::remove_file(refs.git_dir().join("HEAD")) {
            Ok(()) => {
                update_ref_remove_reflog(repo, "HEAD")?;
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    match refs.delete_ref(&effective_name) {
        Ok(()) => {
            update_ref_remove_reflog(repo, &effective_name)?;
            if name == "HEAD" || update_ref_points_head_at(refs, &effective_name) {
                update_ref_remove_reflog(repo, "HEAD")?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn update_ref_effective_name(refs: &RefStore, name: &str, no_deref: bool) -> String {
    if name == "HEAD"
        && !no_deref
        && let Ok(RefTarget::Symbolic(target)) = refs.read_head()
    {
        return target;
    }
    name.to_owned()
}

fn update_ref_reflog_old_id(refs: &RefStore, name: &str, no_deref: bool) -> Result<ObjectId> {
    if name == "HEAD" && no_deref {
        return match refs.read_head() {
            Ok(RefTarget::Direct(id)) => Ok(id),
            Ok(RefTarget::Symbolic(target)) => {
                refs.resolve(&target).or_else(|_| Ok(zero_object_id()))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(zero_object_id()),
            Err(error) => Err(CliError::Io(error)),
        };
    }
    match refs.resolve(name) {
        Ok(id) => Ok(id),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(zero_object_id()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn update_ref_record_reflogs(
    repo: &GitRepo,
    refs: &RefStore,
    name: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    create_reflog: bool,
    message: Option<&str>,
) -> Result<()> {
    if update_ref_should_write_reflog(name, create_reflog) {
        update_ref_append_reflog(repo, name, old_id, new_id, message)?;
    }
    if name != "HEAD" && update_ref_points_head_at(refs, name) {
        update_ref_append_reflog(repo, "HEAD", old_id, new_id, message)?;
    }
    Ok(())
}

fn update_ref_should_write_reflog(name: &str, create_reflog: bool) -> bool {
    create_reflog
        || name == "HEAD"
        || name.starts_with("refs/heads/")
        || name.starts_with("refs/remotes/")
        || name.starts_with("refs/notes/")
}

fn update_ref_points_head_at(refs: &RefStore, name: &str) -> bool {
    matches!(refs.read_head(), Ok(RefTarget::Symbolic(target)) if target == name)
}

fn update_ref_append_reflog(
    repo: &GitRepo,
    name: &str,
    old_id: &ObjectId,
    new_id: &ObjectId,
    message: Option<&str>,
) -> Result<()> {
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let path = repo.git_dir.join("logs").join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        file,
        "{} {} {} <{}> {} {}\t{}",
        old_id.to_hex(),
        new_id.to_hex(),
        committer.name,
        committer.email,
        committer.timestamp,
        committer.timezone,
        message.unwrap_or("")
    )?;
    Ok(())
}

fn update_ref_remove_reflog(repo: &GitRepo, name: &str) -> Result<()> {
    match fs::remove_file(repo.git_dir.join("logs").join(name)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn symbolic_ref(
    quiet: bool,
    short: bool,
    name: &str,
    target: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let refs = runtime.refs();
    if target.len() > 1 {
        return Err(CliError::Stderr {
            code: 129,
            text: symbolic_ref_usage(),
        });
    }
    if let Some(target) = target.first() {
        let name = name.to_owned();
        let target = target.to_owned();
        refs.write_symbolic_ref(&name, &target)
            .map_err(|error| map_primitive_error(error, "write symbolic ref"))?;
        return Ok(());
    }

    let target = match refs.read_symbolic_ref(&name.to_owned()) {
        Ok(Some(target)) => target,
        Ok(None) if quiet => return Err(CliError::Exit(1)),
        Ok(None) => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("ref {name} is not a symbolic ref"),
            });
        }
        Err(error) => {
            return Err(map_primitive_error(error, "read symbolic ref"));
        }
    };
    if short {
        println!("{}", branch_display_name(&target));
    } else {
        println!("{target}");
    }
    Ok(())
}

pub(crate) fn refs_command(command: RefsCommand) -> Result<()> {
    match command {
        RefsCommand::Verify { strict, verbose } => refs_verify(strict, verbose),
    }
}

fn refs_verify(_strict: bool, verbose: bool) -> Result<()> {
    let repo = find_repo()?;
    let refs = RefStore::new(repo.git_dir, GitHashAlgorithm::Sha1);
    if verbose {
        eprintln!("Checking references consistency");
    }
    if let Ok(RefTarget::Symbolic(target)) = refs.read_head() {
        update_ref_validate_stdin_name(&target)
            .map_err(|message| CliError::Fatal { code: 128, message })?;
    }
    refs.for_each_resolved_ref("refs/", |name, _| {
        if verbose {
            eprintln!("Checking {name}");
        }
        update_ref_validate_stdin_name(&name)
            .map_err(|message| CliError::Fatal { code: 128, message })?;
        Ok::<(), CliError>(())
    })?;
    if verbose {
        eprintln!("Checking packed-refs file .git/packed-refs");
    }
    Ok(())
}

pub(crate) fn repo_command(command: RepoCommand) -> Result<()> {
    match command {
        RepoCommand::Info {
            format,
            nul_terminated,
            all,
            keys,
            keys_or_values,
        } => repo_info(format.as_deref(), nul_terminated, all, keys, keys_or_values),
        RepoCommand::Structure {
            format,
            nul_terminated,
        } => repo_structure(format.as_deref(), nul_terminated),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepoOutputFormat {
    Lines,
    Nul,
    Table,
}

fn repo_info(
    format: Option<&str>,
    nul_terminated: bool,
    all: bool,
    keys: bool,
    requested: Vec<String>,
) -> Result<()> {
    let format = repo_output_format(format, nul_terminated, false)?;
    let available = repo_info_keys();
    if keys {
        if all || !requested.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "repo info --keys cannot be combined with keys or --all".into(),
            });
        }
        return print_repo_keys(&available, format);
    }
    let keys = if all {
        if !requested.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "repo info --all cannot be combined with explicit keys".into(),
            });
        }
        available.iter().map(|key| (*key).to_owned()).collect()
    } else if requested.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "repo info requires --all, --keys, or at least one key".into(),
        });
    } else {
        requested
    };
    let repo = find_repo_or_bare()?;
    for key in keys {
        if !available.contains(&key.as_str()) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown repo info key '{key}'"),
            });
        }
        let value = repo_info_value(&repo, &key)?;
        match format {
            RepoOutputFormat::Lines => println!("{key}={value}"),
            RepoOutputFormat::Nul => {
                print!("{key}\n{value}");
                io::stdout().write_all(&[0])?;
            }
            RepoOutputFormat::Table => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "repo info does not support table output".into(),
                });
            }
        }
    }
    Ok(())
}

fn repo_info_keys() -> [&'static str; 4] {
    [
        "layout.bare",
        "layout.shallow",
        "object.format",
        "references.format",
    ]
}

fn print_repo_keys(keys: &[&str], format: RepoOutputFormat) -> Result<()> {
    for key in keys {
        match format {
            RepoOutputFormat::Lines => println!("{key}"),
            RepoOutputFormat::Nul => {
                print!("{key}");
                io::stdout().write_all(&[0])?;
            }
            RepoOutputFormat::Table => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "repo info keys do not support table output".into(),
                });
            }
        }
    }
    Ok(())
}

fn repo_info_value(repo: &GitRepo, key: &str) -> Result<String> {
    match key {
        "layout.bare" => Ok(repo_is_bare(repo).to_string()),
        "layout.shallow" => Ok(repo.git_dir.join("shallow").is_file().to_string()),
        "object.format" => Ok("sha1".to_owned()),
        "references.format" => Ok("files".to_owned()),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown repo info key '{key}'"),
        }),
    }
}

fn repo_structure(format: Option<&str>, nul_terminated: bool) -> Result<()> {
    let format = repo_output_format(format, nul_terminated, true)?;
    let repo = find_repo_or_bare()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let loose = collect_loose_object_stats(&repo.objects_dir, GitHashAlgorithm::Sha1, false)?;
    let pack = collect_pack_object_stats(&repo.objects_dir)?;
    let mut reference_count = 0usize;
    refs.for_each_ref_name("refs/", |_| {
        reference_count = reference_count.saturating_add(1);
        Ok::<(), CliError>(())
    })?;
    let rows = [
        ("references.count", reference_count.to_string()),
        ("objects.loose.count", loose.count.to_string()),
        ("objects.packed.count", pack.objects.to_string()),
        (
            "objects.total.count",
            (loose.count + pack.objects).to_string(),
        ),
    ];
    match format {
        RepoOutputFormat::Table => {
            println!("Repository structure");
            for (key, value) in rows {
                println!("{key:<24} {value}");
            }
        }
        RepoOutputFormat::Lines => {
            for (key, value) in rows {
                println!("{key}={value}");
            }
        }
        RepoOutputFormat::Nul => {
            for (key, value) in rows {
                print!("{key}\n{value}");
                io::stdout().write_all(&[0])?;
            }
        }
    }
    Ok(())
}

fn repo_output_format(
    format: Option<&str>,
    nul_terminated: bool,
    allow_table: bool,
) -> Result<RepoOutputFormat> {
    let format = if nul_terminated { Some("nul") } else { format };
    match format {
        None if allow_table => Ok(RepoOutputFormat::Table),
        None => Ok(RepoOutputFormat::Lines),
        Some("lines") => Ok(RepoOutputFormat::Lines),
        Some("nul") => Ok(RepoOutputFormat::Nul),
        Some("table") if allow_table => Ok(RepoOutputFormat::Table),
        Some(other) => Err(CliError::Fatal {
            code: 129,
            message: format!("unsupported repo output format '{other}'"),
        }),
    }
}

fn update_ref_usage() -> String {
    "usage: git update-ref [<options>] -d <refname> [<old-oid>]
   or: git update-ref [<options>]    <refname> <new-oid> [<old-oid>]
   or: git update-ref [<options>] --stdin [-z] [--batch-updates]

    -m <reason>           reason of the update
    -d                    delete the reference
    --no-deref            update <refname> not the one it points to
    --deref               opposite of --no-deref
    -z                    stdin has NUL-terminated arguments
    --[no-]stdin          read updates from stdin
    --[no-]create-reflog  create a reflog
    -0, --[no-]batch-updates
                          batch reference updates

"
    .to_owned()
}

fn symbolic_ref_usage() -> String {
    "usage: git symbolic-ref [-m <reason>] <name> <ref>
   or: git symbolic-ref [-q] [--short] [--no-recurse] <name>
   or: git symbolic-ref --delete [-q] <name>

    -q, --[no-]quiet      suppress error message for non-symbolic (detached) refs
    -d, --[no-]delete     delete symbolic ref
    --[no-]short          shorten ref output
    --[no-]recurse        recursively dereference (default)
    -m <reason>           reason of the update

"
    .to_owned()
}

#[derive(Debug, Clone, Copy)]
struct ShowRefFormat {
    hash: Option<usize>,
    abbrev: Option<usize>,
}

fn show_ref_matches(ref_name: &str, pattern: &str) -> bool {
    ref_name == pattern || ref_name.ends_with(&format!("/{pattern}"))
}

fn print_show_ref_row(id: &ObjectId, name: &str, format: ShowRefFormat) -> Result<()> {
    let mut hex = id.to_hex();
    if let Some(length) = format.hash.or(format.abbrev) {
        hex.truncate(length.min(hex.len()));
    }
    if format.hash.is_some() {
        println!("{hex}");
    } else {
        println!("{hex} {name}");
    }
    Ok(())
}

pub(crate) fn show_ref(
    head: bool,
    heads: bool,
    tags: bool,
    hash: Option<usize>,
    abbrev: Option<usize>,
    verify: bool,
    patterns: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    let refs = runtime.refs();
    let format = ShowRefFormat { hash, abbrev };
    if verify {
        if patterns.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "--verify requires at least one ref".into(),
            });
        }
        for ref_name in patterns {
            let Some(raw_id) = refs
                .read_ref(&ref_name)
                .map_err(|error| map_primitive_error(error, "resolve reference"))?
            else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("'{ref_name}' - not a valid ref"),
                });
            };
            let id = parse_primitive_object_id(&raw_id).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("'{ref_name}' - not a valid ref"),
            })?;
            print_show_ref_row(&id, &ref_name, format)?;
        }
        return Ok(());
    }
    let prefixes = match (heads, tags) {
        (false, false) => vec!["refs/"],
        (true, false) => vec!["refs/heads/"],
        (false, true) => vec!["refs/tags/"],
        (true, true) => vec!["refs/heads/", "refs/tags/"],
    };
    let mut rows = BTreeMap::new();
    for prefix in prefixes {
        for (name, raw_id) in refs
            .list_refs(Some(prefix))
            .map_err(|error| map_primitive_error(error, "list references"))?
        {
            if patterns.is_empty()
                || patterns
                    .iter()
                    .any(|pattern| show_ref_matches(&name, pattern))
            {
                let id = parse_primitive_object_id(&raw_id).map_err(|error| CliError::Fatal {
                    code: 128,
                    message: format!("show-ref metadata decode failed for '{name}': {error:?}"),
                })?;
                rows.insert(name, id);
            }
        }
    }
    let head_id = if head {
        match refs.read_ref(&"HEAD".to_owned()) {
            Ok(Some(raw_id)) => {
                Some(
                    parse_primitive_object_id(&raw_id).map_err(|error| CliError::Fatal {
                        code: 128,
                        message: format!("HEAD metadata decode failed: {error:?}"),
                    })?,
                )
            }
            Ok(None) => None,
            Err(error) => return Err(map_primitive_error(error, "read HEAD")),
        }
    } else {
        None
    };
    if rows.is_empty() && head_id.is_none() {
        return Err(CliError::Exit(1));
    }
    if let Some(id) = head_id {
        print_show_ref_row(&id, "HEAD", format)?;
    }
    for (name, id) in rows {
        print_show_ref_row(&id, &name, format)?;
    }
    Ok(())
}

pub(crate) fn for_each_ref(
    format: Option<&str>,
    sort: Vec<String>,
    patterns: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let format = format.unwrap_or("%(objectname) %(objecttype)\t%(refname)");
    let requirements = for_each_ref_requirements(format, &sort)?;
    if sort.is_empty()
        && let Some(parts) = simple_for_each_ref_format_parts(format)
    {
        let refs = OwnedCliRefsStoreAdapter::from_path(&repo.git_dir, GitHashAlgorithm::Sha1);
        print_simple_for_each_ref_rows(&refs, &patterns, &parts)?;
        return Ok(());
    }
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    if sort.is_empty() {
        print_for_each_ref_rows(
            runtime.refs(),
            runtime.objects(),
            &patterns,
            format,
            &requirements,
        )?;
        return Ok(());
    }
    let mut rows =
        collect_for_each_ref_rows(runtime.refs(), runtime.objects(), &patterns, &requirements)?;
    apply_for_each_ref_sort(&mut rows, &sort)?;
    for row in &rows {
        println!("{}", render_for_each_ref_row(format, row)?);
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct ForEachRefRow {
    pub(crate) ref_name: String,
    pub(crate) object_id: ObjectId,
    pub(crate) object_kind: GitObjectKind,
    pub(crate) subject: String,
    pub(crate) tagger_name: String,
    pub(crate) tagger_email: String,
    pub(crate) tagger_timestamp: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ForEachRefRequirements {
    need_object_kind: bool,
    need_subject: bool,
    need_tagger: bool,
}

#[derive(Debug, Clone, Copy)]
enum SimpleForEachRefFormatPart<'a> {
    Literal(&'a str),
    RefName,
    RefNameShort,
    ObjectName,
    ObjectNameShort,
}

fn collect_for_each_ref_rows(
    refs: &dyn GitRefsStore,
    objects: &dyn GitObjectStore,
    patterns: &[String],
    requirements: &ForEachRefRequirements,
) -> Result<Vec<ForEachRefRow>> {
    let mut rows = Vec::new();
    for (ref_name, object_id) in refs
        .list_refs(Some("refs/"))
        .map_err(|error| map_primitive_error(error, "list refs"))?
    {
        if patterns.is_empty()
            || patterns
                .iter()
                .any(|pattern| ref_pattern_matches(&ref_name, pattern))
        {
            rows.push(build_for_each_ref_row(
                &ref_name,
                &object_id,
                objects,
                requirements,
            )?);
        }
    }
    Ok(rows)
}

fn print_for_each_ref_rows(
    refs: &dyn GitRefsStore,
    objects: &dyn GitObjectStore,
    patterns: &[String],
    format: &str,
    requirements: &ForEachRefRequirements,
) -> Result<()> {
    let mut stdout = io::stdout().lock();
    let simple_format = simple_for_each_ref_format_parts(format);
    let mut outcome = Ok(());
    refs.visit_refs(Some("refs/"), &mut |ref_name, object_id| {
        if outcome.is_err() {
            return Ok(());
        }
        if !patterns.is_empty()
            && !patterns
                .iter()
                .any(|pattern| ref_pattern_matches(&ref_name, pattern))
        {
            return Ok(());
        }
        if let Some(parts) = simple_format.as_deref() {
            if let Err(error) =
                write_simple_for_each_ref_row(&mut stdout, parts, ref_name, object_id)
            {
                outcome = Err(error);
            }
            return Ok(());
        }
        match build_for_each_ref_row(ref_name, object_id, objects, requirements)
            .and_then(|row| render_for_each_ref_row(format, &row))
        {
            Ok(rendered) => {
                if let Err(error) = writeln!(stdout, "{rendered}") {
                    outcome = Err(CliError::Io(error));
                }
            }
            Err(error) => outcome = Err(error),
        }
        Ok(())
    })
    .map_err(|error| map_primitive_error(error, "list refs"))?;
    outcome
}

fn print_simple_for_each_ref_rows(
    refs: &dyn GitRefsStore,
    patterns: &[String],
    parts: &[SimpleForEachRefFormatPart<'_>],
) -> Result<()> {
    let mut stdout = io::stdout().lock();
    let mut outcome = Ok(());
    refs.visit_refs(Some("refs/"), &mut |ref_name, object_id| {
        if outcome.is_err() {
            return Ok(());
        }
        if !patterns.is_empty()
            && !patterns
                .iter()
                .any(|pattern| ref_pattern_matches(&ref_name, pattern))
        {
            return Ok(());
        }
        if let Err(error) = write_simple_for_each_ref_row(&mut stdout, parts, ref_name, object_id) {
            outcome = Err(error);
        }
        Ok(())
    })
    .map_err(|error| map_primitive_error(error, "list refs"))?;
    outcome
}

fn simple_for_each_ref_format_parts(format: &str) -> Option<Vec<SimpleForEachRefFormatPart<'_>>> {
    let mut parts = Vec::new();
    let mut rest = format;
    while let Some(start) = rest.find("%(") {
        if start > 0 {
            parts.push(SimpleForEachRefFormatPart::Literal(&rest[..start]));
        }
        let after_start = &rest[start + 2..];
        let end = after_start.find(')')?;
        let atom = match &after_start[..end] {
            "refname" => SimpleForEachRefFormatPart::RefName,
            "refname:short" => SimpleForEachRefFormatPart::RefNameShort,
            "objectname" => SimpleForEachRefFormatPart::ObjectName,
            "objectname:short" => SimpleForEachRefFormatPart::ObjectNameShort,
            _ => return None,
        };
        parts.push(atom);
        rest = &after_start[end + 1..];
    }
    if !rest.is_empty() {
        parts.push(SimpleForEachRefFormatPart::Literal(rest));
    }
    Some(parts)
}

fn write_simple_for_each_ref_row<W: Write>(
    out: &mut W,
    parts: &[SimpleForEachRefFormatPart<'_>],
    ref_name: &str,
    object_id: &str,
) -> Result<()> {
    for part in parts {
        match part {
            SimpleForEachRefFormatPart::Literal(literal) => out.write_all(literal.as_bytes())?,
            SimpleForEachRefFormatPart::RefName => out.write_all(ref_name.as_bytes())?,
            SimpleForEachRefFormatPart::RefNameShort => {
                out.write_all(short_ref_name_str(ref_name).as_bytes())?
            }
            SimpleForEachRefFormatPart::ObjectName => out.write_all(object_id.as_bytes())?,
            SimpleForEachRefFormatPart::ObjectNameShort => {
                out.write_all(&object_id.as_bytes()[..object_id.len().min(7)])?
            }
        }
    }
    out.write_all(b"\n")?;
    Ok(())
}

fn build_for_each_ref_row(
    ref_name: &str,
    object_id: &str,
    objects: &dyn GitObjectStore,
    requirements: &ForEachRefRequirements,
) -> Result<ForEachRefRow> {
    let object_id = parse_primitive_object_id(object_id)?;
    let object_id_hex = object_id.to_hex();
    let (object_kind, metadata) =
        load_for_each_ref_metadata(ref_name, &object_id_hex, objects, requirements)?;
    Ok(ForEachRefRow {
        ref_name: ref_name.to_owned(),
        object_id,
        object_kind,
        subject: metadata.subject,
        tagger_name: metadata.tagger_name,
        tagger_email: metadata.tagger_email,
        tagger_timestamp: metadata.tagger_timestamp,
    })
}

fn load_for_each_ref_metadata(
    ref_name: &str,
    object_id: &String,
    objects: &dyn GitObjectStore,
    requirements: &ForEachRefRequirements,
) -> Result<(GitObjectKind, RefObjectMetadata)> {
    if !requirements.need_object_kind && !requirements.need_subject && !requirements.need_tagger {
        return Ok((GitObjectKind::Commit, RefObjectMetadata::default()));
    }

    let object = objects
        .read_envelope(object_id, None)
        .map_err(|error| map_primitive_error(error, "read for-each-ref object envelope"))?;
    let kind = parse_git_object_kind(ref_name, &object.object_type)?;

    if !requirements.need_subject && !requirements.need_tagger {
        return Ok((kind, RefObjectMetadata::default()));
    }

    let content = objects
        .read_object_content(object_id)
        .map_err(|error| map_primitive_error(error, "read for-each-ref object content"))?;
    let metadata =
        object_ref_metadata_parts(object_id, kind.as_str(), &content).map_err(|error| {
            CliError::Fatal {
                code: 128,
                message: format!("for-each-ref metadata decode failed for {ref_name}: {error:?}"),
            }
        })?;
    Ok((kind, metadata))
}

fn for_each_ref_requirements(format: &str, sort: &[String]) -> Result<ForEachRefRequirements> {
    let mut requirements = ForEachRefRequirements::default();
    for atom in for_each_ref_format_atoms(format)? {
        apply_for_each_ref_atom_requirements(atom, &mut requirements)?;
    }
    for key in sort {
        let key = key.strip_prefix('-').unwrap_or(key);
        apply_for_each_ref_atom_requirements(key, &mut requirements)?;
    }
    Ok(requirements)
}

fn for_each_ref_format_atoms(format: &str) -> Result<Vec<&str>> {
    let mut atoms = Vec::new();
    let mut rest = format;
    while let Some(start) = rest.find("%(") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find(')') else {
            return Err(CliError::Fatal {
                code: 128,
                message: "unterminated for-each-ref format atom".into(),
            });
        };
        atoms.push(&after_start[..end]);
        rest = &after_start[end + 1..];
    }
    Ok(atoms)
}

fn apply_for_each_ref_atom_requirements(
    atom: &str,
    requirements: &mut ForEachRefRequirements,
) -> Result<()> {
    match atom {
        "refname" | "refname:short" | "objectname" | "objectname:short" => {}
        "objecttype" => requirements.need_object_kind = true,
        "subject" | "contents:subject" => {
            requirements.need_object_kind = true;
            requirements.need_subject = true;
        }
        "taggername" | "taggeremail" | "taggerdate" | "taggerdate:unix" => {
            requirements.need_object_kind = true;
            requirements.need_tagger = true;
        }
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown field name: {atom}"),
            });
        }
    }
    Ok(())
}

fn map_primitive_error(error: PrimitiveError, context: &str) -> CliError {
    let message = match error {
        PrimitiveError::Io(error) => return CliError::from(error),
        PrimitiveError::ExitStatus { code } => return CliError::Exit(code),
        PrimitiveError::ExitMessage { code, message } => {
            return CliError::Stderr {
                code,
                text: message,
            };
        }
        PrimitiveError::Fatal { code, message } => return CliError::Fatal { code, message },
        PrimitiveError::Config { details }
        | PrimitiveError::Storage { details }
        | PrimitiveError::Crypto { details }
        | PrimitiveError::Transport { details }
        | PrimitiveError::Authorization { details }
        | PrimitiveError::Validation { details }
        | PrimitiveError::Git { details } => details,
        PrimitiveError::UnsupportedRuntime { runtime } => runtime,
        PrimitiveError::NotImplemented(message) => message.to_owned(),
    };

    CliError::Fatal {
        code: 128,
        message: format!("{context}: {message}"),
    }
}

pub(crate) fn apply_for_each_ref_sort(rows: &mut [ForEachRefRow], sort: &[String]) -> Result<()> {
    if sort.is_empty() {
        return Ok(());
    }
    for key in sort {
        let (descending, key) = key
            .strip_prefix('-')
            .map(|key| (true, key))
            .unwrap_or((false, key.as_str()));
        let compare = |left: &ForEachRefRow, right: &ForEachRefRow| match key {
            "refname" => left.ref_name.cmp(&right.ref_name),
            "objectname" => left.object_id.to_hex().cmp(&right.object_id.to_hex()),
            "objecttype" => left.object_kind.as_str().cmp(right.object_kind.as_str()),
            "subject" => left.subject.cmp(&right.subject),
            "contents:subject" => left.subject.cmp(&right.subject),
            "taggerdate" => left.tagger_timestamp.cmp(&right.tagger_timestamp),
            _ => std::cmp::Ordering::Equal,
        };
        match key {
            "refname" | "objectname" | "objecttype" | "subject" | "contents:subject"
            | "taggerdate" => {
                if descending {
                    rows.sort_by(|left, right| compare(right, left));
                } else {
                    rows.sort_by(compare);
                }
            }
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("unknown field name: {key}"),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn render_for_each_ref_row(format: &str, row: &ForEachRefRow) -> Result<String> {
    let mut out = String::new();
    let mut rest = format;
    while let Some(start) = rest.find("%(") {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find(')') else {
            return Err(CliError::Fatal {
                code: 128,
                message: "unterminated for-each-ref format atom".into(),
            });
        };
        let atom = &after_start[..end];
        out.push_str(&for_each_ref_atom(atom, row)?);
        rest = &after_start[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn for_each_ref_atom(atom: &str, row: &ForEachRefRow) -> Result<String> {
    match atom {
        "refname" => Ok(row.ref_name.clone()),
        "refname:short" => Ok(short_ref_name(&row.ref_name)),
        "objectname" => Ok(row.object_id.to_hex()),
        "objectname:short" => Ok(short_object_id(&row.object_id)),
        "objecttype" => Ok(row.object_kind.as_str().to_owned()),
        "subject" => Ok(row.subject.clone()),
        "contents:subject" => Ok(row.subject.clone()),
        "taggername" => Ok(row.tagger_name.clone()),
        "taggeremail" => {
            if row.tagger_email.is_empty() {
                Ok(String::new())
            } else {
                Ok(format!("<{}>", row.tagger_email))
            }
        }
        "taggerdate:unix" => Ok(row
            .tagger_timestamp
            .map(|timestamp| timestamp.to_string())
            .unwrap_or_default()),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown field name: {atom}"),
        }),
    }
}

fn ref_pattern_matches(ref_name: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    ref_name == pattern || ref_name.starts_with(&format!("{pattern}/"))
}

#[derive(Default)]
pub(crate) struct RefObjectMetadata {
    pub(crate) subject: String,
    pub(crate) tagger_name: String,
    pub(crate) tagger_email: String,
    pub(crate) tagger_timestamp: Option<i64>,
}

fn parse_primitive_object_id(value: &str) -> Result<ObjectId> {
    let algorithm = match value.len() {
        40 => GitHashAlgorithm::Sha1,
        64 => GitHashAlgorithm::Sha256,
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid object id length in ref payload: {value}"),
            });
        }
    };
    ObjectId::from_hex(algorithm, value).map_err(CliError::from)
}

fn parse_git_object_kind(ref_name: &str, object_type: &str) -> Result<GitObjectKind> {
    GitObjectKind::parse(object_type.as_bytes()).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("invalid object type for {ref_name}: {object_type}"),
    })
}

fn object_ref_metadata_parts(
    object_id: &str,
    object_type: &str,
    content: &[u8],
) -> Result<RefObjectMetadata> {
    let object_kind = parse_git_object_kind(object_id, object_type)?;
    let algorithm = match object_id.len() {
        40 => GitHashAlgorithm::Sha1,
        64 => GitHashAlgorithm::Sha256,
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid object id length for metadata decoding: {object_id}"),
            });
        }
    };
    match object_kind {
        GitObjectKind::Commit => {
            let commit = decode_commit(algorithm, content)?;
            Ok(RefObjectMetadata {
                subject: commit_subject(&commit.message),
                tagger_name: String::new(),
                tagger_email: String::new(),
                tagger_timestamp: None,
            })
        }
        GitObjectKind::Tag => {
            let tag = decode_tag(algorithm, content)?;
            Ok(RefObjectMetadata {
                subject: tag_subject(&tag.message),
                tagger_name: signature_name(&tag.tagger),
                tagger_email: signature_email(&tag.tagger),
                tagger_timestamp: signature_timestamp(&tag.tagger),
            })
        }
        GitObjectKind::Tree | GitObjectKind::Blob => Ok(RefObjectMetadata {
            subject: String::new(),
            tagger_name: String::new(),
            tagger_email: String::new(),
            tagger_timestamp: None,
        }),
    }
}

pub(crate) fn object_ref_metadata(object: &LooseObject) -> Result<RefObjectMetadata> {
    object_ref_metadata_parts(&object.id.to_hex(), object.kind.as_str(), &object.content)
}

#[derive(Debug, Clone)]
pub(crate) struct ReplaceOptions {
    pub(crate) list: bool,
    pub(crate) delete: bool,
    pub(crate) force: bool,
    pub(crate) format: Option<String>,
    pub(crate) edit: bool,
    pub(crate) graft: bool,
    pub(crate) convert_graft_file: bool,
    pub(crate) raw: bool,
    pub(crate) args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplaceFormat {
    Short,
    Medium,
    Long,
}

pub(crate) fn replace(options: ReplaceOptions) -> Result<()> {
    if options.list && options.delete {
        return Err(CliError::Fatal {
            code: 129,
            message: "replace --list cannot be combined with --delete".into(),
        });
    }
    let format = parse_replace_format(options.format.as_deref())?;
    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);

    if options.convert_graft_file {
        return replace_convert_graft_file(&repo, &refs, &store, options.force);
    }
    if options.graft {
        return replace_graft(&repo, &refs, &store, &options.args, options.force);
    }
    if options.edit {
        return replace_edit(
            &repo,
            &refs,
            &store,
            &options.args,
            options.force,
            options.raw,
        );
    }
    if options.delete {
        return replace_delete(&repo, &refs, &options.args);
    }
    if options.list || options.args.is_empty() {
        if options.args.len() > 1 {
            return Err(replace_bad_arguments());
        }
        return replace_list(
            &refs,
            &store,
            format,
            options.args.first().map(String::as_str),
        );
    }
    if options.args.len() != 2 {
        return Err(replace_bad_arguments());
    }
    replace_create(
        &repo,
        &refs,
        &store,
        &options.args[0],
        &options.args[1],
        options.force,
    )
}

fn replace_create(
    repo: &GitRepo,
    refs: &RefStore,
    store: &LooseObjectStore,
    object: &str,
    replacement: &str,
    force: bool,
) -> Result<()> {
    let object_id = resolve_objectish(repo, object)?;
    let replacement_id = resolve_objectish(repo, replacement)?;
    let object = store.read_object(&object_id)?;
    let replacement = store.read_object(&replacement_id)?;
    if !force && object.kind != replacement.kind {
        return Err(CliError::Fatal {
            code: 255,
            message: format!(
                "Objects must be of the same type.\n'{}' points to a replaced object of type '{}'\nwhile '{}' points to a replacement object of type '{}'.",
                object_id.to_hex(),
                object.kind.as_str(),
                replacement_id.to_hex(),
                replacement.kind.as_str()
            ),
        });
    }

    let ref_name = replace_ref_name(&object_id);
    if !force && ref_exists(refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("replace ref '{}' already exists", object_id.to_hex()),
        });
    }
    refs.write_ref(&ref_name, &replacement_id)?;
    Ok(())
}

fn replace_graft(
    repo: &GitRepo,
    refs: &RefStore,
    store: &LooseObjectStore,
    args: &[String],
    force: bool,
) -> Result<()> {
    if args.is_empty() {
        return Err(replace_bad_arguments());
    }
    let object_id = resolve_commitish(repo, store, &args[0])?;
    let commit_cache = CommitObjectCache::new(store);
    let original = commit_cache.read_commit(&object_id)?;
    let mut builder = CommitBuilder::new(
        original.tree.clone(),
        signature_from_raw_commit_header(original.author.clone())?,
        signature_from_raw_commit_header(original.committer.clone())?,
    );
    for parent in &args[1..] {
        builder = builder.parent(resolve_commitish(repo, store, parent)?);
    }
    let replacement = builder.message(original.message.clone())?.encode()?;
    let replacement_id = store.write_object(GitObjectKind::Commit, &replacement)?;
    if replacement_id == object_id {
        return Err(CliError::Fatal {
            code: 255,
            message: format!(
                "new commit is the same as the old one: '{}'",
                object_id.to_hex()
            ),
        });
    }

    let ref_name = replace_ref_name(&object_id);
    if !force && ref_exists(refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("replace ref '{}' already exists", object_id.to_hex()),
        });
    }
    refs.write_ref(&ref_name, &replacement_id)?;
    Ok(())
}

fn replace_edit(
    repo: &GitRepo,
    refs: &RefStore,
    store: &LooseObjectStore,
    args: &[String],
    force: bool,
    raw: bool,
) -> Result<()> {
    if args.len() != 1 {
        return Err(replace_bad_arguments());
    }
    let object_id = resolve_objectish(repo, &args[0])?;
    let object = store.read_object(&object_id)?;
    let initial = replace_render_edit_buffer(store, &object, raw)?;
    let edited = edit_temp_buffer(repo, "REPLACE_EDITOBJ", &initial, false)?;
    let replacement_id = replace_store_edited_object(store, &object, edited, raw)?;
    if replacement_id == object_id {
        return Ok(());
    }

    let ref_name = replace_ref_name(&object_id);
    if !force && ref_exists(refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("replace ref '{}' already exists", object_id.to_hex()),
        });
    }
    refs.write_ref(&ref_name, &replacement_id)?;
    Ok(())
}

fn replace_render_edit_buffer(
    store: &LooseObjectStore,
    object: &LooseObject,
    raw: bool,
) -> Result<Vec<u8>> {
    if raw || object.kind != GitObjectKind::Tree {
        return Ok(object.content.clone());
    }
    render_tree_for_replace_edit(store, &object.id)
}

fn replace_store_edited_object(
    store: &LooseObjectStore,
    original: &LooseObject,
    edited: Vec<u8>,
    raw: bool,
) -> Result<ObjectId> {
    let content = match original.kind {
        GitObjectKind::Blob => edited,
        GitObjectKind::Commit => {
            let _ = decode_commit(GitHashAlgorithm::Sha1, &edited)?;
            edited
        }
        GitObjectKind::Tag => {
            let _ = decode_tag(GitHashAlgorithm::Sha1, &edited)?;
            edited
        }
        GitObjectKind::Tree if raw => edited,
        GitObjectKind::Tree => replace_parse_edited_tree(store, &edited)?,
    };
    store
        .write_object(original.kind, &content)
        .map_err(CliError::Io)
}

fn replace_parse_edited_tree(store: &LooseObjectStore, edited: &[u8]) -> Result<Vec<u8>> {
    let records = split_mktree_records(edited, false)?;
    let mut entries = records
        .into_iter()
        .filter(|record| !record.is_empty())
        .map(|record| parse_mktree_entry(store, &record, false))
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by(compare_mktree_entries);
    encode_tree(&entries).map_err(CliError::Io)
}

fn render_tree_for_replace_edit(store: &LooseObjectStore, tree_id: &ObjectId) -> Result<Vec<u8>> {
    let tree_cache = TreeObjectCache::new(store);
    let mut out = Vec::new();
    render_tree_entries_for_replace_edit(&tree_cache, tree_id, Vec::new(), &mut out)?;
    Ok(out)
}

fn render_tree_entries_for_replace_edit(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_id: &ObjectId,
    prefix: Vec<u8>,
    out: &mut Vec<u8>,
) -> Result<()> {
    for entry in tree_cache.read_tree(tree_id)?.iter() {
        let path = tree_entry_path(&prefix, &entry.name);
        out.extend_from_slice(
            format!(
                "{} {} {}\t{}\n",
                tree_mode_display(entry.mode),
                tree_entry_kind(entry.mode).as_str(),
                entry.id.to_hex(),
                String::from_utf8_lossy(&path)
            )
            .as_bytes(),
        );
    }
    Ok(())
}

fn replace_convert_graft_file(
    repo: &GitRepo,
    refs: &RefStore,
    store: &LooseObjectStore,
    force: bool,
) -> Result<()> {
    let path = repo.git_dir.join("info/grafts");
    let input = match fs::read_to_string(&path) {
        Ok(input) => input,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let args = line
            .split_whitespace()
            .map(|part| part.to_owned())
            .collect::<Vec<_>>();
        replace_graft(repo, refs, store, &args, force)?;
    }
    remove_file_if_exists(&path)?;
    Ok(())
}

fn signature_from_raw_commit_header(raw: Vec<u8>) -> Result<Signature> {
    let raw = String::from_utf8(raw).map_err(|_| CliError::Fatal {
        code: 128,
        message: "commit signature is not valid UTF-8".into(),
    })?;
    let mut parts = raw.rsplitn(3, ' ');
    let timezone = parts
        .next()
        .ok_or_else(invalid_commit_signature)?
        .to_owned();
    let timestamp = parts
        .next()
        .ok_or_else(invalid_commit_signature)?
        .parse::<i64>()
        .map_err(|_| invalid_commit_signature())?;
    let identity = parts.next().ok_or_else(invalid_commit_signature)?;
    let (name, email_with_bracket) = identity
        .rsplit_once(" <")
        .ok_or_else(invalid_commit_signature)?;
    let email = email_with_bracket
        .strip_suffix('>')
        .ok_or_else(invalid_commit_signature)?;
    Ok(Signature {
        name: name.to_owned(),
        email: email.to_owned(),
        timestamp,
        timezone,
    })
}

fn invalid_commit_signature() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "commit signature is invalid".into(),
    }
}

fn replace_delete(repo: &GitRepo, refs: &RefStore, objects: &[String]) -> Result<()> {
    if objects.is_empty() {
        return Err(replace_bad_arguments());
    }
    for object in objects {
        let id = resolve_objectish(repo, object)?;
        refs.delete_ref(&replace_ref_name(&id))?;
        println!("Deleted replace ref '{}'", id.to_hex());
    }
    Ok(())
}

fn replace_list(
    refs: &RefStore,
    store: &LooseObjectStore,
    format: ReplaceFormat,
    pattern: Option<&str>,
) -> Result<()> {
    refs.for_each_ref_name("refs/replace/", |ref_name| {
        let Some(name) = ref_name.strip_prefix("refs/replace/") else {
            return Ok(());
        };
        if pattern.is_some_and(|pattern| !wildcard_match(pattern, name)) {
            return Ok(());
        }
        let RefTarget::Direct(replacement_id) = refs.read_ref(ref_name)? else {
            return Ok(());
        };
        match format {
            ReplaceFormat::Short => println!("{name}"),
            ReplaceFormat::Medium => println!("{name} -> {}", replacement_id.to_hex()),
            ReplaceFormat::Long => {
                let object_id = ObjectId::from_hex(GitHashAlgorithm::Sha1, name)?;
                let object = store.read_object(&object_id)?;
                let replacement = store.read_object(&replacement_id)?;
                println!(
                    "{} ({}) -> {} ({})",
                    object_id.to_hex(),
                    object.kind.as_str(),
                    replacement_id.to_hex(),
                    replacement.kind.as_str()
                );
            }
        }
        Ok::<(), CliError>(())
    })?;
    Ok(())
}

fn parse_replace_format(format: Option<&str>) -> Result<ReplaceFormat> {
    match format.unwrap_or("short") {
        "short" => Ok(ReplaceFormat::Short),
        "medium" => Ok(ReplaceFormat::Medium),
        "long" => Ok(ReplaceFormat::Long),
        other => Err(CliError::Fatal {
            code: 255,
            message: format!(
                "invalid replace format '{other}'\nvalid formats are 'short', 'medium' and 'long'"
            ),
        }),
    }
}

fn replace_ref_name(id: &ObjectId) -> String {
    format!("refs/replace/{}", id.to_hex())
}

fn replace_bad_arguments() -> CliError {
    CliError::Fatal {
        code: 129,
        message: "bad number of arguments".into(),
    }
}

pub(crate) fn commit_patch_id_for_cherry_cached(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
) -> Result<Option<String>> {
    let commit = commit_cache.read_commit(id)?;
    if commit.parents.len() > 1 {
        return Ok(None);
    }
    let old_index = if let Some(parent) = commit.parents.first() {
        let parent_commit = commit_cache.read_commit(parent)?;
        read_commit_tree_index_cached(tree_cache, &parent_commit)?
    } else {
        GitIndex::new()
    };
    let new_index = read_commit_tree_index_cached(tree_cache, &commit)?;
    let entries = diff_indexes(&old_index, &new_index)?;
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    let mut patchlen = 0usize;
    for entry in entries {
        let old_entry = find_index_entry(&old_index, &entry.path);
        let new_entry = find_index_entry(&new_index, &entry.path);
        patchlen += hash_patch_id_entry(store, &mut hasher, &entry, old_entry, new_entry)?;
    }
    if patchlen == 0 {
        return Ok(None);
    }
    Ok(Some(hasher.finalize().to_hex()))
}

fn hash_patch_id_entry(
    store: &LooseObjectStore,
    hasher: &mut GitObjectHash,
    entry: &skron_git_core::IndexDiffEntry,
    old_entry: Option<&IndexEntry>,
    new_entry: Option<&IndexEntry>,
) -> Result<usize> {
    let display_path = String::from_utf8_lossy(&entry.path);
    let mode = new_entry
        .or(old_entry)
        .map(|entry| index_mode_octal(entry.mode))
        .unwrap_or("100644");
    let mut patchlen = 0usize;
    patchlen += hash_patch_id_line(
        hasher,
        format!("diff --git a/{display_path} b/{display_path}").as_bytes(),
    );
    match entry.status {
        IndexDiffStatus::Added => {
            patchlen += hash_patch_id_line(hasher, format!("new file mode {mode}").as_bytes());
        }
        IndexDiffStatus::Deleted => {
            patchlen += hash_patch_id_line(hasher, format!("deleted file mode {mode}").as_bytes());
        }
        IndexDiffStatus::Modified | IndexDiffStatus::Renamed | IndexDiffStatus::Copied => {}
    }
    let old_content = old_entry
        .map(|entry| read_index_entry_content(store, entry))
        .transpose()?
        .unwrap_or_default();
    let new_content = new_entry
        .map(|entry| read_index_entry_content(store, entry))
        .transpose()?
        .unwrap_or_default();
    if old_content.is_empty() && new_content.is_empty() {
        return Ok(patchlen);
    }
    if is_binary_content(&old_content) || is_binary_content(&new_content) {
        return Ok(patchlen);
    }
    let old_label = if entry.status == IndexDiffStatus::Added {
        "/dev/null".to_owned()
    } else {
        format!("a/{display_path}")
    };
    let new_label = if entry.status == IndexDiffStatus::Deleted {
        "/dev/null".to_owned()
    } else {
        format!("b/{display_path}")
    };
    patchlen += hash_patch_id_line(hasher, format!("--- {old_label}").as_bytes());
    patchlen += hash_patch_id_line(hasher, format!("+++ {new_label}").as_bytes());
    patchlen += hash_patch_id_hunks(hasher, &old_content, &new_content);
    Ok(patchlen)
}

fn hash_patch_id_hunks(
    hasher: &mut GitObjectHash,
    old_content: &[u8],
    new_content: &[u8],
) -> usize {
    let old_lines = split_diff_lines(old_content);
    let new_lines = split_diff_lines(new_content);
    let ops = diff_line_ops(&old_lines, &new_lines);
    let mut patchlen = 0usize;
    for (start, end) in unified_hunk_ranges(&ops, 3, 0) {
        for op in &ops[start..end] {
            match op {
                DiffLineOp::Equal(line) => {
                    patchlen += hash_patch_id_prefixed_line(hasher, b' ', line);
                }
                DiffLineOp::Delete(line) => {
                    patchlen += hash_patch_id_prefixed_line(hasher, b'-', line);
                }
                DiffLineOp::Insert(line) => {
                    patchlen += hash_patch_id_prefixed_line(hasher, b'+', line);
                }
            }
        }
    }
    patchlen
}

fn hash_patch_id_prefixed_line(hasher: &mut GitObjectHash, prefix: u8, line: &[u8]) -> usize {
    let mut buffer = Vec::with_capacity(line.len() + 1);
    buffer.push(prefix);
    buffer.extend_from_slice(line);
    hash_patch_id_line(hasher, &buffer)
}

fn hash_patch_id_line(hasher: &mut GitObjectHash, line: &[u8]) -> usize {
    let normalized = patch_id_normalize_line(line, PatchIdMode::Unstable);
    hasher.update(&normalized);
    normalized.len()
}

pub(crate) fn patch_id(stable: bool, unstable: bool, verbatim: bool) -> Result<()> {
    if stable && unstable {
        return Err(CliError::Fatal {
            code: 129,
            message: "options '--stable' and '--unstable' cannot be used together".into(),
        });
    }
    if verbatim && (stable || unstable) {
        return Err(CliError::Fatal {
            code: 129,
            message: "option '--verbatim' cannot be combined with '--stable' or '--unstable'"
                .into(),
        });
    }
    let mode = if stable {
        PatchIdMode::Stable
    } else if verbatim {
        PatchIdMode::Verbatim
    } else {
        PatchIdMode::Unstable
    };
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    for (patch, oid) in patch_id_generate(&input, mode) {
        println!("{} {}", encode_hex(&patch), oid);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchIdMode {
    Stable,
    Unstable,
    Verbatim,
}

pub(crate) fn remote_command(verbose: bool, command: Option<RemoteCommand>) -> Result<()> {
    let repo = find_repo()?;
    match command {
        None => list_remotes(&repo, verbose),
        Some(RemoteCommand::Add { name, url }) => remote_add(&repo, &name, &url),
        Some(RemoteCommand::GetUrl { name }) => remote_get_url(&repo, &name),
        Some(RemoteCommand::SetUrl {
            push,
            add,
            delete,
            name,
            url,
            old_url,
        }) => remote_set_url(&repo, &name, push, add, delete, &url, old_url.as_deref()),
        Some(RemoteCommand::Remove { name }) => remote_remove(&repo, &name),
        Some(RemoteCommand::Rename { old, new }) => remote_rename(&repo, &old, &new),
        Some(RemoteCommand::SetHead { name, args }) => remote_set_head(&repo, &name, args),
        Some(RemoteCommand::Show { no_query, name }) => {
            remote_show(&repo, verbose, no_query, &name)
        }
        Some(RemoteCommand::Prune { dry_run, name }) => remote_prune(&repo, &name, dry_run),
        Some(RemoteCommand::SetBranches {
            add,
            name,
            branches,
        }) => remote_set_branches(&repo, &name, add, branches),
        Some(RemoteCommand::Update { prune, remotes }) => remote_update(&repo, prune, remotes),
    }
}

fn list_remotes(repo: &GitRepo, verbose: bool) -> Result<()> {
    for name in remote_names(repo)? {
        if verbose {
            let url = remote_url(repo, &name)?;
            println!("{name}\t{url} (fetch)");
            println!("{name}\t{url} (push)");
        } else {
            println!("{name}");
        }
    }
    Ok(())
}

fn remote_add(repo: &GitRepo, name: &str, url: &str) -> Result<()> {
    validate_remote_name(name)?;
    if remote_exists(repo, name)? {
        return Err(CliError::Fatal {
            code: 3,
            message: format!("remote {name} already exists."),
        });
    }
    set_config_value(repo, &format!("remote.{name}.url"), url)?;
    set_config_value(
        repo,
        &format!("remote.{name}.fetch"),
        &format!("+refs/heads/*:refs/remotes/{name}/*"),
    )
}

fn remote_get_url(repo: &GitRepo, name: &str) -> Result<()> {
    if !remote_exists(repo, name)? {
        return Err(CliError::Stderr {
            code: 2,
            text: format!("error: No such remote '{name}'\n"),
        });
    }
    println!("{}", remote_url(repo, name)?);
    Ok(())
}

fn remote_set_url(
    repo: &GitRepo,
    name: &str,
    push: bool,
    add: bool,
    delete: bool,
    url: &str,
    old_url: Option<&str>,
) -> Result<()> {
    if !remote_exists(repo, name)? {
        return Err(CliError::Stderr {
            code: 2,
            text: format!("error: No such remote '{name}'\n"),
        });
    }
    let key = if push { "pushurl" } else { "url" };
    let config_name = format!("remote.{name}.{key}");
    if add {
        return add_config_value(repo, &config_name, url);
    }
    if delete {
        return delete_remote_url_value(repo, name, key, url, push);
    }
    set_remote_url_value(repo, name, key, url, old_url)
}

fn add_config_value(repo: &GitRepo, name: &str, value: &str) -> Result<()> {
    let path = local_config_path(repo)?;
    let new_entry = parse_config_entry(name, value)?;
    let mut entries = read_config_file(&path)?;
    let insert_at = entries
        .iter()
        .rposition(|entry| {
            entry.section == new_entry.section && entry.subsection == new_entry.subsection
        })
        .map(|idx| idx + 1)
        .unwrap_or(entries.len());
    entries.insert(insert_at, new_entry);
    write_config_entries(&path, &entries)?;
    Ok(())
}

fn set_remote_url_value(
    repo: &GitRepo,
    remote: &str,
    key: &str,
    new_url: &str,
    old_url: Option<&str>,
) -> Result<()> {
    let path = local_config_path(repo)?;
    let mut entries = read_config_file(&path)?;
    let indices = config_value_indices(&entries, "remote", remote, key);
    if key == "pushurl" && old_url.is_none() && indices.is_empty() {
        let new_entry = parse_config_entry(&format!("remote.{remote}.{key}"), new_url)?;
        let insert_at = entries
            .iter()
            .rposition(|entry| entry.section == "remote" && entry.subsection == remote)
            .map(|idx| idx + 1)
            .unwrap_or(entries.len());
        entries.insert(insert_at, new_entry);
        write_config_entries(&path, &entries)?;
        return Ok(());
    }
    if old_url.is_none() && indices.len() > 1 {
        return Err(CliError::Stderr {
            code: 128,
            text: format!(
                "warning: remote.{remote}.{key} has multiple values\nfatal: could not set 'remote.{remote}.{key}' to '{new_url}'\n"
            ),
        });
    }
    let target_index = if let Some(old_url) = old_url {
        let matches = matching_config_value_indices(&entries, &indices, old_url)?;
        if matches.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("No such URL found: {old_url}"),
            });
        }
        if matches.len() > 1 {
            return Err(CliError::Stderr {
                code: 128,
                text: format!(
                    "warning: remote.{remote}.{key} has multiple values\nfatal: could not set 'remote.{remote}.{key}' to '{new_url}'\n"
                ),
            });
        }
        matches[0]
    } else {
        *indices.first().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("No such URL found: {new_url}"),
        })?
    };
    entries[target_index].value = new_url.to_owned();
    write_config_entries(&path, &entries)?;
    Ok(())
}

fn delete_remote_url_value(
    repo: &GitRepo,
    remote: &str,
    key: &str,
    pattern: &str,
    push: bool,
) -> Result<()> {
    let path = local_config_path(repo)?;
    let mut entries = read_config_file(&path)?;
    let indices = config_value_indices(&entries, "remote", remote, key);
    let matches = matching_config_value_indices(&entries, &indices, pattern)?;
    if matches.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("could not unset 'remote.{remote}.{key}'"),
        });
    }
    if !push && matches.len() == indices.len() {
        return Err(CliError::Fatal {
            code: 128,
            message: "Will not delete all non-push URLs".into(),
        });
    }
    let matches = matches.into_iter().collect::<HashSet<_>>();
    let mut index = 0_usize;
    entries.retain(|_| {
        let keep = !matches.contains(&index);
        index += 1;
        keep
    });
    write_config_entries(&path, &entries)?;
    Ok(())
}

fn config_value_indices(
    entries: &[ConfigEntry],
    section: &str,
    subsection: &str,
    key: &str,
) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (entry.section == section && entry.subsection == subsection && entry.key == key)
                .then_some(index)
        })
        .collect()
}

fn matching_config_value_indices(
    entries: &[ConfigEntry],
    indices: &[usize],
    pattern: &str,
) -> Result<Vec<usize>> {
    let regex = regex::Regex::new(pattern).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid regex: {pattern}"),
    })?;
    Ok(indices
        .iter()
        .copied()
        .filter(|index| regex.is_match(&entries[*index].value))
        .collect())
}

fn remote_remove(repo: &GitRepo, name: &str) -> Result<()> {
    ensure_remote_exists(repo, name)?;
    let mut entries = read_common_config_entries(repo)?;
    remove_remote_config_entries(&mut entries, name);
    write_common_config_entries(repo, &entries)?;
    Ok(())
}

fn remote_rename(repo: &GitRepo, old: &str, new: &str) -> Result<()> {
    ensure_remote_exists(repo, old)?;
    validate_remote_name(new)?;
    if remote_exists(repo, new)? {
        return Err(CliError::Stderr {
            code: 3,
            text: format!("error: remote {new} already exists.\n"),
        });
    }
    let mut entries = read_common_config_entries(repo)?;
    for entry in &mut entries {
        if entry.section == "remote" && entry.subsection == old {
            new.clone_into(&mut entry.subsection);
            if entry.key == "fetch" {
                entry.value = entry.value.replace(
                    &format!("refs/remotes/{old}/"),
                    &format!("refs/remotes/{new}/"),
                );
            }
        }
        if entry.section == "branch" && entry.key == "remote" && entry.value == old {
            new.clone_into(&mut entry.value);
        }
    }
    write_common_config_entries(repo, &entries)?;
    Ok(())
}

fn remote_set_head(repo: &GitRepo, name: &str, args: Vec<String>) -> Result<()> {
    if args.len() == 1 && matches!(args[0].as_str(), "-d" | "--delete") {
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let _ = refs.delete_ref(&format!("refs/remotes/{name}/HEAD"));
        return Ok(());
    }
    if args.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "remote set-head requires -a, --auto, -d, --delete, or a branch".into(),
        });
    }
    if !remote_exists(repo, name)? {
        return Err(remote_repository_unavailable_error(name));
    }
    let auto = matches!(args[0].as_str(), "-a" | "--auto");
    let branch = if auto {
        let url = remote_url(repo, name)?;
        let Some(source_path) = local_repository_path_from_location(&url)? else {
            return Err(remote_repository_unavailable_error(name));
        };
        let source = local_clone_source(&source_path)
            .map_err(|_| remote_repository_unavailable_error(name))?;
        let source_refs = RefStore::new(&source.git_dir, GitHashAlgorithm::Sha1);
        source_head_branch(&source_refs)?.ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("Cannot determine remote HEAD for {name}"),
        })?
    } else {
        let branch = args[0].trim_start_matches("refs/heads/").to_owned();
        if branch.starts_with('-') {
            return Err(CliError::Fatal {
                code: 129,
                message: format!("unknown remote set-head option '{branch}'"),
            });
        }
        let _ = branch_ref_name(&branch)?;
        branch
    };
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target = format!("refs/remotes/{name}/{branch}");
    if !ref_exists(&refs, &target)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("Not a valid ref: {target}"),
        });
    }
    refs.write_symbolic_ref(&format!("refs/remotes/{name}/HEAD"), &target)?;
    if auto {
        println!("{name}/HEAD set to {branch}");
    }
    Ok(())
}

fn remote_show(repo: &GitRepo, _verbose: bool, _no_query: bool, name: &str) -> Result<()> {
    let url = if remote_exists(repo, name)? {
        remote_url(repo, name)?
    } else {
        name.to_owned()
    };
    println!("* remote {name}");
    println!("  Fetch URL: {url}");
    println!("  Push  URL: {url}");
    println!("  HEAD branch: (not queried)");

    let branches = local_remote_branch_names(repo, name)?;
    if !branches.is_empty() {
        println!("  Remote branches: (status not queried)");
        for branch in branches {
            println!("    {branch}");
        }
    }

    let pull_branches = local_pull_branches_for_remote(repo, name)?;
    if !pull_branches.is_empty() {
        let label = if pull_branches.len() == 1 {
            "Local branch configured for 'git pull':"
        } else {
            "Local branches configured for 'git pull':"
        };
        println!("  {label}");
        for (branch, merge) in pull_branches {
            println!("    {branch} merges with remote {merge}");
        }
    }

    println!("  Local ref configured for 'git push' (status not queried):");
    println!("    (matching) pushes to (matching)");
    Ok(())
}

fn remote_prune(repo: &GitRepo, name: &str, dry_run: bool) -> Result<()> {
    if !remote_exists(repo, name)? {
        return Err(remote_repository_unavailable_error(name));
    }
    let url = remote_url(repo, name)?;
    let stale = stale_remote_branch_names(repo, name)?;
    if stale.is_empty() {
        return Ok(());
    }
    println!("Pruning {name}");
    println!("URL: {url}");
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    for branch in stale {
        if dry_run {
            println!(" * [would prune] {name}/{branch}");
        } else {
            refs.delete_ref(&format!("refs/remotes/{name}/{branch}"))?;
            println!(" * [pruned] {name}/{branch}");
        }
    }
    Ok(())
}

fn remote_set_branches(repo: &GitRepo, name: &str, add: bool, branches: Vec<String>) -> Result<()> {
    if !remote_exists(repo, name)? {
        return Err(CliError::Stderr {
            code: 2,
            text: format!("error: No such remote '{name}'\n"),
        });
    }
    let mut new_entries = Vec::with_capacity(branches.len());
    for branch in branches {
        let refspec = remote_branch_refspec(name, &branch)?;
        new_entries.push(parse_config_entry(
            &format!("remote.{name}.fetch"),
            &refspec,
        )?);
    }

    let mut entries = read_common_config_entries(repo)?;
    if !add {
        entries.retain(|entry| {
            !(entry.section == "remote" && entry.subsection == name && entry.key == "fetch")
        });
    }
    if !new_entries.is_empty() {
        let insert_at = entries
            .iter()
            .rposition(|entry| entry.section == "remote" && entry.subsection == name)
            .map(|idx| idx + 1)
            .unwrap_or(entries.len());
        entries.splice(insert_at..insert_at, new_entries);
    }
    write_common_config_entries(repo, &entries)?;
    Ok(())
}

fn remote_branch_refspec(remote: &str, branch: &str) -> Result<String> {
    if branch == "*" {
        return Ok(format!("+refs/heads/*:refs/remotes/{remote}/*"));
    }
    let ref_name = branch_ref_name(branch)?;
    let Some(branch) = ref_name.strip_prefix("refs/heads/") else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("expected branch ref from branch_ref_name, got '{ref_name}'"),
        });
    };
    Ok(format!(
        "+refs/heads/{branch}:refs/remotes/{remote}/{branch}"
    ))
}

fn remote_update(repo: &GitRepo, prune: bool, remotes: Vec<String>) -> Result<()> {
    let (remotes, show_fetching) = if remotes.is_empty() {
        let default_group = remote_group_members(repo, "default")?;
        if default_group.is_empty() {
            let names = default_remote_update_names(repo)?;
            let show_fetching = names.len() > 1;
            (
                names
                    .into_iter()
                    .map(|name| RemoteUpdateTarget {
                        name,
                        from_group: false,
                    })
                    .collect::<Vec<_>>(),
                show_fetching,
            )
        } else {
            (
                default_group
                    .into_iter()
                    .map(|name| RemoteUpdateTarget {
                        name,
                        from_group: true,
                    })
                    .collect::<Vec<_>>(),
                true,
            )
        }
    } else {
        let mut resolved = Vec::new();
        for remote_or_group in remotes {
            let group = remote_group_members(repo, &remote_or_group)?;
            if !group.is_empty() {
                resolved.extend(group.into_iter().map(|name| RemoteUpdateTarget {
                    name,
                    from_group: true,
                }));
            } else if remote_exists(repo, &remote_or_group)? {
                resolved.push(RemoteUpdateTarget {
                    name: remote_or_group,
                    from_group: false,
                });
            } else {
                return Err(CliError::Stderr {
                    code: 1,
                    text: format!("fatal: no such remote or remote group: {remote_or_group}\n"),
                });
            }
        }
        (resolved, true)
    };
    let mut failed = false;
    for target in remotes {
        if show_fetching {
            println!("Fetching {}", target.name);
        }
        if target.from_group && !remote_exists(repo, &target.name)? {
            eprintln!(
                "fatal: '{}' does not appear to be a git repository",
                target.name
            );
            eprintln!("fatal: Could not read from remote repository.");
            eprintln!();
            eprintln!("Please make sure you have the correct access rights");
            eprintln!("and the repository exists.");
            eprintln!("error: could not fetch {}", target.name);
            failed = true;
            continue;
        }
        transport_commands::fetch_with_repo_and_remote(
            repo.clone(),
            target.name.clone(),
            None,
            128,
        )?;
        if prune {
            prune_remote_tracking_refs_silent(repo, &target.name)?;
        }
    }
    if failed {
        return Err(CliError::Exit(1));
    }
    Ok(())
}

struct RemoteUpdateTarget {
    name: String,
    from_group: bool,
}

fn default_remote_update_names(repo: &GitRepo) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for name in remote_names(repo)? {
        let skip = read_config_section_value(repo, "remote", &name, "skipdefaultupdate")?
            .and_then(|value| parse_git_bool(&value))
            .unwrap_or(false);
        if !skip {
            names.push(name);
        }
    }
    Ok(names)
}

fn remote_group_members(repo: &GitRepo, group: &str) -> io::Result<Vec<String>> {
    Ok(read_config_entries(repo)?
        .into_iter()
        .filter(|entry| {
            entry.section == "remotes" && entry.subsection.is_empty() && entry.key == group
        })
        .flat_map(|entry| {
            entry
                .value
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect())
}

fn prune_remote_tracking_refs_silent(repo: &GitRepo, name: &str) -> Result<()> {
    let stale = stale_remote_branch_names(repo, name)?;
    if stale.is_empty() {
        return Ok(());
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    for branch in stale {
        refs.delete_ref(&format!("refs/remotes/{name}/{branch}"))?;
    }
    Ok(())
}

fn stale_remote_branch_names(repo: &GitRepo, name: &str) -> Result<Vec<String>> {
    let url = remote_url(repo, name)?;
    let Some(source_path) = local_repository_path_from_location(&url)? else {
        return Ok(Vec::new());
    };
    let source =
        local_clone_source(&source_path).map_err(|_| remote_repository_unavailable_error(name))?;
    let source_refs = RefStore::new(&source.git_dir, GitHashAlgorithm::Sha1);
    let mut source_branches = BTreeSet::new();
    source_refs.for_each_ref_name("refs/heads/", |ref_name| {
        if let Some(branch) = ref_name.strip_prefix("refs/heads/") {
            source_branches.insert(branch.to_owned());
        }
        Ok::<(), CliError>(())
    })?;
    Ok(local_remote_branch_names(repo, name)?
        .into_iter()
        .filter(|branch| !source_branches.contains(branch))
        .collect())
}

fn local_remote_branch_names(repo: &GitRepo, name: &str) -> Result<Vec<String>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let prefix = format!("refs/remotes/{name}/");
    let mut branches = Vec::new();
    refs.for_each_ref_name(&prefix, |ref_name| {
        let Some(branch) = ref_name.strip_prefix(&prefix) else {
            return Ok(());
        };
        if branch == "HEAD" {
            return Ok(());
        }
        branches.push(branch.to_owned());
        Ok::<(), CliError>(())
    })?;
    branches.sort();
    Ok(branches)
}

fn local_pull_branches_for_remote(repo: &GitRepo, name: &str) -> Result<Vec<(String, String)>> {
    let mut branches = BTreeMap::<String, (bool, Option<String>)>::new();
    for entry in read_config_entries(repo)? {
        if entry.section != "branch" || entry.subsection.is_empty() {
            continue;
        }
        let state = branches.entry(entry.subsection).or_default();
        if entry.key == "remote" && entry.value == name {
            state.0 = true;
        }
        if entry.key == "merge" {
            state.1 = entry
                .value
                .strip_prefix("refs/heads/")
                .map(|branch| branch.to_owned());
        }
    }
    Ok(branches
        .into_iter()
        .filter_map(|(branch, (matches_remote, merge))| {
            matches_remote
                .then(|| merge.map(|merge| (branch, merge)))
                .flatten()
        })
        .collect())
}

pub(crate) fn remote_repository_unavailable_error(remote: &str) -> CliError {
    CliError::Stderr {
        code: 128,
        text: format!(
            "fatal: '{remote}' does not appear to be a git repository\n\
             fatal: Could not read from remote repository.\n\n\
             Please make sure you have the correct access rights\n\
             and the repository exists.\n"
        ),
    }
}

#[derive(Debug, Clone)]
struct BranchOptions {
    remotes: bool,
    all: bool,
    list: bool,
    show_current: bool,
    delete: bool,
    force_delete: bool,
    move_branch: bool,
    force_move: bool,
    copy_branch: bool,
    force_copy: bool,
    set_upstream_to: Option<String>,
    unset_upstream: bool,
    contains: Option<String>,
    merged: Option<String>,
    no_merged: Option<String>,
    name: Option<String>,
    start_point: Option<String>,
}

#[derive(Debug, Clone)]
struct TagOptions {
    delete: bool,
    verify: bool,
    list: bool,
    force: bool,
    annotate: bool,
    messages: Vec<String>,
    contains: Option<String>,
    no_contains: Option<String>,
    merged: Option<String>,
    no_merged: Option<String>,
    sort: Vec<String>,
    format: Option<String>,
    args: Vec<String>,
}
fn ls_tree(recursive: bool, name_only: bool, treeish: &str, paths: Vec<String>) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let tree_id = resolve_treeish_or_invalid_object(&repo, &store, treeish)?;
    let tree_cache = TreeObjectCache::new(&store);
    if paths.is_empty() {
        print_tree_entries(&tree_cache, &tree_id, Vec::new(), recursive, name_only)?;
        return Ok(());
    }

    for path in paths {
        let path = normalize_git_path(&path)?;
        if path.is_empty() {
            print_tree_entries(&tree_cache, &tree_id, Vec::new(), recursive, name_only)?;
            continue;
        }
        let Some(entry) = find_tree_entry(&store, &tree_id, path.as_bytes())? else {
            continue;
        };
        if recursive && entry.mode == TreeMode::Tree {
            print_tree_entries(&tree_cache, &entry.id, path.into_bytes(), true, name_only)?;
        } else {
            print_tree_entry(&entry, path.as_bytes(), name_only)?;
        }
    }
    Ok(())
}

fn branch(options: BranchOptions) -> Result<()> {
    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let has_branch_filter =
        options.contains.is_some() || options.merged.is_some() || options.no_merged.is_some();
    if options.show_current {
        if options.remotes
            || options.all
            || options.delete
            || options.force_delete
            || options.move_branch
            || options.force_move
            || options.copy_branch
            || options.force_copy
            || options.set_upstream_to.is_some()
            || options.unset_upstream
            || has_branch_filter
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "--show-current cannot be combined with other branch modes".into(),
            });
        }
        if let Some(current) = current_branch_ref(&refs)? {
            println!("{}", branch_display_name(&current));
        }
        return Ok(());
    }
    if options.set_upstream_to.is_some() || options.unset_upstream {
        if options.set_upstream_to.is_some() && options.unset_upstream {
            return Err(CliError::Fatal {
                code: 129,
                message: "--set-upstream-to cannot be combined with --unset-upstream".into(),
            });
        }
        if options.delete
            || options.force_delete
            || options.move_branch
            || options.force_move
            || options.copy_branch
            || options.force_copy
            || options.remotes
            || options.all
            || options.start_point.is_some()
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "upstream configuration cannot be combined with other branch modes".into(),
            });
        }
        if let Some(upstream) = options.set_upstream_to {
            branch_set_upstream(&repo, &refs, &upstream, options.name.as_deref())?;
        } else {
            branch_unset_upstream(&repo, &refs, options.name.as_deref())?;
        }
        return Ok(());
    }
    if options.move_branch || options.force_move || options.copy_branch || options.force_copy {
        if (options.move_branch || options.force_move)
            && (options.copy_branch || options.force_copy)
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "-m/-M cannot be combined with -c/-C".into(),
            });
        }
        if options.delete || options.force_delete || options.remotes || options.all {
            return Err(CliError::Fatal {
                code: 129,
                message: "-m/-M/-c/-C cannot be combined with other branch modes".into(),
            });
        }
        if options.copy_branch || options.force_copy {
            branch_copy(
                &repo,
                &refs,
                options.name,
                options.start_point,
                options.force_copy,
            )?;
        } else {
            branch_rename(
                &repo,
                &refs,
                options.name,
                options.start_point,
                options.force_move,
            )?;
        }
        return Ok(());
    }
    if options.delete || options.force_delete {
        let Some(name) = options.name else {
            return Err(CliError::Fatal {
                code: 128,
                message: "branch name required".into(),
            });
        };
        let mut names = vec![name];
        if let Some(start_point) = options.start_point {
            names.push(start_point);
        }
        return branch_delete(&repo, &refs, names, options.force_delete);
    }

    if options.list {
        let mut patterns = Vec::new();
        if let Some(name) = &options.name {
            patterns.push(name.clone());
        }
        if let Some(start_point) = &options.start_point {
            patterns.push(start_point.clone());
        }
        return branch_list(&repo, &refs, &options, &patterns);
    }

    if let Some(name) = options.name {
        if options.remotes || options.all {
            return Err(CliError::Fatal {
                code: 129,
                message: "-r/-a cannot be combined with branch creation".into(),
            });
        }
        let ref_name = branch_ref_name(&name)?;
        if ref_exists(&refs, &ref_name)? {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("a branch named '{name}' already exists"),
            });
        }
        let start = options.start_point.unwrap_or_else(|| "HEAD".to_owned());
        let id = resolve_commitish(
            &repo,
            &LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1),
            &start,
        )?;
        refs.write_ref(&ref_name, &id)?;
        return Ok(());
    }

    branch_list(&repo, &refs, &options, &[])
}

fn branch_list(
    repo: &GitRepo,
    refs: &RefStore,
    options: &BranchOptions,
    patterns: &[String],
) -> Result<()> {
    let current = current_branch_ref(refs)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let branch_filter = branch_list_filter(repo, &store, options)?;
    if !options.remotes || options.all {
        refs.for_each_resolved_ref("refs/heads/", |ref_name, id| {
            if !branch_filter_matches(&commit_cache, id, branch_filter.as_ref())? {
                return Ok(());
            }
            if !branch_pattern_matches(ref_name, patterns, false) {
                return Ok(());
            }
            let marker = if current.as_deref() == Some(ref_name) {
                "*"
            } else {
                " "
            };
            println!("{marker} {}", branch_display_name(ref_name));
            Ok::<(), CliError>(())
        })?;
    }
    if options.remotes || options.all {
        refs.for_each_resolved_ref("refs/remotes/", |ref_name, id| {
            if !branch_filter_matches(&commit_cache, id, branch_filter.as_ref())? {
                return Ok(());
            }
            if !branch_pattern_matches(ref_name, patterns, true) {
                return Ok(());
            }
            let display = remote_branch_display(refs, ref_name, options.all)?;
            println!("  {display}");
            Ok::<(), CliError>(())
        })?;
    }
    Ok(())
}

fn branch_pattern_matches(ref_name: &str, patterns: &[String], remote: bool) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let display = if remote {
        ref_name
            .strip_prefix("refs/remotes/")
            .unwrap_or(ref_name)
            .to_owned()
    } else {
        branch_display_name(ref_name)
    };
    patterns
        .iter()
        .any(|pattern| wildcard_match(pattern, &display))
}

#[derive(Debug, Clone)]
struct BranchListFilter {
    contains: Option<ObjectId>,
    merged: Option<ObjectId>,
    no_merged: Option<ObjectId>,
}

fn branch_list_filter(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &BranchOptions,
) -> Result<Option<BranchListFilter>> {
    let contains = options
        .contains
        .as_deref()
        .map(|target| {
            resolve_commitish(repo, store, target).map_err(|_| CliError::Stderr {
                code: 129,
                text: format!("error: malformed object name {target}\n"),
            })
        })
        .transpose()?;
    let merged = options
        .merged
        .as_deref()
        .map(|target| {
            resolve_commitish(repo, store, target).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("malformed object name {target}"),
            })
        })
        .transpose()?;
    let no_merged = options
        .no_merged
        .as_deref()
        .map(|target| {
            resolve_commitish(repo, store, target).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("malformed object name {target}"),
            })
        })
        .transpose()?;
    if contains.is_none() && merged.is_none() && no_merged.is_none() {
        return Ok(None);
    }
    Ok(Some(BranchListFilter {
        contains,
        merged,
        no_merged,
    }))
}

fn branch_filter_matches(
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    branch_id: &ObjectId,
    filter: Option<&BranchListFilter>,
) -> Result<bool> {
    let Some(filter) = filter else {
        return Ok(true);
    };
    if let Some(target) = &filter.contains
        && !is_ancestor_commit_cached(commit_cache, target, branch_id)?
    {
        return Ok(false);
    }
    if let Some(target) = &filter.merged
        && !is_ancestor_commit_cached(commit_cache, branch_id, target)?
    {
        return Ok(false);
    }
    if let Some(target) = &filter.no_merged
        && is_ancestor_commit_cached(commit_cache, branch_id, target)?
    {
        return Ok(false);
    }
    Ok(true)
}

fn branch_delete(
    repo: &GitRepo,
    refs: &RefStore,
    names: Vec<String>,
    force_delete: bool,
) -> Result<()> {
    let current = current_branch_ref(refs)?;
    let mut errors = String::new();
    let store = if force_delete {
        None
    } else {
        Some(LooseObjectStore::new(
            repo.objects_dir.clone(),
            GitHashAlgorithm::Sha1,
        ))
    };
    let head_id = if force_delete {
        None
    } else {
        Some(refs.resolve("HEAD").map_err(|_| CliError::Fatal {
            code: 128,
            message: "cannot delete branch safely without a valid HEAD".into(),
        })?)
    };
    let commit_cache = store.as_ref().map(CommitObjectCache::new);

    for name in names {
        let ref_name = branch_ref_name(&name)?;
        if current.as_deref() == Some(ref_name.as_str()) {
            errors.push_str(&format!(
                "error: cannot delete branch '{name}' used by worktree at '{}'\n",
                repo.root.display()
            ));
            continue;
        }
        let branch_id = match refs.resolve(&ref_name) {
            Ok(id) => id,
            Err(_) => {
                errors.push_str(&format!("error: branch '{name}' not found\n"));
                continue;
            }
        };
        if let (Some(commit_cache), Some(head_id)) = (commit_cache.as_ref(), head_id.as_ref())
            && !is_ancestor_commit_cached(commit_cache, &branch_id, head_id)?
        {
            errors.push_str(&format!(
                "error: The branch '{name}' is not fully merged.\n\
                 If you are sure you want to delete it, run 'git branch -D {name}'.\n"
            ));
            continue;
        }
        refs.delete_ref(&ref_name)?;
        println!("Deleted branch {name}.");
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(CliError::Stderr {
            code: 1,
            text: errors,
        })
    }
}

fn branch_rename(
    repo: &GitRepo,
    refs: &RefStore,
    name: Option<String>,
    new_name: Option<String>,
    force: bool,
) -> Result<()> {
    let (old_name, new_name) = match (name, new_name) {
        (Some(old_name), Some(new_name)) => (old_name, new_name),
        (Some(new_name), None) => {
            let Some(current) = current_branch_ref(refs)? else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "cannot rename the current branch while detached".into(),
                });
            };
            (branch_display_name(&current), new_name)
        }
        (None, _) => {
            return Err(CliError::Fatal {
                code: 129,
                message: "branch name required for -m/-M".into(),
            });
        }
    };
    let old_ref = branch_ref_name(&old_name)?;
    let new_ref = branch_ref_name(&new_name)?;
    if old_ref == new_ref {
        return Ok(());
    }
    let id = refs.resolve(&old_ref).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("no branch named '{old_name}'"),
    })?;
    if !force && ref_exists(refs, &new_ref)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("a branch named '{new_name}' already exists"),
        });
    }
    refs.write_ref(&new_ref, &id)?;
    refs.delete_ref(&old_ref)?;
    if current_branch_ref(refs)?.as_deref() == Some(old_ref.as_str()) {
        refs.write_head_symbolic(&new_ref)?;
    }
    rename_branch_config(repo, &old_name, &new_name)?;
    Ok(())
}

fn branch_copy(
    repo: &GitRepo,
    refs: &RefStore,
    name: Option<String>,
    new_name: Option<String>,
    force: bool,
) -> Result<()> {
    let (old_name, new_name) = match (name, new_name) {
        (Some(old_name), Some(new_name)) => (old_name, new_name),
        (Some(new_name), None) => {
            let Some(current) = current_branch_ref(refs)? else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "cannot copy the current branch while detached".into(),
                });
            };
            (branch_display_name(&current), new_name)
        }
        (None, _) => {
            return Err(CliError::Fatal {
                code: 129,
                message: "branch name required for -c/-C".into(),
            });
        }
    };
    let old_ref = branch_ref_name(&old_name)?;
    let new_ref = branch_ref_name(&new_name)?;
    if old_ref == new_ref {
        return Ok(());
    }
    let id = refs.resolve(&old_ref).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("no branch named '{old_name}'"),
    })?;
    if !force && ref_exists(refs, &new_ref)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("a branch named '{new_name}' already exists"),
        });
    }
    refs.write_ref(&new_ref, &id)?;
    copy_branch_config(repo, &old_name, &new_name)?;
    Ok(())
}

fn branch_set_upstream(
    repo: &GitRepo,
    refs: &RefStore,
    upstream: &str,
    branch: Option<&str>,
) -> Result<()> {
    let branch = branch_target_name(refs, branch, "set upstream")?;
    let branch_ref = branch_ref_name(&branch)?;
    if !ref_exists(refs, &branch_ref)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("branch '{branch}' does not exist"),
        });
    }
    let upstream = parse_existing_upstream_ref(refs, upstream)?;
    set_config_value(repo, &format!("branch.{branch}.remote"), &upstream.remote)?;
    set_config_value(repo, &format!("branch.{branch}.merge"), &upstream.merge)?;
    println!("branch '{branch}' set up to track '{}'.", upstream.display);
    Ok(())
}

fn branch_unset_upstream(repo: &GitRepo, refs: &RefStore, branch: Option<&str>) -> Result<()> {
    let branch = branch_target_name(refs, branch, "unset upstream")?;
    let branch_ref = branch_ref_name(&branch)?;
    if read_branch_upstream(repo, &branch)?.is_none() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("branch '{branch}' has no upstream information"),
        });
    }
    if !ref_exists(refs, &branch_ref)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("branch '{branch}' does not exist"),
        });
    }
    remove_branch_upstream_config(repo, &branch)?;
    Ok(())
}

fn branch_target_name(refs: &RefStore, branch: Option<&str>, action: &str) -> Result<String> {
    if let Some(branch) = branch {
        return Ok(branch.to_owned());
    }
    current_branch_ref(refs)?
        .map(|name| branch_display_name(&name))
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("cannot {action} for detached HEAD"),
        })
}

#[derive(Debug, Clone)]
struct ParsedUpstream {
    remote: String,
    merge: String,
    display: String,
}

fn parse_existing_upstream_ref(refs: &RefStore, upstream: &str) -> Result<ParsedUpstream> {
    if let Some(rest) = upstream.strip_prefix("refs/remotes/") {
        let (remote, branch) = split_remote_branch(rest)?;
        let ref_name = format!("refs/remotes/{remote}/{branch}");
        ensure_ref_exists(refs, &ref_name)?;
        return Ok(ParsedUpstream {
            remote: remote.to_owned(),
            merge: format!("refs/heads/{branch}"),
            display: format!("{remote}/{branch}"),
        });
    }
    if let Some((remote, branch)) = upstream.split_once('/') {
        let ref_name = format!("refs/remotes/{remote}/{branch}");
        if ref_exists(refs, &ref_name)? {
            return Ok(ParsedUpstream {
                remote: remote.to_owned(),
                merge: format!("refs/heads/{branch}"),
                display: upstream.to_owned(),
            });
        }
    }
    let branch = upstream.strip_prefix("refs/heads/").unwrap_or(upstream);
    let ref_name = format!("refs/heads/{branch}");
    ensure_ref_exists(refs, &ref_name)?;
    Ok(ParsedUpstream {
        remote: ".".to_owned(),
        merge: ref_name,
        display: branch.to_owned(),
    })
}

fn split_remote_branch(value: &str) -> Result<(&str, &str)> {
    value.split_once('/').ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("invalid upstream branch '{value}'"),
    })
}

fn ensure_ref_exists(refs: &RefStore, name: &str) -> Result<()> {
    if ref_exists(refs, name)? {
        Ok(())
    } else {
        let display = short_ref_name(name);
        Err(CliError::Stderr {
            code: 128,
            text: branch_missing_upstream_error(&display),
        })
    }
}

fn branch_missing_upstream_error(upstream: &str) -> String {
    format!(
        "fatal: the requested upstream branch '{upstream}' does not exist\n\
         hint:\n\
         hint: If you are planning on basing your work on an upstream\n\
         hint: branch that already exists at the remote, you may need to\n\
         hint: run \"git fetch\" to retrieve it.\n\
         hint:\n\
         hint: If you are planning to push out a new local branch that\n\
         hint: will track its remote counterpart, you may want to use\n\
         hint: \"git push -u\" to set the upstream config as you push.\n\
         hint: Disable this message with \"git config set advice.setUpstreamFailure false\"\n"
    )
}

fn tag(options: TagOptions) -> Result<()> {
    let repo = find_repo()?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let has_list_filter = options.contains.is_some()
        || options.no_contains.is_some()
        || options.merged.is_some()
        || options.no_merged.is_some();
    let has_list_modifier = has_list_filter || !options.sort.is_empty() || options.format.is_some();
    if options.verify {
        if options.delete
            || options.list
            || options.force
            || options.annotate
            || !options.messages.is_empty()
            || has_list_modifier
        {
            return Err(CliError::Fatal {
                code: 129,
                message: "-v cannot be combined with other tag modes".into(),
            });
        }
        return verify_tag(false, false, options.args);
    }
    if options.delete {
        if options.annotate || !options.messages.is_empty() || has_list_modifier {
            return Err(CliError::Fatal {
                code: 129,
                message: "-a/-m/list modifiers cannot be combined with -d".into(),
            });
        }
        if options.args.is_empty() {
            return Ok(());
        }
        for name in options.args {
            let ref_name = tag_ref_name(&name)?;
            if refs.resolve(&ref_name).is_err() {
                return Err(CliError::Message(format!("tag '{name}' not found.")));
            }
            refs.delete_ref(&ref_name)?;
            println!("Deleted tag '{name}'");
        }
        return Ok(());
    }

    if options.args.is_empty() && (options.annotate || !options.messages.is_empty()) {
        return Err(CliError::Stderr {
            code: 129,
            text: tag_usage(),
        });
    }

    if options.list || options.args.is_empty() || has_list_modifier {
        if options.annotate || !options.messages.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "-a/-m cannot be combined with tag listing".into(),
            });
        }
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        let commit_cache = CommitObjectCache::new(&store);
        let filter = tag_list_filter(&repo, &store, &options)?;
        let patterns = options.args;
        let mut rows = Vec::new();
        refs.for_each_resolved_ref("refs/tags/", |ref_name, object_id| {
            let display = tag_display_name(ref_name);
            if !tag_filter_matches(&store, &commit_cache, object_id, filter.as_ref())? {
                return Ok(());
            }
            if patterns.is_empty()
                || patterns
                    .iter()
                    .any(|pattern| wildcard_match(pattern, &display))
            {
                let object = store.read_object(&object_id)?;
                let metadata = reference_commands::object_ref_metadata(&object)?;
                rows.push(reference_commands::ForEachRefRow {
                    ref_name: ref_name.to_owned(),
                    object_id: object_id.clone(),
                    object_kind: object.kind,
                    subject: metadata.subject,
                    tagger_name: metadata.tagger_name,
                    tagger_email: metadata.tagger_email,
                    tagger_timestamp: metadata.tagger_timestamp,
                });
            }
            Ok::<(), CliError>(())
        })?;
        reference_commands::apply_for_each_ref_sort(&mut rows, &options.sort)?;
        if let Some(format) = options.format.as_deref() {
            for row in rows {
                println!(
                    "{}",
                    reference_commands::render_for_each_ref_row(format, &row)?
                );
            }
        } else {
            for row in rows {
                println!("{}", tag_display_name(&row.ref_name));
            }
        }
        return Ok(());
    }

    if options.args.len() > 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "tag creation accepts a tag name and optional target".into(),
        });
    }
    let name = &options.args[0];
    let ref_name = tag_ref_name(name)?;
    if !options.force && ref_exists(&refs, &ref_name)? {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("tag '{name}' already exists"),
        });
    }
    let target = options.args.get(1).map(String::as_str).unwrap_or("HEAD");
    let id = resolve_objectish(&repo, target).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("Failed to resolve '{target}' as a valid ref."),
    })?;
    let id = if options.annotate || !options.messages.is_empty() {
        if options.messages.is_empty() {
            return Err(editor_required_message_error());
        }
        let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
        let target_object = store.read_object(&id)?;
        let tagger = signature_from_identity(&repo, "GIT_COMMITTER")?;
        let message = commit_tree_message(options.messages)?;
        let tag = TagBuilder::new(id, target_object.kind, name, tagger)?
            .message(message)?
            .encode()?;
        store.write_object(GitObjectKind::Tag, &tag)?
    } else {
        id
    };
    refs.write_ref(&ref_name, &id)?;
    Ok(())
}

#[derive(Debug, Clone)]
struct TagListFilter {
    contains: Option<ObjectId>,
    no_contains: Option<ObjectId>,
    merged: Option<ObjectId>,
    no_merged: Option<ObjectId>,
}

fn tag_list_filter(
    repo: &GitRepo,
    store: &LooseObjectStore,
    options: &TagOptions,
) -> Result<Option<TagListFilter>> {
    let contains = tag_filter_target(repo, store, options.contains.as_deref(), true)?;
    let no_contains = tag_filter_target(repo, store, options.no_contains.as_deref(), true)?;
    let merged = tag_filter_target(repo, store, options.merged.as_deref(), false)?;
    let no_merged = tag_filter_target(repo, store, options.no_merged.as_deref(), false)?;
    if contains.is_none() && no_contains.is_none() && merged.is_none() && no_merged.is_none() {
        return Ok(None);
    }
    Ok(Some(TagListFilter {
        contains,
        no_contains,
        merged,
        no_merged,
    }))
}

fn tag_filter_target(
    repo: &GitRepo,
    store: &LooseObjectStore,
    target: Option<&str>,
    error_is_usage: bool,
) -> Result<Option<ObjectId>> {
    target
        .map(|target| {
            resolve_commitish(repo, store, target).map_err(|_| {
                if error_is_usage {
                    CliError::Stderr {
                        code: 129,
                        text: format!("error: malformed object name {target}\n"),
                    }
                } else {
                    CliError::Fatal {
                        code: 128,
                        message: format!("malformed object name {target}"),
                    }
                }
            })
        })
        .transpose()
}

fn tag_filter_matches(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    tag_id: &ObjectId,
    filter: Option<&TagListFilter>,
) -> Result<bool> {
    let Some(filter) = filter else {
        return Ok(true);
    };
    let Some(commit_id) = peel_to_commit(store, tag_id.clone())? else {
        return Ok(false);
    };
    if let Some(target) = &filter.contains
        && !is_ancestor_commit_cached(commit_cache, target, &commit_id)?
    {
        return Ok(false);
    }
    if let Some(target) = &filter.no_contains
        && is_ancestor_commit_cached(commit_cache, target, &commit_id)?
    {
        return Ok(false);
    }
    if let Some(target) = &filter.merged
        && !is_ancestor_commit_cached(commit_cache, &commit_id, target)?
    {
        return Ok(false);
    }
    if let Some(target) = &filter.no_merged
        && is_ancestor_commit_cached(commit_cache, &commit_id, target)?
    {
        return Ok(false);
    }
    Ok(true)
}

fn tag_usage() -> String {
    "usage: git tag [-a | -s | -u <key-id>] [-f] [-m <msg> | -F <file>] [-e]
               [(--trailer <token>[(=|:)<value>])...]
               <tagname> [<commit> | <object>]
   or: git tag -d <tagname>...
   or: git tag [-n[<num>]] -l [--contains <commit>] [--no-contains <commit>]
               [--points-at <object>] [--column[=<options>] | --no-column]
               [--create-reflog] [--sort=<key>] [--format=<format>]
               [--merged <commit>] [--no-merged <commit>] [<pattern>...]
   or: git tag -v [--format=<format>] <tagname>...

    -l, --list            list tag names
    -n[<n>]               print <n> lines of each tag message
    -d, --delete          delete tags
    -v, --verify          verify tags

Tag creation options
    -a, --[no-]annotate   annotated tag, needs a message
    -m, --message <message>
                          tag message
    -F, --[no-]file <file>
                          read message from file
    --trailer <trailer>   add custom trailer(s)
    -e, --[no-]edit       force edit of tag message
    -s, --[no-]sign       annotated and GPG-signed tag
    --[no-]cleanup <mode> how to strip spaces and #comments from message
    -u, --[no-]local-user <key-id>
                          use another key to sign the tag
    -f, --[no-]force      replace the tag if exists
    --[no-]create-reflog  create a reflog

Tag listing options
    --[no-]column[=<style>]
                          show tag list in columns
    --contains <commit>   print only tags that contain the commit
    --no-contains <commit>
                          print only tags that don't contain the commit
    --merged <commit>     print only tags that are merged
    --no-merged <commit>  print only tags that are not merged
    --[no-]omit-empty     do not output a newline after empty formatted refs
    --[no-]sort <key>     field name to sort on
    --[no-]points-at <object>
                          print only tags of the object
    --[no-]format <format>
                          format to use for the output
    --[no-]color[=<when>] respect format colors
    -i, --[no-]ignore-case
                          sorting and filtering are case insensitive

"
    .to_owned()
}

pub(crate) fn ls_tree_command(
    recursive: bool,
    name_only: bool,
    treeish: &str,
    paths: Vec<String>,
) -> Result<()> {
    ls_tree(recursive, name_only, treeish, paths)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn branch_command(
    remotes: bool,
    all: bool,
    list: bool,
    show_current: bool,
    delete: bool,
    force_delete: bool,
    move_branch: bool,
    force_move: bool,
    copy_branch: bool,
    force_copy: bool,
    set_upstream_to: Option<String>,
    unset_upstream: bool,
    contains: Option<String>,
    merged: Option<String>,
    no_merged: Option<String>,
    name: Option<String>,
    start_point: Option<String>,
) -> Result<()> {
    branch(BranchOptions {
        remotes,
        all,
        list,
        show_current,
        delete,
        force_delete,
        move_branch,
        force_move,
        copy_branch,
        force_copy,
        set_upstream_to,
        unset_upstream,
        contains,
        merged,
        no_merged,
        name,
        start_point,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tag_command(
    delete: bool,
    verify: bool,
    list: bool,
    force: bool,
    annotate: bool,
    messages: Vec<String>,
    contains: Option<String>,
    no_contains: Option<String>,
    merged: Option<String>,
    no_merged: Option<String>,
    sort: Vec<String>,
    format: Option<String>,
    args: Vec<String>,
) -> Result<()> {
    tag(TagOptions {
        delete,
        verify,
        list,
        force,
        annotate,
        messages,
        contains,
        no_contains,
        merged,
        no_merged,
        sort,
        format,
        args,
    })
}

#[derive(Debug, Clone)]
struct RevParseOptions {
    short: Option<usize>,
    abbrev_ref: Option<String>,
    verify: bool,
    show_object_format: Vec<String>,
    show_toplevel: bool,
    show_prefix: bool,
    show_cdup: bool,
    git_dir: bool,
    absolute_git_dir: bool,
    git_common_dir: bool,
    git_paths: Vec<PathBuf>,
    is_inside_git_dir: bool,
    is_inside_work_tree: bool,
    is_bare_repository: bool,
    is_shallow_repository: bool,
    revs: Vec<String>,
}

fn rev_parse(options: RevParseOptions, raw_args: &[String]) -> Result<()> {
    let discovery_modes = [
        options.show_toplevel,
        options.show_prefix,
        options.show_cdup,
        !options.show_object_format.is_empty(),
        options.git_dir,
        options.absolute_git_dir,
        options.git_common_dir,
        !options.git_paths.is_empty(),
        options.is_inside_git_dir,
        options.is_inside_work_tree,
        options.is_bare_repository,
        options.is_shallow_repository,
    ]
    .into_iter()
    .filter(|mode| *mode)
    .count();
    if discovery_modes > 0 {
        let repo = find_repo_or_bare()?;
        let bare = repo_is_bare(&repo);
        let inside_git_dir = is_inside_git_dir(&repo)?;
        let inside_work_tree = !bare && !inside_git_dir;
        print_rev_parse_ordered(
            &repo,
            &options,
            raw_args,
            bare,
            inside_git_dir,
            inside_work_tree,
        )?;
        return Ok(());
    }
    for mode in &options.show_object_format {
        print_rev_parse_object_format(mode)?;
    }
    if let Some(mode) = options.abbrev_ref {
        if mode != "loose" && mode != "strict" {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unknown mode for --abbrev-ref: {mode}"),
            });
        }
        let repo = find_repo()?;
        let revs = if options.revs.is_empty() {
            vec!["HEAD"]
        } else {
            options.revs.iter().map(String::as_str).collect()
        };
        for rev in revs {
            println!("{}", abbrev_ref_name(&repo, rev)?);
        }
        return Ok(());
    }
    if options.verify && options.revs.len() != 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "Needed a single revision".into(),
        });
    }
    if options.revs.is_empty() {
        if !options.show_object_format.is_empty() {
            return Ok(());
        }
        return if options.verify {
            Err(CliError::Fatal {
                code: 128,
                message: "Needed a single revision".into(),
            })
        } else {
            Ok(())
        };
    };
    let repo = find_repo()?;
    for rev in &options.revs {
        print_rev_parse_object(&repo, rev, options.short, options.verify)?;
    }
    Ok(())
}

fn print_rev_parse_ordered(
    repo: &GitRepo,
    options: &RevParseOptions,
    raw_args: &[String],
    bare: bool,
    inside_git_dir: bool,
    inside_work_tree: bool,
) -> Result<()> {
    let mut index = usize::from(raw_args.first().is_some_and(|arg| arg == "rev-parse"));
    while index < raw_args.len() {
        let arg = &raw_args[index];
        match arg.as_str() {
            "--git-dir" => println!("{}", git_dir_display(repo)?),
            "--absolute-git-dir" => {
                println!("{}", canonical_or_absolute(repo.git_dir.clone()).display())
            }
            "--git-common-dir" => println!("{}", git_common_dir_display(repo)?),
            "--git-path" => {
                let Some(path) = raw_args.get(index + 1) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "--git-path requires a value".into(),
                    });
                };
                println!("{}", git_path_display(repo, std::path::Path::new(path))?);
                index += 1;
            }
            "--is-inside-git-dir" => println!("{}", if inside_git_dir { "true" } else { "false" }),
            "--is-inside-work-tree" => {
                println!("{}", if inside_work_tree { "true" } else { "false" });
            }
            "--is-bare-repository" => println!("{}", if bare { "true" } else { "false" }),
            "--is-shallow-repository" => {
                println!(
                    "{}",
                    if repo.git_dir.join("shallow").is_file() {
                        "true"
                    } else {
                        "false"
                    }
                );
            }
            "--show-toplevel" => {
                if !inside_work_tree {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: "this operation must be run in a work tree".into(),
                    });
                }
                println!("{}", repo.root.display());
            }
            "--show-prefix" => {
                if bare {
                    println!();
                } else {
                    println!("{}", repo_relative_prefix(repo)?);
                }
            }
            "--show-cdup" => {
                if bare {
                    println!();
                } else {
                    println!("{}", repo_relative_cdup(repo)?);
                }
            }
            "--show-object-format" => print_rev_parse_object_format("storage")?,
            "--verify" => {}
            "--short" => {}
            "--abbrev-ref" => {}
            other if other.starts_with("--show-object-format=") => {
                print_rev_parse_object_format(
                    other
                        .split_once('=')
                        .map(|(_, mode)| mode)
                        .unwrap_or("storage"),
                )?;
            }
            other if other.starts_with("--short=") || other.starts_with("--abbrev-ref=") => {}
            other if other.starts_with('-') => {}
            rev => {
                if let Some(mode) = options.abbrev_ref.as_deref() {
                    if mode != "loose" && mode != "strict" {
                        return Err(CliError::Fatal {
                            code: 128,
                            message: format!("unknown mode for --abbrev-ref: {mode}"),
                        });
                    }
                    println!("{}", abbrev_ref_name(repo, rev)?);
                } else {
                    print_rev_parse_object(repo, rev, options.short, options.verify)?;
                }
            }
        }
        index += 1;
    }
    Ok(())
}

fn print_rev_parse_object_format(mode: &str) -> Result<()> {
    match mode {
        "storage" | "input" | "output" => {
            println!("sha1");
            Ok(())
        }
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown mode for --show-object-format: {mode}"),
        }),
    }
}
fn git_dir_display(repo: &GitRepo) -> Result<String> {
    if let Some(display) = global_git_dir_display() {
        return Ok(display);
    }
    let cwd = std::env::current_dir()?;
    match repo.git_dir.strip_prefix(&cwd) {
        Ok(relative) if relative.as_os_str().is_empty() => Ok(".".to_owned()),
        Ok(relative) if relative == std::path::Path::new(".git") => Ok(".git".to_owned()),
        Ok(relative) => Ok(relative.display().to_string()),
        Err(_) => Ok(repo.git_dir.display().to_string()),
    }
}
fn git_common_dir_display(repo: &GitRepo) -> Result<String> {
    let common_dir = read_common_git_dir(&repo.git_dir)?;
    if repo_is_bare(repo) {
        return Ok(canonical_or_absolute(common_dir).display().to_string());
    }
    relative_display_from_cwd(&common_dir)
}
fn git_path_display(repo: &GitRepo, path: &std::path::Path) -> Result<String> {
    let git_path = read_common_git_dir(&repo.git_dir)?.join(path);
    if repo_is_bare(repo) {
        return Ok(canonical_or_absolute(git_path).display().to_string());
    }
    relative_display_from_cwd(&git_path)
}
fn relative_display_from_cwd(path: &std::path::Path) -> Result<String> {
    let cwd = canonical_or_absolute(std::env::current_dir()?);
    let path = canonical_or_absolute(path.to_path_buf());
    if let Ok(relative) = path.strip_prefix(&cwd) {
        return if relative.as_os_str().is_empty() {
            Ok(".".to_owned())
        } else {
            Ok(relative.display().to_string())
        };
    }
    if let Some(relative) = relative_path_between(&cwd, &path) {
        return Ok(relative.display().to_string());
    }
    Ok(path.display().to_string())
}
fn is_inside_git_dir(repo: &GitRepo) -> Result<bool> {
    let cwd = std::env::current_dir()?;
    Ok(cwd == repo.git_dir || cwd.starts_with(&repo.git_dir))
}
fn repo_relative_prefix(repo: &GitRepo) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let relative = cwd.strip_prefix(&repo.root).map_err(|_| CliError::Fatal {
        code: 128,
        message: "current directory is outside work tree".into(),
    })?;
    if relative.as_os_str().is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "{}/",
        relative
            .components()
            .map(|component| { component.as_os_str().to_string_lossy() })
            .collect::<Vec<_>>()
            .join("/")
    ))
}
fn repo_relative_cdup(repo: &GitRepo) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let relative = cwd.strip_prefix(&repo.root).map_err(|_| CliError::Fatal {
        code: 128,
        message: "current directory is outside work tree".into(),
    })?;
    let depth = relative.components().count();
    Ok("../".repeat(depth))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rev_parse_command(
    short: Option<usize>,
    abbrev_ref: Option<String>,
    verify: bool,
    show_object_format: Vec<String>,
    show_toplevel: bool,
    show_prefix: bool,
    show_cdup: bool,
    git_dir: bool,
    absolute_git_dir: bool,
    git_common_dir: bool,
    git_paths: Vec<PathBuf>,
    is_inside_git_dir: bool,
    is_inside_work_tree: bool,
    is_bare_repository: bool,
    is_shallow_repository: bool,
    revs: Vec<String>,
    raw_args: &[String],
) -> Result<()> {
    rev_parse(
        RevParseOptions {
            short,
            abbrev_ref,
            verify,
            show_object_format,
            show_toplevel,
            show_prefix,
            show_cdup,
            git_dir,
            absolute_git_dir,
            git_common_dir,
            git_paths,
            is_inside_git_dir,
            is_inside_work_tree,
            is_bare_repository,
            is_shallow_repository,
            revs,
        },
        raw_args,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use skron_primitives::git_runtime::GitObjectEnvelope;

    #[test]
    fn for_each_ref_rows_use_loose_ref_over_stale_packed_ref() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let git_dir = dir.path().join(".git");
        let objects =
            OwnedCliObjectStoreAdapter::from_path(&git_dir.join("objects"), GitHashAlgorithm::Sha1);
        let refs = OwnedCliRefsStoreAdapter::from_path(&git_dir, GitHashAlgorithm::Sha1);
        fs::create_dir_all(&git_dir.join("objects")).expect("objects dir");
        let stale_id = objects
            .write_object_content(
                &GitObjectEnvelope {
                    id: "0".repeat(40),
                    size: 0,
                    object_type: "blob".to_owned(),
                    metadata: Default::default(),
                },
                b"stale ref target\n",
            )
            .expect("write stale object");
        let live_id = objects
            .write_object_content(
                &GitObjectEnvelope {
                    id: "0".repeat(40),
                    size: 0,
                    object_type: "blob".to_owned(),
                    metadata: Default::default(),
                },
                b"live ref target\n",
            )
            .expect("write live object");
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/main\n", stale_id),
        )
        .expect("write packed refs");
        refs.write_ref(&"refs/heads/main".to_owned(), &live_id)
            .expect("write loose ref");

        let rows =
            collect_for_each_ref_rows(&refs, &objects, &[], &ForEachRefRequirements::default())
                .expect("collect rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ref_name, "refs/heads/main");
        assert_eq!(
            rows[0].object_id,
            parse_primitive_object_id(&live_id).expect("parse live id")
        );
    }
}
