use std::env;
use std::io::{BufRead, BufWriter, Write};
use std::sync::Arc;

use super::*;

const UNPACK_OBJECTS_STDIN_BUF_CAPACITY: usize = 256 * 1024;
const HASH_OBJECT_STREAM_BUF_CAPACITY: usize = 256 * 1024;
const CAT_FILE_BATCH_OUTPUT_BUF_CAPACITY: usize = 256 * 1024;

#[derive(Debug, Clone)]
enum BatchFormat {
    Default,
    Custom(String),
}

enum BatchLookup {
    Object(ResolvedObjectish),
    Special { kind: &'static str, payload: String },
}

#[derive(Debug, Clone, Copy)]
enum CatFileFilter {
    BlobNone,
    BlobLimit(usize),
    ObjectType(GitObjectKind),
}

impl BatchFormat {
    fn from_cli(format: String) -> Result<Self> {
        if format.is_empty() {
            Ok(Self::Default)
        } else {
            validate_batch_format(&format)?;
            Ok(Self::Custom(format))
        }
    }

    fn uses_rest(&self) -> bool {
        match self {
            Self::Default => false,
            Self::Custom(format) => format.contains("%(rest)"),
        }
    }

    fn uses_deltabase(&self) -> bool {
        match self {
            Self::Default => false,
            Self::Custom(format) => format.contains("%(deltabase)"),
        }
    }

    fn uses_disk_size(&self) -> bool {
        match self {
            Self::Default => false,
            Self::Custom(format) => format.contains("%(objectsize:disk)"),
        }
    }
}

pub(crate) fn hash_object_command(
    object_type: &str,
    write: bool,
    stdin: bool,
    paths: Vec<PathBuf>,
) -> Result<()> {
    let kind = parse_object_kind(object_type)?;
    if !stdin && paths.is_empty() {
        return Err(CliError::Message(
            "`hash-object` requires --stdin or at least one path".into(),
        ));
    }
    let worktree_repo = find_repo().ok();
    let write_repo = if write {
        Some(find_repo_or_bare()?)
    } else {
        None
    };
    let store = write_repo
        .as_ref()
        .map(|repo| LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1));

    if stdin {
        println!("{}", write_or_hash_stdin(store.as_ref(), kind)?.to_hex());
    }

    for path in paths {
        println!(
            "{}",
            write_or_hash_path(store.as_ref(), worktree_repo.as_ref(), kind, &path)?.to_hex()
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cat_file(
    type_only: bool,
    pretty: bool,
    size: bool,
    exists: bool,
    textconv: bool,
    filters: bool,
    path: Option<String>,
    batch_check: Option<String>,
    batch: Option<String>,
    batch_command: Option<String>,
    batch_all_objects: bool,
    buffer: bool,
    no_buffer: bool,
    follow_symlinks: bool,
    nul: bool,
    full_nul: bool,
    unordered: bool,
    no_unordered: bool,
    filter: Option<String>,
    no_filter: bool,
    objects: Vec<String>,
) -> Result<()> {
    let object_filter = if no_filter {
        None
    } else {
        filter.as_deref().map(parse_cat_file_filter).transpose()?
    };
    let has_batch_check = batch_check.is_some();
    let has_batch = batch.is_some();
    let has_batch_command = batch_command.is_some();
    let selected = [
        type_only,
        pretty,
        size,
        exists,
        textconv,
        filters,
        has_batch_check,
        has_batch,
        has_batch_command,
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selected == 0 && objects.len() == 2 {
        if path.is_some()
            || buffer
            || no_buffer
            || follow_symlinks
            || batch_all_objects
            || nul
            || full_nul
        {
            return Err(cat_file_usage_incompatible(
                "options are incompatible with this mode",
            ));
        }
        return cat_file_typed_object(&objects[0], &objects[1]);
    }
    if selected == 0 {
        if path.is_some()
            || buffer
            || no_buffer
            || follow_symlinks
            || batch_all_objects
            || nul
            || full_nul
        {
            return Err(cat_file_usage_incompatible(
                "option needs batch mode or a compatible object mode",
            ));
        }
        return Err(CliError::Stderr {
            code: 129,
            text: cat_file_usage(),
        });
    }
    if selected != 1 {
        return Err(cat_file_usage_incompatible(
            "options cannot be used together; incompatible with batch mode",
        ));
    }
    if !has_batch_check && !has_batch && !has_batch_command {
        if path.is_some() && !(textconv || filters) {
            return Err(cat_file_usage_incompatible(
                "--path is incompatible with this mode",
            ));
        }
        if batch_all_objects {
            return Err(cat_file_usage_incompatible(
                "options cannot be used together; incompatible with batch mode",
            ));
        }
        if buffer || no_buffer || follow_symlinks || nul || full_nul {
            return Err(cat_file_usage_incompatible(
                "option needs batch mode or a compatible object mode",
            ));
        }
    }
    if batch_all_objects && !(has_batch_check || has_batch || has_batch_command) {
        return Err(CliError::Fatal {
            code: 129,
            message: "'--batch-all-objects' requires a batch mode".into(),
        });
    }
    let repo = find_repo_or_bare()?;
    if has_batch_check || has_batch || has_batch_command {
        if path.is_some() {
            return Err(cat_file_usage_incompatible(
                "--path is incompatible with batch mode",
            ));
        }
        if !objects.is_empty() {
            return Err(cat_file_usage_incompatible(
                "batch modes are incompatible with object arguments",
            ));
        }
        let (mode, format) = match (batch_check, batch, batch_command) {
            (Some(format), None, None) => (BatchMode::Check, BatchFormat::from_cli(format)?),
            (None, Some(format), None) => (BatchMode::Contents, BatchFormat::from_cli(format)?),
            (None, None, Some(format)) => (BatchMode::Command, BatchFormat::from_cli(format)?),
            _ => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: "`cat-file` batch mode was already validated".into(),
                });
            }
        };
        return cat_file_batch(
            &repo,
            mode,
            format,
            buffer,
            batch_all_objects,
            nul || full_nul,
            full_nul,
            unordered && !no_unordered,
            follow_symlinks,
            object_filter,
        );
    }
    if objects.is_empty() {
        return Err(CliError::Stderr {
            code: 129,
            text: cat_file_usage_error(&cat_file_required_object_message(
                type_only, pretty, size, exists, textconv, filters,
            )),
        });
    }
    if objects.len() > 1 {
        return Err(CliError::Stderr {
            code: 129,
            text: cat_file_usage_error("too many arguments"),
        });
    }
    let objectish = &objects[0];
    let id = match resolve_objectish(&repo, objectish) {
        Ok(id) => id,
        Err(_) => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("Not a valid object name {objectish}"),
            });
        }
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if filters {
        return cat_file_filters(&repo, &store, objectish, &id, path.as_deref());
    }
    if textconv {
        return cat_file_textconv(&repo, &store, objectish, &id, path.as_deref());
    }
    if exists {
        return if store.contains_object(&id)? {
            Ok(())
        } else {
            Err(CliError::Exit(1))
        };
    }
    if type_only {
        match store.object_header_hint(&id) {
            Ok(Some((kind, _))) => {
                println!("{}", kind.as_str());
                return Ok(());
            }
            Ok(None) => {}
            Err(error) => {
                return Err(cat_file_object_read_error(error, objectish, pretty));
            }
        }
        match store.object_kind_hint(&id) {
            Ok(Some(kind)) => {
                println!("{}", kind.as_str());
                return Ok(());
            }
            Ok(None) => {}
            Err(error) => {
                return Err(cat_file_object_read_error(error, objectish, pretty));
            }
        }
    }
    if size {
        match store.object_header_hint(&id) {
            Ok(Some((_, object_size))) => {
                println!("{object_size}");
                return Ok(());
            }
            Ok(None) => {}
            Err(error) => {
                return Err(cat_file_object_read_error(error, objectish, pretty));
            }
        }
    }
    let object = match cat_file_read_object(&repo, &store, &id) {
        Ok(object) => object,
        Err(error) => return Err(cat_file_object_read_error(error, objectish, pretty)),
    };
    if type_only {
        println!("{}", object.kind.as_str());
    } else if size {
        println!("{}", object.content.len());
    } else if pretty {
        if object.kind == GitObjectKind::Tree {
            print_tree(&store, &id)?;
        } else {
            io::stdout().write_all(&object.content)?;
        }
    }
    Ok(())
}

fn cat_file_filters(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
    id: &ObjectId,
    path: Option<&str>,
) -> Result<()> {
    let relative = cat_file_filter_path(objectish, path)?;
    let object = match cat_file_read_object(repo, store, id) {
        Ok(object) => object,
        Err(error) => return Err(cat_file_object_read_error(error, objectish, false)),
    };
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("object {objectish} is not a blob"),
        });
    }
    let content = smudge_worktree_content(
        repo,
        &relative,
        id,
        &WorktreeCheckoutMetadata::default(),
        object.content,
    )?;
    io::stdout().write_all(&content)?;
    Ok(())
}

fn cat_file_textconv(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
    id: &ObjectId,
    path: Option<&str>,
) -> Result<()> {
    let relative = cat_file_filter_path(objectish, path)?;
    let object = match cat_file_read_object(repo, store, id) {
        Ok(object) => object,
        Err(error) => return Err(cat_file_object_read_error(error, objectish, false)),
    };
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("object {objectish} is not a blob"),
        });
    }
    let Some(command) = cat_file_textconv_command(repo, &relative)? else {
        io::stdout().write_all(&object.content)?;
        return Ok(());
    };
    let output = run_cat_file_textconv(repo, &command, &object.content)?;
    io::stdout().write_all(&output)?;
    Ok(())
}

fn cat_file_filter_path(objectish: &str, path: Option<&str>) -> Result<Vec<u8>> {
    match path {
        Some(path) => normalize_git_path(path)
            .map(|path| path.into_bytes())
            .map_err(CliError::Io),
        None => objectish_path_component(objectish)
            .map_err(CliError::Io)?
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("<object>:<path> required, only <object> '{objectish}' given"),
            }),
    }
}

fn cat_file_textconv_command(repo: &GitRepo, relative: &[u8]) -> Result<Option<String>> {
    let attributes = GitAttributes::load_from_root(&repo.root)?;
    let driver = attributes
        .check(relative, &["diff".to_owned()])
        .into_iter()
        .find_map(|(_, value)| match value {
            AttributeValue::Value(driver) if !driver.is_empty() => Some(driver),
            _ => None,
        });
    let Some(driver) = driver else {
        return Ok(None);
    };
    Ok(read_config_value(repo, &format!("diff.{driver}.textconv"))?)
}

