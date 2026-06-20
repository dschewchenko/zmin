use super::*;
use chrono::{Datelike, Timelike};

#[derive(Debug, Clone)]
pub(crate) struct ArchiveOptions {
    pub(crate) format: Option<String>,
    pub(crate) prefix: Option<String>,
    pub(crate) output: Option<PathBuf>,
    pub(crate) add_files: Vec<PathBuf>,
    pub(crate) add_virtual_files: Vec<String>,
    pub(crate) mtime: Option<String>,
    pub(crate) list: bool,
    pub(crate) verbose: bool,
    pub(crate) treeish: Option<String>,
    pub(crate) paths: Vec<String>,
}

pub(crate) fn get_tar_commit_id() -> Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let commit = parse_tar_commit_id(&input).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "git get-tar-commit-id: EOF before reading tar header: No such file or directory"
            .into(),
    })?;
    println!("{commit}");
    Ok(())
}

fn parse_tar_commit_id(input: &[u8]) -> Option<String> {
    let header = input.get(..512)?;
    if header.iter().all(|byte| *byte == 0) {
        return None;
    }
    let name = tar_header_string(&header[..100]);
    if name != "pax_global_header" || header.get(156) != Some(&b'g') {
        return None;
    }
    let size = parse_tar_octal(header.get(124..136)?)?;
    let payload_start = 512usize;
    let payload_end = payload_start.checked_add(size)?;
    let payload = input.get(payload_start..payload_end)?;
    parse_pax_comment(payload)
}

fn tar_header_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn parse_tar_octal(bytes: &[u8]) -> Option<usize> {
    let raw = bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if raw.is_empty() || !raw.iter().all(|byte| matches!(byte, b'0'..=b'7')) {
        return None;
    }
    usize::from_str_radix(std::str::from_utf8(&raw).ok()?, 8).ok()
}

fn parse_pax_comment(payload: &[u8]) -> Option<String> {
    for line in payload.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let space = line.iter().position(|byte| *byte == b' ')?;
        let length = std::str::from_utf8(&line[..space])
            .ok()?
            .parse::<usize>()
            .ok()?;
        if length != line.len() + 1 {
            continue;
        }
        let record = &line[space + 1..];
        let Some(value) = record.strip_prefix(b"comment=") else {
            continue;
        };
        let value = std::str::from_utf8(value).ok()?;
        if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some(value.to_owned());
        }
    }
    None
}

pub(crate) fn archive(options: ArchiveOptions) -> Result<()> {
    if options.list {
        println!("tar");
        println!("tgz");
        println!("tar.gz");
        println!("zip");
        return Ok(());
    }
    let format = ArchiveFormat::parse(options.format.as_deref().unwrap_or("tar"))?;
    let Some(treeish) = options.treeish.as_deref() else {
        return Err(CliError::Fatal {
            code: 129,
            message: "archive requires a tree-ish".into(),
        });
    };
    let repo = find_repo_or_bare()?;
    let out = archive_to_bytes(&repo, &options, treeish, format)?;
    if let Some(path) = options.output {
        fs::write(path, out)?;
    } else {
        // `git archive` uses stdout as the archive data channel when no output
        // path is given. This is command output, not diagnostic logging.
        let mut stdout = io::stdout().lock();
        io::copy(&mut std::io::Cursor::new(out), &mut stdout)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArchiveFormat {
    Tar,
    Tgz,
    Zip,
}

impl ArchiveFormat {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "tar" => Ok(Self::Tar),
            "tgz" | "tar.gz" => Ok(Self::Tgz),
            "zip" => Ok(Self::Zip),
            other => Err(CliError::Fatal {
                code: 128,
                message: format!("Unknown archive format '{other}'"),
            }),
        }
    }
}

fn archive_to_bytes(
    repo: &GitRepo,
    options: &ArchiveOptions,
    treeish: &str,
    format: ArchiveFormat,
) -> Result<Vec<u8>> {
    let tar = archive_to_tar_bytes(repo, options, treeish)?;
    match format {
        ArchiveFormat::Tar => Ok(tar),
        ArchiveFormat::Tgz => {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&tar)?;
            encoder.finish().map_err(CliError::Io)
        }
        ArchiveFormat::Zip => archive_to_zip_bytes(repo, options, treeish),
    }
}

