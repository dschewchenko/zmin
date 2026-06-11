use std::borrow::Cow;

use super::*;
use skron_primitives::git_runtime::GitTransport;

const HTTP_HELPER_INLINE_BODY_LIMIT: usize = 8 * 1024 * 1024;
const PACK_RECEIPT_BUF_CAPACITY: usize = 64 * 1024;
const HTTP_HELPER_FILE_READ_BUF_CAPACITY: usize = PACK_RECEIPT_BUF_CAPACITY;
const HTTP_DIRECT_WRITE_BUF_CAPACITY: usize = 64 * 1024;
const HTTP_DIRECT_READ_BUF_CAPACITY: usize = 64 * 1024;
const HTTP_RESPONSE_DRAIN_BUF_CAPACITY: usize = HTTP_DIRECT_READ_BUF_CAPACITY;
const HTTP_HELPER_PIPE_BUF_CAPACITY: usize = 64 * 1024;
const DAEMON_TRANSPORT_READ_BUF_CAPACITY: usize = 64 * 1024;
const PKT_LINE_PAYLOAD_CAPACITY_HINT: usize = 1024;
const PKT_LINE_PAYLOAD_INITIAL_CAPACITY_LIMIT: usize = 64 * 1024;
const HTTP_RESPONSE_LINE_LIMIT: usize = 64 * 1024;
const TRANSPORT_TEXT_LINE_LIMIT: usize = 64 * 1024;
const TRANSPORT_STDIN_BUF_CAPACITY: usize = TRANSPORT_TEXT_LINE_LIMIT;
const HTTP_REMOTE_REF_ROWS_CAPACITY_HINT: usize = 64;
const HTTP_REMOTE_REF_ROW_BYTES_HINT: usize = 80;
const RECEIVE_PACK_UPDATE_CAPACITY_HINT: usize = 8;
const UPLOAD_PACK_WANT_CAPACITY_HINT: usize = 8;
const UPLOAD_PACK_HAVE_CAPACITY_HINT: usize = 64;
const UPLOAD_PACK_SHALLOW_CAPACITY_HINT: usize = 8;
const UPLOAD_PACK_DEEPEN_NOT_CAPACITY_HINT: usize = 4;
const UPLOAD_PACK_BASE_ID_INITIAL_CAPACITY_LIMIT: usize = 8192;
const UPLOAD_PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT: usize = 8192;
const TRANSPORT_HISTORY_COLLECTION_CAPACITY_LIMIT: usize = 8192;
const TRANSPORT_REF_COLLECTION_CAPACITY_LIMIT: usize = 8192;
const TAG_PEEL_SEEN_CAPACITY_HINT: usize = 4;
const CLONE_CONFIG_VALUES_CAPACITY_HINT: usize = 4;
const HTTP_EXTRA_HEADER_CAPACITY_HINT: usize = 4;
const HTTP_CREDENTIAL_HELPER_CAPACITY_HINT: usize = 2;
const HTTP_REDIRECT_LIMIT: usize = 10;
const COPY_REACHABLE_SEEN_INITIAL_CAPACITY_LIMIT: usize = 8192;
static TEMP_HTTP_HELPER_BODY_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(crate) struct UploadPackOptions {
    pub(crate) strict: bool,
    pub(crate) no_strict: bool,
    pub(crate) stateless_rpc: bool,
    pub(crate) advertise_refs: bool,
    pub(crate) timeout: Option<u64>,
    pub(crate) directory: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct DaemonOptions {
    pub(crate) verbose: bool,
    pub(crate) export_all: bool,
    pub(crate) timeout: Option<u64>,
    pub(crate) init_timeout: Option<u64>,
    pub(crate) max_connections: Option<usize>,
    pub(crate) strict_paths: bool,
    pub(crate) base_path: Option<PathBuf>,
    pub(crate) base_path_relaxed: bool,
    pub(crate) reuseaddr: bool,
    pub(crate) pid_file: Option<PathBuf>,
    pub(crate) inetd: bool,
    pub(crate) listen: Vec<String>,
    pub(crate) port: Option<u16>,
    pub(crate) directories: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct HttpFetchOptions {
    pub(crate) commit: bool,
    pub(crate) tags: bool,
    pub(crate) all: bool,
    pub(crate) verbose: bool,
    pub(crate) recover: bool,
    pub(crate) write_ref: Vec<String>,
    pub(crate) stdin: bool,
    pub(crate) packfile: Option<String>,
    pub(crate) index_pack_args: Vec<String>,
    pub(crate) args: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct HttpPushOptions {
    pub(crate) all: bool,
    pub(crate) dry_run: bool,
    pub(crate) force: bool,
    pub(crate) verbose: bool,
    pub(crate) remote: String,
    pub(crate) heads: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FetchPackOptions {
    pub(crate) all: bool,
    pub(crate) stdin: bool,
    pub(crate) quiet: bool,
    pub(crate) keep: bool,
    pub(crate) thin: bool,
    pub(crate) include_tag: bool,
    pub(crate) upload_pack: Option<String>,
    pub(crate) depth: Option<usize>,
    pub(crate) no_progress: bool,
    pub(crate) diag_url: bool,
    pub(crate) verbose: bool,
    pub(crate) directory: String,
    pub(crate) refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SendPackOptions {
    pub(crate) mirror: bool,
    pub(crate) dry_run: bool,
    pub(crate) force: bool,
    pub(crate) receive_pack: Option<String>,
    pub(crate) verbose: bool,
    pub(crate) thin: bool,
    pub(crate) atomic: bool,
    pub(crate) all: bool,
    pub(crate) stdin: bool,
    pub(crate) directory: String,
    pub(crate) refs: Vec<String>,
}

fn primitive_runtime_for_repo(repo: &GitRepo) -> CliPrimitiveRuntime {
    CliPrimitiveRuntime::new_from_repo(repo, GitHashAlgorithm::Sha1)
}

fn refs_adapter_from_git_dir(path: impl AsRef<std::path::Path>) -> OwnedCliRefsStoreAdapter {
    OwnedCliRefsStoreAdapter::from_path(path, GitHashAlgorithm::Sha1)
}

fn object_adapter_from_objects_dir(path: impl AsRef<std::path::Path>) -> LooseObjectStore {
    LooseObjectStore::new(path.as_ref(), GitHashAlgorithm::Sha1)
}

pub(crate) fn upload_pack(options: UploadPackOptions) -> Result<()> {
    if options.strict && options.no_strict {
        return Err(CliError::Fatal {
            code: 129,
            message: "options --strict and --no-strict cannot be used together".into(),
        });
    }
    let _ = options.timeout;
    let repo = upload_pack_repo_from_path(&options.directory, options.strict)?;
    let runtime = primitive_runtime_for_repo(&repo);
    let refs = runtime.refs_store_adapter();
    {
        let stdout = io::stdout();
        let stdout = stdout.lock();
        let mut stdout = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdout);
        write_upload_pack_advertisement_from_adapter(&refs, &mut stdout)?;
        stdout.flush()?;
    }

    if options.advertise_refs {
        return Ok(());
    }

    let stdin = io::stdin();
    let mut stdin = io::BufReader::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdin.lock());
    let request = read_upload_pack_request_from_stdin(&mut stdin)?;
    if request.wants.is_empty() {
        return Ok(());
    }
    upload_pack_respond_with_pack(&repo, request, options.stateless_rpc)
}

pub(crate) fn http_fetch(options: HttpFetchOptions) -> Result<()> {
    if let Some(packfile) = options.packfile.as_deref() {
        return http_fetch_packfile(packfile, &options.index_pack_args, &options.args);
    }
    if !options.index_pack_args.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--index-pack-args' requires '--packfile'".into(),
        });
    }
    let _ = (options.tags, options.recover);
    let (commit_id, url) = parse_http_fetch_args(&options)?;
    let repo = find_repo_or_bare()?;
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let mut roots = Vec::with_capacity(http_fetch_root_initial_capacity(
        options.stdin,
        commit_id.is_some(),
    ));
    if options.stdin {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::with_capacity(TRANSPORT_STDIN_BUF_CAPACITY, stdin.lock());
        collect_first_token_object_ids_from_reader(&mut stdin, &mut roots)?;
    }
    if let Some(id) = commit_id.as_deref() {
        roots.push(ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?);
    }
    if roots.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git http-fetch [-c] [-t] [-a] [-v] [--recover] [-w ref] [--stdin | commit-id] url".into(),
        });
    }
    let url = parsed_http_url_with_extra_headers(Some(&repo), &url)?;
    let mut helper = RemoteHttpHelperSession::spawn(&url)?;
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    let mut seen = HashSet::with_capacity(transport_ref_collection_capacity(roots.len()));
    let mut fetch_context = HttpFetchObjectContext {
        url: &url,
        helper: &mut helper,
        store: &store,
        commit_cache: &commit_cache,
        tree_cache: &tree_cache,
        options: &options,
        seen: &mut seen,
        suffix_buffer: String::new(),
    };
    for id in &roots {
        http_fetch_object_recursive(&mut fetch_context, id)?;
    }
    let refs = refs_adapter_from_git_dir(&repo.git_dir);
    if let Some(first) = roots.first() {
        for ref_name in options.write_ref {
            refs.write_ref(&ref_name, first)?;
        }
    }
    Ok(())
}

fn collect_first_token_object_ids_from_reader<R: BufRead>(
    reader: &mut R,
    out: &mut Vec<ObjectId>,
) -> Result<()> {
    let mut line = String::new();
    loop {
        if read_limited_transport_text_line(reader, &mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(id) = line.split_whitespace().next() else {
            continue;
        };
        out.push(ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?);
    }
    Ok(())
}

fn collect_trimmed_lines_from_reader<R: BufRead>(
    reader: &mut R,
    out: &mut Vec<String>,
) -> Result<()> {
    let mut line = String::new();
    loop {
        if read_limited_transport_text_line(reader, &mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if !line.is_empty() {
            out.push(line.to_owned());
        }
    }
    Ok(())
}

fn http_fetch_root_initial_capacity(read_stdin: bool, has_commit_id: bool) -> usize {
    let stdin_hint = if read_stdin {
        HTTP_REMOTE_REF_ROWS_CAPACITY_HINT
    } else {
        0
    };
    transport_ref_collection_capacity(stdin_hint + usize::from(has_commit_id))
}

fn http_fetch_packfile(
    packfile_hash: &str,
    index_pack_args: &[String],
    args: &[String],
) -> Result<()> {
    if packfile_hash.len() != 40 || !packfile_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("argument to --packfile must be a valid hash (got '{packfile_hash}')"),
        });
    }
    if index_pack_args.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "the option '--packfile' requires '--index-pack-args'".into(),
        });
    }
    let index_pack_args = parse_http_fetch_index_pack_args(index_pack_args)?;
    let [url] = args else {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git http-fetch --packfile=<hash> --index-pack-args=<args> <url>"
                .into(),
        });
    };
    let repo = find_repo_or_bare()?;
    let url = parsed_http_url_with_extra_headers(Some(&repo), url)?;
    let (head, mut body) = http_request_reader(&url, "GET", "", &[])?;
    if head.status_code != 200 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP packfile request failed: {}", head.status_line),
        });
    }
    let pack_dir = repo.objects_dir.join("pack");
    fs::create_dir_all(&pack_dir)?;
    let (temp_pack, file) = temp_http_pack_file(&repo.objects_dir)?;
    {
        let mut file = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, file);
        copy_http_response_body_to_writer(
            &mut body,
            &mut file,
            head.content_length,
            head.chunked,
            "HTTP packfile response ended early",
        )?;
        file.flush()?;
    }
    if !pack_file_starts_with_pack_magic(&temp_pack)? {
        let _ = fs::remove_file(&temp_pack);
        return Err(CliError::Fatal {
            code: 128,
            message: "downloaded packfile is not a Git pack".into(),
        });
    }
    let indexed = if index_pack_args.no_rev_index {
        let indexed = index_pack_file_index_only(GitHashAlgorithm::Sha1, &temp_pack)?;
        HttpFetchIndexedPack {
            pack_id: indexed.pack_id,
            index: indexed.index,
            reverse_index: None,
        }
    } else {
        let indexed = index_pack_file(GitHashAlgorithm::Sha1, &temp_pack)?;
        HttpFetchIndexedPack {
            pack_id: indexed.pack_id,
            index: indexed.index,
            reverse_index: Some(indexed.reverse_index),
        }
    };
    let _ = index_pack_args.verbose;
    if !object_id_hex_eq(&indexed.pack_id, packfile_hash) {
        let _ = fs::remove_file(&temp_pack);
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "downloaded packfile hash mismatch: expected {packfile_hash}, got {}",
                indexed.pack_id.to_hex()
            ),
        });
    }
    install_temp_pack_file(
        &pack_file_path(&pack_dir, &indexed.pack_id, ".pack"),
        &temp_pack,
        &indexed.pack_id,
    )?;
    let index_path = index_pack_args
        .index_output
        .clone()
        .unwrap_or_else(|| pack_file_path(&pack_dir, &indexed.pack_id, ".idx"));
    write_content_addressed_file(&index_path, &indexed.index)?;
    if let Some(reverse_index) = indexed.reverse_index.as_ref() {
        write_content_addressed_file(
            &pack_file_path(&pack_dir, &indexed.pack_id, ".rev"),
            reverse_index,
        )?;
    }
    if let Some(keep_message) = index_pack_args.keep {
        fs::write(
            pack_file_path(&pack_dir, &indexed.pack_id, ".keep"),
            keep_message,
        )?;
        println!("keep\t{}", indexed.pack_id.to_hex());
    } else {
        println!("pack\t{}", indexed.pack_id.to_hex());
    }
    Ok(())
}

#[derive(Default)]
struct HttpFetchIndexPackArgs {
    keep: Option<String>,
    no_rev_index: bool,
    index_output: Option<PathBuf>,
    stdin: bool,
    verbose: bool,
}

struct HttpFetchIndexedPack {
    pack_id: ObjectId,
    index: Vec<u8>,
    reverse_index: Option<Vec<u8>>,
}

fn parse_http_fetch_index_pack_args(args: &[String]) -> Result<HttpFetchIndexPackArgs> {
    let Some(command) = args.first() else {
        return Ok(HttpFetchIndexPackArgs::default());
    };
    if command != "index-pack" {
        return Err(CliError::Stderr {
            code: 128,
            text: format!(
                "git: '{command}' is not a git command. See 'git --help'.\nfatal: finish_http_pack_request gave result -1\n"
            ),
        });
    }
    let mut parsed = HttpFetchIndexPackArgs::default();
    let mut cursor = 1usize;
    while cursor < args.len() {
        let arg = args[cursor].as_str();
        match arg {
            "--stdin" => parsed.stdin = true,
            "-v" => parsed.verbose = true,
            "--rev-index" => {}
            "--no-rev-index" => parsed.no_rev_index = true,
            "--keep" => parsed.keep = Some(String::new()),
            _ if arg.starts_with("--keep=") => {
                parsed.keep = Some(
                    arg.strip_prefix("--keep=")
                        .expect("checked keep prefix")
                        .to_owned(),
                );
            }
            "-o" => {
                cursor += 1;
                let Some(path) = args.get(cursor) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: "-o requires a value".into(),
                    });
                };
                parsed.index_output = Some(PathBuf::from(path));
            }
            value if value.starts_with("--index-output=") => {
                parsed.index_output = value.strip_prefix("--index-output=").map(PathBuf::from);
            }
            value
                if value == "--fix-thin"
                    || value == "--strict"
                    || value.starts_with("--strict=")
                    || value == "--fsck-objects"
                    || value.starts_with("--fsck-objects=") => {}
            _ => {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("unsupported index-pack option '{arg}'"),
                });
            }
        }
        cursor += 1;
    }
    if !parsed.stdin {
        return Err(CliError::Stderr {
            code: 128,
            text: "usage: git index-pack [-v] [-o <index-file>] [--keep | --keep=<msg>] [--[no-]rev-index] [--verify] [--strict[=<msg-id>=<severity>...]] [--fsck-objects[=<msg-id>=<severity>...]] (<pack-file> | --stdin [--fix-thin] [<pack-file>])\nfatal: finish_http_pack_request gave result -1\n".into(),
        });
    }
    Ok(parsed)
}

pub(crate) fn http_push(options: HttpPushOptions) -> Result<()> {
    if options.all && !options.heads.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "http-push --all cannot be combined with explicit heads".into(),
        });
    }
    let repo = find_repo()?;
    let refs = refs_adapter_from_git_dir(&repo.git_dir);
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let commit_cache = CommitObjectCache::new(&store);
    let url = http_push_remote_url(&repo, &options.remote)?;
    let url = parsed_http_url_with_extra_headers(Some(&repo), &url)?;
    let mut helper = if url.scheme == HttpScheme::Https {
        Some(RemoteHttpHelperSession::spawn(&url)?)
    } else {
        None
    };
    let specs = http_push_refspecs(&refs, &options)?;
    let initial_capacity = transport_ref_collection_capacity(specs.len());
    let mut pushes = Vec::with_capacity(initial_capacity);
    let mut roots = Vec::with_capacity(initial_capacity);
    for spec in specs {
        let push_ref = parse_push_refspec(&repo, &refs, &spec, &options.remote)?;
        if let Some(id) = push_ref.id.clone() {
            roots.push(id);
        }
        pushes.push(push_ref);
    }
    let mut objects = collect_reachable_object_ids_from_roots(&store, &roots)?
        .into_iter()
        .collect::<Vec<_>>();
    objects.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    let mut suffix_buffer = String::new();
    for id in objects {
        let hex = id.to_hex();
        write_loose_object_http_suffix_from_hex(&mut suffix_buffer, &hex);
        if options.dry_run {
            if options.verbose {
                println!("would put {suffix_buffer}");
            }
            continue;
        }
        let body = compressed_loose_object_body(&repo, &store, &id, &hex)?;
        if let Some(helper) = helper.as_mut() {
            http_put_body_with_helper(&url, helper, &suffix_buffer, &body)?;
        } else {
            http_put_body_direct(&url, &suffix_buffer, &body)?;
        }
        if options.verbose {
            println!("put {suffix_buffer}");
        }
    }
    for push_ref in pushes {
        if let Some(helper) = helper.as_mut() {
            validate_http_push_update(&url, helper, &commit_cache, &push_ref, options.force)?;
        } else {
            validate_http_push_update_direct(&url, &commit_cache, &push_ref, options.force)?;
        }
        let display = push_ref
            .source_display
            .clone()
            .or_else(|| push_ref.id.as_ref().map(ObjectId::to_hex))
            .unwrap_or_else(|| "(delete)".to_owned());
        let destination = push_ref
            .destination
            .strip_prefix("refs/heads/")
            .unwrap_or(&push_ref.destination);
        if options.dry_run {
            println!("{} -> {} (dry run)", display, destination);
            continue;
        }
        if let Some(id) = push_ref.id.as_ref() {
            let body = format!("{}\n", id.to_hex());
            if let Some(helper) = helper.as_mut() {
                http_put_with_helper(&url, helper, &push_ref.destination, body.as_bytes())?;
            } else {
                http_put_direct(&url, &push_ref.destination, body.as_bytes())?;
            }
        } else {
            if let Some(helper) = helper.as_mut() {
                http_delete_with_helper(&url, helper, &push_ref.destination)?;
            } else {
                http_delete_direct(&url, &push_ref.destination)?;
            }
        }
        println!("{} -> {}", display, destination);
    }
    Ok(())
}

fn http_push_remote_url(repo: &GitRepo, remote: &str) -> Result<String> {
    if is_http_transport_url(remote) {
        Ok(remote.to_owned())
    } else {
        remote_url(repo, remote)
    }
}

fn http_push_refspecs(
    refs: &OwnedCliRefsStoreAdapter,
    options: &HttpPushOptions,
) -> Result<Vec<String>> {
    if options.all {
        let mut refspecs = refs.ref_names("refs/heads/")?;
        refspecs.reserve(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
        return Ok(refspecs);
    }
    if options.heads.is_empty() {
        Ok(vec![default_push_refspec(refs)?])
    } else {
        Ok(options.heads.clone())
    }
}

fn compressed_loose_object_body(
    repo: &GitRepo,
    store: &LooseObjectStore,
    id: &ObjectId,
    hex: &str,
) -> Result<PackBody> {
    let path = loose_object_path(&repo.objects_dir, hex)?;
    match fs::File::open(&path) {
        Ok(_) => Ok(PackBody::File {
            path,
            remove_on_drop: false,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(body) = compressed_streamable_blob_body(store, id)? {
                return Ok(body);
            }
            let object = store.read_object(id)?;
            let temp_path = temp_http_helper_body_path()?;
            let result = fs::write(
                &temp_path,
                encode_loose_object(object.kind, &object.content)?,
            )
            .map(|()| PackBody::File {
                path: temp_path.clone(),
                remove_on_drop: true,
            })
            .map_err(CliError::Io);
            if result.is_err() {
                let _ = fs::remove_file(temp_path);
            }
            result
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn compressed_streamable_blob_body(
    store: &LooseObjectStore,
    id: &ObjectId,
) -> Result<Option<PackBody>> {
    let Some(size) = store.streamable_blob_size_hint(id)? else {
        return Ok(None);
    };
    let temp_path = temp_http_helper_body_path()?;
    let result = write_compressed_streamable_blob_body(&temp_path, store, id, size)
        .map(|()| PackBody::File {
            path: temp_path.clone(),
            remove_on_drop: true,
        })
        .map_err(CliError::Io);
    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result.map(Some)
}

fn write_compressed_streamable_blob_body(
    temp_path: &Path,
    store: &LooseObjectStore,
    id: &ObjectId,
    size: usize,
) -> io::Result<()> {
    let file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(temp_path)?;
    let mut writer = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, file);
    {
        let mut encoder = ZlibEncoder::new(&mut writer, Compression::default());
        encoder.write_all(GitObjectKind::Blob.as_bytes())?;
        encoder.write_all(b" ")?;
        write!(encoder, "{size}")?;
        encoder.write_all(b"\0")?;
        if !store.write_streamable_blob(id, &mut encoder)? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "streamable blob disappeared while writing HTTP body",
            ));
        }
        encoder.finish()?;
    }
    writer.flush()
}

fn validate_http_push_update(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    push_ref: &PushRef,
    force: bool,
) -> Result<()> {
    let Some(current) = http_get_optional_with_helper(url, helper, &push_ref.destination)? else {
        return Ok(());
    };
    let current = String::from_utf8_lossy(&current);
    let current = current.trim();
    if current.is_empty() {
        return Ok(());
    }
    let current = ObjectId::from_hex(GitHashAlgorithm::Sha1, current)?;
    let Some(new_id) = &push_ref.id else {
        return Ok(());
    };
    if !force && !is_ancestor_commit_cached(commit_cache, &current, new_id)? {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "failed to push some refs to '{}': non-fast-forward",
                push_ref.destination
            ),
        });
    }
    Ok(())
}

fn validate_http_push_update_direct(
    url: &ParsedHttpUrl,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    push_ref: &PushRef,
    force: bool,
) -> Result<()> {
    let Some(current) = http_get_optional_direct(url, &push_ref.destination)? else {
        return Ok(());
    };
    let current = String::from_utf8_lossy(&current);
    let current = current.trim();
    if current.is_empty() {
        return Ok(());
    }
    let current = ObjectId::from_hex(GitHashAlgorithm::Sha1, current)?;
    let Some(new_id) = &push_ref.id else {
        return Ok(());
    };
    if !force && !is_ancestor_commit_cached(commit_cache, &current, new_id)? {
        return Err(CliError::Fatal {
            code: 1,
            message: format!(
                "failed to push some refs to '{}': non-fast-forward",
                push_ref.destination
            ),
        });
    }
    Ok(())
}

fn parse_http_fetch_args(options: &HttpFetchOptions) -> Result<(Option<String>, String)> {
    match (options.stdin, options.args.as_slice()) {
        (true, [url]) => Ok((None, url.clone())),
        (false, [commit_id, url]) => Ok((Some(commit_id.clone()), url.clone())),
        _ => Err(CliError::Fatal {
            code: 129,
            message: "usage: git http-fetch [-c] [-t] [-a] [-v] [--recover] [-w ref] [--stdin | commit-id] url".into(),
        }),
    }
}

pub(crate) struct HttpFetchObjectContext<'a> {
    url: &'a ParsedHttpUrl,
    helper: &'a mut RemoteHttpHelperSession,
    store: &'a LooseObjectStore,
    commit_cache: &'a CommitObjectCache<'a, LooseObjectStore>,
    tree_cache: &'a TreeObjectCache<'a, LooseObjectStore>,
    options: &'a HttpFetchOptions,
    seen: &'a mut HashSet<ObjectId>,
    suffix_buffer: String,
}

impl<'a> HttpFetchObjectContext<'a> {
    pub(crate) fn new(
        url: &'a ParsedHttpUrl,
        helper: &'a mut RemoteHttpHelperSession,
        store: &'a LooseObjectStore,
        commit_cache: &'a CommitObjectCache<'a, LooseObjectStore>,
        tree_cache: &'a TreeObjectCache<'a, LooseObjectStore>,
        options: &'a HttpFetchOptions,
        seen: &'a mut HashSet<ObjectId>,
    ) -> Self {
        Self {
            url,
            helper,
            store,
            commit_cache,
            tree_cache,
            options,
            seen,
            suffix_buffer: String::new(),
        }
    }
}

pub(crate) fn http_fetch_object_recursive(
    context: &mut HttpFetchObjectContext<'_>,
    id: &ObjectId,
) -> Result<()> {
    if !context.seen.insert(id.clone()) {
        return Ok(());
    }
    let object = match context.store.read_object(id) {
        Ok(object) => object,
        Err(_) => {
            http_fetch_loose_object(context, id)?;
            if context.options.verbose {
                eprintln!("got {}", id.to_hex());
            }
            context.store.read_object(id)?
        }
    };
    match object.kind {
        GitObjectKind::Commit => {
            let commit = context.commit_cache.read_commit(id)?;
            if !context.options.commit {
                http_fetch_object_recursive(context, &commit.tree)?;
            }
            if context.options.all {
                for parent in &commit.parents {
                    http_fetch_object_recursive(context, parent)?;
                }
            }
        }
        GitObjectKind::Tree => {
            for entry in context.tree_cache.read_tree(id)?.iter() {
                if entry.mode != TreeMode::Gitlink {
                    http_fetch_object_recursive(context, &entry.id)?;
                }
            }
        }
        GitObjectKind::Tag => {
            let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
            http_fetch_object_recursive(context, &tag.target)?;
        }
        GitObjectKind::Blob => {}
    }
    Ok(())
}

fn http_fetch_loose_object(context: &mut HttpFetchObjectContext<'_>, id: &ObjectId) -> Result<()> {
    let hex = id.to_hex();
    write_loose_object_http_suffix_from_hex(&mut context.suffix_buffer, &hex);
    let path = loose_object_path(context.store.objects_dir(), &hex)?;
    if path.exists() {
        return Ok(());
    }
    let parent = path.parent().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "loose object path has no parent".into(),
    })?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        "tmp_http_fetch_{}_{}",
        std::process::id(),
        &hex[2..14]
    ));
    if let Err(error) =
        http_get_to_file_with_helper(context.url, context.helper, &context.suffix_buffer, &tmp)
    {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    match fs::hard_link(&tmp, &path) {
        Ok(()) => {
            let _ = fs::remove_file(&tmp);
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&tmp);
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            return Err(CliError::Io(error));
        }
    }
    Ok(())
}

fn write_loose_object_http_suffix_from_hex(out: &mut String, hex: &str) {
    out.clear();
    out.reserve("objects/".len() + hex.len() + 1);
    out.push_str("objects/");
    out.push_str(&hex[..2]);
    out.push('/');
    out.push_str(&hex[2..]);
}

pub(crate) fn http_fetch_smart_pack_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
) -> Result<bool> {
    if roots.is_empty() {
        return Ok(true);
    }
    let request = build_upload_pack_request(roots, None)?;

    let response_path = temp_http_helper_output_path()?;
    let result = (|| {
        let head = {
            let _trace = phase_trace("http_fetch_smart_pack.helper_request_to_file");
            helper.request_to_file(
                url,
                "POST",
                "git-upload-pack",
                &request,
                &PackBody::Empty,
                &response_path,
            )?
        };
        if head.status_code != 200 {
            return Ok(false);
        }
        let mut body = http_helper_file_body_reader(fs::File::open(&response_path)?);
        let (temp_pack, file) = temp_http_pack_file(objects_dir)?;
        let pack_path = {
            let _trace = phase_trace("http_fetch_smart_pack.sideband_to_pack");
            parse_upload_pack_sideband_pack_to_open_file(&mut body, &temp_pack, file)?
        };
        let Some(pack_path) = pack_path else {
            let _ = fs::remove_file(&temp_pack);
            return Ok(false);
        };
        {
            let _trace = phase_trace("http_fetch_smart_pack.index_pack");
            write_indexed_pack_file(objects_dir, &pack_path)?;
        }
        Ok(true)
    })();
    let _ = fs::remove_file(response_path);
    result
}

fn http_fetch_smart_pack_direct(
    url: &ParsedHttpUrl,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
) -> Result<bool> {
    if roots.is_empty() {
        return Ok(true);
    }
    let request = build_upload_pack_request(roots, None)?;
    let (head, mut body) = {
        let _trace = phase_trace("http_fetch_smart_pack.direct_request");
        http_request_reader(url, "POST", "git-upload-pack", &request)?
    };
    if head.status_code != 200 {
        return Ok(false);
    }
    let (temp_pack, file) = temp_http_pack_file(objects_dir)?;
    let pack_path = {
        let _trace = phase_trace("http_fetch_smart_pack.sideband_to_pack");
        parse_upload_pack_sideband_pack_to_open_file(&mut body, &temp_pack, file)?
    };
    let Some(pack_path) = pack_path else {
        let _ = fs::remove_file(&temp_pack);
        return Ok(false);
    };
    {
        let _trace = phase_trace("http_fetch_smart_pack.index_pack");
        write_indexed_pack_file(objects_dir, &pack_path)?;
    }
    Ok(true)
}

fn parse_upload_pack_sideband_pack_to_open_file<R: Read>(
    reader: &mut R,
    pack_path: &Path,
    file: fs::File,
) -> Result<Option<PathBuf>> {
    let mut payload = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    let ack = loop {
        match read_upload_pack_response_pkt_line_into(reader, &mut payload)? {
            PktLineRead::Eof => return Ok(None),
            PktLineRead::Flush => continue,
            PktLineRead::Payload => break payload.as_slice(),
        }
    };
    if ack != b"NAK\n" && !ack.starts_with(b"ACK ") {
        return Ok(None);
    }
    write_upload_pack_sideband_pack_to_open_file(reader, file).map(|wrote_pack| {
        if wrote_pack {
            Some(pack_path.to_path_buf())
        } else {
            None
        }
    })
}

fn write_upload_pack_sideband_pack_to_open_file<R: Read>(
    reader: &mut R,
    file: fs::File,
) -> Result<bool> {
    let mut file = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, file);
    let mut first_bytes = [0_u8; 4];
    let mut first_bytes_len = 0_usize;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let payload_len = match read_upload_pack_response_payload_len(reader)? {
            None => break,
            Some(0) => continue,
            Some(payload_len) => payload_len,
        };
        let mut band = [0_u8; 1];
        reader.read_exact(&mut band)?;
        let sideband_len = payload_len - 1;
        match band {
            [1] => stream_sideband_payload_to_pack(
                reader,
                sideband_len,
                &mut file,
                &mut first_bytes,
                &mut first_bytes_len,
                &mut buffer,
            )?,
            [2] => discard_exact_payload_with_buffer(reader, sideband_len, &mut buffer)?,
            [3] => {
                let payload = read_exact_payload_to_vec(reader, sideband_len)?;
                return Err(CliError::Fatal {
                    code: 128,
                    message: String::from_utf8_lossy(&payload).trim().to_owned(),
                });
            }
            [_] => {
                discard_exact_payload_with_buffer(reader, sideband_len, &mut buffer)?;
                return Ok(false);
            }
        }
    }
    file.flush()?;
    Ok(first_bytes_len == first_bytes.len() && first_bytes == *b"PACK")
}

fn stream_sideband_payload_to_pack<R: Read, W: Write>(
    reader: &mut R,
    len: usize,
    writer: &mut W,
    first_bytes: &mut [u8; 4],
    first_bytes_len: &mut usize,
    buffer: &mut [u8],
) -> Result<()> {
    let mut remaining = len;
    while remaining > 0 {
        let want = remaining.min(buffer.len());
        let read = reader.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(CliError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "sideband pack payload ended early",
            )));
        }
        if *first_bytes_len < first_bytes.len() {
            let copy_len = (first_bytes.len() - *first_bytes_len).min(read);
            first_bytes[*first_bytes_len..*first_bytes_len + copy_len]
                .copy_from_slice(&buffer[..copy_len]);
            *first_bytes_len += copy_len;
        }
        writer.write_all(&buffer[..read])?;
        remaining -= read;
    }
    Ok(())
}

enum PktLineRead {
    Eof,
    Flush,
    Payload,
}

fn read_upload_pack_response_pkt_line_into<R: Read>(
    input: &mut R,
    payload: &mut Vec<u8>,
) -> Result<PktLineRead> {
    let Some(payload_len) = read_upload_pack_response_payload_len(input)? else {
        return Ok(PktLineRead::Eof);
    };
    if payload_len == 0 {
        payload.clear();
        return Ok(PktLineRead::Flush);
    }
    read_exact_payload_into(input, payload_len, payload)?;
    Ok(PktLineRead::Payload)
}

fn read_upload_pack_response_payload_len<R: Read>(input: &mut R) -> Result<Option<usize>> {
    let mut header = [0_u8; 4];
    match input.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(CliError::Io(error)),
    };
    let len = parse_pkt_line_len(&header, "invalid upload-pack pkt-line header")?;
    if len == 0 {
        return Ok(Some(0));
    }
    len.checked_sub(4).map(Some).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "invalid upload-pack pkt-line length".into(),
    })
}

fn read_exact_payload_to_vec<R: Read + ?Sized>(input: &mut R, len: usize) -> Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(pkt_line_payload_initial_capacity(len));
    read_exact_payload_into(input, len, &mut payload)?;
    Ok(payload)
}

fn read_exact_payload_into<R: Read + ?Sized>(
    input: &mut R,
    len: usize,
    payload: &mut Vec<u8>,
) -> Result<()> {
    payload.clear();
    let mut remaining = len;
    while remaining > 0 {
        let read_len = remaining.min(PACK_RECEIPT_BUF_CAPACITY);
        let start = payload.len();
        payload.reserve_exact(read_len);
        let spare = payload.spare_capacity_mut();
        // SAFETY: payload reserved read_len spare bytes immediately before this slice.
        // The bytes are exposed with set_len only after read_exact succeeds.
        let target =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr().cast::<u8>(), read_len) };
        input.read_exact(target).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                CliError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "pkt-line payload ended early",
                ))
            } else {
                CliError::Io(error)
            }
        })?;
        // SAFETY: the previous read_exact initialized exactly read_len bytes.
        unsafe {
            payload.set_len(start + read_len);
        }
        remaining -= read_len;
    }
    Ok(())
}

fn pkt_line_payload_initial_capacity(len: usize) -> usize {
    len.min(PKT_LINE_PAYLOAD_INITIAL_CAPACITY_LIMIT)
}

fn discard_exact_payload_with_buffer<R: Read>(
    input: &mut R,
    len: usize,
    buffer: &mut [u8],
) -> Result<()> {
    let mut remaining = len;
    while remaining > 0 {
        let want = remaining.min(buffer.len());
        let read = input.read(&mut buffer[..want])?;
        if read == 0 {
            return Err(CliError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "pkt-line payload ended early",
            )));
        }
        remaining -= read;
    }
    Ok(())
}

fn write_indexed_pack_file(objects_dir: &std::path::Path, temp_pack_path: &Path) -> Result<()> {
    let indexed = index_pack_file_index_only(GitHashAlgorithm::Sha1, temp_pack_path)?;
    let pack_dir = objects_dir.join("pack");
    fs::create_dir_all(&pack_dir)?;
    let pack_path = pack_file_path(&pack_dir, &indexed.pack_id, ".pack");
    install_temp_pack_file(&pack_path, temp_pack_path, &indexed.pack_id)?;
    write_content_addressed_file(
        &pack_file_path(&pack_dir, &indexed.pack_id, ".idx"),
        &indexed.index,
    )?;
    Ok(())
}

fn pack_file_path(pack_dir: &Path, pack_id: &ObjectId, suffix: &str) -> PathBuf {
    pack_dir.join(pack_file_name(pack_id, suffix))
}

fn pack_file_name(pack_id: &ObjectId, suffix: &str) -> String {
    let mut name = String::with_capacity("pack-".len() + pack_id.hex_len() + suffix.len());
    name.push_str("pack-");
    pack_id.write_hex(&mut name).expect("writing hex to String");
    name.push_str(suffix);
    name
}

fn object_id_hex_eq(id: &ObjectId, hex: &str) -> bool {
    if hex.len() != id.hex_len() {
        return false;
    }
    let mut encoded = [0_u8; 64];
    let mut cursor = 0;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in id.as_bytes() {
        encoded[cursor] = HEX[(byte >> 4) as usize];
        encoded[cursor + 1] = HEX[(byte & 0x0f) as usize];
        cursor += 2;
    }
    &encoded[..cursor] == hex.as_bytes()
}

fn pack_file_starts_with_pack_magic(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut magic = [0_u8; 4];
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(&magic == b"PACK"),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(CliError::Io(error)),
    }
}

enum PackBody {
    Empty,
    File { path: PathBuf, remove_on_drop: bool },
}

impl PackBody {
    fn len(&self) -> Result<usize> {
        match self {
            Self::Empty => Ok(0),
            Self::File { path, .. } => {
                file_len_usize(path, "pack file is too large for this platform")
            }
        }
    }

    fn write_len_to<W: Write>(&self, writer: &mut W, len: usize) -> Result<()> {
        match self {
            Self::Empty => Ok(()),
            Self::File { path, .. } => {
                let mut file = fs::File::open(path)?;
                copy_exact_len(&mut file, writer, len)?;
                Ok(())
            }
        }
    }
}

impl Drop for PackBody {
    fn drop(&mut self) {
        if let Self::File {
            path,
            remove_on_drop: true,
        } = self
        {
            let _ = fs::remove_file(path);
        }
    }
}

fn copy_stream<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<u64> {
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let len = reader.read(&mut buffer)?;
        if len == 0 {
            return Ok(copied);
        }
        writer.write_all(&buffer[..len])?;
        copied = copied
            .checked_add(len as u64)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "stream length overflow".into(),
            })?;
    }
}

fn copy_exact_len<R: Read, W: Write>(reader: &mut R, writer: &mut W, len: usize) -> Result<()> {
    copy_exact_len_with_message(reader, writer, len, "request body file ended early")
}

fn copy_exact_len_with_message<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    len: usize,
    early_eof_message: &'static str,
) -> Result<()> {
    let mut remaining = len;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let read_len = remaining.min(buffer.len());
        reader
            .read_exact(&mut buffer[..read_len])
            .map_err(|error| {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    CliError::Fatal {
                        code: 128,
                        message: early_eof_message.into(),
                    }
                } else {
                    CliError::Io(error)
                }
            })?;
        writer.write_all(&buffer[..read_len])?;
        remaining -= read_len;
    }
    Ok(())
}

fn copy_http_response_body_to_writer<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    content_length: Option<usize>,
    chunked: bool,
    early_eof_message: &'static str,
) -> Result<u64> {
    if !chunked && let Some(len) = content_length {
        copy_exact_len_with_message(reader, writer, len, early_eof_message)?;
        return Ok(len as u64);
    }
    copy_stream(reader, writer)
}

fn file_len_usize(path: &Path, error_message: &'static str) -> Result<usize> {
    usize::try_from(fs::metadata(path)?.len()).map_err(|_| CliError::Fatal {
        code: 128,
        message: error_message.into(),
    })
}

fn write_push_pack_to_temp_file(
    repo: &GitRepo,
    store: &LooseObjectStore,
    object_ids: &[ObjectId],
) -> Result<PackBody> {
    if object_ids.is_empty() {
        return Ok(PackBody::Empty);
    }
    let (temp_pack, file) = temp_http_pack_file(&repo.objects_dir)?;
    let result = (|| {
        let mut file = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, file);
        let packed_first_store = store.packed_first();
        write_undeltified_pack_from_store(
            &packed_first_store,
            GitHashAlgorithm::Sha1,
            object_ids,
            &mut file,
        )?;
        file.flush()?;
        Ok(PackBody::File {
            path: temp_pack.clone(),
            remove_on_drop: true,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result
}

fn write_push_pack_to_writer<W: Write>(
    store: &LooseObjectStore,
    object_ids: &[ObjectId],
    writer: &mut W,
) -> Result<()> {
    if object_ids.is_empty() {
        return Ok(());
    }
    let packed_first_store = store.packed_first();
    write_undeltified_pack_from_store(
        &packed_first_store,
        GitHashAlgorithm::Sha1,
        object_ids,
        writer,
    )?;
    Ok(())
}

fn install_temp_pack_file(
    path: &Path,
    temp_pack_path: &Path,
    expected_pack_id: &ObjectId,
) -> Result<()> {
    match fs::metadata(path) {
        Ok(_) => {
            let existing = index_pack_file_index_only(GitHashAlgorithm::Sha1, path)?;
            if existing.pack_id == *expected_pack_id {
                let _ = fs::remove_file(temp_pack_path);
                return Ok(());
            }
            return Err(CliError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists with different contents", path.display()),
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    }
    match fs::hard_link(temp_pack_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(temp_pack_path);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(temp_pack_path);
            if index_pack_file_index_only(GitHashAlgorithm::Sha1, path)
                .is_ok_and(|existing| existing.pack_id == *expected_pack_id)
            {
                Ok(())
            } else {
                Err(CliError::Io(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{} already exists with different contents", path.display()),
                )))
            }
        }
        Err(error) => {
            let _ = fs::remove_file(temp_pack_path);
            Err(CliError::Io(error))
        }
    }
}

fn temp_http_pack_path(objects_dir: &std::path::Path) -> Result<PathBuf> {
    let (path, file) = temp_http_pack_file(objects_dir)?;
    drop(file);
    Ok(path)
}

fn temp_http_pack_file(objects_dir: &std::path::Path) -> Result<(PathBuf, fs::File)> {
    let pack_dir = objects_dir.join("pack");
    fs::create_dir_all(&pack_dir)?;
    for attempt in 0..1000_u32 {
        let path = pack_dir.join(format!(
            "tmp_http_pack_{}_{attempt}.pack",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Err(CliError::Fatal {
        code: 128,
        message: "could not allocate temporary HTTP pack path".into(),
    })
}

fn loose_object_path(objects_dir: &std::path::Path, hex: &str) -> Result<PathBuf> {
    if hex.len() != 40 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid object id: {hex}"),
        });
    }
    Ok(objects_dir.join(&hex[..2]).join(&hex[2..]))
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedHttpUrl {
    scheme: HttpScheme,
    host: String,
    port: u16,
    path: String,
    authorization: Option<String>,
    extra_headers: Vec<String>,
    tls_no_verify: bool,
    ca_file: Option<PathBuf>,
    client_cert_file: Option<PathBuf>,
    client_key_file: Option<PathBuf>,
    proxy: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpScheme {
    Http,
    Https,
}

impl HttpScheme {
    fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https => 443,
        }
    }
}

#[derive(Debug, Default)]
struct HttpUrlConfig {
    authorization: Option<String>,
    headers: Vec<String>,
    tls_no_verify: bool,
    ca_file: Option<PathBuf>,
    client_cert_file: Option<PathBuf>,
    client_key_file: Option<PathBuf>,
    proxy: Option<String>,
}

#[derive(Debug, Default)]
struct HttpCredentialConfig {
    helpers: Vec<String>,
    username: Option<String>,
    use_http_path: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpCredentialStoreRow {
    protocol: String,
    host: String,
    path: Option<String>,
    username: String,
    password: String,
}

impl ParsedHttpUrl {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let Some((scheme, rest)) = value.split_once("://") else {
            return Err(CliError::Fatal {
                code: 128,
                message: "HTTP transport supports http:// and https:// URLs".into(),
            });
        };
        let scheme = if scheme.eq_ignore_ascii_case("http") {
            HttpScheme::Http
        } else if scheme.eq_ignore_ascii_case("https") {
            HttpScheme::Https
        } else {
            return Err(CliError::Fatal {
                code: 128,
                message: "HTTP transport supports http:// and https:// URLs".into(),
            });
        };
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (authorization, host_authority) = parse_http_url_authorization(authority)?
            .map_or((None, authority), |(auth, host)| (Some(auth), host));
        let (host, port) = if host_authority.starts_with('[') {
            let Some(closing) = host_authority.find(']') else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "invalid HTTP URL authority: unmatched ']' in IPv6 host".into(),
                });
            };
            if closing == 0 {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "invalid HTTP URL host".into(),
                });
            }
            let host = host_authority[..=closing].to_owned();
            let tail = &host_authority[closing + 1..];
            let port = if tail.is_empty() {
                scheme.default_port()
            } else if let Some(port) = tail.strip_prefix(':') {
                parse_http_port(port)?
            } else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "invalid HTTP URL host".into(),
                });
            };
            (host, port)
        } else if let Some((host, port)) = host_authority.rsplit_once(':') {
            if host.is_empty() || host.contains(':') {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "invalid HTTP URL host".into(),
                });
            }
            (host.to_owned(), parse_http_port(port)?)
        } else {
            (host_authority.to_owned(), scheme.default_port())
        };
        if host.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "HTTP URL host cannot be empty".into(),
            });
        }
        if host.starts_with('[') && !host.ends_with(']') {
            return Err(CliError::Fatal {
                code: 128,
                message: "invalid HTTP URL authority: unmatched ']' in IPv6 host".into(),
            });
        }
        Ok(Self {
            scheme,
            host,
            port,
            path: format!("/{path}").trim_end_matches('/').to_owned(),
            authorization,
            extra_headers: Vec::new(),
            tls_no_verify: false,
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
            proxy: None,
        })
    }

    fn with_http_config(mut self, config: HttpUrlConfig) -> Self {
        if self.authorization.is_none() {
            self.authorization = config.authorization;
        }
        self.extra_headers = config.headers;
        self.tls_no_verify = config.tls_no_verify;
        self.ca_file = config.ca_file;
        self.client_cert_file = config.client_cert_file;
        self.client_key_file = config.client_key_file;
        self.proxy = config.proxy;
        self
    }

    fn is_default_port(&self) -> bool {
        self.port == self.scheme.default_port()
    }

    fn write_path_with_suffix<W: Write>(&self, writer: &mut W, suffix: &str) -> io::Result<()> {
        if suffix.is_empty() {
            if self.path.is_empty() {
                writer.write_all(b"/")
            } else {
                writer.write_all(self.path.as_bytes())
            }
        } else if self.path.is_empty() {
            writer.write_all(b"/")?;
            writer.write_all(suffix.as_bytes())
        } else {
            writer.write_all(self.path.as_bytes())?;
            writer.write_all(b"/")?;
            writer.write_all(suffix.as_bytes())
        }
    }

    fn write_host_header<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        if self.is_default_port() {
            writer.write_all(self.host.as_bytes())
        } else {
            writer.write_all(self.host.as_bytes())?;
            writer.write_all(b":")?;
            write_decimal_usize(writer, usize::from(self.port))
        }
    }

    fn write_full_url_with_suffix<W: Write>(&self, writer: &mut W, suffix: &str) -> io::Result<()> {
        match self.scheme {
            HttpScheme::Http => writer.write_all(b"http://")?,
            HttpScheme::Https => writer.write_all(b"https://")?,
        }
        self.write_host_header(writer)?;
        self.write_path_with_suffix(writer, suffix)
    }

    fn full_url_with_suffix_len(&self, suffix: &str) -> usize {
        self.scheme_prefix_len() + self.host_header_len() + self.path_with_suffix_len(suffix)
    }

    fn scheme_prefix_len(&self) -> usize {
        match self.scheme {
            HttpScheme::Http => "http://".len(),
            HttpScheme::Https => "https://".len(),
        }
    }

    fn host_header_len(&self) -> usize {
        self.host.len()
            + if self.is_default_port() {
                0
            } else {
                1 + decimal_len(usize::from(self.port))
            }
    }

    fn scheme_name(&self) -> &'static str {
        match self.scheme {
            HttpScheme::Http => "http",
            HttpScheme::Https => "https",
        }
    }

    fn credential_host(&self) -> String {
        self.host_header_string()
    }

    fn host_header_string(&self) -> String {
        if self.is_default_port() {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn connect_host(&self) -> &str {
        self.host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(&self.host)
    }

    fn path_with_suffix_len(&self, suffix: &str) -> usize {
        if suffix.is_empty() {
            if self.path.is_empty() {
                1
            } else {
                self.path.len()
            }
        } else if self.path.is_empty() {
            1 + suffix.len()
        } else {
            self.path.len() + 1 + suffix.len()
        }
    }

    fn path_with_suffix_string(&self, suffix: &str) -> Result<String> {
        let mut out = Vec::with_capacity(self.path_with_suffix_len(suffix));
        self.write_path_with_suffix(&mut out, suffix)?;
        String::from_utf8(out).map_err(|_| CliError::Fatal {
            code: 128,
            message: "HTTP redirect path is not valid UTF-8".into(),
        })
    }
}

fn parsed_http_url_with_extra_headers(
    repo: Option<&GitRepo>,
    value: &str,
) -> Result<ParsedHttpUrl> {
    let url = ParsedHttpUrl::parse(value)?;
    let config = http_config_for_url(repo, &url)?;
    Ok(url.with_http_config(config))
}

fn http_config_for_url(repo: Option<&GitRepo>, url: &ParsedHttpUrl) -> Result<HttpUrlConfig> {
    let mut user_agent = std::env::var("GIT_HTTP_USER_AGENT")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let env_user_agent = user_agent.is_some();
    let mut extra_headers = Vec::with_capacity(HTTP_EXTRA_HEADER_CAPACITY_HINT);
    let mut tls_no_verify = false;
    let mut ca_file = None;
    let mut client_cert_file = None;
    let mut client_key_file = None;
    let mut proxy = None;
    let mut credential_config = HttpCredentialConfig {
        helpers: Vec::with_capacity(HTTP_CREDENTIAL_HELPER_CAPACITY_HINT),
        username: None,
        use_http_path: false,
    };
    if let Some(repo) = repo {
        let entries = read_config_entries(repo)?;
        for entry in &entries {
            if entry.section != "http" {
                continue;
            }
            if !entry.subsection.is_empty()
                && !http_config_subsection_matches_url(&entry.subsection, url)
            {
                continue;
            }
            if entry.key == "extraheader" {
                if entry.value.is_empty() {
                    extra_headers.clear();
                } else {
                    extra_headers.push(entry.value.clone());
                }
            } else if entry.key == "useragent" && !env_user_agent {
                if entry.value.is_empty() {
                    user_agent = None;
                } else {
                    user_agent = Some(entry.value.clone());
                }
            } else if entry.key == "sslverify" {
                if let Some(value) = entry.bool_value() {
                    tls_no_verify = !value;
                }
            } else if entry.key == "sslcainfo" {
                ca_file = http_config_path_value(&entry.value);
            } else if entry.key == "sslcert" {
                client_cert_file = http_config_path_value(&entry.value);
            } else if entry.key == "sslkey" {
                client_key_file = http_config_path_value(&entry.value);
            } else if entry.key == "proxy" {
                proxy = http_config_string_value(&entry.value);
            }
        }
        credential_config = http_credential_config_for_url(&entries, url);
    }
    let mut headers = Vec::with_capacity(extra_headers.len() + usize::from(user_agent.is_some()));
    if let Some(user_agent) = user_agent
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        headers.push(format!("User-Agent: {user_agent}"));
    }
    headers.extend(extra_headers);
    let authorization = http_authorization_from_credential_config(url, &credential_config)?;
    Ok(HttpUrlConfig {
        authorization,
        headers,
        tls_no_verify,
        ca_file,
        client_cert_file,
        client_key_file,
        proxy,
    })
}

fn http_config_path_value(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn http_config_string_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn http_credential_config_for_url(
    entries: &[ConfigEntry],
    url: &ParsedHttpUrl,
) -> HttpCredentialConfig {
    let mut config = HttpCredentialConfig {
        helpers: Vec::with_capacity(HTTP_CREDENTIAL_HELPER_CAPACITY_HINT),
        username: None,
        use_http_path: false,
    };
    for entry in entries {
        if entry.section != "credential" {
            continue;
        }
        if !entry.subsection.is_empty()
            && !http_config_subsection_matches_url(&entry.subsection, url)
        {
            continue;
        }
        if entry.key == "helper" {
            if entry.value.is_empty() {
                config.helpers.clear();
            } else {
                config.helpers.push(entry.value.clone());
            }
        } else if entry.key == "username" {
            config.username = http_config_string_value(&entry.value);
        } else if entry.key == "usehttppath"
            && let Some(value) = entry.bool_value()
        {
            config.use_http_path = value;
        }
    }
    config
}

fn http_authorization_from_credential_config(
    url: &ParsedHttpUrl,
    config: &HttpCredentialConfig,
) -> Result<Option<String>> {
    if config.helpers.is_empty() {
        return Ok(None);
    }
    for helper in config.helpers.iter().rev() {
        let Some(row) = http_credential_from_store_helper(url, config, helper)? else {
            continue;
        };
        let credentials = format!("{}:{}", row.username, row.password);
        return Ok(Some(format!(
            "Basic {}",
            base64_encode(credentials.as_bytes())
        )));
    }
    Ok(None)
}

fn http_credential_from_store_helper(
    url: &ParsedHttpUrl,
    config: &HttpCredentialConfig,
    helper: &str,
) -> Result<Option<HttpCredentialStoreRow>> {
    let Some(path) = http_credential_store_helper_path(helper)? else {
        return Ok(None);
    };
    let rows = http_read_credential_store_rows(&path)?;
    let host = url.credential_host();
    let path = config.use_http_path.then(|| url.path.clone());
    for row in rows.iter().rev() {
        if http_credential_store_row_matches(row, url.scheme_name(), &host, path.as_deref())
            && config
                .username
                .as_deref()
                .is_none_or(|username| row.username == username)
        {
            return Ok(Some(row.clone()));
        }
    }
    Ok(None)
}

fn http_credential_store_helper_path(helper: &str) -> Result<Option<PathBuf>> {
    let mut parts = helper.split_whitespace();
    let Some(name) = parts.next() else {
        return Ok(None);
    };
    if name != "store" {
        return Ok(None);
    }
    let mut file = None;
    while let Some(part) = parts.next() {
        if part == "--file" {
            let Some(path) = parts.next() else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "credential-store helper is missing --file path".into(),
                });
            };
            file = Some(PathBuf::from(path));
        } else if let Some(path) = part.strip_prefix("--file=") {
            file = Some(PathBuf::from(path));
        }
    }
    if let Some(file) = file {
        return Ok(Some(file));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "credential-store requires HOME or --file".into(),
    })?;
    Ok(Some(PathBuf::from(home).join(".git-credentials")))
}

fn http_read_credential_store_rows(path: &Path) -> Result<Vec<HttpCredentialStoreRow>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    Ok(content
        .lines()
        .filter_map(http_parse_credential_store_row)
        .collect())
}

fn http_parse_credential_store_row(line: &str) -> Option<HttpCredentialStoreRow> {
    let (protocol, rest) = line.split_once("://")?;
    let (user_pass, host_path) = rest.rsplit_once('@')?;
    let (username, password) = user_pass.split_once(':')?;
    let (host, path) = host_path
        .split_once('/')
        .map(|(host, path)| (host, Some(format!("/{path}"))))
        .unwrap_or((host_path, None));
    Some(HttpCredentialStoreRow {
        protocol: protocol.to_owned(),
        host: host.to_owned(),
        path,
        username: percent_decode_http_credential_value(username).ok()?,
        password: percent_decode_http_credential_value(password).ok()?,
    })
}

fn percent_decode_http_credential_value(value: &str) -> Result<String> {
    percent_decode_http_userinfo(value)
}

fn http_credential_store_row_matches(
    row: &HttpCredentialStoreRow,
    protocol: &str,
    host: &str,
    path: Option<&str>,
) -> bool {
    row.protocol == protocol
        && row.host == host
        && match path {
            Some(path) => row.path.as_deref().is_none_or(|row_path| row_path == path),
            None => true,
        }
}

fn http_config_subsection_matches_url(subsection: &str, url: &ParsedHttpUrl) -> bool {
    let Some(config_url) = ParsedHttpUrl::parse(subsection).ok() else {
        return false;
    };
    config_url.scheme == url.scheme
        && config_url.host == url.host
        && config_url.port == url.port
        && http_config_path_matches_url(&config_url.path, &url.path)
}

fn http_config_path_matches_url(config_path: &str, url_path: &str) -> bool {
    config_path == "/"
        || config_path == url_path
        || url_path
            .strip_prefix(config_path)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn parse_http_port(port: &str) -> Result<u16> {
    port.parse::<u16>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("invalid HTTP URL port: {port}"),
    })
}

fn parse_http_url_authorization<'a>(authority: &'a str) -> Result<Option<(String, &'a str)>> {
    let Some((userinfo, host_authority)) = authority.rsplit_once('@') else {
        return Ok(None);
    };
    if userinfo.is_empty() {
        return Ok(None);
    }
    let credentials = percent_decode_http_userinfo(userinfo)?;
    let credentials = if credentials.contains(':') {
        credentials
    } else {
        format!("{credentials}:")
    };
    Ok(Some((
        format!("Basic {}", base64_encode(credentials.as_bytes())),
        host_authority,
    )))
}

fn percent_decode_http_userinfo(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(high) = bytes.get(index + 1) else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("HTTP URL userinfo percent escape is invalid: {value}"),
                });
            };
            let Some(low) = bytes.get(index + 2) else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("HTTP URL userinfo percent escape is invalid: {value}"),
                });
            };
            out.push(
                decode_percent_hex_byte(*high, *low).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("HTTP URL userinfo percent escape is invalid: {value}"),
                })?,
            );
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| CliError::Fatal {
        code: 128,
        message: "HTTP URL userinfo is not valid UTF-8".into(),
    })
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let chunk = &bytes[index..index + 3];
        out.push(TABLE[(chunk[0] >> 2) as usize] as char);
        out.push(TABLE[(((chunk[0] & 0x03) << 4) | (chunk[1] >> 4)) as usize] as char);
        out.push(TABLE[(((chunk[1] & 0x0f) << 2) | (chunk[2] >> 6)) as usize] as char);
        out.push(TABLE[(chunk[2] & 0x3f) as usize] as char);
        index += 3;
    }
    match bytes.len() - index {
        1 => {
            let byte = bytes[index];
            out.push(TABLE[(byte >> 2) as usize] as char);
            out.push(TABLE[((byte & 0x03) << 4) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let first = bytes[index];
            let second = bytes[index + 1];
            out.push(TABLE[(first >> 2) as usize] as char);
            out.push(TABLE[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
            out.push(TABLE[((second & 0x0f) << 2) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

pub(crate) fn is_http_transport_url(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once("://") else {
        return false;
    };
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

fn http_get_to_file_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    suffix: &str,
    path: &Path,
) -> Result<()> {
    let head = helper.request_to_file(url, "GET", suffix, &[], &PackBody::Empty, path)?;
    if head.status_code != 200 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP object request failed: {}", head.status_line),
        });
    }
    Ok(())
}

fn http_get_optional_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    suffix: &str,
) -> Result<Option<Vec<u8>>> {
    let response = helper.request_to_body(url, "GET", suffix, &[], &PackBody::Empty)?;
    match response.status_code {
        200 => {
            let body_len = response.body_len;
            Ok(Some(response.body.into_vec(body_len)?))
        }
        404 => Ok(None),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP ref request failed: {}", response.status_line),
        }),
    }
}

fn http_put_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    suffix: &str,
    body: &[u8],
) -> Result<()> {
    let response = helper.request_to_body(url, "PUT", suffix, body, &PackBody::Empty)?;
    if matches!(response.status_code, 200 | 201 | 204) {
        response.body.with_reader(|response_body| {
            drain_http_response_body(response_body)?;
            Ok(())
        })?;
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP PUT failed: {}", response.status_line),
        })
    }
}

fn http_put_direct(url: &ParsedHttpUrl, suffix: &str, body: &[u8]) -> Result<()> {
    let (head, mut response_body) = http_request_reader(url, "PUT", suffix, body)?;
    drain_http_response_body(&mut response_body)?;
    if matches!(head.status_code, 200 | 201 | 204) {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP PUT failed: {}", head.status_line),
        })
    }
}

fn http_put_body_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    suffix: &str,
    body: &PackBody,
) -> Result<()> {
    let response = helper.request_to_body(url, "PUT", suffix, &[], body)?;
    if matches!(response.status_code, 200 | 201 | 204) {
        response.body.with_reader(|response_body| {
            drain_http_response_body(response_body)?;
            Ok(())
        })?;
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP PUT failed: {}", response.status_line),
        })
    }
}

fn http_put_body_direct(url: &ParsedHttpUrl, suffix: &str, body: &PackBody) -> Result<()> {
    let (head, mut response_body) = http_request_reader_parts(url, "PUT", suffix, &[], body)?;
    drain_http_response_body(&mut response_body)?;
    if matches!(head.status_code, 200 | 201 | 204) {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP PUT failed: {}", head.status_line),
        })
    }
}

fn http_delete_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    suffix: &str,
) -> Result<()> {
    let response = helper.request_to_body(url, "DELETE", suffix, &[], &PackBody::Empty)?;
    if matches!(response.status_code, 200 | 202 | 204 | 404) {
        response.body.with_reader(|response_body| {
            drain_http_response_body(response_body)?;
            Ok(())
        })?;
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP DELETE failed: {}", response.status_line),
        })
    }
}

fn http_delete_direct(url: &ParsedHttpUrl, suffix: &str) -> Result<()> {
    let (head, mut response_body) = http_request_reader(url, "DELETE", suffix, &[])?;
    drain_http_response_body(&mut response_body)?;
    if matches!(head.status_code, 200 | 202 | 204 | 404) {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP DELETE failed: {}", head.status_line),
        })
    }
}

#[derive(Debug)]
struct HttpResponseHead {
    status_code: u16,
    status_line: HttpStatusLine,
    chunked: bool,
    content_length: Option<usize>,
    location: Option<String>,
}

#[derive(Debug)]
enum HttpStatusLine {
    Raw(String),
    Parts { version: String, status: String },
}

impl HttpStatusLine {
    fn raw(line: String) -> Self {
        Self::Raw(line)
    }

    fn parts(version: String, status: String) -> Self {
        Self::Parts { version, status }
    }

    #[cfg(test)]
    fn raw_capacity(&self) -> Option<usize> {
        match self {
            Self::Raw(line) => Some(line.capacity()),
            Self::Parts { .. } => None,
        }
    }
}

impl std::fmt::Display for HttpStatusLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Raw(line) => f.write_str(line),
            Self::Parts { version, status } => {
                f.write_str("HTTP/")?;
                f.write_str(version)?;
                f.write_str(" ")?;
                f.write_str(status)
            }
        }
    }
}

enum HttpBodyReader {
    ContentLength(FixedLengthHttpBody<io::BufReader<std::net::TcpStream>>),
    Chunked(ChunkedHttpBody<io::BufReader<std::net::TcpStream>>),
    ConnectionClose(io::BufReader<std::net::TcpStream>),
    File {
        reader: io::BufReader<fs::File>,
        path: PathBuf,
    },
}

impl Read for HttpBodyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::ContentLength(reader) => reader.read(buf),
            Self::Chunked(reader) => reader.read(buf),
            Self::ConnectionClose(reader) => reader.read(buf),
            Self::File { reader, .. } => reader.read(buf),
        }
    }
}

impl Drop for HttpBodyReader {
    fn drop(&mut self) {
        if let Self::File { path, .. } = self {
            let _ = fs::remove_file(path);
        }
    }
}

struct ChunkedHttpBody<R> {
    reader: R,
    line: String,
    remaining: usize,
    done: bool,
}

struct FixedLengthHttpBody<R> {
    reader: R,
    remaining: usize,
}

impl<R: Read> Read for FixedLengthHttpBody<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 || buf.is_empty() {
            return Ok(0);
        }
        let read_len = buf.len().min(self.remaining);
        let read = self.reader.read(&mut buf[..read_len])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP response ended early",
            ));
        }
        self.remaining -= read;
        Ok(read)
    }
}

impl<R: BufRead> BufRead for FixedLengthHttpBody<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.remaining == 0 {
            return Ok(&[]);
        }
        let buf = self.reader.fill_buf()?;
        if buf.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP response ended early",
            ));
        }
        Ok(&buf[..buf.len().min(self.remaining)])
    }

    fn consume(&mut self, amt: usize) {
        let consumed = amt.min(self.remaining);
        self.reader.consume(consumed);
        self.remaining -= consumed;
    }
}

impl<R: BufRead> Read for ChunkedHttpBody<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            self.remaining = read_http_chunk_size(&mut self.reader, &mut self.line)?;
            if self.remaining == 0 {
                drain_http_chunk_trailers(&mut self.reader, &mut self.line)?;
                self.done = true;
                return Ok(0);
            }
        }
        let read_len = buf.len().min(self.remaining);
        let read = self.reader.read(&mut buf[..read_len])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP chunked response ended early",
            ));
        }
        self.remaining -= read;
        if self.remaining == 0 {
            let mut crlf = [0_u8; 2];
            self.reader.read_exact(&mut crlf)?;
            if crlf != *b"\r\n" {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP chunk is missing terminator",
                ));
            }
        }
        Ok(read)
    }
}

fn http_request_reader(
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
    body: &[u8],
) -> Result<(HttpResponseHead, HttpBodyReader)> {
    http_request_reader_parts(url, method, suffix, body, &PackBody::Empty)
}

fn http_request_reader_parts(
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
    first_body: &[u8],
    second_body: &PackBody,
) -> Result<(HttpResponseHead, HttpBodyReader)> {
    http_request_reader_parts_redirects(url, method, suffix, first_body, second_body, 0)
}

fn http_request_reader_parts_redirects(
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
    first_body: &[u8],
    second_body: &PackBody,
    redirect_count: usize,
) -> Result<(HttpResponseHead, HttpBodyReader)> {
    if redirect_count > HTTP_REDIRECT_LIMIT {
        return Err(CliError::Fatal {
            code: 128,
            message: "HTTP redirect limit exceeded".into(),
        });
    }
    if url.scheme == HttpScheme::Https {
        let temp_body = temp_http_helper_output_path()?;
        let mut helper = RemoteHttpHelperSession::spawn(&url)?;
        match helper.request_to_file(url, method, suffix, first_body, second_body, &temp_body) {
            Ok(head) => {
                if is_http_redirect_status(head.status_code) {
                    let Some(location) = head.location.as_deref() else {
                        let _ = fs::remove_file(&temp_body);
                        return Err(CliError::Fatal {
                            code: 128,
                            message: format!(
                                "HTTP redirect missing Location header: {}",
                                head.status_line
                            ),
                        });
                    };
                    let target = http_redirect_target_url(url, suffix, location)?;
                    let _ = fs::remove_file(&temp_body);
                    return http_request_reader_parts_redirects(
                        &target,
                        method,
                        "",
                        first_body,
                        second_body,
                        redirect_count + 1,
                    );
                }
                let reader = http_helper_file_body_reader(fs::File::open(&temp_body)?);
                return Ok((
                    head,
                    HttpBodyReader::File {
                        reader,
                        path: temp_body,
                    },
                ));
            }
            Err(error) => {
                let _ = fs::remove_file(&temp_body);
                return Err(error);
            }
        }
    }
    let mut stream = std::net::TcpStream::connect((url.connect_host(), url.port))?;
    {
        let mut writer = io::BufWriter::with_capacity(HTTP_DIRECT_WRITE_BUF_CAPACITY, &mut stream);
        write_http_request_parts(&mut writer, url, method, suffix, first_body, second_body)?;
        writer.flush()?;
    }
    let mut reader = io::BufReader::with_capacity(HTTP_DIRECT_READ_BUF_CAPACITY, stream);
    let head = read_http_response_head(&mut reader)?;
    if is_http_redirect_status(head.status_code) {
        let Some(location) = head.location.as_deref() else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "HTTP redirect missing Location header: {}",
                    head.status_line
                ),
            });
        };
        let target = http_redirect_target_url(url, suffix, location)?;
        return http_request_reader_parts_redirects(
            &target,
            method,
            "",
            first_body,
            second_body,
            redirect_count + 1,
        );
    }
    let body = if head.chunked {
        HttpBodyReader::Chunked(ChunkedHttpBody {
            reader,
            line: String::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT),
            remaining: 0,
            done: false,
        })
    } else if let Some(content_length) = head.content_length {
        HttpBodyReader::ContentLength(FixedLengthHttpBody {
            reader,
            remaining: content_length,
        })
    } else {
        HttpBodyReader::ConnectionClose(reader)
    };
    Ok((head, body))
}

fn is_http_redirect_status(status_code: u16) -> bool {
    matches!(status_code, 301 | 302 | 303 | 307 | 308)
}

fn http_redirect_target_url(
    url: &ParsedHttpUrl,
    suffix: &str,
    location: &str,
) -> Result<ParsedHttpUrl> {
    let location = location.trim();
    if location.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "HTTP redirect Location is empty".into(),
        });
    }
    let target = if is_http_transport_url(location) {
        ParsedHttpUrl::parse(location)?
    } else if location.starts_with("//") {
        let mut absolute = String::new();
        absolute.push_str(url.scheme_name());
        absolute.push(':');
        absolute.push_str(location);
        ParsedHttpUrl::parse(&absolute)?
    } else if location.starts_with('/') {
        let mut absolute = String::new();
        absolute.push_str(url.scheme_name());
        absolute.push_str("://");
        absolute.push_str(&url.host_header_string());
        absolute.push_str(location);
        ParsedHttpUrl::parse(&absolute)?
    } else {
        let mut absolute = String::new();
        absolute.push_str(url.scheme_name());
        absolute.push_str("://");
        absolute.push_str(&url.host_header_string());
        let current_path = url.path_with_suffix_string(suffix)?;
        let base = current_path
            .rsplit_once('/')
            .map_or("/", |(base, _)| if base.is_empty() { "/" } else { base });
        absolute.push_str(base);
        if !absolute.ends_with('/') {
            absolute.push('/');
        }
        absolute.push_str(location);
        ParsedHttpUrl::parse(&absolute)?
    };
    Ok(target.with_redirect_config_from(url))
}

impl ParsedHttpUrl {
    fn with_redirect_config_from(mut self, previous: &ParsedHttpUrl) -> Self {
        let same_origin = self.scheme == previous.scheme
            && self.host == previous.host
            && self.port == previous.port;
        self.authorization = same_origin
            .then(|| previous.authorization.clone())
            .flatten()
            .or(self.authorization);
        self.extra_headers = if same_origin {
            previous.extra_headers.clone()
        } else {
            previous
                .extra_headers
                .iter()
                .filter(|header| !http_extra_header_is_credential(header))
                .cloned()
                .collect()
        };
        self.tls_no_verify = previous.tls_no_verify;
        self.ca_file = previous.ca_file.clone();
        self.client_cert_file = previous.client_cert_file.clone();
        self.client_key_file = previous.client_key_file.clone();
        self.proxy = previous.proxy.clone();
        self
    }
}

fn http_extra_header_is_credential(header: &str) -> bool {
    let Some((name, _)) = header.split_once(':') else {
        return false;
    };
    let name = name.trim();
    name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("proxy-authorization")
}

pub(crate) struct RemoteHttpHelperSession {
    child: std::process::Child,
    stdin: io::BufWriter<std::process::ChildStdin>,
    stdout: io::BufReader<std::process::ChildStdout>,
    line: String,
    request_frame: Vec<u8>,
}

fn append_http_helper_request_frame_head(
    request_frame: &mut Vec<u8>,
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
    output_file: Option<&Path>,
) -> Result<()> {
    request_frame.extend_from_slice(b"REQUEST\nMETHOD ");
    request_frame.extend_from_slice(method.as_bytes());
    request_frame.extend_from_slice(b"\nURL ");
    url.write_full_url_with_suffix(request_frame, suffix)?;
    request_frame.push(b'\n');
    if let Some(path) = output_file {
        request_frame.extend_from_slice(b"OUTPUT-FILE ");
        write!(request_frame, "{}", path.display())?;
        request_frame.push(b'\n');
    }
    if let Some(authorization) = url.authorization.as_deref() {
        request_frame.extend_from_slice(b"HEADER Authorization: ");
        request_frame.extend_from_slice(authorization.as_bytes());
        request_frame.push(b'\n');
    }
    for header in &url.extra_headers {
        request_frame.extend_from_slice(b"HEADER ");
        request_frame.extend_from_slice(header.as_bytes());
        request_frame.push(b'\n');
    }
    Ok(())
}

fn append_http_helper_request_frame(
    request_frame: &mut Vec<u8>,
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
    first_body_len: usize,
    second_body: &PackBody,
    output_file: Option<&Path>,
) -> Result<bool> {
    request_frame.clear();
    let needed = http_helper_request_frame_capacity(
        url,
        method,
        suffix,
        first_body_len,
        second_body,
        output_file,
    );
    if request_frame.capacity() < needed {
        request_frame.reserve_exact(needed.saturating_sub(request_frame.len()));
    }
    append_http_helper_request_frame_head(request_frame, url, method, suffix, output_file)?;
    if let Some(content_type) = smart_http_post_content_type(url, method, suffix) {
        request_frame.extend_from_slice(b"HEADER Content-Type: ");
        request_frame.extend_from_slice(content_type.as_bytes());
        request_frame.push(b'\n');
    }
    match second_body {
        PackBody::Empty => {
            if first_body_len == 0 {
                request_frame.push(b'\n');
                Ok(false)
            } else {
                request_frame.extend_from_slice(b"CONTENT-LENGTH ");
                append_decimal_usize(request_frame, first_body_len);
                request_frame.extend_from_slice(b"\n\n");
                Ok(true)
            }
        }
        PackBody::File { path, .. } if first_body_len == 0 => {
            request_frame.extend_from_slice(b"BODY-FILE ");
            write!(request_frame, "{}", path.display())?;
            request_frame.push(b'\n');
            request_frame.push(b'\n');
            Ok(false)
        }
        PackBody::File { path, .. } => {
            request_frame.extend_from_slice(b"BODY-FILE ");
            write!(request_frame, "{}", path.display())?;
            request_frame.extend_from_slice(b"\nBODY-PREFIX-LENGTH ");
            append_decimal_usize(request_frame, first_body_len);
            request_frame.extend_from_slice(b"\n\n");
            Ok(true)
        }
    }
}

fn http_helper_request_frame_capacity(
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
    first_body_len: usize,
    second_body: &PackBody,
    output_file: Option<&Path>,
) -> usize {
    let mut len = "REQUEST\nMETHOD ".len()
        + method.len()
        + "\nURL ".len()
        + url.full_url_with_suffix_len(suffix)
        + 1;
    if let Some(path) = output_file {
        len += "OUTPUT-FILE ".len() + path_display_len(path) + 1;
    }
    if let Some(authorization) = url.authorization.as_deref() {
        len += "HEADER Authorization: ".len() + authorization.len() + 1;
    }
    for header in &url.extra_headers {
        len += "HEADER ".len() + header.len() + 1;
    }
    if let Some(content_type) = smart_http_post_content_type(url, method, suffix) {
        len += "HEADER Content-Type: ".len() + content_type.len() + 1;
    }
    match second_body {
        PackBody::Empty => {
            if first_body_len == 0 {
                len + 1
            } else {
                len + "CONTENT-LENGTH ".len() + decimal_len(first_body_len) + "\n\n".len()
            }
        }
        PackBody::File { path, .. } if first_body_len == 0 => {
            len + "BODY-FILE ".len() + path_display_len(path) + "\n\n".len()
        }
        PackBody::File { path, .. } => {
            len + "BODY-FILE ".len()
                + path_display_len(path)
                + "\nBODY-PREFIX-LENGTH ".len()
                + decimal_len(first_body_len)
                + "\n\n".len()
        }
    }
}

fn path_display_len(path: &Path) -> usize {
    path.as_os_str().as_encoded_bytes().len()
}

fn smart_http_post_content_type(
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
) -> Option<&'static str> {
    if method != "POST" {
        return None;
    }
    if suffix == "git-upload-pack" || url.path.ends_with("/git-upload-pack") {
        Some("application/x-git-upload-pack-request")
    } else if suffix == "git-receive-pack" || url.path.ends_with("/git-receive-pack") {
        Some("application/x-git-receive-pack-request")
    } else {
        None
    }
}

impl RemoteHttpHelperSession {
    pub(crate) fn spawn_for_url(url: &str) -> Result<Self> {
        let parsed_url = parsed_http_url_with_extra_headers(None, url)?;
        Self::spawn(&parsed_url)
    }

    fn spawn(url: &ParsedHttpUrl) -> Result<Self> {
        let helper = remote_http_helper_path()?;
        let mut command = ProcessCommand::new(helper);
        command.arg("--batch");
        if let Some(version) = remote_http_helper_version_arg_for_url(url.scheme, Some(url))? {
            command.arg("--http-version").arg(version);
        }
        if let Some(ca_file) = remote_http_helper_ca_file_arg_for_url(url)? {
            command.arg("--ca-file").arg(ca_file);
        }
        if let Some(client_cert_file) = remote_http_helper_client_cert_file_arg_for_url(url)? {
            command.arg("--client-cert-file").arg(client_cert_file);
        }
        if let Some(client_key_file) = remote_http_helper_client_key_file_arg_for_url(url)? {
            command.arg("--client-key-file").arg(client_key_file);
        }
        if remote_http_helper_tls_no_verify_arg_for_url(url) {
            command.arg("--tls-no-verify");
        }
        if let Some((name, value)) = remote_http_helper_proxy_env_for_url(url) {
            command.env(name, value);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "skron-git-remote-http stdin is unavailable".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "skron-git-remote-http stdout is unavailable".into(),
        })?;
        Ok(Self {
            child,
            stdin: io::BufWriter::with_capacity(HTTP_HELPER_PIPE_BUF_CAPACITY, stdin),
            stdout: io::BufReader::with_capacity(HTTP_HELPER_PIPE_BUF_CAPACITY, stdout),
            line: String::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT),
            request_frame: Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT),
        })
    }

    fn request_to_body(
        &mut self,
        url: &ParsedHttpUrl,
        method: &str,
        suffix: &str,
        first_body: &[u8],
        second_body: &PackBody,
    ) -> Result<HelperHttpResponse> {
        self.request_to_body_redirects(url, method, suffix, first_body, second_body, 0)
    }

    fn request_to_body_redirects(
        &mut self,
        url: &ParsedHttpUrl,
        method: &str,
        suffix: &str,
        first_body: &[u8],
        second_body: &PackBody,
        redirect_count: usize,
    ) -> Result<HelperHttpResponse> {
        if redirect_count > HTTP_REDIRECT_LIMIT {
            return Err(CliError::Fatal {
                code: 128,
                message: "HTTP redirect limit exceeded".into(),
            });
        }
        self.write_request(url, method, suffix, first_body, second_body, None)?;
        let response = self.read_response(None)?;
        if is_http_redirect_status(response.status_code) {
            let Some(location) = response.location.as_deref() else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "HTTP redirect missing Location header: {}",
                        response.status_line
                    ),
                });
            };
            let target = http_redirect_target_url(url, suffix, location)?;
            return self.request_to_body_redirects(
                &target,
                method,
                "",
                first_body,
                second_body,
                redirect_count + 1,
            );
        }
        Ok(response)
    }

    fn request_to_file(
        &mut self,
        url: &ParsedHttpUrl,
        method: &str,
        suffix: &str,
        first_body: &[u8],
        second_body: &PackBody,
        output_file: &Path,
    ) -> Result<HttpResponseHead> {
        self.write_request(
            url,
            method,
            suffix,
            first_body,
            second_body,
            Some(output_file),
        )?;
        let response = self.read_response(Some(output_file))?;
        let head = HttpResponseHead {
            status_code: response.status_code,
            status_line: response.status_line,
            chunked: false,
            content_length: Some(response.body_len),
            location: response.location,
        };
        if response.body.is_inline() && !is_http_redirect_status(head.status_code) {
            return Err(CliError::Fatal {
                code: 128,
                message: "HTTP helper returned an inline body for a file response".into(),
            });
        }
        Ok(head)
    }

    fn write_request(
        &mut self,
        url: &ParsedHttpUrl,
        method: &str,
        suffix: &str,
        first_body: &[u8],
        second_body: &PackBody,
        output_file: Option<&Path>,
    ) -> Result<()> {
        let write_first_body = append_http_helper_request_frame(
            &mut self.request_frame,
            url,
            method,
            suffix,
            first_body.len(),
            second_body,
            output_file,
        )?;
        self.stdin.write_all(&self.request_frame)?;
        if write_first_body {
            self.stdin.write_all(first_body)?;
        }
        self.stdin.flush()?;
        Ok(())
    }

    fn read_response(&mut self, output_file: Option<&Path>) -> Result<HelperHttpResponse> {
        read_helper_line(&mut self.stdout, &mut self.line)?;
        if self.line != "RESPONSE" {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("unexpected HTTP helper response line: {}", self.line),
            });
        }
        let mut version = None;
        let mut status = None;
        let mut content_length = None;
        let mut body_file = None;
        let mut location = None;
        loop {
            read_helper_line(&mut self.stdout, &mut self.line)?;
            if self.line.is_empty() {
                break;
            }
            if let Some(value) = self.line.strip_prefix("VERSION ") {
                update_helper_response_field(&mut version, "VERSION", value)?;
            } else if let Some(value) = self.line.strip_prefix("STATUS ") {
                update_helper_response_field(&mut status, "STATUS", value)?;
            } else if let Some(value) = self.line.strip_prefix("CONTENT-LENGTH ") {
                update_helper_content_length(&mut content_length, value)?;
            } else if let Some(value) = self.line.strip_prefix("BODY-FILE ") {
                update_helper_body_file(&mut body_file, PathBuf::from(value), output_file)?;
            } else if let Some(value) = self.line.strip_prefix("HEADER ") {
                update_helper_response_header(&mut location, value);
            } else {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("unexpected HTTP helper response header: {}", self.line),
                });
            }
        }
        let version = version.ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "HTTP helper response is missing VERSION".into(),
        })?;
        let status = status.ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "HTTP helper response is missing STATUS".into(),
        })?;
        let status_code = parse_helper_status_code(&status)?;
        let has_body_file = body_file.is_some();
        let body_len = helper_response_body_len(content_length, has_body_file)?;
        let body = if let Some(body_file) = body_file {
            helper_file_response_body(body_file, body_len)?
        } else {
            let mut body = Vec::new();
            if body_len > 0 {
                if body_len > HTTP_HELPER_INLINE_BODY_LIMIT {
                    return Err(CliError::Fatal {
                        code: 128,
                        message: format!(
                            "HTTP helper returned an inline body larger than {} bytes; expected BODY-FILE",
                            HTTP_HELPER_INLINE_BODY_LIMIT
                        ),
                    });
                }
                body = read_exact_helper_body(
                    &mut self.stdout,
                    body_len,
                    "HTTP helper response ended before completing inline body",
                )?;
            }
            let mut trailing_lf = [0_u8; 1];
            self.stdout.read_exact(&mut trailing_lf)?;
            if trailing_lf != *b"\n" {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "HTTP helper response body is missing frame terminator".into(),
                });
            }
            HelperHttpBody::Memory(body)
        };
        Ok(HelperHttpResponse {
            status_code,
            status_line: HttpStatusLine::parts(version, status),
            body,
            body_len,
            location,
        })
    }
}

fn update_helper_response_field(
    field: &mut Option<String>,
    name: &'static str,
    value: &str,
) -> Result<()> {
    if field.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP helper returned duplicate {name} header"),
        });
    }
    *field = Some(value.to_owned());
    Ok(())
}

fn update_helper_body_file(
    body_file: &mut Option<HelperHttpFileBody>,
    path: PathBuf,
    output_file: Option<&Path>,
) -> Result<()> {
    if body_file.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "HTTP helper returned duplicate BODY-FILE header".into(),
        });
    }
    *body_file = Some(helper_http_file_body(path, output_file)?);
    Ok(())
}

fn update_helper_response_header(location: &mut Option<String>, header: &str) {
    if location.is_some() {
        return;
    }
    let Some((name, value)) = header.split_once(':') else {
        return;
    };
    if name.trim().eq_ignore_ascii_case("location") {
        *location = Some(value.trim().to_owned());
    }
}

fn helper_file_response_body(
    body_file: HelperHttpFileBody,
    body_len: usize,
) -> Result<HelperHttpBody> {
    validate_helper_file_body_len(
        fs::metadata(&body_file.path).map_err(CliError::Io)?.len(),
        body_len,
    )?;
    Ok(HelperHttpBody::File(body_file))
}

fn http_helper_file_body_reader(file: fs::File) -> io::BufReader<fs::File> {
    io::BufReader::with_capacity(HTTP_HELPER_FILE_READ_BUF_CAPACITY, file)
}

fn update_helper_content_length(content_length: &mut Option<usize>, value: &str) -> Result<()> {
    let parsed = parse_decimal_usize(value.as_bytes()).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: format!("HTTP helper returned invalid Content-Length: {value}"),
    })?;
    if content_length.is_some_and(|existing| existing != parsed) {
        return Err(CliError::Fatal {
            code: 128,
            message: "HTTP helper returned conflicting Content-Length headers".into(),
        });
    }
    *content_length = Some(parsed);
    Ok(())
}

fn helper_response_body_len(content_length: Option<usize>, has_body_file: bool) -> Result<usize> {
    match (content_length, has_body_file) {
        (Some(len), _) => Ok(len),
        (None, true) => Err(CliError::Fatal {
            code: 128,
            message: "HTTP helper response BODY-FILE is missing Content-Length".into(),
        }),
        (None, false) => Ok(0),
    }
}

fn helper_http_file_body(path: PathBuf, output_file: Option<&Path>) -> Result<HelperHttpFileBody> {
    let remove_on_drop = match output_file {
        Some(output_file) if path == output_file => false,
        Some(output_file) => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "HTTP helper wrote response body to unexpected file: expected {}, got {}",
                    output_file.display(),
                    path.display()
                ),
            });
        }
        None => true,
    };
    Ok(HelperHttpFileBody {
        path,
        remove_on_drop,
    })
}

fn read_exact_helper_body<R: Read>(
    reader: &mut R,
    len: usize,
    early_eof_message: &'static str,
) -> Result<Vec<u8>> {
    let mut body = Vec::with_capacity(helper_inline_body_initial_capacity(len));
    let mut remaining = len;
    while remaining > 0 {
        let read_len = remaining.min(PACK_RECEIPT_BUF_CAPACITY);
        let start = body.len();
        if body.capacity().saturating_sub(start) < read_len {
            body.reserve_exact(read_len);
        }
        let spare = body.spare_capacity_mut();
        // SAFETY: body has at least read_len spare bytes before this slice.
        // The bytes are exposed with set_len only after read_exact succeeds.
        let target =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr().cast::<u8>(), read_len) };
        reader.read_exact(target).map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                CliError::Fatal {
                    code: 128,
                    message: early_eof_message.into(),
                }
            } else {
                CliError::Io(error)
            }
        })?;
        // SAFETY: read_exact initialized exactly read_len bytes after start.
        unsafe {
            body.set_len(start + read_len);
        }
        remaining -= read_len;
    }
    Ok(body)
}

fn helper_inline_body_initial_capacity(len: usize) -> usize {
    len.min(PACK_RECEIPT_BUF_CAPACITY)
}

fn parse_helper_status_code(status: &str) -> Result<u16> {
    let bytes = status.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_digit()
        || !bytes[1].is_ascii_digit()
        || !bytes[2].is_ascii_digit()
        || bytes.get(3).is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP helper response status is malformed: {status}"),
        });
    }
    Ok(u16::from(bytes[0] - b'0') * 100
        + u16::from(bytes[1] - b'0') * 10
        + u16::from(bytes[2] - b'0'))
}

impl Drop for RemoteHttpHelperSession {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "DONE");
        let _ = self.stdin.flush();
        let _ = self.child.wait();
    }
}

struct HelperHttpResponse {
    status_code: u16,
    status_line: HttpStatusLine,
    body: HelperHttpBody,
    body_len: usize,
    location: Option<String>,
}

enum HelperHttpBody {
    Memory(Vec<u8>),
    File(HelperHttpFileBody),
}

struct HelperHttpFileBody {
    path: PathBuf,
    remove_on_drop: bool,
}

impl Drop for HelperHttpFileBody {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl HelperHttpBody {
    fn is_inline(&self) -> bool {
        matches!(self, Self::Memory(body) if !body.is_empty())
    }

    fn with_reader<T>(self, read: impl FnOnce(&mut dyn BufRead) -> Result<T>) -> Result<T> {
        match self {
            Self::Memory(body) => {
                let mut cursor = io::Cursor::new(body);
                read(&mut cursor)
            }
            Self::File(body) => {
                let result = (|| {
                    let file = fs::File::open(&body.path).map_err(CliError::Io)?;
                    let mut reader = http_helper_file_body_reader(file);
                    read(&mut reader)
                })();
                result
            }
        }
    }

    fn into_vec(self, expected_len: usize) -> Result<Vec<u8>> {
        match self {
            Self::Memory(body) => Ok(body),
            Self::File(body) => {
                let result = (|| {
                    let file = fs::File::open(&body.path).map_err(CliError::Io)?;
                    validate_helper_file_body_len(
                        file.metadata().map_err(CliError::Io)?.len(),
                        expected_len,
                    )?;
                    let mut reader = http_helper_file_body_reader(file);
                    read_exact_helper_body(
                        &mut reader,
                        expected_len,
                        "HTTP helper response ended before completing file body",
                    )
                })();
                result
            }
        }
    }
}

fn validate_helper_file_body_len(actual_len: u64, expected_len: usize) -> Result<()> {
    let expected_len = u64::try_from(expected_len).map_err(|_| CliError::Fatal {
        code: 128,
        message: "HTTP helper response Content-Length is too large".into(),
    })?;
    if actual_len != expected_len {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "HTTP helper response BODY-FILE length mismatch: expected {expected_len}, got {actual_len}"
            ),
        });
    }
    Ok(())
}

fn read_helper_line<R: BufRead>(reader: &mut R, line: &mut String) -> Result<()> {
    if read_limited_transport_text_line(reader, line)? == 0 {
        return Err(CliError::Fatal {
            code: 128,
            message: "HTTP helper ended before completing response".into(),
        });
    }
    truncate_line_ending(line);
    Ok(())
}

fn truncate_line_ending(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}

fn remote_http_helper_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SKRON_GIT_REMOTE_HTTP") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()?;
    let helper = current.with_file_name(if cfg!(windows) {
        "skron-git-remote-http.exe"
    } else {
        "skron-git-remote-http"
    });
    if helper.is_file() {
        Ok(helper)
    } else {
        Err(CliError::Fatal {
            code: 128,
            message: format!(
                "skron-git-remote-http helper is required for HTTPS transport: {}",
                helper.display()
            ),
        })
    }
}

fn remote_http_helper_version_arg_for_url(
    scheme: HttpScheme,
    url: Option<&ParsedHttpUrl>,
) -> Result<Option<&'static str>> {
    let Ok(raw) = std::env::var("SKRON_GIT_HTTP_VERSION") else {
        if should_force_http1_for_auto(scheme, url) {
            return Ok(Some("http1"));
        }
        return Ok(None);
    };
    let normalized = raw.trim().to_ascii_lowercase();
    let version = match normalized.as_str() {
        "" | "auto" => {
            if should_force_http1_for_auto(scheme, url) {
                return Ok(Some("http1"));
            }
            return Ok(None);
        }
        "http1" | "http1.1" | "http/1.1" | "1" | "1.1" => "http1",
        "http2" | "http/2" | "h2" | "2" => {
            if matches!(scheme, HttpScheme::Https) {
                "http2"
            } else {
                "http1"
            }
        }
        "http3" | "http/3" | "h3" | "quic" | "3" => {
            if matches!(scheme, HttpScheme::Https) {
                "http3"
            } else {
                "http1"
            }
        }
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "unsupported SKRON_GIT_HTTP_VERSION '{raw}'; expected auto, http1, http2, or http3"
                ),
            });
        }
    };
    Ok(Some(version))
}

fn should_force_http1_for_auto(scheme: HttpScheme, url: Option<&ParsedHttpUrl>) -> bool {
    if !matches!(scheme, HttpScheme::Https) {
        return false;
    }
    let Some(url) = url else {
        return false;
    };
    !auto_http3_probe_host(&url.host)
}

fn auto_http3_probe_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    let normalized = host.trim_start_matches('[').trim_end_matches(']');
    let Ok(ip) = normalized.parse::<std::net::IpAddr>() else {
        return true;
    };
    !is_local_auto_http3_probe_ip(ip)
}

fn is_local_auto_http3_probe_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unicast_link_local()
                || ip.is_unique_local()
        }
    }
}

fn remote_http_helper_ca_file_arg_for_url(url: &ParsedHttpUrl) -> Result<Option<PathBuf>> {
    remote_http_helper_path_env_arg("GIT_SSL_CAINFO")
        .map(|path| path.or_else(|| url.ca_file.clone()))
}

fn remote_http_helper_client_cert_file_arg_for_url(url: &ParsedHttpUrl) -> Result<Option<PathBuf>> {
    remote_http_helper_path_env_arg("GIT_SSL_CERT")
        .map(|path| path.or_else(|| url.client_cert_file.clone()))
}

fn remote_http_helper_client_key_file_arg_for_url(url: &ParsedHttpUrl) -> Result<Option<PathBuf>> {
    remote_http_helper_path_env_arg("GIT_SSL_KEY")
        .map(|path| path.or_else(|| url.client_key_file.clone()))
}

fn remote_http_helper_path_env_arg(name: &str) -> Result<Option<PathBuf>> {
    let Some(path) = std::env::var_os(name) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    Ok(Some(path))
}

fn remote_http_helper_tls_no_verify_arg() -> bool {
    let Some(value) = std::env::var_os("GIT_SSL_NO_VERIFY") else {
        return false;
    };
    let Some(value) = value.to_str() else {
        return true;
    };
    let value = value.trim();
    !value.is_empty()
        && !value.eq_ignore_ascii_case("0")
        && !value.eq_ignore_ascii_case("false")
        && !value.eq_ignore_ascii_case("no")
}

fn remote_http_helper_tls_no_verify_arg_for_url(url: &ParsedHttpUrl) -> bool {
    url.tls_no_verify || remote_http_helper_tls_no_verify_arg()
}

fn remote_http_helper_proxy_env_for_url(url: &ParsedHttpUrl) -> Option<(&'static str, String)> {
    remote_http_helper_proxy_env_for_url_with_env(url, remote_http_helper_env_value_is_set)
}

fn remote_http_helper_proxy_env_for_url_with_env(
    url: &ParsedHttpUrl,
    is_env_set: impl Fn(&str) -> bool,
) -> Option<(&'static str, String)> {
    if remote_http_helper_proxy_env_is_set(url.scheme, is_env_set) {
        return None;
    }
    url.proxy
        .as_ref()
        .map(|proxy| (remote_http_helper_proxy_env_name(url.scheme), proxy.clone()))
}

fn remote_http_helper_proxy_env_name(scheme: HttpScheme) -> &'static str {
    match scheme {
        HttpScheme::Http => "HTTP_PROXY",
        HttpScheme::Https => "HTTPS_PROXY",
    }
}

fn remote_http_helper_proxy_env_is_set(
    scheme: HttpScheme,
    is_env_set: impl Fn(&str) -> bool,
) -> bool {
    match scheme {
        HttpScheme::Http => {
            is_env_set("HTTP_PROXY")
                || is_env_set("http_proxy")
                || is_env_set("ALL_PROXY")
                || is_env_set("all_proxy")
        }
        HttpScheme::Https => {
            is_env_set("HTTPS_PROXY")
                || is_env_set("https_proxy")
                || is_env_set("ALL_PROXY")
                || is_env_set("all_proxy")
        }
    }
}

fn remote_http_helper_env_value_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn temp_http_helper_body_path() -> Result<PathBuf> {
    temp_http_helper_temp_path(true)
}

fn temp_http_helper_output_path() -> Result<PathBuf> {
    temp_http_helper_temp_path(false)
}

fn temp_http_helper_temp_path(create_file: bool) -> Result<PathBuf> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..1000_u32 {
        let counter =
            TEMP_HTTP_HELPER_BODY_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "skron-http-body-{}-{now:x}-{counter:x}-{attempt}.tmp",
            std::process::id()
        ));
        if !create_file {
            if !path.exists() {
                return Ok(path);
            }
            continue;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CliError::Io(error)),
        }
    }
    Err(CliError::Fatal {
        code: 128,
        message: "could not allocate temporary HTTP helper body path".into(),
    })
}

fn write_http_request_parts<W: Write>(
    stream: &mut W,
    url: &ParsedHttpUrl,
    method: &str,
    suffix: &str,
    first_body: &[u8],
    second_body: &PackBody,
) -> Result<()> {
    let second_body_len = second_body.len()?;
    let body_len = first_body
        .len()
        .checked_add(second_body_len)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "HTTP request body is too large".into(),
        })?;
    if body_len == 0 {
        stream.write_all(method.as_bytes())?;
        stream.write_all(b" ")?;
        url.write_path_with_suffix(stream, suffix)?;
        stream.write_all(b" HTTP/1.1\r\nHost: ")?;
        url.write_host_header(stream)?;
        if let Some(authorization) = url.authorization.as_deref() {
            stream.write_all(b"\r\nAuthorization: ")?;
            stream.write_all(authorization.as_bytes())?;
        }
        for header in &url.extra_headers {
            stream.write_all(b"\r\n")?;
            stream.write_all(header.as_bytes())?;
        }
        stream.write_all(b"\r\nConnection: close\r\n\r\n")?;
    } else {
        stream.write_all(method.as_bytes())?;
        stream.write_all(b" ")?;
        url.write_path_with_suffix(stream, suffix)?;
        stream.write_all(b" HTTP/1.1\r\nHost: ")?;
        url.write_host_header(stream)?;
        if let Some(authorization) = url.authorization.as_deref() {
            stream.write_all(b"\r\nAuthorization: ")?;
            stream.write_all(authorization.as_bytes())?;
        }
        for header in &url.extra_headers {
            stream.write_all(b"\r\n")?;
            stream.write_all(header.as_bytes())?;
        }
        stream.write_all(b"\r\n")?;
        if let Some(content_type) = smart_http_post_content_type(url, method, suffix) {
            stream.write_all(b"Content-Type: ")?;
            stream.write_all(content_type.as_bytes())?;
            stream.write_all(b"\r\n")?;
        }
        stream.write_all(b"Content-Length: ")?;
        write_decimal_usize(stream, body_len)?;
        stream.write_all(b"\r\nConnection: close\r\n\r\n")?;
        stream.write_all(first_body)?;
        second_body.write_len_to(stream, second_body_len)?;
    }
    Ok(())
}

fn read_http_response_head<R: BufRead>(reader: &mut R) -> Result<HttpResponseHead> {
    let mut status_line = String::new();
    if read_limited_http_response_line(reader, &mut status_line)? == 0 {
        return Err(CliError::Fatal {
            code: 128,
            message: "HTTP response is empty".into(),
        });
    }
    truncate_line_ending(&mut status_line);
    let status_code =
        parse_http_response_status_code(&status_line).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("HTTP response status is malformed: {status_line}"),
        })?;
    let mut chunked = false;
    let mut content_length = None;
    let mut location = None;
    let mut line = String::new();
    loop {
        if read_limited_http_response_line(reader, &mut line)? == 0 {
            return Err(CliError::Fatal {
                code: 128,
                message: "HTTP response ended before headers completed".into(),
            });
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        parse_http_response_header_line(&line, &mut chunked, &mut content_length, &mut location)?;
    }
    Ok(HttpResponseHead {
        status_code,
        status_line: HttpStatusLine::raw(status_line),
        chunked,
        content_length,
        location,
    })
}

fn parse_http_response_status_code(status_line: &str) -> Option<u16> {
    let mut parts = status_line.split_ascii_whitespace();
    parts.next()?;
    parse_decimal_u16(parts.next()?.as_bytes())
}

fn read_limited_http_response_line<R: BufRead>(
    reader: &mut R,
    line: &mut String,
) -> io::Result<usize> {
    read_limited_transport_line(
        reader,
        line,
        HTTP_RESPONSE_LINE_LIMIT,
        "HTTP response line too long",
    )
}

fn read_limited_transport_text_line<R: BufRead>(
    reader: &mut R,
    line: &mut String,
) -> io::Result<usize> {
    read_limited_transport_line(
        reader,
        line,
        TRANSPORT_TEXT_LINE_LIMIT,
        "transport text line too long",
    )
}

fn read_limited_transport_line<R: BufRead>(
    reader: &mut R,
    line: &mut String,
    limit: usize,
    error_message: &'static str,
) -> io::Result<usize> {
    line.clear();
    if line.capacity() == 0 {
        line.reserve(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    }
    // SAFETY: `line` is not observed as a String while bytes are appended.
    // Before returning after mutation, the buffer is either validated as UTF-8
    // or cleared back to a valid empty string.
    let bytes = unsafe { line.as_mut_vec() };
    loop {
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) => {
                bytes.clear();
                return Err(error);
            }
        };
        if available.is_empty() {
            break;
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if bytes.len().saturating_add(take) > limit {
            bytes.clear();
            return Err(io::Error::new(io::ErrorKind::InvalidData, error_message));
        }
        bytes.extend_from_slice(&available[..take]);
        reader.consume(take);
        if bytes.ends_with(b"\n") {
            break;
        }
    }
    if bytes.is_empty() {
        return Ok(0);
    }
    if std::str::from_utf8(bytes).is_err() {
        bytes.clear();
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP response line is not UTF-8",
        ));
    }
    Ok(bytes.len())
}

fn parse_http_response_header_line(
    line: &str,
    chunked: &mut bool,
    content_length: &mut Option<usize>,
    location: &mut Option<String>,
) -> Result<()> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(());
    };
    if name.eq_ignore_ascii_case("content-length") {
        let trimmed = value.trim();
        let parsed = parse_decimal_usize(trimmed.as_bytes()).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("HTTP response Content-Length is malformed: {trimmed}"),
        })?;
        if content_length.is_some_and(|existing| existing != parsed) {
            return Err(CliError::Fatal {
                code: 128,
                message: "HTTP response has conflicting Content-Length headers".into(),
            });
        }
        *content_length = Some(parsed);
    } else if name.eq_ignore_ascii_case("transfer-encoding") {
        for coding in value.split(',').map(str::trim) {
            if coding.eq_ignore_ascii_case("chunked") {
                *chunked = true;
            } else if !coding.is_empty() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("unsupported HTTP transfer encoding: {coding}"),
                });
            }
        }
    } else if name.eq_ignore_ascii_case("location") && location.is_none() {
        *location = Some(value.trim().to_owned());
    }
    Ok(())
}

fn read_http_chunk_size<R: BufRead>(reader: &mut R, line: &mut String) -> io::Result<usize> {
    if read_limited_http_response_line(reader, line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "HTTP chunked response is missing chunk size",
        ));
    }
    parse_http_chunk_size(line.as_bytes())
}

fn parse_http_chunk_size(line: &[u8]) -> io::Result<usize> {
    let mut size = 0_usize;
    let mut saw_digit = false;
    for byte in line {
        let value = match *byte {
            b'0'..=b'9' => usize::from(*byte - b'0'),
            b'a'..=b'f' => usize::from(*byte - b'a' + 10),
            b'A'..=b'F' => usize::from(*byte - b'A' + 10),
            b';' | b'\r' | b'\n' => break,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid HTTP chunk size",
                ));
            }
        };
        saw_digit = true;
        size = size
            .checked_mul(16)
            .and_then(|size| size.checked_add(value))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "HTTP chunk too large"))?;
    }
    if !saw_digit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP chunk size",
        ));
    }
    Ok(size)
}

fn parse_decimal_u16(value: &[u8]) -> Option<u16> {
    let mut parsed = 0_u16;
    let mut saw_digit = false;
    for byte in value {
        let digit = match *byte {
            b'0'..=b'9' => u16::from(*byte - b'0'),
            _ => return None,
        };
        saw_digit = true;
        parsed = parsed.checked_mul(10)?.checked_add(digit)?;
    }
    saw_digit.then_some(parsed)
}

fn parse_decimal_usize(value: &[u8]) -> Option<usize> {
    let mut parsed = 0_usize;
    let mut saw_digit = false;
    for byte in value {
        let digit = match *byte {
            b'0'..=b'9' => usize::from(*byte - b'0'),
            _ => return None,
        };
        saw_digit = true;
        parsed = parsed.checked_mul(10)?.checked_add(digit)?;
    }
    saw_digit.then_some(parsed)
}

fn write_decimal_usize<W: Write>(writer: &mut W, mut value: usize) -> io::Result<()> {
    let mut digits = [0_u8; 20];
    let mut cursor = digits.len();
    loop {
        cursor -= 1;
        digits[cursor] = b'0' + u8::try_from(value % 10).expect("decimal digit fits u8");
        value /= 10;
        if value == 0 {
            break;
        }
    }
    writer.write_all(&digits[cursor..])
}

fn append_decimal_usize(buffer: &mut Vec<u8>, value: usize) {
    write_decimal_usize(buffer, value).expect("writing decimal to Vec cannot fail");
}

fn drain_http_chunk_trailers<R: BufRead>(reader: &mut R, line: &mut String) -> io::Result<()> {
    loop {
        if read_limited_http_response_line(reader, line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP chunked response ended before trailer terminator",
            ));
        }
        if line.as_str() == "\r\n" || line.as_str() == "\n" {
            return Ok(());
        }
    }
}

pub(crate) fn daemon(options: DaemonOptions) -> Result<()> {
    let _ = (
        options.timeout,
        options.init_timeout,
        options.max_connections,
        options.strict_paths,
        options.base_path_relaxed,
        options.reuseaddr,
    );
    if options.inetd {
        let mut input = io::stdin().lock();
        let mut output = io::stdout();
        return daemon_serve_connection(&options, &mut input, &mut output);
    }

    let host = options
        .listen
        .first()
        .map(String::as_str)
        .unwrap_or("0.0.0.0");
    let port = options.port.unwrap_or(9418);
    let listener = std::net::TcpListener::bind((host, port)).map_err(CliError::Io)?;
    if let Some(path) = options.pid_file.as_deref() {
        fs::write(path, std::process::id().to_string())?;
    }
    for stream in listener.incoming() {
        let mut stream = stream.map_err(CliError::Io)?;
        let mut reader = daemon_transport_reader(stream.try_clone().map_err(CliError::Io)?);
        if let Err(err) = daemon_serve_connection(&options, &mut reader, &mut stream)
            && options.verbose
        {
            eprintln!("daemon: {err:?}");
        }
    }
    Ok(())
}

fn daemon_serve_connection<R: BufRead, W: Write>(
    options: &DaemonOptions,
    input: &mut R,
    output: &mut W,
) -> Result<()> {
    let Some(request) = read_pkt_line_payload_from_reader(input)? else {
        return Ok(());
    };
    let request = parse_daemon_request(&request)?;
    if request.service != "git-upload-pack" {
        return Err(CliError::Stderr {
            code: 255,
            text: String::new(),
        });
    }
    let repo_path = resolve_daemon_repo_path(options, &request.path)?;
    let repo = upload_pack_repo_from_path(&repo_path, true)?;
    if !options.export_all && !repo.git_dir.join("git-daemon-export-ok").is_file() {
        return Err(CliError::Fatal {
            code: 1,
            message: "repository is not exported".into(),
        });
    }
    let runtime = primitive_runtime_for_repo(&repo);
    let refs = runtime.refs_store_adapter();
    write_upload_pack_advertisement_from_adapter(&refs, output)?;
    output.flush()?;

    let request = read_upload_pack_request_from_stdin(input)?;
    if request.wants.is_empty() {
        return Ok(());
    }
    upload_pack_respond_with_pack_to_writer(&repo, request, false, output)
}

fn unsupported_remote_helper_error(url: &str, prefix: String) -> CliError {
    let helper = remote_helper_protocol(url).unwrap_or(url);
    CliError::Stderr {
        code: 128,
        text: format!(
            "{prefix}git: 'remote-{helper}' is not a git command. See 'git --help'.\nfatal: remote helper '{helper}' aborted session\n"
        ),
    }
}

fn remote_helper_protocol(url: &str) -> Option<&str> {
    url.find("://")
        .and_then(|index| (index > 0).then_some(&url[..index]))
}

fn unsupported_clone_destination_label(
    repository: &str,
    directory: Option<&std::path::Path>,
) -> String {
    if let Some(directory) = directory {
        return directory.display().to_string();
    }
    repository
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(|name| name.trim_end_matches(".git").to_owned())
        .unwrap_or_else(|| repository.to_owned())
}

#[derive(Debug)]
struct DaemonRequest {
    service: String,
    path: String,
}

fn parse_daemon_request(payload: &[u8]) -> Result<DaemonRequest> {
    let command = payload.split(|byte| *byte == 0).next().unwrap_or(payload);
    let command = std::str::from_utf8(command)
        .map_err(|_| CliError::Fatal {
            code: 1,
            message: "daemon request contains non-utf8 command".into(),
        })?
        .trim_end_matches('\n');
    let mut parts = command.split_whitespace();
    let service = parts.next().ok_or_else(|| CliError::Fatal {
        code: 1,
        message: "daemon request is missing service".into(),
    })?;
    let path = parts.next().ok_or_else(|| CliError::Fatal {
        code: 1,
        message: "daemon request is missing repository path".into(),
    })?;
    if parts.next().is_some() {
        return Err(CliError::Fatal {
            code: 1,
            message: "daemon request has extra command fields".into(),
        });
    }
    Ok(DaemonRequest {
        service: service.to_owned(),
        path: path.to_owned(),
    })
}

fn resolve_daemon_repo_path(options: &DaemonOptions, request_path: &str) -> Result<PathBuf> {
    if request_path
        .split('/')
        .any(|component| component == ".." || component == ".")
    {
        return Err(CliError::Fatal {
            code: 1,
            message: "daemon repository path is not normalized".into(),
        });
    }
    let relative = request_path.trim_start_matches('/');
    let path = if let Some(base_path) = options.base_path.as_deref() {
        absolute_path_from_arg(base_path)?.join(relative)
    } else {
        absolute_path_from_arg(std::path::Path::new(request_path))?
    };
    if !options.directories.is_empty() {
        let allowed = options
            .directories
            .iter()
            .map(|directory| absolute_path_from_arg(directory))
            .collect::<Result<Vec<_>>>()?;
        if !allowed.iter().any(|directory| path.starts_with(directory)) {
            return Err(CliError::Fatal {
                code: 1,
                message: "repository path is outside daemon export directories".into(),
            });
        }
    }
    Ok(path)
}

pub(crate) fn upload_pack_repo_from_path(path: &std::path::Path, strict: bool) -> Result<GitRepo> {
    let path = absolute_path_from_arg(path)?;
    if path.join("HEAD").is_file() && path.join("objects").is_dir() {
        return Ok(GitRepo {
            root: path.clone(),
            index_path: path.join("index"),
            objects_dir: path.join("objects"),
            git_dir: path,
        });
    }
    if !strict && path.join(".git").is_dir() {
        let git_dir = path.join(".git");
        return Ok(GitRepo {
            root: path,
            index_path: git_dir.join("index"),
            objects_dir: git_dir.join("objects"),
            git_dir,
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!(
            "'{}' does not appear to be a git repository",
            path.display()
        ),
    })
}

fn write_upload_pack_advertisement<W: Write>(refs: &RefStore, out: &mut W) -> Result<()> {
    let capabilities = upload_pack_capabilities(refs)?;
    let mut wrote = false;
    if let Ok(head) = refs.resolve("HEAD") {
        write_ref_advertisement_pkt_line(out, Some(&head), "HEAD", Some(&capabilities))?;
        wrote = true;
    }

    refs.for_each_server_info_ref(|id, name| {
        write_ref_advertisement_pkt_line(
            out,
            Some(id),
            name,
            (!wrote).then_some(capabilities.as_str()),
        )?;
        wrote = true;
        Ok::<(), CliError>(())
    })?;

    if !wrote {
        write_ref_advertisement_pkt_line(out, None, "capabilities^{}", Some(&capabilities))?;
        out.write_all(b"0000")?;
        return Ok(());
    }
    out.write_all(b"0000")?;
    Ok(())
}

fn upload_pack_capabilities(refs: &RefStore) -> Result<String> {
    let mut capabilities = String::from(
        "multi_ack thin-pack side-band side-band-64k ofs-delta shallow filter \
         no-progress include-tag multi_ack_detailed no-done",
    );
    if let Ok(RefTarget::Symbolic(target)) = refs.read_head() {
        capabilities.push_str(" symref=HEAD:");
        capabilities.push_str(&target);
    }
    capabilities.push_str(" object-format=sha1 agent=skron/0.1.0");
    Ok(capabilities)
}

pub(crate) fn write_pkt_line<W: Write>(out: &mut W, payload: &[u8]) -> Result<()> {
    write_pkt_line_header(out, payload.len())?;
    out.write_all(payload)?;
    Ok(())
}

fn write_ref_advertisement_pkt_line<W: Write>(
    out: &mut W,
    id: Option<&ObjectId>,
    name: &str,
    capabilities: Option<&str>,
) -> Result<()> {
    let id_len = id
        .map(ObjectId::hex_len)
        .unwrap_or_else(|| GitHashAlgorithm::Sha1.digest_len() * 2);
    let capability_len = capabilities
        .map(|capabilities| 1 + capabilities.len())
        .unwrap_or(0);
    let payload_len = id_len
        .checked_add(1)
        .and_then(|len| len.checked_add(name.len()))
        .and_then(|len| len.checked_add(capability_len))
        .and_then(|len| len.checked_add(1))
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "pkt-line length overflow".into(),
        })?;
    write_pkt_line_header(out, payload_len)?;
    write_object_id_or_zero(out, id, GitHashAlgorithm::Sha1)?;
    out.write_all(b" ")?;
    out.write_all(name.as_bytes())?;
    if let Some(capabilities) = capabilities {
        out.write_all(&[0])?;
        out.write_all(capabilities.as_bytes())?;
    }
    out.write_all(b"\n")?;
    Ok(())
}

fn write_object_id_or_zero<W: Write>(
    out: &mut W,
    id: Option<&ObjectId>,
    algorithm: GitHashAlgorithm,
) -> Result<()> {
    if let Some(id) = id {
        id.write_hex_io(out)?;
    } else {
        const ZERO_HEX: [u8; 64] = [b'0'; 64];
        out.write_all(&ZERO_HEX[..algorithm.digest_len() * 2])?;
    }
    Ok(())
}

fn write_pkt_line_header<W: Write>(out: &mut W, payload_len: usize) -> Result<()> {
    let len = payload_len.checked_add(4).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "pkt-line length overflow".into(),
    })?;
    if len > 0xffff {
        return Err(CliError::Fatal {
            code: 128,
            message: "pkt-line payload is too large".into(),
        });
    }
    let mut header = [0_u8; 4];
    write_pkt_len_bytes(&mut header, len);
    out.write_all(&header)?;
    Ok(())
}

fn append_pkt_line_len(out: &mut Vec<u8>, payload_len: usize) -> Result<()> {
    let len = payload_len.checked_add(4).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "pkt-line length overflow".into(),
    })?;
    if len > 0xffff {
        return Err(CliError::Fatal {
            code: 128,
            message: "pkt-line payload is too large".into(),
        });
    }
    append_pkt_len(out, len);
    Ok(())
}

fn write_pkt_len_bytes(out: &mut [u8; 4], len: usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out[0] = HEX[(len >> 12) & 0x0f];
    out[1] = HEX[(len >> 8) & 0x0f];
    out[2] = HEX[(len >> 4) & 0x0f];
    out[3] = HEX[len & 0x0f];
}

fn append_pkt_len(out: &mut Vec<u8>, len: usize) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(HEX[(len >> 12) & 0x0f]);
    out.push(HEX[(len >> 8) & 0x0f]);
    out.push(HEX[(len >> 4) & 0x0f]);
    out.push(HEX[len & 0x0f]);
}

fn append_object_id_or_zero(out: &mut Vec<u8>, id: Option<&ObjectId>, algorithm: GitHashAlgorithm) {
    if let Some(id) = id {
        id.write_hex_bytes(out);
    } else {
        out.resize(out.len() + algorithm.digest_len() * 2, b'0');
    }
}

#[derive(Debug)]
struct UploadPackRequest {
    wants: Vec<ObjectId>,
    haves: Vec<ObjectId>,
    shallows: Vec<ObjectId>,
    deepen: Option<usize>,
    deepen_since: Option<i64>,
    deepen_not: Vec<String>,
    deepen_relative: bool,
    filter: Option<UploadPackFilter>,
    side_band: bool,
}

impl Default for UploadPackRequest {
    fn default() -> Self {
        Self {
            wants: Vec::with_capacity(UPLOAD_PACK_WANT_CAPACITY_HINT),
            haves: Vec::with_capacity(UPLOAD_PACK_HAVE_CAPACITY_HINT),
            shallows: Vec::with_capacity(UPLOAD_PACK_SHALLOW_CAPACITY_HINT),
            deepen: None,
            deepen_since: None,
            deepen_not: Vec::with_capacity(UPLOAD_PACK_DEEPEN_NOT_CAPACITY_HINT),
            deepen_relative: false,
            filter: None,
            side_band: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UploadPackFilter {
    BlobNone,
    BlobLimit(u64),
    ObjectType(GitObjectKind),
    TreeDepth(usize),
    SparseOid(String),
    Combine(Vec<UploadPackFilter>),
}

fn upload_pack_respond_with_pack(
    repo: &GitRepo,
    request: UploadPackRequest,
    stateless_rpc: bool,
) -> Result<()> {
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut stdout = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdout);
    upload_pack_respond_with_pack_to_writer(repo, request, stateless_rpc, &mut stdout)
}

fn upload_pack_respond_with_pack_to_writer<W: Write>(
    repo: &GitRepo,
    request: UploadPackRequest,
    stateless_rpc: bool,
    output: &mut W,
) -> Result<()> {
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let common_have = upload_pack_common_have(&store, &request.haves);
    let shallow_boundaries = upload_pack_shallow_boundaries(repo, &store, &request)?;
    let pack = upload_pack_build_pack_file(repo, &store, &request)?;
    if !shallow_boundaries.is_empty() {
        for boundary in &shallow_boundaries {
            write_shallow_pkt_line(output, boundary)?;
        }
        output.write_all(b"0000")?;
    }
    if let Some(have) = common_have {
        write_ack_pkt_line(output, &have)?;
    } else {
        write_pkt_line(output, b"NAK\n")?;
    }
    if request.side_band {
        let mut file = fs::File::open(pack.path())?;
        write_sideband_pack_from_reader(output, &mut file)?;
    } else {
        let len = file_len_usize(pack.path(), "pack file is too large for this platform")?;
        let mut file = fs::File::open(pack.path())?;
        copy_exact_len_with_message(&mut file, output, len, "pack file ended early")?;
        if !stateless_rpc {
            output.write_all(b"0000")?;
        }
    }
    output.flush()?;
    Ok(())
}

pub(crate) fn process_upload_pack_request_from_reader(
    repo: &GitRepo,
    input: &mut dyn Read,
    stateless_rpc: bool,
) -> Result<Vec<u8>> {
    let mut input = io::BufReader::with_capacity(PACK_RECEIPT_BUF_CAPACITY, input);
    let request = read_upload_pack_request_from_stdin(&mut input)?;
    if request.wants.is_empty() {
        return Ok(Vec::new());
    }

    let mut output = Vec::new();
    {
        let mut writer = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, &mut output);
        upload_pack_respond_with_pack_to_writer(repo, request, stateless_rpc, &mut writer)?;
        writer.flush()?;
    }
    Ok(output)
}

struct TempUploadPack {
    path: PathBuf,
}

impl TempUploadPack {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempUploadPack {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn read_upload_pack_request_from_stdin<R: BufRead>(
    input: &mut R,
) -> Result<UploadPackRequest> {
    let mut request = UploadPackRequest::default();
    let mut payload = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    loop {
        if !read_pkt_line_payload_into(input, &mut payload)? {
            if request.wants.is_empty() {
                return Ok(request);
            }
            continue;
        }
        let line = trim_lf_payload(&payload);
        if let Some(rest) = line.strip_prefix(b"want ") {
            let id = first_ascii_token(rest).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "upload-pack want line is missing object id".into(),
            })?;
            request
                .wants
                .push(ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id)?);
            if ascii_tokens(rest).any(|part| part == b"side-band" || part == b"side-band-64k") {
                request.side_band = true;
            }
        } else if line == b"done" {
            break;
        } else if let Some(rest) = line.strip_prefix(b"have ") {
            let id = first_ascii_token(rest).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "upload-pack have line is missing object id".into(),
            })?;
            request
                .haves
                .push(ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id)?);
        } else if let Some(depth) = line.strip_prefix(b"deepen ") {
            let depth = parse_ascii_usize(depth).map_err(|_| CliError::Fatal {
                code: 128,
                message: format!(
                    "upload-pack deepen line is invalid: {}",
                    protocol_line_for_error(line)
                ),
            })?;
            if depth == 0 {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "upload-pack deepen depth must be positive".into(),
                });
            }
            request.deepen = Some(depth);
        } else if let Some(id) = line.strip_prefix(b"shallow ") {
            request.shallows.push(ObjectId::from_hex_bytes(
                GitHashAlgorithm::Sha1,
                first_ascii_token(id).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "upload-pack shallow line is missing object id".into(),
                })?,
            )?);
        } else if let Some(filter) = line.strip_prefix(b"filter ") {
            request.filter = Some(parse_upload_pack_filter(ascii_line_as_str(filter)?)?);
        } else if let Some(timestamp) = line.strip_prefix(b"deepen-since ") {
            request.deepen_since =
                Some(parse_ascii_i64(timestamp).map_err(|_| CliError::Fatal {
                    code: 128,
                    message: format!(
                        "upload-pack deepen-since line is invalid: {}",
                        protocol_line_for_error(line)
                    ),
                })?);
        } else if let Some(rev) = line.strip_prefix(b"deepen-not ") {
            let rev = trim_ascii_whitespace(rev);
            if rev.is_empty() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "upload-pack deepen-not line is invalid: {}",
                        protocol_line_for_error(line)
                    ),
                });
            }
            request.deepen_not.push(ascii_line_as_str(rev)?.to_owned());
        } else if line == b"deepen-relative" {
            request.deepen_relative = true;
        }
    }
    sort_dedup_object_ids(&mut request.wants);
    sort_dedup_object_ids(&mut request.haves);
    sort_dedup_object_ids(&mut request.shallows);
    Ok(request)
}

fn trim_lf_payload(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

fn trim_ascii_whitespace(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|idx| idx + 1)
        .unwrap_or(start);
    &line[start..end]
}

fn ascii_tokens(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(|byte| byte.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
}

fn head_symref_branch_from_capabilities(capabilities: &[u8]) -> Option<String> {
    const PREFIX: &[u8] = b"symref=HEAD:refs/heads/";
    ascii_tokens(capabilities).find_map(|capability| {
        capability
            .strip_prefix(PREFIX)
            .map(|branch| String::from_utf8_lossy(branch).into_owned())
    })
}

fn first_ascii_token(line: &[u8]) -> Option<&[u8]> {
    ascii_tokens(line).next()
}

fn ascii_token_as_str(token: &[u8]) -> Result<&str> {
    std::str::from_utf8(token).map_err(|_| CliError::Fatal {
        code: 128,
        message: "protocol token contains non-utf8 bytes".into(),
    })
}

fn ascii_line_as_str(line: &[u8]) -> Result<&str> {
    std::str::from_utf8(line).map_err(|_| CliError::Fatal {
        code: 128,
        message: "protocol line contains non-utf8 bytes".into(),
    })
}

fn parse_ascii_usize(value: &[u8]) -> std::result::Result<usize, std::num::ParseIntError> {
    std::str::from_utf8(trim_ascii_whitespace(value))
        .unwrap_or("")
        .parse()
}

fn parse_ascii_i64(value: &[u8]) -> std::result::Result<i64, std::num::ParseIntError> {
    std::str::from_utf8(trim_ascii_whitespace(value))
        .unwrap_or("")
        .parse()
}

fn protocol_line_for_error(line: &[u8]) -> String {
    String::from_utf8_lossy(line).into_owned()
}

fn parse_upload_pack_filter(raw: &str) -> Result<UploadPackFilter> {
    if raw == "blob:none" {
        return Ok(UploadPackFilter::BlobNone);
    }
    if let Some(limit) = raw.strip_prefix("blob:limit=") {
        let limit = parse_upload_pack_filter_size(limit)
            .ok_or_else(|| invalid_upload_pack_filter_spec(raw))?;
        return Ok(UploadPackFilter::BlobLimit(limit));
    }
    if let Some(kind) = raw.strip_prefix("object:type=") {
        let kind = match kind {
            "blob" => GitObjectKind::Blob,
            "tree" => GitObjectKind::Tree,
            "commit" => GitObjectKind::Commit,
            "tag" => GitObjectKind::Tag,
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!(
                        "'{kind}' for 'object:type=<type>' is not a valid object type"
                    ),
                });
            }
        };
        return Ok(UploadPackFilter::ObjectType(kind));
    }
    if let Some(depth) = raw.strip_prefix("tree:") {
        let depth = depth.parse::<usize>().map_err(|_| CliError::Fatal {
            code: 128,
            message: "expected 'tree:<depth>'".into(),
        })?;
        return Ok(UploadPackFilter::TreeDepth(depth));
    }
    if let Some(blobish) = raw.strip_prefix("sparse:oid=") {
        if blobish.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "unable to access sparse blob in ''".into(),
            });
        }
        return Ok(UploadPackFilter::SparseOid(blobish.to_owned()));
    }
    if let Some(filters) = raw.strip_prefix("combine:") {
        if filters.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "expected something after combine:".into(),
            });
        }
        let filters = filters
            .split('+')
            .map(percent_decode_upload_pack_filter)
            .map(|decoded| decoded.and_then(|filter| parse_upload_pack_filter(&filter)))
            .collect::<Result<Vec<_>>>()?;
        return Ok(UploadPackFilter::Combine(filters));
    }
    Err(invalid_upload_pack_filter_spec(raw))
}

fn invalid_upload_pack_filter_spec(raw: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("invalid filter-spec '{raw}'"),
    }
}

fn percent_decode_upload_pack_filter(value: &str) -> Result<String> {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            let high = *bytes.get(idx + 1).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("upload-pack filter percent escape is invalid: {value}"),
            })?;
            let low = *bytes.get(idx + 2).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("upload-pack filter percent escape is invalid: {value}"),
            })?;
            out.push(
                decode_percent_hex_byte(high, low).ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("upload-pack filter percent escape is invalid: {value}"),
                })?,
            );
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    String::from_utf8(out).map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("upload-pack filter is not utf-8: {value}"),
    })
}

fn decode_percent_hex_byte(high: u8, low: u8) -> Option<u8> {
    Some((hex_value(high)? << 4) | hex_value(low)?)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_pkt_line_len(header: &[u8; 4], invalid_header_message: &'static str) -> Result<usize> {
    let mut len = 0_usize;
    for byte in header {
        let Some(value) = hex_value(*byte) else {
            return Err(CliError::Fatal {
                code: 128,
                message: invalid_header_message.into(),
            });
        };
        len = (len << 4) | usize::from(value);
    }
    Ok(len)
}

fn parse_upload_pack_filter_size(value: &str) -> Option<u64> {
    let (number, multiplier) = match value.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&value[..value.len() - 1], 1024_u64),
        Some(b'm' | b'M') => (&value[..value.len() - 1], 1024_u64 * 1024),
        Some(b'g' | b'G') => (&value[..value.len() - 1], 1024_u64 * 1024 * 1024),
        _ => (value, 1),
    };
    if number.is_empty() {
        return None;
    }
    number.parse::<u64>().ok()?.checked_mul(multiplier)
}

fn upload_pack_common_have(store: &LooseObjectStore, haves: &[ObjectId]) -> Option<ObjectId> {
    haves
        .iter()
        .find(|have| store.contains_object(have).unwrap_or(false))
        .cloned()
}

pub(crate) fn read_pkt_line_payload_from_reader<R: Read + ?Sized>(
    input: &mut R,
) -> Result<Option<Vec<u8>>> {
    let mut payload = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    if read_pkt_line_payload_into(input, &mut payload)? {
        Ok(Some(payload))
    } else {
        Ok(None)
    }
}

fn read_pkt_line_payload_into<R: Read + ?Sized>(
    input: &mut R,
    payload: &mut Vec<u8>,
) -> Result<bool> {
    let mut header = [0_u8; 4];
    match input.read_exact(&mut header) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
        Err(err) => return Err(CliError::Io(err)),
    };
    let len = parse_pkt_line_len(&header, "invalid upload-pack pkt-line header")?;
    if len == 0 {
        return Ok(false);
    }
    let payload_len = len.checked_sub(4).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "invalid upload-pack pkt-line length".into(),
    })?;
    read_exact_payload_into(input, payload_len, payload)?;
    Ok(true)
}

fn upload_pack_build_pack_file(
    repo: &GitRepo,
    store: &LooseObjectStore,
    request: &UploadPackRequest,
) -> Result<TempUploadPack> {
    let ids = upload_pack_collect_pack_ids(repo, store, request)?;
    let (temp_pack, file) = temp_http_pack_file(&repo.objects_dir)?;
    let result = (|| {
        let mut file = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, file);
        let packed_first_store = store.packed_first();
        write_pack_from_store_with_options(
            &packed_first_store,
            GitHashAlgorithm::Sha1,
            &ids,
            pack_encode_options(None, None),
            &mut file,
        )?;
        file.flush()?;
        Ok(TempUploadPack {
            path: temp_pack.clone(),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result
}

fn upload_pack_collect_pack_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
    request: &UploadPackRequest,
) -> Result<Vec<ObjectId>> {
    let exclude_ids = upload_pack_exclude_ids(request, !request.deepen_relative);
    let exclude_revs = request.deepen_not.as_slice();
    let commit_cache = CommitObjectCache::new(store);
    let excluded_commits = collect_rev_list_excluded_commits_from_ids_cached(
        repo,
        store,
        &commit_cache,
        exclude_ids.as_ref(),
        exclude_revs,
    )?;
    let commits = if let Some(depth) = request.deepen {
        let (roots, depth) = upload_pack_depth_roots(request, depth);
        upload_pack_depth_limited_commits(store, roots, depth)?
    } else if let Some(timestamp) = request.deepen_since {
        let mut excluded = HashSet::with_capacity(transport_history_collection_capacity(
            excluded_commits.len(),
        ));
        excluded.extend(excluded_commits.iter());
        let commits =
            upload_pack_since_limited_commits(store, &request.wants, timestamp, &excluded)?;
        commits
    } else {
        let mut excluded = HashSet::with_capacity(transport_history_collection_capacity(
            excluded_commits.len(),
        ));
        excluded.extend(excluded_commits.iter().cloned());
        collect_commits_from_ids_cached_with_excluded(
            repo,
            &commit_cache,
            &request.wants,
            None,
            &excluded,
        )?
    };
    let id_capacity = upload_pack_pack_ids_capacity_hint(request.wants.len(), commits.len());
    let mut ids = Vec::with_capacity(id_capacity);
    let mut seen = HashSet::<ObjectId>::with_capacity(id_capacity);
    for want in &request.wants {
        if seen.insert(want.clone()) {
            ids.push(want.clone());
        }
    }
    for commit in &commits {
        if seen.insert(commit.clone()) {
            ids.push(commit.clone());
        }
    }
    if let Some(filter) = request.filter.as_ref() {
        let mut sparse_patterns_cache =
            HashMap::with_capacity(upload_pack_sparse_filter_cache_capacity(filter));
        if upload_pack_filter_needs_path(filter) {
            for_each_rev_list_object_path_cached(
                store,
                &commit_cache,
                &commits,
                &excluded_commits,
                |id, kind, path| {
                    let size = upload_pack_filter_object_size(store, filter, id, kind)?;
                    if upload_pack_filter_excludes_object(
                        repo,
                        store,
                        filter,
                        &mut sparse_patterns_cache,
                        kind,
                        size,
                        path,
                    )? {
                        return Ok(());
                    }
                    if seen.insert(id.clone()) {
                        ids.push(id.clone());
                    }
                    Ok(())
                },
            )?;
        } else {
            for_each_rev_list_object_id_into_cached(
                store,
                &commit_cache,
                &commits,
                &[],
                &excluded_commits,
                &mut seen,
                |id| {
                    let kind = object_kind_hint_or_read(store, id)?;
                    let size = upload_pack_filter_object_size(store, filter, id, kind)?;
                    if !upload_pack_filter_excludes_object(
                        repo,
                        store,
                        filter,
                        &mut sparse_patterns_cache,
                        kind,
                        size,
                        None,
                    )? {
                        ids.push(id.clone());
                    }
                    Ok(())
                },
            )?;
        }
    } else {
        collect_rev_list_object_ids_into_cached(
            store,
            &commit_cache,
            &commits,
            &[],
            &excluded_commits,
            &mut seen,
            &mut ids,
        )?;
    }
    Ok(ids)
}

fn upload_pack_pack_ids_capacity_hint(wants_len: usize, commits_len: usize) -> usize {
    wants_len
        .min(UPLOAD_PACK_BASE_ID_INITIAL_CAPACITY_LIMIT)
        .saturating_add(commits_len.min(UPLOAD_PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT))
}

fn transport_history_collection_capacity(len: usize) -> usize {
    len.min(TRANSPORT_HISTORY_COLLECTION_CAPACITY_LIMIT)
}

fn reserve_transport_history_vec<T>(items: &mut Vec<T>, len: usize) {
    let desired_spare = transport_history_collection_capacity(len);
    let spare = items.capacity().saturating_sub(items.len());
    if spare < desired_spare {
        items.reserve(desired_spare);
    }
}

fn reserve_transport_history_set<T: Eq + std::hash::Hash>(items: &mut HashSet<T>, len: usize) {
    let desired_spare = transport_history_collection_capacity(len);
    let spare = items.capacity().saturating_sub(items.len());
    if spare < desired_spare {
        items.reserve(desired_spare);
    }
}

fn reserve_transport_history_queue<T>(items: &mut VecDeque<T>, len: usize) {
    let desired_spare = transport_history_collection_capacity(len);
    let spare = items.capacity().saturating_sub(items.len());
    if spare < desired_spare {
        items.reserve(desired_spare);
    }
}

fn transport_ref_collection_capacity(len: usize) -> usize {
    len.min(TRANSPORT_REF_COLLECTION_CAPACITY_LIMIT)
}

fn upload_pack_filter_object_size(
    store: &LooseObjectStore,
    filter: &UploadPackFilter,
    id: &ObjectId,
    kind: GitObjectKind,
) -> Result<u64> {
    if kind != GitObjectKind::Blob || !upload_pack_filter_needs_blob_size(filter) {
        return Ok(0);
    }
    if let Some(size) = store.blob_size_hint(id)? {
        return Ok(size as u64);
    }
    Ok(store.read_object(id)?.content.len() as u64)
}

fn upload_pack_filter_needs_blob_size(filter: &UploadPackFilter) -> bool {
    match filter {
        UploadPackFilter::BlobLimit(_) => true,
        UploadPackFilter::Combine(filters) => {
            filters.iter().any(upload_pack_filter_needs_blob_size)
        }
        UploadPackFilter::BlobNone
        | UploadPackFilter::ObjectType(_)
        | UploadPackFilter::TreeDepth(_)
        | UploadPackFilter::SparseOid(_) => false,
    }
}

fn upload_pack_filter_needs_path(filter: &UploadPackFilter) -> bool {
    match filter {
        UploadPackFilter::TreeDepth(_) | UploadPackFilter::SparseOid(_) => true,
        UploadPackFilter::Combine(filters) => filters.iter().any(upload_pack_filter_needs_path),
        UploadPackFilter::BlobNone
        | UploadPackFilter::BlobLimit(_)
        | UploadPackFilter::ObjectType(_) => false,
    }
}

fn upload_pack_sparse_filter_cache_capacity(filter: &UploadPackFilter) -> usize {
    match filter {
        UploadPackFilter::SparseOid(_) => 1,
        UploadPackFilter::Combine(filters) => filters
            .iter()
            .map(upload_pack_sparse_filter_cache_capacity)
            .sum(),
        UploadPackFilter::BlobNone
        | UploadPackFilter::BlobLimit(_)
        | UploadPackFilter::ObjectType(_)
        | UploadPackFilter::TreeDepth(_) => 0,
    }
}

fn upload_pack_exclude_ids(
    request: &UploadPackRequest,
    include_shallows: bool,
) -> Cow<'_, [ObjectId]> {
    if !include_shallows || request.shallows.is_empty() {
        return Cow::Borrowed(&request.haves);
    }
    let exclude_capacity = transport_history_collection_capacity(
        request.haves.len().saturating_add(request.shallows.len()),
    );
    let mut excludes = Vec::with_capacity(exclude_capacity);
    excludes.extend(request.haves.iter().cloned());
    excludes.extend(request.shallows.iter().cloned());
    Cow::Owned(excludes)
}

fn upload_pack_filter_excludes_object(
    repo: &GitRepo,
    store: &LooseObjectStore,
    filter: &UploadPackFilter,
    sparse_patterns_cache: &mut HashMap<String, Vec<Vec<u8>>>,
    kind: GitObjectKind,
    size: u64,
    path: Option<&[u8]>,
) -> Result<bool> {
    match filter {
        UploadPackFilter::BlobNone => Ok(kind == GitObjectKind::Blob),
        UploadPackFilter::BlobLimit(limit) => Ok(kind == GitObjectKind::Blob && size >= *limit),
        UploadPackFilter::ObjectType(requested) => Ok(kind != *requested),
        UploadPackFilter::TreeDepth(depth) => {
            if !matches!(kind, GitObjectKind::Tree | GitObjectKind::Blob) {
                return Ok(false);
            }
            let path_depth = path
                .filter(|path| !path.is_empty())
                .map(|path| {
                    path.split(|byte| *byte == b'/')
                        .filter(|part| !part.is_empty())
                        .count()
                })
                .unwrap_or(0);
            Ok(path_depth >= *depth)
        }
        UploadPackFilter::SparseOid(blobish) => {
            if kind != GitObjectKind::Blob {
                return Ok(false);
            }
            let Some(path) = path else {
                return Ok(true);
            };
            let patterns =
                upload_pack_sparse_oid_patterns_cached(sparse_patterns_cache, blobish, || {
                    upload_pack_sparse_oid_patterns(repo, store, blobish)
                })?;
            Ok(!sparse_filter_path_matches(path, patterns))
        }
        UploadPackFilter::Combine(filters) => {
            for filter in filters {
                if upload_pack_filter_excludes_object(
                    repo,
                    store,
                    filter,
                    sparse_patterns_cache,
                    kind,
                    size,
                    path,
                )? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

fn upload_pack_sparse_oid_patterns_cached<'a, F>(
    cache: &'a mut HashMap<String, Vec<Vec<u8>>>,
    blobish: &str,
    load: F,
) -> Result<&'a [Vec<u8>]>
where
    F: FnOnce() -> Result<Vec<Vec<u8>>>,
{
    if !cache.contains_key(blobish) {
        cache.insert(blobish.to_owned(), load()?);
    }
    Ok(cache
        .get(blobish)
        .expect("sparse patterns cached after insert")
        .as_slice())
}

fn upload_pack_sparse_oid_patterns(
    repo: &GitRepo,
    store: &LooseObjectStore,
    blobish: &str,
) -> Result<Vec<Vec<u8>>> {
    let id = resolve_objectish(repo, blobish)?;
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Blob {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("upload-pack sparse filter object {blobish} is not a blob"),
        });
    }
    Ok(object
        .content
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .map(|line| line.trim_ascii())
        .filter(|line| !line.is_empty() && !line.starts_with(b"#"))
        .map(<[u8]>::to_vec)
        .collect())
}

fn sparse_filter_path_matches(path: &[u8], patterns: &[Vec<u8>]) -> bool {
    if patterns.is_empty() {
        return true;
    }
    let mut matched = false;
    for pattern in patterns {
        let (exclude, raw) = pattern
            .strip_prefix(b"!")
            .map_or((false, pattern.as_slice()), |raw| (true, raw));
        if sparse_filter_pattern_matches(path, raw) {
            matched = !exclude;
        }
    }
    matched
}

fn sparse_filter_pattern_matches(path: &[u8], raw: &[u8]) -> bool {
    let mut pattern = raw.strip_prefix(b"/").unwrap_or(raw);
    let directory_only = pattern.ends_with(b"/");
    if directory_only {
        pattern = pattern.strip_suffix(b"/").unwrap_or(pattern);
    }
    if pattern.is_empty() {
        return true;
    }
    if directory_only {
        return path_matches_prefix_component(path, pattern);
    }
    if pattern.contains(&b'*') || pattern.contains(&b'?') || pattern.contains(&b'[') {
        let path_text = String::from_utf8_lossy(path);
        let pattern_text = String::from_utf8_lossy(pattern);
        return wildcard_match_pathspec(&pattern_text, &path_text, false, true);
    }
    path_matches_prefix_component(path, pattern)
}

fn path_matches_prefix_component(path: &[u8], prefix: &[u8]) -> bool {
    path == prefix || (path.starts_with(prefix) && path.get(prefix.len()) == Some(&b'/'))
}

fn upload_pack_shallow_boundaries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    request: &UploadPackRequest,
) -> Result<Vec<ObjectId>> {
    if let Some(depth) = request.deepen {
        let (roots, depth) = upload_pack_depth_roots(request, depth);
        let mut boundaries = shallow_boundaries(store, roots, depth)?;
        if !request.deepen_not.is_empty() {
            let exclude_ids = upload_pack_exclude_ids(request, !request.deepen_relative);
            let commits = upload_pack_depth_limited_commits(store, roots, depth)?;
            boundaries.extend(upload_pack_exclusion_shallow_boundaries(
                repo,
                store,
                &commits,
                &exclude_ids,
                &request.deepen_not,
            )?);
            sort_dedup_object_ids(&mut boundaries);
        }
        return Ok(boundaries);
    }
    if let Some(timestamp) = request.deepen_since {
        return upload_pack_since_shallow_boundaries(
            repo,
            store,
            &request.wants,
            timestamp,
            request,
        );
    }
    if !request.deepen_not.is_empty() {
        let exclude_ids = upload_pack_exclude_ids(request, true);
        let commit_cache = CommitObjectCache::new(store);
        let commits = collect_commits_from_ids_with_id_exclusions_cached(
            repo,
            store,
            &commit_cache,
            &request.wants,
            &exclude_ids,
            &request.deepen_not,
            None,
        )?;
        return upload_pack_exclusion_shallow_boundaries(
            repo,
            store,
            &commits,
            &exclude_ids,
            &request.deepen_not,
        );
    }
    Ok(Vec::new())
}

fn upload_pack_depth_roots(request: &UploadPackRequest, depth: usize) -> (&[ObjectId], usize) {
    if request.deepen_relative && !request.shallows.is_empty() {
        (&request.shallows, depth.saturating_add(1))
    } else {
        (&request.wants, depth)
    }
}

fn upload_pack_depth_limited_commits(
    store: &LooseObjectStore,
    wants: &[ObjectId],
    depth: usize,
) -> Result<Vec<ObjectId>> {
    let commit_cache = CommitObjectCache::new(store);
    let initial_capacity = transport_history_collection_capacity(wants.len());
    let mut pending = VecDeque::with_capacity(initial_capacity);
    pending.extend(wants.iter().cloned().map(|id| (id, 1usize)));
    let mut seen = HashSet::with_capacity(initial_capacity);
    let mut commits = Vec::with_capacity(initial_capacity);
    while let Some((id, level)) = pending.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let commit = commit_cache.read_commit(&id)?;
        commits.push(id);
        if level >= depth {
            continue;
        }
        reserve_transport_history_queue(&mut pending, commit.parents.len());
        for parent in &commit.parents {
            pending.push_back((parent.clone(), level + 1));
        }
    }
    Ok(commits)
}

fn upload_pack_since_limited_commits(
    store: &LooseObjectStore,
    wants: &[ObjectId],
    since: i64,
    excluded: &HashSet<&ObjectId>,
) -> Result<Vec<ObjectId>> {
    let commit_cache = CommitObjectCache::new(store);
    let initial_capacity = transport_history_collection_capacity(wants.len());
    let mut pending = VecDeque::with_capacity(initial_capacity);
    pending.extend(wants.iter().cloned());
    let mut seen = HashSet::with_capacity(initial_capacity);
    let mut commits = Vec::with_capacity(initial_capacity);
    while let Some(id) = pending.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let commit = commit_cache.read_commit(&id)?;
        let timestamp = signature_timestamp(&commit.committer).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("commit {} has invalid committer timestamp", id.to_hex()),
        })?;
        if timestamp < since {
            continue;
        }
        if excluded.contains(&id) {
            continue;
        }
        commits.push(id);
        reserve_transport_history_queue(&mut pending, commit.parents.len());
        for parent in &commit.parents {
            pending.push_back(parent.clone());
        }
    }
    Ok(commits)
}

fn upload_pack_since_shallow_boundaries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    wants: &[ObjectId],
    since: i64,
    request: &UploadPackRequest,
) -> Result<Vec<ObjectId>> {
    let commit_cache = CommitObjectCache::new(store);
    let exclude_ids = upload_pack_exclude_ids(request, true);
    let excluded_commits = collect_rev_list_excluded_commits_from_ids_cached(
        repo,
        store,
        &commit_cache,
        &exclude_ids,
        &request.deepen_not,
    )?;
    let mut excluded = HashSet::with_capacity(transport_history_collection_capacity(
        excluded_commits.len(),
    ));
    excluded.extend(excluded_commits.iter());
    let commits = upload_pack_since_limited_commits(store, wants, since, &excluded)?;
    let mut included = HashSet::with_capacity(transport_history_collection_capacity(commits.len()));
    included.extend(commits.iter());
    let mut boundaries = Vec::with_capacity(transport_history_collection_capacity(
        commits.len().min(wants.len()),
    ));
    for id in &commits {
        let commit = commit_cache.read_commit(id)?;
        let has_excluded_parent = commit.parents.iter().any(|parent| {
            if included.contains(parent) {
                return false;
            }
            excluded.contains(parent)
                || commit_cache
                    .read_commit(parent)
                    .ok()
                    .and_then(|commit| signature_timestamp(&commit.committer))
                    .is_some_and(|timestamp| timestamp < since)
        });
        if has_excluded_parent {
            boundaries.push(id.clone());
        }
    }
    sort_dedup_object_ids(&mut boundaries);
    Ok(boundaries)
}

fn upload_pack_exclusion_shallow_boundaries(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commits: &[ObjectId],
    exclude_roots: &[ObjectId],
    exclude_revs: &[String],
) -> Result<Vec<ObjectId>> {
    let commit_cache = CommitObjectCache::new(store);
    let mut included = HashSet::with_capacity(transport_history_collection_capacity(commits.len()));
    included.extend(commits.iter());
    let excluded_commits = collect_rev_list_excluded_commits_from_ids_cached(
        repo,
        store,
        &commit_cache,
        exclude_roots,
        exclude_revs,
    )?;
    let mut excluded = HashSet::with_capacity(transport_history_collection_capacity(
        excluded_commits.len(),
    ));
    excluded.extend(excluded_commits);
    let mut boundaries = Vec::with_capacity(transport_history_collection_capacity(
        commits.len().min(excluded.len()),
    ));
    for id in commits {
        let commit = commit_cache.read_commit(id)?;
        if commit
            .parents
            .iter()
            .any(|parent| !included.contains(parent) && excluded.contains(parent))
        {
            boundaries.push(id.clone());
        }
    }
    sort_dedup_object_ids(&mut boundaries);
    Ok(boundaries)
}

fn sort_dedup_object_ids(ids: &mut Vec<ObjectId>) {
    if ids.len() < 2 {
        return;
    }
    match object_id_order(ids) {
        ObjectIdOrder::SortedUnique => return,
        ObjectIdOrder::SortedWithDuplicates => {}
        ObjectIdOrder::Unsorted => ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes())),
    }
    ids.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObjectIdOrder {
    SortedUnique,
    SortedWithDuplicates,
    Unsorted,
}

fn object_id_order(ids: &[ObjectId]) -> ObjectIdOrder {
    let mut has_duplicates = false;
    for pair in ids.windows(2) {
        let left = pair[0].as_bytes();
        let right = pair[1].as_bytes();
        if left > right {
            return ObjectIdOrder::Unsorted;
        }
        has_duplicates |= left == right;
    }
    if has_duplicates {
        ObjectIdOrder::SortedWithDuplicates
    } else {
        ObjectIdOrder::SortedUnique
    }
}

const SIDEBAND_PACK_CHUNK_SIZE: usize = 65_520;

fn write_shallow_pkt_line<W: Write>(out: &mut W, id: &ObjectId) -> Result<()> {
    let payload_len = b"shallow ".len() + id.hex_len() + 1;
    write_pkt_line_header(out, payload_len)?;
    out.write_all(b"shallow ")?;
    id.write_hex_io(out)?;
    out.write_all(b"\n")?;
    Ok(())
}

fn write_ack_pkt_line<W: Write>(out: &mut W, id: &ObjectId) -> Result<()> {
    let payload_len = b"ACK ".len() + id.hex_len() + 1;
    write_pkt_line_header(out, payload_len)?;
    out.write_all(b"ACK ")?;
    id.write_hex_io(out)?;
    out.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn write_sideband_pack<W: Write>(out: &mut W, pack: &[u8]) -> Result<()> {
    for chunk in pack.chunks(SIDEBAND_PACK_CHUNK_SIZE) {
        write_sideband_pack_chunk(out, chunk)?;
    }
    out.write_all(b"0000")?;
    Ok(())
}

fn write_sideband_pack_from_reader<W: Write, R: Read>(out: &mut W, reader: &mut R) -> Result<()> {
    let mut buffer = [0_u8; SIDEBAND_PACK_CHUNK_SIZE];
    loop {
        let len = reader.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        write_sideband_pack_chunk(out, &buffer[..len])?;
    }
    out.write_all(b"0000")?;
    Ok(())
}

fn write_sideband_pack_chunk<W: Write>(out: &mut W, chunk: &[u8]) -> Result<()> {
    write_pkt_line_header(out, chunk.len() + 1)?;
    out.write_all(&[1])?;
    out.write_all(chunk)?;
    Ok(())
}

pub(crate) fn fetch_pack(options: FetchPackOptions) -> Result<()> {
    if options.depth == Some(0) {
        return Err(CliError::Fatal {
            code: 128,
            message: "depth value is not a positive number".into(),
        });
    }
    if options.keep || options.upload_pack.is_some() || options.diag_url || options.verbose {
        return Err(CliError::Fatal {
            code: 129,
            message: "fetch-pack currently supports local refs without optional negotiation modes"
                .into(),
        });
    }
    let destination = find_repo_or_bare()?;
    let source_path = absolute_path_from_arg(std::path::Path::new(&options.directory))?;
    let source = local_clone_source(&source_path)?;
    let source_repo = GitRepo {
        root: source.git_dir.clone(),
        index_path: source.git_dir.join("index"),
        objects_dir: source.git_dir.join("objects"),
        git_dir: source.git_dir,
    };
    let source_refs = refs_adapter_from_git_dir(&source_repo.git_dir);
    let mut requested = options.refs;
    if options.stdin {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::with_capacity(TRANSPORT_STDIN_BUF_CAPACITY, stdin.lock());
        collect_trimmed_lines_from_reader(&mut stdin, &mut requested)?;
    }
    if options.all {
        source_refs.for_each_ref_name("refs/", |ref_name| {
            requested.push(ref_name.to_owned());
            Ok::<(), CliError>(())
        })?;
    }
    if requested.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "fetch-pack requires at least one ref unless --all is used".into(),
        });
    }
    requested.sort();
    requested.dedup();

    let source_store = object_adapter_from_objects_dir(&source_repo.objects_dir);
    let destination_store = object_adapter_from_objects_dir(&destination.objects_dir);
    let requested_capacity = transport_ref_collection_capacity(requested.len());
    let mut fetched_objects = HashSet::with_capacity(requested_capacity);
    let mut shallow_roots = Vec::with_capacity(if options.depth.is_some() {
        requested_capacity
    } else {
        0
    });
    for ref_name in requested {
        let id = source_refs.resolve(&ref_name)?;
        if let Some(depth) = options.depth {
            if object_kind_hint_or_read(&source_store, &id)? == GitObjectKind::Commit {
                let depth_limited_commits = upload_pack_depth_limited_commits(
                    &source_store,
                    std::slice::from_ref(&id),
                    depth,
                )?;
                copy_reachable_objects_for_depth_into(
                    &source_store,
                    &destination_store,
                    &depth_limited_commits,
                    &mut fetched_objects,
                )?;
                shallow_roots.push(id.clone());
            } else {
                copy_reachable_objects_into(
                    &source_repo,
                    &source_store,
                    &destination_store,
                    &id,
                    &mut fetched_objects,
                )?;
            }
        } else {
            copy_reachable_objects_into(
                &source_repo,
                &source_store,
                &destination_store,
                &id,
                &mut fetched_objects,
            )?;
        }
        if !options.quiet {
            println!("{} {}", id.to_hex(), ref_name);
        }
    }
    if let Some(depth) = options.depth {
        let shallow_root_capacity = transport_ref_collection_capacity(shallow_roots.len());
        let mut unique_roots = HashSet::with_capacity(shallow_root_capacity);
        let mut roots = Vec::with_capacity(shallow_root_capacity);
        for id in shallow_roots {
            if unique_roots.insert(id.clone()) {
                roots.push(id);
            }
        }
        write_shallow_file(
            &destination,
            shallow_boundaries(&source_store, &roots, depth)?,
        )?;
    }
    if options.include_tag {
        copy_fetch_pack_included_tags(
            &source_refs,
            &source_store,
            &destination_store,
            &source_repo,
            &mut fetched_objects,
            options.depth,
        )?;
    }
    let _ = (options.thin, options.no_progress);
    Ok(())
}

pub(crate) fn copy_reachable_objects(
    repo: &GitRepo,
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    id: &ObjectId,
) -> Result<HashSet<ObjectId>> {
    let mut seen = HashSet::with_capacity(copy_reachable_seen_initial_capacity(
        source.object_id_capacity_hint()?,
        1,
    ));
    copy_reachable_objects_into(repo, source, destination, id, &mut seen)?;
    Ok(seen)
}

fn copy_reachable_seen_initial_capacity(store_hint: usize, roots_len: usize) -> usize {
    store_hint
        .max(roots_len)
        .min(COPY_REACHABLE_SEEN_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn copy_reachable_objects_into(
    repo: &GitRepo,
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    copy_reachable_objects_inner(repo, source, destination, id, seen)
}

fn copy_reachable_objects_inner(
    repo: &GitRepo,
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    let mut current = id.clone();
    loop {
        if !copy_object_if_missing(source, destination, &current, seen)? {
            return Ok(());
        }
        let kind = object_kind_hint_or_read(source, &current)?;
        if kind == GitObjectKind::Tag {
            let object = source.read_object(&current)?;
            let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
            current = tag.target;
            continue;
        }
        if kind != GitObjectKind::Commit {
            return Ok(());
        }
        break;
    }
    let commit_cache = CommitObjectCache::new(source);
    let commits =
        collect_commits_from_ids_cached(repo, &commit_cache, std::slice::from_ref(&current), None)?;
    reserve_transport_history_set(seen, commits.len());
    for commit in &commits {
        let _ = copy_object_if_missing(source, destination, commit, seen)?;
    }
    for_each_rev_list_object_id_into_cached(
        source,
        &commit_cache,
        &commits,
        &[],
        &[],
        seen,
        |object_id| copy_object_payload_if_missing(source, destination, object_id),
    )?;
    Ok(())
}

fn copy_reachable_objects_for_depth_into(
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    commit_ids: &[ObjectId],
    seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    reserve_transport_history_set(seen, commit_ids.len());
    for commit_id in commit_ids {
        let _ = copy_object_if_missing(source, destination, commit_id, seen)?;
    }
    let commit_cache = CommitObjectCache::new(source);
    for_each_rev_list_object_id_into_cached(
        source,
        &commit_cache,
        commit_ids,
        &[],
        &[],
        seen,
        |object_id| copy_object_payload_if_missing(source, destination, object_id),
    )?;
    Ok(())
}

fn copy_object_if_missing(
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
) -> Result<bool> {
    if !seen.insert(id.clone()) {
        return Ok(false);
    }
    copy_object_payload_if_missing(source, destination, id)?;
    Ok(true)
}

fn copy_object_payload_if_missing(
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    id: &ObjectId,
) -> Result<()> {
    match destination.contains_object(id) {
        Ok(true) => Ok(()),
        Ok(false) => copy_object_payload_to_known_missing_destination(source, destination, id),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            copy_object_payload_to_known_missing_destination(source, destination, id)
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn copy_object_payload_to_known_missing_destination(
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    id: &ObjectId,
) -> Result<()> {
    if source.copy_loose_object_to_known_missing(destination, id)? {
        return Ok(());
    }
    if copy_streamable_blob_object(source, destination, id)? {
        return Ok(());
    }
    let object = source.read_object(id)?;
    destination.write_object(object.kind, &object.content)?;
    Ok(())
}

fn copy_streamable_blob_object(
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    id: &ObjectId,
) -> Result<bool> {
    let Some(size) = source.streamable_blob_size_hint(id)? else {
        return Ok(false);
    };
    match destination.write_streamed_blob(id, size, |writer| {
        if source.write_streamable_blob(id, writer)? {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "streamable blob disappeared while copying",
            ))
        }
    }) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn copy_fetch_pack_included_tags(
    refs: &RefStore,
    source: &LooseObjectStore,
    destination: &LooseObjectStore,
    repo: &GitRepo,
    fetched_objects: &mut HashSet<ObjectId>,
    depth: Option<usize>,
) -> Result<()> {
    let mut tag_ids = Vec::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
    refs.for_each_resolved_ref("refs/tags/", |_, id| {
        tag_ids.push(id.clone());
        Ok::<(), CliError>(())
    })?;
    let mut changed = true;
    while changed {
        changed = false;
        for tag_id in &tag_ids {
            if fetched_objects.contains(tag_id) {
                continue;
            }
            if object_kind_hint_or_read(source, tag_id)? != GitObjectKind::Tag {
                continue;
            }
            if fetch_pack_tag_peels_into(source, tag_id, fetched_objects)? {
                if depth.is_some() {
                    let before = fetched_objects.len();
                    let _ = copy_object_if_missing(source, destination, tag_id, fetched_objects)?;
                    changed |= fetched_objects.len() != before;
                } else {
                    let before = fetched_objects.len();
                    copy_reachable_objects_into(
                        repo,
                        source,
                        destination,
                        tag_id,
                        fetched_objects,
                    )?;
                    changed |= fetched_objects.len() != before;
                    continue;
                }
            }
        }
    }
    Ok(())
}

fn fetch_pack_tag_peels_into(
    source: &LooseObjectStore,
    tag_id: &ObjectId,
    fetched_objects: &HashSet<ObjectId>,
) -> Result<bool> {
    let mut current = tag_id.clone();
    let mut seen = HashSet::with_capacity(TAG_PEEL_SEEN_CAPACITY_HINT);
    loop {
        if !seen.insert(current.clone()) {
            return Ok(false);
        }
        let object = source.read_object(&current)?;
        if object.kind != GitObjectKind::Tag {
            return Ok(fetched_objects.contains(&current));
        }
        let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
        if fetched_objects.contains(&tag.target) {
            return Ok(true);
        }
        if tag.target_kind != GitObjectKind::Tag {
            return Ok(false);
        }
        current = tag.target;
    }
}

pub(crate) fn send_pack(options: SendPackOptions) -> Result<()> {
    if options.receive_pack.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "send-pack currently supports local refs without optional protocol modes"
                .into(),
        });
    }
    let repo = find_repo_or_bare()?;
    let destination_path = absolute_path_from_arg(std::path::Path::new(&options.directory))?;
    let destination = local_clone_source(&destination_path)?;
    let source_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let destination_refs = refs_adapter_from_git_dir(&destination.git_dir);
    let source_store = object_adapter_from_objects_dir(&repo.objects_dir);
    let destination_store = object_adapter_from_objects_dir(destination.git_dir.join("objects"));
    let destination_commit_cache = CommitObjectCache::new(&destination_store);

    let mut specs = options.refs;
    if options.stdin {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::with_capacity(TRANSPORT_STDIN_BUF_CAPACITY, stdin.lock());
        collect_trimmed_lines_from_reader(&mut stdin, &mut specs)?;
    }
    if options.mirror {
        specs = send_pack_mirror_refspecs(&source_refs, &destination_refs)?;
    } else if options.all {
        source_refs.for_each_ref_name("refs/heads/", |ref_name| {
            specs.push(ref_name.to_owned());
            Ok::<(), CliError>(())
        })?;
    }
    if specs.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "send-pack requires at least one ref unless --all is used".into(),
        });
    }
    specs.sort();
    specs.dedup();

    let initial_capacity = transport_ref_collection_capacity(specs.len());
    let mut push_refs = Vec::with_capacity(initial_capacity);
    for spec in specs {
        let push_ref = parse_push_refspec(&repo, &source_refs, &spec, &options.directory)?;
        validate_push_update(
            &destination_refs,
            &destination_commit_cache,
            &push_ref,
            options.mirror || options.force || push_ref.force,
        )?;
        if push_ref.id.is_none() {
            validate_push_delete(&destination_refs, &push_ref.destination)?;
        }
        push_refs.push(push_ref);
    }

    let mut copied = HashSet::with_capacity(initial_capacity);
    for push_ref in push_refs {
        if !options.dry_run {
            if let Some(id) = &push_ref.id {
                copy_reachable_objects_into(
                    &repo,
                    &source_store,
                    &destination_store,
                    id,
                    &mut copied,
                )?;
                destination_refs.write_ref(&push_ref.destination, id)?;
            } else {
                destination_refs.delete_ref(&push_ref.destination)?;
            }
        }
        if options.verbose && !options.dry_run {
            let display = push_ref
                .id
                .as_ref()
                .map(ObjectId::to_hex)
                .unwrap_or_else(|| "(delete)".to_owned());
            eprintln!("{} -> {}", display, push_ref.destination);
        }
    }
    let _ = (options.thin, options.atomic, copied);
    Ok(())
}

fn send_pack_mirror_refspecs(
    source_refs: &RefStore,
    destination_refs: &RefStore,
) -> Result<Vec<String>> {
    let mut specs = Vec::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
    let mut source_names = HashSet::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
    source_refs.for_each_ref_name("refs/", |ref_name| {
        source_names.insert(ref_name.to_owned());
        specs.push(format!("+{ref_name}:{ref_name}"));
        Ok::<(), CliError>(())
    })?;
    destination_refs.for_each_ref_name("refs/", |ref_name| {
        if !source_names.contains(ref_name) {
            specs.push(format!(":{ref_name}"));
        }
        Ok::<(), CliError>(())
    })?;
    Ok(specs)
}

#[derive(Debug, Clone)]
struct ParsedDaemonUrl {
    host: String,
    port: u16,
    path: String,
}

#[derive(Debug, Clone)]
struct ParsedSshUrl {
    user: Option<String>,
    host: String,
    port: Option<u16>,
    path: String,
}

struct RemoteCommandSession {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<io::BufReader<std::process::ChildStdout>>,
    stderr: Option<std::process::ChildStderr>,
}

#[derive(Debug, Clone)]
struct ReceivePackAdvertisement {
    refs: BTreeMap<String, ObjectId>,
    capabilities: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LsRemoteRow {
    pub(crate) id: ObjectId,
    pub(crate) name: String,
}

pub(crate) struct CloneCommandInput {
    pub(crate) quiet: bool,
    pub(crate) reject_shallow: bool,
    pub(crate) no_reject_shallow: bool,
    pub(crate) template: Option<PathBuf>,
    pub(crate) no_template: bool,
    pub(crate) configs: Vec<String>,
    pub(crate) no_checkout: bool,
    pub(crate) checkout: bool,
    pub(crate) recurse_submodules: Vec<String>,
    pub(crate) recursive: Vec<String>,
    pub(crate) no_recurse_submodules: bool,
    pub(crate) jobs: Option<String>,
    pub(crate) shallow_submodules: bool,
    pub(crate) remote_submodules: bool,
    pub(crate) origin: String,
    pub(crate) no_tags: bool,
    pub(crate) tags: bool,
    pub(crate) single_branch: bool,
    pub(crate) no_single_branch: bool,
    pub(crate) separate_git_dir: Option<PathBuf>,
    pub(crate) references: Vec<PathBuf>,
    pub(crate) reference_if_able: Vec<PathBuf>,
    pub(crate) shared: bool,
    pub(crate) dissociate: bool,
    pub(crate) no_hardlinks: bool,
    pub(crate) hardlinks: bool,
    pub(crate) depth: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) repository: String,
    pub(crate) directory: Option<PathBuf>,
    pub(crate) bare: bool,
    pub(crate) mirror: bool,
}

pub(crate) fn clone(options: CloneOptions) -> Result<()> {
    let CloneOptions {
        quiet,
        configs,
        template,
        reject_shallow,
        recurse_submodules,
        remote_submodules,
        shallow_submodules,
        bare,
        mirror,
        no_checkout,
        remote_name,
        no_tags,
        single_branch,
        no_single_branch,
        separate_git_dir,
        references,
        reference_if_able,
        shared,
        dissociate,
        no_hardlinks,
        depth,
        branch,
        keep_partial_on_missing_branch,
        repository,
        directory,
    } = options;
    let depth = depth.as_deref().map(validate_positive_depth).transpose()?;
    let effective_bare = bare || mirror;
    if effective_bare && separate_git_dir.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--bare' and '--separate-git-dir' cannot be used together".into(),
        });
    }
    if is_http_transport_url(&repository) {
        return clone_dumb_http(CloneHttpOptions {
            quiet,
            configs,
            template,
            reject_shallow,
            recurse_submodules,
            remote_submodules,
            shallow_submodules,
            effective_bare,
            mirror,
            no_checkout,
            remote_name,
            no_tags,
            single_branch,
            no_single_branch,
            separate_git_dir,
            references,
            reference_if_able,
            shared,
            dissociate,
            depth,
            branch,
            keep_partial_on_missing_branch: false,
            repository,
            directory,
        });
    }
    if is_git_daemon_transport_url(&repository) {
        return clone_git_daemon(CloneHttpOptions {
            quiet,
            configs,
            template,
            reject_shallow,
            recurse_submodules,
            remote_submodules,
            shallow_submodules,
            effective_bare,
            mirror,
            no_checkout,
            remote_name,
            no_tags,
            single_branch,
            no_single_branch,
            separate_git_dir,
            references,
            reference_if_able,
            shared,
            dissociate,
            depth,
            branch,
            keep_partial_on_missing_branch: false,
            repository,
            directory,
        });
    }
    if is_ssh_transport_url(&repository) {
        return clone_ssh(CloneHttpOptions {
            quiet,
            configs,
            template,
            reject_shallow,
            recurse_submodules,
            remote_submodules,
            shallow_submodules,
            effective_bare,
            mirror,
            no_checkout,
            remote_name,
            no_tags,
            single_branch,
            no_single_branch,
            separate_git_dir,
            references,
            reference_if_able,
            shared,
            dissociate,
            depth,
            branch,
            keep_partial_on_missing_branch: false,
            repository,
            directory,
        });
    }
    let Some(source_path) = local_repository_path_from_location(&repository)? else {
        let destination_label =
            unsupported_clone_destination_label(&repository, directory.as_deref());
        let prefix = if quiet {
            String::new()
        } else {
            format!("Cloning into '{destination_label}'...\n")
        };
        return Err(unsupported_remote_helper_error(&repository, prefix));
    };
    let _trace = phase_trace("clone_local");

    let source = local_clone_source(&source_path)?;
    let destination = match &directory {
        Some(path) => absolute_path_from_arg(path)?,
        None => default_clone_directory(&source_path, effective_bare)?,
    };
    let destination_existed = destination.exists();
    let destination_label = clone_destination_label(directory.as_deref(), &destination);
    ensure_clone_destination(&destination, &destination_label)?;
    let shallow_file_clone = depth.is_some() && repository.starts_with("file://");
    if depth.is_some() && !shallow_file_clone {
        eprintln!("warning: --depth is ignored in local clones; use file:// instead.");
    }
    if !quiet {
        if effective_bare {
            eprintln!("Cloning into bare repository '{destination_label}'...");
        } else {
            eprintln!("Cloning into '{destination_label}'...");
        }
    }
    if reject_shallow && is_shallow_git_dir(&source.git_dir) {
        return Err(CliError::Fatal {
            code: 128,
            message: "source repository is shallow, reject to clone.".into(),
        });
    }
    validate_remote_name(&remote_name)?;
    let mut reference_object_dirs = reference_object_dirs(&references)?;
    reference_object_dirs.extend(reference_if_able_object_dirs(&reference_if_able));
    let effective_single_branch = !no_single_branch && (single_branch || depth.is_some());

    let source_refs = refs_adapter_from_git_dir(&source.git_dir);
    if shared {
        reference_object_dirs.push(canonical_or_absolute(source.git_dir.join("objects")));
    }
    let source_store = object_adapter_from_objects_dir(source.git_dir.join("objects"));
    let head_branch = source_head_branch(&source_refs)?;
    let target = clone_target(
        &source_refs,
        &source_store,
        branch.as_deref(),
        head_branch.as_deref(),
        keep_partial_on_missing_branch,
    )?;
    let initial_branch = target
        .branch_name()
        .or(head_branch.as_deref())
        .unwrap_or("main")
        .to_owned();
    let result = {
        let _trace = phase_trace("clone_local.init_repository");
        init_repository(
            &destination,
            InitRepositoryOptions {
                bare: effective_bare,
                initial_branch,
            },
        )?
    };
    let git_dir = match separate_git_dir {
        Some(path) => relocate_separate_git_dir(&destination, &result.git_dir, &path)?,
        None => result.git_dir.clone(),
    };
    let repo = GitRepo {
        root: result.worktree,
        git_dir: git_dir.clone(),
        objects_dir: git_dir.join("objects"),
        index_path: git_dir.join("index"),
    };
    if let Some(template) = template.as_ref() {
        apply_clone_template(&repo, template)?;
    }
    let apply_configs_result = {
        let _trace = phase_trace("clone_local.apply_configs");
        apply_clone_configs(&repo, &configs)
    };
    if let Err(error) = apply_configs_result {
        cleanup_failed_clone_config(&destination, &repo.git_dir, destination_existed);
        return Err(error);
    }
    if !dissociate {
        let _trace = phase_trace("clone_local.write_alternates");
        write_alternates_file(&repo.objects_dir, &reference_object_dirs)?;
    }
    if !shared || dissociate {
        let _trace = phase_trace("clone_local.validate_ownership");
        validate_local_clone_ownership(&source.git_dir, &repo.git_dir)?;
        drop(_trace);
        if no_hardlinks {
            let _trace = phase_trace("clone_local.copy_objects");
            copy_dir_contents_to_fresh_destination(
                &source.git_dir.join("objects"),
                &repo.objects_dir,
            )?;
        } else {
            let _trace = phase_trace("clone_local.hardlink_objects");
            hardlink_dir_contents_to_fresh_destination(
                &source.git_dir.join("objects"),
                &repo.objects_dir,
            )?;
        }
    } else {
        let _trace = phase_trace("clone_local.validate_security");
        validate_local_clone_security(&source.git_dir, &repo.git_dir)?;
    }

    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    {
        let _trace = phase_trace("clone_local.write_refs");
        if mirror {
            write_fresh_mirror_clone_refs(&source_refs, &destination_refs)?;
        } else if effective_bare {
            write_fresh_bare_clone_refs(&source_refs, &destination_refs, !no_tags)?;
        } else if matches!(target, CloneTarget::MissingBranch { .. }) {
            write_fresh_clone_remote_refs(
                &source_refs,
                &destination_refs,
                &remote_name,
                head_branch.as_deref(),
                !no_tags,
            )?;
        } else if effective_single_branch {
            write_fresh_head_remote_ref(
                &source_refs,
                &destination_refs,
                &remote_name,
                target.branch_name(),
                branch.is_none(),
                branch.is_none() && !no_tags,
            )?;
            if let CloneTarget::Tag { name, .. } = &target {
                copy_single_tag_ref(&source_refs, &destination_refs, name)?;
            }
        } else {
            write_fresh_clone_remote_refs(
                &source_refs,
                &destination_refs,
                &remote_name,
                head_branch.as_deref(),
                !no_tags,
            )?;
        }
    }
    let mut clone_config_values = Vec::with_capacity(CLONE_CONFIG_VALUES_CAPACITY_HINT);
    clone_config_values.push((format!("remote.{remote_name}.url"), repository.clone()));
    if !effective_bare {
        clone_config_values.push((
            format!("remote.{remote_name}.fetch"),
            clone_fetch_refspec(&remote_name, &target, effective_single_branch),
        ));
    } else if mirror {
        clone_config_values.push((
            format!("remote.{remote_name}.fetch"),
            "+refs/*:refs/*".into(),
        ));
        clone_config_values.push((format!("remote.{remote_name}.mirror"), "true".into()));
    }
    if no_tags || mirror {
        clone_config_values.push((format!("remote.{remote_name}.tagOpt"), "--no-tags".into()));
    }
    {
        let _trace = phase_trace("clone_local.write_config");
        set_config_values(&repo, &clone_config_values)?;
    }

    if let CloneTarget::MissingBranch { name } = &target {
        destination_refs.write_head_symbolic(&format!("refs/heads/{name}"))?;
        set_config_value(&repo, &format!("branch.{name}.remote"), &remote_name)?;
        set_config_value(
            &repo,
            &format!("branch.{name}.merge"),
            &format!("refs/heads/{name}"),
        )?;
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "fatal: 'origin/{name}' is not a commit and a branch '{name}' cannot be created from it\n"
            ),
        });
    }

    let head_id = match target {
        CloneTarget::Branch { ref id, .. }
        | CloneTarget::Tag { ref id, .. }
        | CloneTarget::Detached { ref id } => id.clone(),
        CloneTarget::MissingBranch { .. } => unreachable!("handled missing clone branch"),
        CloneTarget::Empty => {
            println!("warning: You appear to have cloned an empty repository.");
            return Ok(());
        }
    };

    if effective_bare {
        match &target {
            CloneTarget::Branch { name, .. } => {
                destination_refs.write_head_symbolic(&format!("refs/heads/{name}"))?;
            }
            CloneTarget::Tag { id, .. } | CloneTarget::Detached { id } => {
                destination_refs.write_head_direct(id)?;
            }
            CloneTarget::MissingBranch { .. } => unreachable!("handled missing clone branch"),
            CloneTarget::Empty => {}
        }
    } else if let CloneTarget::Branch { name: branch, .. } = &target {
        destination_refs.write_ref(&format!("refs/heads/{branch}"), &head_id)?;
        destination_refs.write_head_symbolic(&format!("refs/heads/{branch}"))?;
        set_config_values(
            &repo,
            &[
                (format!("branch.{branch}.remote"), remote_name.clone()),
                (
                    format!("branch.{branch}.merge"),
                    format!("refs/heads/{branch}"),
                ),
            ],
        )?;
    } else {
        destination_refs.write_head_direct(&head_id)?;
    }
    if let Some(depth) = depth.filter(|_| shallow_file_clone) {
        let roots = if no_single_branch {
            branch_head_ids(&source_refs)?
        } else {
            vec![head_id.clone()]
        };
        write_shallow_file(&repo, shallow_boundaries(&source_store, &roots, depth)?)?;
    }
    if effective_bare || no_checkout {
        return Ok(());
    }

    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    {
        let _trace = phase_trace("clone_local.checkout");
        checkout_fresh_worktree(&repo, &store, &head_id)?;
    }
    if !recurse_submodules.is_empty() {
        let _trace = phase_trace("clone_local.submodules");
        clone_submodules(
            &repo,
            &repository,
            &recurse_submodules,
            remote_submodules,
            shallow_submodules,
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum CloneTarget {
    Empty,
    Branch { name: String, id: ObjectId },
    MissingBranch { name: String },
    Tag { name: String, id: ObjectId },
    Detached { id: ObjectId },
}

impl CloneTarget {
    fn branch_name(&self) -> Option<&str> {
        match self {
            Self::Branch { name, .. } | Self::MissingBranch { name } => Some(name),
            Self::Empty | Self::Tag { .. } | Self::Detached { .. } => None,
        }
    }
}

struct CloneHttpOptions {
    quiet: bool,
    configs: Vec<String>,
    template: Option<PathBuf>,
    reject_shallow: bool,
    recurse_submodules: Vec<String>,
    remote_submodules: bool,
    shallow_submodules: bool,
    effective_bare: bool,
    mirror: bool,
    no_checkout: bool,
    remote_name: String,
    no_tags: bool,
    single_branch: bool,
    no_single_branch: bool,
    separate_git_dir: Option<PathBuf>,
    references: Vec<PathBuf>,
    reference_if_able: Vec<PathBuf>,
    shared: bool,
    dissociate: bool,
    depth: Option<usize>,
    branch: Option<String>,
    keep_partial_on_missing_branch: bool,
    repository: String,
    directory: Option<PathBuf>,
}

fn clone_dumb_http(options: CloneHttpOptions) -> Result<()> {
    let _trace = phase_trace("clone_http");
    if !options.references.is_empty() || !options.reference_if_able.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "reference repositories are not supported for dumb HTTP clone yet".into(),
        });
    }
    let _ = (
        options.remote_submodules,
        options.shallow_submodules,
        options.keep_partial_on_missing_branch,
    );
    validate_remote_name(&options.remote_name)?;
    let _ = options.reject_shallow;

    let destination = match &options.directory {
        Some(path) => absolute_path_from_arg(path)?,
        None => default_http_clone_directory(&options.repository, options.effective_bare)?,
    };
    let destination_existed = destination.exists();
    let destination_label = clone_destination_label(options.directory.as_deref(), &destination);
    ensure_clone_destination(&destination, &destination_label)?;
    if !options.quiet {
        if options.effective_bare {
            eprintln!("Cloning into bare repository '{destination_label}'...");
        } else {
            eprintln!("Cloning into '{destination_label}'...");
        }
    }

    let url = parsed_http_url_with_extra_headers(None, &options.repository)?;
    let mut helper = if url.scheme == HttpScheme::Https {
        Some(RemoteHttpHelperSession::spawn(&url)?)
    } else {
        None
    };
    let (refs, head_branch) = {
        let _trace = phase_trace("clone_http.discovery");
        discover_http_refs_with_helper(
            &url,
            helper.as_mut().map(std::convert::identity),
            false,
            false,
            false,
            &[],
        )?
    };
    let target = http_clone_target(&refs, options.branch.as_deref(), head_branch.as_deref())?;
    let initial_branch = target
        .branch_name()
        .or(head_branch.as_deref())
        .unwrap_or("main")
        .to_owned();
    let result = {
        let _trace = phase_trace("clone_http.init_repository");
        init_repository(
            &destination,
            InitRepositoryOptions {
                bare: options.effective_bare,
                initial_branch,
            },
        )?
    };
    let git_dir = match options.separate_git_dir {
        Some(path) => relocate_separate_git_dir(&destination, &result.git_dir, &path)?,
        None => result.git_dir.clone(),
    };
    let repo = GitRepo {
        root: result.worktree,
        git_dir: git_dir.clone(),
        objects_dir: git_dir.join("objects"),
        index_path: git_dir.join("index"),
    };
    if let Some(template) = options.template.as_ref() {
        apply_clone_template(&repo, template)?;
    }
    if let Err(error) = apply_clone_configs(&repo, &options.configs) {
        cleanup_failed_clone_config(&destination, &repo.git_dir, destination_existed);
        return Err(error);
    }

    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let roots = http_clone_fetch_roots(
        &refs,
        &target,
        options.no_tags,
        options.single_branch,
        options.no_single_branch,
    );
    let shallow_boundaries = {
        let _trace = phase_trace("clone_http.fetch_objects");
        if let Some(depth) = options.depth {
            if let Some(helper) = helper.as_mut() {
                http_fetch_smart_pack_with_depth_with_helper(
                    &url,
                    helper,
                    &repo.objects_dir,
                    &roots,
                    Some(depth),
                )?
            } else {
                http_fetch_smart_pack_with_depth_direct(
                    &url,
                    &repo.objects_dir,
                    &roots,
                    Some(depth),
                )?
            }
        } else {
            let pack_fetched = if let Some(helper) = helper.as_mut() {
                http_fetch_smart_pack_with_helper(&url, helper, &repo.objects_dir, &roots)?
            } else {
                http_fetch_smart_pack_direct(&url, &repo.objects_dir, &roots)?
            };
            if !pack_fetched {
                let helper = helper.get_or_insert(RemoteHttpHelperSession::spawn(&url)?);
                let commit_cache = CommitObjectCache::new(&store);
                let tree_cache = TreeObjectCache::new(&store);
                let mut seen =
                    HashSet::with_capacity(transport_ref_collection_capacity(roots.len()));
                let fetch_options = HttpFetchOptions {
                    commit: false,
                    tags: false,
                    all: true,
                    verbose: false,
                    recover: false,
                    write_ref: Vec::new(),
                    stdin: false,
                    packfile: None,
                    index_pack_args: Vec::new(),
                    args: Vec::new(),
                };
                let mut fetch_context = HttpFetchObjectContext {
                    url: &url,
                    helper,
                    store: &store,
                    commit_cache: &commit_cache,
                    tree_cache: &tree_cache,
                    options: &fetch_options,
                    seen: &mut seen,
                    suffix_buffer: String::new(),
                };
                for id in &roots {
                    http_fetch_object_recursive(&mut fetch_context, id)?;
                }
            }
            Vec::new()
        }
    };
    if let Some(depth) = options.depth {
        let shallow_roots = clone_shallow_roots(
            &repo,
            &http_clone_fetch_roots(
                &refs,
                &target,
                options.no_tags,
                options.single_branch,
                options.no_single_branch,
            ),
        )?;
        write_shallow_file(
            &repo,
            boundaries_or_local_fallback(&repo, &shallow_roots, depth, shallow_boundaries)?,
        )?;
    }

    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    {
        let _trace = phase_trace("clone_http.write_refs");
        http_write_clone_refs(
            &destination_refs,
            &refs,
            &options.remote_name,
            head_branch.as_deref(),
            &target,
            HttpCloneRefOptions {
                mirror: options.mirror,
                effective_bare: options.effective_bare,
                no_tags: options.no_tags,
                single_branch: options.single_branch,
                no_single_branch: options.no_single_branch,
                requested_branch: options.branch.is_some(),
            },
        )?;
    }
    if options.depth.is_some() {
        prune_missing_tag_refs(&destination_refs, &store)?;
    }
    set_config_value(
        &repo,
        &format!("remote.{}.url", options.remote_name),
        &options.repository,
    )?;
    if !options.effective_bare {
        set_config_value(
            &repo,
            &format!("remote.{}.fetch", options.remote_name),
            &clone_fetch_refspec(
                &options.remote_name,
                &target,
                !options.no_single_branch && options.single_branch,
            ),
        )?;
    } else if options.mirror {
        set_config_value(
            &repo,
            &format!("remote.{}.fetch", options.remote_name),
            "+refs/*:refs/*",
        )?;
        set_config_value(
            &repo,
            &format!("remote.{}.mirror", options.remote_name),
            "true",
        )?;
    }
    if options.no_tags || options.mirror {
        set_config_value(
            &repo,
            &format!("remote.{}.tagOpt", options.remote_name),
            "--no-tags",
        )?;
    }

    let head_id = match target {
        CloneTarget::Branch { ref id, .. }
        | CloneTarget::Tag { ref id, .. }
        | CloneTarget::Detached { ref id } => id.clone(),
        CloneTarget::MissingBranch { .. } => {
            unreachable!("HTTP clone does not keep missing branch")
        }
        CloneTarget::Empty => {
            println!("warning: You appear to have cloned an empty repository.");
            return Ok(());
        }
    };
    if options.effective_bare {
        match &target {
            CloneTarget::Branch { name, .. } => {
                destination_refs.write_head_symbolic(&format!("refs/heads/{name}"))?;
            }
            CloneTarget::Tag { id, .. } | CloneTarget::Detached { id } => {
                destination_refs.write_head_direct(id)?;
            }
            CloneTarget::MissingBranch { .. } => {
                unreachable!("HTTP clone does not keep missing branch")
            }
            CloneTarget::Empty => {}
        }
    } else if let CloneTarget::Branch { name: branch, .. } = &target {
        destination_refs.write_ref(&format!("refs/heads/{branch}"), &head_id)?;
        destination_refs.write_head_symbolic(&format!("refs/heads/{branch}"))?;
        set_config_value(
            &repo,
            &format!("branch.{branch}.remote"),
            &options.remote_name,
        )?;
        set_config_value(
            &repo,
            &format!("branch.{branch}.merge"),
            &format!("refs/heads/{branch}"),
        )?;
    } else {
        destination_refs.write_head_direct(&head_id)?;
    }
    if options.effective_bare || options.no_checkout {
        return Ok(());
    }
    {
        let _trace = phase_trace("clone_http.checkout");
        checkout_fresh_worktree(&repo, &store, &head_id)?;
    }
    if !options.recurse_submodules.is_empty() {
        clone_submodules(
            &repo,
            &options.repository,
            &options.recurse_submodules,
            options.remote_submodules,
            options.shallow_submodules,
        )?;
    }
    Ok(())
}

fn default_http_clone_directory(url: &str, bare: bool) -> Result<PathBuf> {
    let parsed = ParsedHttpUrl::parse(url)?;
    let source_name = parsed
        .path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "cannot infer clone directory".into(),
        })?;
    let name = if bare {
        if source_name.ends_with(".git") {
            source_name.to_owned()
        } else {
            format!("{source_name}.git")
        }
    } else {
        source_name.trim_end_matches(".git").to_owned()
    };
    Ok(std::env::current_dir()?.join(name))
}

fn default_daemon_clone_directory(url: &str, bare: bool) -> Result<PathBuf> {
    let parsed = ParsedDaemonUrl::parse(url)?;
    let source_name = parsed
        .path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "cannot infer clone directory".into(),
        })?;
    let name = if bare {
        if source_name.ends_with(".git") {
            source_name.to_owned()
        } else {
            format!("{source_name}.git")
        }
    } else {
        source_name.trim_end_matches(".git").to_owned()
    };
    Ok(std::env::current_dir()?.join(name))
}

fn http_head_branch_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
) -> Result<Option<String>> {
    let Some(body) = http_get_optional_with_helper(url, helper, "HEAD")? else {
        return Ok(None);
    };
    let value = String::from_utf8_lossy(&body);
    Ok(value
        .trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned))
}

fn http_head_branch_direct(url: &ParsedHttpUrl) -> Result<Option<String>> {
    let Some(body) = http_get_optional_direct(url, "HEAD")? else {
        return Ok(None);
    };
    let value = String::from_utf8_lossy(&body);
    Ok(value
        .trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned))
}

struct SmartHttpDiscovery {
    rows: Vec<LsRemoteRow>,
    head_branch: Option<String>,
}

enum HttpDiscoveryTransport<'a> {
    Direct,
    Helper(&'a mut RemoteHttpHelperSession),
}

fn discover_http_refs(
    url: &ParsedHttpUrl,
    transport: HttpDiscoveryTransport<'_>,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<(Vec<LsRemoteRow>, Option<String>)> {
    match transport {
        HttpDiscoveryTransport::Direct => {
            if let Some(discovery) =
                http_smart_discovery_direct(url, heads, tags, refs_only, patterns)?
            {
                return Ok((discovery.rows, discovery.head_branch));
            }
            let rows = http_ls_remote_rows_direct(url, heads, tags, refs_only, patterns)?;
            let head_branch = http_head_branch_direct(url)?;
            Ok((rows, head_branch))
        }
        HttpDiscoveryTransport::Helper(helper) => {
            if let Some(discovery) =
                http_smart_discovery_with_helper(url, helper, heads, tags, refs_only, patterns)?
            {
                return Ok((discovery.rows, discovery.head_branch));
            }
            let rows =
                http_ls_remote_rows_with_helper(url, helper, heads, tags, refs_only, patterns)?;
            let head_branch = http_head_branch_with_helper(url, helper)?;
            Ok((rows, head_branch))
        }
    }
}

fn discover_http_refs_with_helper(
    url: &ParsedHttpUrl,
    helper: Option<&mut RemoteHttpHelperSession>,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<(Vec<LsRemoteRow>, Option<String>)> {
    let transport = match helper {
        Some(helper) => HttpDiscoveryTransport::Helper(helper),
        None => HttpDiscoveryTransport::Direct,
    };
    discover_http_refs(url, transport, heads, tags, refs_only, patterns)
}

fn http_clone_target(
    refs: &[LsRemoteRow],
    requested: Option<&str>,
    head_branch: Option<&str>,
) -> Result<CloneTarget> {
    let Some(requested) = requested else {
        return Ok(refs
            .iter()
            .find(|row| row.name == "HEAD")
            .map(|row| {
                head_branch
                    .map(|name| CloneTarget::Branch {
                        name: name.to_owned(),
                        id: row.id.clone(),
                    })
                    .unwrap_or_else(|| CloneTarget::Detached { id: row.id.clone() })
            })
            .unwrap_or(CloneTarget::Empty));
    };
    let branch_ref = branch_ref_name(requested)?;
    if let Some(row) = refs.iter().find(|row| row.name == branch_ref) {
        return Ok(CloneTarget::Branch {
            name: requested.to_owned(),
            id: row.id.clone(),
        });
    }
    let tag_ref = tag_ref_name(requested)?;
    if let Some(row) = refs
        .iter()
        .find(|row| is_peeled_tag_ref(&row.name, &tag_ref))
    {
        return Ok(CloneTarget::Tag {
            name: requested.to_owned(),
            id: row.id.clone(),
        });
    }
    if let Some(row) = refs.iter().find(|row| row.name == tag_ref) {
        return Ok(CloneTarget::Tag {
            name: requested.to_owned(),
            id: row.id.clone(),
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("Remote branch {requested} not found in upstream origin"),
    })
}

fn is_peeled_tag_ref(ref_name: &str, tag_ref: &str) -> bool {
    ref_name.len() == tag_ref.len() + "^{}".len()
        && ref_name.starts_with(tag_ref)
        && ref_name.as_bytes()[tag_ref.len()..] == *b"^{}"
}

fn http_clone_fetch_roots(
    refs: &[LsRemoteRow],
    target: &CloneTarget,
    no_tags: bool,
    single_branch: bool,
    no_single_branch: bool,
) -> Vec<ObjectId> {
    let mut roots = Vec::with_capacity(transport_ref_collection_capacity(refs.len()));
    let effective_single_branch = !no_single_branch && single_branch;
    if effective_single_branch {
        match target {
            CloneTarget::Branch { id, .. }
            | CloneTarget::Tag { id, .. }
            | CloneTarget::Detached { id } => roots.push(id.clone()),
            CloneTarget::MissingBranch { .. } => {}
            CloneTarget::Empty => {}
        }
    } else {
        for row in refs {
            if row.name == "HEAD" || row.name.ends_with("^{}") {
                continue;
            }
            if no_tags && row.name.starts_with("refs/tags/") {
                continue;
            }
            if row.name.starts_with("refs/") {
                roots.push(row.id.clone());
            }
        }
    }
    sort_dedup_object_ids(&mut roots);
    roots
}

struct HttpCloneRefOptions {
    mirror: bool,
    effective_bare: bool,
    no_tags: bool,
    single_branch: bool,
    no_single_branch: bool,
    requested_branch: bool,
}

fn http_write_clone_refs(
    destination: &RefStore,
    refs: &[LsRemoteRow],
    remote: &str,
    head_branch: Option<&str>,
    target: &CloneTarget,
    options: HttpCloneRefOptions,
) -> Result<()> {
    if options.mirror {
        for row in refs.iter().filter(|row| row.name.starts_with("refs/")) {
            if !row.name.ends_with("^{}") {
                destination.write_ref(&row.name, &row.id)?;
            }
        }
        return Ok(());
    }
    if options.effective_bare {
        for row in refs.iter().filter(|row| row.name.starts_with("refs/")) {
            if !row.name.ends_with("^{}")
                && (!options.no_tags || !row.name.starts_with("refs/tags/"))
            {
                destination.write_ref(&row.name, &row.id)?;
            }
        }
        return Ok(());
    }
    let effective_single_branch = !options.no_single_branch && options.single_branch;
    if effective_single_branch {
        if let CloneTarget::Branch { name, id } = target {
            destination.write_ref(&format!("refs/remotes/{remote}/{name}"), id)?;
            if !options.requested_branch {
                destination.write_symbolic_ref(
                    &format!("refs/remotes/{remote}/HEAD"),
                    &format!("refs/remotes/{remote}/{name}"),
                )?;
            }
        }
    } else {
        for row in refs
            .iter()
            .filter(|row| row.name.starts_with("refs/heads/"))
        {
            let branch = row
                .name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{}'", row.name),
                })?;
            destination.write_ref(&format!("refs/remotes/{remote}/{branch}"), &row.id)?;
        }
        if let Some(branch) = head_branch {
            destination.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
    }
    if !options.no_tags {
        for row in refs.iter().filter(|row| row.name.starts_with("refs/tags/")) {
            if !row.name.ends_with("^{}") {
                destination.write_ref(&row.name, &row.id)?;
            }
        }
    }
    Ok(())
}

fn clone_fetch_refspec(remote: &str, target: &CloneTarget, single_branch: bool) -> String {
    if single_branch {
        match target {
            CloneTarget::Branch { name, .. } => {
                return format!("+refs/heads/{name}:refs/remotes/{remote}/{name}");
            }
            CloneTarget::Tag { name, .. } => {
                return format!("+refs/tags/{name}:refs/tags/{name}");
            }
            CloneTarget::Empty
            | CloneTarget::Detached { .. }
            | CloneTarget::MissingBranch { .. } => {}
        }
    }
    format!("+refs/heads/*:refs/remotes/{remote}/*")
}

fn clone_target(
    refs: &RefStore,
    store: &LooseObjectStore,
    requested: Option<&str>,
    head_branch: Option<&str>,
    keep_partial_on_missing_branch: bool,
) -> Result<CloneTarget> {
    let Some(requested) = requested else {
        return match refs.resolve("HEAD") {
            Ok(id) => Ok(head_branch
                .map(|name| CloneTarget::Branch {
                    name: name.to_owned(),
                    id: id.clone(),
                })
                .unwrap_or(CloneTarget::Detached { id })),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(CloneTarget::Empty),
            Err(error) => Err(CliError::Io(error)),
        };
    };
    let branch_ref = branch_ref_name(requested)?;
    if let Ok(id) = refs.resolve(&branch_ref) {
        return Ok(CloneTarget::Branch {
            name: requested.to_owned(),
            id,
        });
    }
    let tag_ref = tag_ref_name(requested)?;
    if let Ok(id) = refs.resolve(&tag_ref) {
        let id = peel_tag(store, &id)?.unwrap_or(id);
        let commit_cache = CommitObjectCache::new(store);
        commit_cache.read_commit(&id)?;
        return Ok(CloneTarget::Tag {
            name: requested.to_owned(),
            id,
        });
    }
    if keep_partial_on_missing_branch {
        return Ok(CloneTarget::MissingBranch {
            name: requested.to_owned(),
        });
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("Remote branch {requested} not found in upstream origin"),
    })
}

pub(crate) fn run_clone(input: CloneCommandInput, raw_args: &[String]) -> Result<()> {
    validate_clone_jobs(input.jobs.as_deref(), raw_args)?;
    let (single_branch, no_single_branch) = clone_single_branch_flags(
        raw_args,
        input.single_branch,
        input.no_single_branch,
        input.depth.is_some(),
    );
    let no_tags = clone_no_tags(raw_args, input.no_tags, input.tags);
    let template = clone_template_path(raw_args, input.template, input.no_template);
    clone(CloneOptions {
        quiet: input.quiet,
        configs: input.configs,
        template,
        reject_shallow: clone_reject_shallow(
            raw_args,
            input.reject_shallow,
            input.no_reject_shallow,
        ),
        recurse_submodules: clone_recurse_submodule_specs(
            raw_args,
            input.recurse_submodules,
            input.recursive,
            input.no_recurse_submodules,
        ),
        remote_submodules: input.remote_submodules,
        shallow_submodules: input.shallow_submodules,
        bare: input.bare,
        mirror: input.mirror,
        no_checkout: clone_no_checkout(raw_args, input.no_checkout, input.checkout),
        remote_name: input.origin,
        no_tags,
        single_branch,
        no_single_branch,
        separate_git_dir: input.separate_git_dir,
        references: input.references,
        reference_if_able: input.reference_if_able,
        shared: input.shared,
        dissociate: input.dissociate,
        no_hardlinks: clone_no_hardlinks(raw_args, input.no_hardlinks, input.hardlinks),
        depth: input.depth,
        branch: input.branch,
        keep_partial_on_missing_branch: false,
        repository: input.repository,
        directory: input.directory,
    })
}

pub(crate) fn validate_positive_depth(depth: &str) -> Result<usize> {
    depth
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("depth {depth} is not a positive number"),
        })
}

fn ls_remote_rows(
    refs: &RefStore,
    store: &LooseObjectStore,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Vec<LsRemoteRow>> {
    let include_head = !heads && !tags && !refs_only;
    let mut rows =
        Vec::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT + usize::from(include_head));
    if include_head && let Ok(id) = refs.resolve("HEAD") {
        push_ls_remote_row(&mut rows, id, "HEAD", patterns);
    }
    if !tags || heads {
        refs.for_each_resolved_ref("refs/heads/", |ref_name, id| {
            push_ls_remote_row(&mut rows, id.clone(), ref_name, patterns);
            Ok::<(), CliError>(())
        })?;
    }
    if !heads || tags {
        refs.for_each_resolved_ref("refs/tags/", |ref_name, id| {
            let pushed = push_ls_remote_row(&mut rows, id.clone(), ref_name, patterns);
            if pushed
                && !refs_only
                && let Some(peeled) = peel_tag(store, id)?
            {
                rows.push(LsRemoteRow {
                    id: peeled,
                    name: format!("{ref_name}^{{}}"),
                });
            }
            Ok::<(), CliError>(())
        })?;
    }
    Ok(rows)
}

pub(crate) fn http_ls_remote_rows(
    url: &str,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Vec<LsRemoteRow>> {
    let url = parsed_http_url_with_extra_headers(None, url)?;
    if url.scheme == HttpScheme::Http {
        return http_ls_remote_rows_direct(&url, heads, tags, refs_only, patterns);
    }
    let mut helper = RemoteHttpHelperSession::spawn(&url)?;
    http_ls_remote_rows_with_helper(&url, &mut helper, heads, tags, refs_only, patterns)
}

fn http_ls_remote_rows_direct(
    url: &ParsedHttpUrl,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Vec<LsRemoteRow>> {
    if let Some(discovery) = http_smart_discovery_direct(url, heads, tags, refs_only, patterns)? {
        return Ok(discovery.rows);
    }
    let mut rows = Vec::with_capacity(
        HTTP_REMOTE_REF_ROWS_CAPACITY_HINT + usize::from(!heads && !tags && !refs_only),
    );
    if !heads
        && !tags
        && !refs_only
        && let Some(id) = http_resolve_ref_direct(url, "HEAD", 0)?
    {
        push_ls_remote_row(&mut rows, id, "HEAD", patterns);
    }
    let (head, body) = http_request_reader(url, "GET", "info/refs", &[])?;
    if head.status_code != 200 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP object request failed: {}", head.status_line),
        });
    }
    reserve_http_remote_ref_rows_capacity(&mut rows, head.content_length);
    parse_dumb_http_info_refs_rows_from_body(body, &mut rows, heads, tags, refs_only, patterns)?;
    Ok(rows)
}

fn http_smart_discovery_direct(
    url: &ParsedHttpUrl,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Option<SmartHttpDiscovery>> {
    let (head, body) = http_request_reader(url, "GET", "info/refs?service=git-upload-pack", &[])?;
    match head.status_code {
        200 => {
            let mut body = body;
            let capacity = http_remote_ref_rows_capacity_hint(head.content_length, 0);
            if let Some(discovery) = parse_smart_http_discovery_from_reader_with_capacity(
                &mut body, heads, tags, refs_only, patterns, capacity,
            )? {
                return Ok(Some(discovery));
            }
        }
        404 => {}
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("HTTP ref request failed: {}", head.status_line),
            });
        }
    }
    Ok(None)
}

pub(crate) fn http_ls_remote_rows_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Vec<LsRemoteRow>> {
    if let Some(discovery) =
        http_smart_discovery_with_helper(url, helper, heads, tags, refs_only, patterns)?
    {
        return Ok(discovery.rows);
    }
    let mut rows = Vec::with_capacity(
        HTTP_REMOTE_REF_ROWS_CAPACITY_HINT + usize::from(!heads && !tags && !refs_only),
    );
    if !heads
        && !tags
        && !refs_only
        && let Some(id) = http_resolve_ref_with_helper(url, helper, "HEAD", 0)?
    {
        push_ls_remote_row(&mut rows, id, "HEAD", patterns);
    }
    let response = helper.request_to_body(url, "GET", "info/refs", &[], &PackBody::Empty)?;
    if response.status_code != 200 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP object request failed: {}", response.status_line),
        });
    }
    reserve_http_remote_ref_rows_capacity(&mut rows, Some(response.body_len));
    response.body.with_reader(|reader| {
        parse_dumb_http_info_refs_rows_from_reader(
            reader, &mut rows, heads, tags, refs_only, patterns,
        )
    })?;
    Ok(rows)
}

fn http_smart_discovery_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Option<SmartHttpDiscovery>> {
    let response = helper.request_to_body(
        url,
        "GET",
        "info/refs?service=git-upload-pack",
        &[],
        &PackBody::Empty,
    )?;
    match response.status_code {
        200 => {
            let capacity = http_remote_ref_rows_capacity_hint(Some(response.body_len), 0);
            if let Some(discovery) = response.body.with_reader(|reader| {
                parse_smart_http_discovery_from_reader_with_capacity(
                    reader, heads, tags, refs_only, patterns, capacity,
                )
            })? {
                return Ok(Some(discovery));
            }
        }
        404 => {}
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("HTTP ref request failed: {}", response.status_line),
            });
        }
    }
    Ok(None)
}

fn parse_smart_http_ls_remote_rows_from_reader<R: Read + ?Sized>(
    reader: &mut R,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Option<Vec<LsRemoteRow>>> {
    parse_smart_http_discovery_from_reader(reader, heads, tags, refs_only, patterns)
        .map(|value| value.map(|discovery| discovery.rows))
}

fn parse_smart_http_discovery_from_reader<R: Read + ?Sized>(
    reader: &mut R,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Option<SmartHttpDiscovery>> {
    parse_smart_http_discovery_from_reader_with_capacity(
        reader,
        heads,
        tags,
        refs_only,
        patterns,
        HTTP_REMOTE_REF_ROWS_CAPACITY_HINT,
    )
}

fn parse_smart_http_discovery_from_reader_with_capacity<R: Read + ?Sized>(
    reader: &mut R,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
    rows_capacity: usize,
) -> Result<Option<SmartHttpDiscovery>> {
    if !read_smart_http_service_header(
        reader,
        &[*b"001e", *b"001d"],
        b"# service=git-upload-pack\n",
        b"# service=git-upload-pack",
    )? {
        return Ok(None);
    }
    let mut rows = Vec::with_capacity(rows_capacity);
    let mut head_branch = None;
    let mut first = true;
    let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    while read_pkt_line_payload_into(reader, &mut line)? {
        let payload = if let Some(nul_pos) = line.iter().position(|byte| *byte == 0) {
            if first {
                let capabilities = &line[nul_pos + 1..];
                head_branch = head_symref_branch_from_capabilities(capabilities);
            }
            &line[..nul_pos]
        } else {
            &line[..]
        };
        first = false;
        let Some((id, name)) = split_ls_remote_space_payload(payload) else {
            continue;
        };
        if !ls_remote_ref_name_selected(name, heads, tags, refs_only) {
            continue;
        }
        let id = ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id)?;
        push_ls_remote_row_bytes(&mut rows, id, name, patterns);
    }
    Ok(Some(SmartHttpDiscovery { rows, head_branch }))
}

fn reserve_http_remote_ref_rows_capacity(
    rows: &mut Vec<LsRemoteRow>,
    content_length: Option<usize>,
) {
    let desired = http_remote_ref_rows_capacity_hint(content_length, 0);
    if rows.capacity() < desired {
        rows.reserve_exact(desired - rows.capacity());
    }
}

fn http_remote_ref_rows_capacity_hint(content_length: Option<usize>, extra_rows: usize) -> usize {
    let base = HTTP_REMOTE_REF_ROWS_CAPACITY_HINT.saturating_add(extra_rows);
    let Some(content_length) = content_length else {
        return base;
    };
    let estimated_rows = content_length
        .checked_div(HTTP_REMOTE_REF_ROW_BYTES_HINT)
        .unwrap_or(0)
        .saturating_add(extra_rows);
    base.max(transport_ref_collection_capacity(estimated_rows))
}

fn read_smart_http_service_header<R: Read + ?Sized>(
    reader: &mut R,
    allowed_lengths: &[[u8; 4]],
    service_with_lf: &[u8],
    service_without_lf: &[u8],
) -> Result<bool> {
    let mut header = [0_u8; 4];
    match reader.read(&mut header[..1]).map_err(CliError::Io)? {
        0 => return Ok(false),
        1 => {}
        _ => unreachable!("single-byte read returned more than one byte"),
    }
    reader.read_exact(&mut header[1..]).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            CliError::Fatal {
                code: 128,
                message: "HTTP response ended early".into(),
            }
        } else {
            CliError::Io(error)
        }
    })?;
    if !header.iter().all(u8::is_ascii_hexdigit) {
        return Err(CliError::Fatal {
            code: 128,
            message: "HTTP response ended early".into(),
        });
    }
    if !allowed_lengths.contains(&header) {
        return Ok(false);
    }
    let len = parse_pkt_line_len(&header, "invalid smart HTTP service pkt-line header")?;
    let payload_len = len.checked_sub(4).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "invalid smart HTTP service pkt-line length".into(),
    })?;
    let mut payload = [0_u8; 64];
    let payload = payload
        .get_mut(..payload_len)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "invalid smart HTTP service pkt-line length".into(),
        })?;
    reader.read_exact(payload).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            CliError::Fatal {
                code: 128,
                message: "HTTP response ended early".into(),
            }
        } else {
            CliError::Io(error)
        }
    })?;
    if payload != service_with_lf && payload != service_without_lf {
        return Ok(false);
    }
    let mut flush = [0_u8; 4];
    reader.read_exact(&mut flush).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            CliError::Fatal {
                code: 128,
                message: "HTTP response ended early".into(),
            }
        } else {
            CliError::Io(error)
        }
    })?;
    Ok(flush == *b"0000")
}

fn parse_dumb_http_info_refs_rows_from_reader<R: BufRead + ?Sized>(
    reader: &mut R,
    rows: &mut Vec<LsRemoteRow>,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<()> {
    let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).map_err(CliError::Io)? == 0 {
            break;
        }
        let Some((id, name)) = split_ls_remote_tab_payload(&line) else {
            continue;
        };
        if !ls_remote_ref_name_selected(name, heads, tags, refs_only) {
            continue;
        }
        let id = ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id)?;
        push_ls_remote_row_bytes(rows, id, name, patterns);
    }
    Ok(())
}

fn parse_dumb_http_info_refs_rows_from_body(
    mut body: HttpBodyReader,
    rows: &mut Vec<LsRemoteRow>,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<()> {
    if matches!(body, HttpBodyReader::Chunked(_)) {
        let mut reader = io::BufReader::with_capacity(HTTP_DIRECT_READ_BUF_CAPACITY, body);
        return parse_dumb_http_info_refs_rows_from_reader(
            &mut reader,
            rows,
            heads,
            tags,
            refs_only,
            patterns,
        );
    }
    match &mut body {
        HttpBodyReader::ContentLength(reader) => parse_dumb_http_info_refs_rows_from_reader(
            reader, rows, heads, tags, refs_only, patterns,
        ),
        HttpBodyReader::ConnectionClose(reader) => parse_dumb_http_info_refs_rows_from_reader(
            reader, rows, heads, tags, refs_only, patterns,
        ),
        HttpBodyReader::File { reader, .. } => parse_dumb_http_info_refs_rows_from_reader(
            reader, rows, heads, tags, refs_only, patterns,
        ),
        HttpBodyReader::Chunked(_) => unreachable!("chunked body returned above"),
    }
}

fn split_ls_remote_space_payload(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let line = trim_lf_payload(line);
    split_once_byte(line, b' ').map(|(id, name)| (trim_ascii_whitespace(id), name))
}

fn split_ls_remote_tab_payload(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let line = trim_lf_payload(line);
    split_once_byte(line, b'\t')
        .map(|(id, name)| (trim_ascii_whitespace(id), trim_ascii_whitespace(name)))
}

fn ls_remote_ref_name_selected(name: &[u8], heads: bool, tags: bool, refs_only: bool) -> bool {
    if name.is_empty() || (refs_only && (name == b"HEAD" || name.ends_with(b"^{}"))) {
        return false;
    }
    if heads || tags {
        return (heads && name.starts_with(b"refs/heads/"))
            || (tags && name.starts_with(b"refs/tags/"));
    }
    !refs_only || name.starts_with(b"refs/")
}

fn push_ls_remote_row_bytes(
    rows: &mut Vec<LsRemoteRow>,
    id: ObjectId,
    name: &[u8],
    patterns: &[String],
) -> bool {
    let name = String::from_utf8_lossy(name);
    if !patterns.is_empty()
        && !patterns
            .iter()
            .any(|pattern| ls_remote_pattern_matches(&name, pattern))
    {
        return false;
    }
    rows.push(LsRemoteRow {
        id,
        name: name.into_owned(),
    });
    true
}

fn http_resolve_ref_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    name: &str,
    depth: usize,
) -> Result<Option<ObjectId>> {
    if depth > 8 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP symbolic ref nesting is too deep: {name}"),
        });
    }
    let Some(body) = http_get_optional_with_helper(url, helper, name)? else {
        return Ok(None);
    };
    let value = String::from_utf8_lossy(&body);
    let value = value.trim();
    if let Some(target) = value.strip_prefix("ref: ") {
        return http_resolve_ref_with_helper(url, helper, target.trim(), depth + 1);
    }
    Ok(Some(ObjectId::from_hex(GitHashAlgorithm::Sha1, value)?))
}

fn http_get_optional_direct(url: &ParsedHttpUrl, suffix: &str) -> Result<Option<Vec<u8>>> {
    let (head, mut body) = http_request_reader(url, "GET", suffix, &[])?;
    match head.status_code {
        200 => {
            let mut value = Vec::with_capacity(
                head.content_length
                    .unwrap_or(0)
                    .min(PKT_LINE_PAYLOAD_INITIAL_CAPACITY_LIMIT),
            );
            read_http_response_body_to_vec(&mut body, &mut value)?;
            Ok(Some(value))
        }
        404 => Ok(None),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP ref request failed: {}", head.status_line),
        }),
    }
}

fn read_http_response_body_to_vec<R: Read>(reader: &mut R, out: &mut Vec<u8>) -> Result<()> {
    reader
        .read_to_end(out)
        .map(|_| ())
        .map_err(map_http_response_body_io)
}

fn drain_http_response_body<R: Read + ?Sized>(reader: &mut R) -> Result<()> {
    let mut buffer = [0_u8; HTTP_RESPONSE_DRAIN_BUF_CAPACITY];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(map_http_response_body_io)?;
        if read == 0 {
            return Ok(());
        }
    }
}

fn map_http_response_body_io(error: io::Error) -> CliError {
    if error.kind() == io::ErrorKind::UnexpectedEof
        && error.to_string().contains("HTTP response ended early")
    {
        CliError::Fatal {
            code: 128,
            message: "HTTP response ended early".into(),
        }
    } else {
        CliError::Io(error)
    }
}

fn http_resolve_ref_direct(
    url: &ParsedHttpUrl,
    name: &str,
    depth: usize,
) -> Result<Option<ObjectId>> {
    if depth > 8 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP symbolic ref nesting is too deep: {name}"),
        });
    }
    let Some(body) = http_get_optional_direct(url, name)? else {
        return Ok(None);
    };
    let value = String::from_utf8_lossy(&body);
    let value = value.trim();
    if let Some(target) = value.strip_prefix("ref: ") {
        return http_resolve_ref_direct(url, target.trim(), depth + 1);
    }
    Ok(Some(ObjectId::from_hex(GitHashAlgorithm::Sha1, value)?))
}

fn push_ls_remote_row(
    rows: &mut Vec<LsRemoteRow>,
    id: ObjectId,
    name: &str,
    patterns: &[String],
) -> bool {
    if !patterns.is_empty()
        && !patterns
            .iter()
            .any(|pattern| ls_remote_pattern_matches(name, pattern))
    {
        return false;
    }
    rows.push(LsRemoteRow {
        id,
        name: name.to_owned(),
    });
    true
}

fn ls_remote_pattern_matches(name: &str, pattern: &str) -> bool {
    wildcard_match(pattern, name)
        || name
            .rsplit('/')
            .any(|component| wildcard_match(pattern, component))
}

fn peel_tag(store: &LooseObjectStore, id: &ObjectId) -> Result<Option<ObjectId>> {
    let mut current = id.clone();
    let mut peeled_any = false;
    for _ in 0..8 {
        let object = store.read_object(&current)?;
        if object.kind != GitObjectKind::Tag {
            return Ok(peeled_any.then_some(current));
        }
        peeled_any = true;
        current = decode_tag(GitHashAlgorithm::Sha1, &object.content)?.target;
    }
    Err(CliError::Fatal {
        code: 128,
        message: "tag nesting is too deep".into(),
    })
}

pub(crate) fn run_ls_remote(
    heads: bool,
    tags: bool,
    refs_only: bool,
    repository: Option<String>,
    patterns: Vec<String>,
) -> Result<()> {
    let repo = find_repo().ok();
    let repository = match repository {
        Some(repository) => repository,
        None => "origin".to_owned(),
    };
    let url = match repo.as_ref() {
        Some(repo) if remote_exists(repo, &repository)? => remote_url(repo, &repository)?,
        _ => repository.clone(),
    };
    if is_http_transport_url(&url) {
        let parsed_url = parsed_http_url_with_extra_headers(repo.as_ref(), &url)?;
        let rows = if parsed_url.scheme == HttpScheme::Http {
            http_ls_remote_rows_direct(&parsed_url, heads, tags, refs_only, &patterns)?
        } else {
            let mut helper = RemoteHttpHelperSession::spawn(&parsed_url)?;
            let (rows, _) = discover_http_refs(
                &parsed_url,
                HttpDiscoveryTransport::Helper(&mut helper),
                heads,
                tags,
                refs_only,
                &patterns,
            )?;
            rows
        };
        for row in rows {
            println!("{}\t{}", row.id.to_hex(), row.name);
        }
        return Ok(());
    }
    if is_git_daemon_transport_url(&url) {
        let rows = daemon_ls_remote_rows(&url, heads, tags, refs_only, &patterns)?;
        for row in rows {
            println!("{}\t{}", row.id.to_hex(), row.name);
        }
        return Ok(());
    }
    if is_ssh_transport_url(&url) {
        let rows = ssh_ls_remote_rows(&url, heads, tags, refs_only, &patterns)?;
        for row in rows {
            println!("{}\t{}", row.id.to_hex(), row.name);
        }
        return Ok(());
    }
    let Some(source_path) = local_repository_path_from_location(&url)? else {
        return Err(unsupported_remote_helper_error(&url, String::new()));
    };
    let remote = source_path.to_string_lossy().to_string();
    let discovery = CliTransportAdapter
        .discover_refs(&remote)
        .map_err(|error| CliError::Fatal {
            code: 128,
            message: format!("transport discovery failed: {error}"),
        })?;
    let mut rows = Vec::with_capacity(transport_ref_collection_capacity(discovery.refs.len()));
    for (name, id) in discovery.refs {
        let id = ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id.as_bytes())?;
        if ls_remote_ref_name_selected(name.as_bytes(), heads, tags, refs_only) {
            push_ls_remote_row(&mut rows, id, &name, &patterns);
        }
    }
    for row in rows {
        println!("{}\t{}", row.id.to_hex(), row.name);
    }
    Ok(())
}

pub(crate) fn run_fetch(
    depth: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
) -> Result<()> {
    fetch_with_depth(remote, branch, depth.as_deref(), 128)
}

pub(crate) fn run_pull(
    ff_only: bool,
    rebase_mode: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let refs = refs_adapter_from_git_dir(&repo.git_dir);
    let current_branch = current_branch_ref(&refs)?.ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "cannot pull into detached HEAD".into(),
    })?;
    let current_branch_short = branch_display_name(&current_branch);
    let pull_rebase_mode =
        pull_rebase_after_fetch(&repo, &current_branch_short, rebase_mode.as_deref())?;
    if rebase_mode.is_some() && pull_rebase_mode.rebases() && ff_only {
        return Err(CliError::Fatal {
            code: 128,
            message: "options '--rebase' and '--ff-only' cannot be used together".into(),
        });
    }
    let remote = match remote {
        Some(remote) => remote,
        None => read_config_section_value(&repo, "branch", &current_branch_short, "remote")?
            .unwrap_or_else(|| "origin".to_owned()),
    };
    let branch = match branch {
        Some(branch) => branch,
        None => read_config_section_value(&repo, "branch", &current_branch_short, "merge")?
            .map(|merge| short_ref_name(&merge))
            .unwrap_or_else(|| current_branch_short.clone()),
    };
    validate_remote_name(&remote)?;
    let _ = branch_ref_name(&branch)?;

    fetch_with_missing_ref_code(Some(remote.clone()), Some(branch.clone()), 1)?;
    let target = format!("refs/remotes/{remote}/{branch}");
    if pull_rebase_mode.rebases() && !ff_only {
        return sequencer_commands::rebase(
            false,
            false,
            None,
            vec![target],
            pull_rebase_mode == PullRebaseMode::RebaseMerges,
            pull_rebase_mode == PullRebaseMode::Interactive,
        );
    }
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    fast_forward_to(&repo, &store, &target, "pull", ff_only)
}

pub(crate) fn run_push(
    force: bool,
    set_upstream: bool,
    remote: Option<String>,
    refspecs: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let remote = remote.unwrap_or_else(|| "origin".to_owned());
    validate_remote_name(&remote)?;
    if !remote_exists(&repo, &remote)? {
        return Err(remote_repository_unavailable_error(&remote));
    }
    let url = remote_url(&repo, &remote)?;
    if is_ssh_transport_url(&url) {
        return push_with_ssh_remote(repo, remote, force, set_upstream, refspecs, &url);
    }
    if is_http_transport_url(&url) {
        return push_with_http_remote(repo, remote, force, set_upstream, refspecs, &url);
    }
    if is_git_daemon_transport_url(&url) {
        return push_with_daemon_remote(repo, remote, force, set_upstream, refspecs, &url);
    }
    let Some(remote_path) = local_repository_path_from_location(&url)? else {
        return Err(unsupported_remote_helper_error(&url, String::new()));
    };

    let destination = local_clone_source(&remote_path)?;
    let source_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let destination_refs = refs_adapter_from_git_dir(&destination.git_dir);
    let destination_store = object_adapter_from_objects_dir(destination.git_dir.join("objects"));
    let destination_commit_cache = CommitObjectCache::new(&destination_store);
    let specs = if refspecs.is_empty() {
        vec![default_push_refspec(&source_refs)?]
    } else {
        refspecs
    };

    for spec in specs {
        let push_ref = parse_push_refspec(&repo, &source_refs, &spec, &url)?;
        if push_ref.id.is_some() {
            copy_dir_contents(&repo.objects_dir, &destination.git_dir.join("objects"))?;
            validate_push_update(
                &destination_refs,
                &destination_commit_cache,
                &push_ref,
                force || push_ref.force,
            )?;
        }
        if let Some(id) = &push_ref.id {
            destination_refs.write_ref(&push_ref.destination, id)?;
        } else {
            validate_push_delete(&destination_refs, &push_ref.destination)?;
            destination_refs.delete_ref(&push_ref.destination)?;
        }
        if set_upstream && push_ref.id.is_some() {
            set_push_upstream(&repo, &push_ref, &remote)?;
        }
        let source_display = push_ref
            .source_display
            .clone()
            .or_else(|| push_ref.id.as_ref().map(ObjectId::to_hex))
            .unwrap_or_else(|| "(delete)".to_owned());
        println!(
            "{} -> {}",
            source_display,
            push_ref
                .destination
                .strip_prefix("refs/heads/")
                .unwrap_or(&push_ref.destination)
        );
    }
    Ok(())
}

fn push_with_daemon_remote(
    repo: GitRepo,
    remote: String,
    force: bool,
    set_upstream: bool,
    refspecs: Vec<String>,
    url: &str,
) -> Result<()> {
    let source_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let specs = if refspecs.is_empty() {
        vec![default_push_refspec(&source_refs)?]
    } else {
        refspecs
    };
    let advertisement = daemon_receive_pack_advertisement(url)?;
    let source_store = object_adapter_from_objects_dir(&repo.objects_dir);
    let source_commit_cache = CommitObjectCache::new(&source_store);

    let initial_capacity = transport_ref_collection_capacity(specs.len());
    let mut push_refs = Vec::with_capacity(initial_capacity);
    let mut object_ids = Vec::with_capacity(initial_capacity);
    let mut seen_objects = HashSet::with_capacity(initial_capacity);
    for spec in specs {
        let push_ref = parse_push_refspec(&repo, &source_refs, &spec, url)?;
        if let Some(id) = &push_ref.id {
            if let Some(current) = advertisement.refs.get(&push_ref.destination)
                && !force
                && !push_ref.force
                && !is_ancestor_commit_cached(&source_commit_cache, current, id)?
            {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!(
                        "failed to push some refs to '{}': non-fast-forward",
                        push_ref.destination
                    ),
                });
            }
            collect_push_pack_ids(
                &repo,
                &source_store,
                &source_commit_cache,
                id,
                &mut object_ids,
                &mut seen_objects,
            )?;
        } else if !advertisement.refs.contains_key(&push_ref.destination) {
            return Err(CliError::Stderr {
                code: 1,
                text: format!(
                    "error: unable to delete '{}': remote ref does not exist\n\
                     error: failed to push some refs\n",
                    push_ref
                        .destination
                        .strip_prefix("refs/heads/")
                        .unwrap_or(&push_ref.destination)
                ),
            });
        }
        let old = advertisement.refs.get(&push_ref.destination).cloned();
        push_refs.push((push_ref, old));
    }

    daemon_send_receive_pack(url, &push_refs, &source_store, &object_ids)?;

    for (push_ref, _) in &push_refs {
        if set_upstream && push_ref.id.is_some() {
            set_push_upstream(&repo, push_ref, &remote)?;
        }
        let source_display = push_ref
            .source_display
            .clone()
            .or_else(|| push_ref.id.as_ref().map(ObjectId::to_hex))
            .unwrap_or_else(|| "(delete)".to_owned());
        println!(
            "{} -> {}",
            source_display,
            push_ref
                .destination
                .strip_prefix("refs/heads/")
                .unwrap_or(&push_ref.destination)
        );
    }
    Ok(())
}

fn push_with_http_remote(
    repo: GitRepo,
    remote: String,
    force: bool,
    set_upstream: bool,
    refspecs: Vec<String>,
    url: &str,
) -> Result<()> {
    let parsed_url = parsed_http_url_with_extra_headers(Some(&repo), url)?;
    push_with_https_helper_remote(
        repo,
        remote,
        force,
        set_upstream,
        refspecs,
        url,
        &parsed_url,
    )
}

fn push_with_https_helper_remote(
    repo: GitRepo,
    remote: String,
    force: bool,
    set_upstream: bool,
    refspecs: Vec<String>,
    url_text: &str,
    url: &ParsedHttpUrl,
) -> Result<()> {
    let mut helper = RemoteHttpHelperSession::spawn(&url)?;
    let source_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let specs = if refspecs.is_empty() {
        vec![default_push_refspec(&source_refs)?]
    } else {
        refspecs
    };
    let advertisement = http_receive_pack_advertisement_with_helper(url, &mut helper)?;
    let source_store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let source_commit_cache = CommitObjectCache::new(&source_store);

    let initial_capacity = transport_ref_collection_capacity(specs.len());
    let mut push_refs = Vec::with_capacity(initial_capacity);
    let mut object_ids = Vec::with_capacity(initial_capacity);
    let mut seen_objects = HashSet::with_capacity(initial_capacity);
    for spec in specs {
        let push_ref = parse_push_refspec(&repo, &source_refs, &spec, url_text)?;
        if let Some(id) = &push_ref.id {
            if let Some(current) = advertisement.refs.get(&push_ref.destination)
                && !force
                && !push_ref.force
                && !is_ancestor_commit_cached(&source_commit_cache, current, id)?
            {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!(
                        "failed to push some refs to '{}': non-fast-forward",
                        push_ref.destination
                    ),
                });
            }
            collect_push_pack_ids(
                &repo,
                &source_store,
                &source_commit_cache,
                id,
                &mut object_ids,
                &mut seen_objects,
            )?;
        } else if !advertisement.refs.contains_key(&push_ref.destination) {
            return Err(CliError::Stderr {
                code: 1,
                text: format!(
                    "error: unable to delete '{}': remote ref does not exist\n\
                     error: failed to push some refs\n",
                    push_ref
                        .destination
                        .strip_prefix("refs/heads/")
                        .unwrap_or(&push_ref.destination)
                ),
            });
        }
        let old = advertisement.refs.get(&push_ref.destination).cloned();
        push_refs.push((push_ref, old));
    }

    let pack = write_push_pack_to_temp_file(&repo, &source_store, &object_ids)?;
    http_send_receive_pack_with_helper_session(url, &mut helper, &push_refs, &pack)?;

    for (push_ref, _) in &push_refs {
        if set_upstream && push_ref.id.is_some() {
            set_push_upstream(&repo, push_ref, &remote)?;
        }
        let source_display = push_ref
            .source_display
            .clone()
            .or_else(|| push_ref.id.as_ref().map(ObjectId::to_hex))
            .unwrap_or_else(|| "(delete)".to_owned());
        println!(
            "{} -> {}",
            source_display,
            push_ref
                .destination
                .strip_prefix("refs/heads/")
                .unwrap_or(&push_ref.destination)
        );
    }
    Ok(())
}

fn push_with_ssh_remote(
    repo: GitRepo,
    remote: String,
    force: bool,
    set_upstream: bool,
    refspecs: Vec<String>,
    url: &str,
) -> Result<()> {
    let source_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let specs = if refspecs.is_empty() {
        vec![default_push_refspec(&source_refs)?]
    } else {
        refspecs
    };
    let advertisement = ssh_receive_pack_advertisement(url)?;
    let source_store = object_adapter_from_objects_dir(&repo.objects_dir);
    let source_commit_cache = CommitObjectCache::new(&source_store);

    let initial_capacity = specs.len();
    let mut push_refs = Vec::with_capacity(initial_capacity);
    let mut object_ids = Vec::with_capacity(initial_capacity);
    let mut seen_objects = HashSet::with_capacity(initial_capacity);
    for spec in specs {
        let push_ref = parse_push_refspec(&repo, &source_refs, &spec, url)?;
        if let Some(id) = &push_ref.id {
            if let Some(current) = advertisement.refs.get(&push_ref.destination)
                && !force
                && !push_ref.force
                && !is_ancestor_commit_cached(&source_commit_cache, current, id)?
            {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!(
                        "failed to push some refs to '{}': non-fast-forward",
                        push_ref.destination
                    ),
                });
            }
            collect_push_pack_ids(
                &repo,
                &source_store,
                &source_commit_cache,
                id,
                &mut object_ids,
                &mut seen_objects,
            )?;
        } else if !advertisement.refs.contains_key(&push_ref.destination) {
            return Err(CliError::Stderr {
                code: 1,
                text: format!(
                    "error: unable to delete '{}': remote ref does not exist\n\
                     error: failed to push some refs\n",
                    push_ref
                        .destination
                        .strip_prefix("refs/heads/")
                        .unwrap_or(&push_ref.destination)
                ),
            });
        }
        let old = advertisement.refs.get(&push_ref.destination).cloned();
        push_refs.push((push_ref, old));
    }

    ssh_send_receive_pack(url, &push_refs, &source_store, &object_ids)?;

    for (push_ref, _) in &push_refs {
        if set_upstream && push_ref.id.is_some() {
            set_push_upstream(&repo, push_ref, &remote)?;
        }
        let source_display = push_ref
            .source_display
            .clone()
            .or_else(|| push_ref.id.as_ref().map(ObjectId::to_hex))
            .unwrap_or_else(|| "(delete)".to_owned());
        println!(
            "{} -> {}",
            source_display,
            push_ref
                .destination
                .strip_prefix("refs/heads/")
                .unwrap_or(&push_ref.destination)
        );
    }
    Ok(())
}

fn collect_push_pack_ids(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
    out: &mut Vec<ObjectId>,
    seen: &mut HashSet<ObjectId>,
) -> Result<()> {
    let mut current = id.clone();
    loop {
        if !seen.insert(current.clone()) {
            return Ok(());
        }
        out.push(current.clone());
        let kind = object_kind_hint_or_read(store, &current)?;
        if kind == GitObjectKind::Tag {
            let object = store.read_object(&current)?;
            let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
            current = tag.target;
            continue;
        }
        if kind != GitObjectKind::Commit {
            return Ok(());
        }
        break;
    }
    let commits =
        collect_commits_from_ids_cached(repo, commit_cache, std::slice::from_ref(&current), None)?;
    reserve_transport_history_vec(out, commits.len());
    reserve_transport_history_set(seen, commits.len());
    for commit in &commits {
        if seen.insert(commit.clone()) {
            out.push(commit.clone());
        }
    }
    collect_rev_list_object_ids_into_cached(store, commit_cache, &commits, &[], &[], seen, out)?;
    Ok(())
}

fn fetch_with_depth(
    remote: Option<String>,
    branch: Option<String>,
    depth: Option<&str>,
    missing_ref_code: i32,
) -> Result<()> {
    let repo = find_repo()?;
    let remote = remote.unwrap_or_else(|| "origin".to_owned());
    validate_remote_name(&remote)?;
    if !remote_exists(&repo, &remote)? {
        return Err(remote_repository_unavailable_error(&remote));
    }
    if let Some(depth) = depth.map(validate_positive_depth).transpose()? {
        return fetch_with_repo_and_remote_depth(repo, remote, branch, missing_ref_code, depth);
    }
    fetch_with_repo_and_remote(repo, remote, branch, missing_ref_code)
}

fn fetch_with_missing_ref_code(
    remote: Option<String>,
    branch: Option<String>,
    missing_ref_code: i32,
) -> Result<()> {
    let repo = find_repo()?;
    let remote = remote.unwrap_or_else(|| "origin".to_owned());
    validate_remote_name(&remote)?;
    if !remote_exists(&repo, &remote)? {
        return Err(remote_repository_unavailable_error(&remote));
    }
    fetch_with_repo_and_remote(repo, remote, branch, missing_ref_code)
}

pub(crate) fn fetch_with_repo_and_remote(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
) -> Result<()> {
    let url = remote_url(&repo, &remote)?;
    if is_http_transport_url(&url) {
        return fetch_with_http_remote(repo, remote, branch, missing_ref_code, &url);
    }
    if is_git_daemon_transport_url(&url) {
        return fetch_with_daemon_remote(repo, remote, branch, missing_ref_code, &url);
    }
    if is_ssh_transport_url(&url) {
        return fetch_with_ssh_remote(repo, remote, branch, missing_ref_code, &url);
    }
    let Some(source_path) = local_repository_path_from_location(&url)? else {
        return Err(unsupported_remote_helper_error(&url, String::new()));
    };

    let source = local_clone_source(&source_path)?;
    let source_refs = refs_adapter_from_git_dir(&source.git_dir);
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    copy_dir_contents(&source.git_dir.join("objects"), &repo.objects_dir)?;
    if let Some(ref branch) = branch {
        let ref_name = branch_ref_name(&branch)?;
        let id = source_refs
            .resolve(&ref_name)
            .map_err(|_| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        return Ok(());
    }
    copy_remote_refs(
        &source_refs,
        &destination_refs,
        &remote,
        source_head_branch(&source_refs)?.as_deref(),
        true,
    )
}

fn fetch_with_http_remote(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
    url: &str,
) -> Result<()> {
    let parsed_url = parsed_http_url_with_extra_headers(Some(&repo), url)?;
    let mut helper = if parsed_url.scheme == HttpScheme::Https {
        Some(RemoteHttpHelperSession::spawn(&parsed_url)?)
    } else {
        None
    };
    let (rows, head_branch) = discover_http_refs_with_helper(
        &parsed_url,
        helper.as_mut().map(std::convert::identity),
        false,
        false,
        false,
        &[],
    )?;
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let fetch_options = HttpFetchOptions {
        commit: false,
        tags: false,
        all: true,
        verbose: false,
        recover: false,
        write_ref: Vec::new(),
        stdin: false,
        packfile: None,
        index_pack_args: Vec::new(),
        args: Vec::new(),
    };
    let mut roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    if let Some(branch) = branch {
        let ref_name = branch_ref_name(&branch)?;
        let id = rows
            .iter()
            .find(|row| row.name == ref_name)
            .map(|row| row.id.clone())
            .ok_or_else(|| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        roots.push(id);
    } else {
        for row in rows
            .iter()
            .filter(|row| row.name.starts_with("refs/heads/"))
        {
            let branch = row
                .name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{}'", row.name),
                })?;
            destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &row.id)?;
            roots.push(row.id.clone());
        }
        if let Some(branch) = head_branch {
            destination_refs.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
        for row in rows.iter().filter(|row| row.name.starts_with("refs/tags/")) {
            if !row.name.ends_with("^{}") {
                destination_refs.write_ref(&row.name, &row.id)?;
                roots.push(row.id.clone());
            }
        }
    }
    sort_dedup_object_ids(&mut roots);
    let pack_fetched = if let Some(helper) = helper.as_mut() {
        http_fetch_smart_pack_with_helper(&parsed_url, helper, &repo.objects_dir, &roots)?
    } else {
        http_fetch_smart_pack_direct(&parsed_url, &repo.objects_dir, &roots)?
    };
    if !pack_fetched {
        let helper = helper.get_or_insert(RemoteHttpHelperSession::spawn(&parsed_url)?);
        let commit_cache = CommitObjectCache::new(&store);
        let tree_cache = TreeObjectCache::new(&store);
        let mut seen = HashSet::with_capacity(transport_ref_collection_capacity(roots.len()));
        let mut fetch_context = HttpFetchObjectContext {
            url: &parsed_url,
            helper,
            store: &store,
            commit_cache: &commit_cache,
            tree_cache: &tree_cache,
            options: &fetch_options,
            seen: &mut seen,
            suffix_buffer: String::new(),
        };
        for id in roots {
            http_fetch_object_recursive(&mut fetch_context, &id)?;
        }
    }
    Ok(())
}

fn fetch_with_http_remote_depth(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
    url: &str,
    depth: usize,
) -> Result<()> {
    let parsed_url = parsed_http_url_with_extra_headers(Some(&repo), url)?;
    let mut helper = if parsed_url.scheme == HttpScheme::Https {
        Some(RemoteHttpHelperSession::spawn(&parsed_url)?)
    } else {
        None
    };
    let (rows, head_branch) = discover_http_refs_with_helper(
        &parsed_url,
        helper.as_mut().map(std::convert::identity),
        false,
        false,
        false,
        &[],
    )?;
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let mut request_roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    let mut shallow_roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    if let Some(branch) = branch {
        let ref_name = branch_ref_name(&branch)?;
        let id = rows
            .iter()
            .find(|row| row.name == ref_name)
            .map(|row| row.id.clone())
            .ok_or_else(|| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        request_roots.push(id.clone());
        shallow_roots.push(id);
    } else {
        for row in rows
            .iter()
            .filter(|row| row.name.starts_with("refs/heads/"))
        {
            let branch = row
                .name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{}'", row.name),
                })?;
            destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &row.id)?;
            request_roots.push(row.id.clone());
            shallow_roots.push(row.id.clone());
        }
        if let Some(branch) = head_branch {
            destination_refs.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
        for row in rows.iter().filter(|row| row.name.starts_with("refs/tags/")) {
            if !row.name.ends_with("^{}") {
                destination_refs.write_ref(&row.name, &row.id)?;
                request_roots.push(row.id.clone());
            }
        }
    }
    sort_dedup_object_ids(&mut request_roots);
    sort_dedup_object_ids(&mut shallow_roots);

    let shallow_boundaries = if let Some(helper) = helper.as_mut() {
        http_fetch_smart_pack_with_depth_with_helper(
            &parsed_url,
            helper,
            &repo.objects_dir,
            &request_roots,
            Some(depth),
        )?
    } else {
        http_fetch_smart_pack_with_depth_direct(
            &parsed_url,
            &repo.objects_dir,
            &request_roots,
            Some(depth),
        )?
    };
    write_shallow_file(&repo, shallow_boundaries)
}

fn fetch_with_daemon_remote(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
    url: &str,
) -> Result<()> {
    let rows = daemon_ls_remote_rows(url, false, false, false, &[])?;
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let mut roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    if let Some(branch) = branch {
        let ref_name = branch_ref_name(&branch)?;
        let id = rows
            .iter()
            .find(|row| row.name == ref_name)
            .map(|row| row.id.clone())
            .ok_or_else(|| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        roots.push(id);
    } else {
        for row in rows
            .iter()
            .filter(|row| row.name.starts_with("refs/heads/"))
        {
            let branch = row
                .name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{}'", row.name),
                })?;
            destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &row.id)?;
            roots.push(row.id.clone());
        }
        if let Some(branch) = daemon_head_branch(url)? {
            destination_refs.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
        for row in rows.iter().filter(|row| row.name.starts_with("refs/tags/")) {
            if !row.name.ends_with("^{}") {
                destination_refs.write_ref(&row.name, &row.id)?;
                roots.push(row.id.clone());
            }
        }
    }
    sort_dedup_object_ids(&mut roots);
    daemon_fetch_pack(url, &repo.objects_dir, &roots)
}

fn fetch_with_daemon_remote_depth(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
    url: &str,
    depth: usize,
) -> Result<()> {
    let rows = daemon_ls_remote_rows(url, false, false, false, &[])?;
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let mut request_roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    let mut shallow_roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    if let Some(branch) = branch {
        let ref_name = branch_ref_name(&branch)?;
        let id = rows
            .iter()
            .find(|row| row.name == ref_name)
            .map(|row| row.id.clone())
            .ok_or_else(|| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        request_roots.push(id.clone());
        shallow_roots.push(id);
    } else {
        for row in rows
            .iter()
            .filter(|row| row.name.starts_with("refs/heads/"))
        {
            let branch = row
                .name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{}'", row.name),
                })?;
            destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &row.id)?;
            request_roots.push(row.id.clone());
            shallow_roots.push(row.id.clone());
        }
        if let Some(branch) = daemon_head_branch(url)? {
            destination_refs.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
        for row in rows.iter().filter(|row| row.name.starts_with("refs/tags/")) {
            if !row.name.ends_with("^{}") {
                destination_refs.write_ref(&row.name, &row.id)?;
                request_roots.push(row.id.clone());
            }
        }
    }
    sort_dedup_object_ids(&mut request_roots);
    sort_dedup_object_ids(&mut shallow_roots);
    let boundaries =
        daemon_fetch_pack_with_depth(url, &repo.objects_dir, &request_roots, Some(depth))?;
    write_shallow_file(
        &repo,
        boundaries_or_local_fallback(&repo, &shallow_roots, depth, boundaries)?,
    )
}

fn fetch_with_ssh_remote(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
    url: &str,
) -> Result<()> {
    let rows = ssh_ls_remote_rows(url, false, false, false, &[])?;
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let mut roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    if let Some(branch) = branch {
        let ref_name = branch_ref_name(&branch)?;
        let id = rows
            .iter()
            .find(|row| row.name == ref_name)
            .map(|row| row.id.clone())
            .ok_or_else(|| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        roots.push(id);
    } else {
        for row in rows
            .iter()
            .filter(|row| row.name.starts_with("refs/heads/"))
        {
            let branch = row
                .name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{}'", row.name),
                })?;
            destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &row.id)?;
            roots.push(row.id.clone());
        }
        if let Some(branch) = ssh_head_branch(url)? {
            destination_refs.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
        for row in rows.iter().filter(|row| row.name.starts_with("refs/tags/")) {
            if !row.name.ends_with("^{}") {
                destination_refs.write_ref(&row.name, &row.id)?;
                roots.push(row.id.clone());
            }
        }
    }
    sort_dedup_object_ids(&mut roots);
    ssh_fetch_pack(url, &repo.objects_dir, &roots)
}

fn fetch_with_ssh_remote_depth(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
    url: &str,
    depth: usize,
) -> Result<()> {
    let rows = ssh_ls_remote_rows(url, false, false, false, &[])?;
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let mut request_roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    let mut shallow_roots = Vec::with_capacity(transport_ref_collection_capacity(rows.len()));
    if let Some(branch) = branch {
        let ref_name = branch_ref_name(&branch)?;
        let id = rows
            .iter()
            .find(|row| row.name == ref_name)
            .map(|row| row.id.clone())
            .ok_or_else(|| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        request_roots.push(id.clone());
        shallow_roots.push(id);
    } else {
        for row in rows
            .iter()
            .filter(|row| row.name.starts_with("refs/heads/"))
        {
            let branch = row
                .name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{}'", row.name),
                })?;
            destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &row.id)?;
            request_roots.push(row.id.clone());
            shallow_roots.push(row.id.clone());
        }
        if let Some(branch) = ssh_head_branch(url)? {
            destination_refs.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
        for row in rows.iter().filter(|row| row.name.starts_with("refs/tags/")) {
            if !row.name.ends_with("^{}") {
                destination_refs.write_ref(&row.name, &row.id)?;
                request_roots.push(row.id.clone());
            }
        }
    }
    sort_dedup_object_ids(&mut request_roots);
    sort_dedup_object_ids(&mut shallow_roots);
    let boundaries =
        ssh_fetch_pack_with_depth(url, &repo.objects_dir, &request_roots, Some(depth))?;
    write_shallow_file(
        &repo,
        boundaries_or_local_fallback(&repo, &shallow_roots, depth, boundaries)?,
    )
}

fn clone_git_daemon(options: CloneHttpOptions) -> Result<()> {
    let CloneHttpOptions {
        quiet,
        configs,
        template,
        reject_shallow,
        recurse_submodules,
        remote_submodules,
        shallow_submodules,
        effective_bare,
        mirror,
        no_checkout,
        remote_name,
        no_tags,
        single_branch,
        no_single_branch,
        separate_git_dir,
        references,
        reference_if_able,
        shared: _shared,
        dissociate,
        depth,
        branch,
        keep_partial_on_missing_branch: _,
        repository,
        directory,
    } = options;
    let destination = match &directory {
        Some(path) => absolute_path_from_arg(path)?,
        None => default_daemon_clone_directory(&repository, effective_bare)?,
    };
    let destination_existed = destination.exists();
    let destination_label = clone_destination_label(directory.as_deref(), &destination);
    ensure_clone_destination(&destination, &destination_label)?;
    if !quiet {
        if effective_bare {
            eprintln!("Cloning into bare repository '{destination_label}'...");
        } else {
            eprintln!("Cloning into '{destination_label}'...");
        }
    }
    let _ = (
        reject_shallow,
        recurse_submodules,
        remote_submodules,
        shallow_submodules,
    );
    validate_remote_name(&remote_name)?;
    let mut reference_object_dirs = reference_object_dirs(&references)?;
    reference_object_dirs.extend(reference_if_able_object_dirs(&reference_if_able));
    let effective_single_branch = !no_single_branch && single_branch;
    let rows = daemon_ls_remote_rows(&repository, false, false, false, &[])?;
    let head_branch = daemon_head_branch(&repository)?;
    let target = http_clone_target(&rows, branch.as_deref(), head_branch.as_deref())?;
    let initial_branch = target
        .branch_name()
        .or(head_branch.as_deref())
        .unwrap_or("main")
        .to_owned();
    let result = init_repository(
        &destination,
        InitRepositoryOptions {
            bare: effective_bare,
            initial_branch,
        },
    )?;
    let git_dir = match separate_git_dir {
        Some(path) => relocate_separate_git_dir(&destination, &result.git_dir, &path)?,
        None => result.git_dir.clone(),
    };
    let repo = GitRepo {
        root: result.worktree,
        git_dir: git_dir.clone(),
        objects_dir: git_dir.join("objects"),
        index_path: git_dir.join("index"),
    };
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    if let Some(template) = template.as_ref() {
        apply_clone_template(&repo, template)?;
    }
    if let Err(error) = apply_clone_configs(&repo, &configs) {
        cleanup_failed_clone_config(&destination, &repo.git_dir, destination_existed);
        return Err(error);
    }
    if !dissociate {
        write_alternates_file(&repo.objects_dir, &reference_object_dirs)?;
    }
    let roots = http_clone_fetch_roots(&rows, &target, no_tags, single_branch, no_single_branch);
    let shallow_boundaries =
        daemon_fetch_pack_with_depth(&repository, &repo.objects_dir, &roots, depth)?;
    if let Some(depth) = depth {
        let shallow_roots = clone_shallow_roots(&repo, &roots)?;
        write_shallow_file(
            &repo,
            boundaries_or_local_fallback(&repo, &shallow_roots, depth, shallow_boundaries)?,
        )?;
    }

    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    http_write_clone_refs(
        &destination_refs,
        &rows,
        &remote_name,
        head_branch.as_deref(),
        &target,
        HttpCloneRefOptions {
            mirror,
            effective_bare,
            no_tags,
            single_branch,
            no_single_branch,
            requested_branch: branch.is_some(),
        },
    )?;
    if depth.is_some() {
        prune_missing_tag_refs(&destination_refs, &store)?;
    }
    set_config_value(&repo, &format!("remote.{remote_name}.url"), &repository)?;
    if !effective_bare {
        set_config_value(
            &repo,
            &format!("remote.{remote_name}.fetch"),
            &clone_fetch_refspec(&remote_name, &target, effective_single_branch),
        )?;
    } else if mirror {
        set_config_value(
            &repo,
            &format!("remote.{remote_name}.fetch"),
            "+refs/*:refs/*",
        )?;
    }
    if mirror {
        set_config_value(&repo, &format!("remote.{remote_name}.mirror"), "true")?;
    }
    if no_tags || mirror {
        set_config_value(&repo, &format!("remote.{remote_name}.tagOpt"), "--no-tags")?;
    }
    let head_id = match target {
        CloneTarget::Branch { ref id, .. }
        | CloneTarget::Tag { ref id, .. }
        | CloneTarget::Detached { ref id } => id.clone(),
        CloneTarget::MissingBranch { .. } => {
            unreachable!("git daemon clone does not keep missing branch")
        }
        CloneTarget::Empty => {
            println!("warning: You appear to have cloned an empty repository.");
            return Ok(());
        }
    };
    if effective_bare {
        match &target {
            CloneTarget::Branch { name, .. } => {
                destination_refs.write_head_symbolic(&format!("refs/heads/{name}"))?;
            }
            CloneTarget::Tag { id, .. } | CloneTarget::Detached { id } => {
                destination_refs.write_head_direct(id)?;
            }
            CloneTarget::MissingBranch { .. } => {
                unreachable!("git daemon clone does not keep missing branch")
            }
            CloneTarget::Empty => {}
        }
    } else if let CloneTarget::Branch {
        name: branch_name, ..
    } = &target
    {
        destination_refs.write_ref(&format!("refs/heads/{branch_name}"), &head_id)?;
        destination_refs.write_head_symbolic(&format!("refs/heads/{branch_name}"))?;
        set_config_value(&repo, &format!("branch.{branch_name}.remote"), &remote_name)?;
        set_config_value(
            &repo,
            &format!("branch.{branch_name}.merge"),
            &format!("refs/heads/{branch_name}"),
        )?;
    } else {
        destination_refs.write_head_direct(&head_id)?;
    }
    if effective_bare || no_checkout {
        return Ok(());
    }
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    checkout_fresh_worktree(&repo, &store, &head_id)
}

fn clone_ssh(options: CloneHttpOptions) -> Result<()> {
    let CloneHttpOptions {
        quiet,
        configs,
        template,
        reject_shallow,
        recurse_submodules,
        remote_submodules,
        shallow_submodules,
        effective_bare,
        mirror,
        no_checkout,
        remote_name,
        no_tags,
        single_branch,
        no_single_branch,
        separate_git_dir,
        references,
        reference_if_able,
        shared: _shared,
        dissociate,
        depth,
        branch,
        keep_partial_on_missing_branch: _,
        repository,
        directory,
    } = options;
    let destination = match &directory {
        Some(path) => absolute_path_from_arg(path)?,
        None => default_daemon_clone_directory(&repository, effective_bare)?,
    };
    let destination_existed = destination.exists();
    let destination_label = clone_destination_label(directory.as_deref(), &destination);
    ensure_clone_destination(&destination, &destination_label)?;
    if !quiet {
        if effective_bare {
            eprintln!("Cloning into bare repository '{destination_label}'...");
        } else {
            eprintln!("Cloning into '{destination_label}'...");
        }
    }
    let _ = (
        reject_shallow,
        recurse_submodules,
        remote_submodules,
        shallow_submodules,
    );
    validate_remote_name(&remote_name)?;
    let mut reference_object_dirs = reference_object_dirs(&references)?;
    reference_object_dirs.extend(reference_if_able_object_dirs(&reference_if_able));
    let effective_single_branch = !no_single_branch && single_branch;
    let rows = ssh_ls_remote_rows(&repository, false, false, false, &[])?;
    let head_branch = ssh_head_branch(&repository)?;
    let target = http_clone_target(&rows, branch.as_deref(), head_branch.as_deref())?;
    let initial_branch = target
        .branch_name()
        .or(head_branch.as_deref())
        .unwrap_or("main")
        .to_owned();
    let result = init_repository(
        &destination,
        InitRepositoryOptions {
            bare: effective_bare,
            initial_branch,
        },
    )?;
    let git_dir = match separate_git_dir {
        Some(path) => relocate_separate_git_dir(&destination, &result.git_dir, &path)?,
        None => result.git_dir.clone(),
    };
    let repo = GitRepo {
        root: result.worktree,
        git_dir: git_dir.clone(),
        objects_dir: git_dir.join("objects"),
        index_path: git_dir.join("index"),
    };
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    if let Some(template) = template.as_ref() {
        apply_clone_template(&repo, template)?;
    }
    if let Err(error) = apply_clone_configs(&repo, &configs) {
        cleanup_failed_clone_config(&destination, &repo.git_dir, destination_existed);
        return Err(error);
    }
    if !dissociate {
        write_alternates_file(&repo.objects_dir, &reference_object_dirs)?;
    }
    let roots = http_clone_fetch_roots(&rows, &target, no_tags, single_branch, no_single_branch);
    let shallow_boundaries =
        ssh_fetch_pack_with_depth(&repository, &repo.objects_dir, &roots, depth)?;
    if let Some(depth) = depth {
        let shallow_roots = clone_shallow_roots(&repo, &roots)?;
        write_shallow_file(
            &repo,
            boundaries_or_local_fallback(&repo, &shallow_roots, depth, shallow_boundaries)?,
        )?;
    }

    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    http_write_clone_refs(
        &destination_refs,
        &rows,
        &remote_name,
        head_branch.as_deref(),
        &target,
        HttpCloneRefOptions {
            mirror,
            effective_bare,
            no_tags,
            single_branch,
            no_single_branch,
            requested_branch: branch.is_some(),
        },
    )?;
    if depth.is_some() {
        prune_missing_tag_refs(&destination_refs, &store)?;
    }
    set_config_value(&repo, &format!("remote.{remote_name}.url"), &repository)?;
    if !effective_bare {
        set_config_value(
            &repo,
            &format!("remote.{remote_name}.fetch"),
            &clone_fetch_refspec(&remote_name, &target, effective_single_branch),
        )?;
    } else if mirror {
        set_config_value(
            &repo,
            &format!("remote.{remote_name}.fetch"),
            "+refs/*:refs/*",
        )?;
    }
    if mirror {
        set_config_value(&repo, &format!("remote.{remote_name}.mirror"), "true")?;
    }
    if no_tags || mirror {
        set_config_value(&repo, &format!("remote.{remote_name}.tagOpt"), "--no-tags")?;
    }
    let head_id = match target {
        CloneTarget::Branch { ref id, .. }
        | CloneTarget::Tag { ref id, .. }
        | CloneTarget::Detached { ref id } => id.clone(),
        CloneTarget::MissingBranch { .. } => {
            unreachable!("SSH clone does not keep missing branch")
        }
        CloneTarget::Empty => {
            println!("warning: You appear to have cloned an empty repository.");
            return Ok(());
        }
    };
    if effective_bare {
        match &target {
            CloneTarget::Branch { name, .. } => {
                destination_refs.write_head_symbolic(&format!("refs/heads/{name}"))?;
            }
            CloneTarget::Tag { id, .. } | CloneTarget::Detached { id } => {
                destination_refs.write_head_direct(id)?;
            }
            CloneTarget::MissingBranch { .. } => {
                unreachable!("SSH clone does not keep missing branch")
            }
            CloneTarget::Empty => {}
        }
    } else if let CloneTarget::Branch {
        name: branch_name, ..
    } = &target
    {
        destination_refs.write_ref(&format!("refs/heads/{branch_name}"), &head_id)?;
        destination_refs.write_head_symbolic(&format!("refs/heads/{branch_name}"))?;
        set_config_value(&repo, &format!("branch.{branch_name}.remote"), &remote_name)?;
        set_config_value(
            &repo,
            &format!("branch.{branch_name}.merge"),
            &format!("refs/heads/{branch_name}"),
        )?;
    } else {
        destination_refs.write_head_direct(&head_id)?;
    }
    if effective_bare || no_checkout {
        return Ok(());
    }
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    checkout_fresh_worktree(&repo, &store, &head_id)
}

fn fetch_with_repo_and_remote_depth(
    repo: GitRepo,
    remote: String,
    branch: Option<String>,
    missing_ref_code: i32,
    depth: usize,
) -> Result<()> {
    let url = remote_url(&repo, &remote)?;
    if is_http_transport_url(&url) {
        return fetch_with_http_remote_depth(repo, remote, branch, missing_ref_code, &url, depth);
    }
    if is_git_daemon_transport_url(&url) {
        return fetch_with_daemon_remote_depth(repo, remote, branch, missing_ref_code, &url, depth);
    }
    if is_ssh_transport_url(&url) {
        return fetch_with_ssh_remote_depth(repo, remote, branch, missing_ref_code, &url, depth);
    }
    let Some(source_path) = local_repository_path_from_location(&url)? else {
        return Err(unsupported_remote_helper_error(&url, String::new()));
    };
    let source = local_clone_source(&source_path)?;
    let source_refs = refs_adapter_from_git_dir(&source.git_dir);
    let destination_refs = refs_adapter_from_git_dir(&repo.git_dir);
    let source_store = object_adapter_from_objects_dir(source.git_dir.join("objects"));
    let destination_store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let source_repo = GitRepo {
        root: source.git_dir.clone(),
        git_dir: source.git_dir.clone(),
        objects_dir: source.git_dir.join("objects"),
        index_path: source.git_dir.join("index"),
    };
    if let Some(branch) = branch {
        let mut fetched_objects = HashSet::with_capacity(1);
        let mut roots = Vec::with_capacity(1);
        let ref_name = branch_ref_name(&branch)?;
        let id = source_refs
            .resolve(&ref_name)
            .map_err(|_| missing_remote_ref_error(&branch, missing_ref_code))?;
        destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
        let depth_limited_commits =
            upload_pack_depth_limited_commits(&source_store, std::slice::from_ref(&id), depth)?;
        copy_reachable_objects_for_depth_into(
            &source_store,
            &destination_store,
            &depth_limited_commits,
            &mut fetched_objects,
        )?;
        roots.push(id);
        copy_fetch_pack_included_tags(
            &source_refs,
            &source_store,
            &destination_store,
            &source_repo,
            &mut fetched_objects,
            Some(depth),
        )?;
        write_shallow_file(&repo, shallow_boundaries(&source_store, &roots, depth)?)?;
        return Ok(());
    } else {
        let mut fetched_objects = HashSet::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
        let mut roots = Vec::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
        source_refs.for_each_resolved_ref("refs/heads/", |ref_name, id| {
            let branch = ref_name
                .strip_prefix("refs/heads/")
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("invalid source branch ref '{ref_name}'"),
                })?;
            destination_refs.write_ref(&format!("refs/remotes/{remote}/{branch}"), &id)?;
            let depth_limited_commits =
                upload_pack_depth_limited_commits(&source_store, std::slice::from_ref(id), depth)?;
            copy_reachable_objects_for_depth_into(
                &source_store,
                &destination_store,
                &depth_limited_commits,
                &mut fetched_objects,
            )?;
            roots.push(id.clone());
            Ok::<(), CliError>(())
        })?;
        if let Some(branch) = source_head_branch(&source_refs)? {
            destination_refs.write_symbolic_ref(
                &format!("refs/remotes/{remote}/HEAD"),
                &format!("refs/remotes/{remote}/{branch}"),
            )?;
        }
        source_refs.for_each_ref_name("refs/tags/", |ref_name| {
            match source_refs.read_ref(ref_name)? {
                RefTarget::Direct(id) => {
                    destination_refs.write_ref(ref_name, &id)?;
                    let _ = copy_object_if_missing(
                        &source_store,
                        &destination_store,
                        &id,
                        &mut fetched_objects,
                    )?;
                }
                RefTarget::Symbolic(target) => {
                    destination_refs.write_symbolic_ref(ref_name, &target)?
                }
            }
            Ok::<(), CliError>(())
        })?;
        copy_fetch_pack_included_tags(
            &source_refs,
            &source_store,
            &destination_store,
            &source_repo,
            &mut fetched_objects,
            Some(depth),
        )?;
        write_shallow_file(&repo, shallow_boundaries(&source_store, &roots, depth)?)?;
    }
    Ok(())
}

fn boundaries_or_local_fallback(
    repo: &GitRepo,
    roots: &[ObjectId],
    depth: usize,
    remote_boundaries: Vec<ObjectId>,
) -> Result<Vec<ObjectId>> {
    if !remote_boundaries.is_empty() {
        return Ok(remote_boundaries);
    }
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    shallow_boundaries(&store, roots, depth)
}

fn clone_shallow_roots(repo: &GitRepo, roots: &[ObjectId]) -> Result<Vec<ObjectId>> {
    let store = object_adapter_from_objects_dir(repo.objects_dir.clone());
    let mut out = Vec::with_capacity(transport_ref_collection_capacity(roots.len()));
    let mut seen = HashSet::with_capacity(transport_ref_collection_capacity(roots.len()));
    for id in roots {
        let kind = object_kind_hint_or_read(&store, id)?;
        let commit_id = if kind == GitObjectKind::Tag {
            peel_tag(&store, id)?.unwrap_or_else(|| id.clone())
        } else {
            id.clone()
        };
        if object_kind_hint_or_read(&store, &commit_id)? == GitObjectKind::Commit
            && seen.insert(commit_id.clone())
        {
            out.push(commit_id);
        }
    }
    Ok(out)
}

fn prune_missing_tag_refs(refs: &RefStore, store: &LooseObjectStore) -> Result<()> {
    let mut missing_refs = Vec::new();
    refs.for_each_resolved_ref("refs/tags/", |ref_name, id| {
        if store.read_object(id).is_err() {
            missing_refs.push(ref_name.to_owned());
        }
        Ok::<(), CliError>(())
    })?;
    for ref_name in missing_refs {
        refs.delete_ref(&ref_name)?;
    }
    Ok(())
}

fn object_kind_hint_or_read(store: &LooseObjectStore, id: &ObjectId) -> Result<GitObjectKind> {
    match store.object_kind_hint(id)? {
        Some(kind) => Ok(kind),
        None => Ok(store.read_object(id)?.kind),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PullRebaseMode {
    Disabled,
    Rebase,
    RebaseMerges,
    Interactive,
}

impl PullRebaseMode {
    fn rebases(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

fn pull_rebase_after_fetch(
    repo: &GitRepo,
    branch: &str,
    cli_mode: Option<&str>,
) -> Result<PullRebaseMode> {
    if let Some(mode) = cli_mode {
        return parse_pull_rebase_mode(mode, "--rebase");
    }
    if let Some(mode) = read_config_section_value(repo, "branch", branch, "rebase")? {
        return parse_pull_rebase_mode(&mode, &format!("branch.{branch}.rebase"));
    }
    if let Some(mode) = read_config_value(repo, "pull.rebase")? {
        return parse_pull_rebase_mode(&mode, "pull.rebase");
    }
    Ok(PullRebaseMode::Disabled)
}

fn parse_pull_rebase_mode(value: &str, source: &str) -> Result<PullRebaseMode> {
    if let Some(value) = parse_git_bool(value) {
        return Ok(if value {
            PullRebaseMode::Rebase
        } else {
            PullRebaseMode::Disabled
        });
    }
    match value {
        "merges" => Ok(PullRebaseMode::RebaseMerges),
        "interactive" => Ok(PullRebaseMode::Interactive),
        _ if source == "--rebase" => Err(CliError::Stderr {
            code: 129,
            text: format!("error: invalid value for '--rebase': '{value}'\n"),
        }),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("invalid value for '{source}': '{value}'"),
        }),
    }
}

fn missing_remote_ref_error(branch: &str, code: i32) -> CliError {
    CliError::Stderr {
        code,
        text: format!("fatal: couldn't find remote ref {branch}\n"),
    }
}

impl ParsedDaemonUrl {
    fn parse(value: &str) -> Result<Self> {
        let rest = value
            .strip_prefix("git://")
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "git daemon transport supports git:// URLs".into(),
            })?;
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
            let port = port.parse::<u16>().map_err(|_| CliError::Fatal {
                code: 128,
                message: format!("invalid git:// URL port: {port}"),
            })?;
            (host.to_owned(), port)
        } else {
            (authority.to_owned(), 9418)
        };
        if host.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "git:// URL host cannot be empty".into(),
            });
        }
        Ok(Self {
            host,
            port,
            path: format!("/{path}").trim_end_matches('/').to_owned(),
        })
    }

    fn host_header_len(&self) -> usize {
        self.host.len() + daemon_host_port_suffix_len(self.port)
    }
}

fn daemon_host_port_suffix_len(port: u16) -> usize {
    if port == 9418 {
        0
    } else {
        1 + decimal_len(usize::from(port))
    }
}

pub(crate) fn is_git_daemon_transport_url(value: &str) -> bool {
    value.starts_with("git://")
}

impl ParsedSshUrl {
    fn parse(value: &str) -> Result<Self> {
        if let Some(rest) = value.strip_prefix("ssh://") {
            let (authority, raw_path) = rest.split_once('/').unwrap_or((rest, ""));
            let (user_host, port) = if let Some((left, right)) = authority.rsplit_once(':') {
                if !right.is_empty() && right.as_bytes().iter().all(u8::is_ascii_digit) {
                    let port = right.parse::<u16>().map_err(|_| CliError::Fatal {
                        code: 128,
                        message: format!("invalid ssh URL port: {right}"),
                    })?;
                    (left, Some(port))
                } else {
                    (authority, None)
                }
            } else {
                (authority, None)
            };
            let (user, host) = if let Some((user, host)) = user_host.rsplit_once('@') {
                (Some(user.to_owned()), host.to_owned())
            } else {
                (None, user_host.to_owned())
            };
            if host.is_empty() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "ssh URL host cannot be empty".into(),
                });
            }
            let path = format!("/{raw_path}");
            if path == "/" {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "ssh URL path cannot be empty".into(),
                });
            }
            return Ok(Self {
                user,
                host,
                port,
                path,
            });
        }

        let Some((host_part, path_part)) = value.split_once(':') else {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid ssh/scp transport URL: {value}"),
            });
        };
        if host_part.is_empty() || path_part.is_empty() || value.contains("://") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid ssh/scp transport URL: {value}"),
            });
        }
        let (user, host) = if let Some((user, host)) = host_part.rsplit_once('@') {
            (Some(user.to_owned()), host.to_owned())
        } else {
            (None, host_part.to_owned())
        };
        if host.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid ssh/scp transport host in '{value}'"),
            });
        }
        Ok(Self {
            user,
            host,
            port: None,
            path: path_part.to_owned(),
        })
    }

    fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }
}

impl RemoteCommandSession {
    fn finish(mut self) -> Result<()> {
        drop(self.stdin.take());
        let status = self.child.wait()?;
        let mut stderr = String::new();
        if let Some(mut pipe) = self.stderr.take() {
            pipe.read_to_string(&mut stderr)?;
        }
        if status.success() {
            return Ok(());
        }
        let stderr = stderr.trim();
        Err(CliError::Fatal {
            code: status.code().unwrap_or(128),
            message: if stderr.is_empty() {
                "ssh transport command failed".into()
            } else {
                stderr.to_owned()
            },
        })
    }

    fn abandon(mut self) -> Result<()> {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(mut pipe) = self.stderr.take() {
            let mut stderr = String::new();
            let _ = pipe.read_to_string(&mut stderr);
        }
        Ok(())
    }
}

pub(crate) fn is_ssh_transport_url(value: &str) -> bool {
    value.starts_with("ssh://")
        || (!value.contains("://")
            && value.contains(':')
            && !value.starts_with('/')
            && !value.starts_with("./")
            && !value.starts_with("../"))
}

fn shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn ssh_command_argv() -> Result<Vec<String>> {
    if let Ok(command) = std::env::var("GIT_SSH_COMMAND") {
        let words = split_shell_words(&command)?;
        if words.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "GIT_SSH_COMMAND is empty".into(),
            });
        }
        return Ok(words);
    }
    if let Ok(command) = std::env::var("GIT_SSH") {
        if command.trim().is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "GIT_SSH is empty".into(),
            });
        }
        return Ok(vec![command]);
    }
    Ok(vec!["ssh".to_owned()])
}

fn spawn_ssh_remote_command(url: &ParsedSshUrl, service: &str) -> Result<RemoteCommandSession> {
    let mut argv = ssh_command_argv()?;
    let program = argv.remove(0);
    let mut command = std::process::Command::new(program);
    command.args(argv);
    if let Some(port) = url.port {
        command.arg("-p").arg(port.to_string());
    }
    command
        .arg(url.destination())
        .arg(format!("{service} {}", shell_quote_single(&url.path)))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    Ok(RemoteCommandSession {
        stdin: child.stdin.take(),
        stdout: Some(io::BufReader::new(child.stdout.take().ok_or_else(
            || CliError::Fatal {
                code: 128,
                message: "ssh transport did not provide stdout".into(),
            },
        )?)),
        stderr: child.stderr.take(),
        child,
    })
}

fn ssh_receive_pack_advertisement(url: &str) -> Result<ReceivePackAdvertisement> {
    let parsed = ParsedSshUrl::parse(url)?;
    let mut session = spawn_ssh_remote_command(&parsed, "git-receive-pack")?;
    let advertisement = {
        let stdout = session.stdout.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdout is unavailable".into(),
        })?;
        parse_receive_pack_advertisement(stdout)?
    };
    session.abandon()?;
    Ok(advertisement)
}

fn http_receive_pack_advertisement_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
) -> Result<ReceivePackAdvertisement> {
    let response = helper.request_to_body(
        url,
        "GET",
        "info/refs?service=git-receive-pack",
        &[],
        &PackBody::Empty,
    )?;
    if response.status_code != 200 {
        return Err(CliError::Fatal {
            code: 128,
            message: "remote does not advertise git-receive-pack over HTTP".into(),
        });
    }
    response
        .body
        .with_reader(|reader| parse_smart_http_receive_pack_advertisement_from_reader(reader))?
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "remote git-receive-pack advertisement is malformed".into(),
        })
}

fn daemon_receive_pack_advertisement(url: &str) -> Result<ReceivePackAdvertisement> {
    let url = ParsedDaemonUrl::parse(url)?;
    let mut stream = daemon_transport_connect(&url)?;
    daemon_transport_service_handshake(&mut stream, &url, "git-receive-pack")?;
    let mut reader = daemon_transport_reader(stream);
    parse_receive_pack_advertisement(&mut reader)
}

fn parse_receive_pack_advertisement<R: BufRead + ?Sized>(
    reader: &mut R,
) -> Result<ReceivePackAdvertisement> {
    let mut refs = BTreeMap::new();
    let mut capabilities = BTreeSet::new();
    let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    while read_pkt_line_payload_into(reader, &mut line)? {
        let payload = line.split(|byte| *byte == 0).next().unwrap_or(&line);
        let Some((id, name)) = split_ls_remote_space_payload(payload) else {
            continue;
        };
        let id = ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id)?;
        refs.insert(String::from_utf8_lossy(name).into_owned(), id);
        if let Some(nul_pos) = line.iter().position(|byte| *byte == 0) {
            for capability in ascii_tokens(trim_lf_payload(&line[nul_pos + 1..])) {
                capabilities.insert(String::from_utf8_lossy(capability).into_owned());
            }
        }
    }
    Ok(ReceivePackAdvertisement { refs, capabilities })
}

fn parse_smart_http_receive_pack_advertisement_from_reader<R: BufRead + ?Sized>(
    reader: &mut R,
) -> Result<Option<ReceivePackAdvertisement>> {
    if !read_smart_http_service_header(
        reader,
        &[*b"001f", *b"001e"],
        b"# service=git-receive-pack\n",
        b"# service=git-receive-pack",
    )? {
        return Ok(None);
    }
    Ok(Some(parse_receive_pack_advertisement(reader)?))
}

fn build_upload_pack_request(roots: &[ObjectId], depth: Option<usize>) -> Result<Vec<u8>> {
    let mut request = Vec::with_capacity(upload_pack_request_capacity(roots, depth));
    let first_extra = b" side-band-64k ofs-delta include-tag";
    for (idx, root) in roots.iter().enumerate() {
        let extra = if idx == 0 {
            first_extra.as_slice()
        } else {
            &[]
        };
        append_pkt_line_len(
            &mut request,
            b"want ".len() + root.hex_len() + extra.len() + 1,
        )?;
        request.extend_from_slice(b"want ");
        root.write_hex_bytes(&mut request);
        request.extend_from_slice(extra);
        request.push(b'\n');
    }
    if let Some(depth) = depth {
        let depth_start = request.len();
        append_pkt_line_len(&mut request, b"deepen ".len() + decimal_len(depth) + 1)?;
        request.extend_from_slice(b"deepen ");
        append_decimal_usize(&mut request, depth);
        request.push(b'\n');
        debug_assert_eq!(
            request.len() - depth_start,
            4 + b"deepen ".len() + decimal_len(depth) + 1
        );
    }
    request.extend_from_slice(b"0000");
    append_pkt_line_len(&mut request, b"done\n".len())?;
    request.extend_from_slice(b"done\n");
    Ok(request)
}

fn upload_pack_request_capacity(roots: &[ObjectId], depth: Option<usize>) -> usize {
    let first_extra = " side-band-64k ofs-delta include-tag".len();
    let wants = roots
        .iter()
        .enumerate()
        .map(|(idx, root)| {
            4 + "want ".len() + root.hex_len() + usize::from(idx == 0) * first_extra + 1
        })
        .sum::<usize>();
    let deepen = depth
        .map(|depth| 4 + "deepen ".len() + decimal_len(depth) + 1)
        .unwrap_or(0);
    wants + deepen + 4 + 4 + "done\n".len()
}

fn decimal_len(mut value: usize) -> usize {
    let mut len = 1;
    while value >= 10 {
        value /= 10;
        len += 1;
    }
    len
}

fn parse_upload_pack_sideband_response_to_file<R: Read>(
    reader: &mut R,
    pack_path: &Path,
    shallow_hint: usize,
) -> Result<Option<Vec<ObjectId>>> {
    let file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(pack_path)?;
    parse_upload_pack_sideband_response_to_open_file(reader, file, shallow_hint)
}

fn parse_upload_pack_sideband_response_to_open_file<R: Read>(
    reader: &mut R,
    file: fs::File,
    shallow_hint: usize,
) -> Result<Option<Vec<ObjectId>>> {
    let mut shallow_boundaries = Vec::with_capacity(shallow_hint);
    let mut payload = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    loop {
        match read_upload_pack_response_pkt_line_into(reader, &mut payload)? {
            PktLineRead::Eof => return Ok(None),
            PktLineRead::Flush => {
                continue;
            }
            PktLineRead::Payload => {}
        }
        if let Some(id) = upload_pack_shallow_id_from_payload(&payload) {
            shallow_boundaries.push(ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id)?);
            continue;
        }
        if payload != b"NAK\n" && !payload.starts_with(b"ACK ") {
            return Ok(None);
        }
        break;
    }
    if !write_upload_pack_sideband_pack_to_open_file(reader, file)? {
        return Ok(None);
    }
    sort_dedup_object_ids(&mut shallow_boundaries);
    Ok(Some(shallow_boundaries))
}

fn upload_pack_shallow_id_from_payload(payload: &[u8]) -> Option<&[u8]> {
    payload
        .strip_prefix(b"shallow ")
        .map(|id| trim_ascii_whitespace(trim_lf_payload(id)))
        .filter(|id| !id.is_empty())
}

fn http_fetch_smart_pack_with_depth_with_helper(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
    depth: Option<usize>,
) -> Result<Vec<ObjectId>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let request = build_upload_pack_request(roots, depth)?;
    let response_path = temp_http_helper_output_path()?;
    let result = (|| {
        let head = helper.request_to_file(
            url,
            "POST",
            "git-upload-pack",
            &request,
            &PackBody::Empty,
            &response_path,
        )?;
        if head.status_code != 200 {
            return Err(CliError::Fatal {
                code: 128,
                message: "dumb http transport does not support shallow capabilities".into(),
            });
        }
        let mut body = http_helper_file_body_reader(fs::File::open(&response_path)?);
        let (temp_pack, file) = temp_http_pack_file(objects_dir)?;
        let Some(shallow_boundaries) =
            parse_upload_pack_sideband_response_to_open_file(&mut body, file, roots.len())?
        else {
            let _ = fs::remove_file(&temp_pack);
            return Err(CliError::Fatal {
                code: 128,
                message: "remote upload-pack response did not contain a pack".into(),
            });
        };
        write_indexed_pack_file(objects_dir, &temp_pack)?;
        Ok(shallow_boundaries)
    })();
    let _ = fs::remove_file(response_path);
    result
}

fn http_fetch_smart_pack_with_depth_direct(
    url: &ParsedHttpUrl,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
    depth: Option<usize>,
) -> Result<Vec<ObjectId>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let request = build_upload_pack_request(roots, depth)?;
    let (head, mut body) = http_request_reader(url, "POST", "git-upload-pack", &request)?;
    if head.status_code != 200 {
        return Err(CliError::Fatal {
            code: 128,
            message: "dumb http transport does not support shallow capabilities".into(),
        });
    }
    let (temp_pack, file) = temp_http_pack_file(objects_dir)?;
    let Some(shallow_boundaries) =
        parse_upload_pack_sideband_response_to_open_file(&mut body, file, roots.len())?
    else {
        let _ = fs::remove_file(&temp_pack);
        return Err(CliError::Fatal {
            code: 128,
            message: "remote upload-pack response did not contain a pack".into(),
        });
    };
    write_indexed_pack_file(objects_dir, &temp_pack)?;
    Ok(shallow_boundaries)
}

pub(crate) fn ssh_ls_remote_rows(
    url: &str,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Vec<LsRemoteRow>> {
    let parsed = ParsedSshUrl::parse(url)?;
    let mut session = spawn_ssh_remote_command(&parsed, "git-upload-pack")?;
    let rows = {
        let stdout = session.stdout.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdout is unavailable".into(),
        })?;
        parse_daemon_upload_pack_rows(stdout, heads, tags, refs_only, patterns)?
    };
    if rows.is_empty() {
        session.finish()?;
    } else {
        session.abandon()?;
    }
    Ok(rows)
}

fn ssh_head_branch(url: &str) -> Result<Option<String>> {
    let parsed = ParsedSshUrl::parse(url)?;
    let mut session = spawn_ssh_remote_command(&parsed, "git-upload-pack")?;
    let branch = {
        let stdout = session.stdout.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdout is unavailable".into(),
        })?;
        let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
        let branch = if read_pkt_line_payload_into(stdout, &mut line)? {
            line.iter()
                .position(|byte| *byte == 0)
                .and_then(|nul_pos| head_symref_branch_from_capabilities(&line[nul_pos + 1..]))
        } else {
            None
        };
        while read_pkt_line_payload_into(stdout, &mut line)? {}
        branch
    };
    session.abandon()?;
    Ok(branch)
}

pub(crate) fn ssh_fetch_pack(
    url: &str,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
) -> Result<()> {
    ssh_fetch_pack_with_depth(url, objects_dir, roots, None).map(|_| ())
}

fn ssh_fetch_pack_with_depth(
    url: &str,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
    depth: Option<usize>,
) -> Result<Vec<ObjectId>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let parsed = ParsedSshUrl::parse(url)?;
    let mut session = spawn_ssh_remote_command(&parsed, "git-upload-pack")?;
    {
        let stdout = session.stdout.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdout is unavailable".into(),
        })?;
        let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
        while read_pkt_line_payload_into(stdout, &mut line)? {}
    }

    let request = build_upload_pack_request(roots, depth)?;
    session
        .stdin
        .as_mut()
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdin is unavailable".into(),
        })?
        .write_all(&request)?;
    drop(session.stdin.take());

    let temp_pack = temp_http_pack_path(objects_dir)?;
    let Some(shallow_boundaries) = parse_upload_pack_sideband_response_to_file(
        session.stdout.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdout is unavailable".into(),
        })?,
        &temp_pack,
        roots.len(),
    )?
    else {
        let _ = fs::remove_file(&temp_pack);
        return Err(CliError::Fatal {
            code: 128,
            message: "ssh upload-pack response did not contain a pack".into(),
        });
    };
    write_indexed_pack_file(objects_dir, &temp_pack)?;
    session.finish()?;
    Ok(shallow_boundaries)
}

fn ssh_send_receive_pack(
    url: &str,
    push_refs: &[(PushRef, Option<ObjectId>)],
    store: &LooseObjectStore,
    object_ids: &[ObjectId],
) -> Result<()> {
    let parsed = ParsedSshUrl::parse(url)?;
    let mut session = spawn_ssh_remote_command(&parsed, "git-receive-pack")?;
    let capabilities = {
        let stdout = session.stdout.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdout is unavailable".into(),
        })?;
        parse_receive_pack_advertisement(stdout)?.capabilities
    };
    if !capabilities.contains("report-status") {
        return Err(CliError::Fatal {
            code: 128,
            message: "remote receive-pack does not advertise report-status".into(),
        });
    }

    let request = build_receive_pack_request_commands(push_refs)?;
    let stdin = session.stdin.as_mut().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "ssh transport stdin is unavailable".into(),
    })?;
    stdin.write_all(&request)?;
    write_push_pack_to_writer(store, object_ids, stdin)?;
    drop(session.stdin.take());

    {
        let stdout = session.stdout.as_mut().ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "ssh transport stdout is unavailable".into(),
        })?;
        parse_receive_pack_report_status(stdout)?;
    }
    session.finish()
}

fn http_send_receive_pack_with_helper_session(
    url: &ParsedHttpUrl,
    helper: &mut RemoteHttpHelperSession,
    push_refs: &[(PushRef, Option<ObjectId>)],
    pack: &PackBody,
) -> Result<()> {
    let request = build_receive_pack_request_commands(push_refs)?;
    let response = helper.request_to_body(url, "POST", "git-receive-pack", &request, pack)?;
    if response.status_code != 200 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("HTTP receive-pack request failed: {}", response.status_line),
        });
    }
    response
        .body
        .with_reader(|response_body| parse_receive_pack_report_status(response_body))
}

fn parse_receive_pack_report_status<R: BufRead + ?Sized>(reader: &mut R) -> Result<()> {
    let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    if !read_pkt_line_payload_into(reader, &mut line)? {
        return Err(CliError::Fatal {
            code: 128,
            message: "remote receive-pack did not return report-status".into(),
        });
    }
    let status = trim_lf_payload(&line);
    if status != b"unpack ok" {
        return Err(CliError::Fatal {
            code: 1,
            message: protocol_line_for_error(status),
        });
    }
    while read_pkt_line_payload_into(reader, &mut line)? {
        let status = trim_lf_payload(&line);
        if let Some(ref_name) = status.strip_prefix(b"ok ") {
            let _ = ref_name;
            continue;
        }
        if let Some(message) = status.strip_prefix(b"ng ") {
            return Err(CliError::Fatal {
                code: 1,
                message: protocol_line_for_error(message),
            });
        }
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "unexpected receive-pack status line: {}",
                protocol_line_for_error(status)
            ),
        });
    }
    Ok(())
}

fn build_receive_pack_request_commands(
    push_refs: &[(PushRef, Option<ObjectId>)],
) -> Result<Vec<u8>> {
    let mut request = Vec::with_capacity(receive_pack_request_capacity(push_refs));
    let mut updates = push_refs.iter();
    if let Some((first, old)) = updates.next() {
        append_receive_pack_command(
            &mut request,
            old.as_ref(),
            first.id.as_ref(),
            &first.destination,
            Some(b"report-status ofs-delta"),
        )?;
        for (push_ref, old) in updates {
            append_receive_pack_command(
                &mut request,
                old.as_ref(),
                push_ref.id.as_ref(),
                &push_ref.destination,
                None,
            )?;
        }
    }
    request.extend_from_slice(b"0000");
    Ok(request)
}

fn append_receive_pack_command(
    request: &mut Vec<u8>,
    old: Option<&ObjectId>,
    new: Option<&ObjectId>,
    destination: &str,
    capabilities: Option<&[u8]>,
) -> Result<()> {
    let zero_len = GitHashAlgorithm::Sha1.digest_len() * 2;
    let old_len = old.map(ObjectId::hex_len).unwrap_or(zero_len);
    let new_len = new.map(ObjectId::hex_len).unwrap_or(zero_len);
    let capability_len = capabilities
        .map(|capabilities| 1 + capabilities.len())
        .unwrap_or(0);
    append_pkt_line_len(
        request,
        old_len + 1 + new_len + 1 + destination.len() + capability_len + 1,
    )?;
    append_object_id_or_zero(request, old, GitHashAlgorithm::Sha1);
    request.push(b' ');
    append_object_id_or_zero(request, new, GitHashAlgorithm::Sha1);
    request.push(b' ');
    request.extend_from_slice(destination.as_bytes());
    if let Some(capabilities) = capabilities {
        request.push(0);
        request.extend_from_slice(capabilities);
    }
    request.push(b'\n');
    Ok(())
}

fn receive_pack_request_capacity(push_refs: &[(PushRef, Option<ObjectId>)]) -> usize {
    let zero_len = GitHashAlgorithm::Sha1.digest_len() * 2;
    push_refs
        .iter()
        .enumerate()
        .map(|(idx, (push_ref, old))| {
            let old_len = old.as_ref().map(ObjectId::hex_len).unwrap_or(zero_len);
            let new_len = push_ref
                .id
                .as_ref()
                .map(ObjectId::hex_len)
                .unwrap_or(zero_len);
            4 + old_len
                + 1
                + new_len
                + 1
                + push_ref.destination.len()
                + usize::from(idx == 0) * "\0report-status ofs-delta".len()
                + 1
        })
        .sum::<usize>()
        + 4
}

fn daemon_transport_connect(url: &ParsedDaemonUrl) -> Result<std::net::TcpStream> {
    let stream = std::net::TcpStream::connect((url.host.as_str(), url.port))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

fn daemon_transport_reader(stream: std::net::TcpStream) -> io::BufReader<std::net::TcpStream> {
    io::BufReader::with_capacity(DAEMON_TRANSPORT_READ_BUF_CAPACITY, stream)
}

fn daemon_transport_handshake(
    stream: &mut std::net::TcpStream,
    url: &ParsedDaemonUrl,
) -> Result<()> {
    daemon_transport_service_handshake(stream, url, "git-upload-pack")
}

fn daemon_transport_service_handshake(
    stream: &mut std::net::TcpStream,
    url: &ParsedDaemonUrl,
    service: &str,
) -> Result<()> {
    let mut request = Vec::with_capacity(daemon_service_request_capacity(url, service));
    write_daemon_service_request(&mut request, url, service)?;
    stream.write_all(&request)?;
    stream.flush()?;
    Ok(())
}

fn daemon_service_request_capacity(url: &ParsedDaemonUrl, service: &str) -> usize {
    4 + daemon_service_request_payload_len(url, service)
}

fn daemon_service_request_payload_len(url: &ParsedDaemonUrl, service: &str) -> usize {
    service.len() + 1 + url.path.len() + "\0host=".len() + url.host_header_len() + 1
}

fn write_daemon_service_request(
    out: &mut Vec<u8>,
    url: &ParsedDaemonUrl,
    service: &str,
) -> Result<()> {
    write_pkt_line_header(out, daemon_service_request_payload_len(url, service))?;
    out.extend_from_slice(service.as_bytes());
    out.push(b' ');
    out.extend_from_slice(url.path.as_bytes());
    out.extend_from_slice(b"\0host=");
    out.extend_from_slice(url.host.as_bytes());
    if url.port != 9418 {
        out.push(b':');
        append_decimal_usize(out, usize::from(url.port));
    }
    out.push(0);
    Ok(())
}

pub(crate) fn daemon_ls_remote_rows(
    url: &str,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Vec<LsRemoteRow>> {
    let url = ParsedDaemonUrl::parse(url)?;
    let mut stream = daemon_transport_connect(&url)?;
    daemon_transport_handshake(&mut stream, &url)?;
    let mut reader = daemon_transport_reader(stream);
    parse_daemon_upload_pack_rows(&mut reader, heads, tags, refs_only, patterns)
}

fn parse_daemon_upload_pack_rows<R: BufRead>(
    reader: &mut R,
    heads: bool,
    tags: bool,
    refs_only: bool,
    patterns: &[String],
) -> Result<Vec<LsRemoteRow>> {
    let mut rows = Vec::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
    let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    while read_pkt_line_payload_into(reader, &mut line)? {
        let payload = line.split(|byte| *byte == 0).next().unwrap_or(&line);
        let Some((id, name)) = split_ls_remote_space_payload(payload) else {
            continue;
        };
        if !ls_remote_ref_name_selected(name, heads, tags, refs_only) {
            continue;
        }
        let id = ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, id)?;
        push_ls_remote_row_bytes(&mut rows, id, name, patterns);
    }
    Ok(rows)
}

fn daemon_head_branch(url: &str) -> Result<Option<String>> {
    let url = ParsedDaemonUrl::parse(url)?;
    let mut stream = daemon_transport_connect(&url)?;
    daemon_transport_handshake(&mut stream, &url)?;
    let mut reader = daemon_transport_reader(stream);
    let Some(line) = read_pkt_line_payload_from_reader(&mut reader)? else {
        return Ok(None);
    };
    let Some(nul_pos) = line.iter().position(|byte| *byte == 0) else {
        return Ok(None);
    };
    let capabilities = &line[nul_pos + 1..];
    Ok(head_symref_branch_from_capabilities(capabilities))
}

pub(crate) fn daemon_fetch_pack(
    url: &str,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
) -> Result<()> {
    daemon_fetch_pack_with_depth(url, objects_dir, roots, None).map(|_| ())
}

fn daemon_fetch_pack_with_depth(
    url: &str,
    objects_dir: &std::path::Path,
    roots: &[ObjectId],
    depth: Option<usize>,
) -> Result<Vec<ObjectId>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let url = ParsedDaemonUrl::parse(url)?;
    let mut stream = daemon_transport_connect(&url)?;
    daemon_transport_handshake(&mut stream, &url)?;
    let mut reader = daemon_transport_reader(stream.try_clone()?);
    let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    while read_pkt_line_payload_into(&mut reader, &mut line)? {}

    let request = build_upload_pack_request(roots, depth)?;
    stream.write_all(&request)?;
    stream.flush()?;

    let temp_pack = temp_http_pack_path(objects_dir)?;
    let Some(shallow_boundaries) =
        parse_upload_pack_sideband_response_to_file(&mut reader, &temp_pack, roots.len())?
    else {
        let _ = fs::remove_file(&temp_pack);
        return Err(CliError::Fatal {
            code: 128,
            message: "git daemon upload-pack response did not contain a pack".into(),
        });
    };
    write_indexed_pack_file(objects_dir, &temp_pack)?;
    Ok(shallow_boundaries)
}

fn daemon_send_receive_pack(
    url: &str,
    push_refs: &[(PushRef, Option<ObjectId>)],
    store: &LooseObjectStore,
    object_ids: &[ObjectId],
) -> Result<()> {
    let url = ParsedDaemonUrl::parse(url)?;
    let mut stream = daemon_transport_connect(&url)?;
    daemon_transport_service_handshake(&mut stream, &url, "git-receive-pack")?;
    let mut reader = daemon_transport_reader(stream.try_clone()?);
    let advertisement = parse_receive_pack_advertisement(&mut reader)?;
    if !advertisement.capabilities.contains("report-status") {
        return Err(CliError::Fatal {
            code: 128,
            message: "remote receive-pack does not advertise report-status".into(),
        });
    }

    let request = build_receive_pack_request_commands(push_refs)?;
    stream.write_all(&request)?;
    write_push_pack_to_writer(store, object_ids, &mut stream)?;
    stream.flush()?;

    parse_receive_pack_report_status(&mut reader)
}

pub(crate) fn http_backend() -> Result<()> {
    let method = std::env::var("REQUEST_METHOD").map_err(|_| CliError::Fatal {
        code: 1,
        message: "No REQUEST_METHOD from server".into(),
    })?;
    let path_info = std::env::var("PATH_INFO").map_err(|_| CliError::Fatal {
        code: 1,
        message: "No PATH_INFO from server".into(),
    })?;
    let query = std::env::var("QUERY_STRING").unwrap_or_default();
    let project_root = std::env::var("GIT_PROJECT_ROOT").map_err(|_| CliError::Fatal {
        code: 1,
        message: "No GIT_PROJECT_ROOT from server".into(),
    })?;

    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut stdout = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdout);
    if method != "GET" {
        if method == "POST" && path_info.ends_with("/git-upload-pack") && query.is_empty() {
            let repo = http_backend_repo(&project_root, &path_info, "/git-upload-pack")?;
            if !http_backend_repo_exported(&repo.git_dir) {
                return http_backend_status(&mut stdout, "404 Not Found");
            }
            http_backend_no_cache_headers(&mut stdout, "application/x-git-upload-pack-result")?;
            stdout.flush()?;
            let result = (|| {
                let stdin = io::stdin();
                let mut stdin =
                    io::BufReader::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdin.lock());
                let request = read_upload_pack_request_from_stdin(&mut stdin)?;
                if request.wants.is_empty() {
                    return Ok(());
                }
                upload_pack_respond_with_pack(&repo, request, true)
            })();
            return http_backend_child_result(result);
        }
        if method == "POST" && path_info.ends_with("/git-receive-pack") && query.is_empty() {
            let repo = http_backend_repo(&project_root, &path_info, "/git-receive-pack")?;
            if !http_backend_repo_exported(&repo.git_dir) {
                return http_backend_status(&mut stdout, "404 Not Found");
            }
            let runtime = primitive_runtime_for_repo(&repo);
            let refs = runtime.refs_store_adapter();
            http_backend_no_cache_headers(&mut stdout, "application/x-git-receive-pack-result")?;
            stdout.flush()?;
            let stdin = io::stdin();
            let mut stdin = io::BufReader::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdin.lock());
            return receive_pack_apply_request(&refs, &mut stdin, &mut stdout);
        }
        return http_backend_status(&mut stdout, "405 Method Not Allowed");
    }
    if path_info.ends_with("/info/refs") && query == "service=git-upload-pack" {
        let repo = http_backend_repo(&project_root, &path_info, "/info/refs")?;
        if !http_backend_repo_exported(&repo.git_dir) {
            return http_backend_status(&mut stdout, "404 Not Found");
        }
        let runtime = primitive_runtime_for_repo(&repo);
        let refs = runtime.refs_store_adapter();
        http_backend_no_cache_headers(&mut stdout, "application/x-git-upload-pack-advertisement")?;
        write_pkt_line(&mut stdout, b"# service=git-upload-pack\n")?;
        stdout.write_all(b"0000")?;
        write_upload_pack_advertisement_from_adapter(&refs, &mut stdout)?;
        stdout.flush()?;
        return Ok(());
    }
    if path_info.ends_with("/info/refs") && query == "service=git-receive-pack" {
        let repo = http_backend_repo(&project_root, &path_info, "/info/refs")?;
        if !http_backend_repo_exported(&repo.git_dir) {
            return http_backend_status(&mut stdout, "404 Not Found");
        }
        let runtime = primitive_runtime_for_repo(&repo);
        let refs = runtime.refs_store_adapter();
        http_backend_no_cache_headers(&mut stdout, "application/x-git-receive-pack-advertisement")?;
        write_pkt_line(&mut stdout, b"# service=git-receive-pack\n")?;
        stdout.write_all(b"0000")?;
        write_receive_pack_advertisement_from_adapter(&refs, &mut stdout)?;
        stdout.flush()?;
        return Ok(());
    }
    if path_info.ends_with("/info/refs") && query.is_empty() {
        let repo = http_backend_repo(&project_root, &path_info, "/info/refs")?;
        if !http_backend_repo_exported(&repo.git_dir) {
            return http_backend_status(&mut stdout, "404 Not Found");
        }
        let runtime = primitive_runtime_for_repo(&repo);
        let refs = runtime.refs_store_adapter();
        http_backend_no_cache_headers(&mut stdout, "text/plain; charset=utf-8")?;
        for (name, id) in refs
            .server_info_refs()
            .map_err(|error| CliError::Io(io::Error::other(error.to_string())))?
        {
            writeln!(stdout, "{}\t{}", id, name)?;
        }
        stdout.flush()?;
        return Ok(());
    }
    http_backend_status(&mut stdout, "404 Not Found")
}

fn http_backend_child_result(result: Result<()>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(CliError::Fatal { code: 128, message }) => Err(CliError::Stderr {
            code: 1,
            text: format!("fatal: {message}\n"),
        }),
        Err(error) => Err(error),
    }
}

fn http_backend_repo(project_root: &str, path_info: &str, suffix: &str) -> Result<GitRepo> {
    let repo_path = path_info
        .strip_suffix(suffix)
        .ok_or_else(|| CliError::Fatal {
            code: 1,
            message: "invalid git http path".into(),
        })?
        .trim_start_matches('/');
    if repo_path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(CliError::Fatal {
            code: 1,
            message: "invalid git http repository path".into(),
        });
    }
    let path = PathBuf::from(project_root).join(repo_path);
    upload_pack_repo_from_path(&path, true)
}

fn http_backend_repo_exported(git_dir: &std::path::Path) -> bool {
    std::env::var_os("GIT_HTTP_EXPORT_ALL").is_some()
        || git_dir.join("git-daemon-export-ok").is_file()
}

fn http_backend_no_cache_headers<W: Write>(out: &mut W, content_type: &str) -> Result<()> {
    write!(
        out,
        "Expires: Fri, 01 Jan 1980 00:00:00 GMT\r\n\
         Pragma: no-cache\r\n\
         Cache-Control: no-cache, max-age=0, must-revalidate\r\n\
         Content-Type: {content_type}\r\n\
         \r\n"
    )?;
    Ok(())
}

fn http_backend_status<W: Write>(out: &mut W, status: &str) -> Result<()> {
    write!(
        out,
        "Status: {status}\r\n\
         Expires: Fri, 01 Jan 1980 00:00:00 GMT\r\n\
         Pragma: no-cache\r\n\
         Cache-Control: no-cache, max-age=0, must-revalidate\r\n\
         \r\n"
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
struct ReceivePackUpdate {
    old: ObjectId,
    new: Option<ObjectId>,
    ref_name: String,
}

pub(crate) fn receive_pack(_quiet: bool, directory: PathBuf) -> Result<()> {
    let repo = upload_pack_repo_from_path(&directory, true)?;
    let runtime = primitive_runtime_for_repo(&repo);
    let refs = runtime.refs_store_adapter();
    let stdout = io::stdout();
    let stdout = stdout.lock();
    let mut stdout = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdout);
    write_receive_pack_advertisement_from_adapter(&refs, &mut stdout)?;
    stdout.flush()?;

    let stdin = io::stdin();
    let mut stdin = io::BufReader::with_capacity(PACK_RECEIPT_BUF_CAPACITY, stdin.lock());
    receive_pack_apply_request(&refs, &mut stdin, &mut stdout)
}

pub(crate) fn receive_pack_apply_request<R: BufRead, W: Write>(
    refs: &OwnedCliRefsStoreAdapter,
    input: &mut R,
    out: &mut W,
) -> Result<()> {
    let (updates, report_status) = read_receive_pack_updates(input)?;

    let mut pack_path = None;
    if updates.iter().any(|update| update.new.is_some()) {
        let (temp_pack, file) = temp_http_pack_file(&repo_objects_dir(refs))?;
        let result = (|| {
            let mut file = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, file);
            copy_stream(input, &mut file)?;
            file.flush()?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temp_pack);
            return Err(error);
        }
        pack_path = Some(temp_pack);
    }
    if let Some(pack_path) = pack_path.as_deref() {
        let store = object_adapter_from_objects_dir(&repo_objects_dir(refs));
        let result = unpack_pack_file_to_loose(&store, GitHashAlgorithm::Sha1, pack_path);
        let _ = fs::remove_file(pack_path);
        result?;
    }
    for update in &updates {
        apply_receive_pack_update(&refs, update)?;
    }
    if report_status {
        write_pkt_line(out, b"unpack ok\n")?;
        for update in &updates {
            write_receive_pack_ok_pkt_line(out, &update.ref_name)?;
        }
        out.write_all(b"0000")?;
        out.flush()?;
    }
    Ok(())
}

pub(crate) fn process_receive_pack_request_from_reader(
    repo: &GitRepo,
    input: &mut dyn Read,
) -> Result<Vec<u8>> {
    let refs = OwnedCliRefsStoreAdapter::from_path(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut input = io::BufReader::with_capacity(PACK_RECEIPT_BUF_CAPACITY, input);
    let mut output = Vec::new();
    {
        let mut writer = io::BufWriter::with_capacity(PACK_RECEIPT_BUF_CAPACITY, &mut output);
        receive_pack_apply_request(&refs, &mut input, &mut writer)?;
        writer.flush()?;
    }
    Ok(output)
}

fn repo_objects_dir(refs: &OwnedCliRefsStoreAdapter) -> PathBuf {
    refs.objects_dir()
}

fn read_receive_pack_updates<R: BufRead>(input: &mut R) -> Result<(Vec<ReceivePackUpdate>, bool)> {
    let mut updates = Vec::with_capacity(RECEIVE_PACK_UPDATE_CAPACITY_HINT);
    let mut report_status = false;
    let mut line = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
    while read_pkt_line_payload_into(input, &mut line)? {
        let line = trim_lf_payload(&line);
        let (command, capabilities) = split_once_byte(line, b'\0').unwrap_or((line, b""));
        if ascii_tokens(capabilities).any(|capability| capability == b"report-status") {
            report_status = true;
        }
        updates.push(parse_receive_pack_update_bytes(command)?);
    }
    Ok((updates, report_status))
}

fn split_once_byte(line: &[u8], needle: u8) -> Option<(&[u8], &[u8])> {
    let idx = line.iter().position(|byte| *byte == needle)?;
    Some((&line[..idx], &line[idx + 1..]))
}

fn write_receive_pack_ok_pkt_line<W: Write>(out: &mut W, ref_name: &str) -> Result<()> {
    let payload_len = 3_usize
        .checked_add(ref_name.len())
        .and_then(|len| len.checked_add(1))
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "pkt-line length overflow".into(),
        })?;
    write_pkt_line_header(out, payload_len)?;
    out.write_all(b"ok ")?;
    out.write_all(ref_name.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn shell(command: Option<String>, args: Vec<String>) -> Result<()> {
    let Some(command) = command else {
        return Err(CliError::Fatal {
            code: 128,
            message: "Run with no arguments or with -c cmd".into(),
        });
    };
    let mut words = split_shell_words(&command)?;
    words.extend(args);
    let Some(program) = words.first().map(String::as_str) else {
        return Err(CliError::Fatal {
            code: 128,
            message: "Run with no arguments or with -c cmd".into(),
        });
    };
    match program {
        "git-upload-pack" => {
            let directory = shell_single_directory_arg(&words)?;
            upload_pack(UploadPackOptions {
                strict: false,
                no_strict: false,
                stateless_rpc: false,
                advertise_refs: false,
                timeout: None,
                directory,
            })
        }
        "git-receive-pack" => {
            let directory = shell_single_directory_arg(&words)?;
            receive_pack(false, directory)
        }
        "git-upload-archive" => {
            let directory = shell_single_directory_arg(&words)?;
            archive_commands::upload_archive(directory)
        }
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unrecognized command '{program}'"),
        }),
    }
}

fn shell_single_directory_arg(words: &[String]) -> Result<PathBuf> {
    if words.len() != 2 {
        return Err(CliError::Fatal {
            code: 128,
            message: "git shell command requires exactly one repository path".into(),
        });
    }
    Ok(PathBuf::from(&words[1]))
}

fn split_shell_words(input: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote = None;
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (None, '\'') => quote = Some('\''),
            (None, '"') => quote = Some('"'),
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (None, ch) if ch.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            (_, ch) => current.push(ch),
        }
    }
    if quote.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "unterminated quote in git shell command".into(),
        });
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

fn write_receive_pack_advertisement<W: Write>(refs: &RefStore, out: &mut W) -> Result<()> {
    let capabilities =
        "report-status delete-refs quiet ofs-delta object-format=sha1 agent=skron/0.1.0";
    let mut wrote = false;
    refs.for_each_server_info_ref(|id, name| {
        write_ref_advertisement_pkt_line(out, Some(id), name, (!wrote).then_some(capabilities))?;
        wrote = true;
        Ok::<(), CliError>(())
    })?;
    if !wrote {
        write_ref_advertisement_pkt_line(out, None, "capabilities^{}", Some(capabilities))?;
        out.write_all(b"0000")?;
        return Ok(());
    }
    out.write_all(b"0000")?;
    Ok(())
}

fn parse_receive_pack_update_bytes(command: &[u8]) -> Result<ReceivePackUpdate> {
    let mut parts = ascii_tokens(command);
    let old = parts.next().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "receive-pack update is missing old object id".into(),
    })?;
    let new = parts.next().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "receive-pack update is missing new object id".into(),
    })?;
    let ref_name = parts.next().ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "receive-pack update is missing ref name".into(),
    })?;
    if parts.next().is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "receive-pack update has extra fields".into(),
        });
    }
    let new = if is_zero_object_id_bytes(new) {
        None
    } else {
        Some(ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, new)?)
    };
    Ok(ReceivePackUpdate {
        old: ObjectId::from_hex_bytes(GitHashAlgorithm::Sha1, old)?,
        new,
        ref_name: ascii_token_as_str(ref_name)?.to_owned(),
    })
}

fn apply_receive_pack_update(
    refs: &OwnedCliRefsStoreAdapter,
    update: &ReceivePackUpdate,
) -> Result<()> {
    let action = match (is_zero_object_id_object(&update.old), update.new.as_ref()) {
        (true, Some(_)) => "create",
        (_, Some(_)) => "update",
        (true, None) | (false, None) => "delete",
    };
    let expected = match action {
        "create" => None,
        _ => Some(update.old.clone()),
    };
    let new_oid = update.new.as_ref();
    let current = match expected.as_ref() {
        None => None,
        Some(_) => refs.read_ref_oid(&update.ref_name)?,
    };
    if let Some(expected) = expected.as_ref() {
        if current != Some(expected.clone()) {
            return Err(CliError::Fatal {
                code: 1,
                message: format!(
                    "update rejected: current value for {} does not match expected old value",
                    update.ref_name
                ),
            });
        }
    }
    match action {
        "create" => {
            if refs.read_ref_oid(&update.ref_name)?.is_some() {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("cannot create existing ref {}", update.ref_name),
                });
            }
            let Some(new_id) = new_oid else {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("missing new value for create of {}", update.ref_name),
                });
            };
            if is_zero_object_id_object(&new_id) {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("new value cannot be zero for create of {}", update.ref_name),
                });
            }
            refs.as_ref_store().write_ref(&update.ref_name, new_id)?;
        }
        "update" => {
            let Some(new_id) = new_oid else {
                return Err(CliError::Fatal {
                    code: 1,
                    message: format!("missing new value for update of {}", update.ref_name),
                });
            };
            refs.as_ref_store().write_ref(&update.ref_name, new_id)?;
        }
        "delete" => {
            refs.validate_push_delete(&update.ref_name)?;
            refs.delete_ref(&update.ref_name)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn write_upload_pack_advertisement_from_adapter<W: Write>(
    refs: &OwnedCliRefsStoreAdapter,
    out: &mut W,
) -> Result<()> {
    let capabilities = upload_pack_capabilities_from_adapter(refs)?;
    let mut wrote = false;
    if let Some(head) = refs.resolve_ref("HEAD")? {
        write_ref_advertisement_pkt_line(out, Some(&head), "HEAD", Some(&capabilities))?;
        wrote = true;
    }
    refs.for_each_server_info_ref(|id, name| {
        write_ref_advertisement_pkt_line(
            out,
            Some(id),
            name,
            (!wrote).then_some(capabilities.as_str()),
        )?;
        wrote = true;
        Ok::<(), CliError>(())
    })?;
    if !wrote {
        write_ref_advertisement_pkt_line(out, None, "capabilities^{}", Some(&capabilities))?;
        out.write_all(b"0000")?;
        return Ok(());
    }
    out.write_all(b"0000")?;
    Ok(())
}

fn upload_pack_capabilities_from_adapter(refs: &OwnedCliRefsStoreAdapter) -> Result<String> {
    let mut capabilities = String::from(
        "multi_ack thin-pack side-band side-band-64k ofs-delta shallow filter \
         no-progress include-tag multi_ack_detailed no-done",
    );
    if let Some(target) = refs.head_symbolic_ref() {
        capabilities.push_str(" symref=HEAD:");
        capabilities.push_str(&target);
    }
    capabilities.push_str(" object-format=sha1 agent=skron/0.1.0");
    Ok(capabilities)
}

fn write_receive_pack_advertisement_from_adapter<W: Write>(
    refs: &OwnedCliRefsStoreAdapter,
    out: &mut W,
) -> Result<()> {
    write_receive_pack_advertisement_from_adapter_impl(refs, out)
}

fn write_receive_pack_advertisement_from_adapter_impl<W: Write>(
    refs: &OwnedCliRefsStoreAdapter,
    out: &mut W,
) -> Result<()> {
    let capabilities =
        "report-status delete-refs quiet ofs-delta object-format=sha1 agent=skron/0.1.0";
    let mut wrote = false;
    if let Some(id) = refs.resolve_ref("HEAD")? {
        write_ref_advertisement_pkt_line(out, Some(&id), "HEAD", Some(capabilities))?;
        wrote = true;
    }
    refs.for_each_server_info_ref(|id, name| {
        write_ref_advertisement_pkt_line(out, Some(id), name, (!wrote).then_some(capabilities))?;
        wrote = true;
        Ok::<(), CliError>(())
    })?;
    if !wrote {
        write_ref_advertisement_pkt_line(out, None, "capabilities^{}", Some(capabilities))?;
        out.write_all(b"0000")?;
        return Ok(());
    }
    out.write_all(b"0000")?;
    Ok(())
}

fn is_zero_object_id_bytes(value: &[u8]) -> bool {
    value.len() == GitHashAlgorithm::Sha1.digest_len() * 2 && value.iter().all(|byte| *byte == b'0')
}

fn is_zero_object_id_object(value: &ObjectId) -> bool {
    value.as_bytes().iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod transport_request_tests {
    use super::*;
    use std::ffi::{OsStr, OsString};

    fn oid(hex: &str) -> ObjectId {
        ObjectId::from_hex(GitHashAlgorithm::Sha1, hex).expect("object id")
    }

    fn test_repo_at(root: &std::path::Path) -> GitRepo {
        let git_dir = root.join(".git");
        let objects_dir = git_dir.join("objects");
        std::fs::create_dir_all(&objects_dir).expect("test repo objects");
        GitRepo {
            root: root.to_path_buf(),
            git_dir,
            objects_dir,
            index_path: root.join(".git").join("index"),
        }
    }

    #[test]
    fn transport_small_collection_capacity_hints_cover_expected_entries() {
        assert_eq!(TAG_PEEL_SEEN_CAPACITY_HINT, 4);
        assert_eq!(CLONE_CONFIG_VALUES_CAPACITY_HINT, 4);
    }

    #[test]
    fn copy_reachable_seen_initial_capacity_is_bounded_for_large_stores() {
        assert_eq!(copy_reachable_seen_initial_capacity(usize::MAX, 1), 8192);
        assert_eq!(copy_reachable_seen_initial_capacity(2, 4), 4);
        assert_eq!(copy_reachable_seen_initial_capacity(0, 0), 1);
    }

    #[test]
    fn transport_history_collection_capacity_is_bounded() {
        assert_eq!(transport_history_collection_capacity(usize::MAX), 8192);
        assert_eq!(transport_history_collection_capacity(2), 2);
        assert_eq!(transport_history_collection_capacity(0), 0);
    }

    #[test]
    fn transport_history_reserve_helpers_do_not_grow_when_spare_capacity_is_enough() {
        let mut ids = Vec::with_capacity(4);
        ids.push(oid("1111111111111111111111111111111111111111"));
        let ids_capacity = ids.capacity();

        reserve_transport_history_vec(&mut ids, 2);

        assert_eq!(ids.capacity(), ids_capacity);

        let mut seen = HashSet::with_capacity(4);
        seen.insert(oid("2222222222222222222222222222222222222222"));
        let seen_capacity = seen.capacity();

        reserve_transport_history_set(&mut seen, 2);

        assert_eq!(seen.capacity(), seen_capacity);

        let mut pending = VecDeque::with_capacity(4);
        pending.push_back(oid("3333333333333333333333333333333333333333"));
        let pending_capacity = pending.capacity();

        reserve_transport_history_queue(&mut pending, 2);

        assert_eq!(pending.capacity(), pending_capacity);
    }

    #[test]
    fn transport_history_queue_reserve_is_bounded() {
        let mut pending = VecDeque::<ObjectId>::new();

        reserve_transport_history_queue(&mut pending, usize::MAX);

        assert_eq!(
            pending.capacity(),
            TRANSPORT_HISTORY_COLLECTION_CAPACITY_LIMIT
        );
    }

    #[test]
    fn transport_ref_collection_capacity_is_bounded() {
        assert_eq!(transport_ref_collection_capacity(usize::MAX), 8192);
        assert_eq!(transport_ref_collection_capacity(2), 2);
        assert_eq!(transport_ref_collection_capacity(0), 0);
    }

    #[test]
    fn http_fetch_root_initial_capacity_covers_stdin_and_positional_roots() {
        assert_eq!(http_fetch_root_initial_capacity(false, false), 0);
        assert_eq!(http_fetch_root_initial_capacity(false, true), 1);
        assert_eq!(
            http_fetch_root_initial_capacity(true, false),
            HTTP_REMOTE_REF_ROWS_CAPACITY_HINT
        );
        assert_eq!(
            http_fetch_root_initial_capacity(true, true),
            HTTP_REMOTE_REF_ROWS_CAPACITY_HINT + 1
        );
    }

    #[test]
    fn object_id_order_tracks_sorted_unique_duplicate_and_unsorted_inputs() {
        let first = oid("1111111111111111111111111111111111111111");
        let second = oid("2222222222222222222222222222222222222222");

        assert_eq!(object_id_order(&[]), ObjectIdOrder::SortedUnique);
        assert_eq!(
            object_id_order(std::slice::from_ref(&first)),
            ObjectIdOrder::SortedUnique
        );
        assert_eq!(
            object_id_order(&[first.clone(), second.clone()]),
            ObjectIdOrder::SortedUnique
        );
        assert_eq!(
            object_id_order(&[first.clone(), first.clone(), second.clone()]),
            ObjectIdOrder::SortedWithDuplicates
        );
        assert_eq!(object_id_order(&[second, first]), ObjectIdOrder::Unsorted);
    }

    #[test]
    fn sort_dedup_object_ids_returns_early_for_sorted_unique_input() {
        let first = oid("1111111111111111111111111111111111111111");
        let second = oid("2222222222222222222222222222222222222222");
        let mut ids = vec![first.clone(), second.clone()];

        sort_dedup_object_ids(&mut ids);

        assert_eq!(ids, vec![first, second]);
    }

    #[test]
    fn sort_dedup_object_ids_dedupes_sorted_input() {
        let first = oid("1111111111111111111111111111111111111111");
        let second = oid("2222222222222222222222222222222222222222");
        let third = oid("3333333333333333333333333333333333333333");
        let mut ids = vec![first.clone(), first.clone(), second.clone(), third.clone()];

        sort_dedup_object_ids(&mut ids);

        assert_eq!(ids, vec![first, second, third]);
    }

    #[test]
    fn sort_dedup_object_ids_sorts_unsorted_input() {
        let first = oid("1111111111111111111111111111111111111111");
        let second = oid("2222222222222222222222222222222222222222");
        let mut ids = vec![second.clone(), first.clone(), second.clone()];

        sort_dedup_object_ids(&mut ids);

        assert_eq!(ids, vec![first, second]);
    }

    #[test]
    fn peeled_tag_ref_matcher_avoids_allocated_candidate_names() {
        assert!(is_peeled_tag_ref("refs/tags/v1^{}", "refs/tags/v1"));
        assert!(!is_peeled_tag_ref("refs/tags/v1", "refs/tags/v1"));
        assert!(!is_peeled_tag_ref("refs/tags/v10^{}", "refs/tags/v1"));
        assert!(!is_peeled_tag_ref("refs/tags/v1^{}extra", "refs/tags/v1"));
    }

    #[test]
    fn http_clone_target_prefers_peeled_tag_without_allocating_per_row_name() {
        let tag_object = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let peeled = oid("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        let refs = vec![
            LsRemoteRow {
                id: tag_object,
                name: "refs/tags/v1".into(),
            },
            LsRemoteRow {
                id: peeled.clone(),
                name: "refs/tags/v1^{}".into(),
            },
        ];

        let target = http_clone_target(&refs, Some("v1"), None).expect("clone target");

        assert!(matches!(target, CloneTarget::Tag { id, .. } if id == peeled));
    }

    #[test]
    fn http_clone_fetch_roots_filters_and_deduplicates_refs() {
        let head = oid("1111111111111111111111111111111111111111");
        let branch = oid("2222222222222222222222222222222222222222");
        let tag = oid("3333333333333333333333333333333333333333");
        let peeled = oid("4444444444444444444444444444444444444444");
        let refs = vec![
            LsRemoteRow {
                id: head,
                name: "HEAD".into(),
            },
            LsRemoteRow {
                id: branch.clone(),
                name: "refs/heads/main".into(),
            },
            LsRemoteRow {
                id: branch.clone(),
                name: "refs/heads/duplicate".into(),
            },
            LsRemoteRow {
                id: tag,
                name: "refs/tags/v1".into(),
            },
            LsRemoteRow {
                id: peeled,
                name: "refs/tags/v1^{}".into(),
            },
        ];

        let roots = http_clone_fetch_roots(&refs, &CloneTarget::Empty, true, false, false);

        assert_eq!(roots, vec![branch]);
    }

    #[test]
    fn upload_pack_request_matches_git_pkt_line_shape() {
        let first = oid("1111111111111111111111111111111111111111");
        let second = oid("2222222222222222222222222222222222222222");
        let actual = build_upload_pack_request(&[first.clone(), second.clone()], Some(12))
            .expect("upload-pack request");
        let mut expected = Vec::new();
        write_pkt_line(
            &mut expected,
            format!(
                "want {} side-band-64k ofs-delta include-tag\n",
                first.to_hex()
            )
            .as_bytes(),
        )
        .expect("first want");
        write_pkt_line(
            &mut expected,
            format!("want {}\n", second.to_hex()).as_bytes(),
        )
        .expect("second want");
        write_pkt_line(&mut expected, b"deepen 12\n").expect("deepen");
        expected.extend_from_slice(b"0000");
        write_pkt_line(&mut expected, b"done\n").expect("done");
        assert_eq!(actual, expected);
    }

    #[test]
    fn object_id_stdin_reader_streams_first_tokens_in_order() {
        let first = "1111111111111111111111111111111111111111";
        let second = "2222222222222222222222222222222222222222";
        let mut reader =
            io::Cursor::new(format!("\n {first} first\n{second}\tsecond\n").into_bytes());
        let mut ids = Vec::new();

        collect_first_token_object_ids_from_reader(&mut reader, &mut ids).expect("object ids");

        assert_eq!(ids, vec![oid(first), oid(second)]);
    }

    #[test]
    fn object_id_stdin_reader_rejects_unbounded_lines() {
        let mut input = Vec::new();
        input.resize(TRANSPORT_TEXT_LINE_LIMIT + 1, b'1');
        input.push(b'\n');
        let mut reader = io::Cursor::new(input);
        let mut ids = Vec::new();

        let error = collect_first_token_object_ids_from_reader(&mut reader, &mut ids)
            .expect_err("oversized object id input line");

        match error {
            CliError::Io(error) => {
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert_eq!(error.to_string(), "transport text line too long");
            }
            other => panic!("expected io error, got {other:?}"),
        }
    }

    #[test]
    fn trimmed_line_reader_streams_non_empty_lines() {
        let mut reader = io::Cursor::new(b"\n refs/heads/main \n+topic:topic\n\n".to_vec());
        let mut lines = vec!["existing".to_owned()];

        collect_trimmed_lines_from_reader(&mut reader, &mut lines).expect("lines");

        assert_eq!(
            lines,
            vec![
                "existing".to_owned(),
                "refs/heads/main".to_owned(),
                "+topic:topic".to_owned(),
            ]
        );
    }

    #[test]
    fn trimmed_line_reader_rejects_unbounded_lines() {
        let mut input = Vec::new();
        input.resize(TRANSPORT_TEXT_LINE_LIMIT + 1, b'a');
        input.push(b'\n');
        let mut reader = io::Cursor::new(input);
        let mut lines = Vec::new();

        let error = collect_trimmed_lines_from_reader(&mut reader, &mut lines)
            .expect_err("oversized transport text line");

        match error {
            CliError::Io(error) => {
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert_eq!(error.to_string(), "transport text line too long");
            }
            other => panic!("expected io error, got {other:?}"),
        }
    }

    #[test]
    fn transport_line_reader_reuses_caller_string_buffer() {
        let mut reader = io::Cursor::new(b"HTTP/1.1 200 OK\r\n".to_vec());
        let mut line = String::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);
        let capacity = line.capacity();

        let len = read_limited_transport_line(
            &mut reader,
            &mut line,
            HTTP_RESPONSE_LINE_LIMIT,
            "HTTP response line too long",
        )
        .expect("line");

        assert_eq!(len, "HTTP/1.1 200 OK\r\n".len());
        assert_eq!(line, "HTTP/1.1 200 OK\r\n");
        assert_eq!(line.capacity(), capacity);
    }

    #[test]
    fn transport_line_reader_clears_buffer_after_invalid_utf8() {
        let mut reader = io::Cursor::new([0xff, b'\n']);
        let mut line = String::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);

        let error = read_limited_transport_line(
            &mut reader,
            &mut line,
            HTTP_RESPONSE_LINE_LIMIT,
            "HTTP response line too long",
        )
        .expect_err("invalid utf8");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "HTTP response line is not UTF-8");
        assert!(line.is_empty());
    }

    #[test]
    fn http_helper_line_reader_rejects_unbounded_lines() {
        let mut input = Vec::new();
        input.resize(TRANSPORT_TEXT_LINE_LIMIT + 1, b'a');
        input.push(b'\n');
        let mut reader = io::Cursor::new(input);
        let mut line = String::new();

        let error = read_helper_line(&mut reader, &mut line).expect_err("oversized helper line");

        match error {
            CliError::Io(error) => {
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert_eq!(error.to_string(), "transport text line too long");
            }
            other => panic!("expected io error, got {other:?}"),
        }
    }

    #[test]
    fn upload_pack_exclude_ids_borrows_haves_until_shallows_are_needed() {
        let have = oid("1111111111111111111111111111111111111111");
        let shallow = oid("2222222222222222222222222222222222222222");
        let request = UploadPackRequest {
            haves: vec![have.clone()],
            shallows: vec![shallow.clone()],
            ..UploadPackRequest::default()
        };

        let borrowed = upload_pack_exclude_ids(&request, false);
        assert!(matches!(borrowed, Cow::Borrowed(_)));
        assert_eq!(borrowed.as_ref(), std::slice::from_ref(&have));

        let owned = upload_pack_exclude_ids(&request, true);
        assert!(matches!(owned, Cow::Owned(_)));
        assert_eq!(owned.as_ref(), &[have, shallow]);
    }

    #[test]
    fn head_symref_branch_parser_uses_byte_tokens() {
        let branch = head_symref_branch_from_capabilities(
            b"multi_ack symref=HEAD:refs/heads/main ofs-delta\n",
        );

        assert_eq!(branch.as_deref(), Some("main"));
        assert!(head_symref_branch_from_capabilities(b"multi_ack ofs-delta\n").is_none());
    }

    #[test]
    fn upload_pack_shallow_id_parser_uses_bytes() {
        let id = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

        assert_eq!(
            upload_pack_shallow_id_from_payload(
                b"shallow aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"
            ),
            Some(id.as_slice())
        );
        assert!(
            upload_pack_shallow_id_from_payload(b"ACK aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n")
                .is_none()
        );
    }

    #[test]
    fn upload_pack_sideband_response_parses_shallow_ids_as_bytes() {
        let shallow = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let dir = tempfile::tempdir().expect("tempdir");
        let pack_path = dir.path().join("pack.tmp");
        fs::File::create(&pack_path).expect("create pack placeholder");
        let mut body = Vec::new();
        write_pkt_line(
            &mut body,
            format!("shallow {}\n", shallow.to_hex()).as_bytes(),
        )
        .expect("shallow");
        write_pkt_line(&mut body, b"NAK\n").expect("nak");
        write_sideband_pack(&mut body, b"PACK-body").expect("pack");
        let mut reader = io::Cursor::new(body);

        let shallow_boundaries =
            parse_upload_pack_sideband_response_to_file(&mut reader, &pack_path, 1)
                .expect("parse sideband")
                .expect("shallow boundaries");

        assert_eq!(shallow_boundaries, vec![shallow]);
        assert_eq!(fs::read(pack_path).expect("pack file"), b"PACK-body");
    }

    #[test]
    fn upload_pack_sideband_pack_parser_reuses_open_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack_path = dir.path().join("pack.tmp");
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pack_path)
            .expect("create temp pack");
        let mut body = Vec::new();
        write_pkt_line(&mut body, b"NAK\n").expect("nak");
        write_sideband_pack(&mut body, b"PACK-body").expect("pack");
        let mut reader = io::Cursor::new(body);

        let parsed = parse_upload_pack_sideband_pack_to_open_file(&mut reader, &pack_path, file)
            .expect("parse sideband pack")
            .expect("pack path");

        assert_eq!(parsed, pack_path);
        assert_eq!(fs::read(pack_path).expect("pack file"), b"PACK-body");
    }

    #[test]
    fn upload_pack_sideband_response_parser_reuses_open_temp_file() {
        let shallow = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let dir = tempfile::tempdir().expect("tempdir");
        let pack_path = dir.path().join("pack.tmp");
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pack_path)
            .expect("create temp pack");
        let mut body = Vec::new();
        write_pkt_line(
            &mut body,
            format!("shallow {}\n", shallow.to_hex()).as_bytes(),
        )
        .expect("shallow");
        write_pkt_line(&mut body, b"NAK\n").expect("nak");
        write_sideband_pack(&mut body, b"PACK-body").expect("pack");
        let mut reader = io::Cursor::new(body);

        let shallow_boundaries =
            parse_upload_pack_sideband_response_to_open_file(&mut reader, file, 1)
                .expect("parse sideband")
                .expect("shallow boundaries");

        assert_eq!(shallow_boundaries, vec![shallow]);
        assert_eq!(fs::read(pack_path).expect("pack file"), b"PACK-body");
    }

    #[test]
    fn receive_pack_request_matches_git_pkt_line_shape() {
        let first = PushRef {
            id: Some(oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
            destination: "refs/heads/main".to_owned(),
            source_display: None,
            force: false,
        };
        let second = PushRef {
            id: None,
            destination: "refs/heads/old".to_owned(),
            source_display: None,
            force: false,
        };
        let actual = build_receive_pack_request_commands(&[
            (
                first.clone(),
                Some(oid("1111111111111111111111111111111111111111")),
            ),
            (
                second.clone(),
                Some(oid("2222222222222222222222222222222222222222")),
            ),
        ])
        .expect("receive-pack request");
        let mut expected = Vec::new();
        write_pkt_line(
            &mut expected,
            format!(
                "1111111111111111111111111111111111111111 {} {}\0report-status ofs-delta\n",
                first.id.as_ref().expect("new id").to_hex(),
                first.destination
            )
            .as_bytes(),
        )
        .expect("first update");
        write_pkt_line(
            &mut expected,
            format!(
                "2222222222222222222222222222222222222222 {} {}\n",
                "0".repeat(GitHashAlgorithm::Sha1.digest_len() * 2),
                second.destination
            )
            .as_bytes(),
        )
        .expect("second update");
        expected.extend_from_slice(b"0000");
        assert_eq!(actual, expected);
    }

    #[test]
    fn receive_pack_update_reader_uses_capacity_hint() {
        let old = oid("1111111111111111111111111111111111111111");
        let new = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let mut input = Vec::new();
        write_pkt_line(
            &mut input,
            format!(
                "{} {} refs/heads/main\0report-status ofs-delta\n",
                old.to_hex(),
                new.to_hex()
            )
            .as_bytes(),
        )
        .expect("update");
        input.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(io::Cursor::new(input));

        let (updates, report_status) =
            read_receive_pack_updates(&mut reader).expect("receive-pack updates");

        assert!(report_status);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates.capacity(), RECEIVE_PACK_UPDATE_CAPACITY_HINT);
        assert_eq!(updates[0].old, old);
        assert_eq!(updates[0].new.as_ref(), Some(&new));
        assert_eq!(updates[0].ref_name, "refs/heads/main");
    }

    #[test]
    fn ref_advertisement_writer_matches_pkt_line_shape() {
        let id = oid("3333333333333333333333333333333333333333");
        let mut actual = Vec::new();
        write_ref_advertisement_pkt_line(
            &mut actual,
            Some(&id),
            "refs/heads/main",
            Some("multi_ack ofs-delta"),
        )
        .expect("ref advertisement");

        let mut expected = Vec::new();
        write_pkt_line(
            &mut expected,
            format!("{} refs/heads/main\0multi_ack ofs-delta\n", id.to_hex()).as_bytes(),
        )
        .expect("expected advertisement");
        assert_eq!(actual, expected);
    }

    #[test]
    fn streamed_http_loose_blob_body_matches_git_framing() {
        let repo = tempfile::TempDir::new().expect("repo");
        let store = LooseObjectStore::new(repo.path().join("objects"), GitHashAlgorithm::Sha1);
        let content = b"large blob body without buffering\n";
        let id = store
            .write_object(GitObjectKind::Blob, content)
            .expect("write blob");
        let temp_path = temp_http_helper_body_path().expect("temp body path");

        write_compressed_streamable_blob_body(&temp_path, &store, &id, content.len())
            .expect("write streamed HTTP body");

        let encoded = fs::read(&temp_path).expect("read encoded body");
        fs::remove_file(&temp_path).expect("remove temp body");
        let mut decoded = Vec::new();
        ZlibDecoder::new(encoded.as_slice())
            .read_to_end(&mut decoded)
            .expect("decode streamed HTTP body");
        let mut expected = format!("blob {}\0", content.len()).into_bytes();
        expected.extend_from_slice(content);
        assert_eq!(decoded, expected);
    }

    #[test]
    fn temp_http_helper_body_path_allocates_unique_existing_files() {
        let first = temp_http_helper_body_path().expect("first temp body path");
        let second = temp_http_helper_body_path().expect("second temp body path");

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        fs::remove_file(first).expect("remove first temp body");
        fs::remove_file(second).expect("remove second temp body");
    }

    #[test]
    fn temp_http_helper_output_path_allocates_unique_uncreated_paths() {
        let first = temp_http_helper_output_path().expect("first temp output path");
        let second = temp_http_helper_output_path().expect("second temp output path");

        assert_ne!(first, second);
        assert!(!first.exists());
        assert!(!second.exists());
    }

    #[test]
    fn sparse_oid_patterns_cache_loads_each_blobish_once() {
        let mut cache = HashMap::new();
        let loads = std::cell::Cell::new(0);

        let first = upload_pack_sparse_oid_patterns_cached(&mut cache, "spec", || {
            loads.set(loads.get() + 1);
            Ok(vec![b"keep/".to_vec()])
        })
        .expect("first cached sparse patterns")
        .to_vec();
        let second = upload_pack_sparse_oid_patterns_cached(&mut cache, "spec", || {
            loads.set(loads.get() + 1);
            Ok(vec![b"drop/".to_vec()])
        })
        .expect("second cached sparse patterns")
        .to_vec();

        assert_eq!(first, vec![b"keep/".to_vec()]);
        assert_eq!(second, first);
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn upload_pack_depth_roots_borrow_request_ids_without_clone() {
        let want = oid("1111111111111111111111111111111111111111");
        let shallow = oid("2222222222222222222222222222222222222222");
        let request = UploadPackRequest {
            wants: vec![want],
            shallows: vec![shallow],
            deepen_relative: true,
            ..UploadPackRequest::default()
        };

        let (roots, depth) = upload_pack_depth_roots(&request, usize::MAX);

        assert_eq!(roots, request.shallows.as_slice());
        assert!(std::ptr::eq(roots.as_ptr(), request.shallows.as_ptr()));
        assert_eq!(depth, usize::MAX);
    }

    #[test]
    fn upload_pack_depth_roots_borrow_wants_for_absolute_depth() {
        let want = oid("3333333333333333333333333333333333333333");
        let shallow = oid("4444444444444444444444444444444444444444");
        let request = UploadPackRequest {
            wants: vec![want],
            shallows: vec![shallow],
            deepen_relative: false,
            ..UploadPackRequest::default()
        };

        let (roots, depth) = upload_pack_depth_roots(&request, 3);

        assert_eq!(roots, request.wants.as_slice());
        assert!(std::ptr::eq(roots.as_ptr(), request.wants.as_ptr()));
        assert_eq!(depth, 3);
    }

    #[test]
    fn upload_pack_request_parser_uses_capacity_hints_for_growing_lists() {
        let want = oid("1111111111111111111111111111111111111111");
        let have = oid("2222222222222222222222222222222222222222");
        let shallow = oid("3333333333333333333333333333333333333333");
        let mut input = Vec::new();
        write_pkt_line(
            &mut input,
            format!("want {} side-band-64k\n", want.to_hex()).as_bytes(),
        )
        .expect("want");
        write_pkt_line(&mut input, format!("have {}\n", have.to_hex()).as_bytes()).expect("have");
        write_pkt_line(
            &mut input,
            format!("shallow {}\n", shallow.to_hex()).as_bytes(),
        )
        .expect("shallow");
        write_pkt_line(&mut input, b"deepen-not refs/heads/old\n").expect("deepen not");
        write_pkt_line(&mut input, b"done\n").expect("done");
        let mut reader = io::BufReader::new(io::Cursor::new(input));

        let request =
            read_upload_pack_request_from_stdin(&mut reader).expect("upload-pack request");

        assert_eq!(request.wants, vec![want]);
        assert_eq!(request.haves, vec![have]);
        assert_eq!(request.shallows, vec![shallow]);
        assert_eq!(request.deepen_not, vec!["refs/heads/old"]);
        assert!(request.side_band);
        assert!(request.wants.capacity() >= UPLOAD_PACK_WANT_CAPACITY_HINT);
        assert!(request.haves.capacity() >= UPLOAD_PACK_HAVE_CAPACITY_HINT);
        assert!(request.shallows.capacity() >= UPLOAD_PACK_SHALLOW_CAPACITY_HINT);
        assert!(request.deepen_not.capacity() >= UPLOAD_PACK_DEEPEN_NOT_CAPACITY_HINT);
    }

    #[test]
    fn upload_pack_pack_ids_capacity_hint_is_bounded_for_large_histories() {
        assert_eq!(upload_pack_pack_ids_capacity_hint(2, 3), 5);
        assert_eq!(
            upload_pack_pack_ids_capacity_hint(2, UPLOAD_PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT + 10),
            2 + UPLOAD_PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT
        );
        assert_eq!(
            upload_pack_pack_ids_capacity_hint(usize::MAX, usize::MAX),
            UPLOAD_PACK_BASE_ID_INITIAL_CAPACITY_LIMIT
                + UPLOAD_PACK_OBJECT_ID_GROWTH_CAPACITY_LIMIT
        );
    }

    #[test]
    fn upload_pack_filter_path_requirement_is_only_for_path_sensitive_filters() {
        assert!(!upload_pack_filter_needs_path(
            &parse_upload_pack_filter("blob:none").expect("blob none")
        ));
        assert!(!upload_pack_filter_needs_path(
            &parse_upload_pack_filter("blob:limit=1k").expect("blob limit")
        ));
        assert!(!upload_pack_filter_needs_path(
            &parse_upload_pack_filter("object:type=blob").expect("object type")
        ));
        assert!(upload_pack_filter_needs_path(
            &parse_upload_pack_filter("tree:1").expect("tree depth")
        ));
        assert!(upload_pack_filter_needs_path(
            &parse_upload_pack_filter("sparse:oid=HEAD:patterns").expect("sparse oid")
        ));
        assert!(upload_pack_filter_needs_path(
            &parse_upload_pack_filter("combine:blob%3Anone+tree%3A1").expect("combined filter")
        ));
    }

    #[test]
    fn upload_pack_sparse_filter_cache_capacity_counts_sparse_filters() {
        assert_eq!(
            upload_pack_sparse_filter_cache_capacity(
                &parse_upload_pack_filter("blob:none").expect("blob none")
            ),
            0
        );
        assert_eq!(
            upload_pack_sparse_filter_cache_capacity(
                &parse_upload_pack_filter("sparse:oid=HEAD:patterns").expect("sparse oid")
            ),
            1
        );
        assert_eq!(
            upload_pack_sparse_filter_cache_capacity(
                &parse_upload_pack_filter(
                    "combine:sparse%3Aoid%3DHEAD%3Aone+blob%3Anone+sparse%3Aoid%3DHEAD%3Atwo"
                )
                .expect("combined sparse filters")
            ),
            2
        );
    }

    #[test]
    fn upload_pack_tree_depth_filter_handles_raw_path_bytes() {
        let repo = tempfile::TempDir::new().expect("repo");
        let store = LooseObjectStore::new(repo.path().join("objects"), GitHashAlgorithm::Sha1);
        let filter = UploadPackFilter::TreeDepth(2);
        let mut sparse_patterns_cache = HashMap::new();

        assert!(
            upload_pack_filter_excludes_object(
                &GitRepo {
                    root: repo.path().to_path_buf(),
                    index_path: repo.path().join("index"),
                    objects_dir: repo.path().join("objects"),
                    git_dir: repo.path().to_path_buf(),
                },
                &store,
                &filter,
                &mut sparse_patterns_cache,
                GitObjectKind::Blob,
                0,
                Some(b"dir/\xff.bin"),
            )
            .expect("tree depth filter")
        );
    }

    #[test]
    fn sparse_filter_plain_prefix_matches_path_components_without_concat() {
        assert!(sparse_filter_pattern_matches(b"dir", b"dir"));
        assert!(sparse_filter_pattern_matches(b"dir/file.txt", b"dir"));
        assert!(sparse_filter_pattern_matches(b"dir/file.txt", b"dir/"));
        assert!(!sparse_filter_pattern_matches(b"dirname/file.txt", b"dir"));
        assert!(!sparse_filter_pattern_matches(b"dirish", b"dir/"));
    }

    #[test]
    fn parsed_http_url_writes_full_url_without_changing_suffix_rules() {
        let url = ParsedHttpUrl::parse("https://example.test:8443/repo.git").expect("parsed URL");
        let mut direct = Vec::new();

        url.write_full_url_with_suffix(&mut direct, "info/refs?service=git-upload-pack")
            .expect("write URL");

        assert_eq!(
            direct,
            b"https://example.test:8443/repo.git/info/refs?service=git-upload-pack"
        );
    }

    #[test]
    fn parsed_http_url_strips_userinfo_and_builds_basic_auth_header() {
        let url =
            ParsedHttpUrl::parse("https://user:p%40ss@example.test/repo.git").expect("parsed URL");
        let mut full = Vec::new();
        let mut request = Vec::new();

        url.write_full_url_with_suffix(&mut full, "info/refs")
            .expect("write full URL");
        write_http_request_parts(
            &mut request,
            &url,
            "GET",
            "info/refs",
            &[],
            &PackBody::Empty,
        )
        .expect("write request");

        assert_eq!(url.host, "example.test");
        assert_eq!(full, b"https://example.test/repo.git/info/refs");
        assert_eq!(
            request,
            b"GET /repo.git/info/refs HTTP/1.1\r\nHost: example.test\r\nAuthorization: Basic dXNlcjpwQHNz\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn parsed_http_url_treats_username_only_as_empty_password() {
        let url = ParsedHttpUrl::parse("http://user@example.test/repo.git").expect("parsed URL");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjo="));
    }

    #[test]
    fn http_config_path_match_stays_inside_path_boundary() {
        assert!(http_config_path_matches_url("/repo.git", "/repo.git"));
        assert!(http_config_path_matches_url(
            "/repo.git",
            "/repo.git/info/refs"
        ));
        assert!(!http_config_path_matches_url(
            "/repo.git",
            "/repo.git-private"
        ));
    }

    #[test]
    fn parsed_http_url_reads_configured_ssl_verify_false() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        std::fs::write(repo.git_dir.join("config"), "[http]\n\tsslVerify = false\n")
            .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with config");

        assert!(url.tls_no_verify);
    }

    #[test]
    fn parsed_http_url_scopes_ssl_verify_to_matching_http_url() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        std::fs::write(
            repo.git_dir.join("config"),
            "[http \"https://example.test/repo.git\"]\n\tsslVerify = false\n",
        )
        .expect("config");

        let child =
            parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git/info")
                .expect("matching URL");
        let sibling = parsed_http_url_with_extra_headers(
            Some(&repo),
            "https://example.test/repo.git-private",
        )
        .expect("non-matching URL");

        assert!(child.tls_no_verify);
        assert!(!sibling.tls_no_verify);
    }

    #[test]
    fn parsed_http_url_reads_configured_tls_identity_paths() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let ca = dir.path().join("ca.pem");
        let cert = dir.path().join("client.pem");
        let key = dir.path().join("client.key");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[http]\n\tsslCAInfo = {}\n\tsslCert = {}\n\tsslKey = {}\n",
                ca.display(),
                cert.display(),
                key.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with config");

        assert_eq!(url.ca_file.as_deref(), Some(ca.as_path()));
        assert_eq!(url.client_cert_file.as_deref(), Some(cert.as_path()));
        assert_eq!(url.client_key_file.as_deref(), Some(key.as_path()));
    }

    #[test]
    fn parsed_http_url_scopes_tls_identity_paths_to_matching_http_url() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let ca = dir.path().join("ca.pem");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[http \"https://example.test/repo.git\"]\n\tsslCAInfo = {}\n",
                ca.display()
            ),
        )
        .expect("config");

        let child =
            parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git/info")
                .expect("matching URL");
        let sibling = parsed_http_url_with_extra_headers(
            Some(&repo),
            "https://example.test/repo.git-private",
        )
        .expect("non-matching URL");

        assert_eq!(child.ca_file.as_deref(), Some(ca.as_path()));
        assert_eq!(sibling.ca_file, None);
    }

    #[test]
    fn parsed_http_url_reads_configured_proxy() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        std::fs::write(
            repo.git_dir.join("config"),
            "[http]\n\tproxy = http://proxy.example:8080\n",
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with config");

        assert_eq!(url.proxy.as_deref(), Some("http://proxy.example:8080"));
    }

    #[test]
    fn parsed_http_url_scopes_proxy_to_matching_http_url() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        std::fs::write(
            repo.git_dir.join("config"),
            "[http \"https://example.test/repo.git\"]\n\tproxy = http://proxy.example:8080\n",
        )
        .expect("config");

        let child =
            parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git/info")
                .expect("matching URL");
        let sibling = parsed_http_url_with_extra_headers(
            Some(&repo),
            "https://example.test/repo.git-private",
        )
        .expect("non-matching URL");

        assert_eq!(child.proxy.as_deref(), Some("http://proxy.example:8080"));
        assert_eq!(sibling.proxy, None);
    }

    #[test]
    fn parsed_http_url_reads_credential_store_helper_basic_auth() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials = dir.path().join("credentials");
        std::fs::write(&credentials, "https://user:p%40ss@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git")
            .expect("parsed URL with credential helper");

        assert_eq!(url.authorization.as_deref(), Some("Basic dXNlcjpwQHNz"));
    }

    #[test]
    fn parsed_http_url_keeps_url_userinfo_before_credential_store() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials = dir.path().join("credentials");
        std::fs::write(&credentials, "https://stored:secret@example.test\n").expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n",
                credentials.display()
            ),
        )
        .expect("config");

        let url = parsed_http_url_with_extra_headers(
            Some(&repo),
            "https://url-user:url-pass@example.test/repo.git",
        )
        .expect("parsed URL with credential helper");

        assert_eq!(
            url.authorization.as_deref(),
            Some("Basic dXJsLXVzZXI6dXJsLXBhc3M=")
        );
    }

    #[test]
    fn parsed_http_url_scopes_credential_store_username_to_matching_http_url() {
        let dir = tempfile::TempDir::new().expect("repo");
        let repo = test_repo_at(dir.path());
        let credentials = dir.path().join("credentials");
        std::fs::write(
            &credentials,
            "https://first:first-pass@example.test\nhttps://second:second-pass@example.test\n",
        )
        .expect("credentials");
        std::fs::write(
            repo.git_dir.join("config"),
            format!(
                "[credential]\n\thelper = store --file {}\n[credential \"https://example.test/repo.git\"]\n\tusername = first\n",
                credentials.display()
            ),
        )
        .expect("config");

        let child =
            parsed_http_url_with_extra_headers(Some(&repo), "https://example.test/repo.git/info")
                .expect("matching URL");
        let sibling = parsed_http_url_with_extra_headers(
            Some(&repo),
            "https://example.test/repo.git-private",
        )
        .expect("non-matching URL");

        assert_eq!(
            child.authorization.as_deref(),
            Some("Basic Zmlyc3Q6Zmlyc3QtcGFzcw==")
        );
        assert_eq!(
            sibling.authorization.as_deref(),
            Some("Basic c2Vjb25kOnNlY29uZC1wYXNz")
        );
    }

    #[test]
    fn http_redirect_keeps_authorization_only_for_same_origin() {
        let source = ParsedHttpUrl::parse("https://user:pass@example.test/repo.git")
            .expect("source URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: vec![
                    "Authorization: Bearer scoped".to_owned(),
                    "Proxy-Authorization: Basic proxy".to_owned(),
                    "X-Skron-Trace: keep".to_owned(),
                ],
                tls_no_verify: false,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
                proxy: None,
            });

        let same = http_redirect_target_url(&source, "info/refs", "/repo.git/objects/info/packs")
            .expect("same-origin redirect");
        let cross =
            http_redirect_target_url(&source, "info/refs", "https://other.example.test/repo.git")
                .expect("cross-origin redirect");
        let different_port =
            http_redirect_target_url(&source, "info/refs", "https://example.test:8443/repo.git")
                .expect("different-port redirect");

        assert_eq!(same.authorization.as_deref(), Some("Basic dXNlcjpwYXNz"));
        assert_eq!(cross.authorization, None);
        assert_eq!(different_port.authorization, None);
        assert_eq!(
            same.extra_headers,
            vec![
                "Authorization: Bearer scoped".to_owned(),
                "Proxy-Authorization: Basic proxy".to_owned(),
                "X-Skron-Trace: keep".to_owned()
            ]
        );
        assert_eq!(cross.extra_headers, vec!["X-Skron-Trace: keep".to_owned()]);
        assert_eq!(
            different_port.extra_headers,
            vec!["X-Skron-Trace: keep".to_owned()]
        );
    }

    #[test]
    fn parsed_http_url_writes_configured_extra_headers() {
        let url = ParsedHttpUrl::parse("http://example.test/repo.git")
            .expect("parsed URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: vec![
                    "X-Skron-Token: local".to_owned(),
                    "X-Skron-Trace: one".to_owned(),
                ],
                tls_no_verify: false,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
                proxy: None,
            });
        let mut request = Vec::new();

        write_http_request_parts(
            &mut request,
            &url,
            "GET",
            "info/refs",
            &[],
            &PackBody::Empty,
        )
        .expect("write request");

        assert_eq!(
            request,
            b"GET /repo.git/info/refs HTTP/1.1\r\nHost: example.test\r\nX-Skron-Token: local\r\nX-Skron-Trace: one\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn parsed_http_url_rejects_invalid_userinfo_percent_escape() {
        let error = ParsedHttpUrl::parse("https://user%xx@example.test/repo.git")
            .expect_err("invalid userinfo escape");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP URL userinfo percent escape is invalid: user%xx"
        ));
    }

    #[test]
    fn parsed_http_url_writes_root_suffix_without_formatter() {
        let url = ParsedHttpUrl::parse("https://example.test").expect("parsed URL");
        let mut full = Vec::new();
        let mut path = Vec::new();

        url.write_full_url_with_suffix(&mut full, "info/refs")
            .expect("write full URL");
        url.write_path_with_suffix(&mut path, "")
            .expect("write path");

        assert_eq!(full, b"https://example.test/info/refs");
        assert_eq!(path, b"/");
    }

    #[test]
    fn helper_request_frame_writes_inline_body_headers_without_formatter() {
        let url = ParsedHttpUrl::parse("https://example.test/repo.git").expect("parsed URL");
        let mut frame = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);

        let write_body = append_http_helper_request_frame(
            &mut frame,
            &url,
            "POST",
            "git-upload-pack",
            12345,
            &PackBody::Empty,
            None,
        )
        .expect("request frame");

        assert!(write_body);
        assert_eq!(
            frame,
            b"REQUEST\nMETHOD POST\nURL https://example.test/repo.git/git-upload-pack\nHEADER Content-Type: application/x-git-upload-pack-request\nCONTENT-LENGTH 12345\n\n"
        );
    }

    #[test]
    fn helper_request_frame_keeps_post_content_type_for_redirect_url_path() {
        let url =
            ParsedHttpUrl::parse("https://example.test/repo.git/git-upload-pack").expect("url");
        let mut frame = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);

        let write_body = append_http_helper_request_frame(
            &mut frame,
            &url,
            "POST",
            "",
            6,
            &PackBody::Empty,
            None,
        )
        .expect("request frame");

        assert!(write_body);
        assert_eq!(
            frame,
            b"REQUEST\nMETHOD POST\nURL https://example.test/repo.git/git-upload-pack\nHEADER Content-Type: application/x-git-upload-pack-request\nCONTENT-LENGTH 6\n\n"
        );
    }

    #[test]
    fn helper_request_frame_keeps_receive_pack_content_type_for_redirect_url_path() {
        let url =
            ParsedHttpUrl::parse("https://example.test/repo.git/git-receive-pack").expect("url");
        let mut frame = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);

        let write_body = append_http_helper_request_frame(
            &mut frame,
            &url,
            "POST",
            "",
            6,
            &PackBody::Empty,
            None,
        )
        .expect("request frame");

        assert!(write_body);
        assert_eq!(
            frame,
            b"REQUEST\nMETHOD POST\nURL https://example.test/repo.git/git-receive-pack\nHEADER Content-Type: application/x-git-receive-pack-request\nCONTENT-LENGTH 6\n\n"
        );
    }

    #[test]
    fn helper_request_frame_forwards_url_userinfo_authorization_header() {
        let url =
            ParsedHttpUrl::parse("https://user:pass@example.test/repo.git").expect("parsed URL");
        let mut frame = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);

        let write_body = append_http_helper_request_frame(
            &mut frame,
            &url,
            "GET",
            "info/refs?service=git-upload-pack",
            0,
            &PackBody::Empty,
            None,
        )
        .expect("request frame");

        assert!(!write_body);
        assert_eq!(
            frame,
            b"REQUEST\nMETHOD GET\nURL https://example.test/repo.git/info/refs?service=git-upload-pack\nHEADER Authorization: Basic dXNlcjpwYXNz\n\n"
        );
    }

    #[test]
    fn helper_request_frame_forwards_configured_extra_headers() {
        let url = ParsedHttpUrl::parse("https://example.test/repo.git")
            .expect("parsed URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: vec!["X-Skron-Token: local".to_owned()],
                tls_no_verify: false,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
                proxy: None,
            });
        let mut frame = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);

        let write_body = append_http_helper_request_frame(
            &mut frame,
            &url,
            "GET",
            "info/refs?service=git-upload-pack",
            0,
            &PackBody::Empty,
            None,
        )
        .expect("request frame");

        assert!(!write_body);
        assert_eq!(
            frame,
            b"REQUEST\nMETHOD GET\nURL https://example.test/repo.git/info/refs?service=git-upload-pack\nHEADER X-Skron-Token: local\n\n"
        );
    }

    #[test]
    fn helper_request_frame_writes_body_file_and_prefix_length() {
        let url = ParsedHttpUrl::parse("https://example.test/repo.git").expect("parsed URL");
        let output_file = Path::new("/tmp/skron-output.pack");
        let body_file = PackBody::File {
            path: PathBuf::from("/tmp/skron-body.pack"),
            remove_on_drop: false,
        };
        let mut frame = Vec::with_capacity(PKT_LINE_PAYLOAD_CAPACITY_HINT);

        let write_body = append_http_helper_request_frame(
            &mut frame,
            &url,
            "POST",
            "git-receive-pack",
            9,
            &body_file,
            Some(output_file),
        )
        .expect("request frame");

        assert!(write_body);
        assert_eq!(
            frame,
            b"REQUEST\nMETHOD POST\nURL https://example.test/repo.git/git-receive-pack\nOUTPUT-FILE /tmp/skron-output.pack\nHEADER Content-Type: application/x-git-receive-pack-request\nBODY-FILE /tmp/skron-body.pack\nBODY-PREFIX-LENGTH 9\n\n"
        );
    }

    #[test]
    fn helper_request_frame_reserves_long_paths_without_growth() {
        let long_repo = "segment/".repeat(512);
        let url = ParsedHttpUrl::parse(&format!("https://example.test/{long_repo}repo.git"))
            .expect("parsed URL");
        let output_path = PathBuf::from(format!("/tmp/{long_repo}output.pack"));
        let body_file = PackBody::File {
            path: PathBuf::from(format!("/tmp/{long_repo}body.pack")),
            remove_on_drop: false,
        };
        let mut frame = Vec::new();

        append_http_helper_request_frame(
            &mut frame,
            &url,
            "POST",
            "git-receive-pack",
            123_456,
            &body_file,
            Some(&output_path),
        )
        .expect("request frame");

        assert_eq!(frame.capacity(), frame.len());
    }

    #[test]
    fn direct_http_request_body_file_copy_uses_exact_declared_length() {
        let mut reader = io::Cursor::new(b"pack-extra".to_vec());
        let mut out = Vec::new();

        copy_exact_len(&mut reader, &mut out, 4).expect("copy body");

        assert_eq!(out, b"pack");
        assert_eq!(reader.position(), 4);
    }

    #[test]
    fn direct_http_request_body_file_copy_reports_early_eof() {
        let mut reader = io::Cursor::new(b"pack".to_vec());
        let mut out = Vec::new();

        let error = copy_exact_len(&mut reader, &mut out, 8).expect_err("short body");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "request body file ended early"
        ));
    }

    #[test]
    fn exact_len_copy_reports_call_site_early_eof_message() {
        let mut reader = io::Cursor::new(b"pack".to_vec());
        let mut out = Vec::new();

        let error = copy_exact_len_with_message(&mut reader, &mut out, 8, "pack file ended early")
            .expect_err("short body");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "pack file ended early"
        ));
    }

    #[test]
    fn http_response_body_copy_uses_declared_length_for_non_chunked_body() {
        let mut reader = io::Cursor::new(b"pack-extra".to_vec());
        let mut out = Vec::new();

        let copied = copy_http_response_body_to_writer(
            &mut reader,
            &mut out,
            Some(4),
            false,
            "HTTP packfile response ended early",
        )
        .expect("copy body");

        assert_eq!(copied, 4);
        assert_eq!(out, b"pack");
        assert_eq!(reader.position(), 4);
    }

    #[test]
    fn http_response_body_copy_preserves_chunked_body_reader_semantics() {
        let mut reader = io::Cursor::new(b"pack-extra".to_vec());
        let mut out = Vec::new();

        let copied = copy_http_response_body_to_writer(
            &mut reader,
            &mut out,
            Some(4),
            true,
            "HTTP packfile response ended early",
        )
        .expect("copy body");

        assert_eq!(copied, 10);
        assert_eq!(out, b"pack-extra");
    }

    #[test]
    fn http_response_body_copy_reports_declared_length_early_eof() {
        let mut reader = io::Cursor::new(b"pack".to_vec());
        let mut out = Vec::new();

        let error = copy_http_response_body_to_writer(
            &mut reader,
            &mut out,
            Some(8),
            false,
            "HTTP packfile response ended early",
        )
        .expect_err("short body");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP packfile response ended early"
        ));
    }

    #[test]
    fn decimal_usize_writer_emits_zero_and_max_without_allocation() {
        let mut buffer = Vec::new();

        append_decimal_usize(&mut buffer, 0);
        buffer.push(b'\n');
        append_decimal_usize(&mut buffer, usize::MAX);

        assert_eq!(buffer, format!("0\n{}", usize::MAX).as_bytes());
    }

    #[test]
    fn decimal_len_counts_digits() {
        assert_eq!(decimal_len(0), 1);
        assert_eq!(decimal_len(9), 1);
        assert_eq!(decimal_len(10), 2);
        assert_eq!(decimal_len(999), 3);
        assert_eq!(decimal_len(usize::MAX), usize::MAX.to_string().len());
    }

    #[test]
    fn pack_file_name_writes_hex_without_intermediate_string() {
        let id = oid("1111111111111111111111111111111111111111");

        assert_eq!(
            pack_file_name(&id, ".pack"),
            "pack-1111111111111111111111111111111111111111.pack"
        );
        assert_eq!(
            pack_file_path(Path::new("/tmp/pack"), &id, ".idx"),
            PathBuf::from("/tmp/pack/pack-1111111111111111111111111111111111111111.idx")
        );
    }

    #[test]
    fn object_id_hex_eq_compares_lowercase_hex_without_allocating() {
        let id = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        assert!(object_id_hex_eq(
            &id,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!object_id_hex_eq(
            &id,
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ));
        assert!(!object_id_hex_eq(
            &id,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn upload_pack_request_writes_depth_without_formatter_growth() {
        let first = oid("1111111111111111111111111111111111111111");
        let second = oid("2222222222222222222222222222222222222222");
        let roots = [first, second];

        let request =
            build_upload_pack_request(&roots, Some(123_456)).expect("upload-pack request");

        assert_eq!(
            request.len(),
            upload_pack_request_capacity(&roots, Some(123_456))
        );
        assert!(
            request
                .windows(b"deepen 123456\n".len())
                .any(|window| window == b"deepen 123456\n")
        );
    }

    #[test]
    fn daemon_service_request_writes_default_port_without_formatter() {
        let url = ParsedDaemonUrl {
            host: "example.test".to_owned(),
            port: 9418,
            path: "/repo.git".to_owned(),
        };
        let mut request =
            Vec::with_capacity(daemon_service_request_capacity(&url, "git-upload-pack"));

        write_daemon_service_request(&mut request, &url, "git-upload-pack")
            .expect("daemon request");

        assert_eq!(
            request,
            b"0030git-upload-pack /repo.git\0host=example.test\0"
        );
        assert_eq!(request.capacity(), request.len());
    }

    #[test]
    fn daemon_service_request_writes_explicit_port_without_formatter() {
        let url = ParsedDaemonUrl {
            host: "example.test".to_owned(),
            port: 9419,
            path: "/repo.git".to_owned(),
        };
        let mut request =
            Vec::with_capacity(daemon_service_request_capacity(&url, "git-upload-pack"));

        write_daemon_service_request(&mut request, &url, "git-upload-pack")
            .expect("daemon request");

        assert_eq!(
            request,
            b"0035git-upload-pack /repo.git\0host=example.test:9419\0"
        );
        assert_eq!(request.capacity(), request.len());
    }

    #[test]
    fn parsed_http_url_supports_ipv6_hosts_in_brackets() {
        let url =
            ParsedHttpUrl::parse("https://[2001:4860:4860::8888]/repo.git").expect("ipv6 parsed");
        let mut direct = Vec::new();

        url.write_host_header(&mut direct)
            .expect("write host header");

        assert_eq!(direct, b"[2001:4860:4860::8888]");
        assert_eq!(url.connect_host(), "2001:4860:4860::8888");
        assert_eq!(url.port, 443);

        let mut full = Vec::new();
        url.write_full_url_with_suffix(&mut full, "info/refs")
            .expect("write url");
        assert_eq!(full, b"https://[2001:4860:4860::8888]/repo.git/info/refs");
    }

    #[test]
    fn parsed_http_url_supports_ipv6_host_with_port() {
        let url = ParsedHttpUrl::parse("https://[2001:4860:4860::8888]:8443/repo.git")
            .expect("ipv6 with port");
        let mut direct = Vec::new();

        write_http_request_parts(&mut direct, &url, "GET", "info/refs", &[], &PackBody::Empty)
            .expect("write request");

        assert_eq!(url.connect_host(), "2001:4860:4860::8888");
        assert_eq!(
            direct,
            b"GET /repo.git/info/refs HTTP/1.1\r\nHost: [2001:4860:4860::8888]:8443\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn parsed_http_url_rejects_invalid_ipv6_without_brackets() {
        let error = ParsedHttpUrl::parse("https://2001:4860:4860::8888/repo.git")
            .expect_err("ipv6 without brackets");
        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "invalid HTTP URL host"
        ));
    }

    #[test]
    fn parsed_http_url_rejects_ipv6_with_empty_port() {
        let error = ParsedHttpUrl::parse("https://[2001:4860:4860::8888]:/repo.git")
            .expect_err("ipv6 with empty port");
        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "invalid HTTP URL port: "
        ));
    }

    #[test]
    fn parsed_http_url_parse_is_case_insensitive_for_scheme() {
        assert_eq!(
            ParsedHttpUrl::parse("HTTPS://example.test/repo.git")
                .expect("uppercase https")
                .scheme,
            HttpScheme::Https,
        );
        assert_eq!(
            ParsedHttpUrl::parse("HTTP://example.test/repo.git")
                .expect("uppercase http")
                .scheme,
            HttpScheme::Http,
        );
    }

    #[test]
    fn http_transport_detection_is_case_insensitive() {
        assert!(is_http_transport_url("HTTPS://example.test/repo.git"));
        assert!(is_http_transport_url("http://example.test/repo.git"));
        assert!(!is_http_transport_url("SSH://example.test/repo.git"));
    }

    #[test]
    fn http_request_writer_uses_direct_url_parts() {
        let url = ParsedHttpUrl::parse("http://example.test:8080/repo.git").expect("parsed URL");
        let mut request = Vec::new();

        write_http_request_parts(
            &mut request,
            &url,
            "GET",
            "info/refs",
            &[],
            &PackBody::Empty,
        )
        .expect("write HTTP request");

        assert_eq!(
            request,
            b"GET /repo.git/info/refs HTTP/1.1\r\nHost: example.test:8080\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn http_request_writer_emits_post_body_head_without_formatter() {
        let url = ParsedHttpUrl::parse("http://example.test/repo.git").expect("parsed URL");
        let mut request = Vec::new();

        write_http_request_parts(
            &mut request,
            &url,
            "POST",
            "git-upload-pack",
            b"body",
            &PackBody::Empty,
        )
        .expect("write HTTP request");

        assert_eq!(
            request,
            b"POST /repo.git/git-upload-pack HTTP/1.1\r\nHost: example.test\r\nContent-Type: application/x-git-upload-pack-request\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
        );
    }

    #[test]
    fn http_request_writer_keeps_post_content_type_for_redirect_url_path() {
        let url = ParsedHttpUrl::parse("http://example.test/repo.git/git-upload-pack")
            .expect("parsed URL");
        let mut request = Vec::new();

        write_http_request_parts(&mut request, &url, "POST", "", b"body", &PackBody::Empty)
            .expect("write HTTP request");

        assert_eq!(
            request,
            b"POST /repo.git/git-upload-pack HTTP/1.1\r\nHost: example.test\r\nContent-Type: application/x-git-upload-pack-request\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
        );
    }

    #[test]
    fn http_request_writer_keeps_receive_pack_content_type_for_redirect_url_path() {
        let url = ParsedHttpUrl::parse("http://example.test/repo.git/git-receive-pack")
            .expect("parsed URL");
        let mut request = Vec::new();

        write_http_request_parts(&mut request, &url, "POST", "", b"body", &PackBody::Empty)
            .expect("write HTTP request");

        assert_eq!(
            request,
            b"POST /repo.git/git-receive-pack HTTP/1.1\r\nHost: example.test\r\nContent-Type: application/x-git-receive-pack-request\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody"
        );
    }

    #[test]
    fn http_request_reader_replays_post_body_and_content_type_after_redirect() {
        fn read_test_http_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>) {
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            let header_end = loop {
                let read = stream.read(&mut buf).expect("read request");
                assert_ne!(read, 0, "request ended before headers completed");
                request.extend_from_slice(&buf[..read]);
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    break header_end;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]).into_owned();
            let content_len = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            let mut body = request[header_end + 4..].to_vec();
            while body.len() < content_len {
                let read = stream.read(&mut buf).expect("read request body");
                assert_ne!(read, 0, "request ended before body completed");
                body.extend_from_slice(&buf[..read]);
            }
            body.truncate(content_len);
            (headers, body)
        }

        let target_listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind target");
        let target_port = target_listener.local_addr().expect("target addr").port();
        let redirect_listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind redirect");
        let redirect_port = redirect_listener
            .local_addr()
            .expect("redirect addr")
            .port();
        let first_request = std::sync::Arc::new(std::sync::Mutex::new(None));
        let second_request = std::sync::Arc::new(std::sync::Mutex::new(None));
        let first_request_thread = first_request.clone();
        let second_request_thread = second_request.clone();
        let target_base = format!("http://127.0.0.1:{target_port}");
        let redirect_handle = std::thread::spawn(move || {
            let (mut stream, _) = redirect_listener.accept().expect("accept redirect");
            let (headers, body) = read_test_http_request(&mut stream);
            let raw_path = headers
                .lines()
                .next()
                .and_then(|line| line.split_ascii_whitespace().nth(1))
                .unwrap_or("/")
                .to_owned();
            *first_request_thread.lock().expect("first lock") = Some((headers, body));
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: {target_base}{raw_path}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write redirect");
        });
        let target_handle = std::thread::spawn(move || {
            let (mut stream, _) = target_listener.accept().expect("accept target");
            let request = read_test_http_request(&mut stream);
            *second_request_thread.lock().expect("second lock") = Some(request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("write ok");
        });
        let url = ParsedHttpUrl::parse(&format!("http://127.0.0.1:{redirect_port}/repo.git"))
            .expect("url");

        let (head, mut response_body) =
            http_request_reader(&url, "POST", "git-upload-pack", b"want=1").expect("response");
        let mut response = Vec::new();
        response_body
            .read_to_end(&mut response)
            .expect("read response");
        redirect_handle.join().expect("join redirect");
        target_handle.join().expect("join target");

        assert_eq!(head.status_code, 200);
        assert_eq!(response, b"ok");
        let (first_headers, first_body) = first_request
            .lock()
            .expect("first lock")
            .clone()
            .expect("first request");
        let (second_headers, second_body) = second_request
            .lock()
            .expect("second lock")
            .clone()
            .expect("second request");
        assert!(first_headers.starts_with("POST /repo.git/git-upload-pack "));
        assert!(second_headers.starts_with("POST /repo.git/git-upload-pack "));
        assert!(first_headers.contains("Content-Type: application/x-git-upload-pack-request\r\n"));
        assert!(second_headers.contains("Content-Type: application/x-git-upload-pack-request\r\n"));
        assert_eq!(first_body, b"want=1");
        assert_eq!(second_body, b"want=1");
    }

    #[test]
    fn http_response_head_parser_streams_headers() {
        let mut reader = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 12\r\n\r\n",
        );

        let head = read_http_response_head(&mut reader).expect("response head");

        assert_eq!(head.status_code, 200);
        assert!(head.chunked);
        assert_eq!(head.content_length, Some(12));
    }

    #[test]
    fn http_response_head_parser_trims_status_line_in_place() {
        let mut reader = io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");

        let head = read_http_response_head(&mut reader).expect("response head");

        assert_eq!(head.status_line.to_string(), "HTTP/1.1 200 OK");
        assert!(
            head.status_line.raw_capacity().expect("raw status line")
                >= PKT_LINE_PAYLOAD_CAPACITY_HINT
        );
    }

    #[test]
    fn fixed_length_http_body_buf_read_stops_at_declared_length() {
        let cursor = io::Cursor::new(b"abc\nextra".to_vec());
        let mut body = FixedLengthHttpBody {
            reader: io::BufReader::new(cursor),
            remaining: 4,
        };
        let mut line = Vec::new();

        assert_eq!(body.read_until(b'\n', &mut line).expect("read line"), 4);
        assert_eq!(line, b"abc\n");
        assert_eq!(body.read_until(b'\n', &mut line).expect("read eof"), 0);

        let mut inner = body.reader;
        let mut rest = Vec::new();
        inner.read_to_end(&mut rest).expect("read rest");
        assert_eq!(rest, b"extra");
    }

    #[test]
    fn http_response_head_parser_rejects_conflicting_content_length() {
        let mut reader =
            io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nContent-Length: 13\r\n\r\n");

        let error = read_http_response_head(&mut reader).expect_err("conflicting length");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP response has conflicting Content-Length headers"
        ));
    }

    #[test]
    fn http_response_head_parser_rejects_malformed_content_length() {
        let mut reader = io::Cursor::new(b"HTTP/1.1 200 OK\r\nContent-Length: nope\r\n\r\n");

        let error = read_http_response_head(&mut reader).expect_err("malformed length");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP response Content-Length is malformed: nope"
        ));
    }

    #[test]
    fn http_response_head_parser_rejects_oversized_status_code() {
        let mut reader = io::Cursor::new(b"HTTP/1.1 999999 OK\r\nContent-Length: 0\r\n\r\n");

        let error = read_http_response_head(&mut reader).expect_err("oversized status");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP response status is malformed: HTTP/1.1 999999 OK"
        ));
    }

    #[test]
    fn http_response_head_parser_rejects_oversized_content_length() {
        let mut reader = io::Cursor::new(
            b"HTTP/1.1 200 OK\r\nContent-Length: 999999999999999999999999999999999999\r\n\r\n",
        );

        let error = read_http_response_head(&mut reader).expect_err("oversized length");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP response Content-Length is malformed: 999999999999999999999999999999999999"
        ));
    }

    #[test]
    fn http_response_line_reader_rejects_unbounded_lines() {
        let mut input = vec![b'a'; HTTP_RESPONSE_LINE_LIMIT + 1];
        input.push(b'\n');
        let mut reader = io::BufReader::new(io::Cursor::new(input));
        let mut line = String::new();

        let error = read_limited_http_response_line(&mut reader, &mut line)
            .expect_err("oversized response line");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "HTTP response line too long");
    }

    #[test]
    fn http_chunk_reader_rejects_unbounded_size_lines() {
        let mut input = vec![b'1'; HTTP_RESPONSE_LINE_LIMIT + 1];
        input.extend_from_slice(b"\r\n");
        let mut reader = io::BufReader::new(io::Cursor::new(input));
        let mut line = String::new();

        let error = read_http_chunk_size(&mut reader, &mut line).expect_err("oversized chunk line");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "HTTP response line too long");
    }

    #[test]
    fn helper_line_reader_reuses_buffer_and_trims_in_place() {
        let mut reader = io::Cursor::new(b"VERSION 1.1\r\nSTATUS 200 OK\n".to_vec());
        let mut line = String::with_capacity(64);
        let capacity = line.capacity();

        read_helper_line(&mut reader, &mut line).expect("first helper line");
        assert_eq!(line, "VERSION 1.1");
        assert_eq!(line.capacity(), capacity);

        read_helper_line(&mut reader, &mut line).expect("second helper line");
        assert_eq!(line, "STATUS 200 OK");
        assert_eq!(line.capacity(), capacity);
    }

    #[test]
    fn helper_status_code_parser_uses_fixed_http_status_prefix() {
        assert_eq!(parse_helper_status_code("200 OK").expect("200"), 200);
        assert_eq!(parse_helper_status_code("404 Not Found").expect("404"), 404);

        let error = parse_helper_status_code("20 OK").expect_err("short status code should fail");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper response status is malformed: 20 OK"
        ));
    }

    #[test]
    fn helper_content_length_parser_rejects_conflicting_values() {
        let mut content_length = None;

        update_helper_content_length(&mut content_length, "12").expect("first length");
        let error = update_helper_content_length(&mut content_length, "34").expect_err("conflict");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper returned conflicting Content-Length headers"
        ));
    }

    #[test]
    fn helper_response_field_parser_rejects_duplicate_singletons() {
        let mut version = None;

        update_helper_response_field(&mut version, "VERSION", "1.1").expect("first version");
        let error =
            update_helper_response_field(&mut version, "VERSION", "2").expect_err("duplicate");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper returned duplicate VERSION header"
        ));
        assert_eq!(version.as_deref(), Some("1.1"));
    }

    #[test]
    fn helper_status_line_formats_from_parts_without_raw_buffer() {
        let status_line = HttpStatusLine::parts("1.1".to_owned(), "200 OK".to_owned());

        assert_eq!(status_line.to_string(), "HTTP/1.1 200 OK");
        assert_eq!(status_line.raw_capacity(), None);
    }

    #[test]
    fn helper_response_header_parser_keeps_first_location() {
        let mut location = None;

        update_helper_response_header(&mut location, "Content-Type: text/plain");
        update_helper_response_header(&mut location, "Location: /repo.git");
        update_helper_response_header(&mut location, "Location: /other.git");

        assert_eq!(location.as_deref(), Some("/repo.git"));
    }

    #[cfg(unix)]
    #[test]
    fn helper_request_to_body_follows_redirect_response_frame() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().expect("temp dir");
        let helper = temp.path().join("fake-remote-http.sh");
        fs::write(
            &helper,
            r#"#!/bin/sh
set -eu
count=0
while IFS= read -r line; do
  if [ "$line" = "DONE" ]; then
    exit 0
  fi
  if [ "$line" != "REQUEST" ]; then
    echo "unexpected frame: $line" >&2
    exit 2
  fi
  while IFS= read -r line; do
    [ -z "$line" ] && break
  done
  count=$((count + 1))
  if [ "$count" -eq 1 ]; then
    printf 'RESPONSE\nVERSION 1.1\nSTATUS 302 Found\nHEADER location: https://target.test/repo.git/info/refs?service=git-upload-pack\nCONTENT-LENGTH 0\n\n\n'
  else
    printf 'RESPONSE\nVERSION 1.1\nSTATUS 200 OK\nCONTENT-LENGTH 2\n\nok\n'
  fi
done
"#,
        )
        .expect("write helper");
        let mut perms = fs::metadata(&helper)
            .expect("helper metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).expect("chmod helper");
        let _guard = TestEnvVarGuard::set("SKRON_GIT_REMOTE_HTTP", &helper);
        let url = ParsedHttpUrl::parse("https://source.test/repo.git").expect("url");
        let mut session = RemoteHttpHelperSession::spawn(&url).expect("helper session");

        let response = session
            .request_to_body(
                &url,
                "GET",
                "info/refs?service=git-upload-pack",
                &[],
                &PackBody::Empty,
            )
            .expect("redirected response");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.into_vec(response.body_len).unwrap(), b"ok");
    }

    #[cfg(unix)]
    #[test]
    fn helper_request_to_body_replays_inline_body_after_redirect() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().expect("temp dir");
        let helper = temp.path().join("fake-remote-http.sh");
        fs::write(
            &helper,
            r#"#!/bin/sh
set -eu
count=0
log="$0.log"
: > "$log"
while IFS= read -r line; do
  if [ "$line" = "DONE" ]; then
    exit 0
  fi
  if [ "$line" != "REQUEST" ]; then
    echo "unexpected frame: $line" >&2
    exit 2
  fi
  content_length=0
  while IFS= read -r line; do
    [ -z "$line" ] && break
    case "$line" in
      CONTENT-LENGTH\ *)
        content_length=${line#CONTENT-LENGTH }
        ;;
    esac
  done
  body=""
  if [ "$content_length" -gt 0 ]; then
    body=$(dd bs=1 count="$content_length" 2>/dev/null || true)
  fi
  printf '%s\n' "$body" >> "$log"
  count=$((count + 1))
  if [ "$count" -eq 1 ]; then
    printf 'RESPONSE\nVERSION 1.1\nSTATUS 302 Found\nHEADER location: https://target.test/repo.git/git-upload-pack\nCONTENT-LENGTH 0\n\n\n'
  else
    printf 'RESPONSE\nVERSION 1.1\nSTATUS 200 OK\nCONTENT-LENGTH 2\n\nok\n'
  fi
done
"#,
        )
        .expect("write helper");
        let mut perms = fs::metadata(&helper)
            .expect("helper metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).expect("chmod helper");
        let _guard = TestEnvVarGuard::set("SKRON_GIT_REMOTE_HTTP", &helper);
        let url = ParsedHttpUrl::parse("https://source.test/repo.git").expect("url");
        let mut session = RemoteHttpHelperSession::spawn(&url).expect("helper session");

        let response = session
            .request_to_body(&url, "POST", "git-upload-pack", b"want=1", &PackBody::Empty)
            .expect("redirected response");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.into_vec(response.body_len).unwrap(), b"ok");
        assert_eq!(
            fs::read_to_string(helper.with_extension("sh.log")).expect("body log"),
            "want=1\nwant=1\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn helper_request_to_file_allows_inline_redirect_response_frame() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::TempDir::new().expect("temp dir");
        let helper = temp.path().join("fake-remote-http.sh");
        fs::write(
            &helper,
            r#"#!/bin/sh
set -eu
while IFS= read -r line; do
  if [ "$line" = "DONE" ]; then
    exit 0
  fi
  if [ "$line" != "REQUEST" ]; then
    echo "unexpected frame: $line" >&2
    exit 2
  fi
  while IFS= read -r line; do
    [ -z "$line" ] && break
  done
  printf 'RESPONSE\nVERSION 1.1\nSTATUS 302 Found\nHEADER location: https://target.test/repo.git/objects/pack/pack-test.pack\nCONTENT-LENGTH 0\n\n\n'
done
"#,
        )
        .expect("write helper");
        let mut perms = fs::metadata(&helper)
            .expect("helper metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&helper, perms).expect("chmod helper");
        let _guard = TestEnvVarGuard::set("SKRON_GIT_REMOTE_HTTP", &helper);
        let url = ParsedHttpUrl::parse("https://source.test/repo.git").expect("url");
        let output = temp.path().join("pack.out");
        let mut session = RemoteHttpHelperSession::spawn(&url).expect("helper session");

        let head = session
            .request_to_file(
                &url,
                "GET",
                "objects/pack/pack-test.pack",
                &[],
                &PackBody::Empty,
                &output,
            )
            .expect("redirect head");

        assert_eq!(head.status_code, 302);
        assert_eq!(
            head.location.as_deref(),
            Some("https://target.test/repo.git/objects/pack/pack-test.pack")
        );
    }

    #[test]
    fn helper_body_file_parser_rejects_duplicate_and_cleans_owned_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let first = temp.path().join("first-body");
        let second = temp.path().join("second-body");
        fs::write(&first, b"first").expect("write first");
        fs::write(&second, b"second").expect("write second");
        let mut body_file = None;

        update_helper_body_file(&mut body_file, first.clone(), None).expect("first body file");
        let error =
            update_helper_body_file(&mut body_file, second.clone(), None).expect_err("duplicate");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper returned duplicate BODY-FILE header"
        ));
        assert!(first.exists());
        assert!(second.exists());
        drop(body_file);
        assert!(!first.exists());
        assert!(second.exists());
    }

    #[test]
    fn helper_file_response_requires_content_length() {
        let error = helper_response_body_len(None, true).expect_err("missing body length");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper response BODY-FILE is missing Content-Length"
        ));
        assert_eq!(helper_response_body_len(Some(12), true).unwrap(), 12);
        assert_eq!(helper_response_body_len(None, false).unwrap(), 0);
    }

    #[test]
    fn helper_inline_body_read_is_exact_sized() {
        let mut reader = io::Cursor::new(b"body\ntrailing".to_vec());

        let body = read_exact_helper_body(
            &mut reader,
            4,
            "HTTP helper response ended before completing inline body",
        )
        .expect("helper body");

        assert_eq!(body, b"body");
        let mut frame_lf = [0_u8; 1];
        reader.read_exact(&mut frame_lf).expect("frame lf");
        assert_eq!(frame_lf, *b"\n");
    }

    #[test]
    fn helper_inline_body_read_handles_medium_body_with_bounded_initial_capacity() {
        let input = vec![0x41; PACK_RECEIPT_BUF_CAPACITY + 17];
        let mut reader = io::Cursor::new(input.clone());

        let body = read_exact_helper_body(
            &mut reader,
            input.len(),
            "HTTP helper response ended before completing inline body",
        )
        .expect("helper body");

        assert_eq!(body, input);
    }

    #[test]
    fn helper_inline_body_initial_capacity_is_bounded() {
        assert_eq!(helper_inline_body_initial_capacity(0), 0);
        assert_eq!(helper_inline_body_initial_capacity(17), 17);
        assert_eq!(
            helper_inline_body_initial_capacity(PACK_RECEIPT_BUF_CAPACITY + 17),
            PACK_RECEIPT_BUF_CAPACITY
        );
        assert_eq!(
            helper_inline_body_initial_capacity(usize::MAX),
            PACK_RECEIPT_BUF_CAPACITY
        );
    }

    #[test]
    fn helper_inline_body_read_reports_early_eof_as_fatal() {
        let mut reader = io::Cursor::new(b"bo".to_vec());

        let error = read_exact_helper_body(
            &mut reader,
            4,
            "HTTP helper response ended before completing inline body",
        )
        .expect_err("early eof");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper response ended before completing inline body"
        ));
    }

    #[test]
    fn remote_http_helper_ca_file_arg_reads_git_ssl_cainfo() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("ca.pem");
        let url = ParsedHttpUrl::parse("https://example.test/repo.git").expect("parsed URL");
        let _guard = TestEnvVarGuard::set("GIT_SSL_CAINFO", &path);

        let parsed = remote_http_helper_ca_file_arg_for_url(&url)
            .expect("ca file env")
            .expect("ca file path");

        assert_eq!(parsed, path);
    }

    #[test]
    fn remote_http_helper_ca_file_arg_ignores_empty_value() {
        let url = ParsedHttpUrl::parse("https://example.test/repo.git").expect("parsed URL");
        let _guard = TestEnvVarGuard::set("GIT_SSL_CAINFO", "");

        assert_eq!(
            remote_http_helper_ca_file_arg_for_url(&url).expect("empty env"),
            None
        );
    }

    #[test]
    fn remote_http_helper_tls_identity_args_use_url_config_or_env() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let config_ca = temp.path().join("config-ca.pem");
        let config_cert = temp.path().join("config-client.pem");
        let config_key = temp.path().join("config-client.key");
        let env_ca = temp.path().join("env-ca.pem");
        let env_cert = temp.path().join("env-client.pem");
        let env_key = temp.path().join("env-client.key");
        let configured_url = ParsedHttpUrl::parse("https://example.test/repo.git")
            .expect("parsed URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: Vec::new(),
                tls_no_verify: false,
                ca_file: Some(config_ca.clone()),
                client_cert_file: Some(config_cert.clone()),
                client_key_file: Some(config_key.clone()),
                proxy: None,
            });

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_CAINFO", "");
            assert_eq!(
                remote_http_helper_ca_file_arg_for_url(&configured_url).expect("configured ca"),
                Some(config_ca.clone())
            );
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_CERT", "");
            assert_eq!(
                remote_http_helper_client_cert_file_arg_for_url(&configured_url)
                    .expect("configured cert"),
                Some(config_cert.clone())
            );
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_KEY", "");
            assert_eq!(
                remote_http_helper_client_key_file_arg_for_url(&configured_url)
                    .expect("configured key"),
                Some(config_key.clone())
            );
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_CAINFO", &env_ca);
            assert_eq!(
                remote_http_helper_ca_file_arg_for_url(&configured_url).expect("env ca"),
                Some(env_ca)
            );
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_CERT", &env_cert);
            assert_eq!(
                remote_http_helper_client_cert_file_arg_for_url(&configured_url).expect("env cert"),
                Some(env_cert)
            );
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_KEY", &env_key);
            assert_eq!(
                remote_http_helper_client_key_file_arg_for_url(&configured_url).expect("env key"),
                Some(env_key)
            );
        }
    }

    #[test]
    fn remote_http_helper_proxy_env_uses_config_when_env_is_unset() {
        let https_url = ParsedHttpUrl::parse("https://example.test/repo.git")
            .expect("parsed URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: Vec::new(),
                tls_no_verify: false,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
                proxy: Some("http://proxy.example:8080".to_owned()),
            });
        let http_url = ParsedHttpUrl::parse("http://example.test/repo.git")
            .expect("parsed URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: Vec::new(),
                tls_no_verify: false,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
                proxy: Some("http://proxy.example:8080".to_owned()),
            });

        assert_eq!(
            remote_http_helper_proxy_env_for_url_with_env(&https_url, |_| false),
            Some(("HTTPS_PROXY", "http://proxy.example:8080".to_owned()))
        );
        assert_eq!(
            remote_http_helper_proxy_env_for_url_with_env(&http_url, |_| false),
            Some(("HTTP_PROXY", "http://proxy.example:8080".to_owned()))
        );
    }

    #[test]
    fn remote_http_helper_proxy_env_keeps_existing_env_override() {
        let url = ParsedHttpUrl::parse("https://example.test/repo.git")
            .expect("parsed URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: Vec::new(),
                tls_no_verify: false,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
                proxy: Some("http://proxy.example:8080".to_owned()),
            });

        assert_eq!(
            remote_http_helper_proxy_env_for_url_with_env(&url, |name| name == "HTTPS_PROXY"),
            None
        );
    }

    #[test]
    fn remote_http_helper_client_identity_args_read_git_ssl_cert_and_key() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let cert = temp.path().join("client.crt");
        let key = temp.path().join("client.key");
        let url = ParsedHttpUrl::parse("https://example.test/repo.git").expect("parsed URL");

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_CERT", &cert);
            assert_eq!(
                remote_http_helper_client_cert_file_arg_for_url(&url)
                    .expect("cert env")
                    .expect("cert path"),
                cert
            );
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_KEY", &key);
            assert_eq!(
                remote_http_helper_client_key_file_arg_for_url(&url)
                    .expect("key env")
                    .expect("key path"),
                key
            );
        }
    }

    #[test]
    fn remote_http_helper_client_identity_args_ignore_empty_values() {
        let url = ParsedHttpUrl::parse("https://example.test/repo.git").expect("parsed URL");
        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_CERT", "");
            assert_eq!(
                remote_http_helper_client_cert_file_arg_for_url(&url).expect("empty cert env"),
                None
            );
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_KEY", "");
            assert_eq!(
                remote_http_helper_client_key_file_arg_for_url(&url).expect("empty key env"),
                None
            );
        }
    }

    #[test]
    fn remote_http_helper_tls_no_verify_arg_reads_git_ssl_no_verify() {
        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_NO_VERIFY", "true");
            assert!(remote_http_helper_tls_no_verify_arg());
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_NO_VERIFY", "1");
            assert!(remote_http_helper_tls_no_verify_arg());
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_NO_VERIFY", "");
            assert!(!remote_http_helper_tls_no_verify_arg());
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_NO_VERIFY", "false");
            assert!(!remote_http_helper_tls_no_verify_arg());
        }
    }

    #[test]
    fn remote_http_helper_tls_no_verify_arg_uses_url_config_or_env() {
        let configured_url = ParsedHttpUrl::parse("https://example.test/repo.git")
            .expect("parsed URL")
            .with_http_config(HttpUrlConfig {
                authorization: None,
                headers: Vec::new(),
                tls_no_verify: true,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
                proxy: None,
            });
        let default_url =
            ParsedHttpUrl::parse("https://example.test/repo.git").expect("parsed URL");

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_NO_VERIFY", "false");
            assert!(remote_http_helper_tls_no_verify_arg_for_url(
                &configured_url
            ));
            assert!(!remote_http_helper_tls_no_verify_arg_for_url(&default_url));
        }

        {
            let _guard = TestEnvVarGuard::set("GIT_SSL_NO_VERIFY", "true");
            assert!(remote_http_helper_tls_no_verify_arg_for_url(&default_url));
        }
    }

    #[test]
    fn remote_http_helper_version_arg_respects_scheme_for_http2_http3() {
        {
            let _guard = TestEnvVarGuard::set("SKRON_GIT_HTTP_VERSION", "");
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Https, None).expect("default"),
                None
            );
        }

        {
            let _guard = TestEnvVarGuard::set("SKRON_GIT_HTTP_VERSION", "http2");
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Https, None)
                    .expect("https version"),
                Some("http2")
            );
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Http, None)
                    .expect("http version"),
                Some("http1")
            );
        }

        {
            let _guard = TestEnvVarGuard::set("SKRON_GIT_HTTP_VERSION", "http3");
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Https, None)
                    .expect("https version"),
                Some("http3")
            );
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Http, None)
                    .expect("http version"),
                Some("http1")
            );
        }

        {
            let _guard = TestEnvVarGuard::set("SKRON_GIT_HTTP_VERSION", "  HTTP/3 ");
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Https, None)
                    .expect("trimmed https version"),
                Some("http3")
            );
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Http, None)
                    .expect("trimmed http version"),
                Some("http1")
            );
        }
    }

    #[test]
    fn remote_http_helper_version_arg_for_url_forces_http1_on_auto_for_local_https() {
        let _guard = TestEnvVarGuard::remove("SKRON_GIT_HTTP_VERSION");

        let local_urls = [
            "https://localhost/repo.git",
            "https://127.0.0.1/repo.git",
            "https://10.0.0.5/repo.git",
            "https://[::1]/repo.git",
            "https://[fc00::1]/repo.git",
        ];
        for url in local_urls {
            let parsed = ParsedHttpUrl::parse(url).expect("local url parse");
            assert_eq!(
                remote_http_helper_version_arg_for_url(HttpScheme::Https, Some(&parsed))
                    .expect("local https auto"),
                Some("http1")
            );
        }

        let remote =
            ParsedHttpUrl::parse("https://github.com/example/repo.git").expect("remote url parse");
        assert_eq!(
            remote_http_helper_version_arg_for_url(HttpScheme::Https, Some(&remote))
                .expect("remote auto"),
            None
        );
        let local_http =
            ParsedHttpUrl::parse("http://localhost/repo.git").expect("local http parse");
        assert_eq!(
            remote_http_helper_version_arg_for_url(HttpScheme::Http, Some(&local_http))
                .expect("local http auto"),
            None
        );
    }

    #[test]
    fn remote_http_helper_version_arg_for_url_keeps_explicit_auto_remote_https() {
        let _guard = TestEnvVarGuard::set("SKRON_GIT_HTTP_VERSION", "auto");
        let remote =
            ParsedHttpUrl::parse("https://github.com/example/repo.git").expect("remote url parse");

        assert_eq!(
            remote_http_helper_version_arg_for_url(HttpScheme::Https, Some(&remote))
                .expect("remote auto"),
            None
        );
    }

    #[test]
    fn remote_http_helper_version_arg_for_url_forces_http1_with_custom_ca() {
        let _guard = TestEnvVarGuard::set_and_remove(
            "GIT_SSL_CAINFO",
            "/tmp/local-ca.pem",
            "SKRON_GIT_HTTP_VERSION",
        );
        let local = ParsedHttpUrl::parse("https://127.0.0.1/repo.git").expect("local url parse");

        assert_eq!(
            remote_http_helper_version_arg_for_url(HttpScheme::Https, Some(&local))
                .expect("local custom ca auto"),
            Some("http1")
        );
    }

    #[test]
    fn remote_http_helper_version_arg_for_url_forces_http1_with_client_cert() {
        let _guard = TestEnvVarGuard::set_and_remove(
            "GIT_SSL_CERT",
            "/tmp/client.pem",
            "SKRON_GIT_HTTP_VERSION",
        );
        let local = ParsedHttpUrl::parse("https://127.0.0.1/repo.git").expect("local url parse");

        assert_eq!(
            remote_http_helper_version_arg_for_url(HttpScheme::Https, Some(&local))
                .expect("local client cert auto"),
            Some("http1")
        );
    }

    #[test]
    fn auto_http3_probe_host_treats_local_ipv6_as_private_loopback() {
        assert!(!auto_http3_probe_host("localhost"));
        assert!(!auto_http3_probe_host("[::1]"));
        assert!(!auto_http3_probe_host("[fc00::1]"));
        assert!(!auto_http3_probe_host("[fe80::1]"));
        assert!(!auto_http3_probe_host("[127.0.0.1]"));
        assert!(auto_http3_probe_host("[2001:4860:4860::8888]"));
        assert!(auto_http3_probe_host("github.com"));
    }

    #[test]
    fn remote_http_helper_version_arg_rejects_unsupported_values() {
        let _guard = TestEnvVarGuard::set("SKRON_GIT_HTTP_VERSION", "h1");

        let error = remote_http_helper_version_arg_for_url(HttpScheme::Https, None)
            .expect_err("unsupported");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "unsupported SKRON_GIT_HTTP_VERSION 'h1'; expected auto, http1, http2, or http3"
        ));
    }

    struct TestEnvVarGuard {
        entries: Vec<(&'static str, Option<OsString>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TestEnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let lock = test_env_lock().lock().expect("env mutex");
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                entries: vec![(key, previous)],
                _lock: lock,
            }
        }

        fn remove(key: &'static str) -> Self {
            let lock = test_env_lock().lock().expect("env mutex");
            let previous = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self {
                entries: vec![(key, previous)],
                _lock: lock,
            }
        }

        fn set_and_remove(
            set_key: &'static str,
            value: impl AsRef<OsStr>,
            remove_key: &'static str,
        ) -> Self {
            let lock = test_env_lock().lock().expect("env mutex");
            let set_previous = std::env::var_os(set_key);
            let remove_previous = std::env::var_os(remove_key);
            unsafe {
                std::env::set_var(set_key, value);
                std::env::remove_var(remove_key);
            }
            Self {
                entries: vec![(set_key, set_previous), (remove_key, remove_previous)],
                _lock: lock,
            }
        }
    }

    fn test_env_lock() -> &'static std::sync::Mutex<()> {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        &ENV_LOCK
    }

    impl Drop for TestEnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                for (key, previous) in self.entries.iter().rev() {
                    if let Some(previous) = previous.as_ref() {
                        std::env::set_var(key, previous);
                    } else {
                        std::env::remove_var(key);
                    }
                }
            }
        }
    }

    #[test]
    fn helper_file_body_reader_removes_owned_file_after_success() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body = HelperHttpBody::File(HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        });

        let text = body
            .with_reader(|reader| {
                let mut text = String::new();
                reader.read_to_string(&mut text).map_err(CliError::Io)?;
                Ok(text)
            })
            .expect("read file body");

        assert_eq!(text, "streamed body");
        assert!(!path.exists());
    }

    #[test]
    fn helper_file_body_reader_removes_owned_file_after_error() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body = HelperHttpBody::File(HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        });

        let result: Result<()> = body.with_reader(|_| {
            Err(CliError::Fatal {
                code: 128,
                message: "parse failed".into(),
            })
        });

        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn helper_file_body_drop_removes_unconsumed_owned_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body = HelperHttpBody::File(HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        });

        drop(body);

        assert!(!path.exists());
    }

    #[test]
    fn helper_file_body_drop_preserves_caller_owned_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body = HelperHttpBody::File(HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: false,
        });

        drop(body);

        assert!(path.exists());
    }

    #[test]
    fn helper_response_file_body_removes_owned_file_after_header_error() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body_file = helper_http_file_body(path.clone(), None).expect("owned helper file");

        drop(body_file);

        assert!(!path.exists());
    }

    #[test]
    fn helper_response_file_body_rejects_unexpected_output_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let expected = temp.path().join("expected-body");
        let unexpected = temp.path().join("unexpected-body");

        let error = match helper_http_file_body(unexpected.clone(), Some(&expected)) {
            Ok(_) => panic!("unexpected output file should fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == &format!(
                "HTTP helper wrote response body to unexpected file: expected {}, got {}",
                expected.display(),
                unexpected.display()
            )
        ));
    }

    #[test]
    fn helper_file_response_body_validates_length_before_streaming() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body_file = HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        };

        let error = match helper_file_response_body(body_file, 4) {
            Ok(_) => panic!("length mismatch should fail"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper response BODY-FILE length mismatch: expected 4, got 13"
        ));
        assert!(!path.exists());
    }

    #[test]
    fn helper_file_response_body_accepts_exact_length() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body_file = HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        };

        let body = helper_file_response_body(body_file, 13).expect("exact file body");

        assert!(matches!(body, HelperHttpBody::File(_)));
        drop(body);
        assert!(!path.exists());
    }

    #[test]
    fn helper_memory_body_into_vec_reuses_inline_bytes() {
        let bytes = b"inline body".to_vec();
        let ptr = bytes.as_ptr();
        let body = HelperHttpBody::Memory(bytes);

        let actual = body.into_vec(11).expect("inline body");

        assert_eq!(actual, b"inline body");
        assert_eq!(actual.as_ptr(), ptr);
    }

    #[test]
    fn helper_file_body_reader_uses_pack_sized_buffer() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"body").expect("write body");
        let file = fs::File::open(&path).expect("open body");

        let reader = http_helper_file_body_reader(file);

        assert_eq!(reader.capacity(), HTTP_HELPER_FILE_READ_BUF_CAPACITY);
    }

    #[test]
    fn helper_file_body_into_vec_removes_owned_file() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"streamed body").expect("write body");
        let body = HelperHttpBody::File(HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        });

        let actual = body.into_vec(13).expect("file body");

        assert_eq!(actual, b"streamed body");
        assert_eq!(actual.capacity(), 13);
        assert!(!path.exists());
    }

    #[test]
    fn helper_file_body_into_vec_rejects_extra_file_bytes() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"body-extra").expect("write body");
        let body = HelperHttpBody::File(HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        });

        let error = body.into_vec(4).expect_err("extra file bytes");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper response BODY-FILE length mismatch: expected 4, got 10"
        ));
        assert!(!path.exists());
    }

    #[test]
    fn helper_file_body_into_vec_rejects_early_eof() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("helper-body");
        fs::write(&path, b"body").expect("write body");
        let body = HelperHttpBody::File(HelperHttpFileBody {
            path: path.clone(),
            remove_on_drop: true,
        });

        let error = body.into_vec(8).expect_err("short file body");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP helper response BODY-FILE length mismatch: expected 8, got 4"
        ));
        assert!(!path.exists());
    }

    #[test]
    fn smart_http_ls_remote_parser_streams_service_advertisement() {
        let id = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let mut body = Vec::new();
        write_pkt_line(&mut body, b"# service=git-upload-pack\n").expect("service");
        body.extend_from_slice(b"0000");
        write_pkt_line(
            &mut body,
            format!("{} HEAD\0multi_ack\n", id.to_hex()).as_bytes(),
        )
        .expect("head");
        write_pkt_line(
            &mut body,
            format!("{} refs/heads/main\n", id.to_hex()).as_bytes(),
        )
        .expect("main");
        body.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(io::Cursor::new(body));

        let rows =
            parse_smart_http_ls_remote_rows_from_reader(&mut reader, false, false, false, &[])
                .expect("parse smart")
                .expect("smart rows");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows.capacity(), HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
        assert_eq!(rows[0].name, "HEAD");
        assert_eq!(rows[1].name, "refs/heads/main");
    }

    #[test]
    fn smart_http_ls_remote_parser_uses_response_size_capacity_hint() {
        let id = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let mut body = Vec::new();
        write_pkt_line(&mut body, b"# service=git-upload-pack\n").expect("service");
        body.extend_from_slice(b"0000");
        write_pkt_line(
            &mut body,
            format!("{} HEAD\0multi_ack\n", id.to_hex()).as_bytes(),
        )
        .expect("head");
        body.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(io::Cursor::new(body));
        let capacity =
            http_remote_ref_rows_capacity_hint(Some(HTTP_REMOTE_REF_ROW_BYTES_HINT * 200), 0);

        let discovery = parse_smart_http_discovery_from_reader_with_capacity(
            &mut reader,
            false,
            false,
            false,
            &[],
            capacity,
        )
        .expect("parse smart")
        .expect("smart discovery");

        assert_eq!(capacity, 200);
        assert_eq!(discovery.rows.len(), 1);
        assert_eq!(discovery.rows.capacity(), capacity);
        assert_eq!(
            http_remote_ref_rows_capacity_hint(Some(usize::MAX), 0),
            TRANSPORT_REF_COLLECTION_CAPACITY_LIMIT
        );
    }

    #[test]
    fn daemon_upload_pack_rows_parser_uses_capacity_hint() {
        let id = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let mut body = Vec::new();
        write_pkt_line(
            &mut body,
            format!("{} HEAD\0multi_ack\n", id.to_hex()).as_bytes(),
        )
        .expect("head");
        write_pkt_line(
            &mut body,
            format!("{} refs/heads/main\n", id.to_hex()).as_bytes(),
        )
        .expect("main");
        body.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(io::Cursor::new(body));

        let rows =
            parse_daemon_upload_pack_rows(&mut reader, false, false, false, &[]).expect("rows");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows.capacity(), HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);
        assert_eq!(rows[0].name, "HEAD");
        assert_eq!(rows[1].name, "refs/heads/main");
    }

    #[test]
    fn upload_pack_advertisement_writes_head_before_sorted_refs_without_extra_rows() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let git_dir = temp.path().join(".git");
        let objects_dir = git_dir.join("objects");
        fs::create_dir_all(&objects_dir).expect("objects dir");
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"advertised object\n")
            .expect("write object");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/z", &id).expect("write z");
        refs.write_ref("refs/heads/main", &id).expect("write main");
        refs.write_head_symbolic("refs/heads/main")
            .expect("write HEAD");

        let mut body = Vec::new();
        write_upload_pack_advertisement(&refs, &mut body).expect("advertisement");

        assert!(String::from_utf8_lossy(&body).contains(" HEAD\0multi_ack"));
        let mut reader = io::BufReader::new(io::Cursor::new(body));
        let rows =
            parse_daemon_upload_pack_rows(&mut reader, false, false, false, &[]).expect("rows");

        assert_eq!(
            rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
            vec!["HEAD", "refs/heads/main", "refs/heads/z"]
        );
    }

    #[test]
    fn local_ls_remote_rows_use_loose_ref_over_packed_ref() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let git_dir = temp.path().join(".git");
        let objects_dir = git_dir.join("objects");
        fs::create_dir_all(&objects_dir).expect("objects dir");
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);
        let packed_id = store
            .write_object(GitObjectKind::Blob, b"packed ref target\n")
            .expect("write packed target");
        let loose_id = store
            .write_object(GitObjectKind::Blob, b"loose ref target\n")
            .expect("write loose target");
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/heads/feature\n", packed_id.to_hex()),
        )
        .expect("write packed refs");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/feature", &loose_id)
            .expect("write loose ref");

        let rows = ls_remote_rows(&refs, &store, true, false, false, &[]).expect("ls remote rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "refs/heads/feature");
        assert_eq!(rows[0].id, loose_id);
    }

    #[test]
    fn prune_missing_tags_keeps_loose_ref_over_stale_packed_ref() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let git_dir = temp.path().join(".git");
        let objects_dir = git_dir.join("objects");
        fs::create_dir_all(&objects_dir).expect("objects dir");
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);
        let stale_id = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let live_id = store
            .write_object(GitObjectKind::Blob, b"live tag target\n")
            .expect("write live target");
        fs::write(
            git_dir.join("packed-refs"),
            format!("{} refs/tags/v1\n", stale_id.to_hex()),
        )
        .expect("write packed refs");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/tags/v1", &live_id)
            .expect("write loose tag ref");

        prune_missing_tag_refs(&refs, &store).expect("prune missing tags");

        assert_eq!(refs.resolve("refs/tags/v1").expect("tag ref"), live_id);
    }

    #[test]
    fn send_pack_mirror_refspecs_streams_source_and_destination_names() {
        let source = tempfile::TempDir::new().expect("source repo");
        let source_git_dir = source.path().join(".git");
        let source_objects_dir = source_git_dir.join("objects");
        fs::create_dir_all(&source_objects_dir).expect("source objects dir");
        let source_store = LooseObjectStore::new(&source_objects_dir, GitHashAlgorithm::Sha1);
        let source_id = source_store
            .write_object(GitObjectKind::Blob, b"source ref target\n")
            .expect("write source target");
        let stale_source_id = source_store
            .write_object(GitObjectKind::Blob, b"stale source target\n")
            .expect("write stale source target");
        fs::write(
            source_git_dir.join("packed-refs"),
            format!("{} refs/heads/main\n", stale_source_id.to_hex()),
        )
        .expect("write source packed refs");
        let source_refs = RefStore::new(&source_git_dir, GitHashAlgorithm::Sha1);
        source_refs
            .write_ref("refs/heads/main", &source_id)
            .expect("write source loose ref");

        let destination = tempfile::TempDir::new().expect("destination repo");
        let destination_git_dir = destination.path().join(".git");
        let destination_objects_dir = destination_git_dir.join("objects");
        fs::create_dir_all(&destination_objects_dir).expect("destination objects dir");
        let destination_store =
            LooseObjectStore::new(&destination_objects_dir, GitHashAlgorithm::Sha1);
        let destination_id = destination_store
            .write_object(GitObjectKind::Blob, b"destination ref target\n")
            .expect("write destination target");
        let destination_refs = RefStore::new(&destination_git_dir, GitHashAlgorithm::Sha1);
        destination_refs
            .write_ref("refs/tags/orphan", &destination_id)
            .expect("write destination ref");

        let specs =
            send_pack_mirror_refspecs(&source_refs, &destination_refs).expect("mirror refspecs");

        assert_eq!(
            specs,
            vec![
                "+refs/heads/main:refs/heads/main".to_owned(),
                ":refs/tags/orphan".to_owned(),
            ]
        );
    }

    #[test]
    fn receive_pack_advertisement_streams_sorted_refs_with_capabilities() {
        let temp = tempfile::TempDir::new().expect("temp repo");
        let git_dir = temp.path().join(".git");
        let objects_dir = git_dir.join("objects");
        fs::create_dir_all(&objects_dir).expect("objects dir");
        let store = LooseObjectStore::new(&objects_dir, GitHashAlgorithm::Sha1);
        let id = store
            .write_object(GitObjectKind::Blob, b"receive advertised object\n")
            .expect("write object");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref("refs/heads/z", &id).expect("write z");
        refs.write_ref("refs/heads/main", &id).expect("write main");

        let mut body = Vec::new();
        write_receive_pack_advertisement(&refs, &mut body).expect("advertisement");
        let mut reader = io::BufReader::new(io::Cursor::new(body));
        let advertisement = parse_receive_pack_advertisement(&mut reader).expect("advertisement");

        assert_eq!(
            advertisement
                .refs
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["refs/heads/main", "refs/heads/z"]
        );
        assert!(advertisement.capabilities.contains("report-status"));
        assert!(advertisement.capabilities.contains("delete-refs"));
    }

    #[test]
    fn ls_remote_row_parsers_filter_before_string_rows() {
        let id = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let mut smart_body = Vec::new();
        write_pkt_line(&mut smart_body, b"# service=git-upload-pack\n").expect("service");
        smart_body.extend_from_slice(b"0000");
        write_pkt_line(
            &mut smart_body,
            format!("{} HEAD\0multi_ack\n", id.to_hex()).as_bytes(),
        )
        .expect("head");
        write_pkt_line(
            &mut smart_body,
            format!("{} refs/heads/main\n", id.to_hex()).as_bytes(),
        )
        .expect("main");
        write_pkt_line(
            &mut smart_body,
            format!("{} refs/tags/v1\n", id.to_hex()).as_bytes(),
        )
        .expect("tag");
        smart_body.extend_from_slice(b"0000");
        let mut smart_reader = io::BufReader::new(io::Cursor::new(smart_body));

        let smart_rows = parse_smart_http_ls_remote_rows_from_reader(
            &mut smart_reader,
            true,
            false,
            false,
            &["main".to_owned()],
        )
        .expect("parse smart")
        .expect("smart rows");

        assert_eq!(smart_rows.len(), 1);
        assert_eq!(smart_rows[0].name, "refs/heads/main");

        let dumb_body = format!(
            "{}\tHEAD\n{}\trefs/heads/main\n{}\trefs/tags/v1\n",
            id.to_hex(),
            id.to_hex(),
            id.to_hex()
        );
        let mut dumb_reader = io::BufReader::new(io::Cursor::new(dumb_body.into_bytes()));
        let mut dumb_rows = Vec::with_capacity(HTTP_REMOTE_REF_ROWS_CAPACITY_HINT);

        parse_dumb_http_info_refs_rows_from_reader(
            &mut dumb_reader,
            &mut dumb_rows,
            false,
            true,
            false,
            &["v1".to_owned()],
        )
        .expect("parse dumb");

        assert_eq!(dumb_rows.len(), 1);
        assert_eq!(dumb_rows[0].name, "refs/tags/v1");
    }

    #[test]
    fn smart_http_discovery_parser_reads_head_symref() {
        let id = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let mut body = Vec::new();
        write_pkt_line(&mut body, b"# service=git-upload-pack\n").expect("service");
        body.extend_from_slice(b"0000");
        write_pkt_line(
            &mut body,
            format!(
                "{} HEAD\0multi_ack symref=HEAD:refs/heads/main\n",
                id.to_hex()
            )
            .as_bytes(),
        )
        .expect("head");
        write_pkt_line(
            &mut body,
            format!("{} refs/heads/main\n", id.to_hex()).as_bytes(),
        )
        .expect("main");
        body.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(io::Cursor::new(body));

        let discovery =
            parse_smart_http_discovery_from_reader(&mut reader, false, false, false, &[])
                .expect("parse smart")
                .expect("smart discovery");

        assert_eq!(discovery.head_branch.as_deref(), Some("main"));
        assert_eq!(discovery.rows.len(), 2);
        assert_eq!(
            discovery.rows.capacity(),
            HTTP_REMOTE_REF_ROWS_CAPACITY_HINT
        );
    }

    #[test]
    fn smart_http_discovery_parser_accepts_partial_service_header_reads() {
        struct OneByteReader {
            bytes: io::Cursor<Vec<u8>>,
        }

        impl Read for OneByteReader {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                if output.is_empty() {
                    return Ok(0);
                }
                self.bytes.read(&mut output[..1])
            }
        }

        let id = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let mut body = Vec::new();
        write_pkt_line(&mut body, b"# service=git-upload-pack\n").expect("service");
        body.extend_from_slice(b"0000");
        write_pkt_line(
            &mut body,
            format!("{} HEAD\0multi_ack\n", id.to_hex()).as_bytes(),
        )
        .expect("head");
        body.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(OneByteReader {
            bytes: io::Cursor::new(body),
        });

        let discovery =
            parse_smart_http_discovery_from_reader(&mut reader, false, false, false, &[])
                .expect("parse smart")
                .expect("smart discovery");

        assert_eq!(discovery.rows.len(), 1);
        assert_eq!(discovery.rows[0].name, "HEAD");
    }

    #[test]
    fn smart_http_receive_pack_parser_streams_service_advertisement() {
        let id = oid("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        let mut body = Vec::new();
        write_pkt_line(&mut body, b"# service=git-receive-pack\n").expect("service");
        body.extend_from_slice(b"0000");
        write_pkt_line(
            &mut body,
            format!(
                "{} refs/heads/main\0report-status delete-refs\n",
                id.to_hex()
            )
            .as_bytes(),
        )
        .expect("main");
        body.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(io::Cursor::new(body));

        let advertisement = parse_smart_http_receive_pack_advertisement_from_reader(&mut reader)
            .expect("parse smart")
            .expect("advertisement");

        assert_eq!(advertisement.refs.get("refs/heads/main"), Some(&id));
        assert!(advertisement.capabilities.contains("report-status"));
        assert!(advertisement.capabilities.contains("delete-refs"));
    }

    #[test]
    fn receive_pack_advertisement_parser_uses_byte_rows() {
        let first = oid("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        let second = oid("cccccccccccccccccccccccccccccccccccccccc");
        let mut body = Vec::new();
        write_pkt_line(
            &mut body,
            format!(
                "{} refs/heads/main\0report-status delete-refs ofs-delta\n",
                first.to_hex()
            )
            .as_bytes(),
        )
        .expect("main");
        write_pkt_line(
            &mut body,
            format!("{} refs/heads/topic\n", second.to_hex()).as_bytes(),
        )
        .expect("topic");
        body.extend_from_slice(b"0000");
        let mut reader = io::BufReader::new(io::Cursor::new(body));

        let advertisement = parse_receive_pack_advertisement(&mut reader).expect("advertisement");

        assert_eq!(advertisement.refs.get("refs/heads/main"), Some(&first));
        assert_eq!(advertisement.refs.get("refs/heads/topic"), Some(&second));
        assert!(advertisement.capabilities.contains("report-status"));
        assert!(advertisement.capabilities.contains("delete-refs"));
        assert!(advertisement.capabilities.contains("ofs-delta"));
    }

    #[test]
    fn smart_http_service_reader_rejects_dumb_ref_body_without_allocating_payload() {
        let mut reader = io::BufReader::new(io::Cursor::new(
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/main\n".to_vec(),
        ));

        let rows =
            parse_smart_http_ls_remote_rows_from_reader(&mut reader, false, false, false, &[])
                .expect("parse dumb as non-smart");

        assert!(rows.is_none());
    }

    #[test]
    fn http_response_body_reader_preserves_raw_bytes_without_string_roundtrip() {
        let mut reader = io::Cursor::new(b"ref: refs/heads/main\n\xff".to_vec());
        let mut out = Vec::new();

        read_http_response_body_to_vec(&mut reader, &mut out).expect("read body");

        assert_eq!(out, b"ref: refs/heads/main\n\xff");
    }

    #[test]
    fn http_response_body_drain_maps_early_eof_like_vec_reader() {
        let mut body = FixedLengthHttpBody {
            reader: io::BufReader::new(io::Cursor::new(b"bo".to_vec())),
            remaining: 4,
        };

        let error = drain_http_response_body(&mut body).expect_err("early eof");

        assert!(matches!(
            error,
            CliError::Fatal {
                code: 128,
                ref message
            } if message == "HTTP response ended early"
        ));
    }

    #[test]
    fn http_chunk_size_parser_accepts_extensions_and_detects_overflow() {
        assert_eq!(parse_http_chunk_size(b"a;name=value\r\n").unwrap(), 10);
        assert_eq!(parse_http_chunk_size(b"0\r\n").unwrap(), 0);

        let error = parse_http_chunk_size(b"ffffffffffffffffffffffffffffffff\r\n")
            .expect_err("oversized chunk");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "HTTP chunk too large");
    }

    #[test]
    fn decimal_usize_parser_detects_invalid_and_overflow_values() {
        assert_eq!(parse_decimal_usize(b"12345"), Some(12_345));
        assert_eq!(parse_decimal_usize(b""), None);
        assert_eq!(parse_decimal_usize(b"12x"), None);
        assert_eq!(
            parse_decimal_usize(b"999999999999999999999999999999999999"),
            None
        );
    }

    #[test]
    fn decimal_u16_parser_detects_invalid_and_overflow_values() {
        assert_eq!(parse_decimal_u16(b"200"), Some(200));
        assert_eq!(parse_decimal_u16(b""), None);
        assert_eq!(parse_decimal_u16(b"20x"), None);
        assert_eq!(parse_decimal_u16(b"999999"), None);
    }

    #[test]
    fn chunked_http_body_reuses_line_buffer_for_chunks_and_trailers() {
        let reader = io::BufReader::new(io::Cursor::new(
            b"4;ext=value\r\nbody\r\n0\r\nGit-Trace: done\r\n\r\n".to_vec(),
        ));
        let mut body = ChunkedHttpBody {
            reader,
            line: String::with_capacity(128),
            remaining: 0,
            done: false,
        };
        let line_capacity = body.line.capacity();
        let mut out = Vec::new();

        body.read_to_end(&mut out).expect("read chunked body");

        assert_eq!(out, b"body");
        assert_eq!(body.line.capacity(), line_capacity);
        assert!(body.done);
    }

    #[test]
    fn dumb_http_info_refs_parser_streams_rows() {
        let head = oid("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let tag = oid("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        let body = format!(
            "{}\trefs/heads/main\n{}\trefs/tags/v1.0\n{}\trefs/tags/v1.0^{{}}\n",
            head.to_hex(),
            tag.to_hex(),
            head.to_hex()
        );
        let mut reader = io::BufReader::new(io::Cursor::new(body.into_bytes()));
        let mut rows = Vec::new();

        parse_dumb_http_info_refs_rows_from_reader(&mut reader, &mut rows, false, false, true, &[])
            .expect("parse dumb info refs");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "refs/heads/main");
        assert_eq!(rows[1].name, "refs/tags/v1.0");
    }

    #[test]
    fn pkt_line_payload_read_is_exact_sized() {
        let mut reader = io::Cursor::new(b"payload-next".to_vec());

        let payload = read_exact_payload_to_vec(&mut reader, 7).expect("payload");

        assert_eq!(payload, b"payload");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).expect("remaining bytes");
        assert_eq!(rest, b"-next");
    }

    #[test]
    fn pkt_line_payload_initial_capacity_is_bounded() {
        assert_eq!(pkt_line_payload_initial_capacity(0), 0);
        assert_eq!(pkt_line_payload_initial_capacity(2), 2);
        assert_eq!(
            pkt_line_payload_initial_capacity(usize::MAX),
            PKT_LINE_PAYLOAD_INITIAL_CAPACITY_LIMIT
        );
    }

    #[test]
    fn pkt_line_payload_read_reports_early_eof() {
        let mut reader = io::Cursor::new(b"pay".to_vec());

        let error = read_exact_payload_to_vec(&mut reader, 7).expect_err("early eof");

        assert!(matches!(error, CliError::Io(ref io_error)
            if io_error.kind() == io::ErrorKind::UnexpectedEof
                && io_error.to_string() == "pkt-line payload ended early"));
    }

    #[test]
    fn sideband_pack_stream_uses_caller_buffer() {
        let mut reader = io::Cursor::new(b"PACK-body-rest".to_vec());
        let mut writer = Vec::new();
        let mut first_bytes = [0_u8; 4];
        let mut first_bytes_len = 0_usize;
        let mut buffer = [0_u8; 3];

        stream_sideband_payload_to_pack(
            &mut reader,
            9,
            &mut writer,
            &mut first_bytes,
            &mut first_bytes_len,
            &mut buffer,
        )
        .expect("stream pack payload");

        assert_eq!(writer, b"PACK-body");
        assert_eq!(first_bytes, *b"PACK");
        assert_eq!(first_bytes_len, 4);
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).expect("remaining bytes");
        assert_eq!(rest, b"-rest");
    }

    #[test]
    fn sideband_pack_writer_does_not_need_payload_vec() {
        let mut out = Vec::new();

        write_sideband_pack(&mut out, b"PACK-body").expect("write sideband pack");

        assert_eq!(out, b"000e\x01PACK-body0000");
    }

    #[test]
    fn sideband_pack_reader_writer_frames_chunks() {
        let mut input = vec![0x41; SIDEBAND_PACK_CHUNK_SIZE + 1];
        let mut reader = io::Cursor::new(std::mem::take(&mut input));
        let mut out = Vec::new();

        write_sideband_pack_from_reader(&mut out, &mut reader).expect("write sideband pack");

        assert_eq!(&out[..4], b"fff5");
        assert_eq!(out[4], 1);
        assert_eq!(
            out[5..5 + SIDEBAND_PACK_CHUNK_SIZE],
            vec![0x41; SIDEBAND_PACK_CHUNK_SIZE]
        );
        let second = 5 + SIDEBAND_PACK_CHUNK_SIZE;
        assert_eq!(&out[second..second + 4], b"0006");
        assert_eq!(out[second + 4], 1);
        assert_eq!(out[second + 5], 0x41);
        assert_eq!(&out[second + 6..], b"0000");
    }

    #[test]
    fn sideband_discard_uses_caller_buffer_exactly() {
        let mut reader = io::Cursor::new(b"progress-rest".to_vec());
        let mut buffer = [0_u8; 4];

        discard_exact_payload_with_buffer(&mut reader, 8, &mut buffer).expect("discard payload");

        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).expect("remaining bytes");
        assert_eq!(rest, b"-rest");
    }
}