fn run_cat_file_textconv(repo: &GitRepo, command: &str, content: &[u8]) -> Result<Vec<u8>> {
    let temp_path = create_cat_file_textconv_temp(repo, content)?;
    let output = ProcessCommand::new(git_shell_command_path())
        .arg("-c")
        .arg(format!("{command} \"$1\""))
        .arg("zmin-textconv")
        .arg(&temp_path)
        .current_dir(&repo.root)
        .output();
    let _ = fs::remove_file(&temp_path);
    let output = output.map_err(CliError::Io)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(CliError::Stderr {
            code: output.status.code().unwrap_or(1),
            text: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn create_cat_file_textconv_temp(repo: &GitRepo, content: &[u8]) -> Result<PathBuf> {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..100u32 {
        let path = repo
            .git_dir
            .join(format!("zmin-textconv-{pid}-{nanos}-{attempt}.tmp"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(content)?;
                file.flush()?;
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Err(CliError::Io(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create textconv temporary file",
    )))
}

fn cat_file_read_object(
    repo: &GitRepo,
    store: &LooseObjectStore,
    id: &ObjectId,
) -> io::Result<LooseObject> {
    match store.read_object(id) {
        Ok(object) => Ok(object),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match admin_commands::backfill_promisor_objects(repo, std::slice::from_ref(id)) {
                Ok(true) => store.read_object(id),
                Ok(false) => Err(error),
                Err(CliError::Io(io_error)) => Err(io_error),
                Err(cli_error) => Err(io::Error::other(format!("{cli_error:?}"))),
            }
        }
        Err(error) => Err(error),
    }
}

fn cat_file_object_read_error(error: io::Error, objectish: &str, pretty: bool) -> CliError {
    if error.kind() == io::ErrorKind::NotFound {
        return if pretty {
            CliError::Fatal {
                code: 128,
                message: format!("Not a valid object name {objectish}"),
            }
        } else {
            CliError::Stderr {
                code: 128,
                text: "fatal: git cat-file: could not get object info\n".to_owned(),
            }
        };
    }
    if error.kind() == io::ErrorKind::InvalidData
        && error.to_string() == "object type header too long"
    {
        let fatal = if pretty {
            format!("fatal: Not a valid object name {objectish}\n")
        } else {
            "fatal: git cat-file: could not get object info\n".to_owned()
        };
        return CliError::Stderr {
            code: 128,
            text: format!("error: header for {objectish} too long, exceeds 32 bytes\n{fatal}"),
        };
    }
    CliError::Io(error)
}

fn cat_file_typed_object(object_type: &str, objectish: &str) -> Result<()> {
    let expected = parse_object_kind(object_type).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid object type \"{object_type}\""),
    })?;
    let repo = find_repo_or_bare()?;
    let id = resolve_objectish(&repo, objectish).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("Not a valid object name {objectish}"),
    })?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut object = cat_file_read_object(&repo, &store, &id)
        .map_err(|error| cat_file_typed_object_read_error(error, objectish))?;
    for _ in 0..8 {
        if object.kind != GitObjectKind::Tag || expected == GitObjectKind::Tag {
            break;
        }
        let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
        object = cat_file_read_object(&repo, &store, &tag.target)
            .map_err(|error| cat_file_typed_object_read_error(error, &tag.target.to_hex()))?;
    }
    if object.kind != expected {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "object {objectish} is a {}, not a {object_type}",
                object.kind.as_str()
            ),
        });
    }
    io::stdout().write_all(&object.content)?;
    Ok(())
}

fn cat_file_typed_object_read_error(error: io::Error, objectish: &str) -> CliError {
    if error.to_string() == "corrupt deflate stream" {
        return CliError::Stderr {
            code: 128,
            text: format!(
                "error: unable to unpack {objectish} header\nerror: inflate: needs dictionary\n"
            ),
        };
    }
    CliError::Io(error)
}

fn cat_file_required_object_message(
    type_only: bool,
    pretty: bool,
    size: bool,
    exists: bool,
    textconv: bool,
    filters: bool,
) -> String {
    let mode = if type_only {
        "-t"
    } else if pretty {
        "-p"
    } else if size {
        "-s"
    } else if exists {
        "-e"
    } else if textconv {
        "--textconv"
    } else if filters {
        "--filters"
    } else {
        "<type>"
    };
    format!("<object> required with '{mode}'")
}

fn cat_file_usage_error(message: &str) -> String {
    format!("fatal: {message}\n\n{}", cat_file_usage())
}

fn cat_file_usage_incompatible(message: &str) -> CliError {
    CliError::Stderr {
        code: 129,
        text: format!("error: {message}\n\n{}", cat_file_usage()),
    }
}

fn cat_file_usage() -> String {
    "usage: git cat-file <type> <object>
   or: git cat-file (-e | -p | -t | -s) <object>
   or: git cat-file (--textconv | --filters)
                    [<rev>:<path|tree-ish> | --path=<path|tree-ish> <rev>]
   or: git cat-file (--batch | --batch-check | --batch-command) [--batch-all-objects]
                    [--buffer] [--follow-symlinks] [--unordered]
                    [--textconv | --filters] [-Z]

Check object existence or emit object contents
    -e                    check if <object> exists
    -p                    pretty-print <object> content

Emit [broken] object attributes
    -t                    show object type (one of 'blob', 'tree', 'commit', 'tag', ...)
    -s                    show object size
    --[no-]use-mailmap    use mail map file
    --[no-]mailmap ...    alias of --use-mailmap

Batch objects requested on stdin (or --batch-all-objects)
    --batch[=<format>]    show full <object> or <rev> contents
    --batch-check[=<format>]
                          like --batch, but don't emit <contents>
    -Z                    stdin and stdout is NUL-terminated
    --batch-command[=<format>]
                          read commands from stdin
    --batch-all-objects   with --batch[-check]: ignores stdin, batches all known objects

Change or optimize batch output
    --[no-]buffer         buffer --batch output
    --[no-]follow-symlinks
                          follow in-tree symlinks
    --[no-]unordered      do not order objects before emitting them

Emit object (blob or tree) with conversion or filter (stand-alone, or with batch)
    --textconv            run textconv on object's content
    --filters             run filters on object's content
    --[no-]path blob|tree use a <path> for (--textconv | --filters); Not with 'batch'
    --[no-]filter <args>  object filtering

"
    .to_owned()
}

fn parse_cat_file_filter(spec: &str) -> Result<CatFileFilter> {
    if spec == "blob:none" {
        return Ok(CatFileFilter::BlobNone);
    }
    if let Some(raw_limit) = spec.strip_prefix("blob:limit=") {
        return parse_blob_limit_filter(raw_limit).map(CatFileFilter::BlobLimit);
    }
    if let Some(raw_type) = spec.strip_prefix("object:type=") {
        let kind = parse_object_kind(raw_type).map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("invalid filter-spec '{spec}'"),
        })?;
        return Ok(CatFileFilter::ObjectType(kind));
    }
    if let Some(option) = spec.strip_prefix("sparse:path=") {
        let _ = option;
        return Err(CliError::Stderr {
            code: 128,
            text: "fatal: sparse:path filters support has been dropped\n".into(),
        });
    }
    if let Some((name, _)) = spec.split_once('=') {
        return Err(CliError::Stderr {
            code: 129,
            text: format!("usage: objects filter not supported: '{name}'\n"),
        });
    }
    if let Some((name, _)) = spec.split_once(':') {
        return Err(CliError::Stderr {
            code: 129,
            text: format!("usage: objects filter not supported: '{name}'\n"),
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("invalid filter-spec '{spec}'"),
    })
}

fn parse_blob_limit_filter(raw_limit: &str) -> Result<usize> {
    let (digits, multiplier) = match raw_limit.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&raw_limit[..raw_limit.len() - 1], 1024usize),
        Some(b'm' | b'M') => (&raw_limit[..raw_limit.len() - 1], 1024usize * 1024),
        Some(b'g' | b'G') => (&raw_limit[..raw_limit.len() - 1], 1024usize * 1024 * 1024),
        _ => (raw_limit, 1usize),
    };
    let value = digits.parse::<usize>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid filter-spec 'blob:limit={raw_limit}'"),
    })?;
    value
        .checked_mul(multiplier)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("invalid filter-spec 'blob:limit={raw_limit}'"),
        })
}

fn cat_file_filter_includes(
    store: &LooseObjectStore,
    id: &ObjectId,
    filter: Option<CatFileFilter>,
) -> io::Result<bool> {
    let Some(filter) = filter else {
        return Ok(true);
    };
    let Some((kind, size)) = store.object_header_hint(id)? else {
        return Ok(false);
    };
    match filter {
        CatFileFilter::BlobNone => Ok(kind != GitObjectKind::Blob),
        CatFileFilter::BlobLimit(limit) => Ok(kind != GitObjectKind::Blob || size <= limit),
        CatFileFilter::ObjectType(expected) => Ok(kind == expected),
    }
}

fn cat_file_batch(
    repo: &GitRepo,
    mode: BatchMode,
    format: BatchFormat,
    buffer: bool,
    batch_all_objects: bool,
    input_nul: bool,
    output_nul: bool,
    unordered: bool,
    follow_symlinks: bool,
    object_filter: Option<CatFileFilter>,
) -> Result<()> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let stdout = io::stdout();
    let stdout = stdout.lock();
    if buffer {
        let mut stdout = BufWriter::with_capacity(CAT_FILE_BATCH_OUTPUT_BUF_CAPACITY, stdout);
        cat_file_batch_with_writer(
            repo,
            &store,
            &mut stdout,
            mode,
            &format,
            buffer,
            batch_all_objects,
            input_nul,
            output_nul,
            unordered,
            follow_symlinks,
            object_filter,
        )?;
        if env::var_os("GIT_TEST_CAT_FILE_NO_FLUSH_ON_EXIT").is_some() {
            std::mem::forget(stdout);
        } else {
            stdout.flush()?;
        }
        return Ok(());
    }

    let mut stdout = stdout;
    cat_file_batch_with_writer(
        repo,
        &store,
        &mut stdout,
        mode,
        &format,
        buffer,
        batch_all_objects,
        input_nul,
        output_nul,
        unordered,
        follow_symlinks,
        object_filter,
    )
}

fn cat_file_batch_with_writer<W: io::Write>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut stdout: &mut W,
    mode: BatchMode,
    format: &BatchFormat,
    buffer: bool,
    batch_all_objects: bool,
    input_nul: bool,
    output_nul: bool,
    unordered: bool,
    follow_symlinks: bool,
    object_filter: Option<CatFileFilter>,
) -> Result<()> {
    if batch_all_objects {
        if format.uses_disk_size() {
            for (id, disk_size) in store.disk_object_entries()? {
                if !cat_file_filter_includes(store, &id, object_filter)? {
                    continue;
                }
                write_batch_all_object(
                    stdout,
                    store,
                    &id,
                    mode,
                    format,
                    output_nul,
                    Some(disk_size),
                )?;
            }
            return Ok(());
        }
        if unordered {
            let mut write_object = |id: &ObjectId| -> io::Result<()> {
                if !cat_file_filter_includes(store, id, object_filter)? {
                    return Ok(());
                }
                write_batch_all_object(stdout, store, id, mode, format, output_nul, None)
            };
            store.for_each_object_id(&mut write_object)?;
            return Ok(());
        }
        for id in store.object_ids()? {
            if !cat_file_filter_includes(store, &id, object_filter)? {
                continue;
            }
            write_batch_all_object(stdout, store, &id, mode, format, output_nul, None)?;
        }
        return Ok(());
    }

    let split_rest = format.uses_rest();
    let stdin = io::stdin();
    let mut stdin = io::BufReader::new(stdin.lock());
    let mut record = Vec::new();
    loop {
        record.clear();
        let read = if input_nul {
            stdin.read_until(0, &mut record)?
        } else {
            stdin.read_until(b'\n', &mut record)?
        };
        if read == 0 {
            break;
        }
        if input_nul {
            if record.last() == Some(&0) {
                record.pop();
            }
        } else {
            while record
                .last()
                .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
            {
                record.pop();
            }
        }
        let line = String::from_utf8(record.clone()).map_err(|error| {
            CliError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                error.to_string(),
            ))
        })?;
        let line = line.as_str();
        let command = if mode == BatchMode::Command {
            parse_batch_command(line, buffer, split_rest)?
        } else {
            let (objectish, rest) = if split_rest {
                split_batch_object_input(line)
            } else {
                (line, "")
            };
            BatchCommand::Object(objectish, rest, mode == BatchMode::Contents)
        };
        let BatchCommand::Object(objectish, rest, include_content) = command else {
            stdout.flush()?;
            continue;
        };
        let resolved = match resolve_batch_objectish(repo, store, objectish, follow_symlinks) {
            Ok(BatchLookup::Object(resolved)) => resolved,
            Ok(BatchLookup::Special { kind, payload }) => {
                write_batch_special(&mut stdout, kind, &payload, output_nul)?;
                continue;
            }
            Err(_) => {
                write_batch_missing(&mut stdout, objectish, output_nul)?;
                continue;
            }
        };
        let read_id =
            cat_file_replacement_id(repo, &resolved.id)?.unwrap_or_else(|| resolved.id.clone());
        if !cat_file_filter_includes(store, &read_id, object_filter)? {
            write_batch_excluded(&mut stdout, &resolved.id, output_nul)?;
            continue;
        }
        let mode_atom = resolved.mode.as_deref().unwrap_or("");
        if !include_content
            && mode_atom == "160000"
            && store.object_header_hint(&read_id)?.is_none()
        {
            write_batch_submodule(&mut stdout, &resolved.id, output_nul)?;
            continue;
        }
        if !include_content
            && write_object_batch_header(
                &mut stdout,
                store,
                &resolved.id,
                &read_id,
                format,
                rest,
                mode_atom,
                output_nul,
            )?
        {
            continue;
        }
        match store.read_object(&read_id) {
            Ok(object) => write_batch_object(
                &mut stdout,
                store,
                &resolved.id,
                &object,
                include_content,
                format,
                rest,
                mode_atom,
                output_nul,
            )?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                write_batch_missing(&mut stdout, objectish, output_nul)?
            }
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Ok(())
}