fn archive_to_tar_bytes(
    repo: &GitRepo,
    options: &ArchiveOptions,
    treeish: &str,
) -> Result<Vec<u8>> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let source = archive_tree_source(repo, &store, treeish)?;
    let tree_cache = TreeObjectCache::new(&store);
    let mtime = match options.mtime.as_deref() {
        Some(value) => parse_archive_mtime(value)?,
        None => source.mtime,
    };
    let prefix = normalize_archive_prefix(options.prefix.as_deref().unwrap_or_default());
    let path_filters = options
        .paths
        .iter()
        .map(|path| normalize_git_path(path).map_err(CliError::Io))
        .collect::<Result<Vec<_>>>()?;
    let mut out = Vec::new();
    if let Some(commit_id) = source.commit_id.as_deref() {
        write_tar_pax_global_header(&mut out, commit_id, mtime)?;
    }
    if !prefix.is_empty() {
        write_tar_header(&mut out, &prefix, b"", TarEntryKind::Directory, 0, mtime)?;
    }
    let checkout_metadata = archive_checkout_metadata(repo, treeish, &source.treeish_id)?;
    let context = ArchiveTreeContext {
        repo,
        store: &store,
        tree_cache: &tree_cache,
        checkout_metadata: &checkout_metadata,
        prefix: &prefix,
        mtime,
        verbose: options.verbose,
    };
    if path_filters.is_empty() {
        archive_tree_entries(&context, &source.tree_id, "", &mut out)?;
    } else {
        for path in path_filters {
            if path.is_empty() {
                archive_tree_entries(&context, &source.tree_id, "", &mut out)?;
                continue;
            }
            let Some(entry) = find_tree_entry(&store, &source.tree_id, path.as_bytes())? else {
                continue;
            };
            archive_entry(&context, &entry, &path, &mut out)?;
        }
    }
    for path in &options.add_files {
        archive_add_file(path.as_path(), &prefix, mtime, options.verbose, &mut out)?;
    }
    for value in &options.add_virtual_files {
        archive_add_virtual_file(value.as_str(), mtime, options.verbose, &mut out)?;
    }
    out.extend_from_slice(&[0u8; 1024]);
    Ok(out)
}

fn archive_to_zip_bytes(
    repo: &GitRepo,
    options: &ArchiveOptions,
    treeish: &str,
) -> Result<Vec<u8>> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let source = archive_tree_source(repo, &store, treeish)?;
    let tree_cache = TreeObjectCache::new(&store);
    let mtime = match options.mtime.as_deref() {
        Some(value) => parse_archive_mtime(value)?,
        None => source.mtime,
    };
    let prefix = normalize_archive_prefix(options.prefix.as_deref().unwrap_or_default());
    let path_filters = options
        .paths
        .iter()
        .map(|path| normalize_git_path(path).map_err(CliError::Io))
        .collect::<Result<Vec<_>>>()?;
    let mut zip = ZipArchiveWriter::new();
    if !prefix.is_empty() {
        zip.add_directory(&prefix, mtime)?;
    }
    let checkout_metadata = archive_checkout_metadata(repo, treeish, &source.treeish_id)?;
    let context = ArchiveTreeContext {
        repo,
        store: &store,
        tree_cache: &tree_cache,
        checkout_metadata: &checkout_metadata,
        prefix: &prefix,
        mtime,
        verbose: options.verbose,
    };
    if path_filters.is_empty() {
        archive_tree_entries_zip(&context, &source.tree_id, "", &mut zip)?;
    } else {
        for path in path_filters {
            if path.is_empty() {
                archive_tree_entries_zip(&context, &source.tree_id, "", &mut zip)?;
                continue;
            }
            let Some(entry) = find_tree_entry(&store, &source.tree_id, path.as_bytes())? else {
                continue;
            };
            archive_entry_zip(&context, &entry, &path, &mut zip)?;
        }
    }
    for path in &options.add_files {
        archive_add_file_zip(path.as_path(), &prefix, mtime, options.verbose, &mut zip)?;
    }
    for value in &options.add_virtual_files {
        archive_add_virtual_file_zip(value.as_str(), mtime, options.verbose, &mut zip)?;
    }
    zip.finish()
}

