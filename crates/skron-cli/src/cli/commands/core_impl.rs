use std::io::{BufRead, BufWriter, Write};
use std::sync::Arc;

use super::*;

const UNPACK_OBJECTS_STDIN_BUF_CAPACITY: usize = 256 * 1024;
const HASH_OBJECT_STREAM_BUF_CAPACITY: usize = 256 * 1024;
const CAT_FILE_BATCH_OUTPUT_BUF_CAPACITY: usize = 256 * 1024;

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
    let store = if write {
        let repo = find_repo()?;
        Some(LooseObjectStore::new(
            repo.objects_dir,
            GitHashAlgorithm::Sha1,
        ))
    } else {
        None
    };

    if stdin {
        println!("{}", write_or_hash_stdin(store.as_ref(), kind)?.to_hex());
    }

    for path in paths {
        println!(
            "{}",
            write_or_hash_path(store.as_ref(), kind, &path)?.to_hex()
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
    batch_check: bool,
    batch: bool,
    batch_command: bool,
    batch_all_objects: bool,
    buffer: bool,
    unordered: bool,
    no_unordered: bool,
    objects: Vec<String>,
) -> Result<()> {
    let selected = [
        type_only,
        pretty,
        size,
        exists,
        batch_check,
        batch,
        batch_command,
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selected == 0 {
        return Err(CliError::Stderr {
            code: 129,
            text: cat_file_usage(),
        });
    }
    if selected != 1 {
        return Err(CliError::Message(
            "`cat-file` requires exactly one mode".into(),
        ));
    }
    if batch_all_objects && !(batch_check || batch || batch_command) {
        return Err(CliError::Fatal {
            code: 129,
            message: "'--batch-all-objects' requires a batch mode".into(),
        });
    }
    let repo = find_repo()?;
    if batch_check || batch || batch_command {
        if !objects.is_empty() {
            return Err(CliError::Message(
                "`cat-file` batch modes do not take an object argument".into(),
            ));
        }
        let mode = match (batch_check, batch, batch_command) {
            (true, false, false) => BatchMode::Check,
            (false, true, false) => BatchMode::Contents,
            (false, false, true) => BatchMode::Command,
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
            buffer,
            batch_all_objects,
            unordered && !no_unordered,
        );
    }
    if objects.is_empty() {
        return Err(CliError::Stderr {
            code: 129,
            text: cat_file_usage_error(&cat_file_required_object_message(
                type_only, pretty, size, exists,
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
    let store = LooseObjectStore::new(repo.objects_dir, GitHashAlgorithm::Sha1);
    if exists {
        return if store.contains_object(&id)? {
            Ok(())
        } else {
            Err(CliError::Exit(1))
        };
    }
    if type_only {
        if let Some((kind, _)) = store.object_header_hint(&id)? {
            println!("{}", kind.as_str());
            return Ok(());
        }
        if let Some(kind) = store.object_kind_hint(&id)? {
            println!("{}", kind.as_str());
            return Ok(());
        }
    }
    if size {
        if let Some((_, object_size)) = store.object_header_hint(&id)? {
            println!("{object_size}");
            return Ok(());
        }
    }
    let object = match store.read_object(&id) {
        Ok(object) => object,
        Err(error) => return Err(CliError::Io(error)),
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

fn cat_file_required_object_message(
    type_only: bool,
    pretty: bool,
    size: bool,
    exists: bool,
) -> String {
    let mode = if type_only {
        "-t"
    } else if pretty {
        "-p"
    } else if size {
        "-s"
    } else if exists {
        "-e"
    } else {
        "<type>"
    };
    format!("<object> required with '{mode}'")
}

fn cat_file_usage_error(message: &str) -> String {
    format!("fatal: {message}\n\n{}", cat_file_usage())
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

fn cat_file_batch(
    repo: &GitRepo,
    mode: BatchMode,
    buffer: bool,
    batch_all_objects: bool,
    unordered: bool,
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
            buffer,
            batch_all_objects,
            unordered,
        )?;
        stdout.flush()?;
        return Ok(());
    }

    let mut stdout = stdout;
    cat_file_batch_with_writer(
        repo,
        &store,
        &mut stdout,
        mode,
        buffer,
        batch_all_objects,
        unordered,
    )
}

fn cat_file_batch_with_writer<W: io::Write>(
    repo: &GitRepo,
    store: &LooseObjectStore,
    mut stdout: &mut W,
    mode: BatchMode,
    buffer: bool,
    batch_all_objects: bool,
    unordered: bool,
) -> Result<()> {
    if batch_all_objects {
        if unordered {
            let mut write_object = |id: &ObjectId| -> io::Result<()> {
                write_batch_all_object(stdout, store, id, mode)
            };
            store.for_each_object_id(&mut write_object)?;
            return Ok(());
        }
        for id in store.object_ids()? {
            write_batch_all_object(stdout, store, &id, mode)?;
        }
        return Ok(());
    }

    let stdin = io::stdin();
    let mut stdin = io::BufReader::new(stdin.lock());
    let mut line = String::new();
    loop {
        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let command = if mode == BatchMode::Command {
            parse_batch_command(line, buffer)?
        } else {
            BatchCommand::Object(line, mode == BatchMode::Contents)
        };
        let BatchCommand::Object(objectish, include_content) = command else {
            stdout.flush()?;
            continue;
        };
        let Ok(id) = resolve_objectish(repo, objectish) else {
            writeln!(stdout, "{objectish} missing")?;
            continue;
        };
        if !include_content && write_object_batch_header(&mut stdout, store, &id)? {
            continue;
        }
        match store.read_object(&id) {
            Ok(object) => write_batch_object(&mut stdout, &object, include_content)?,
            Err(_) => writeln!(stdout, "{objectish} missing")?,
        }
    }
    Ok(())
}

fn write_batch_all_object<W: io::Write>(
    writer: &mut W,
    store: &LooseObjectStore,
    id: &ObjectId,
    mode: BatchMode,
) -> io::Result<()> {
    if mode == BatchMode::Check
        && let Some((kind, size)) = store.object_header_hint(id)?
    {
        writeln!(writer, "{} {} {}", id.to_hex(), kind.as_str(), size)?;
        return Ok(());
    }
    let object = store.read_object(id)?;
    writeln!(
        writer,
        "{} {} {}",
        object.id.to_hex(),
        object.kind.as_str(),
        object.content.len()
    )?;
    if mode == BatchMode::Contents {
        writer.write_all(&object.content)?;
        writer.write_all(b"\n")?;
    }
    Ok(())
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
    })?;
    Ok(())
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
    pub(crate) garbage: u64,
    pub(crate) garbage_size_kib: u64,
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
            } else {
                stats.garbage += 1;
                stats.garbage_size_kib += file_allocated_kib(&metadata);
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

fn count_objects_size_field(size_kib: u64, human_readable: bool) -> String {
    if human_readable {
        human_size(size_kib.saturating_mul(1024))
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
    ceil_div(metadata.len(), 1024)
}

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
    let repo = find_repo()?;

    if !verbose {
        let loose = collect_loose_object_stats(&repo.objects_dir, GitHashAlgorithm::Sha1, false)?;
        let size = if human_readable {
            human_size(loose.size_kib.saturating_mul(1024))
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
        count_objects_size_field(loose.size_kib, human_readable)
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
        count_objects_size_field(loose.garbage_size_kib, human_readable)
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
    stdin: bool,
    no_index: bool,
    paths: Vec<PathBuf>,
) -> Result<()> {
    if quiet && verbose {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot have both --quiet and --verbose".into(),
        });
    }
    if stdin && !paths.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "check-ignore paths cannot be combined with --stdin".into(),
        });
    }

    if quiet && !stdin && paths.len() != 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: "--quiet is only valid with a single pathname".into(),
        });
    }

    let repo = find_repo()?;
    let ignore = GitIgnore::load_from_root(&repo.root)?;
    let index = if no_index {
        GitIndex::new()
    } else {
        read_repo_index(&repo)?
    };
    let mut matched = false;

    if stdin {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::new(stdin.lock());
        let mut line = String::new();
        let mut seen_inputs = 0usize;
        loop {
            line.clear();
            if stdin.read_line(&mut line)? == 0 {
                break;
            }
            seen_inputs += 1;
            if quiet && seen_inputs > 1 {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "--quiet is only valid with a single pathname".into(),
                });
            }
            let path = PathBuf::from(line.trim_end_matches(['\r', '\n']));
            matched |= check_ignore_path(&repo, &ignore, &index, no_index, quiet, verbose, &path)?;
        }
        if seen_inputs == 0 {
            return Err(CliError::Fatal {
                code: 129,
                message: "check-ignore requires pathnames or --stdin".into(),
            });
        }
    } else {
        if paths.is_empty() {
            return Err(CliError::Fatal {
                code: 129,
                message: "check-ignore requires pathnames or --stdin".into(),
            });
        }
        for path in &paths {
            matched |= check_ignore_path(&repo, &ignore, &index, no_index, quiet, verbose, path)?;
        }
    }

    if matched {
        Ok(())
    } else {
        Err(CliError::Exit(1))
    }
}