fn write_batch_all_object<W: io::Write>(
    writer: &mut W,
    store: &LooseObjectStore,
    id: &ObjectId,
    mode: BatchMode,
    format: &BatchFormat,
    output_nul: bool,
    disk_size_override: Option<u64>,
) -> io::Result<()> {
    if mode == BatchMode::Check
        && let Some((kind, size)) = store.object_header_hint(id)?
    {
        let delta_base_atom = batch_delta_base_atom(store, id, format)?;
        let disk_size_atom = batch_disk_size_atom(store, id, format, disk_size_override)?;
        write_batch_header(
            writer,
            id,
            kind,
            size,
            format,
            "",
            "",
            &delta_base_atom,
            &disk_size_atom,
            output_nul,
        )?;
        return Ok(());
    }
    let object = store.read_object(id)?;
    let delta_base_atom = batch_delta_base_atom(store, &object.id, format)?;
    let disk_size_atom = batch_disk_size_atom(store, &object.id, format, disk_size_override)?;
    write_batch_header(
        writer,
        &object.id,
        object.kind,
        object.content.len(),
        format,
        "",
        "",
        &delta_base_atom,
        &disk_size_atom,
        output_nul,
    )?;
    if mode == BatchMode::Contents {
        writer.write_all(&object.content)?;
        write_batch_terminator(writer, output_nul)?;
    }
    Ok(())
}

fn cat_file_replacement_id(repo: &GitRepo, id: &ObjectId) -> io::Result<Option<ObjectId>> {
    if env::var_os("GIT_NO_REPLACE_OBJECTS").is_some() {
        return Ok(None);
    }
    let refs = RefStore::new(&repo.git_dir, id.algorithm());
    match refs.resolve(&format!("refs/replace/{}", id.to_hex())) {
        Ok(replacement) => Ok(Some(replacement)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn unpack_file(objectish: &str) -> Result<()> {
    let repo = find_repo()?;
    let id = resolve_objectish(&repo, objectish).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("Not a valid object name {objectish}"),
    })?;
    let store = LooseObjectStore::new(repo.objects_dir, GitHashAlgorithm::Sha1);
    let object = store.read_object(&id).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("unable to read blob object {}", id.to_hex()),
    })?;
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("unable to read blob object {}", id.to_hex()),
        });
    }

    let cwd = canonical_or_absolute(std::env::current_dir()?);
    let path = create_merge_file(&cwd, &object.content)?;
    println!("{}", path.display());
    Ok(())
}

fn create_merge_file(dir: &std::path::Path, content: &[u8]) -> Result<PathBuf> {
    use std::fs::OpenOptions;

    for attempt in 0..1024_u32 {
        let name = format!(".merge_file_{}_{attempt}", std::process::id());
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(content)?;
                return Ok(path.file_name().map(PathBuf::from).unwrap_or(path));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Err(CliError::Fatal {
        code: 128,
        message: "unable to create temporary merge file".into(),
    })
}

pub(crate) fn show_index() -> Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    for_each_pack_index_entry(GitHashAlgorithm::Sha1, input, &mut |entry| {
        println!(
            "{} {} ({:08x})",
            entry.offset,
            entry.object_id.to_hex(),
            entry.crc32
        );
        Ok(())
    })
    .map_err(show_index_error)?;
    Ok(())
}

fn show_index_error(error: io::Error) -> CliError {
    if error.kind() == io::ErrorKind::InvalidData
        && error
            .to_string()
            .starts_with("unsupported pack index version ")
    {
        return CliError::Fatal {
            code: 128,
            message: "unknown index version".into(),
        };
    }
    CliError::Io(error)
}

pub(crate) fn update_server_info() -> Result<()> {
    let repo = find_repo_or_bare()?;
    let runtime = CliPrimitiveRuntime::new_default(&repo);
    fs::create_dir_all(repo.git_dir.join("info"))?;
    let mut info_refs = BufWriter::new(fs::File::create(repo.git_dir.join("info/refs"))?);
    let server_info_refs = runtime
        .refs_store_adapter()
        .server_info_refs()
        .map_err(|error| CliError::Fatal {
            code: 128,
            message: format!("read server-info refs: {error}"),
        })?;

    for (name, id) in server_info_refs {
        writeln!(info_refs, "{}\t{}", id, name)?;
    }
    info_refs.flush()?;

    let packs = PackedObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1).pack_names()?;
    fs::create_dir_all(repo.git_dir.join("objects/info"))?;
    let mut packs_info = BufWriter::new(fs::File::create(repo.git_dir.join("objects/info/packs"))?);
    for pack in packs {
        writeln!(packs_info, "P {pack}")?;
    }
    packs_info.write_all(b"\n")?;
    packs_info.flush()?;
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct LooseObjectStats {
    pub(crate) ids: HashSet<ObjectId>,
    pub(crate) count: u64,
    pub(crate) size_kib: u64,
    pub(crate) size_bytes: u64,
    pub(crate) garbage: u64,
    pub(crate) garbage_size_kib: u64,
    pub(crate) garbage_size_bytes: u64,
}

#[derive(Debug, Default)]
pub(crate) struct PackObjectStats {
    pub(crate) objects: u64,
    pub(crate) packs: u64,
    pub(crate) size_bytes: u64,
}

pub(crate) fn collect_loose_object_stats(
    objects_dir: &std::path::Path,
    algorithm: GitHashAlgorithm,
    collect_ids: bool,
) -> Result<LooseObjectStats> {
    let mut stats = LooseObjectStats::default();
    let entries = match fs::read_dir(objects_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(stats),
        Err(err) => return Err(CliError::Io(err)),
    };
    let suffix_len = algorithm.digest_len() * 2 - 2;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let dir_name = entry.file_name();
        let Some(dir_name) = dir_name.to_str() else {
            continue;
        };
        if !is_loose_object_dir_name(dir_name) {
            continue;
        }
        let file_entries = match fs::read_dir(entry.path()) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(CliError::Io(err)),
        };
        for file_entry in file_entries {
            let file_entry = file_entry?;
            if !file_entry.file_type()?.is_file() {
                continue;
            }
            let metadata = file_entry.metadata()?;
            let file_name = file_entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                stats.garbage += 1;
                stats.garbage_size_kib += file_allocated_kib(&metadata);
                stats.garbage_size_bytes += file_counted_bytes(&metadata);
                continue;
            };
            if file_name.len() == suffix_len
                && file_name.as_bytes().iter().all(u8::is_ascii_hexdigit)
            {
                stats.count += 1;
                if collect_ids {
                    let id = ObjectId::from_hex(algorithm, &format!("{dir_name}{file_name}"))?;
                    stats.ids.insert(id);
                }
                stats.size_kib += file_allocated_kib(&metadata);
                stats.size_bytes += file_counted_bytes(&metadata);
            } else {
                stats.garbage += 1;
                stats.garbage_size_kib += file_allocated_kib(&metadata);
                stats.garbage_size_bytes += file_counted_bytes(&metadata);
            }
        }
    }
    Ok(stats)
}

pub(crate) fn collect_pack_object_stats(objects_dir: &std::path::Path) -> Result<PackObjectStats> {
    let pack_dir = objects_dir.join("pack");
    let entries = match fs::read_dir(pack_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(PackObjectStats::default()),
        Err(err) => return Err(CliError::Io(err)),
    };
    let mut stats = PackObjectStats::default();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        match path.extension().and_then(|value| value.to_str()) {
            Some("pack") => {
                stats.packs += 1;
                stats.size_bytes += entry.metadata()?.len();
            }
            Some("idx") => {
                stats.size_bytes += entry.metadata()?.len();
                stats.objects += pack_index_object_count(&path)? as u64;
            }
            _ => {}
        }
    }
    Ok(stats)
}

fn count_objects_size_field(size_kib: u64, size_bytes: u64, human_readable: bool) -> String {
    if human_readable {
        human_size(size_bytes)
    } else {
        size_kib.to_string()
    }
}

fn count_objects_byte_field(size_bytes: u64, human_readable: bool) -> String {
    if human_readable {
        human_size(size_bytes)
    } else {
        (size_bytes / 1024).to_string()
    }
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let units = ["KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = units[0];
    for candidate in units.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = candidate;
    }
    format!("{value:.2} {unit}")
}

#[cfg(unix)]
fn file_allocated_kib(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;

    ceil_div(metadata.blocks(), 2)
}

#[cfg(not(unix))]
fn file_allocated_kib(metadata: &fs::Metadata) -> u64 {
    metadata.len() / 1024
}

#[cfg(unix)]
fn file_counted_bytes(metadata: &fs::Metadata) -> u64 {
    file_allocated_kib(metadata).saturating_mul(1024)
}

#[cfg(not(unix))]
fn file_counted_bytes(metadata: &fs::Metadata) -> u64 {
    metadata.len()
}

#[cfg(unix)]
fn ceil_div(value: u64, divisor: u64) -> u64 {
    if value == 0 {
        0
    } else {
        (value - 1) / divisor + 1
    }
}

fn is_loose_object_dir_name(value: &str) -> bool {
    value.len() == 2 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

pub(crate) fn count_objects(verbose: bool, human_readable: bool) -> Result<()> {
    let repo = find_repo_or_bare()?;

    if !verbose {
        let loose = collect_loose_object_stats(&repo.objects_dir, GitHashAlgorithm::Sha1, false)?;
        let size = if human_readable {
            human_size(loose.size_bytes)
        } else {
            format!("{} kilobytes", loose.size_kib)
        };
        println!("{} objects, {size}", loose.count);
        return Ok(());
    }

    let pack = collect_pack_object_stats(&repo.objects_dir)?;
    let loose =
        collect_loose_object_stats(&repo.objects_dir, GitHashAlgorithm::Sha1, pack.objects != 0)?;
    let prune_packable = if loose.ids.is_empty() || pack.objects == 0 {
        0
    } else {
        count_prune_packable_objects(&repo.objects_dir, &loose.ids)?
    };

    println!("count: {}", loose.count);
    println!(
        "size: {}",
        count_objects_size_field(loose.size_kib, loose.size_bytes, human_readable)
    );
    println!("in-pack: {}", pack.objects);
    println!("packs: {}", pack.packs);
    println!(
        "size-pack: {}",
        count_objects_byte_field(pack.size_bytes, human_readable)
    );
    println!("prune-packable: {prune_packable}");
    println!("garbage: {}", loose.garbage);
    println!(
        "size-garbage: {}",
        count_objects_size_field(
            loose.garbage_size_kib,
            loose.garbage_size_bytes,
            human_readable
        )
    );
    Ok(())
}

fn count_prune_packable_objects(
    objects_dir: &std::path::Path,
    loose_ids: &HashSet<ObjectId>,
) -> Result<u64> {
    let packed = PackedObjectStore::new(objects_dir, GitHashAlgorithm::Sha1);
    let mut prune_packable = 0_u64;
    for id in loose_ids {
        if packed.contains_object(id)? {
            prune_packable += 1;
        }
    }
    Ok(prune_packable)
}

pub(crate) fn check_ref_format_command(
    allow_onelevel: bool,
    normalize: bool,
    branch: Option<&str>,
    refname: Option<&str>,
) -> Result<()> {
    if let Some(branch) = branch {
        if allow_onelevel || normalize || refname.is_some() {
            return Err(CliError::Fatal {
                code: 129,
                message: "usage: git check-ref-format [--normalize] [<options>] <refname>\n   or: git check-ref-format --branch <branchname-shorthand>".into(),
            });
        }
        let full_ref = if branch.starts_with("refs/") {
            branch.to_owned()
        } else {
            format!("refs/heads/{branch}")
        };
        if check_ref_format(&full_ref, false) {
            println!("{branch}");
            return Ok(());
        }
        return Err(CliError::Fatal {
            code: 128,
            message: format!("'{branch}' is not a valid branch name"),
        });
    }

    let Some(refname) = refname else {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git check-ref-format [--normalize] [<options>] <refname>\n   or: git check-ref-format --branch <branchname-shorthand>".into(),
        });
    };

    let candidate = if normalize && refname.ends_with('/') {
        refname.to_owned()
    } else if normalize {
        normalize_refname(refname)
    } else {
        refname.to_owned()
    };
    if check_ref_format(&candidate, allow_onelevel) {
        if normalize {
            println!("{candidate}");
        }
        Ok(())
    } else {
        Err(CliError::Exit(1))
    }
}