fn parse_archive_mtime(value: &str) -> Result<u64> {
    if value.eq_ignore_ascii_case("never") {
        return Ok(0);
    }
    if value.eq_ignore_ascii_case("now") {
        return Ok(current_archive_timestamp());
    }
    if let Some(timestamp) = parse_archive_unix_timestamp(value) {
        return Ok(timestamp);
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(nonnegative_archive_timestamp(datetime.timestamp()));
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S %z") {
        return Ok(nonnegative_archive_timestamp(datetime.timestamp()));
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_str(value, "%b %e %Y %H:%M:%S %z") {
        return Ok(nonnegative_archive_timestamp(datetime.timestamp()));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
        if let Some(datetime) = date.and_hms_opt(now.hour(), now.minute(), now.second()) {
            return Ok(nonnegative_archive_timestamp(
                datetime.and_utc().timestamp(),
            ));
        }
    }
    Ok(current_archive_timestamp())
}

fn parse_archive_unix_timestamp(value: &str) -> Option<u64> {
    let raw = value.strip_prefix('@').unwrap_or(value);
    if raw.len() < 9 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    raw.parse::<u64>().ok()
}

fn current_archive_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn nonnegative_archive_timestamp(timestamp: i64) -> u64 {
    u64::try_from(timestamp).unwrap_or(0)
}

struct ArchiveTreeSource {
    tree_id: ObjectId,
    commit_id: Option<String>,
    treeish_id: ObjectId,
    mtime: u64,
}

fn archive_tree_source(
    repo: &GitRepo,
    store: &LooseObjectStore,
    treeish: &str,
) -> Result<ArchiveTreeSource> {
    let object_id = resolve_objectish(repo, treeish)?;
    let object = store.read_object(&object_id)?;
    let commit_cache = CommitObjectCache::new(store);
    match object.kind {
        GitObjectKind::Commit => {
            let commit = commit_cache.read_commit(&object_id)?;
            let signature = signature_from_commit_bytes(&commit.committer)?;
            Ok(ArchiveTreeSource {
                tree_id: commit.tree.clone(),
                commit_id: Some(object_id.to_hex()),
                treeish_id: object_id,
                mtime: signature.timestamp as u64,
            })
        }
        GitObjectKind::Tree => Ok(ArchiveTreeSource {
            tree_id: object_id.clone(),
            commit_id: None,
            treeish_id: object_id,
            mtime: 0,
        }),
        GitObjectKind::Tag => {
            let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
            let target = store.read_object(&tag.target)?;
            if target.kind == GitObjectKind::Commit {
                let commit = commit_cache.read_commit(&tag.target)?;
                let signature = signature_from_commit_bytes(&commit.committer)?;
                Ok(ArchiveTreeSource {
                    tree_id: commit.tree.clone(),
                    commit_id: Some(tag.target.to_hex()),
                    treeish_id: tag.target,
                    mtime: signature.timestamp as u64,
                })
            } else if target.kind == GitObjectKind::Tree {
                Ok(ArchiveTreeSource {
                    tree_id: tag.target.clone(),
                    commit_id: None,
                    treeish_id: tag.target,
                    mtime: 0,
                })
            } else {
                Err(CliError::Fatal {
                    code: 128,
                    message: "archive tag does not point to a commit or tree".into(),
                })
            }
        }
        GitObjectKind::Blob => Err(CliError::Fatal {
            code: 128,
            message: "archive tree-ish resolved to a blob".into(),
        }),
    }
}

fn archive_checkout_metadata(
    repo: &GitRepo,
    treeish: &str,
    treeish_id: &ObjectId,
) -> Result<WorktreeCheckoutMetadata> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let ref_name = archive_treeish_ref(&refs, treeish)?;
    Ok(WorktreeCheckoutMetadata {
        ref_name,
        treeish: Some(treeish_id.clone()),
    })
}

fn archive_treeish_ref(refs: &RefStore, treeish: &str) -> Result<Option<String>> {
    if let Some(ref_name) = branch_checkout_ref(refs, treeish)? {
        return Ok(Some(ref_name));
    }
    let ref_name = match tag_ref_name(treeish) {
        Ok(ref_name) => ref_name,
        Err(_) => return Ok(None),
    };
    ref_exists(refs, &ref_name).map(|exists| exists.then_some(ref_name))
}

fn normalize_archive_prefix(prefix: &str) -> String {
    if prefix.is_empty() || prefix.ends_with('/') {
        prefix.to_owned()
    } else {
        format!("{prefix}/")
    }
}