fn check_ignore_path(
    repo: &GitRepo,
    ignore: &GitIgnore,
    index: &GitIndex,
    no_index: bool,
    quiet: bool,
    verbose: bool,
    path: &Path,
) -> Result<bool> {
    let relative = path_arg_to_repo_relative(repo, path)?;
    if !no_index && find_index_entry(index, &relative).is_some() {
        return Ok(false);
    }
    let absolute = repo.root.join(String::from_utf8_lossy(&relative).as_ref());
    let is_dir = absolute.is_dir();
    let Some(ignore_match) = ignore.match_path(&relative, is_dir) else {
        return Ok(false);
    };
    if !quiet {
        let display_path = path.to_string_lossy();
        if verbose {
            println!(
                ".gitignore:{}:{}\t{}",
                ignore_match.line_number, ignore_match.pattern, display_path
            );
        } else {
            println!("{display_path}");
        }
    }
    Ok(true)
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
    print_tree_entries(&tree_cache, tree_id, Vec::new(), false, false)
}

pub(crate) fn print_tree_entries(
    tree_cache: &TreeObjectCache<'_, LooseObjectStore>,
    tree_id: &ObjectId,
    prefix: Vec<u8>,
    recursive: bool,
    name_only: bool,
) -> Result<()> {
    if recursive {
        return print_tree_entries_recursive(tree_cache, tree_id, prefix, name_only);
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

fn parse_batch_command(line: &str, buffer: bool) -> Result<BatchCommand<'_>> {
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
            message: if line.is_empty() {
                "empty command in input".into()
            } else {
                format!("unknown command: '{line}'")
            },
        });
    };
    match command {
        "info" if !objectish.is_empty() => Ok(BatchCommand::Object(objectish, false)),
        "contents" if !objectish.is_empty() => Ok(BatchCommand::Object(objectish, true)),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unknown command: '{line}'"),
        }),
    }
}