fn normalize_refname(value: &str) -> String {
    value
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn check_ignore(
    quiet: bool,
    verbose: bool,
    non_matching: bool,
    stdin: bool,
    nul: bool,
    no_index: bool,
    paths: Vec<PathBuf>,
) -> Result<()> {
    if nul && !stdin {
        return Err(CliError::Fatal {
            code: 128,
            message: "-z only makes sense with --stdin".into(),
        });
    }
    if stdin && !paths.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot specify pathnames with --stdin".into(),
        });
    }

    if !stdin && (paths.is_empty() || paths.iter().any(|path| path.as_os_str().is_empty())) {
        return Err(CliError::Fatal {
            code: 128,
            message: "no path specified".into(),
        });
    }

    if quiet && !stdin && paths.len() != 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "--quiet is only valid with a single pathname".into(),
        });
    }

    if quiet && verbose {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot have both --quiet and --verbose".into(),
        });
    }

    let repo = find_repo()?;
    ensure_check_ignore_worktree(&repo)?;
    let ignore = check_ignore_excludes(&repo)?;
    let index = if no_index {
        GitIndex::new()
    } else {
        read_repo_index(&repo)?
    };
    let mut matched = false;

    if stdin {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::new(stdin.lock());
        let separator = if nul { 0 } else { b'\n' };
        let mut raw = Vec::new();
        let mut seen_inputs = 0usize;
        loop {
            raw.clear();
            if stdin.read_until(separator, &mut raw)? == 0 {
                break;
            }
            if raw.last() == Some(&separator) {
                raw.pop();
            }
            if !nul && raw.last() == Some(&b'\r') {
                raw.pop();
            }
            if raw.is_empty() {
                continue;
            }
            seen_inputs += 1;
            let input_path = String::from_utf8_lossy(&raw);
            let path = if nul {
                PathBuf::from(input_path.as_ref())
            } else {
                PathBuf::from(unquote_check_ignore_stdin_path(&input_path))
            };
            matched |= check_ignore_path(
                &repo,
                &ignore,
                &index,
                CheckIgnoreOutput {
                    no_index,
                    quiet,
                    verbose,
                    non_matching,
                    nul,
                },
                &path,
            )?;
            io::stdout().flush()?;
        }
        if seen_inputs == 0 {
            return Err(CliError::Exit(1));
        }
    } else {
        for path in &paths {
            matched |= check_ignore_path(
                &repo,
                &ignore,
                &index,
                CheckIgnoreOutput {
                    no_index,
                    quiet,
                    verbose,
                    non_matching,
                    nul,
                },
                path,
            )?;
        }
    }

    if matched {
        Ok(())
    } else {
        Err(CliError::Exit(1))
    }
}

fn ensure_check_ignore_worktree(repo: &GitRepo) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let git_dir = repo
        .git_dir
        .canonicalize()
        .unwrap_or_else(|_| repo.git_dir.clone());
    if cwd == git_dir || cwd.starts_with(&git_dir) {
        return Err(CliError::Fatal {
            code: 128,
            message: "this operation must be run in a work tree".into(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct CheckIgnoreOutput {
    no_index: bool,
    quiet: bool,
    verbose: bool,
    non_matching: bool,
    nul: bool,
}

fn check_ignore_path(
    repo: &GitRepo,
    ignore: &GitIgnore,
    index: &GitIndex,
    output: CheckIgnoreOutput,
    path: &Path,
) -> Result<bool> {
    let relative = check_ignore_path_arg_to_repo_relative(repo, path)?;
    if relative.is_empty() {
        if output.non_matching {
            print_check_ignore_non_match(output, path);
        }
        return Ok(false);
    }
    validate_check_ignore_path_boundary(repo, index, path, &relative)?;
    if !output.no_index && find_index_entry(index, &relative).is_some() {
        if output.non_matching {
            print_check_ignore_non_match(output, path);
        }
        return Ok(false);
    }
    let absolute = repo.root.join(String::from_utf8_lossy(&relative).as_ref());
    let is_dir = absolute.is_dir();
    let Some(ignore_match) = ignore.match_path(&relative, is_dir) else {
        if output.non_matching {
            print_check_ignore_non_match(output, path);
        }
        return Ok(false);
    };
    if ignore_match.is_negation && !output.verbose {
        if output.non_matching {
            print_check_ignore_non_match(output, path);
        }
        return Ok(false);
    }
    if !output.quiet {
        let display_path = check_ignore_display_path(output, path);
        if output.verbose {
            print_check_ignore_verbose(
                output,
                &ignore_match.source,
                ignore_match.line_number,
                &ignore_match.pattern,
                &display_path,
            );
        } else if output.nul {
            print!("{display_path}\0");
        } else {
            println!("{display_path}");
        }
    }
    Ok(true)
}

fn check_ignore_path_arg_to_repo_relative(repo: &GitRepo, path: &Path) -> Result<Vec<u8>> {
    if path.to_string_lossy().starts_with(':') {
        return path_arg_to_repo_relative_allow_root(repo, path);
    }
    let absolute = absolute_path_from_arg(path)?;
    let relative = absolute.strip_prefix(&repo.root).map_err(|_| {
        CliError::Message(format!(
            "{} is outside repository {}",
            absolute.display(),
            repo.root.display()
        ))
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(CliError::Message(format!(
                        "{} is outside repository {}",
                        absolute.display(),
                        repo.root.display()
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(parts.join("/").into_bytes())
}

fn validate_check_ignore_path_boundary(
    repo: &GitRepo,
    index: &GitIndex,
    path: &Path,
    relative: &[u8],
) -> Result<()> {
    let relative_text = String::from_utf8_lossy(relative).replace('\\', "/");
    let parts = relative_text.split('/').collect::<Vec<_>>();
    for prefix_len in 1..parts.len() {
        let prefix = parts[..prefix_len].join("/");
        if repo
            .root
            .join(&prefix)
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("pathspec '{}' is beyond a symbolic link", path.display()),
            });
        }
        let prefix_path = repo.root.join(&prefix);
        if find_index_entry(index, prefix.as_bytes())
            .is_some_and(|entry| entry.mode == IndexMode::Gitlink)
            || prefix_path.join(".git").exists()
        {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("Pathspec '{}' is in submodule '{}'", path.display(), prefix),
            });
        }
    }
    Ok(())
}

fn check_ignore_excludes(repo: &GitRepo) -> Result<GitIgnore> {
    let mut ignore = GitIgnore::default();
    if let Some((path, source)) = check_ignore_global_excludes_file(repo)? {
        append_check_ignore_file(&mut ignore, &path, "", &source)?;
    }
    append_check_ignore_file(
        &mut ignore,
        &repo.git_dir.join("info/exclude"),
        "",
        ".git/info/exclude",
    )?;
    append_check_ignore_per_directory_excludes(&repo.root, &repo.root, &mut ignore)?;
    Ok(ignore)
}

fn append_check_ignore_per_directory_excludes(
    root: &Path,
    dir: &Path,
    ignore: &mut GitIgnore,
) -> Result<()> {
    let exclude_path = dir.join(".gitignore");
    let base = repo_relative_path(root, dir)?;
    let base = String::from_utf8_lossy(&base).replace('\\', "/");
    let source = if base.is_empty() {
        ".gitignore".to_owned()
    } else {
        format!("{base}/.gitignore")
    };
    if exclude_path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("unable to access '{}'", source),
        });
    }
    append_check_ignore_file(ignore, &exclude_path, &base, &source)?;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            let relative = repo_relative_path(root, &path)?;
            if !relative.is_empty() && ignore.is_ignored(&relative, true) {
                continue;
            }
            append_check_ignore_per_directory_excludes(root, &path, ignore)?;
        }
    }
    Ok(())
}