struct ArchiveTreeContext<'a> {
    repo: &'a GitRepo,
    store: &'a LooseObjectStore,
    tree_cache: &'a TreeObjectCache<'a, LooseObjectStore>,
    checkout_metadata: &'a WorktreeCheckoutMetadata,
    prefix: &'a str,
    mtime: u64,
    verbose: bool,
}

fn archive_tree_entries(
    context: &ArchiveTreeContext<'_>,
    tree_id: &ObjectId,
    base: &str,
    out: &mut Vec<u8>,
) -> Result<()> {
    for entry in context.tree_cache.read_tree(tree_id)?.iter() {
        let entry_name = String::from_utf8_lossy(&entry.name);
        let path = if base.is_empty() {
            entry_name.into_owned()
        } else {
            format!("{base}/{entry_name}")
        };
        archive_entry(context, entry, &path, out)?;
    }
    Ok(())
}

fn archive_entry(
    context: &ArchiveTreeContext<'_>,
    entry: &TreeEntry,
    path: &str,
    out: &mut Vec<u8>,
) -> Result<()> {
    let archive_path = format!("{}{}", context.prefix, path);
    if context.verbose {
        eprintln!("{archive_path}");
    }
    match entry.mode {
        TreeMode::Tree => {
            let dir_path = format!("{archive_path}/");
            write_tar_header(
                out,
                &dir_path,
                b"",
                TarEntryKind::Directory,
                0,
                context.mtime,
            )?;
            archive_tree_entries(context, &entry.id, path, out)
        }
        TreeMode::File | TreeMode::Executable => {
            let object = context.store.read_object(&entry.id)?;
            let content = smudge_worktree_filter_content(
                context.repo,
                path.as_bytes(),
                &entry.id,
                context.checkout_metadata,
                object.content,
            )?;
            write_tar_header(
                out,
                &archive_path,
                &content,
                TarEntryKind::File(entry.mode == TreeMode::Executable),
                content.len() as u64,
                context.mtime,
            )
        }
        TreeMode::Symlink => {
            let object = context.store.read_object(&entry.id)?;
            write_tar_header(
                out,
                &archive_path,
                &object.content,
                TarEntryKind::Symlink(String::from_utf8_lossy(&object.content).into_owned()),
                0,
                context.mtime,
            )
        }
        TreeMode::Gitlink => Ok(()),
    }
}

fn archive_tree_entries_zip(
    context: &ArchiveTreeContext<'_>,
    tree_id: &ObjectId,
    base: &str,
    zip: &mut ZipArchiveWriter,
) -> Result<()> {
    for entry in context.tree_cache.read_tree(tree_id)?.iter() {
        let entry_name = String::from_utf8_lossy(&entry.name);
        let path = if base.is_empty() {
            entry_name.into_owned()
        } else {
            format!("{base}/{entry_name}")
        };
        archive_entry_zip(context, entry, &path, zip)?;
    }
    Ok(())
}

fn archive_entry_zip(
    context: &ArchiveTreeContext<'_>,
    entry: &TreeEntry,
    path: &str,
    zip: &mut ZipArchiveWriter,
) -> Result<()> {
    let archive_path = format!("{}{}", context.prefix, path);
    if context.verbose {
        eprintln!("{archive_path}");
    }
    match entry.mode {
        TreeMode::Tree => {
            let dir_path = format!("{archive_path}/");
            zip.add_directory(&dir_path, context.mtime)?;
            archive_tree_entries_zip(context, &entry.id, path, zip)
        }
        TreeMode::File | TreeMode::Executable => {
            let object = context.store.read_object(&entry.id)?;
            let content = smudge_worktree_filter_content(
                context.repo,
                path.as_bytes(),
                &entry.id,
                context.checkout_metadata,
                object.content,
            )?;
            zip.add_file(
                &archive_path,
                &content,
                if entry.mode == TreeMode::Executable {
                    0o100755
                } else {
                    0o100644
                },
                context.mtime,
            )
        }
        TreeMode::Symlink => {
            let object = context.store.read_object(&entry.id)?;
            zip.add_file(&archive_path, &object.content, 0o120000, context.mtime)
        }
        TreeMode::Gitlink => Ok(()),
    }
}