fn write_batch_object(
    out: &mut impl Write,
    object: &LooseObject,
    include_content: bool,
) -> Result<()> {
    writeln!(
        out,
        "{} {} {}",
        object.id.to_hex(),
        object.kind.as_str(),
        object.content.len()
    )?;
    if include_content {
        out.write_all(&object.content)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn write_object_batch_header(
    out: &mut impl Write,
    store: &LooseObjectStore,
    id: &ObjectId,
) -> Result<bool> {
    let Some((kind, size)) = store.object_header_hint(id)? else {
        return Ok(false);
    };
    writeln!(out, "{} {} {}", id.to_hex(), kind.as_str(), size)?;
    Ok(true)
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

    let temp_path = unique_temp_sibling(&std::env::temp_dir().join("skron-hash-object-stdin"));
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
        write_or_hash_path(store, kind, &temp_path)
    })();
    let _ = fs::remove_file(&temp_path);
    result
}

fn write_or_hash_path(
    store: Option<&LooseObjectStore>,
    kind: GitObjectKind,
    path: &Path,
) -> Result<ObjectId> {
    if kind != GitObjectKind::Blob {
        let content = fs::read(path)?;
        return write_or_hash(store, kind, &content);
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

pub(crate) fn init_command(
    directory: Option<PathBuf>,
    bare: bool,
    initial_branch: Option<String>,
) -> Result<()> {
    let directory = match directory {
        Some(path) if path.is_absolute() => path,
        Some(path) => std::env::current_dir()?.join(path),
        None => std::env::current_dir()?,
    };
    let result = init_repository(
        directory,
        InitRepositoryOptions {
            bare,
            initial_branch: initial_branch.unwrap_or_else(|| "main".to_owned()),
        },
    )?;
    println!(
        "Initialized empty Git repository in {}/",
        result.git_dir.display()
    );
    Ok(())
}