fn append_check_ignore_file(
    ignore: &mut GitIgnore,
    path: &Path,
    base: &str,
    source: &str,
) -> Result<()> {
    match fs::read_to_string(path) {
        Ok(content) => ignore.append(GitIgnore::parse_with_base_and_source(
            &content, base, source,
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn check_ignore_global_excludes_file(repo: &GitRepo) -> Result<Option<(PathBuf, String)>> {
    if let Some(path) = read_config_value(repo, "core.excludesFile")? {
        if path.is_empty() {
            return Ok(None);
        }
        let source = path.clone();
        let expanded = expand_home_path(&path);
        if expanded.is_relative() {
            return Ok(Some((repo.root.join(expanded), source)));
        }
        return Ok(Some((expanded, source)));
    }
    Ok(None)
}

fn print_check_ignore_non_match(output: CheckIgnoreOutput, path: &Path) {
    if output.quiet || !output.verbose {
        return;
    }
    let display_path = check_ignore_display_path(output, path);
    if output.nul {
        print!("\0\0\0{display_path}\0");
    } else {
        println!("::\t{display_path}");
    }
}

fn unquote_check_ignore_stdin_path(input: &str) -> String {
    let Some(quoted) = input
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return input.to_owned();
    };
    let mut out = String::new();
    let mut chars = quoted.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                out.push(escaped);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn check_ignore_display_path(output: CheckIgnoreOutput, path: &Path) -> String {
    let path = path.to_string_lossy();
    if output.nul || !path.contains('"') {
        return path.into_owned();
    }
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('"');
    for ch in path.chars() {
        if ch == '\\' || ch == '"' {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    quoted
}

fn print_check_ignore_verbose(
    output: CheckIgnoreOutput,
    source: &str,
    line_number: usize,
    pattern: &str,
    path: &str,
) {
    if output.nul {
        print!("{source}\0{line_number}\0{pattern}\0{path}\0");
    } else {
        println!("{source}:{line_number}:{pattern}\t{path}");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MailmapIdentity {
    name: String,
    email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MailmapEntry {
    canonical_name: Option<String>,
    canonical_email: String,
    old_name: Option<String>,
    old_email: String,
}

pub(crate) fn check_mailmap(stdin: bool, identities: Vec<String>) -> Result<()> {
    if stdin && !identities.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "check-mailmap --stdin does not take identity arguments".into(),
        });
    }
    let repo = find_repo()?;
    let entries = read_mailmap(&repo)?;
    if stdin {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::new(stdin.lock());
        let mut line = String::new();
        loop {
            line.clear();
            if stdin.read_line(&mut line)? == 0 {
                break;
            }
            check_mailmap_input(&entries, line.trim_end_matches(['\r', '\n']));
        }
    } else {
        if identities.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "check-mailmap requires an identity or --stdin".into(),
            });
        }
        for input in &identities {
            check_mailmap_input(&entries, input);
        }
    }
    Ok(())
}

fn check_mailmap_input(entries: &[MailmapEntry], input: &str) {
    let Some(identity) = parse_mailmap_identity(input) else {
        println!("{input}");
        return;
    };
    let mapped = apply_mailmap(entries, &identity);
    println!("{} <{}>", mapped.name, mapped.email);
}

fn read_mailmap(repo: &GitRepo) -> Result<Vec<MailmapEntry>> {
    let path = repo.root.join(".mailmap");
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    Ok(raw
        .lines()
        .filter_map(parse_mailmap_entry)
        .collect::<Vec<_>>())
}

fn parse_mailmap_entry(line: &str) -> Option<MailmapEntry> {
    let line = line.split('#').next()?.trim();
    if line.is_empty() {
        return None;
    }
    let identities = parse_mailmap_identities(line);
    match identities.as_slice() {
        [canonical, old] => Some(MailmapEntry {
            canonical_name: non_empty_name(&canonical.name),
            canonical_email: canonical.email.clone(),
            old_name: non_empty_name(&old.name),
            old_email: old.email.clone(),
        }),
        [canonical] if !canonical.name.is_empty() => Some(MailmapEntry {
            canonical_name: Some(canonical.name.clone()),
            canonical_email: canonical.email.clone(),
            old_name: None,
            old_email: canonical.email.clone(),
        }),
        _ => None,
    }
}

fn parse_mailmap_identities(line: &str) -> Vec<MailmapIdentity> {
    let mut identities = Vec::new();
    let mut rest = line.trim();
    while let Some(end) = rest.find('>') {
        let segment = &rest[..=end];
        if let Some(identity) = parse_mailmap_identity(segment) {
            identities.push(identity);
        }
        rest = rest[end + 1..].trim_start();
    }
    identities
}

fn parse_mailmap_identity(value: &str) -> Option<MailmapIdentity> {
    let end = value.rfind('>')?;
    let before_end = &value[..end];
    let start = before_end.rfind('<')?;
    let email = before_end[start + 1..].trim();
    if email.is_empty() {
        return None;
    }
    Some(MailmapIdentity {
        name: before_end[..start].trim().to_owned(),
        email: email.to_owned(),
    })
}

fn non_empty_name(name: &str) -> Option<String> {
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

fn apply_mailmap(entries: &[MailmapEntry], identity: &MailmapIdentity) -> MailmapIdentity {
    for entry in entries {
        if entry.old_email == identity.email
            && entry
                .old_name
                .as_ref()
                .is_none_or(|name| name == &identity.name)
        {
            return MailmapIdentity {
                name: entry
                    .canonical_name
                    .clone()
                    .unwrap_or_else(|| identity.name.clone()),
                email: entry.canonical_email.clone(),
            };
        }
    }
    identity.clone()
}

pub(crate) fn check_attr(all: bool, stdin: bool, args: Vec<String>) -> Result<()> {
    let repo = find_repo()?;
    let attrs = GitAttributes::load_from_root(&repo.root)?;
    let (attr_names, paths) = parse_check_attr_args(all, stdin, args)?;
    if stdin {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::new(stdin.lock());
        let mut line = String::new();
        loop {
            line.clear();
            if stdin.read_line(&mut line)? == 0 {
                break;
            }
            check_attr_path(
                &repo,
                &attrs,
                all,
                &attr_names,
                line.trim_end_matches(['\r', '\n']),
            )?;
        }
    } else {
        for path in &paths {
            check_attr_path(&repo, &attrs, all, &attr_names, path)?;
        }
    }
    Ok(())
}

fn check_attr_path(
    repo: &GitRepo,
    attrs: &GitAttributes,
    all: bool,
    attr_names: &[String],
    path: &str,
) -> Result<()> {
    let relative = path_arg_to_repo_relative(repo, std::path::Path::new(path))?;
    let rows = if all {
        attrs.check_all(&relative)
    } else {
        attrs.check(&relative, attr_names)
    };
    for (name, value) in rows {
        println!("{path}: {name}: {}", value.as_check_attr_value());
    }
    Ok(())
}

pub(crate) fn unpack_objects(
    dry_run: bool,
    quiet: bool,
    _recover: bool,
    _strict: bool,
) -> Result<()> {
    let repo = find_repo()?;
    if dry_run {
        io::copy(&mut io::stdin().lock(), &mut io::sink())?;
        return Ok(());
    }
    let objects_dir = repo.objects_dir;
    let store = LooseObjectStore::new(objects_dir.clone(), GitHashAlgorithm::Sha1);
    fs::create_dir_all(&objects_dir)?;
    let temp_pack = unique_temp_sibling(&objects_dir.join("unpack-objects-stdin.pack"));
    let copy_result = (|| -> Result<()> {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_pack)?;
        let mut file = io::BufWriter::with_capacity(UNPACK_OBJECTS_STDIN_BUF_CAPACITY, file);
        io::copy(&mut io::stdin().lock(), &mut file)?;
        file.flush()?;
        Ok(())
    })();
    if let Err(error) = copy_result {
        let _ = fs::remove_file(&temp_pack);
        return Err(error);
    }
    let stats = match unpack_pack_file_to_loose(&store, GitHashAlgorithm::Sha1, &temp_pack) {
        Ok(stats) => stats,
        Err(error) => {
            let _ = fs::remove_file(&temp_pack);
            return Err(CliError::Io(error));
        }
    };
    let _ = fs::remove_file(&temp_pack);
    if !quiet {
        eprintln!("Unpacked {} objects.", stats.objects);
    }
    Ok(())
}

fn parse_check_attr_args(
    all: bool,
    stdin: bool,
    args: Vec<String>,
) -> Result<(Vec<String>, Vec<String>)> {
    let separator = args.iter().position(|arg| arg == "--");
    if stdin {
        if separator.is_some() {
            return Err(CliError::Fatal {
                code: 129,
                message: "check-attr --stdin does not take path arguments".into(),
            });
        }
        if all {
            return Ok((Vec::new(), Vec::new()));
        }
        if args.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "check-attr requires attributes".into(),
            });
        }
        return Ok((args, Vec::new()));
    }
    let Some(separator) = separator else {
        return Err(CliError::Fatal {
            code: 129,
            message: "check-attr requires `--` before path arguments".into(),
        });
    };
    let attrs = args[..separator].to_vec();
    let paths = args[separator + 1..].to_vec();
    if paths.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "check-attr requires path arguments".into(),
        });
    }
    if all {
        Ok((Vec::new(), paths))
    } else if attrs.is_empty() {
        Err(CliError::Fatal {
            code: 129,
            message: "check-attr requires attributes".into(),
        })
    } else {
        Ok((attrs, paths))
    }
}

fn print_tree(store: &LooseObjectStore, tree_id: &ObjectId) -> Result<()> {
    let tree_cache = TreeObjectCache::new(store);
    print_tree_entries(&tree_cache, tree_id, Vec::new(), false, false, false)
}

pub(crate) fn print_tree_entries(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_id: &ObjectId,
    prefix: Vec<u8>,
    recursive: bool,
    show_trees: bool,
    name_only: bool,
) -> Result<()> {
    if recursive {
        return print_tree_entries_recursive(tree_cache, tree_id, prefix, show_trees, name_only);
    }
    for entry in tree_cache.read_tree(tree_id)?.iter() {
        let path = tree_entry_path(&prefix, &entry.name);
        print_tree_entry(entry, &path, name_only)?;
    }
    Ok(())
}

fn print_tree_entries_recursive(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_id: &ObjectId,
    mut path: Vec<u8>,
    show_trees: bool,
    name_only: bool,
) -> Result<()> {
    struct PendingTreePrint {
        id: ObjectId,
        path_len: usize,
        entries: Option<Arc<[TreeEntry]>>,
        next: usize,
    }

    let initial_path_len = path.len();
    let mut pending = vec![PendingTreePrint {
        id: tree_id.clone(),
        path_len: initial_path_len,
        entries: None,
        next: 0,
    }];
    while !pending.is_empty() {
        let Some(frame) = pending.last_mut() else {
            break;
        };
        if frame.entries.is_none() {
            frame.entries = Some(tree_cache.read_tree(&frame.id)?);
            continue;
        }

        let Some((entry, child_path_len)) = (|| {
            let frame = pending.last_mut()?;
            let entries = frame
                .entries
                .as_ref()
                .expect("tree print frame entries loaded before iteration");
            if frame.next == entries.len() {
                return None;
            }
            let entry = &entries[frame.next];
            frame.next += 1;
            path.truncate(frame.path_len);
            if !path.is_empty() {
                path.push(b'/');
            }
            path.extend_from_slice(&entry.name);
            Some((entry.clone(), path.len()))
        })() else {
            let frame = pending.last().expect("pending tree print frame");
            path.truncate(frame.path_len);
            pending.pop();
            continue;
        };

        if entry.mode == TreeMode::Tree {
            if show_trees {
                print_tree_entry(&entry, &path, name_only)?;
            }
            pending.push(PendingTreePrint {
                id: entry.id,
                path_len: child_path_len,
                entries: None,
                next: 0,
            });
        } else {
            print_tree_entry(&entry, &path, name_only)?;
        }
    }
    path.truncate(initial_path_len);
    Ok(())
}

pub(crate) fn print_tree_entry(entry: &TreeEntry, path: &[u8], name_only: bool) -> Result<()> {
    if name_only {
        println!("{}", String::from_utf8_lossy(path));
        return Ok(());
    }
    println!(
        "{} {} {}\t{}",
        tree_mode_display(entry.mode),
        tree_entry_kind(entry.mode).as_str(),
        entry.id.to_hex(),
        String::from_utf8_lossy(path)
    );
    Ok(())
}

fn parse_batch_command(line: &str, buffer: bool, split_rest: bool) -> Result<BatchCommand<'_>> {
    if line.starts_with(char::is_whitespace) {
        return Err(CliError::Fatal {
            code: 128,
            message: "whitespace before command in input".into(),
        });
    }
    if line == "flush" {
        return if buffer {
            Ok(BatchCommand::Flush)
        } else {
            Err(CliError::Fatal {
                code: 128,
                message: "flush is only for --buffer mode".into(),
            })
        };
    }
    let Some((command, objectish)) = line.split_once(' ') else {
        return Err(CliError::Fatal {
            code: 128,
            message: batch_command_missing_space_error(line),
        });
    };
    match command {
        "flush" => Err(CliError::Fatal {
            code: 128,
            message: "flush takes no arguments".into(),
        }),
        "info" if !objectish.is_empty() => {
            let (objectish, rest) = if split_rest {
                split_batch_object_input(objectish)
            } else {
                (objectish, "")
            };
            Ok(BatchCommand::Object(objectish, rest, false))
        }
        "contents" if !objectish.is_empty() => {
            let (objectish, rest) = if split_rest {
                split_batch_object_input(objectish)
            } else {
                (objectish, "")
            };
            Ok(BatchCommand::Object(objectish, rest, true))
        }
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown command: '{line}'"),
        }),
    }
}

fn batch_command_missing_space_error(line: &str) -> String {
    match line {
        "" => "empty command in input".into(),
        "info" => "info requires arguments".into(),
        "contents" => "contents requires arguments".into(),
        _ => format!("unknown command: '{line}'"),
    }
}

fn split_batch_object_input(input: &str) -> (&str, &str) {
    let Some(split_at) = input.find(char::is_whitespace) else {
        return (input, "");
    };
    let objectish = &input[..split_at];
    let rest = input[split_at..].trim_start();
    (objectish, rest)
}