fn archive_add_file(
    path: &std::path::Path,
    prefix: &str,
    mtime: u64,
    verbose: bool,
    out: &mut Vec<u8>,
) -> Result<()> {
    let content = fs::read(path)?;
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "archive --add-file path has no file name: {}",
                path.display()
            ),
        });
    };
    let archive_path = format!("{prefix}{name}");
    if verbose {
        eprintln!("{archive_path}");
    }
    write_tar_header(
        out,
        &archive_path,
        &content,
        TarEntryKind::File(path_is_executable(path)),
        content.len() as u64,
        mtime,
    )
}

fn archive_add_file_zip(
    path: &std::path::Path,
    prefix: &str,
    mtime: u64,
    verbose: bool,
    zip: &mut ZipArchiveWriter,
) -> Result<()> {
    let content = fs::read(path)?;
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "archive --add-file path has no file name: {}",
                path.display()
            ),
        });
    };
    let archive_path = format!("{prefix}{name}");
    if verbose {
        eprintln!("{archive_path}");
    }
    zip.add_file(
        &archive_path,
        &content,
        if path_is_executable(path) {
            0o100755
        } else {
            0o100644
        },
        mtime,
    )
}

fn archive_add_virtual_file(
    value: &str,
    mtime: u64,
    verbose: bool,
    out: &mut Vec<u8>,
) -> Result<()> {
    let Some((path, content)) = value.split_once(':') else {
        return Err(CliError::Fatal {
            code: 128,
            message: "archive --add-virtual-file expects <path>:<content>".into(),
        });
    };
    if path.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "archive --add-virtual-file path cannot be empty".into(),
        });
    }
    if verbose {
        eprintln!("{path}");
    }
    write_tar_header(
        out,
        path,
        content.as_bytes(),
        TarEntryKind::File(false),
        content.len() as u64,
        mtime,
    )
}

fn archive_add_virtual_file_zip(
    value: &str,
    mtime: u64,
    verbose: bool,
    zip: &mut ZipArchiveWriter,
) -> Result<()> {
    let Some((path, content)) = value.split_once(':') else {
        return Err(CliError::Fatal {
            code: 128,
            message: "archive --add-virtual-file expects <path>:<content>".into(),
        });
    };
    if path.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "archive --add-virtual-file path cannot be empty".into(),
        });
    }
    if verbose {
        eprintln!("{path}");
    }
    zip.add_file(path, content.as_bytes(), 0o100644, mtime)
}

#[cfg(unix)]
fn path_is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn path_is_executable(_path: &std::path::Path) -> bool {
    false
}

#[derive(Debug, Clone)]
enum TarEntryKind {
    File(bool),
    Directory,
    Symlink(String),
    PaxGlobal,
}

fn write_tar_pax_global_header(out: &mut Vec<u8>, commit_id: &str, mtime: u64) -> Result<()> {
    let record = pax_record("comment", commit_id);
    write_tar_header(
        out,
        "pax_global_header",
        record.as_bytes(),
        TarEntryKind::PaxGlobal,
        record.len() as u64,
        mtime,
    )
}

fn pax_record(key: &str, value: &str) -> String {
    let mut len = key.len() + value.len() + 4;
    loop {
        let record = format!("{len} {key}={value}\n");
        if record.len() == len {
            return record;
        }
        len = record.len();
    }
}

fn write_tar_header(
    out: &mut Vec<u8>,
    path: &str,
    content: &[u8],
    kind: TarEntryKind,
    size: u64,
    mtime: u64,
) -> Result<()> {
    let mut header = [0u8; 512];
    let (name, prefix) = split_tar_name(path)?;
    write_tar_bytes(&mut header[0..100], name.as_bytes());
    write_tar_octal(&mut header[100..108], tar_mode(&kind));
    write_tar_octal(&mut header[108..116], 0);
    write_tar_octal(&mut header[116..124], 0);
    write_tar_octal(&mut header[124..136], size);
    write_tar_octal(&mut header[136..148], mtime);
    header[148..156].fill(b' ');
    header[156] = match &kind {
        TarEntryKind::File(_) => b'0',
        TarEntryKind::Directory => b'5',
        TarEntryKind::Symlink(_) => b'2',
        TarEntryKind::PaxGlobal => b'g',
    };
    if let TarEntryKind::Symlink(target) = &kind {
        write_tar_bytes(&mut header[157..257], target.as_bytes());
    }
    write_tar_bytes(&mut header[257..263], b"ustar\0");
    write_tar_bytes(&mut header[263..265], b"00");
    write_tar_bytes(&mut header[265..297], b"root");
    write_tar_bytes(&mut header[297..329], b"root");
    write_tar_bytes(&mut header[345..500], prefix.as_bytes());
    let checksum = header.iter().map(|byte| u32::from(*byte)).sum::<u32>();
    write_tar_checksum(&mut header[148..156], checksum);
    out.extend_from_slice(&header);
    if matches!(kind, TarEntryKind::File(_) | TarEntryKind::PaxGlobal) {
        out.extend_from_slice(content);
        let padding = (512 - (content.len() % 512)) % 512;
        out.extend(std::iter::repeat_n(0, padding));
    }
    Ok(())
}