fn resolve_batch_objectish(
    repo: &GitRepo,
    store: &LooseObjectStore,
    objectish: &str,
    follow_symlinks: bool,
) -> io::Result<BatchLookup> {
    if !follow_symlinks {
        return resolve_objectish_with_mode(repo, objectish).map(BatchLookup::Object);
    }
    let Some((base, path)) = objectish.split_once(':') else {
        return resolve_objectish_with_mode(repo, objectish).map(BatchLookup::Object);
    };
    if base.is_empty() || path.is_empty() {
        return resolve_objectish_with_mode(repo, objectish).map(BatchLookup::Object);
    }
    let root = resolve_treeish(repo, store, base)?;
    resolve_tree_path_following_symlinks(store, &root, path, objectish)
}

fn resolve_tree_path_following_symlinks(
    store: &LooseObjectStore,
    root: &ObjectId,
    path: &str,
    original: &str,
) -> io::Result<BatchLookup> {
    let mut components = split_git_path_components(path);
    let mut seen_symlinks = HashSet::new();
    let mut followed_symlink = false;
    for _ in 0..40 {
        if components.is_empty() {
            return Ok(BatchLookup::Object(ResolvedObjectish {
                id: root.clone(),
                mode: Some(String::from_utf8_lossy(TreeMode::Tree.as_bytes()).into_owned()),
            }));
        }
        let mut current_tree = root.clone();
        let mut parent = Vec::new();
        for index in 0..components.len() {
            let component = &components[index];
            if component == "." || component.is_empty() {
                components = parent
                    .iter()
                    .cloned()
                    .chain(components[index + 1..].iter().cloned())
                    .collect();
                break;
            }
            if component == ".." {
                if parent.is_empty() {
                    if followed_symlink {
                        return Ok(BatchLookup::Special {
                            kind: "symlink",
                            payload: components[index..].join("/"),
                        });
                    }
                    return Err(io::Error::new(io::ErrorKind::NotFound, "path not found"));
                }
                parent.pop();
                components = parent
                    .iter()
                    .cloned()
                    .chain(components[index + 1..].iter().cloned())
                    .collect();
                break;
            }
            let entry = read_tree(store, &current_tree)?
                .into_iter()
                .find(|entry| entry.name.as_slice() == component.as_bytes());
            let Some(entry) = entry else {
                if followed_symlink {
                    return Ok(batch_path_status("dangling", original));
                }
                return Err(io::Error::new(io::ErrorKind::NotFound, "path not found"));
            };
            let remaining = components[index + 1..].to_vec();
            match entry.mode {
                TreeMode::Tree => {
                    current_tree = entry.id;
                    parent.push(component.clone());
                }
                TreeMode::Symlink => {
                    followed_symlink = true;
                    let target = store.read_object(&entry.id)?;
                    let target_path = String::from_utf8_lossy(&target.content).into_owned();
                    let seen_key = format!("{}:{}", parent.join("/"), component);
                    if !seen_symlinks.insert(seen_key) {
                        return Ok(batch_path_status("loop", original));
                    }
                    let SymlinkRewrite::Inside(next) =
                        rewrite_symlink_components(&parent, &target_path, &remaining);
                    components = next;
                    break;
                }
                TreeMode::File | TreeMode::Executable | TreeMode::Gitlink => {
                    if remaining.is_empty() {
                        return Ok(BatchLookup::Object(ResolvedObjectish {
                            id: entry.id,
                            mode: Some(String::from_utf8_lossy(entry.mode.as_bytes()).into_owned()),
                        }));
                    }
                    if followed_symlink {
                        return Ok(batch_path_status("notdir", original));
                    }
                    return Err(io::Error::new(io::ErrorKind::NotFound, "path not found"));
                }
            }
            if index + 1 == components.len() && entry.mode == TreeMode::Tree {
                return Ok(BatchLookup::Object(ResolvedObjectish {
                    id: current_tree,
                    mode: Some(String::from_utf8_lossy(TreeMode::Tree.as_bytes()).into_owned()),
                }));
            }
        }
    }
    Ok(batch_path_status("loop", original))
}

enum SymlinkRewrite {
    Inside(Vec<String>),
}

fn split_git_path_components(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .map(ToOwned::to_owned)
        .collect()
}

fn rewrite_symlink_components(
    parent: &[String],
    target: &str,
    remaining: &[String],
) -> SymlinkRewrite {
    let mut inside = parent.to_vec();
    inside.extend(split_symlink_target_components(target));
    inside.extend_from_slice(remaining);
    SymlinkRewrite::Inside(inside)
}

fn split_symlink_target_components(target: &str) -> Vec<String> {
    let mut components = split_git_path_components(target);
    if target.ends_with('/') {
        components.push(String::new());
    }
    components
}

fn batch_path_status(kind: &'static str, original: &str) -> BatchLookup {
    BatchLookup::Special {
        kind,
        payload: original.to_owned(),
    }
}

fn write_batch_object(
    out: &mut impl Write,
    store: &LooseObjectStore,
    display_id: &ObjectId,
    object: &LooseObject,
    include_content: bool,
    format: &BatchFormat,
    rest: &str,
    mode_atom: &str,
    output_nul: bool,
) -> Result<()> {
    let delta_base_atom = batch_delta_base_atom(store, &object.id, format)?;
    let disk_size_atom = batch_disk_size_atom(store, &object.id, format, None)?;
    write_batch_header(
        out,
        display_id,
        object.kind,
        object.content.len(),
        format,
        rest,
        mode_atom,
        &delta_base_atom,
        &disk_size_atom,
        output_nul,
    )?;
    if include_content {
        out.write_all(&object.content)?;
        write_batch_terminator(out, output_nul)?;
    }
    Ok(())
}

fn write_object_batch_header(
    out: &mut impl Write,
    store: &LooseObjectStore,
    display_id: &ObjectId,
    read_id: &ObjectId,
    format: &BatchFormat,
    rest: &str,
    mode_atom: &str,
    output_nul: bool,
) -> Result<bool> {
    let Some((kind, size)) = store.object_header_hint(read_id)? else {
        return Ok(false);
    };
    let delta_base_atom = batch_delta_base_atom(store, read_id, format)?;
    let disk_size_atom = batch_disk_size_atom(store, read_id, format, None)?;
    write_batch_header(
        out,
        display_id,
        kind,
        size,
        format,
        rest,
        mode_atom,
        &delta_base_atom,
        &disk_size_atom,
        output_nul,
    )?;
    Ok(true)
}

fn batch_delta_base_atom(
    store: &LooseObjectStore,
    id: &ObjectId,
    format: &BatchFormat,
) -> io::Result<String> {
    if !format.uses_deltabase() {
        return Ok(String::new());
    }
    Ok(store
        .delta_base_hint(id)?
        .unwrap_or_else(zero_object_id)
        .to_hex())
}

fn batch_disk_size_atom(
    store: &LooseObjectStore,
    id: &ObjectId,
    format: &BatchFormat,
    disk_size_override: Option<u64>,
) -> io::Result<String> {
    if !format.uses_disk_size() {
        return Ok(String::new());
    }
    if let Some(size) = disk_size_override {
        return Ok(size.to_string());
    }
    Ok(store
        .object_disk_size_hint(id)?
        .map(|size| size.to_string())
        .unwrap_or_else(|| "0".to_owned()))
}

fn write_batch_header(
    out: &mut impl Write,
    id: &ObjectId,
    kind: GitObjectKind,
    size: usize,
    format: &BatchFormat,
    rest: &str,
    mode_atom: &str,
    delta_base_atom: &str,
    disk_size_atom: &str,
    output_nul: bool,
) -> io::Result<()> {
    match format {
        BatchFormat::Default => {
            write!(out, "{} {} {}", id.to_hex(), kind.as_str(), size)?;
            write_batch_terminator(out, output_nul)?;
        }
        BatchFormat::Custom(format) => {
            write!(
                out,
                "{}",
                render_batch_format(
                    format,
                    id,
                    kind,
                    size,
                    rest,
                    mode_atom,
                    delta_base_atom,
                    disk_size_atom,
                )
            )?;
            write_batch_terminator(out, output_nul)?;
        }
    }
    Ok(())
}

fn write_batch_missing(out: &mut impl Write, objectish: &str, output_nul: bool) -> io::Result<()> {
    write!(out, "{objectish} missing")?;
    write_batch_terminator(out, output_nul)
}

fn write_batch_special(
    out: &mut impl Write,
    kind: &str,
    payload: &str,
    output_nul: bool,
) -> io::Result<()> {
    write!(out, "{kind} {}", payload.len())?;
    write_batch_terminator(out, output_nul)?;
    out.write_all(payload.as_bytes())?;
    write_batch_terminator(out, output_nul)
}

fn write_batch_submodule(out: &mut impl Write, id: &ObjectId, output_nul: bool) -> io::Result<()> {
    write!(out, "{} submodule", id.to_hex())?;
    write_batch_terminator(out, output_nul)
}

fn write_batch_excluded(out: &mut impl Write, id: &ObjectId, output_nul: bool) -> io::Result<()> {
    write!(out, "{} excluded", id.to_hex())?;
    write_batch_terminator(out, output_nul)
}

fn write_batch_terminator(out: &mut impl Write, output_nul: bool) -> io::Result<()> {
    out.write_all(if output_nul { b"\0" } else { b"\n" })
}

fn validate_batch_format(format: &str) -> Result<()> {
    let mut rest = format;
    while let Some(start) = rest.find("%(") {
        let atom_start = start + 2;
        let Some(end) = rest[atom_start..].find(')') else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("bad cat-file batch format: {format}"),
            });
        };
        let atom_end = atom_start + end;
        let atom = &rest[atom_start..atom_end];
        match atom {
            "objectname" | "objecttype" | "objectsize" | "objectsize:disk" | "objectmode"
            | "rest" | "deltabase" => {}
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("bad cat-file format: {format}"),
                });
            }
        }
        rest = &rest[atom_end + 1..];
    }
    Ok(())
}

fn render_batch_format(
    format: &str,
    id: &ObjectId,
    kind: GitObjectKind,
    size: usize,
    rest_atom: &str,
    mode_atom: &str,
    delta_base_atom: &str,
    disk_size_atom: &str,
) -> String {
    let mut rendered = String::with_capacity(format.len() + 16);
    let mut rest = format;
    while let Some(start) = rest.find("%(") {
        rendered.push_str(&rest[..start]);
        let atom_start = start + 2;
        let end = rest[atom_start..]
            .find(')')
            .expect("batch format validated before rendering");
        let atom_end = atom_start + end;
        let atom = &rest[atom_start..atom_end];
        match atom {
            "objectname" => rendered.push_str(&id.to_hex()),
            "objecttype" => rendered.push_str(kind.as_str()),
            "objectsize" => rendered.push_str(&size.to_string()),
            "objectsize:disk" => rendered.push_str(disk_size_atom),
            "objectmode" => rendered.push_str(mode_atom),
            "rest" => rendered.push_str(rest_atom),
            "deltabase" => rendered.push_str(delta_base_atom),
            _ => unreachable!("batch format atom validated before rendering"),
        }
        rest = &rest[atom_end + 1..];
    }
    rendered.push_str(rest);
    rendered
}

fn write_or_hash(
    store: Option<&LooseObjectStore>,
    kind: GitObjectKind,
    content: &[u8],
) -> Result<ObjectId> {
    match store {
        Some(store) => Ok(store.write_object(kind, content)?),
        None => Ok(hash_object(GitHashAlgorithm::Sha1, kind, content)),
    }
}

fn write_or_hash_stdin(store: Option<&LooseObjectStore>, kind: GitObjectKind) -> Result<ObjectId> {
    if kind != GitObjectKind::Blob {
        let mut content = Vec::new();
        io::stdin().read_to_end(&mut content)?;
        return write_or_hash(store, kind, &content);
    }

    let temp_path = unique_temp_sibling(&std::env::temp_dir().join("zmin-hash-object-stdin"));
    let result = (|| {
        let temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        let mut temp_file =
            io::BufWriter::with_capacity(HASH_OBJECT_STREAM_BUF_CAPACITY, temp_file);
        let mut stdin = io::BufReader::with_capacity(HASH_OBJECT_STREAM_BUF_CAPACITY, io::stdin());
        io::copy(&mut stdin, &mut temp_file)?;
        temp_file.flush()?;
        write_or_hash_path(store, None, kind, &temp_path)
    })();
    let _ = fs::remove_file(&temp_path);
    result
}

fn write_or_hash_path(
    store: Option<&LooseObjectStore>,
    repo: Option<&GitRepo>,
    kind: GitObjectKind,
    path: &Path,
) -> Result<ObjectId> {
    if is_git_null_path(path) {
        return write_or_hash(store, kind, &[]);
    }
    if kind != GitObjectKind::Blob {
        let content = fs::read(path)?;
        return write_or_hash(store, kind, &content);
    }

    if let Some(repo) = repo {
        let absolute = absolute_path_from_arg(path)?;
        if let Ok(relative) = repo_relative_path(&repo.root, &absolute) {
            let content = clean_worktree_content(repo, &relative, fs::read(&absolute)?)?;
            return write_or_hash(store, kind, &content);
        }
    }

    let file = fs::File::open(path)?;
    let size = usize::try_from(file.metadata()?.len()).map_err(|_| {
        CliError::Message("`hash-object` input is too large for this platform".into())
    })?;
    let mut reader = io::BufReader::with_capacity(HASH_OBJECT_STREAM_BUF_CAPACITY, file);
    match store {
        Some(store) => Ok(store.write_streamed_blob_content(size, |writer| {
            io::copy(&mut reader, writer).map(|_| ())
        })?),
        None => {
            let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
            hasher.update_object_header(kind, size);
            let mut buffer = vec![0_u8; HASH_OBJECT_STREAM_BUF_CAPACITY];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(hasher.finalize())
        }
    }
}

fn is_git_null_path(path: &Path) -> bool {
    if path == Path::new("/dev/null") {
        return true;
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("nul"))
}

pub(crate) fn init_command(
    directory: Option<PathBuf>,
    bare: bool,
    template: Option<PathBuf>,
    separate_git_dir: Option<PathBuf>,
    shared: Option<String>,
    initial_branch: Option<String>,
    object_format: Option<String>,
    ref_format: Option<String>,
    quiet: bool,
) -> Result<()> {
    let bare = bare || global_bare_option();
    let cwd = std::env::current_dir()?;
    let env_git_dir = env_path("GIT_DIR", &cwd);
    let env_work_tree = env_path("GIT_WORK_TREE", &cwd);
    if separate_git_dir.is_some() && bare {
        return Err(CliError::Fatal {
            code: 128,
            message: "--separate-git-dir and --bare cannot be used together".into(),
        });
    }
    if separate_git_dir.is_some() && env_git_dir.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "--separate-git-dir is incompatible with GIT_DIR".into(),
        });
    }
    if env_work_tree.is_some() && (bare || directory.is_some()) {
        return Err(CliError::Fatal {
            code: 128,
            message: "GIT_WORK_TREE cannot be used with init arguments".into(),
        });
    }
    let explicit_initial_branch = initial_branch.clone();
    let explicit_ref_format = ref_format.is_some();
    let initial_branch = resolve_initial_branch(initial_branch)?;
    let object_format = resolve_init_object_format(object_format)?;
    let ref_format = resolve_init_ref_format(ref_format)?;

    if directory.is_none()
        && let Some(git_dir) = env_git_dir
    {
        let env_git_dir_is_dot_git = git_dir.file_name().is_some_and(|name| name == ".git");
        if env_git_dir_is_dot_git && !bare && env_work_tree.is_none() {
            let work_tree = git_dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| cwd.clone());
            let reinit = is_git_dir(&git_dir);
            let previous_head = reinit
                .then(|| std::fs::read_to_string(git_dir.join("HEAD")))
                .transpose()?;
            let result = init_repository(
                &work_tree,
                InitRepositoryOptions {
                    bare: false,
                    initial_branch,
                },
            )?;
            apply_init_repository_format(&result.git_dir, &object_format, &ref_format, reinit)?;
            apply_init_template(&result.git_dir, template.as_ref())?;
            apply_shared_repository(&result.git_dir, shared.as_deref())?;
            restore_reinit_head(
                &result.git_dir,
                previous_head,
                explicit_initial_branch.as_deref(),
            )?;
            hide_dotgit_if_needed(&result.git_dir, false)?;
            print_init_message_if_needed(quiet, reinit, &result.git_dir);
            return Ok(());
        }

        let reinit = is_git_dir(&git_dir);
        let previous_head = reinit
            .then(|| std::fs::read_to_string(git_dir.join("HEAD")))
            .transpose()?;
        let result = init_repository(
            &git_dir,
            InitRepositoryOptions {
                bare: true,
                initial_branch,
            },
        )?;
        apply_init_repository_format(&result.git_dir, &object_format, &ref_format, reinit)?;
        apply_init_template(&result.git_dir, template.as_ref())?;
        apply_shared_repository(&result.git_dir, shared.as_deref())?;
        if let Some(work_tree) = env_work_tree {
            write_non_bare_git_dir_config(&result.git_dir, &work_tree)?;
            apply_shared_repository(&result.git_dir, shared.as_deref())?;
        }
        restore_reinit_head(
            &result.git_dir,
            previous_head,
            explicit_initial_branch.as_deref(),
        )?;
        print_init_message_if_needed(quiet, reinit, &result.git_dir);
        return Ok(());
    }

    let directory = match directory {
        Some(path) if path.is_absolute() => path,
        Some(path) => cwd.join(path),
        None => cwd,
    };
    if let Some(separate_git_dir) = separate_git_dir {
        let git_dir = absolute_path_from_arg(&separate_git_dir)?;
        if reinit_separate_git_dir_if_needed(&directory, &git_dir)? {
            apply_init_repository_format(&git_dir, &object_format, &ref_format, true)?;
            apply_init_template(&git_dir, template.as_ref())?;
            apply_shared_repository(&git_dir, shared.as_deref())?;
            print_init_message_if_needed(quiet, true, &git_dir);
            return Ok(());
        }
        let reinit = is_git_dir(&git_dir);
        let previous_head = reinit
            .then(|| std::fs::read_to_string(git_dir.join("HEAD")))
            .transpose()?;
        let result = init_repository(
            &git_dir,
            InitRepositoryOptions {
                bare: true,
                initial_branch,
            },
        )?;
        apply_init_repository_format(&result.git_dir, &object_format, &ref_format, reinit)?;
        write_non_bare_git_dir_config(&result.git_dir, &directory)?;
        apply_init_template(&result.git_dir, template.as_ref())?;
        apply_shared_repository(&result.git_dir, shared.as_deref())?;
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            directory.join(".git"),
            format!("gitdir: {}\n", git_path_config_output(&result.git_dir)),
        )?;
        hide_dotgit_if_needed(&directory.join(".git"), false)?;
        restore_reinit_head(
            &result.git_dir,
            previous_head,
            explicit_initial_branch.as_deref(),
        )?;
        print_init_message_if_needed(quiet, reinit, &result.git_dir);
        return Ok(());
    }
    let expected_git_dir = if bare {
        directory.clone()
    } else {
        directory.join(".git")
    };
    let shared_parent_dirs = msys_shared_parent_dirs(&expected_git_dir, shared.as_deref());
    if !bare && expected_git_dir.exists() && !expected_git_dir.is_dir() {
        let git_dir = current_worktree_git_dir(&expected_git_dir)?;
        let reinit_git_dir = current_common_git_dir(&git_dir)?;
        if explicit_ref_format {
            reject_reinit_ref_format_change(&reinit_git_dir, &ref_format)?;
        }
        validate_reinit_config(&directory, &git_dir)?;
        if template.is_some() {
            apply_init_template(&reinit_git_dir, template.as_ref())?;
        } else {
            restore_default_exclude(&reinit_git_dir)?;
        }
        print_init_message_if_needed(quiet, true, &reinit_git_dir);
        return Ok(());
    }
    let reinit = is_git_dir(&expected_git_dir);
    let previous_head = reinit
        .then(|| std::fs::read_to_string(expected_git_dir.join("HEAD")))
        .transpose()?;
    if reinit && explicit_ref_format {
        reject_reinit_ref_format_change(&expected_git_dir, &ref_format)?;
    }
    if reinit {
        validate_reinit_config(&directory, &expected_git_dir)?;
        apply_init_repository_format(&expected_git_dir, &object_format, &ref_format, true)?;
        apply_init_template(&expected_git_dir, template.as_ref())?;
        apply_shared_repository(&expected_git_dir, shared.as_deref())?;
        restore_reinit_head(
            &expected_git_dir,
            previous_head,
            explicit_initial_branch.as_deref(),
        )?;
        print_init_message_if_needed(quiet, true, &expected_git_dir);
        return Ok(());
    }
    let result = init_repository(
        directory,
        InitRepositoryOptions {
            bare,
            initial_branch,
        },
    )?;
    apply_init_repository_format(&result.git_dir, &object_format, &ref_format, reinit)?;
    apply_init_template(&result.git_dir, template.as_ref())?;
    apply_shared_repository(&result.git_dir, shared.as_deref())?;
    apply_msys_shared_parent_permissions(&shared_parent_dirs)?;
    hide_dotgit_if_needed(&result.git_dir, bare)?;
    restore_reinit_head(
        &result.git_dir,
        previous_head,
        explicit_initial_branch.as_deref(),
    )?;
    print_init_message_if_needed(quiet, reinit, &result.git_dir);
    Ok(())
}

fn validate_reinit_config(work_tree: &Path, git_dir: &Path) -> Result<()> {
    let common_dir = current_common_git_dir(git_dir)?;
    let repo = GitRepo {
        root: work_tree.to_path_buf(),
        git_dir: git_dir.to_path_buf(),
        objects_dir: common_dir.join("objects"),
        index_path: git_dir.join("index"),
    };
    read_config_entries(&repo)?;
    Ok(())
}