fn tar_mode(kind: &TarEntryKind) -> u64 {
    match kind {
        TarEntryKind::File(true) => 0o775,
        TarEntryKind::File(false) => 0o664,
        TarEntryKind::Directory => 0o775,
        TarEntryKind::Symlink(_) => 0o777,
        TarEntryKind::PaxGlobal => 0o664,
    }
}

fn split_tar_name(path: &str) -> Result<(&str, &str)> {
    let bytes = path.as_bytes();
    if bytes.len() <= 100 {
        return Ok((path, ""));
    }
    for (idx, byte) in bytes.iter().enumerate().rev() {
        if *byte == b'/' && idx <= 155 && bytes.len() - idx - 1 <= 100 {
            return Ok((&path[idx + 1..], &path[..idx]));
        }
    }
    Err(CliError::Fatal {
        code: 128,
        message: format!("archive path is too long for ustar header: {path}"),
    })
}

fn write_tar_bytes(dst: &mut [u8], src: &[u8]) {
    let len = dst.len().min(src.len());
    dst[..len].copy_from_slice(&src[..len]);
}

fn write_tar_octal(dst: &mut [u8], value: u64) {
    let width = dst.len();
    let encoded = format!("{value:0width$o}", width = width - 1);
    write_tar_bytes(&mut dst[..width - 1], encoded.as_bytes());
}

fn write_tar_checksum(dst: &mut [u8], value: u32) {
    let encoded = format!("{value:06o}\0 ");
    write_tar_bytes(dst, encoded.as_bytes());
}

#[derive(Debug)]
struct ZipCentralEntry {
    name: Vec<u8>,
    crc32: u32,
    size: u32,
    mtime: u64,
    mode: u32,
    offset: u32,
}

#[derive(Debug)]
struct ZipArchiveWriter {
    out: Vec<u8>,
    central: Vec<ZipCentralEntry>,
}

impl ZipArchiveWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            central: Vec::new(),
        }
    }

    fn add_directory(&mut self, path: &str, mtime: u64) -> Result<()> {
        let path = if path.ends_with('/') {
            path.to_owned()
        } else {
            format!("{path}/")
        };
        self.add_entry(path.as_bytes(), b"", 0o040775, mtime)
    }

    fn add_file(&mut self, path: &str, content: &[u8], mode: u32, mtime: u64) -> Result<()> {
        self.add_entry(path.as_bytes(), content, mode, mtime)
    }

    fn add_entry(&mut self, name: &[u8], content: &[u8], mode: u32, mtime: u64) -> Result<()> {
        let offset = u32::try_from(self.out.len()).map_err(|_| zip_too_large())?;
        let size = u32::try_from(content.len()).map_err(|_| zip_too_large())?;
        let crc32 = archive_crc32(content);
        let (dos_time, dos_date) = zip_dos_time_date(mtime);

        write_le_u32(&mut self.out, 0x0403_4b50);
        write_le_u16(&mut self.out, 10);
        write_le_u16(&mut self.out, 0);
        write_le_u16(&mut self.out, 0);
        write_le_u16(&mut self.out, dos_time);
        write_le_u16(&mut self.out, dos_date);
        write_le_u32(&mut self.out, crc32);
        write_le_u32(&mut self.out, size);
        write_le_u32(&mut self.out, size);
        write_le_u16(
            &mut self.out,
            u16::try_from(name.len()).map_err(|_| zip_name_too_long())?,
        );
        write_le_u16(&mut self.out, 0);
        self.out.extend_from_slice(name);
        self.out.extend_from_slice(content);
        self.central.push(ZipCentralEntry {
            name: name.to_vec(),
            crc32,
            size,
            mtime,
            mode,
            offset,
        });
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>> {
        let central_offset = u32::try_from(self.out.len()).map_err(|_| zip_too_large())?;
        for entry in &self.central {
            let (dos_time, dos_date) = zip_dos_time_date(entry.mtime);
            write_le_u32(&mut self.out, 0x0201_4b50);
            write_le_u16(&mut self.out, (3 << 8) | 10);
            write_le_u16(&mut self.out, 10);
            write_le_u16(&mut self.out, 0);
            write_le_u16(&mut self.out, 0);
            write_le_u16(&mut self.out, dos_time);
            write_le_u16(&mut self.out, dos_date);
            write_le_u32(&mut self.out, entry.crc32);
            write_le_u32(&mut self.out, entry.size);
            write_le_u32(&mut self.out, entry.size);
            write_le_u16(
                &mut self.out,
                u16::try_from(entry.name.len()).map_err(|_| zip_name_too_long())?,
            );
            write_le_u16(&mut self.out, 0);
            write_le_u16(&mut self.out, 0);
            write_le_u16(&mut self.out, 0);
            write_le_u16(&mut self.out, 0);
            write_le_u32(&mut self.out, entry.mode << 16);
            write_le_u32(&mut self.out, entry.offset);
            self.out.extend_from_slice(&entry.name);
        }
        let central_size =
            u32::try_from(self.out.len() - central_offset as usize).map_err(|_| zip_too_large())?;
        let entry_count = u16::try_from(self.central.len()).map_err(|_| zip_too_large())?;
        write_le_u32(&mut self.out, 0x0605_4b50);
        write_le_u16(&mut self.out, 0);
        write_le_u16(&mut self.out, 0);
        write_le_u16(&mut self.out, entry_count);
        write_le_u16(&mut self.out, entry_count);
        write_le_u32(&mut self.out, central_size);
        write_le_u32(&mut self.out, central_offset);
        write_le_u16(&mut self.out, 0);
        Ok(self.out)
    }
}

fn write_le_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_le_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn zip_dos_time_date(timestamp: u64) -> (u16, u16) {
    let Some(datetime) = chrono::DateTime::from_timestamp(timestamp as i64, 0) else {
        return (0, (1 << 5) | 1);
    };
    let datetime = datetime.naive_utc();
    let year = datetime.year().clamp(1980, 2107) as u16;
    let month = datetime.month().clamp(1, 12) as u16;
    let day = datetime.day().clamp(1, 31) as u16;
    let hour = datetime.hour().min(23) as u16;
    let minute = datetime.minute().min(59) as u16;
    let second = (datetime.second().min(59) / 2) as u16;
    let dos_time = (hour << 11) | (minute << 5) | second;
    let dos_date = ((year - 1980) << 9) | (month << 5) | day;
    (dos_time, dos_date)
}

fn archive_crc32(content: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in content {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn zip_too_large() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "zip archive is too large for ZIP32 output".into(),
    }
}

fn zip_name_too_long() -> CliError {
    CliError::Fatal {
        code: 128,
        message: "zip archive path is too long".into(),
    }
}

pub(crate) fn upload_archive(repository: PathBuf) -> Result<()> {
    let repo = transport_commands::upload_pack_repo_from_path(&repository, false)?;
    let mut stdout = io::stdout();
    transport_commands::write_pkt_line(&mut stdout, b"ACK\n")?;
    stdout.write_all(b"0000")?;
    stdout.flush()?;

    let mut stdin = io::stdin().lock();
    let result = (|| {
        let arguments = read_upload_archive_arguments(&mut stdin)?;
        parse_upload_archive_arguments(arguments)
    })();
    let options = match result {
        Ok(options) => options,
        Err(error) => return upload_archive_protocol_error(&mut stdout, error),
    };

    if options.list {
        transport_commands::write_sideband_pack(&mut stdout, b"tar\ntgz\ntar.gz\nzip\n")?;
        stdout.flush()?;
        return Ok(());
    }

    let treeish = options.treeish.as_deref().ok_or_else(|| CliError::Fatal {
        code: 129,
        message: "upload-archive requires a tree-ish".into(),
    })?;
    let format = ArchiveFormat::parse(options.format.as_deref().unwrap_or("tar"))?;
    let archive = archive_to_bytes(&repo, &options, treeish, format)?;
    transport_commands::write_sideband_pack(&mut stdout, &archive)?;
    stdout.flush()?;
    Ok(())
}