fn reinit_separate_git_dir_if_needed(work_tree: &Path, new_git_dir: &Path) -> Result<bool> {
    let git_path = work_tree.join(".git");
    if !git_path.exists() {
        return Ok(false);
    }
    let current_git_dir = current_worktree_git_dir(&git_path)?;
    let current_common_dir = canonical_or_absolute(current_common_git_dir(&current_git_dir)?);
    if current_git_dir.join("commondir").is_file() && is_bare_git_dir(&current_common_dir) {
        return Err(CliError::Fatal {
            code: 128,
            message: "--separate-git-dir is incompatible with linked worktrees".into(),
        });
    }
    if current_git_dir.join("commondir").is_file() {
        return Ok(true);
    }
    if canonical_or_absolute(current_common_dir.clone())
        != canonical_or_absolute(new_git_dir.to_path_buf())
    {
        if let Some(parent) = new_git_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::rename(&current_common_dir, new_git_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    if current_git_dir.join("commondir").is_file() {
        write_gitdir_pointer(&current_common_dir, new_git_dir)?;
    } else {
        write_gitdir_pointer(&git_path, new_git_dir)?;
    }
    rewrite_linked_worktree_gitfiles(new_git_dir)?;
    Ok(true)
}

fn current_worktree_git_dir(git_path: &Path) -> Result<PathBuf> {
    if git_path.is_file() {
        return read_gitdir_file(git_path);
    }
    if std::fs::symlink_metadata(git_path)?
        .file_type()
        .is_symlink()
    {
        let target = std::fs::read_link(git_path)?;
        let git_dir = if target.is_absolute() {
            target
        } else {
            git_path
                .parent()
                .map(|parent| parent.join(&target))
                .unwrap_or(target)
        };
        return Ok(git_dir);
    }
    Ok(git_path.to_path_buf())
}

fn current_common_git_dir(git_dir: &Path) -> Result<PathBuf> {
    match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(raw) => {
            let path = PathBuf::from(raw.trim());
            if path.is_absolute() {
                Ok(path)
            } else {
                Ok(git_dir.join(path))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(git_dir.to_path_buf()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn write_gitdir_pointer(git_path: &Path, git_dir: &Path) -> Result<()> {
    if git_path.is_dir()
        && !std::fs::symlink_metadata(git_path)?
            .file_type()
            .is_symlink()
    {
        std::fs::remove_dir_all(git_path)?;
    }
    let git_dir = canonical_or_absolute(git_dir.to_path_buf());
    std::fs::write(
        git_path,
        format!("gitdir: {}\n", git_path_config_output(&git_dir)),
    )?;
    Ok(())
}

fn rewrite_linked_worktree_gitfiles(common_git_dir: &Path) -> Result<()> {
    let worktrees = common_git_dir.join("worktrees");
    let entries = match std::fs::read_dir(worktrees) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(CliError::Io(error)),
    };
    for entry in entries {
        let admin_dir = entry?.path();
        let git_file = match std::fs::read_to_string(admin_dir.join("gitdir")) {
            Ok(raw) => PathBuf::from(raw.trim()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(CliError::Io(error)),
        };
        write_gitdir_pointer(&git_file, &admin_dir)?;
    }
    Ok(())
}

fn resolve_init_object_format(explicit: Option<String>) -> Result<String> {
    if let Some(value) = explicit {
        return validate_init_object_format(&value).map(|_| value);
    }
    if let Ok(value) = std::env::var("GIT_DEFAULT_HASH")
        && !value.is_empty()
        && Some(value.as_str()) != std::env::var("GIT_TEST_DEFAULT_HASH").ok().as_deref()
    {
        return validate_init_object_format(&value).map(|_| value);
    }
    if let Some(value) = global_config_value("init", "defaultobjectformat")? {
        if validate_init_object_format(&value).is_ok() {
            return Ok(value);
        }
        eprintln!("warning: unknown hash algorithm '{value}'");
    }
    Ok("sha1".to_owned())
}

fn validate_init_object_format(value: &str) -> Result<()> {
    match value {
        "sha1" | "sha256" => Ok(()),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown hash algorithm '{value}'"),
        }),
    }
}

fn resolve_init_ref_format(explicit: Option<String>) -> Result<String> {
    if let Some(value) = explicit {
        return validate_init_ref_format(&value).map(|_| value);
    }
    let feature_experimental =
        global_config_value("feature", "experimental")?.as_deref() == Some("true");
    if let Ok(value) = std::env::var("GIT_DEFAULT_REF_FORMAT")
        && !value.is_empty()
        && (!is_inherited_default_ref_format(&value) || feature_experimental)
    {
        return validate_init_ref_format(&value).map(|_| value);
    }
    if let Some(value) = global_config_value("init", "defaultrefformat")? {
        if validate_init_ref_format(&value).is_ok() {
            return Ok(value);
        }
        eprintln!("warning: unknown ref storage format '{value}'");
    }
    if feature_experimental {
        return Ok("reftable".to_owned());
    }
    Ok("files".to_owned())
}

fn is_inherited_default_ref_format(value: &str) -> bool {
    match std::env::var("GIT_TEST_DEFAULT_REF_FORMAT") {
        Ok(default) => value == default,
        Err(_) => value == "files",
    }
}

fn validate_init_ref_format(value: &str) -> Result<()> {
    match value {
        "files" | "reftable" => Ok(()),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown ref storage format '{value}'"),
        }),
    }
}

fn apply_init_repository_format(
    git_dir: &Path,
    object_format: &str,
    ref_format: &str,
    reinit: bool,
) -> Result<()> {
    if reinit {
        return Ok(());
    }
    if object_format == "sha256" || ref_format == "reftable" {
        set_config_value_in_file(&git_dir.join("config"), "core.repositoryformatversion", "1")?;
    }
    if object_format == "sha256" {
        set_config_value_in_file(&git_dir.join("config"), "extensions.objectFormat", "sha256")?;
    }
    if ref_format == "reftable" {
        set_config_value_in_file(&git_dir.join("config"), "extensions.refStorage", "reftable")?;
    }
    Ok(())
}

fn reject_reinit_ref_format_change(git_dir: &Path, requested: &str) -> Result<()> {
    let current = read_config_value_from_file(&git_dir.join("config"), "extensions", "refstorage")?
        .unwrap_or_else(|| "files".to_owned());
    if current != requested {
        return Err(CliError::Fatal {
            code: 128,
            message: "attempt to reinitialize repository with different reference storage format"
                .into(),
        });
    }
    Ok(())
}

fn read_config_value_from_file(path: &Path, section: &str, key: &str) -> Result<Option<String>> {
    Ok(read_config_file(path)?
        .into_iter()
        .rev()
        .find(|entry| entry.section == section && entry.subsection.is_empty() && entry.key == key)
        .map(|entry| entry.value))
}

fn resolve_initial_branch(explicit: Option<String>) -> Result<String> {
    if let Some(branch) = explicit {
        validate_init_branch_name(&branch)?;
        return Ok(branch);
    }
    let default_initial_branch_env_present =
        std::env::var_os("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME").is_some();
    if let Some(branch) = std::env::var_os("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME")
        .and_then(|value| (!value.is_empty()).then(|| value.to_string_lossy().into_owned()))
    {
        validate_init_branch_name(&branch)?;
        return Ok(branch);
    }
    if let Some(branch) = global_config_value("init", "defaultbranch")? {
        validate_init_branch_name(&branch)?;
        return Ok(branch);
    }
    if default_initial_branch_env_present
        && global_config_value("advice", "defaultbranchname")?.as_deref() != Some("false")
    {
        eprintln!("\x1b[33mhint: \x1b[mUsing 'master' as the name for the initial branch.");
    }
    Ok("master".to_owned())
}

fn validate_init_branch_name(branch: &str) -> Result<()> {
    branch_ref_name(branch)
        .map(|_| ())
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("invalid branch name: {branch}"),
        })
}

fn restore_reinit_head(
    git_dir: &Path,
    previous_head: Option<String>,
    ignored_initial_branch: Option<&str>,
) -> Result<()> {
    let Some(previous_head) = previous_head else {
        return Ok(());
    };
    std::fs::write(git_dir.join("HEAD"), previous_head)?;
    if let Some(branch) = ignored_initial_branch {
        eprintln!("warning: re-init: ignored --initial-branch={branch}");
    }
    Ok(())
}

fn env_path(name: &str, cwd: &Path) -> Option<PathBuf> {
    let path = normalize_windows_input_path(PathBuf::from(std::env::var_os(name)?));
    if path.is_absolute() {
        Some(path)
    } else {
        Some(cwd.join(path))
    }
}

fn print_init_message(reinit: bool, git_dir: &Path) {
    let action = if reinit {
        "Reinitialized existing"
    } else {
        "Initialized empty"
    };
    println!("{action} Git repository in {}/", git_dir.display());
}

fn print_init_message_if_needed(quiet: bool, reinit: bool, git_dir: &Path) {
    if !quiet {
        print_init_message(reinit, git_dir);
    }
}

fn write_non_bare_git_dir_config(git_dir: &Path, work_tree: &Path) -> Result<()> {
    let filemode = if cfg!(unix) { "true" } else { "false" };
    std::fs::write(
        git_dir.join("config"),
        format!(
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = {filemode}\n\tbare = false\n\tlogallrefupdates = true\n\tworktree = {}\n",
            git_path_config_output(work_tree),
        ),
    )?;
    Ok(())
}

fn apply_init_template(git_dir: &Path, explicit_template: Option<&PathBuf>) -> Result<()> {
    if let Some(template) = explicit_template {
        if template.as_os_str().is_empty() || template.as_os_str() == EMPTY_INIT_TEMPLATE_SENTINEL {
            remove_default_template_files(git_dir)?;
            return Ok(());
        }
        copy_template_dir(template, git_dir)?;
        return Ok(());
    }
    if let Some(template) = global_config_value("init", "templatedir")? {
        copy_template_dir(&expand_home_path(&template), git_dir)?;
    }
    Ok(())
}

fn remove_default_template_files(git_dir: &Path) -> Result<()> {
    match std::fs::remove_file(git_dir.join("info/exclude")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }?;
    match std::fs::remove_dir(git_dir.join("info")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn restore_default_exclude(git_dir: &Path) -> Result<()> {
    let info = git_dir.join("info");
    std::fs::create_dir_all(&info)?;
    let exclude = info.join("exclude");
    if !exclude.exists() {
        std::fs::write(
            exclude,
            "# git ls-files --others --exclude-from=.git/info/exclude\n\
             # Lines that start with '#' are comments.\n\
             # For a project mostly in C, the following would be a good set of\n\
             # exclude patterns (uncomment them if you want to use them):\n\
             # *.[oa]\n\
             # *~\n",
        )?;
    }
    Ok(())
}

fn copy_template_dir(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            std::fs::create_dir_all(&destination_path)?;
            copy_template_dir(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            if let Some(parent) = destination_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn apply_shared_repository(git_dir: &Path, explicit_shared: Option<&str>) -> Result<()> {
    let value = explicit_shared
        .map(str::to_owned)
        .or(global_config_value("core", "sharedrepository")?);
    if let Some(value) = value {
        set_config_value_in_file(&git_dir.join("config"), "core.sharedRepository", &value)?;
        apply_shared_repository_permissions(git_dir, &value)?;
    }
    Ok(())
}

fn hide_dotgit_if_needed(git_path: &Path, bare: bool) -> Result<()> {
    if bare || !cfg!(windows) {
        return Ok(());
    }
    if global_config_value("core", "hidedotfiles")?.as_deref() == Some("false") {
        return Ok(());
    }
    let status = std::process::Command::new("attrib")
        .arg("+h")
        .arg(git_path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: status.code().unwrap_or(1),
            message: format!("failed to hide {}", git_path.display()),
        })
    }
}

#[cfg(unix)]
fn apply_shared_repository_permissions(git_dir: &Path, value: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if value == "0660" || value == "660" {
        std::fs::set_permissions(git_dir, std::fs::Permissions::from_mode(0o2770))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_shared_repository_permissions(_git_dir: &Path, _value: &str) -> Result<()> {
    if cfg!(windows) && (_value == "0660" || _value == "660") {
        run_msys_chmod("2770", _git_dir)?;
    }
    Ok(())
}

fn msys_shared_parent_dirs(git_dir: &Path, explicit_shared: Option<&str>) -> Vec<PathBuf> {
    if !cfg!(windows) || !matches!(explicit_shared, Some("0660" | "660")) {
        return Vec::new();
    }
    let mut dirs = Vec::new();
    let mut cursor = git_dir.parent();
    while let Some(path) = cursor {
        if path.exists() {
            break;
        }
        dirs.push(path.to_path_buf());
        cursor = path.parent();
    }
    dirs.reverse();
    dirs
}

fn apply_msys_shared_parent_permissions(paths: &[PathBuf]) -> Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    for path in paths {
        run_msys_chmod("775", path)?;
    }
    Ok(())
}

fn run_msys_chmod(mode: &str, path: &Path) -> Result<()> {
    let status = std::process::Command::new("chmod")
        .arg(mode)
        .arg(git_path_config_output(path))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: status.code().unwrap_or(1),
            message: format!("chmod {mode} failed for {}", git_path_config_output(path)),
        })
    }
}

fn global_config_value(section: &str, key: &str) -> Result<Option<String>> {
    if let Some(value) = global_command_config_value(section, key) {
        return Ok(Some(value));
    }
    for home in global_config_homes().into_iter().rev() {
        for path in [
            xdg_config_home(&home).join("git/config"),
            home.join(".gitconfig"),
        ] {
            if let Some(value) = read_config_file(&path)?
                .into_iter()
                .rev()
                .find(|entry| {
                    entry.section == section && entry.subsection.is_empty() && entry.key == key
                })
                .map(|entry| entry.value)
            {
                return Ok(Some(value));
            }
        }
    }
    Ok(None)
}

fn expand_home_path(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = global_config_homes().into_iter().next() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}