fn upload_archive_protocol_error<W: Write>(stdout: &mut W, error: CliError) -> Result<()> {
    let message = match error {
        CliError::Fatal { message, .. } => message,
        CliError::Stderr { text, .. } => text.trim_end_matches('\n').to_owned(),
        other => return Err(other),
    };
    upload_archive_sideband_error(stdout, &message)?;
    Err(CliError::Stderr {
        code: 128,
        text: "fatal: sent error to the client: git upload-archive: archiver died with error\n"
            .into(),
    })
}

fn upload_archive_sideband_error<W: Write>(stdout: &mut W, message: &str) -> Result<()> {
    let mut fatal = Vec::with_capacity(message.len() + 9);
    fatal.push(2);
    fatal.extend_from_slice(b"fatal: ");
    fatal.extend_from_slice(message.as_bytes());
    fatal.push(b'\n');
    transport_commands::write_pkt_line(stdout, &fatal)?;

    let mut died = Vec::from([3]);
    died.extend_from_slice(b"git upload-archive: archiver died with error");
    transport_commands::write_pkt_line(stdout, &died)?;
    stdout.write_all(b"0000")?;
    stdout.flush()?;
    Ok(())
}

fn read_upload_archive_arguments<R: BufRead>(input: &mut R) -> Result<Vec<String>> {
    let mut arguments = Vec::new();
    while let Some(line) = transport_commands::read_pkt_line_payload_from_reader(input)? {
        let line = String::from_utf8(line).map_err(|_| CliError::Fatal {
            code: 128,
            message: "upload-archive argument contains non-utf8 pkt-line".into(),
        })?;
        let line = line.trim_end_matches('\n');
        let Some(argument) = line.strip_prefix("argument ") else {
            return Err(CliError::Fatal {
                code: 128,
                message: "'argument' token or flush expected".into(),
            });
        };
        arguments.push(argument.to_owned());
    }
    Ok(arguments)
}

fn parse_upload_archive_arguments(arguments: Vec<String>) -> Result<ArchiveOptions> {
    let mut options = ArchiveOptions {
        format: None,
        prefix: None,
        output: None,
        add_files: Vec::new(),
        add_virtual_files: Vec::new(),
        mtime: None,
        list: false,
        verbose: false,
        treeish: None,
        paths: Vec::new(),
    };
    let mut positional = false;
    for argument in arguments {
        if !positional {
            if argument == "--" {
                positional = true;
                continue;
            }
            if let Some(format) = argument.strip_prefix("--format=") {
                ArchiveFormat::parse(format)?;
                options.format = Some(format.to_owned());
                continue;
            }
            if let Some(prefix) = argument.strip_prefix("--prefix=") {
                options.prefix = Some(prefix.to_owned());
                continue;
            }
            if let Some(path) = argument.strip_prefix("--add-file=") {
                options.add_files.push(PathBuf::from(path));
                continue;
            }
            if let Some(value) = argument.strip_prefix("--add-virtual-file=") {
                options.add_virtual_files.push(value.to_owned());
                continue;
            }
            if let Some(value) = argument.strip_prefix("--mtime=") {
                parse_archive_mtime(value)?;
                options.mtime = Some(value.to_owned());
                continue;
            }
            if argument == "--list" || argument == "-l" {
                options.list = true;
                continue;
            }
            if argument == "--verbose" || argument == "-v" {
                options.verbose = true;
                continue;
            }
            if argument == "--worktree-attributes"
                || argument == "--no-worktree-attributes"
                || matches!(
                    argument.as_str(),
                    "-0" | "-1" | "-2" | "-3" | "-4" | "-5" | "-6" | "-7" | "-8" | "-9"
                )
            {
                continue;
            }
            if argument.starts_with('-') {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("unknown option `{}`", argument.trim_start_matches('-')),
                });
            }
        }
        if options.treeish.is_none() {
            options.treeish = Some(argument);
        } else {
            options.paths.push(argument);
        }
    }
    if options.treeish.is_none() {
        return Err(CliError::Fatal {
            code: 129,
            message: "upload-archive requires a tree-ish".into(),
        });
    }
    Ok(options)
}
