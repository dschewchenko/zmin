use super::*;

const PACK_OBJECTS_STDIN_OBJECT_CAPACITY_HINT: usize = 1024;
const FSCK_SEEN_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_REDUNDANT_INITIAL_CAPACITY_LIMIT: usize = 8192;
const PACK_REV_LIST_OBJECT_INITIAL_CAPACITY_LIMIT: usize = 8192;
const MULTI_PACK_INDEX_INITIAL_CAPACITY_LIMIT: usize = 8192;
const COMMIT_GRAPH_INITIAL_CAPACITY_LIMIT: usize = 8192;
const FSCK_TREE_ENTRY_NAME_INITIAL_CAPACITY_LIMIT: usize = 8192;
const INDEX_PACK_STDIN_BUF_CAPACITY: usize = 256 * 1024;

fn mktag() -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let tag = decode_tag(GitHashAlgorithm::Sha1, &input)?;
    let target = store.read_object(&tag.target)?;
    if target.kind != tag.target_kind {
        return Err(CliError::Fatal {
            code: 128,
            message: "tag object type does not match target".into(),
        });
    }
    let id = store.write_object(GitObjectKind::Tag, &input)?;
    println!("{}", id.to_hex());
    Ok(())
}

pub(crate) fn mktag_command() -> Result<()> {
    mktag()
}

pub(crate) fn commit_graph_command(command: CommitGraphCommand) -> Result<()> {
    commit_graph(command)
}

pub(crate) fn multi_pack_index_command(
    object_dir: Option<PathBuf>,
    command: MultiPackIndexCommand,
) -> Result<()> {
    multi_pack_index(object_dir, command)
}

fn multi_pack_index(object_dir: Option<PathBuf>, command: MultiPackIndexCommand) -> Result<()> {
    let objects_dir = match object_dir {
        Some(path) if path.is_absolute() => path,
        Some(path) => std::env::current_dir()?.join(path),
        None => find_repo()?.objects_dir,
    };
    match command {
        MultiPackIndexCommand::Write {
            progress,
            no_progress,
        } => {
            let _ = (progress, no_progress);
            multi_pack_index_write(&objects_dir, true)
        }
        MultiPackIndexCommand::Verify {
            progress,
            no_progress,
        } => {
            let _ = (progress, no_progress);
            multi_pack_index_verify(&objects_dir)
        }
        MultiPackIndexCommand::Expire {
            progress,
            no_progress,
        } => {
            let _ = (progress, no_progress);
            multi_pack_index_expire(&objects_dir)
        }
        MultiPackIndexCommand::Repack {
            batch_size,
            progress,
            no_progress,
        } => multi_pack_index_repack(&objects_dir, batch_size, progress, no_progress),
    }
}

pub(crate) fn multi_pack_index_write(
    objects_dir: &std::path::Path,
    require_packs: bool,
) -> Result<()> {
    let pack_dir = objects_dir.join("pack");
    let packs = multi_pack_index_pack_names(&pack_dir)?;
    if packs.is_empty() {
        if require_packs {
            return Err(CliError::Stderr {
                code: empty_multi_pack_index_failure_code(),
                text: "error: no pack files to index.\n".into(),
            });
        }
        return Ok(());
    }
    let bytes = encode_multi_pack_index(&pack_dir, &packs)?;
    fs::create_dir_all(&pack_dir)?;
    fs::write(pack_dir.join("multi-pack-index"), bytes)?;
    Ok(())
}

#[cfg(windows)]
fn empty_multi_pack_index_failure_code() -> i32 {
    255
}

#[cfg(not(windows))]
fn empty_multi_pack_index_failure_code() -> i32 {
    1
}

fn multi_pack_index_verify(objects_dir: &std::path::Path) -> Result<()> {
    let path = objects_dir.join("pack/multi-pack-index");
    let bytes = match map_file_bytes(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(CliError::Io(err)),
    };
    verify_multi_pack_index_bytes(bytes.as_slice())
}

fn multi_pack_index_expire(objects_dir: &std::path::Path) -> Result<()> {
    let pack_dir = objects_dir.join("pack");
    let packs = multi_pack_index_pack_names(&pack_dir)?;
    if packs.len() < 2 {
        return Ok(());
    }
    let pack_counts = multi_pack_index_pack_object_counts(&pack_dir, &packs)?;
    let mut redundant = Vec::new();
    for (idx, pack) in packs.iter().enumerate() {
        if pack_counts[idx] == 0
            || !multi_pack_index_pack_is_redundant(&pack_dir, &packs, &pack_counts, idx)?
        {
            continue;
        }
        redundant.push(pack.clone());
    }
    let removed = !redundant.is_empty();
    for pack in redundant {
        remove_pack_family(&pack_dir.join(pack))?;
    }
    if removed {
        multi_pack_index_write(objects_dir, false)?;
    }
    Ok(())
}

fn remove_pack_family(idx_path: &std::path::Path) -> Result<()> {
    for path in [
        idx_path.to_path_buf(),
        idx_path.with_extension("pack"),
        idx_path.with_extension("rev"),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError::Io(io::Error::new(
                    error.kind(),
                    format!("remove {}: {error}", path.display()),
                )));
            }
        }
    }
    Ok(())
}

fn multi_pack_index_pack_object_counts(
    pack_dir: &std::path::Path,
    packs: &[String],
) -> Result<Vec<usize>> {
    let mut counts = Vec::with_capacity(packs.len());
    for pack in packs {
        counts.push(pack_index_object_count_with_context(&pack_dir.join(pack))?);
    }
    Ok(counts)
}

fn pack_index_object_count_with_context(path: &std::path::Path) -> Result<usize> {
    pack_index_object_count(path).map_err(|error| {
        CliError::Io(io::Error::new(
            error.kind(),
            format!("read pack index {}: {error}", path.display()),
        ))
    })
}

fn multi_pack_index_pack_is_redundant(
    pack_dir: &std::path::Path,
    packs: &[String],
    pack_counts: &[usize],
    idx: usize,
) -> Result<bool> {
    let name = &packs[idx];
    let count = pack_counts[idx];
    for (other_idx, other_name) in packs.iter().enumerate() {
        if other_idx == idx {
            continue;
        }
        let other_count = pack_counts[other_idx];
        if count > other_count || (count == other_count && name <= other_name) {
            continue;
        }
        if pack_index_object_ids_are_subset_from_paths(
            GitHashAlgorithm::Sha1,
            &pack_dir.join(name),
            &pack_dir.join(other_name),
        )
        .map_err(|error| {
            CliError::Io(io::Error::new(
                error.kind(),
                format!(
                    "compare pack indexes {} and {}: {error}",
                    pack_dir.join(name).display(),
                    pack_dir.join(other_name).display()
                ),
            ))
        })? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn multi_pack_index_repack(
    objects_dir: &std::path::Path,
    batch_size: Option<u64>,
    progress: bool,
    no_progress: bool,
) -> Result<()> {
    let _ = (progress, no_progress);
    let pack_dir = objects_dir.join("pack");
    let packs = multi_pack_index_pack_names(&pack_dir)?;
    if packs.len() < 2 {
        return Ok(());
    }
    let selected_packs = multi_pack_index_repack_packs(&pack_dir, &packs, batch_size)?;
    if selected_packs.len() < 2 {
        return Ok(());
    }
    let selected_capacity = multi_pack_index_initial_capacity(multi_pack_index_object_capacity(
        &pack_dir,
        &selected_packs,
    )?);
    let mut ids = Vec::with_capacity(selected_capacity);
    for pack in &selected_packs {
        let mut push_id = |id: &ObjectId| {
            ids.push(id.clone());
            Ok(())
        };
        for_each_pack_index_object_id_from_path(
            GitHashAlgorithm::Sha1,
            &pack_dir.join(pack).with_extension("idx"),
            &mut push_id,
        )?;
    }
    ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    ids.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
    if ids.is_empty() {
        return Ok(());
    }
    let store = LooseObjectStore::new(objects_dir.to_path_buf(), GitHashAlgorithm::Sha1);
    let packed_first_store = store.packed_first();
    fs::create_dir_all(&pack_dir)?;
    let temp_pack = unique_temp_sibling(&pack_dir.join("pack-midx-repack.pack"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_pack)?;
        write_pack_from_store_with_options(
            &packed_first_store,
            GitHashAlgorithm::Sha1,
            &ids,
            pack_encode_options(None, None),
            &mut file,
        )?;
        file.flush()?;
        Ok::<_, CliError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result?;
    let indexed = match index_pack_file(GitHashAlgorithm::Sha1, &temp_pack) {
        Ok(indexed) => indexed,
        Err(error) => {
            let _ = fs::remove_file(&temp_pack);
            return Err(CliError::Io(error));
        }
    };
    let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
    install_temp_pack_file(
        &pack_dir.join(format!("{pack_name}.pack")),
        &temp_pack,
        &indexed,
    )?;
    write_content_addressed_file(&pack_dir.join(format!("{pack_name}.idx")), &indexed.index)?;
    write_content_addressed_file(
        &pack_dir.join(format!("{pack_name}.rev")),
        &indexed.reverse_index,
    )?;
    multi_pack_index_write(objects_dir, false)
}

fn install_temp_pack_file(
    path: &std::path::Path,
    temp_pack_path: &std::path::Path,
    indexed: &zmin_git_core::IndexedPack,
) -> Result<()> {
    install_temp_pack_file_with_id(path, temp_pack_path, &indexed.pack_id)
}

fn install_temp_pack_file_with_id(
    path: &std::path::Path,
    temp_pack_path: &std::path::Path,
    pack_id: &ObjectId,
) -> Result<()> {
    match fs::hard_link(temp_pack_path, path) {
        Ok(()) => {
            let _ = fs::remove_file(temp_pack_path);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(temp_pack_path);
            if index_pack_file_index_only(GitHashAlgorithm::Sha1, path)
                .is_ok_and(|existing| existing.pack_id == *pack_id)
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

struct IndexPackCommandIndex {
    pack_id: ObjectId,
    index: Vec<u8>,
    reverse_index: Option<Vec<u8>>,
}

fn index_pack_file_for_output(
    path: &std::path::Path,
    index_version: PackIndexVersion,
    no_rev_index: bool,
) -> Result<IndexPackCommandIndex> {
    if no_rev_index {
        let indexed =
            index_pack_file_index_only_with_version(GitHashAlgorithm::Sha1, path, index_version)?;
        Ok(IndexPackCommandIndex {
            pack_id: indexed.pack_id,
            index: indexed.index,
            reverse_index: None,
        })
    } else {
        let indexed = index_pack_file_with_version(GitHashAlgorithm::Sha1, path, index_version)?;
        Ok(IndexPackCommandIndex {
            pack_id: indexed.pack_id,
            index: indexed.index,
            reverse_index: Some(indexed.reverse_index),
        })
    }
}

fn multi_pack_index_repack_packs(
    pack_dir: &std::path::Path,
    packs: &[String],
    batch_size: Option<u64>,
) -> Result<Vec<String>> {
    let Some(batch_size) = batch_size else {
        return Ok(packs.to_vec());
    };
    if batch_size == 0 {
        return Ok(packs.to_vec());
    }
    let mut selected = Vec::new();
    let mut total = 0_u64;
    for pack in packs {
        let size = fs::metadata(pack_dir.join(pack).with_extension("pack"))?.len();
        if size > batch_size {
            continue;
        }
        total = total.saturating_add(size);
        if total > batch_size {
            break;
        }
        selected.push(pack.clone());
    }
    Ok(selected)
}

#[derive(Debug, Clone)]
struct MultiPackIndexEntry {
    object_id: ObjectId,
    pack_id: u32,
    offset: u64,
}

fn oid_fanout<'a>(
    ids: impl IntoIterator<Item = &'a ObjectId>,
    overflow_message: &'static str,
) -> Result<[u32; 256]> {
    let mut fanout = [0_u32; 256];
    for id in ids {
        let first = id.as_bytes()[0] as usize;
        fanout[first] = fanout[first]
            .checked_add(1)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: overflow_message.into(),
            })?;
    }
    let mut running = 0_u32;
    for bucket in &mut fanout {
        running = running
            .checked_add(*bucket)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: overflow_message.into(),
            })?;
        *bucket = running;
    }
    Ok(fanout)
}

pub(crate) fn multi_pack_index_pack_names(pack_dir: &std::path::Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    match fs::read_dir(pack_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("idx") {
                    continue;
                }
                if path.with_extension("pack").is_file()
                    && let Some(name) = path.file_name().and_then(|name| name.to_str())
                {
                    names.push(name.to_owned());
                }
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(CliError::Io(err)),
    }
    names.sort();
    Ok(names)
}

fn multi_pack_index_object_capacity(pack_dir: &std::path::Path, packs: &[String]) -> Result<usize> {
    packs.iter().try_fold(0_usize, |total, pack_name| {
        let count = pack_index_object_count(&pack_dir.join(pack_name))?;
        total.checked_add(count).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "multi-pack-index has too many objects".into(),
        })
    })
}

fn multi_pack_index_initial_capacity(object_count: usize) -> usize {
    object_count
        .min(MULTI_PACK_INDEX_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn encode_multi_pack_index(pack_dir: &std::path::Path, packs: &[String]) -> Result<Vec<u8>> {
    const HEADER_LEN: u64 = 12;
    const LOOKUP_ENTRY_LEN: u64 = 12;

    let pack_count = u32::try_from(packs.len()).map_err(|_| CliError::Fatal {
        code: 128,
        message: "multi-pack-index has too many pack files".into(),
    })?;
    let selected_capacity =
        multi_pack_index_initial_capacity(multi_pack_index_object_capacity(pack_dir, packs)?);
    let mut selected = HashMap::<ObjectId, (u32, u64)>::with_capacity(selected_capacity);
    for (pack_id, pack_name) in packs.iter().enumerate() {
        let pack_id = u32::try_from(pack_id).map_err(|_| CliError::Fatal {
            code: 128,
            message: "multi-pack-index pack id overflow".into(),
        })?;
        let mut select_entry = |entry: PackIndexEntry| {
            selected
                .entry(entry.object_id)
                .or_insert((pack_id, entry.offset));
            Ok(())
        };
        for_each_pack_index_entry_from_path(
            GitHashAlgorithm::Sha1,
            &pack_dir.join(pack_name),
            &mut select_entry,
        )?;
    }
    let mut entries = Vec::with_capacity(selected.len());
    for (object_id, (pack_id, offset)) in selected {
        entries.push(MultiPackIndexEntry {
            object_id,
            pack_id,
            offset,
        });
    }
    entries.sort_by(|left, right| left.object_id.as_bytes().cmp(right.object_id.as_bytes()));
    let object_count = u32::try_from(entries.len()).map_err(|_| CliError::Fatal {
        code: 128,
        message: "multi-pack-index has too many objects".into(),
    })?;
    let large_offset_count = entries
        .iter()
        .filter(|entry| entry.offset > 0x7fff_ffff)
        .count();
    let chunk_count = if large_offset_count == 0 { 4 } else { 5 };

    let mut pnam_capacity = packs.iter().map(|name| name.len() + 1).sum::<usize>();
    let pnam_padding = (4 - pnam_capacity % 4) % 4;
    pnam_capacity += pnam_padding;
    let mut pnam = Vec::with_capacity(pnam_capacity);
    for name in packs {
        pnam.extend_from_slice(name.as_bytes());
        pnam.push(0);
    }
    while pnam.len() % 4 != 0 {
        pnam.push(0);
    }

    let lookup_len = (u64::from(chunk_count) + 1) * LOOKUP_ENTRY_LEN;
    let pnam_offset = HEADER_LEN + lookup_len;
    let oidf_offset = pnam_offset + pnam.len() as u64;
    let oidl_offset = oidf_offset + 256 * 4;
    let ooff_offset =
        oidl_offset + u64::from(object_count) * GitHashAlgorithm::Sha1.digest_len() as u64;
    let loff_offset = ooff_offset + u64::from(object_count) * 8;
    let end_offset = loff_offset + large_offset_count as u64 * 8;

    let output_capacity = usize::try_from(end_offset + GitHashAlgorithm::Sha1.digest_len() as u64)
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: "multi-pack-index size overflow".into(),
        })?;
    let mut out = Vec::with_capacity(output_capacity);
    out.extend_from_slice(b"MIDX");
    out.push(1);
    out.push(1);
    out.push(chunk_count);
    out.push(0);
    push_u32_be(&mut out, pack_count);
    push_commit_graph_chunk(&mut out, b"PNAM", pnam_offset);
    push_commit_graph_chunk(&mut out, b"OIDF", oidf_offset);
    push_commit_graph_chunk(&mut out, b"OIDL", oidl_offset);
    push_commit_graph_chunk(&mut out, b"OOFF", ooff_offset);
    if large_offset_count != 0 {
        push_commit_graph_chunk(&mut out, b"LOFF", loff_offset);
    }
    push_commit_graph_chunk(&mut out, &[0, 0, 0, 0], end_offset);

    out.extend_from_slice(&pnam);
    let fanout = oid_fanout(
        entries.iter().map(|entry| &entry.object_id),
        "multi-pack-index fanout overflow",
    )?;
    for count in fanout {
        push_u32_be(&mut out, count);
    }
    for entry in &entries {
        out.extend_from_slice(entry.object_id.as_bytes());
    }
    let mut large_offset_index = 0_u32;
    for entry in &entries {
        push_u32_be(&mut out, entry.pack_id);
        if entry.offset <= 0x7fff_ffff {
            push_u32_be(&mut out, entry.offset as u32);
        } else {
            push_u32_be(&mut out, 0x8000_0000 | large_offset_index);
            large_offset_index =
                large_offset_index
                    .checked_add(1)
                    .ok_or_else(|| CliError::Fatal {
                        code: 128,
                        message: "multi-pack-index large offset index overflow".into(),
                    })?;
        }
    }
    for entry in &entries {
        if entry.offset > 0x7fff_ffff {
            out.extend_from_slice(&entry.offset.to_be_bytes());
        }
    }

    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&out);
    let checksum = hasher.finalize();
    out.extend_from_slice(checksum.as_bytes());
    Ok(out)
}

fn verify_multi_pack_index_bytes(bytes: &[u8]) -> Result<()> {
    let digest_len = GitHashAlgorithm::Sha1.digest_len();
    if bytes.len() < 12 + 12 + digest_len {
        return Err(CliError::Fatal {
            code: 1,
            message: "multi-pack-index file is too small".into(),
        });
    }
    if &bytes[..4] != b"MIDX" {
        let actual = u32::from_be_bytes(bytes[..4].try_into().expect("midx signature bytes"));
        let expected = u32::from_be_bytes(*b"MIDX");
        return Err(CliError::Fatal {
            code: 128,
            message: format!(
                "multi-pack-index signature {actual:#010x} does not match signature {expected:#010x}"
            ),
        });
    }
    if bytes[4] != 1 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("multi-pack-index version {} not recognized", bytes[4]),
        });
    }
    if bytes[5] != 1 {
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "error: multi-pack-index hash version {} does not match version 1\nerror: multi-pack-index file exists, but failed to parse\n",
                bytes[5]
            ),
        });
    }
    let chunk_count = bytes[6] as usize;
    let pack_count = read_u32_be(&bytes[8..12])? as usize;
    let lookup_end = 12 + (chunk_count + 1) * 12;
    if lookup_end > bytes.len().saturating_sub(digest_len) {
        return Err(CliError::Fatal {
            code: 1,
            message: "multi-pack-index chunk lookup is truncated".into(),
        });
    }
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&bytes[..bytes.len() - digest_len]);
    if hasher.finalize().as_bytes() != &bytes[bytes.len() - digest_len..] {
        return Err(CliError::Fatal {
            code: 1,
            message: "multi-pack-index checksum mismatch".into(),
        });
    }

    let mut chunks = Vec::with_capacity(chunk_count);
    let graph_data_end = bytes.len() - digest_len;
    let mut previous_offset = 0_u64;
    for idx in 0..=chunk_count {
        let cursor = 12 + idx * 12;
        let chunk_id = [
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ];
        let offset = read_u64_be(&bytes[cursor + 4..cursor + 12])?;
        if offset < previous_offset || offset as usize > graph_data_end {
            return Err(CliError::Fatal {
                code: 1,
                message: "multi-pack-index chunk offsets are invalid".into(),
            });
        }
        previous_offset = offset;
        if idx < chunk_count {
            chunks.push((chunk_id, offset as usize));
        }
    }

    let pnam = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"PNAM", graph_data_end)?;
    let oidf = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDF", graph_data_end)?;
    let oidl = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDL", graph_data_end)?;
    let ooff = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OOFF", graph_data_end)?;
    let loff = chunks
        .iter()
        .any(|(id, _)| id == b"LOFF")
        .then(|| commit_graph_chunk_range_from_offsets(bytes, &chunks, b"LOFF", graph_data_end))
        .transpose()?;
    if pnam
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .count()
        != pack_count
    {
        return Err(CliError::Fatal {
            code: 1,
            message: "multi-pack-index pack-name count mismatch".into(),
        });
    }
    if oidf.len() < 256 * 4 {
        return Err(CliError::Fatal {
            code: 1,
            message: "multi-pack-index OIDF chunk is truncated".into(),
        });
    }
    let mut previous = 0_u32;
    for idx in 0..256 {
        let count = read_u32_be(&oidf[idx * 4..idx * 4 + 4])?;
        if count < previous {
            return Err(CliError::Fatal {
                code: 1,
                message: "multi-pack-index OIDF fanout is not sorted".into(),
            });
        }
        previous = count;
    }
    let count = previous as usize;
    if oidl.len() < count * digest_len || ooff.len() < count * 8 {
        return Err(CliError::Fatal {
            code: 1,
            message: "multi-pack-index object chunks are truncated".into(),
        });
    }
    if !object_id_chunks_are_strictly_sorted(&oidl[..count * digest_len], digest_len) {
        return Err(CliError::Fatal {
            code: 1,
            message: "multi-pack-index object ids are not strictly sorted".into(),
        });
    }
    for idx in 0..count {
        let pack_id = read_u32_be(&ooff[idx * 8..idx * 8 + 4])? as usize;
        if pack_id >= pack_count {
            return Err(CliError::Fatal {
                code: 1,
                message: "multi-pack-index object references invalid pack id".into(),
            });
        }
        let offset = read_u32_be(&ooff[idx * 8 + 4..idx * 8 + 8])?;
        if offset & 0x8000_0000 != 0 {
            let Some(loff) = loff else {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "multi-pack-index large offset missing LOFF chunk".into(),
                });
            };
            let large_idx = (offset & 0x7fff_ffff) as usize;
            if (large_idx + 1) * 8 > loff.len() {
                return Err(CliError::Fatal {
                    code: 1,
                    message: "multi-pack-index large offset is out of bounds".into(),
                });
            }
        }
    }
    Ok(())
}

fn commit_graph(command: CommitGraphCommand) -> Result<()> {
    match command {
        CommitGraphCommand::Write { reachable } => commit_graph_write(reachable),
        CommitGraphCommand::Verify => commit_graph_verify(),
    }
}

pub(crate) fn commit_graph_write(reachable: bool) -> Result<()> {
    if !reachable {
        return Err(CliError::Fatal {
            code: 129,
            message: "commit-graph write requires --reachable".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let commits = collect_reachable_commit_graph_commits(&repo, &store, &commit_cache)?;
    if commits.is_empty() {
        return Ok(());
    }
    let bytes = encode_commit_graph(&commit_cache, &commits)?;
    let path = repo.git_dir.join("objects/info/commit-graph");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn commit_graph_verify() -> Result<()> {
    let repo = find_repo()?;
    let path = repo.git_dir.join("objects/info/commit-graph");
    let bytes = match map_file_bytes(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(CliError::Io(err)),
    };
    verify_commit_graph_bytes(bytes.as_slice())
}

enum FileBytes {
    Empty,
    Mapped(memmap2::Mmap),
}

impl FileBytes {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Empty => &[],
            Self::Mapped(bytes) => bytes,
        }
    }
}

fn map_file_bytes(path: &std::path::Path) -> io::Result<FileBytes> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() == 0 {
        return Ok(FileBytes::Empty);
    }
    let bytes = unsafe { memmap2::Mmap::map(&file)? };
    Ok(FileBytes::Mapped(bytes))
}

fn collect_reachable_commit_graph_commits(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
) -> Result<Vec<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let root_capacity = COMMIT_GRAPH_INITIAL_CAPACITY_LIMIT;
    let mut seen = HashSet::with_capacity(root_capacity);
    let mut starts = Vec::with_capacity(root_capacity);
    refs.for_each_resolved_ref("refs/", |_, id| {
        if let Some(commit) = peel_to_commit(store, id.clone())?
            && seen.insert(commit.clone())
        {
            starts.push(commit);
        }
        Ok::<(), CliError>(())
    })?;
    if let Ok(head) = refs.resolve("HEAD")
        && let Some(commit) = peel_to_commit(store, head)?
        && seen.insert(commit.clone())
    {
        starts.push(commit);
    }
    starts.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    collect_commits_from_ids_cached(repo, commit_cache, &starts, None)
}

fn encode_commit_graph<S: GitObjectStore + ?Sized>(
    commit_cache: &CommitObjectCache<'_, S>,
    commits: &[ObjectId],
) -> Result<Vec<u8>> {
    const HEADER_LEN: u64 = 8;
    const LOOKUP_ENTRY_LEN: u64 = 12;
    const GRAPH_PARENT_NONE: u32 = 0x7000_0000;
    const GRAPH_EXTRA_EDGE_LIST: u32 = 0x8000_0000;
    const GRAPH_LAST_EDGE: u32 = 0x8000_0000;
    const MAX_GRAPH_POSITION: usize = 0x6fff_ffff;
    const MAX_GENERATION: u32 = 0x3fff_ffff;

    if commits.len() > MAX_GRAPH_POSITION {
        return Err(CliError::Fatal {
            code: 128,
            message: "commit graph has too many commits".into(),
        });
    }

    let mut sorted = commits.to_vec();
    sorted.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    sorted.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
    let mut positions = HashMap::with_capacity(commit_graph_initial_capacity(sorted.len()));
    for (idx, id) in sorted.iter().enumerate() {
        positions.insert(id.clone(), idx);
    }
    let CommitGraphGenerationData {
        generations,
        corrected_commit_dates,
        extra_edge_count,
        corrected_date_overflow_count,
    } = commit_graph_generations(commit_cache, &sorted, &positions)?;
    let needs_generation_overflow_chunk = corrected_date_overflow_count != 0;
    let needs_edge_chunk = extra_edge_count != 0;
    let chunk_count = 4 + u8::from(needs_generation_overflow_chunk) + u8::from(needs_edge_chunk);

    let lookup_len = (u64::from(chunk_count) + 1) * LOOKUP_ENTRY_LEN;
    let data_start = HEADER_LEN + lookup_len;
    let oidf_offset = data_start;
    let oidl_offset = oidf_offset + 256 * 4;
    let cdat_offset =
        oidl_offset + sorted.len() as u64 * GitHashAlgorithm::Sha1.digest_len() as u64;
    let cdat_len = sorted.len() as u64 * (GitHashAlgorithm::Sha1.digest_len() as u64 + 16);
    let gda2_offset = cdat_offset + cdat_len;
    let gda2_len = sorted.len() as u64 * 4;
    let gdo2_offset = gda2_offset + gda2_len;
    let gdo2_len = corrected_date_overflow_count as u64 * 8;
    let edge_offset = gdo2_offset + gdo2_len;
    let graph_end_offset = edge_offset + extra_edge_count as u64 * 4;

    let output_capacity = usize::try_from(
        graph_end_offset + GitHashAlgorithm::Sha1.digest_len() as u64,
    )
    .map_err(|_| CliError::Fatal {
        code: 128,
        message: "commit graph size overflow".into(),
    })?;
    let mut out = Vec::with_capacity(output_capacity);
    out.extend_from_slice(b"CGPH");
    out.push(1);
    out.push(1);
    out.push(chunk_count);
    out.push(0);
    push_commit_graph_chunk(&mut out, b"OIDF", oidf_offset);
    push_commit_graph_chunk(&mut out, b"OIDL", oidl_offset);
    push_commit_graph_chunk(&mut out, b"CDAT", cdat_offset);
    push_commit_graph_chunk(&mut out, b"GDA2", gda2_offset);
    if needs_generation_overflow_chunk {
        push_commit_graph_chunk(&mut out, b"GDO2", gdo2_offset);
    }
    if needs_edge_chunk {
        push_commit_graph_chunk(&mut out, b"EDGE", edge_offset);
    }
    let edge_lookup_index = out.len();
    push_commit_graph_chunk(&mut out, &[0, 0, 0, 0], 0);

    let fanout = oid_fanout(&sorted, "commit graph fanout overflow")?;
    for count in fanout {
        push_u32_be(&mut out, count);
    }
    for id in &sorted {
        out.extend_from_slice(id.as_bytes());
    }
    let mut extra_edges = Vec::with_capacity(commit_graph_initial_capacity(extra_edge_count));
    for (idx, id) in sorted.iter().enumerate() {
        let commit = commit_cache.read_commit(id)?;
        let generation = generations[idx].min(MAX_GENERATION);
        let timestamp = commit_graph_commit_timestamp(id, &commit.committer)?;
        let parent_one = commit
            .parents
            .first()
            .map(|parent| commit_graph_parent_position(parent, &positions))
            .transpose()?
            .unwrap_or(GRAPH_PARENT_NONE);
        let parent_two = if commit.parents.len() > 2 {
            let edge_start = u32::try_from(extra_edges.len()).map_err(|_| CliError::Fatal {
                code: 128,
                message: "commit graph edge list overflow".into(),
            })?;
            for (idx, parent) in commit.parents.iter().enumerate().skip(1) {
                let mut position = commit_graph_parent_position(parent, &positions)?;
                if idx == commit.parents.len() - 1 {
                    position |= GRAPH_LAST_EDGE;
                }
                extra_edges.push(position);
            }
            GRAPH_EXTRA_EDGE_LIST | edge_start
        } else {
            commit
                .parents
                .get(1)
                .map(|parent| commit_graph_parent_position(parent, &positions))
                .transpose()?
                .unwrap_or(GRAPH_PARENT_NONE)
        };

        out.extend_from_slice(commit.tree.as_bytes());
        push_u32_be(&mut out, parent_one);
        push_u32_be(&mut out, parent_two);
        push_u32_be(
            &mut out,
            (generation << 2) | ((timestamp >> 32) as u32 & 0x3),
        );
        push_u32_be(&mut out, timestamp as u32);
    }
    let mut corrected_date_overflow_index = 0_u32;
    let mut corrected_date_overflows =
        Vec::with_capacity(commit_graph_initial_capacity(corrected_date_overflow_count));
    for (idx, id) in sorted.iter().enumerate() {
        let commit = commit_cache.read_commit(id)?;
        let timestamp = commit_graph_commit_timestamp(id, &commit.committer)?;
        let offset = commit_graph_generation_offset(timestamp, corrected_commit_dates[idx])?;
        if offset > 0x7fff_ffff {
            push_u32_be(&mut out, 0x8000_0000 | corrected_date_overflow_index);
            corrected_date_overflow_index = corrected_date_overflow_index
                .checked_add(1)
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "commit graph generation overflow list overflow".into(),
                })?;
            corrected_date_overflows.push(offset);
        } else {
            push_u32_be(&mut out, offset as u32);
        }
    }
    for offset in &corrected_date_overflows {
        out.extend_from_slice(&offset.to_be_bytes());
    }
    if needs_edge_chunk {
        for edge in &extra_edges {
            push_u32_be(&mut out, *edge);
        }
    }
    let end_offset = u64::try_from(out.len()).map_err(|_| CliError::Fatal {
        code: 128,
        message: "commit graph size overflow".into(),
    })?;
    out[edge_lookup_index + 4..edge_lookup_index + 12].copy_from_slice(&end_offset.to_be_bytes());

    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&out);
    let checksum = hasher.finalize();
    out.extend_from_slice(checksum.as_bytes());
    Ok(out)
}

struct CommitGraphGenerationData {
    generations: Vec<u32>,
    corrected_commit_dates: Vec<u64>,
    extra_edge_count: usize,
    corrected_date_overflow_count: usize,
}

fn commit_graph_generations<S: GitObjectStore + ?Sized>(
    commit_cache: &CommitObjectCache<'_, S>,
    sorted: &[ObjectId],
    positions: &HashMap<ObjectId, usize>,
) -> Result<CommitGraphGenerationData> {
    let mut generations = vec![0_u32; sorted.len()];
    let mut corrected_commit_dates = vec![0_u64; sorted.len()];
    let mut state = vec![0_u8; sorted.len()];
    let mut pending = Vec::with_capacity(commit_graph_initial_capacity(sorted.len()));
    let mut extra_edge_count = 0usize;
    let mut corrected_date_overflow_count = 0usize;

    for idx in 0..sorted.len() {
        if state[idx] == 2 {
            continue;
        }
        pending.push((idx, false));
        while let Some((position, expanded)) = pending.pop() {
            match (state[position], expanded) {
                (2, _) => continue,
                (_, false) => {
                    state[position] = 1;
                    pending.push((position, true));
                    for parent in &commit_cache.read_commit(&sorted[position])?.parents {
                        let Some(parent_position) = positions.get(parent).copied() else {
                            continue;
                        };
                        match state[parent_position] {
                            0 => pending.push((parent_position, false)),
                            1 => {
                                return Err(CliError::Fatal {
                                    code: 128,
                                    message: "commit graph contains a parent cycle".into(),
                                });
                            }
                            _ => {}
                        }
                    }
                }
                (_, true) => {
                    let commit = commit_cache.read_commit(&sorted[position])?;
                    let timestamp =
                        commit_graph_commit_timestamp(&sorted[position], &commit.committer)?;
                    if commit.parents.len() > 2 {
                        extra_edge_count = extra_edge_count
                            .checked_add(commit.parents.len() - 2)
                            .ok_or_else(|| CliError::Fatal {
                            code: 128,
                            message: "commit graph edge list overflow".into(),
                        })?;
                    }
                    let mut generation = 1_u32;
                    let mut corrected_commit_date = if commit.parents.is_empty() && timestamp == 0 {
                        1
                    } else {
                        timestamp
                    };
                    for parent in &commit.parents {
                        let Some(parent_position) = positions.get(parent).copied() else {
                            continue;
                        };
                        if state[parent_position] != 2 {
                            return Err(CliError::Fatal {
                                code: 128,
                                message: "commit graph parent generation is unresolved".into(),
                            });
                        }
                        generation = generation.max(
                            generations[parent_position].checked_add(1).ok_or_else(|| {
                                CliError::Fatal {
                                    code: 128,
                                    message: "commit graph generation overflow".into(),
                                }
                            })?,
                        );
                        corrected_commit_date = corrected_commit_date.max(
                            corrected_commit_dates[parent_position]
                                .checked_add(1)
                                .ok_or_else(|| CliError::Fatal {
                                    code: 128,
                                    message: "commit graph corrected date overflow".into(),
                                })?,
                        );
                    }
                    generations[position] = generation;
                    corrected_commit_dates[position] = corrected_commit_date;
                    if commit_graph_generation_offset(timestamp, corrected_commit_date)?
                        > 0x7fff_ffff
                    {
                        corrected_date_overflow_count = corrected_date_overflow_count
                            .checked_add(1)
                            .ok_or_else(|| CliError::Fatal {
                                code: 128,
                                message: "commit graph generation overflow list overflow".into(),
                            })?;
                    }
                    state[position] = 2;
                }
            }
        }
    }
    Ok(CommitGraphGenerationData {
        generations,
        corrected_commit_dates,
        extra_edge_count,
        corrected_date_overflow_count,
    })
}

fn commit_graph_commit_timestamp(id: &ObjectId, committer: &[u8]) -> Result<u64> {
    let (timestamp, _) =
        signature_timestamp_timezone(committer).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!("commit {} has invalid committer timestamp", id.to_hex()),
        })?;
    if timestamp < 0 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("commit {} has a negative committer timestamp", id.to_hex()),
        });
    }
    let timestamp = timestamp as u64;
    if timestamp >> 34 != 0 {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("commit {} committer timestamp is too large", id.to_hex()),
        });
    }
    Ok(timestamp)
}

fn commit_graph_generation_offset(timestamp: u64, corrected_commit_date: u64) -> Result<u64> {
    let masked_timestamp = timestamp & ((1_u64 << 34) - 1);
    corrected_commit_date
        .checked_sub(masked_timestamp)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit graph corrected date is earlier than commit timestamp".into(),
        })
}

fn commit_graph_initial_capacity(count: usize) -> usize {
    count.min(COMMIT_GRAPH_INITIAL_CAPACITY_LIMIT).max(1)
}

fn commit_graph_parent_position(
    parent: &ObjectId,
    positions: &HashMap<ObjectId, usize>,
) -> Result<u32> {
    positions
        .get(parent)
        .copied()
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: format!(
                "parent commit {} is outside the commit graph",
                parent.to_hex()
            ),
        })
        .and_then(|position| {
            u32::try_from(position).map_err(|_| CliError::Fatal {
                code: 128,
                message: "commit graph position overflow".into(),
            })
        })
}

fn verify_commit_graph_bytes(bytes: &[u8]) -> Result<()> {
    let digest_len = GitHashAlgorithm::Sha1.digest_len();
    if bytes.len() < 8 + 12 + digest_len {
        return Err(CliError::Fatal {
            code: 1,
            message: "commit-graph file is too small".into(),
        });
    }
    if &bytes[..4] != b"CGPH" {
        let actual = u32::from_be_bytes(bytes[..4].try_into().expect("commit-graph signature"));
        let expected = u32::from_be_bytes(*b"CGPH");
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "error: commit-graph signature {actual:08x} does not match signature {expected:08x}\n"
            ),
        });
    }
    if bytes[4] != 1 {
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "error: commit-graph version {} does not match version 1\n",
                bytes[4]
            ),
        });
    }
    if bytes[5] != 1 {
        return Err(CliError::Stderr {
            code: 1,
            text: format!(
                "error: commit-graph hash version {} does not match version 1\n",
                bytes[5]
            ),
        });
    }
    let chunk_count = bytes[6] as usize;
    let lookup_end = 8 + (chunk_count + 1) * 12;
    if lookup_end > bytes.len().saturating_sub(digest_len) {
        return Err(CliError::Fatal {
            code: 1,
            message: "commit-graph chunk lookup is truncated".into(),
        });
    }
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    hasher.update(&bytes[..bytes.len() - digest_len]);
    if hasher.finalize().as_bytes() != &bytes[bytes.len() - digest_len..] {
        return Err(CliError::Fatal {
            code: 1,
            message: "commit-graph checksum mismatch".into(),
        });
    }

    let graph_data_end = bytes.len() - digest_len - bytes[7] as usize * digest_len;
    let mut chunks = Vec::with_capacity(chunk_count);
    let mut previous_offset = 0_u64;
    for idx in 0..=chunk_count {
        let cursor = 8 + idx * 12;
        let chunk_id = [
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ];
        let offset = read_u64_be(&bytes[cursor + 4..cursor + 12])?;
        if offset < previous_offset || offset as usize > graph_data_end {
            return Err(CliError::Fatal {
                code: 1,
                message: "commit-graph chunk offsets are invalid".into(),
            });
        }
        previous_offset = offset;
        if idx < chunk_count {
            chunks.push((chunk_id, offset as usize));
        }
    }

    let oidf = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDF", graph_data_end)?;
    let oidl = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDL", graph_data_end)?;
    let cdat = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"CDAT", graph_data_end)?;
    if oidf.len() < 256 * 4 {
        return Err(CliError::Fatal {
            code: 1,
            message: "commit-graph OIDF chunk is truncated".into(),
        });
    }

    let mut previous = 0_u32;
    for idx in 0..256 {
        let count = read_u32_be(&oidf[idx * 4..idx * 4 + 4])?;
        if count < previous {
            return Err(CliError::Fatal {
                code: 1,
                message: "commit-graph OIDF fanout is not sorted".into(),
            });
        }
        previous = count;
    }
    let count = previous as usize;
    if oidl.len() < count * digest_len || cdat.len() < count * (digest_len + 16) {
        return Err(CliError::Fatal {
            code: 1,
            message: "commit-graph object chunks are truncated".into(),
        });
    }
    if !object_id_chunks_are_strictly_sorted(&oidl[..count * digest_len], digest_len) {
        return Err(CliError::Fatal {
            code: 1,
            message: "commit-graph object ids are not strictly sorted".into(),
        });
    }
    Ok(())
}

fn object_id_chunks_are_strictly_sorted(bytes: &[u8], digest_len: usize) -> bool {
    let mut chunks = bytes.chunks_exact(digest_len);
    let Some(mut previous) = chunks.next() else {
        return true;
    };
    for current in chunks {
        if previous >= current {
            return false;
        }
        previous = current;
    }
    true
}

#[derive(Debug, Clone)]
struct FsckOptions {
    unreachable: bool,
    dangling: bool,
    no_dangling: bool,
    strict: bool,
    full: bool,
    connectivity_only: bool,
    no_reflogs: bool,
    cache: bool,
    tags: bool,
    root: bool,
    verbose: bool,
    lost_found: bool,
    progress: bool,
    no_progress: bool,
    name_objects: bool,
    references: bool,
    no_references: bool,
    objects: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fsck(
    unreachable: bool,
    dangling: bool,
    no_dangling: bool,
    strict: bool,
    full: bool,
    connectivity_only: bool,
    no_reflogs: bool,
    cache: bool,
    tags: bool,
    root: bool,
    verbose: bool,
    lost_found: bool,
    progress: bool,
    no_progress: bool,
    name_objects: bool,
    references: bool,
    no_references: bool,
    objects: Vec<String>,
) -> Result<()> {
    fsck_impl(FsckOptions {
        unreachable,
        dangling,
        no_dangling,
        strict,
        full,
        connectivity_only,
        no_reflogs,
        cache,
        tags,
        root,
        verbose,
        lost_found,
        progress,
        no_progress,
        name_objects,
        references,
        no_references,
        objects,
    })
}

fn fsck_impl(options: FsckOptions) -> Result<()> {
    let repo = find_repo_or_bare()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut has_errors = false;
    let mut exit_code = 0;

    if !options.connectivity_only {
        let message_config = fsck_message_config(&repo)?;
        let mut checked_gitmodules_missing_entries = HashSet::new();
        let mut checked_gitmodules_parse_blobs = HashSet::new();
        let mut checked_gitmodules_path_blobs = HashSet::new();
        let mut checked_gitmodules_update_blobs = HashSet::new();
        let mut checked_gitmodules_url_blobs = HashSet::new();
        let _ = message_config.gitmodules_name;

        let mut check_object = |id: &ObjectId| {
            match store.read_object(id) {
                Ok(object) => {
                    let content = object.content.as_slice();
                    let decoded_commit = if object.kind == GitObjectKind::Commit {
                        decode_commit(GitHashAlgorithm::Sha1, content).ok()
                    } else {
                        None
                    };
                    if object.kind == GitObjectKind::Commit
                        && fsck_report_missing_commit_header(
                            id,
                            content,
                            message_config.missing_author,
                            b"author ",
                            "missingAuthor",
                            "expected 'author' line",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && fsck_report_missing_commit_header(
                            id,
                            content,
                            message_config.missing_committer,
                            b"committer ",
                            "missingCommitter",
                            "expected 'committer' line",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_bad_email(id, commit, message_config.bad_email)
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_missing_email(id, commit, message_config.missing_email)
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_missing_name_before_email(
                            id,
                            commit,
                            message_config.missing_name_before_email,
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_missing_space_before_email(
                            id,
                            commit,
                            message_config.missing_space_before_email,
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_missing_space_before_date(
                            id,
                            commit,
                            message_config.missing_space_before_date,
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_zero_padded_date(id, commit, message_config.zero_padded_date)
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_bad_timezone(id, commit, message_config.bad_timezone)
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Commit
                        && let Some(commit) = decoded_commit.as_ref()
                        && fsck_report_bad_date(id, commit, message_config.bad_date)
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && fsck_report_bad_tag_name(id, content, message_config.bad_tag_name)
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && let Ok(tagger) = fsck_tag_tagger(content)
                        && fsck_report_signature_issue(
                            "tag",
                            id,
                            &[tagger],
                            message_config.bad_date,
                            commit_signature_has_bad_date,
                            "badDate",
                            "bad date",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && let Ok(tagger) = fsck_tag_tagger(content)
                        && fsck_report_signature_issue(
                            "tag",
                            id,
                            &[tagger],
                            message_config.missing_name_before_email,
                            commit_signature_has_missing_name_before_email,
                            "missingNameBeforeEmail",
                            "missing space before email",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && let Ok(tagger) = fsck_tag_tagger(content)
                        && fsck_report_signature_issue(
                            "tag",
                            id,
                            &[tagger],
                            message_config.missing_space_before_email,
                            commit_signature_has_missing_space_before_email,
                            "missingSpaceBeforeEmail",
                            "missing space before email",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && let Ok(tagger) = fsck_tag_tagger(content)
                        && fsck_report_signature_issue(
                            "tag",
                            id,
                            &[tagger],
                            message_config.bad_email,
                            commit_signature_has_bad_email,
                            "badEmail",
                            "bad email",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && let Ok(tagger) = fsck_tag_tagger(content)
                        && fsck_report_signature_issue(
                            "tag",
                            id,
                            &[tagger],
                            message_config.missing_email,
                            commit_signature_has_missing_email,
                            "missingEmail",
                            "missing email",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && let Ok(tagger) = fsck_tag_tagger(content)
                        && fsck_report_signature_issue(
                            "tag",
                            id,
                            &[tagger],
                            message_config.missing_space_before_date,
                            commit_signature_has_missing_space_before_date,
                            "missingSpaceBeforeDate",
                            "missing space before date",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && let Ok(tagger) = fsck_tag_tagger(content)
                        && fsck_report_signature_issue(
                            "tag",
                            id,
                            &[tagger],
                            message_config.zero_padded_date,
                            commit_signature_has_zero_padded_date,
                            "zeroPaddedDate",
                            "zero-padded date",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tag
                        && fsck_report_missing_tag_header(
                            id,
                            content,
                            message_config.missing_tagger_entry,
                            b"tagger ",
                            "missingTaggerEntry",
                            "expected 'tagger' line",
                        )
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    let tree_scan = if object.kind == GitObjectKind::Tree {
                        fsck_scan_tree(content)
                    } else {
                        None
                    };
                    if let Some(scan) = tree_scan.as_ref() {
                        for (has_issue, severity, message_id, message) in [
                            (
                                scan.analysis.has_zero_padded_filemode,
                                message_config.zero_padded_filemode,
                                "zeroPaddedFilemode",
                                "contains zero-padded file modes",
                            ),
                            (
                                scan.analysis.has_bad_filemode,
                                message_config.bad_filemode,
                                "badFilemode",
                                "contains bad file modes",
                            ),
                            (
                                scan.analysis.has_duplicate_entries,
                                message_config.duplicate_entries,
                                "duplicateEntries",
                                "contains duplicate file entries",
                            ),
                            (
                                scan.analysis.has_null_sha1,
                                message_config.null_sha1,
                                "nullSha1",
                                "contains entries pointing to null sha1",
                            ),
                        ] {
                            if fsck_report_tree_issue(id, has_issue, severity, message_id, message)
                            {
                                has_errors = true;
                                if exit_code == 0 {
                                    exit_code = 1;
                                }
                            }
                        }
                    }
                    if object.kind == GitObjectKind::Tree
                        && tree_scan.as_ref().is_some_and(|scan| {
                            fsck_report_gitmodules_blob(
                                &scan.entries,
                                message_config.gitmodules_blob,
                            )
                        })
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tree
                        && tree_scan.as_ref().is_some_and(|scan| {
                            fsck_report_gitmodules_symlink(
                                id,
                                &scan.entries,
                                message_config.gitmodules_symlink,
                            )
                        })
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tree
                        && tree_scan.as_ref().is_some_and(|scan| {
                            fsck_report_gitmodules_missing(
                                &store,
                                &scan.entries,
                                message_config.gitmodules_missing,
                                &mut checked_gitmodules_missing_entries,
                            )
                        })
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tree
                        && tree_scan.as_ref().is_some_and(|scan| {
                            fsck_report_gitmodules_parse(
                                &store,
                                &scan.entries,
                                message_config.gitmodules_parse,
                                &mut checked_gitmodules_parse_blobs,
                            )
                        })
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tree
                        && tree_scan.as_ref().is_some_and(|scan| {
                            fsck_report_gitmodules_url(
                                &store,
                                &scan.entries,
                                message_config.gitmodules_url,
                                &mut checked_gitmodules_url_blobs,
                            )
                        })
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tree
                        && tree_scan.as_ref().is_some_and(|scan| {
                            fsck_report_gitmodules_path(
                                &store,
                                &scan.entries,
                                message_config.gitmodules_path,
                                &mut checked_gitmodules_path_blobs,
                            )
                        })
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if object.kind == GitObjectKind::Tree
                        && tree_scan.as_ref().is_some_and(|scan| {
                            fsck_report_gitmodules_update(
                                &store,
                                &scan.entries,
                                message_config.gitmodules_update,
                                &mut checked_gitmodules_update_blobs,
                            )
                        })
                    {
                        has_errors = true;
                        if exit_code == 0 {
                            exit_code = 1;
                        }
                    }
                    if let Some(scan) = tree_scan.as_ref() {
                        for (has_issue, severity, message_id, message) in [
                            (
                                scan.analysis.has_full_pathname,
                                message_config.full_pathname,
                                "fullPathname",
                                "contains full pathnames",
                            ),
                            (
                                scan.analysis.has_dot,
                                message_config.has_dot,
                                "hasDot",
                                "contains '.'",
                            ),
                            (
                                scan.analysis.has_dotdot,
                                message_config.has_dotdot,
                                "hasDotdot",
                                "contains '..'",
                            ),
                            (
                                scan.analysis.has_dotgit,
                                message_config.has_dotgit,
                                "hasDotgit",
                                "contains '.git'",
                            ),
                            (
                                scan.analysis.not_sorted,
                                message_config.tree_not_sorted,
                                "treeNotSorted",
                                "not properly sorted",
                            ),
                        ] {
                            if fsck_report_tree_issue(id, has_issue, severity, message_id, message)
                            {
                                has_errors = true;
                                if exit_code == 0 {
                                    exit_code = 1;
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    fsck_report_object_read_error(&repo, id, &error);
                    has_errors = true;
                    exit_code = 3;
                }
            }
            Ok(())
        };
        store.for_each_object_id(&mut check_object)?;
    }

    let roots = fsck_roots(&repo, &options)?;
    let mut seen = HashSet::with_capacity(fsck_seen_initial_capacity(
        store.object_id_capacity_hint()?,
        roots.len(),
    ));
    let mut has_connectivity_errors = false;
    for root in roots {
        if let Err(error) = fsck_mark_object(&store, &root, &mut seen, &mut has_connectivity_errors)
        {
            eprintln!("error: object {}: {}", root.to_hex(), error);
            has_errors = true;
            exit_code = 3;
        }
    }
    if has_connectivity_errors {
        has_errors = true;
        if exit_code == 0 {
            exit_code = 2;
        } else if exit_code == 1 {
            exit_code = 3;
        }
    }

    if options.unreachable || !options.no_dangling {
        let mut report_unreachable = |id: &ObjectId| {
            if !seen.contains(id) {
                let object = match store.read_object(id) {
                    Ok(object) => object,
                    Err(error) => {
                        if !has_errors {
                            fsck_report_object_read_error(&repo, id, &error);
                            has_errors = true;
                            exit_code = 3;
                        }
                        return Ok(());
                    }
                };
                let label = if options.unreachable {
                    "unreachable"
                } else {
                    "dangling"
                };
                println!("{label} {} {}", object.kind.as_str(), id.to_hex());
                if options.lost_found && !options.unreachable {
                    write_lost_found_object(&repo, &object, id)
                        .map_err(|error| io::Error::other(format!("{error:?}")))?;
                }
            }
            Ok(())
        };
        store.for_each_object_id(&mut report_unreachable)?;
    }
    if options.verbose && !has_errors {
        eprintln!("Checking object directories: 100% ({}/{}), done.", 1, 1);
    }
    let _ = (
        options.strict,
        options.full,
        options.connectivity_only,
        options.dangling,
        options.tags,
        options.root,
        options.progress,
        options.no_progress,
        options.name_objects,
        options.references,
    );
    if has_errors {
        Err(CliError::Exit(exit_code))
    } else {
        Ok(())
    }
}

fn fsck_report_object_read_error(repo: &GitRepo, id: &ObjectId, error: &io::Error) {
    let message = error.to_string();
    if message == "loose git object hash mismatch" {
        eprintln!("error: hash-path mismatch: {}", id.to_hex());
        return;
    }
    if message == "corrupt deflate stream" {
        let path = fsck_object_git_path(repo, id);
        eprintln!("error: inflate: data stream error (incorrect header check)");
        eprintln!("error: unable to unpack header of ./{path}");
        eprintln!(
            "error: {}: object corrupt or missing: ./{path}",
            id.to_hex()
        );
        return;
    }
    eprintln!("error: object {}: {}", id.to_hex(), error);
}

fn fsck_object_git_path(repo: &GitRepo, id: &ObjectId) -> String {
    let hex = id.to_hex();
    let path = repo
        .objects_dir
        .join(&hex[..2])
        .join(&hex[2..])
        .display()
        .to_string();
    git_path_output_string(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FsckMessageSeverity {
    Error,
    Warn,
    Ignore,
}

#[derive(Debug, Clone, Copy)]
struct FsckMessageConfig {
    bad_date: FsckMessageSeverity,
    bad_email: FsckMessageSeverity,
    bad_filemode: FsckMessageSeverity,
    bad_tag_name: FsckMessageSeverity,
    bad_timezone: FsckMessageSeverity,
    duplicate_entries: FsckMessageSeverity,
    full_pathname: FsckMessageSeverity,
    gitmodules_blob: FsckMessageSeverity,
    gitmodules_missing: FsckMessageSeverity,
    gitmodules_name: FsckMessageSeverity,
    gitmodules_parse: FsckMessageSeverity,
    gitmodules_path: FsckMessageSeverity,
    gitmodules_symlink: FsckMessageSeverity,
    gitmodules_update: FsckMessageSeverity,
    gitmodules_url: FsckMessageSeverity,
    has_dot: FsckMessageSeverity,
    has_dotdot: FsckMessageSeverity,
    has_dotgit: FsckMessageSeverity,
    missing_author: FsckMessageSeverity,
    missing_committer: FsckMessageSeverity,
    missing_email: FsckMessageSeverity,
    missing_name_before_email: FsckMessageSeverity,
    missing_space_before_date: FsckMessageSeverity,
    missing_space_before_email: FsckMessageSeverity,
    missing_tagger_entry: FsckMessageSeverity,
    null_sha1: FsckMessageSeverity,
    tree_not_sorted: FsckMessageSeverity,
    zero_padded_date: FsckMessageSeverity,
    zero_padded_filemode: FsckMessageSeverity,
}

fn fsck_message_config(repo: &GitRepo) -> Result<FsckMessageConfig> {
    Ok(FsckMessageConfig {
        bad_date: fsck_message_severity(repo, "baddate", FsckMessageSeverity::Error)?,
        bad_email: fsck_message_severity(repo, "bademail", FsckMessageSeverity::Error)?,
        bad_filemode: fsck_message_severity(repo, "badfilemode", FsckMessageSeverity::Warn)?,
        bad_tag_name: fsck_message_severity(repo, "badtagname", FsckMessageSeverity::Warn)?,
        bad_timezone: fsck_message_severity(repo, "badtimezone", FsckMessageSeverity::Error)?,
        duplicate_entries: fsck_message_severity(
            repo,
            "duplicateentries",
            FsckMessageSeverity::Error,
        )?,
        full_pathname: fsck_message_severity(repo, "fullpathname", FsckMessageSeverity::Warn)?,
        gitmodules_blob: fsck_message_severity(repo, "gitmodulesblob", FsckMessageSeverity::Error)?,
        gitmodules_missing: fsck_message_severity(
            repo,
            "gitmodulesmissing",
            FsckMessageSeverity::Error,
        )?,
        gitmodules_name: fsck_message_severity(repo, "gitmodulesname", FsckMessageSeverity::Warn)?,
        gitmodules_parse: fsck_message_severity(
            repo,
            "gitmodulesparse",
            FsckMessageSeverity::Warn,
        )?,
        gitmodules_path: fsck_message_severity(repo, "gitmodulespath", FsckMessageSeverity::Error)?,
        gitmodules_symlink: fsck_message_severity(
            repo,
            "gitmodulessymlink",
            FsckMessageSeverity::Error,
        )?,
        gitmodules_update: fsck_message_severity(
            repo,
            "gitmodulesupdate",
            FsckMessageSeverity::Error,
        )?,
        gitmodules_url: fsck_message_severity(repo, "gitmodulesurl", FsckMessageSeverity::Error)?,
        has_dot: fsck_message_severity(repo, "hasdot", FsckMessageSeverity::Warn)?,
        has_dotdot: fsck_message_severity(repo, "hasdotdot", FsckMessageSeverity::Warn)?,
        has_dotgit: fsck_message_severity(repo, "hasdotgit", FsckMessageSeverity::Warn)?,
        missing_author: fsck_message_severity(repo, "missingauthor", FsckMessageSeverity::Error)?,
        missing_committer: fsck_message_severity(
            repo,
            "missingcommitter",
            FsckMessageSeverity::Error,
        )?,
        missing_email: fsck_message_severity(repo, "missingemail", FsckMessageSeverity::Error)?,
        missing_name_before_email: fsck_message_severity(
            repo,
            "missingnamebeforeemail",
            FsckMessageSeverity::Error,
        )?,
        missing_space_before_date: fsck_message_severity(
            repo,
            "missingspacebeforedate",
            FsckMessageSeverity::Error,
        )?,
        missing_space_before_email: fsck_message_severity(
            repo,
            "missingspacebeforeemail",
            FsckMessageSeverity::Error,
        )?,
        missing_tagger_entry: fsck_message_severity(
            repo,
            "missingtaggerentry",
            FsckMessageSeverity::Warn,
        )?,
        null_sha1: fsck_message_severity(repo, "nullsha1", FsckMessageSeverity::Warn)?,
        tree_not_sorted: fsck_message_severity(repo, "treenotsorted", FsckMessageSeverity::Error)?,
        zero_padded_date: fsck_message_severity(
            repo,
            "zeropaddeddate",
            FsckMessageSeverity::Error,
        )?,
        zero_padded_filemode: fsck_message_severity(
            repo,
            "zeropaddedfilemode",
            FsckMessageSeverity::Warn,
        )?,
    })
}

fn fsck_message_severity(
    repo: &GitRepo,
    message_id: &str,
    default: FsckMessageSeverity,
) -> Result<FsckMessageSeverity> {
    let key = format!("fsck.{message_id}");
    let Some(value) = read_config_value(repo, &key)? else {
        return Ok(default);
    };
    match value.to_ascii_lowercase().as_str() {
        "error" => Ok(FsckMessageSeverity::Error),
        "warn" => Ok(FsckMessageSeverity::Warn),
        "ignore" => Ok(FsckMessageSeverity::Ignore),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("Unknown fsck message type: '{value}'"),
        }),
    }
}

fn apply_fsck_message_overrides(config: &mut FsckMessageConfig, raw: &str) -> Result<()> {
    for override_item in raw.split(',').filter(|item| !item.is_empty()) {
        let (message_id, severity) =
            override_item
                .split_once('=')
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: format!("Unknown fsck message type: '{override_item}'"),
                })?;
        let severity = match severity.to_ascii_lowercase().as_str() {
            "error" => FsckMessageSeverity::Error,
            "warn" => FsckMessageSeverity::Warn,
            "ignore" => FsckMessageSeverity::Ignore,
            _ => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("Unknown fsck message type: '{severity}'"),
                });
            }
        };
        set_fsck_message_severity(config, message_id, severity)?;
    }
    Ok(())
}

fn set_fsck_message_severity(
    config: &mut FsckMessageConfig,
    message_id: &str,
    severity: FsckMessageSeverity,
) -> Result<()> {
    let slot = match message_id.to_ascii_lowercase().as_str() {
        "baddate" => &mut config.bad_date,
        "bademail" => &mut config.bad_email,
        "badfilemode" => &mut config.bad_filemode,
        "badtagname" => &mut config.bad_tag_name,
        "badtimezone" => &mut config.bad_timezone,
        "duplicateentries" => &mut config.duplicate_entries,
        "fullpathname" => &mut config.full_pathname,
        "gitmodulesblob" => &mut config.gitmodules_blob,
        "gitmodulesmissing" => &mut config.gitmodules_missing,
        "gitmodulesname" => &mut config.gitmodules_name,
        "gitmodulesparse" => &mut config.gitmodules_parse,
        "gitmodulespath" => &mut config.gitmodules_path,
        "gitmodulessymlink" => &mut config.gitmodules_symlink,
        "gitmodulesupdate" => &mut config.gitmodules_update,
        "gitmodulesurl" => &mut config.gitmodules_url,
        "hasdot" => &mut config.has_dot,
        "hasdotdot" => &mut config.has_dotdot,
        "hasdotgit" => &mut config.has_dotgit,
        "missingauthor" => &mut config.missing_author,
        "missingcommitter" => &mut config.missing_committer,
        "missingemail" => &mut config.missing_email,
        "missingnamebeforeemail" => &mut config.missing_name_before_email,
        "missingspacebeforedate" => &mut config.missing_space_before_date,
        "missingspacebeforeemail" => &mut config.missing_space_before_email,
        "missingtaggerentry" => &mut config.missing_tagger_entry,
        "nullsha1" => &mut config.null_sha1,
        "treenotsorted" => &mut config.tree_not_sorted,
        "zeropaddeddate" => &mut config.zero_padded_date,
        "zeropaddedfilemode" => &mut config.zero_padded_filemode,
        _ => {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("Unhandled message id: {message_id}"),
            });
        }
    };
    *slot = severity;
    Ok(())
}

fn fsck_report_missing_commit_header(
    id: &ObjectId,
    content: &[u8],
    severity: FsckMessageSeverity,
    header_prefix: &[u8],
    message_id: &str,
    message: &str,
) -> bool {
    if fsck_commit_has_header(content, header_prefix) {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: {message_id}: invalid format - {message}",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: {message_id}: invalid format - {message}",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_missing_tag_header(
    id: &ObjectId,
    content: &[u8],
    severity: FsckMessageSeverity,
    header_prefix: &[u8],
    message_id: &str,
    message: &str,
) -> bool {
    if fsck_tag_has_header(content, header_prefix) {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in tag {}: {message_id}: invalid format - {message}",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in tag {}: {message_id}: invalid format - {message}",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_zero_padded_date(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    if !commit_signature_has_zero_padded_date(&commit.author)
        && !commit_signature_has_zero_padded_date(&commit.committer)
    {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: zeroPaddedDate: invalid author/committer line - zero-padded date",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: zeroPaddedDate: invalid author/committer line - zero-padded date",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_missing_name_before_email(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    if !commit_signature_has_missing_name_before_email(&commit.author)
        && !commit_signature_has_missing_name_before_email(&commit.committer)
    {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: missingNameBeforeEmail: invalid author/committer line - missing space before email",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: missingNameBeforeEmail: invalid author/committer line - missing space before email",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_missing_space_before_date(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    if !commit_signature_has_missing_space_before_date(&commit.author)
        && !commit_signature_has_missing_space_before_date(&commit.committer)
    {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: missingSpaceBeforeDate: invalid author/committer line - missing space before date",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: missingSpaceBeforeDate: invalid author/committer line - missing space before date",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_missing_space_before_email(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    if !commit_signature_has_missing_space_before_email(&commit.author)
        && !commit_signature_has_missing_space_before_email(&commit.committer)
    {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: missingSpaceBeforeEmail: invalid author/committer line - missing space before email",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: missingSpaceBeforeEmail: invalid author/committer line - missing space before email",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_bad_timezone(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    if !commit_signature_has_bad_timezone(&commit.author)
        && !commit_signature_has_bad_timezone(&commit.committer)
    {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: badTimezone: invalid author/committer line - bad time zone",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: badTimezone: invalid author/committer line - bad time zone",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_bad_date(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    fsck_report_signature_issue(
        "commit",
        id,
        &[commit.author.as_slice(), commit.committer.as_slice()],
        severity,
        commit_signature_has_bad_date,
        "badDate",
        "bad date",
    )
}

fn fsck_report_bad_email(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    if !commit_signature_has_bad_email(&commit.author)
        && !commit_signature_has_bad_email(&commit.committer)
    {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: badEmail: invalid author/committer line - bad email",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: badEmail: invalid author/committer line - bad email",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_tree_issue(
    id: &ObjectId,
    has_issue: bool,
    severity: FsckMessageSeverity,
    message_id: &str,
    message: &str,
) -> bool {
    if !has_issue {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!("error in tree {}: {message_id}: {message}", id.to_hex());
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!("warning in tree {}: {message_id}: {message}", id.to_hex());
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_bad_tag_name(id: &ObjectId, content: &[u8], severity: FsckMessageSeverity) -> bool {
    let Ok(name) = fsck_tag_name(content) else {
        return false;
    };
    if check_ref_format(name, true) {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in tag {}: badTagName: invalid 'tag' name: {name}",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in tag {}: badTagName: invalid 'tag' name: {name}",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_gitmodules_parse(
    store: &LooseObjectStore,
    entries: &[FsckTreeEntry],
    severity: FsckMessageSeverity,
    checked_blobs: &mut HashSet<ObjectId>,
) -> bool {
    let mut has_error = false;
    fsck_for_each_gitmodules_blob(store, entries, checked_blobs, |id, blob| {
        if gitmodules_blob_entries(blob).is_some() {
            return;
        }
        match severity {
            FsckMessageSeverity::Error => {
                eprintln!(
                    "error in blob {}: gitmodulesParse: could not parse gitmodules blob",
                    id.to_hex()
                );
                has_error = true;
            }
            FsckMessageSeverity::Warn => {
                eprintln!(
                    "warning in blob {}: gitmodulesParse: could not parse gitmodules blob",
                    id.to_hex()
                );
            }
            FsckMessageSeverity::Ignore => {}
        }
    });
    has_error
}

fn fsck_report_gitmodules_blob(entries: &[FsckTreeEntry], severity: FsckMessageSeverity) -> bool {
    let non_blob = entries.iter().find(|entry| {
        entry.mode != TreeMode::File
            && entry.mode != TreeMode::Executable
            && entry.mode != TreeMode::Symlink
            && entry.name.eq_ignore_ascii_case(b".gitmodules")
    });
    let Some(entry) = non_blob else { return false };
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in tree {}: gitmodulesBlob: non-blob found at .gitmodules",
                entry.id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in tree {}: gitmodulesBlob: non-blob found at .gitmodules",
                entry.id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_gitmodules_symlink(
    id: &ObjectId,
    entries: &[FsckTreeEntry],
    severity: FsckMessageSeverity,
) -> bool {
    let has_symlink = entries.iter().any(|entry| {
        entry.mode == TreeMode::Symlink && entry.name.eq_ignore_ascii_case(b".gitmodules")
    });
    if !has_symlink {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in tree {}: gitmodulesSymlink: .gitmodules is a symbolic link",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in tree {}: gitmodulesSymlink: .gitmodules is a symbolic link",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn fsck_report_gitmodules_missing(
    store: &LooseObjectStore,
    entries: &[FsckTreeEntry],
    severity: FsckMessageSeverity,
    checked_entries: &mut HashSet<ObjectId>,
) -> bool {
    let mut has_error = false;
    for entry in entries {
        if entry.mode == TreeMode::Tree
            || entry.mode == TreeMode::Symlink
            || !entry.name.eq_ignore_ascii_case(b".gitmodules")
            || !checked_entries.insert(entry.id.clone())
        {
            continue;
        }
        let Ok(object) = store.read_object(&entry.id) else {
            match severity {
                FsckMessageSeverity::Error => {
                    eprintln!(
                        "error in blob {}: gitmodulesMissing: unable to read .gitmodules blob",
                        entry.id.to_hex()
                    );
                    has_error = true;
                }
                FsckMessageSeverity::Warn => {
                    eprintln!(
                        "warning in blob {}: gitmodulesMissing: unable to read .gitmodules blob",
                        entry.id.to_hex()
                    );
                }
                FsckMessageSeverity::Ignore => {}
            }
            continue;
        };
        if entry.mode != TreeMode::Gitlink && object.kind == GitObjectKind::Blob {
            continue;
        }
        if entry.mode == TreeMode::Gitlink {
            continue;
        }
    }
    has_error
}

fn fsck_report_gitmodules_url(
    store: &LooseObjectStore,
    entries: &[FsckTreeEntry],
    severity: FsckMessageSeverity,
    checked_blobs: &mut HashSet<ObjectId>,
) -> bool {
    let mut has_error = false;
    fsck_for_each_gitmodules_blob(store, entries, checked_blobs, |id, blob| {
        let Some(entries) = gitmodules_blob_entries(blob) else {
            return;
        };
        for entry in entries {
            if entry.section != "submodule" || entry.key != "url" || !entry.value.starts_with('-') {
                continue;
            }
            match severity {
                FsckMessageSeverity::Error => {
                    eprintln!(
                        "error in blob {}: gitmodulesUrl: disallowed submodule url: {}",
                        id.to_hex(),
                        entry.value
                    );
                    has_error = true;
                }
                FsckMessageSeverity::Warn => {
                    eprintln!(
                        "warning in blob {}: gitmodulesUrl: disallowed submodule url: {}",
                        id.to_hex(),
                        entry.value
                    );
                }
                FsckMessageSeverity::Ignore => {}
            }
        }
    });
    has_error
}

fn fsck_report_gitmodules_path(
    store: &LooseObjectStore,
    entries: &[FsckTreeEntry],
    severity: FsckMessageSeverity,
    checked_blobs: &mut HashSet<ObjectId>,
) -> bool {
    let mut has_error = false;
    fsck_for_each_gitmodules_blob(store, entries, checked_blobs, |id, blob| {
        let Some(entries) = gitmodules_blob_entries(blob) else {
            return;
        };
        for entry in entries {
            if entry.section != "submodule" || entry.key != "path" || !entry.value.starts_with('-')
            {
                continue;
            }
            match severity {
                FsckMessageSeverity::Error => {
                    eprintln!(
                        "error in blob {}: gitmodulesPath: disallowed submodule path: {}",
                        id.to_hex(),
                        entry.value
                    );
                    has_error = true;
                }
                FsckMessageSeverity::Warn => {
                    eprintln!(
                        "warning in blob {}: gitmodulesPath: disallowed submodule path: {}",
                        id.to_hex(),
                        entry.value
                    );
                }
                FsckMessageSeverity::Ignore => {}
            }
        }
    });
    has_error
}

fn fsck_report_gitmodules_update(
    store: &LooseObjectStore,
    entries: &[FsckTreeEntry],
    severity: FsckMessageSeverity,
    checked_blobs: &mut HashSet<ObjectId>,
) -> bool {
    let mut has_error = false;
    fsck_for_each_gitmodules_blob(store, entries, checked_blobs, |id, blob| {
        let Some(entries) = gitmodules_blob_entries(blob) else {
            return;
        };
        for entry in entries {
            if entry.section != "submodule"
                || entry.key != "update"
                || !entry.value.starts_with('!')
            {
                continue;
            }
            match severity {
                FsckMessageSeverity::Error => {
                    eprintln!(
                        "error in blob {}: gitmodulesUpdate: disallowed submodule update setting: {}",
                        id.to_hex(),
                        entry.value
                    );
                    has_error = true;
                }
                FsckMessageSeverity::Warn => {
                    eprintln!(
                        "warning in blob {}: gitmodulesUpdate: disallowed submodule update setting: {}",
                        id.to_hex(),
                        entry.value
                    );
                }
                FsckMessageSeverity::Ignore => {}
            }
        }
    });
    has_error
}

fn fsck_for_each_gitmodules_blob(
    store: &LooseObjectStore,
    entries: &[FsckTreeEntry],
    checked_blobs: &mut HashSet<ObjectId>,
    mut visit: impl FnMut(&ObjectId, &[u8]),
) {
    for entry in entries {
        if entry.mode == TreeMode::Gitlink
            || entry.mode == TreeMode::Symlink
            || entry.mode == TreeMode::Tree
            || !entry.name.eq_ignore_ascii_case(b".gitmodules")
            || !checked_blobs.insert(entry.id.clone())
        {
            continue;
        }
        let Ok(object) = store.read_object(&entry.id) else {
            continue;
        };
        if object.kind == GitObjectKind::Blob {
            visit(&entry.id, &object.content);
        }
    }
}

fn gitmodules_blob_entries(content: &[u8]) -> Option<Vec<ConfigEntry>> {
    let Ok(content) = std::str::from_utf8(content) else {
        return None;
    };
    let mut has_section = false;
    let mut current_section = None::<(String, String)>;
    let mut entries = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') {
            if !trimmed.ends_with(']') || trimmed.len() <= 2 {
                return None;
            }
            let (section, _) = parse_config_section(&trimmed[1..trimmed.len() - 1]);
            if section.trim().is_empty() {
                return None;
            }
            current_section = Some(parse_config_section(&trimmed[1..trimmed.len() - 1]));
            has_section = true;
            continue;
        }
        if !has_section {
            return None;
        }
        let key = trimmed
            .split_once('=')
            .map(|(key, _)| key.trim())
            .unwrap_or(trimmed);
        if key.is_empty() {
            return None;
        }
        let (section, subsection) = current_section.as_ref()?;
        let (key, value, implicit_bool) = trimmed
            .split_once('=')
            .map(|(key, value)| (key.trim(), value.trim(), false))
            .unwrap_or((trimmed, "", true));
        entries.push(ConfigEntry {
            section: section.to_ascii_lowercase(),
            subsection: subsection.clone(),
            key: key.to_ascii_lowercase(),
            value: decode_config_value(value),
            implicit_bool,
            scope: ConfigScope::Local,
            origin: String::new(),
            line: None,
        });
    }
    Some(entries)
}

fn fsck_report_missing_email(
    id: &ObjectId,
    commit: &CommitObject,
    severity: FsckMessageSeverity,
) -> bool {
    if !commit_signature_has_missing_email(&commit.author)
        && !commit_signature_has_missing_email(&commit.committer)
    {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in commit {}: missingEmail: invalid author/committer line - missing email",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in commit {}: missingEmail: invalid author/committer line - missing email",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn commit_signature_has_missing_email(raw: &[u8]) -> bool {
    !raw.contains(&b'<')
}

fn fsck_report_signature_issue(
    object_kind: &str,
    id: &ObjectId,
    signatures: &[&[u8]],
    severity: FsckMessageSeverity,
    predicate: fn(&[u8]) -> bool,
    message_id: &str,
    message: &str,
) -> bool {
    if !signatures.iter().any(|signature| predicate(signature)) {
        return false;
    }
    match severity {
        FsckMessageSeverity::Error => {
            eprintln!(
                "error in {object_kind} {}: {message_id}: invalid author/committer line - {message}",
                id.to_hex()
            );
            true
        }
        FsckMessageSeverity::Warn => {
            eprintln!(
                "warning in {object_kind} {}: {message_id}: invalid author/committer line - {message}",
                id.to_hex()
            );
            false
        }
        FsckMessageSeverity::Ignore => false,
    }
}

fn commit_signature_has_bad_email(raw: &[u8]) -> bool {
    let Some(email_start) = raw.iter().position(|byte| *byte == b'<') else {
        return false;
    };
    !raw[email_start + 1..].contains(&b'>')
}

fn commit_signature_has_missing_space_before_email(raw: &[u8]) -> bool {
    let Some(email_start) = raw.iter().position(|byte| *byte == b'<') else {
        return false;
    };
    email_start > 0 && raw[email_start - 1] != b' '
}

fn commit_signature_has_missing_name_before_email(raw: &[u8]) -> bool {
    let Some(email_start) = raw.iter().position(|byte| *byte == b'<') else {
        return false;
    };
    raw[..email_start].iter().all(u8::is_ascii_whitespace)
}

fn commit_signature_has_missing_space_before_date(raw: &[u8]) -> bool {
    let Some(email_end) = raw.iter().position(|byte| *byte == b'>') else {
        return false;
    };
    raw.get(email_end + 1)
        .is_some_and(|byte| byte.is_ascii_digit())
}

fn commit_signature_has_zero_padded_date(raw: &[u8]) -> bool {
    let Some(date) = commit_signature_date(raw) else {
        return false;
    };
    date.len() > 1 && date[0] == b'0' && date.iter().all(u8::is_ascii_digit)
}

fn commit_signature_date(raw: &[u8]) -> Option<&[u8]> {
    let email_end = raw.iter().position(|byte| *byte == b'>')?;
    let rest = raw.get(email_end + 1..)?.strip_prefix(b" ")?;
    let date_end = rest.iter().position(|byte| *byte == b' ')?;
    Some(&rest[..date_end])
}

fn commit_signature_has_bad_date(raw: &[u8]) -> bool {
    let Some(date) = commit_signature_date(raw) else {
        return false;
    };
    date.is_empty() || !date.iter().all(u8::is_ascii_digit)
}

fn commit_signature_has_bad_timezone(raw: &[u8]) -> bool {
    let Some(timezone) = commit_signature_timezone(raw) else {
        return false;
    };
    !fsck_timezone_is_valid(timezone)
}

fn commit_signature_timezone(raw: &[u8]) -> Option<&[u8]> {
    let email_end = raw.iter().position(|byte| *byte == b'>')?;
    let rest = raw.get(email_end + 1..)?.strip_prefix(b" ")?;
    let date_end = rest.iter().position(|byte| *byte == b' ')?;
    let date = commit_signature_date(raw)?;
    if date.is_empty() || !date.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(&rest[date_end + 1..])
}

fn fsck_timezone_is_valid(timezone: &[u8]) -> bool {
    if timezone.len() != 5
        || (timezone[0] != b'+' && timezone[0] != b'-')
        || !timezone[1..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let hours = u16::from(timezone[1] - b'0') * 10 + u16::from(timezone[2] - b'0');
    let minutes = u16::from(timezone[3] - b'0') * 10 + u16::from(timezone[4] - b'0');
    hours <= 23 && minutes <= 59
}

fn fsck_commit_has_header(content: &[u8], header_prefix: &[u8]) -> bool {
    let Some(headers) = fsck_commit_headers(content) else {
        return false;
    };
    headers
        .split(|byte| *byte == b'\n')
        .any(|line| line.starts_with(header_prefix))
}

fn fsck_tag_has_header(content: &[u8], header_prefix: &[u8]) -> bool {
    let Some(headers) = fsck_tag_headers(content) else {
        return false;
    };
    headers
        .split(|byte| *byte == b'\n')
        .any(|line| line.starts_with(header_prefix))
}

fn fsck_commit_headers(content: &[u8]) -> Option<&[u8]> {
    content
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|message_start| &content[..message_start])
}

fn fsck_tag_headers(content: &[u8]) -> Option<&[u8]> {
    content
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|message_start| &content[..message_start])
}

fn fsck_tag_tagger(content: &[u8]) -> io::Result<&[u8]> {
    let headers = fsck_tag_headers(content)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing header end"))?;
    for line in headers.split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"tagger ") {
            return Ok(value);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "tag missing tagger",
    ))
}

fn fsck_tag_name(content: &[u8]) -> io::Result<&str> {
    let headers = fsck_tag_headers(content)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing header end"))?;
    for line in headers.split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"tag ") {
            return std::str::from_utf8(value)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "tag name is not utf-8"));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "tag missing name",
    ))
}

#[derive(Debug, Default)]
struct FsckTreeAnalysis {
    has_zero_padded_filemode: bool,
    has_bad_filemode: bool,
    has_duplicate_entries: bool,
    has_full_pathname: bool,
    has_null_sha1: bool,
    has_dot: bool,
    has_dotdot: bool,
    has_dotgit: bool,
    not_sorted: bool,
}

struct FsckTreeScan {
    entries: Vec<FsckTreeEntry>,
    analysis: FsckTreeAnalysis,
}

fn fsck_scan_tree(bytes: &[u8]) -> Option<FsckTreeScan> {
    let Ok((entries, has_zero_padded_filemode)) =
        fsck_decode_tree_entries(GitHashAlgorithm::Sha1, bytes)
    else {
        return None;
    };
    let analysis = fsck_analyze_tree_entries(&entries, has_zero_padded_filemode);
    Some(FsckTreeScan { entries, analysis })
}

fn fsck_analyze_tree(bytes: &[u8]) -> Option<FsckTreeAnalysis> {
    let Ok((entries, has_zero_padded_filemode)) =
        fsck_decode_tree_entries(GitHashAlgorithm::Sha1, bytes)
    else {
        return None;
    };
    Some(fsck_analyze_tree_entries(
        &entries,
        has_zero_padded_filemode,
    ))
}

fn fsck_analyze_tree_entries(
    entries: &[FsckTreeEntry],
    has_zero_padded_filemode: bool,
) -> FsckTreeAnalysis {
    let mut analysis = FsckTreeAnalysis {
        has_zero_padded_filemode,
        ..FsckTreeAnalysis::default()
    };
    let mut names = HashSet::with_capacity(fsck_tree_entry_name_initial_capacity(entries.len()));
    let mut previous = None;
    for entry in entries {
        let name = entry.name.as_slice();
        analysis.has_bad_filemode |= entry.bad_filemode;
        if !names.insert(name) {
            analysis.has_duplicate_entries = true;
        }
        analysis.has_full_pathname |= name.contains(&b'/');
        analysis.has_null_sha1 |= object_id_is_null(&entry.id);
        analysis.has_dot |= name == b".";
        analysis.has_dotdot |= name == b"..";
        analysis.has_dotgit |= fsck_tree_name_matches_dotgit(name);
        if let Some(previous_entry) = previous
            && fsck_compare_tree_entries(previous_entry, entry).is_gt()
        {
            analysis.not_sorted = true;
        }
        previous = Some(entry);
    }
    analysis
}

fn fsck_tree_name_matches_dotgit(name: &[u8]) -> bool {
    let candidate = name.split(|byte| *byte == b'/').next().unwrap_or(name);
    let mut candidate = candidate;
    while candidate.ends_with(b".") {
        candidate = &candidate[..candidate.len() - 1];
    }
    candidate.eq_ignore_ascii_case(b".git") || candidate.eq_ignore_ascii_case(b"git~1")
}

fn fsck_tree_entry_name_initial_capacity(entry_count: usize) -> usize {
    entry_count
        .min(FSCK_TREE_ENTRY_NAME_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn write_lost_found_object(repo: &GitRepo, object: &LooseObject, id: &ObjectId) -> Result<()> {
    let (dir_name, content) = match object.kind {
        GitObjectKind::Commit => ("commit", id.to_hex().into_bytes()),
        GitObjectKind::Blob | GitObjectKind::Tree | GitObjectKind::Tag => {
            ("other", object.content.clone())
        }
    };
    let dir = repo.git_dir.join("lost-found").join(dir_name);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(id.to_hex()), content)?;
    Ok(())
}

fn fsck_roots(repo: &GitRepo, options: &FsckOptions) -> Result<Vec<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut roots = Vec::new();
    if options.objects.is_empty() {
        if !options.no_references {
            if let Ok(id) = refs.resolve("HEAD") {
                roots.push(id);
            }
            refs.for_each_resolved_ref("refs/", |_, id| {
                roots.push(id.clone());
                Ok::<(), CliError>(())
            })?;
        }
        if !options.no_reflogs {
            collect_reflog_roots(repo, &mut roots)?;
        }
        if options.cache && repo.index_path.exists() {
            let index = read_index(&repo.index_path)?;
            roots.extend(
                index
                    .entries()
                    .iter()
                    .filter(|entry| entry.mode != IndexMode::Gitlink)
                    .map(|entry| entry.id.clone()),
            );
        }
    } else {
        for object in &options.objects {
            roots.push(resolve_objectish(repo, object)?);
        }
    }
    roots.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    roots.dedup_by(|left, right| left.as_bytes() == right.as_bytes());
    Ok(roots)
}

fn fsck_seen_initial_capacity(store_hint: usize, roots_len: usize) -> usize {
    store_hint
        .min(FSCK_SEEN_INITIAL_CAPACITY_LIMIT)
        .max(roots_len)
        .max(1)
}

fn fsck_mark_object(
    store: &LooseObjectStore,
    id: &ObjectId,
    seen: &mut HashSet<ObjectId>,
    has_connectivity_errors: &mut bool,
) -> io::Result<()> {
    if !seen.insert(id.clone()) {
        return Ok(());
    }
    let object = store.read_object(id)?;
    let content = object.content.as_slice();
    match object.kind {
        GitObjectKind::Blob => {}
        GitObjectKind::Tree => {
            let (entries, _) = fsck_decode_tree_entries(id.algorithm(), content)?;
            for entry in entries {
                if entry.mode == TreeMode::Gitlink {
                    continue;
                }
                if object_id_is_null(&entry.id) {
                    *has_connectivity_errors = true;
                    fsck_print_missing_tree_entry(id, entry.mode, &entry.id);
                } else {
                    match fsck_mark_object(store, &entry.id, seen, has_connectivity_errors) {
                        Ok(()) => {}
                        Err(err) if err.kind() == io::ErrorKind::NotFound => {
                            *has_connectivity_errors = true;
                            fsck_print_missing_tree_entry(id, entry.mode, &entry.id);
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
        }
        GitObjectKind::Commit => {
            let (tree, parents) = fsck_commit_links(GitHashAlgorithm::Sha1, content)?;
            fsck_mark_object(store, &tree, seen, has_connectivity_errors)?;
            for parent in parents {
                fsck_mark_object(store, &parent, seen, has_connectivity_errors)?;
            }
        }
        GitObjectKind::Tag => {
            let target = fsck_tag_target(GitHashAlgorithm::Sha1, content)?;
            fsck_mark_object(store, &target, seen, has_connectivity_errors)?;
        }
    }
    Ok(())
}

fn fsck_print_missing_tree_entry(tree_id: &ObjectId, mode: TreeMode, entry_id: &ObjectId) {
    let target_kind = fsck_tree_entry_object_kind(mode);
    println!(
        "broken link from    tree {}\n              to    {} {}",
        tree_id.to_hex(),
        target_kind,
        entry_id.to_hex()
    );
    println!("missing {} {}", target_kind, entry_id.to_hex());
}

fn object_id_is_null(id: &ObjectId) -> bool {
    id.as_bytes().iter().all(|byte| *byte == 0)
}

fn fsck_tree_entry_object_kind(mode: TreeMode) -> &'static str {
    match mode {
        TreeMode::Tree => "tree",
        TreeMode::File | TreeMode::Executable | TreeMode::Symlink => "blob",
        TreeMode::Gitlink => "commit",
    }
}

fn fsck_commit_links(
    algorithm: GitHashAlgorithm,
    content: &[u8],
) -> io::Result<(ObjectId, Vec<ObjectId>)> {
    let headers = fsck_commit_headers(content)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "commit missing header end"))?;
    let mut tree = None;
    let mut parents = Vec::with_capacity(1);
    for line in headers.split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"tree ") {
            if tree.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "commit has multiple tree headers",
                ));
            }
            tree = Some(parse_fsck_commit_id(algorithm, value, "tree")?);
        } else if let Some(value) = line.strip_prefix(b"parent ") {
            parents.push(parse_fsck_commit_id(algorithm, value, "parent")?);
        }
    }
    let tree = tree
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "commit missing tree header"))?;
    Ok((tree, parents))
}

fn parse_fsck_commit_id(
    algorithm: GitHashAlgorithm,
    value: &[u8],
    label: &str,
) -> io::Result<ObjectId> {
    let value = std::str::from_utf8(value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("commit {label} id is not utf-8"),
        )
    })?;
    ObjectId::from_hex(algorithm, value).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("commit {label} id is invalid"),
        )
    })
}

fn fsck_tag_target(algorithm: GitHashAlgorithm, content: &[u8]) -> io::Result<ObjectId> {
    let headers = fsck_tag_headers(content)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tag missing header end"))?;
    for line in headers.split(|byte| *byte == b'\n') {
        if line.starts_with(b" ") {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"object ") {
            return parse_fsck_tag_id(algorithm, value);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "tag missing object",
    ))
}

fn parse_fsck_tag_id(algorithm: GitHashAlgorithm, value: &[u8]) -> io::Result<ObjectId> {
    let value = std::str::from_utf8(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "tag object id is not utf-8"))?;
    ObjectId::from_hex(algorithm, value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "tag object id is invalid"))
}

#[derive(Debug, Clone)]
struct FsckTreeEntry {
    mode: TreeMode,
    name: Vec<u8>,
    id: ObjectId,
    bad_filemode: bool,
}

fn fsck_decode_tree_entries(
    algorithm: GitHashAlgorithm,
    bytes: &[u8],
) -> io::Result<(Vec<FsckTreeEntry>, bool)> {
    let mut cursor = 0;
    let mut entries = Vec::with_capacity(bytes.len() / (algorithm.digest_len() + 8));
    let mut has_zero_padded_filemode = false;
    while cursor < bytes.len() {
        let mode_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == b' ')
            .map(|offset| cursor + offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree mode missing space"))?;
        let (parsed_mode, zero_padded_filemode, bad_filemode) =
            fsck_parse_tree_mode(&bytes[cursor..mode_end]);
        has_zero_padded_filemode |= zero_padded_filemode;
        cursor = mode_end + 1;

        let name_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| cursor + offset)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "tree name missing NUL"))?;
        let name = bytes[cursor..name_end].to_vec();
        cursor = name_end + 1;

        let digest_len = algorithm.digest_len();
        if bytes.len().saturating_sub(cursor) < digest_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tree object id is truncated",
            ));
        }
        let id = ObjectId::new(algorithm, &bytes[cursor..cursor + digest_len]);
        cursor += digest_len;
        entries.push(FsckTreeEntry {
            mode: parsed_mode,
            name,
            id,
            bad_filemode,
        });
    }
    Ok((entries, has_zero_padded_filemode))
}

fn fsck_compare_tree_entries(left: &FsckTreeEntry, right: &FsckTreeEntry) -> std::cmp::Ordering {
    fsck_compare_tree_names(
        &left.name,
        left.mode == TreeMode::Tree,
        &right.name,
        right.mode == TreeMode::Tree,
    )
}

fn fsck_compare_tree_names(
    left: &[u8],
    left_has_trailing_slash: bool,
    right: &[u8],
    right_has_trailing_slash: bool,
) -> std::cmp::Ordering {
    let left_len = left.len() + usize::from(left_has_trailing_slash);
    let right_len = right.len() + usize::from(right_has_trailing_slash);
    for idx in 0..left_len.min(right_len) {
        let left_byte = if idx < left.len() { left[idx] } else { b'/' };
        let right_byte = if idx < right.len() { right[idx] } else { b'/' };
        match left_byte.cmp(&right_byte) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    left_len.cmp(&right_len)
}

fn fsck_parse_tree_mode(raw_mode: &[u8]) -> (TreeMode, bool, bool) {
    let zero_padded_filemode = raw_mode.len() > 1 && raw_mode[0] == b'0';
    let normalized = if zero_padded_filemode {
        raw_mode
            .iter()
            .position(|byte| *byte != b'0')
            .map(|offset| &raw_mode[offset..])
            .unwrap_or(&[])
    } else {
        raw_mode
    };
    if let Some(mode) = TreeMode::parse(normalized) {
        return (mode, zero_padded_filemode, false);
    }
    if normalized == b"100664" {
        return (TreeMode::File, zero_padded_filemode, false);
    }
    if normalized.starts_with(b"120") {
        return (TreeMode::Symlink, zero_padded_filemode, true);
    }
    if normalized.starts_with(b"160") {
        return (TreeMode::Gitlink, zero_padded_filemode, true);
    }
    (TreeMode::File, zero_padded_filemode, true)
}

pub(crate) fn pack_objects(options: PackObjectsOptions) -> Result<()> {
    if options.stdout && options.base_name.is_some() {
        return Err(CliError::Fatal {
            code: 129,
            message: "pack-objects --stdout cannot be combined with a base name".into(),
        });
    }
    if !options.stdout && options.base_name.is_none() {
        return Err(CliError::Fatal {
            code: 129,
            message: "pack-objects requires --stdout or a base name".into(),
        });
    }
    validate_pack_objects_compat_options(&options)?;
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let ids = collect_pack_objects_input(&repo, &store, options.revs, options.all)?;
    let packed_first_store = store.packed_first();
    let encode_options = pack_objects_encode_options(&options);
    let index_version = requested_pack_index_version(options.index_version.as_deref())?;
    if options.stdout {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        write_pack_from_store_with_options(
            &packed_first_store,
            GitHashAlgorithm::Sha1,
            &ids,
            encode_options,
            &mut stdout,
        )?;
        return Ok(());
    }
    let Some(base_name) = options.base_name else {
        return Err(CliError::Fatal {
            code: 129,
            message: "pack-objects requires --stdout or a base name".into(),
        });
    };
    if let Some(parent) = base_name.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_pack = unique_temp_sibling(&base_name.with_extension("pack.tmp"));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_pack)?;
        write_pack_from_store_with_options(
            &packed_first_store,
            GitHashAlgorithm::Sha1,
            &ids,
            encode_options,
            &mut file,
        )?;
        file.flush()?;
        Ok::<_, CliError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result?;
    let indexed =
        match index_pack_file_with_version(GitHashAlgorithm::Sha1, &temp_pack, index_version) {
            Ok(indexed) => indexed,
            Err(error) => {
                let _ = fs::remove_file(&temp_pack);
                return Err(CliError::Io(error));
            }
        };
    let pack_name = pack_objects_output_path(&base_name, &indexed.pack_id, "pack");
    let index_name = pack_objects_output_path(&base_name, &indexed.pack_id, "idx");
    fs::rename(&temp_pack, pack_name)?;
    fs::write(index_name, &indexed.index)?;
    println!("{}", indexed.pack_id.to_hex());
    Ok(())
}

fn validate_pack_objects_compat_options(options: &PackObjectsOptions) -> Result<()> {
    let _ = requested_pack_index_version(options.index_version.as_deref())?;
    let _ = (
        options.progress,
        options.no_progress,
        options.no_reuse_delta,
        options.no_reuse_object,
        options.delta_base_offset,
    );
    Ok(())
}

fn requested_pack_index_version(version: Option<&str>) -> Result<PackIndexVersion> {
    match version {
        None => Ok(PackIndexVersion::V2),
        Some(value) => parse_requested_pack_index_version(value),
    }
}

fn parse_requested_pack_index_version(value: &str) -> Result<PackIndexVersion> {
    let mut parts = value.split(',');
    let major = parts.next().expect("major pack index version");
    let minor = parts.next();
    if parts.next().is_some() || minor.is_some_and(|part| part.trim().parse::<u64>().is_err()) {
        return Err(bad_pack_index_version(value));
    }

    let major = if major.is_empty() {
        0
    } else {
        major
            .trim()
            .parse::<i64>()
            .map_err(|_| bad_pack_index_version(value))?
    };
    match major {
        0 | 2 => Ok(PackIndexVersion::V2),
        1 => Ok(PackIndexVersion::V1),
        _ => Err(CliError::Fatal {
            code: 128,
            message: format!("unsupported index version {value}"),
        }),
    }
}

fn bad_pack_index_version(value: &str) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("bad index version '{value}'"),
    }
}

fn pack_objects_encode_options(options: &PackObjectsOptions) -> PackEncodeOptions {
    pack_encode_options(options.window, options.depth)
}

pub(crate) fn pack_encode_options(
    window: Option<usize>,
    depth: Option<usize>,
) -> PackEncodeOptions {
    PackEncodeOptions::delta(window.unwrap_or(10), depth.unwrap_or(50))
}

fn collect_pack_objects_input(
    repo: &GitRepo,
    store: &LooseObjectStore,
    revs: bool,
    all: bool,
) -> Result<Vec<ObjectId>> {
    let rev_args = if revs {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::new(stdin.lock());
        collect_pack_rev_args_from_reader(&mut stdin)?
    } else {
        Vec::new()
    };
    if !revs && !all {
        let stdin = io::stdin();
        let mut stdin = io::BufReader::new(stdin.lock());
        return collect_pack_object_ids_from_reader(&mut stdin);
    }
    let revs = collect_rev_list_revs(repo, store, all, rev_args)?;
    let commits = collect_commits_with_exclusions(repo, store, &revs, None)?;
    let commit_cache = CommitObjectCache::new(store);
    let initial_capacity =
        pack_rev_list_object_initial_capacity(commits.len(), revs.extra_objects.len());
    let mut ids = Vec::with_capacity(initial_capacity);
    let mut seen = HashSet::with_capacity(initial_capacity);
    for commit in &commits {
        if seen.insert(commit.clone()) {
            ids.push(commit.clone());
        }
    }
    let excluded_commits = collect_rev_list_excluded_commits(repo, store, &revs)?;
    for (id, _) in &revs.extra_objects {
        if seen.insert(id.clone()) {
            ids.push(id.clone());
        }
    }
    collect_rev_list_object_ids_into_cached(
        store,
        &commit_cache,
        &commits,
        &[],
        &excluded_commits,
        &mut seen,
        &mut ids,
    )?;
    Ok(ids)
}

fn collect_pack_rev_args_from_reader<R: io::BufRead>(reader: &mut R) -> Result<Vec<String>> {
    let mut rev_args = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if !line.is_empty() {
            rev_args.push(line.to_owned());
        }
    }
    Ok(rev_args)
}

fn collect_pack_object_ids_from_reader<R: io::BufRead>(reader: &mut R) -> Result<Vec<ObjectId>> {
    let mut ids = Vec::with_capacity(PACK_OBJECTS_STDIN_OBJECT_CAPACITY_HINT);
    let mut seen = HashSet::with_capacity(PACK_OBJECTS_STDIN_OBJECT_CAPACITY_HINT);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let id_text = line
            .split_whitespace()
            .next()
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "pack-objects object line is empty".into(),
            })?;
        let id = ObjectId::from_hex(GitHashAlgorithm::Sha1, id_text)?;
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn pack_rev_list_object_initial_capacity(commits_len: usize, extra_objects_len: usize) -> usize {
    commits_len
        .saturating_add(extra_objects_len)
        .min(PACK_REV_LIST_OBJECT_INITIAL_CAPACITY_LIMIT)
        .max(1)
}

fn pack_objects_output_path(base_name: &std::path::Path, pack_id: &ObjectId, ext: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}-{}.{}",
        base_name.display(),
        pack_id.to_hex(),
        ext
    ))
}

pub(crate) fn bundle(
    operation: &str,
    version: Option<String>,
    file: PathBuf,
    args: Vec<String>,
) -> Result<()> {
    match operation {
        "create" => bundle_create(file, version, args),
        _ if version.is_some() => Err(CliError::Fatal {
            code: 129,
            message: "--version is only supported for bundle create".into(),
        }),
        "list-heads" => bundle_list_heads(file, args),
        "verify" => bundle_verify(file),
        "unbundle" => bundle_unbundle(file, args),
        _ => Err(CliError::Fatal {
            code: 129,
            message: format!("unknown bundle subcommand '{operation}'"),
        }),
    }
}

fn bundle_create(file: PathBuf, version: Option<String>, revs: Vec<String>) -> Result<()> {
    let bundle_version = parse_bundle_create_version(version.as_deref())?;
    let (max_count, since, revs) = parse_bundle_create_revs(revs)?;
    if revs.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "bundle create requires at least one revision".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let revs = normalize_bundle_create_revs(&repo, revs)?;
    let heads = revs
        .iter()
        .map(|rev| bundle_head_for_rev(&repo, &store, rev))
        .collect::<Result<Vec<_>>>()?;
    let rev_list_revs = bundle_rev_list_revs(&store, &heads, &revs)?;
    let rev_list = collect_rev_list_revs(&repo, &store, false, rev_list_revs)?;
    let mut commits = collect_commits_with_exclusions(&repo, &store, &rev_list, None)?;
    let commit_cache = CommitObjectCache::new(&store);
    if let Some(since) = since {
        commits.retain(|id| {
            commit_cache
                .read_commit(id)
                .ok()
                .and_then(|commit| {
                    signature_timestamp_timezone(&commit.committer).map(|(timestamp, _)| timestamp)
                })
                .is_some_and(|timestamp| timestamp >= since)
        });
    }
    if let Some(max_count) = max_count {
        commits.truncate(max_count);
    }
    let initial_capacity =
        pack_rev_list_object_initial_capacity(commits.len(), rev_list.extra_objects.len());
    let mut ids = Vec::with_capacity(initial_capacity);
    let mut seen = HashSet::with_capacity(initial_capacity);
    for commit in &commits {
        if seen.insert(commit.clone()) {
            ids.push(commit.clone());
        }
    }
    let excluded_commits = collect_rev_list_excluded_commits(&repo, &store, &rev_list)?;
    let mut object_excluded_commits = excluded_commits.clone();
    if max_count.is_some() || since.is_some() {
        append_bundle_limited_parent_exclusions(&store, &commits, &mut object_excluded_commits)?;
    }
    append_bundle_tag_head_objects(&store, &heads, &mut seen, &mut ids)?;
    for (id, _) in &rev_list.extra_objects {
        if seen.insert(id.clone()) {
            ids.push(id.clone());
        }
    }
    collect_rev_list_object_ids_into_cached(
        &store,
        &commit_cache,
        &commits,
        &[],
        &object_excluded_commits,
        &mut seen,
        &mut ids,
    )?;
    if since.is_some() {
        append_bundle_since_boundary_blob(&store, &commit_cache, &commits, &mut seen, &mut ids)?;
    }
    let packed_first_store = store.packed_first();
    let temp_file = unique_temp_sibling(&file);
    let mut bundle_prerequisites = bundle_prerequisite_commits(&repo, &store, &rev_list)?;
    if max_count.is_some() || since.is_some() {
        bundle_prerequisites.extend(object_excluded_commits.iter().cloned());
        bundle_prerequisites.sort_by_key(ObjectId::to_hex);
        bundle_prerequisites.dedup();
    }
    let result = (|| {
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_file)?;
        if bundle_version == "3" {
            out.write_all(b"# v3 git bundle\n")?;
            out.write_all(b"@object-format=sha1\n")?;
        } else {
            out.write_all(b"# v2 git bundle\n")?;
        }
        for id in &bundle_prerequisites {
            out.write_all(b"-")?;
            out.write_all(id.to_hex().as_bytes())?;
            if let Some(subject) = bundle_commit_subject(&store, id)? {
                out.write_all(b" ")?;
                out.write_all(subject.as_bytes())?;
            }
            out.write_all(b"\n")?;
        }
        for head in heads {
            out.write_all(head.id.to_hex().as_bytes())?;
            out.write_all(b" ")?;
            out.write_all(head.name.as_bytes())?;
            out.write_all(b"\n")?;
        }
        out.write_all(b"\n")?;
        write_pack_from_store_with_options(
            &packed_first_store,
            GitHashAlgorithm::Sha1,
            &ids,
            pack_encode_options(None, None),
            &mut out,
        )?;
        out.flush()?;
        Ok::<_, CliError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_file);
    }
    result?;
    fs::rename(temp_file, file)?;
    Ok(())
}

fn parse_bundle_create_version(version: Option<&str>) -> Result<&'static str> {
    let Some(version) = version else {
        return Ok("2");
    };
    let parsed = parse_bundle_version_value(version).ok_or_else(|| CliError::Stderr {
        code: 129,
        text: if version.is_empty() {
            "error: option `version' expects a numerical value\n".into()
        } else {
            "error: option `version' expects an integer value with an optional k/m/g suffix\n"
                .into()
        },
    })?;
    match parsed {
        -1 | 2 => Ok("2"),
        3 => Ok("3"),
        other => Err(CliError::Fatal {
            code: 128,
            message: format!("unsupported bundle version {other}"),
        }),
    }
}

fn parse_bundle_version_value(value: &str) -> Option<i64> {
    let (number, scale) = match value.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&value[..value.len() - 1], 1024_i64),
        Some(b'm' | b'M') => (&value[..value.len() - 1], 1024_i64 * 1024),
        Some(b'g' | b'G') => (&value[..value.len() - 1], 1024_i64 * 1024 * 1024),
        _ => (value, 1),
    };
    number.parse::<i64>().ok()?.checked_mul(scale)
}

fn parse_bundle_create_revs(
    revs: Vec<String>,
) -> Result<(Option<usize>, Option<i64>, Vec<String>)> {
    let mut max_count = None;
    let mut since = None;
    let mut parsed = Vec::with_capacity(revs.len());
    let mut idx = 0;
    while idx < revs.len() {
        let rev = &revs[idx];
        if rev == "-1" {
            max_count = Some(1);
        } else if rev == "--since" || rev == "--after" {
            idx += 1;
            let Some(value) = revs.get(idx) else {
                return Err(CliError::Fatal {
                    code: 129,
                    message: format!("{rev} requires a value"),
                });
            };
            since = Some(parse_bundle_since(value)?);
        } else if let Some(value) = rev
            .strip_prefix("--since=")
            .or_else(|| rev.strip_prefix("--after="))
        {
            since = Some(parse_bundle_since(value)?);
        } else {
            parsed.push(rev.clone());
        }
        idx += 1;
    }
    Ok((max_count, since, parsed))
}

fn parse_bundle_since(value: &str) -> Result<i64> {
    if let Ok(timestamp) = value.parse::<i64>() {
        return Ok(timestamp);
    }
    if let Ok(datetime) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(datetime.timestamp());
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        if let Some(datetime) = date.and_hms_opt(0, 0, 0) {
            return Ok(datetime.and_utc().timestamp());
        }
    }
    if let Ok(datetime) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Ok(datetime.and_utc().timestamp());
    }
    chrono::DateTime::parse_from_str(value, "%a %b %e %H:%M:%S %Y %z")
        .or_else(|_| chrono::DateTime::parse_from_str(value, "%a %b %d %H:%M:%S %Y %z"))
        .map(|datetime| datetime.timestamp())
        .map_err(|_| CliError::Fatal {
            code: 128,
            message: format!("invalid --since date '{value}'"),
        })
}

fn bundle_rev_list_revs(
    store: &LooseObjectStore,
    heads: &[BundleHead],
    revs: &[String],
) -> Result<Vec<String>> {
    revs.iter()
        .zip(heads)
        .map(|(rev, head)| {
            let object = store.read_object(&head.id)?;
            if object.kind != GitObjectKind::Tag {
                return Ok(rev.clone());
            }
            let tag = decode_tag(GitHashAlgorithm::Sha1, &object.content)?;
            Ok(tag.target.to_hex())
        })
        .collect()
}

fn append_bundle_tag_head_objects(
    store: &LooseObjectStore,
    heads: &[BundleHead],
    seen: &mut HashSet<ObjectId>,
    ids: &mut Vec<ObjectId>,
) -> Result<()> {
    for head in heads {
        let object = store.read_object(&head.id)?;
        if object.kind == GitObjectKind::Tag && seen.insert(head.id.clone()) {
            ids.push(head.id.clone());
        }
    }
    Ok(())
}

fn normalize_bundle_create_revs(repo: &GitRepo, revs: Vec<String>) -> Result<Vec<String>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    revs.into_iter()
        .map(|rev| {
            if rev.contains("..") || rev.starts_with('^') || rev.starts_with('-') {
                return Ok(rev);
            }
            let tag = format!("refs/tags/{rev}");
            if ref_exists(&refs, &tag)? {
                Ok(tag)
            } else {
                Ok(rev)
            }
        })
        .collect()
}

fn append_bundle_limited_parent_exclusions(
    store: &LooseObjectStore,
    commits: &[ObjectId],
    out: &mut Vec<ObjectId>,
) -> Result<()> {
    for id in commits {
        let object = store.read_object(id)?;
        if object.kind != GitObjectKind::Commit {
            continue;
        }
        let decoded = decode_commit(GitHashAlgorithm::Sha1, &object.content)?;
        out.extend(decoded.parents);
    }
    Ok(())
}

fn append_bundle_since_boundary_blob<S>(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, S>,
    commits: &[ObjectId],
    seen: &mut HashSet<ObjectId>,
    ids: &mut Vec<ObjectId>,
) -> Result<()>
where
    S: GitObjectStore + ?Sized,
{
    for id in commits {
        let commit = commit_cache.read_commit(id)?;
        for parent in &commit.parents {
            let parent = commit_cache.read_commit(parent)?;
            let tree = store.read_object(&parent.tree)?;
            if tree.kind != GitObjectKind::Tree {
                continue;
            }
            let (entries, _) = fsck_decode_tree_entries(GitHashAlgorithm::Sha1, &tree.content)?;
            for entry in entries {
                if matches!(
                    entry.mode,
                    TreeMode::File | TreeMode::Executable | TreeMode::Symlink
                ) && !ids.iter().any(|id| id == &entry.id)
                {
                    seen.insert(entry.id.clone());
                    ids.push(entry.id);
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

fn bundle_prerequisite_commits(
    repo: &GitRepo,
    store: &LooseObjectStore,
    rev_list: &RevListRevs,
) -> Result<Vec<ObjectId>> {
    let mut ids = Vec::with_capacity(rev_list.exclude.len());
    for rev in &rev_list.exclude {
        ids.push(resolve_commitish_io(repo, store, rev)?);
    }
    ids.sort_by_key(ObjectId::to_hex);
    ids.dedup();
    Ok(ids)
}

fn bundle_commit_subject(store: &LooseObjectStore, id: &ObjectId) -> Result<Option<String>> {
    let object = store.read_object(id)?;
    if object.kind != GitObjectKind::Commit {
        return Ok(None);
    }
    let commit = decode_commit(GitHashAlgorithm::Sha1, &object.content)?;
    let subject = commit
        .message
        .split(|byte| *byte == b'\n')
        .find(|line| !line.is_empty())
        .map(|line| String::from_utf8_lossy(line).into_owned());
    Ok(subject)
}

fn bundle_list_heads(file: PathBuf, patterns: Vec<String>) -> Result<()> {
    let bundle = parse_bundle_command_header(&file)?;
    for head in bundle.heads {
        if bundle_head_matches(&patterns, &head.name) {
            println!("{} {}", head.id.to_hex(), head.name);
        }
    }
    Ok(())
}

fn bundle_verify(file: PathBuf) -> Result<()> {
    let repo = find_repo().ok();
    let bundle = parse_bundle_command_header(&file)?;
    let store = repo
        .as_ref()
        .map(|repo| LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1));
    verify_bundle_prerequisites(store.as_ref(), &bundle.prerequisites)?;
    let _ = index_bundle_pack(&file, bundle.pack_offset, store.as_ref())?;
    println!("The bundle contains these {} refs:", bundle.heads.len());
    for head in &bundle.heads {
        println!("{} {}", head.id.to_hex(), head.name);
    }
    if bundle.prerequisites.is_empty() {
        println!("The bundle records a complete history.");
    } else {
        println!(
            "The bundle requires these {} refs:",
            bundle.prerequisites.len()
        );
        for id in &bundle.prerequisites {
            println!("{}", id.to_hex());
        }
    }
    println!("The bundle uses this hash algorithm: sha1");
    println!("{} is okay", file.display());
    Ok(())
}

fn bundle_unbundle(file: PathBuf, patterns: Vec<String>) -> Result<()> {
    let repo = find_repo()?;
    let bundle = parse_bundle_command_header(&file)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    verify_bundle_prerequisites(Some(&store), &bundle.prerequisites)?;
    let pack_dir = repo.objects_dir.join("pack");
    fs::create_dir_all(&pack_dir)?;
    let temp_pack = copy_bundle_pack_to_temp_in_dir(&file, bundle.pack_offset, &pack_dir)?;
    let indexed = match index_pack_file_with_store(GitHashAlgorithm::Sha1, &temp_pack, &store) {
        Ok(indexed) => indexed,
        Err(error) => {
            let _ = fs::remove_file(&temp_pack);
            return Err(CliError::Io(error));
        }
    };
    let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
    install_temp_pack_file(
        &pack_dir.join(format!("{pack_name}.pack")),
        &temp_pack,
        &indexed,
    )?;
    write_content_addressed_file(&pack_dir.join(format!("{pack_name}.idx")), &indexed.index)?;
    for head in bundle.heads {
        if bundle_head_matches(&patterns, &head.name) {
            println!("{} {}", head.id.to_hex(), head.name);
        }
    }
    Ok(())
}

pub(crate) fn fetch_bundle_refspecs(
    repo: &GitRepo,
    file: &Path,
    location: &str,
    refspecs: &[String],
    depth_ignored: bool,
    quiet: bool,
) -> Result<()> {
    if depth_ignored {
        eprintln!("warning: option \"depth\" is ignored for {location}");
    }
    let bundle = parse_fetch_bundle_header(repo, file, location)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    verify_bundle_prerequisites(Some(&store), &bundle.prerequisites)?;
    let pack_dir = repo.objects_dir.join("pack");
    fs::create_dir_all(&pack_dir)?;
    let temp_pack = copy_bundle_pack_to_temp_in_dir(file, bundle.pack_offset, &pack_dir)?;
    let indexed = match index_pack_file_with_store(GitHashAlgorithm::Sha1, &temp_pack, &store) {
        Ok(indexed) => indexed,
        Err(error) => {
            let _ = fs::remove_file(&temp_pack);
            return Err(CliError::Io(error));
        }
    };
    let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
    install_temp_pack_file(
        &pack_dir.join(format!("{pack_name}.pack")),
        &temp_pack,
        &indexed,
    )?;
    write_content_addressed_file(&pack_dir.join(format!("{pack_name}.idx")), &indexed.index)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut fetch_head = String::new();
    for refspec in refspecs {
        let (source, destination) = parse_bundle_fetch_refspec(refspec)?;
        let head =
            bundle_resolve_fetch_head(&bundle.heads, source).ok_or_else(|| CliError::Fatal {
                code: 128,
                message: format!("couldn't find remote ref {source}"),
            })?;
        if let Some(destination) = destination {
            refs.write_ref(
                &destination_fetch_ref_name_for_bundle(destination)?,
                &head.id,
            )?;
        }
        fetch_head.push_str(&bundle_fetch_head_row(head, location));
    }
    fs::write(repo.git_dir.join("FETCH_HEAD"), fetch_head)?;
    if !quiet {
        eprintln!("From {}", location);
    }
    Ok(())
}

fn parse_bundle_fetch_refspec(refspec: &str) -> Result<(&str, Option<&str>)> {
    let refspec = refspec.strip_prefix('+').unwrap_or(refspec);
    if let Some((source, destination)) = refspec.split_once(':') {
        if source.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: format!("invalid refspec '{refspec}'"),
            });
        }
        return Ok((source, (!destination.is_empty()).then_some(destination)));
    }
    Ok((refspec, None))
}

fn bundle_fetch_head_row(head: &BundleHead, location: &str) -> String {
    format!(
        "{}\t\t{} of {}\n",
        head.id.to_hex(),
        bundle_fetch_head_description(&head.name),
        location
    )
}

fn bundle_fetch_head_description(name: &str) -> String {
    if let Some(branch) = name.strip_prefix("refs/heads/") {
        format!("branch '{branch}'")
    } else if let Some(tag) = name.strip_prefix("refs/tags/") {
        format!("tag '{tag}'")
    } else {
        format!("'{name}'")
    }
}

fn bundle_resolve_fetch_head<'a>(heads: &'a [BundleHead], source: &str) -> Option<&'a BundleHead> {
    let branch = format!("refs/heads/{source}");
    heads
        .iter()
        .find(|head| head.name == source || head.name == branch)
}

fn destination_fetch_ref_name_for_bundle(destination: &str) -> Result<String> {
    if destination.starts_with("refs/") {
        Ok(destination.to_owned())
    } else {
        branch_ref_name(destination)
    }
}

fn bundle_head_matches(patterns: &[String], name: &str) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| pattern == name)
}

fn verify_bundle_prerequisites(
    store: Option<&LooseObjectStore>,
    prerequisites: &[ObjectId],
) -> Result<()> {
    if prerequisites.is_empty() {
        return Ok(());
    }
    let Some(store) = store else {
        return Err(CliError::Fatal {
            code: 128,
            message: "Need a repository to verify a bundle with prerequisites".into(),
        });
    };
    for id in prerequisites {
        store.read_object(id).map_err(|_| CliError::Fatal {
            code: 1,
            message: format!("Repository lacks prerequisite commit {}", id.to_hex()),
        })?;
    }
    Ok(())
}

fn bundle_head_for_rev(repo: &GitRepo, store: &LooseObjectStore, rev: &str) -> Result<BundleHead> {
    let head_rev = rev.rsplit_once("..").map(|(_, head)| head).unwrap_or(rev);
    let id = resolve_objectish(repo, head_rev)?;
    let name = if head_rev == "HEAD" {
        "HEAD".to_owned()
    } else if head_rev.starts_with("refs/") {
        head_rev.to_owned()
    } else {
        let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
        let branch = format!("refs/heads/{head_rev}");
        if ref_exists(&refs, &branch)? {
            branch
        } else {
            head_rev.to_owned()
        }
    };
    let object = store.read_object(&id)?;
    if object.kind != GitObjectKind::Commit && object.kind != GitObjectKind::Tag {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("Refusing to create bundle ref '{name}' for non-commit object"),
        });
    }
    Ok(BundleHead { id, name })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedBundleHeader {
    heads: Vec<BundleHead>,
    prerequisites: Vec<ObjectId>,
    pack_offset: u64,
}

fn parse_bundle_header(path: &std::path::Path) -> Result<ParsedBundleHeader> {
    let mut reader = io::BufReader::new(fs::File::open(path)?);
    let mut pack_offset = 0_u64;
    let mut line = Vec::new();
    let read = reader.read_until(b'\n', &mut line)?;
    if read == 0 {
        return Err(CliError::Fatal {
            code: 128,
            message: "bundle header is missing terminator".into(),
        });
    };
    pack_offset = pack_offset
        .checked_add(read as u64)
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "bundle header is too large".into(),
        })?;
    let header = bundle_header_line(&line)?;
    if !matches!(header, "# v2 git bundle" | "# v3 git bundle") {
        return Err(CliError::Fatal {
            code: 128,
            message: "unsupported bundle format".into(),
        });
    }
    let mut heads = Vec::new();
    let mut prerequisites = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            return Err(CliError::Fatal {
                code: 128,
                message: "bundle header is missing terminator".into(),
            });
        }
        pack_offset = pack_offset
            .checked_add(read as u64)
            .ok_or_else(|| CliError::Fatal {
                code: 128,
                message: "bundle header is too large".into(),
            })?;
        if line == b"\n" {
            break;
        }
        let line = bundle_header_line(&line)?;
        if line.starts_with('@') {
            continue;
        }
        if let Some(prerequisite) = line.strip_prefix('-') {
            let id = prerequisite
                .split_whitespace()
                .next()
                .ok_or_else(|| CliError::Fatal {
                    code: 128,
                    message: "bundle prerequisite line is malformed".into(),
                })?;
            prerequisites.push(ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?);
            continue;
        }
        let (id, name) = line.split_once(' ').ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "bundle ref line is malformed".into(),
        })?;
        heads.push(BundleHead {
            id: ObjectId::from_hex(GitHashAlgorithm::Sha1, id)?,
            name: name.to_owned(),
        });
    }
    Ok(ParsedBundleHeader {
        heads,
        prerequisites,
        pack_offset,
    })
}

fn parse_bundle_command_header(path: &std::path::Path) -> Result<ParsedBundleHeader> {
    parse_bundle_header(path).map_err(|error| match error {
        CliError::Fatal { code: 128, message } if message == "unsupported bundle format" => {
            CliError::Stderr {
                code: 1,
                text: format!(
                    "error: '{}' does not look like a v2 or v3 bundle file\n",
                    path.display()
                ),
            }
        }
        other => other,
    })
}

fn parse_fetch_bundle_header(
    repo: &GitRepo,
    path: &std::path::Path,
    location: &str,
) -> Result<ParsedBundleHeader> {
    parse_bundle_header(path).map_err(|error| match error {
        CliError::Fatal { code: 128, message } if message == "unsupported bundle format" => {
            let _ = fs::write(repo.git_dir.join("FETCH_HEAD"), b"");
            CliError::Stderr {
                code: 128,
                text: format!(
                    "fatal: invalid gitfile format: {location}\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n"
                ),
            }
        }
        other => other,
    })
}

fn bundle_header_line(line: &[u8]) -> Result<&str> {
    let Some(line) = line.strip_suffix(b"\n") else {
        return Err(CliError::Fatal {
            code: 128,
            message: "bundle header is missing terminator".into(),
        });
    };
    std::str::from_utf8(line).map_err(|_| CliError::Fatal {
        code: 128,
        message: "bundle header is not valid UTF-8".into(),
    })
}

fn index_bundle_pack(
    path: &std::path::Path,
    pack_offset: u64,
    store: Option<&LooseObjectStore>,
) -> Result<zmin_git_core::IndexedPack> {
    let bytes = map_file_bytes(path)?;
    let pack = bundle_pack_slice(bytes.as_slice(), pack_offset)?;
    if let Some(store) = store {
        Ok(index_pack_bytes_with_store(
            GitHashAlgorithm::Sha1,
            pack,
            store,
        )?)
    } else {
        Ok(index_pack_bytes(GitHashAlgorithm::Sha1, pack)?)
    }
}

fn bundle_pack_slice(bytes: &[u8], pack_offset: u64) -> Result<&[u8]> {
    let pack_offset = usize::try_from(pack_offset).map_err(|_| CliError::Fatal {
        code: 128,
        message: "bundle header is too large".into(),
    })?;
    bytes.get(pack_offset..).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "bundle pack payload is missing".into(),
    })
}

fn copy_bundle_pack_to_temp_in_dir(
    path: &std::path::Path,
    pack_offset: u64,
    dir: &std::path::Path,
) -> Result<PathBuf> {
    let temp_pack = unique_temp_sibling(&dir.join("bundle-pack.pack"));
    let result = (|| {
        let mut source = fs::File::open(path)?;
        source.seek(SeekFrom::Start(pack_offset))?;
        let mut target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_pack)?;
        io::copy(&mut source, &mut target)?;
        target.flush()?;
        Ok::<_, CliError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_pack);
    }
    result?;
    Ok(temp_pack)
}

pub(crate) fn index_pack(options: IndexPackOptions) -> Result<()> {
    if options.rev_index && options.no_rev_index {
        return Err(CliError::Fatal {
            code: 129,
            message: "index-pack --rev-index cannot be combined with --no-rev-index".into(),
        });
    }
    let index_version = requested_pack_index_version(options.index_version.as_deref())?;
    if options.verify {
        if options.stdin || options.pack_file.is_none() {
            return Err(CliError::Fatal {
                code: 128,
                message: "--verify with no packfile name given".into(),
            });
        }
        let Some(path) = options.pack_file.as_ref() else {
            return Err(CliError::Fatal {
                code: 128,
                message: "--verify with no packfile name given".into(),
            });
        };
        if path.extension().and_then(|value| value.to_str()) != Some("pack") {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "packfile name '{}' does not end with '.pack'",
                    path.display()
                ),
            });
        }
        let indexed =
            index_pack_file_index_only_with_version(GitHashAlgorithm::Sha1, path, index_version)
                .map_err(|error| index_pack_verify_integrity_error(path, error))?;
        let idx_path = path.with_extension("idx");
        if idx_path.exists() {
            if !file_bytes_equal(&idx_path, &indexed.index)? {
                let idx = fs::read(&idx_path)?;
                let entries = decode_pack_index(GitHashAlgorithm::Sha1, idx)
                    .map_err(|_| index_pack_verify_validation_error(&idx_path))?;
                return Err(index_pack_verify_pack_index_mismatch_error(path, &entries));
            }
            let rev_path = path.with_extension("rev");
            if rev_path.exists() {
                validate_pack_reverse_index_file(
                    GitHashAlgorithm::Sha1,
                    &rev_path,
                    indexed.objects,
                )
                .map_err(|_| index_pack_verify_validation_error(&rev_path))?;
            }
        }
        return Ok(());
    }
    if options.stdin
        && !options.fix_thin
        && options.strict.is_none()
        && options.fsck_objects.is_none()
    {
        let repo = find_repo()?;
        let pack_dir = repo.objects_dir.join("pack");
        fs::create_dir_all(&pack_dir)?;
        let temp_pack = unique_temp_sibling(&pack_dir.join("index-pack-stdin.pack"));
        let result = copy_index_pack_stdin_to_temp_pack(&temp_pack);
        if result.is_err() {
            let _ = fs::remove_file(&temp_pack);
        }
        result?;
        let indexed =
            match index_pack_file_for_output(&temp_pack, index_version, options.no_rev_index) {
                Ok(indexed) => indexed,
                Err(error) => {
                    let _ = fs::remove_file(&temp_pack);
                    return Err(error);
                }
            };
        let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
        let pack_path = options
            .pack_file
            .unwrap_or_else(|| pack_dir.join(format!("{pack_name}.pack")));
        if let Some(parent) = pack_path.parent() {
            fs::create_dir_all(parent)?;
        }
        install_temp_pack_file_with_id(&pack_path, &temp_pack, &indexed.pack_id)?;
        let idx_path = options
            .output
            .unwrap_or_else(|| pack_path.with_extension("idx"));
        if let Some(parent) = idx_path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_content_addressed_file(&idx_path, &indexed.index)?;
        if let Some(reverse_index) = indexed.reverse_index.as_ref() {
            write_content_addressed_file(&pack_path.with_extension("rev"), reverse_index)?;
        }
        let kept = if let Some(message) = options.keep {
            fs::write(pack_path.with_extension("keep"), message)?;
            true
        } else {
            false
        };
        let _ = (options.rev_index, options.verbose);
        print_index_pack_installed_output(&indexed.pack_id, kept);
        return Ok(());
    }
    if options.stdin && !options.fix_thin {
        let repo = find_repo()?;
        let pack_dir = repo.objects_dir.join("pack");
        fs::create_dir_all(&pack_dir)?;
        let temp_pack = unique_temp_sibling(&pack_dir.join("index-pack-stdin-validated.pack"));
        let result = copy_index_pack_stdin_to_temp_pack(&temp_pack);
        if result.is_err() {
            let _ = fs::remove_file(&temp_pack);
        }
        result?;
        let indexed =
            match index_pack_file_for_output(&temp_pack, index_version, options.no_rev_index) {
                Ok(indexed) => indexed,
                Err(error) => {
                    let _ = fs::remove_file(&temp_pack);
                    return Err(error);
                }
            };
        if let Err(error) = index_pack_validate_pack_file(
            &repo,
            &temp_pack,
            options.strict.as_deref(),
            options.fsck_objects.as_deref(),
        ) {
            let _ = fs::remove_file(&temp_pack);
            return Err(error);
        }
        let pack_name = format!("pack-{}", indexed.pack_id.to_hex());
        let pack_path = options
            .pack_file
            .unwrap_or_else(|| pack_dir.join(format!("{pack_name}.pack")));
        if let Some(parent) = pack_path.parent() {
            fs::create_dir_all(parent)?;
        }
        install_temp_pack_file_with_id(&pack_path, &temp_pack, &indexed.pack_id)?;
        let idx_path = options
            .output
            .unwrap_or_else(|| pack_path.with_extension("idx"));
        if let Some(parent) = idx_path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_content_addressed_file(&idx_path, &indexed.index)?;
        if let Some(reverse_index) = indexed.reverse_index.as_ref() {
            write_content_addressed_file(&pack_path.with_extension("rev"), reverse_index)?;
        }
        let kept = if let Some(message) = options.keep {
            fs::write(pack_path.with_extension("keep"), message)?;
            true
        } else {
            false
        };
        let _ = (options.rev_index, options.verbose);
        print_index_pack_installed_output(&indexed.pack_id, kept);
        return Ok(());
    }
    if options.stdin && options.fix_thin {
        let repo = find_repo()?;
        let pack_dir = repo.objects_dir.join("pack");
        fs::create_dir_all(&pack_dir)?;
        let input_pack = unique_temp_sibling(&pack_dir.join("index-pack-thin-input.pack"));
        let repaired_pack = unique_temp_sibling(&pack_dir.join("index-pack-thin-repaired.pack"));
        let result = (|| {
            copy_index_pack_stdin_to_temp_pack(&input_pack)?;
            let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
            repair_thin_pack_file_to_path(
                GitHashAlgorithm::Sha1,
                &input_pack,
                &store,
                &repaired_pack,
                index_version,
            )
            .map_err(CliError::Io)
        })();
        let _ = fs::remove_file(&input_pack);
        let repair = match result {
            Ok(repair) => repair,
            Err(error) => {
                let _ = fs::remove_file(&repaired_pack);
                return Err(error);
            }
        };
        if (options.strict.is_some() || options.fsck_objects.is_some())
            && let Err(error) = index_pack_validate_pack_file(
                &repo,
                &repaired_pack,
                options.strict.as_deref(),
                options.fsck_objects.as_deref(),
            )
        {
            let _ = fs::remove_file(&repaired_pack);
            return Err(error);
        }
        let pack_name = format!("pack-{}", repair.indexed.pack_id.to_hex());
        let pack_path = options
            .pack_file
            .unwrap_or_else(|| pack_dir.join(format!("{pack_name}.pack")));
        if let Some(parent) = pack_path.parent() {
            fs::create_dir_all(parent)?;
        }
        install_temp_pack_file(&pack_path, &repaired_pack, &repair.indexed)?;
        let idx_path = options
            .output
            .unwrap_or_else(|| pack_path.with_extension("idx"));
        if let Some(parent) = idx_path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_content_addressed_file(&idx_path, &repair.indexed.index)?;
        if !options.no_rev_index {
            write_content_addressed_file(
                &pack_path.with_extension("rev"),
                &repair.indexed.reverse_index,
            )?;
        }
        let kept = if let Some(message) = options.keep {
            fs::write(pack_path.with_extension("keep"), message)?;
            true
        } else {
            false
        };
        let _ = (options.rev_index, options.verbose, repair.fixed_objects);
        print_index_pack_installed_output(&repair.indexed.pack_id, kept);
        return Ok(());
    }
    if options.fix_thin {
        return Err(CliError::Fatal {
            code: 129,
            message: "index-pack --fix-thin requires --stdin".into(),
        });
    }
    if !options.stdin
        && !options.fix_thin
        && (options.strict.is_some() || options.fsck_objects.is_some())
    {
        let repo = find_repo()?;
        let Some(pack_path) = options.pack_file.clone() else {
            return Err(CliError::Fatal {
                code: 129,
                message: "index-pack requires a pack file or --stdin".into(),
            });
        };
        let indexed = index_pack_file_for_output(&pack_path, index_version, options.no_rev_index)?;
        index_pack_validate_pack_file(
            &repo,
            &pack_path,
            options.strict.as_deref(),
            options.fsck_objects.as_deref(),
        )?;
        let idx_path = options
            .output
            .unwrap_or_else(|| pack_path.with_extension("idx"));
        if let Some(parent) = idx_path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_content_addressed_file(&idx_path, &indexed.index)?;
        if let Some(reverse_index) = indexed.reverse_index.as_ref() {
            write_content_addressed_file(&pack_path.with_extension("rev"), reverse_index)?;
        }
        let kept = if let Some(message) = options.keep {
            fs::write(pack_path.with_extension("keep"), message)?;
            true
        } else {
            false
        };
        let _ = (options.rev_index, options.verbose);
        print_index_pack_output(&indexed.pack_id, kept);
        return Ok(());
    }
    let Some(pack_path) = options.pack_file.clone() else {
        return Err(CliError::Fatal {
            code: 129,
            message: "index-pack requires a pack file or --stdin".into(),
        });
    };
    let idx_path = options
        .output
        .unwrap_or_else(|| pack_path.with_extension("idx"));
    if let Some(parent) = idx_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let indexed = index_pack_file_for_output(&pack_path, index_version, options.no_rev_index)?;
    write_content_addressed_file(&idx_path, &indexed.index)?;
    if let Some(reverse_index) = indexed.reverse_index.as_ref() {
        write_content_addressed_file(&pack_path.with_extension("rev"), reverse_index)?;
    }
    let kept = if let Some(message) = options.keep {
        fs::write(pack_path.with_extension("keep"), message)?;
        true
    } else {
        false
    };
    let _ = (options.rev_index, options.verbose);
    print_index_pack_output(&indexed.pack_id, kept);
    Ok(())
}

fn print_index_pack_output(pack_id: &ObjectId, kept: bool) {
    if kept {
        println!("keep\t{}", pack_id.to_hex());
    } else {
        println!("{}", pack_id.to_hex());
    }
}

fn print_index_pack_installed_output(pack_id: &ObjectId, kept: bool) {
    if kept {
        println!("keep\t{}", pack_id.to_hex());
    } else {
        println!("pack\t{}", pack_id.to_hex());
    }
}

fn copy_index_pack_stdin_to_temp_pack(path: &Path) -> Result<()> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut file = io::BufWriter::with_capacity(INDEX_PACK_STDIN_BUF_CAPACITY, file);
    let mut stdin = io::BufReader::with_capacity(INDEX_PACK_STDIN_BUF_CAPACITY, io::stdin());
    io::copy(&mut stdin, &mut file)?;
    file.flush()?;
    Ok(())
}

fn index_pack_validate_pack_file(
    repo: &GitRepo,
    pack_path: &std::path::Path,
    strict: Option<&str>,
    fsck_objects: Option<&str>,
) -> Result<()> {
    let mut config = fsck_message_config(repo)?;
    if let Some(overrides) = strict.filter(|value| !value.is_empty()) {
        apply_fsck_message_overrides(&mut config, overrides)?;
    }
    if let Some(overrides) = fsck_objects.filter(|value| !value.is_empty()) {
        apply_fsck_message_overrides(&mut config, overrides)?;
    }

    let mut reporter = IndexPackFsckReporter::default();
    for_each_pack_object_file(GitHashAlgorithm::Sha1, pack_path, |id, kind, content| {
        validate_index_pack_object(&mut reporter, &config, id, kind, content);
        Ok(())
    })?;
    reporter.into_result()
}

fn validate_index_pack_object(
    reporter: &mut IndexPackFsckReporter,
    config: &FsckMessageConfig,
    id: &ObjectId,
    kind: GitObjectKind,
    content: &[u8],
) {
    match kind {
        GitObjectKind::Commit => {
            let author_error = reporter.report_missing_header(
                id,
                content,
                config.missing_author,
                b"author ",
                "missingAuthor",
                "expected 'author' line",
            );
            if !author_error {
                reporter.report_missing_header(
                    id,
                    content,
                    config.missing_committer,
                    b"committer ",
                    "missingCommitter",
                    "expected 'committer' line",
                );
            }
            if let Ok(commit) = decode_commit(GitHashAlgorithm::Sha1, content) {
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.bad_email,
                    commit_signature_has_bad_email,
                    "badEmail",
                    "bad email",
                );
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.missing_email,
                    commit_signature_has_missing_email,
                    "missingEmail",
                    "missing email",
                );
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.missing_name_before_email,
                    commit_signature_has_missing_name_before_email,
                    "missingNameBeforeEmail",
                    "missing space before email",
                );
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.missing_space_before_email,
                    commit_signature_has_missing_space_before_email,
                    "missingSpaceBeforeEmail",
                    "missing space before email",
                );
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.missing_space_before_date,
                    commit_signature_has_missing_space_before_date,
                    "missingSpaceBeforeDate",
                    "missing space before date",
                );
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.zero_padded_date,
                    commit_signature_has_zero_padded_date,
                    "zeroPaddedDate",
                    "zero-padded date",
                );
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.bad_timezone,
                    commit_signature_has_bad_timezone,
                    "badTimezone",
                    "bad time zone",
                );
                reporter.report_commit_signature_issue(
                    id,
                    &commit,
                    config.bad_date,
                    commit_signature_has_bad_date,
                    "badDate",
                    "bad date",
                );
            }
        }
        GitObjectKind::Tree => {
            let tree_analysis = fsck_analyze_tree(content);
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_zero_padded_filemode),
                config.zero_padded_filemode,
                "zeroPaddedFilemode",
                "contains zero-padded file modes",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_bad_filemode),
                config.bad_filemode,
                "badFilemode",
                "contains bad file modes",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_duplicate_entries),
                config.duplicate_entries,
                "duplicateEntries",
                "contains duplicate file entries",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_null_sha1),
                config.null_sha1,
                "nullSha1",
                "contains entries pointing to null sha1",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_full_pathname),
                config.full_pathname,
                "fullPathname",
                "contains full pathnames",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_dot),
                config.has_dot,
                "hasDot",
                "contains '.'",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_dotdot),
                config.has_dotdot,
                "hasDotdot",
                "contains '..'",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.has_dotgit),
                config.has_dotgit,
                "hasDotgit",
                "contains '.git'",
            );
            reporter.report_tree_issue(
                id,
                tree_analysis
                    .as_ref()
                    .is_some_and(|analysis| analysis.not_sorted),
                config.tree_not_sorted,
                "treeNotSorted",
                "not properly sorted",
            );
        }
        GitObjectKind::Tag => {
            reporter.report_missing_tag_header(
                id,
                content,
                config.missing_tagger_entry,
                b"tagger ",
                "missingTaggerEntry",
                "expected 'tagger' line",
            );
            if let Ok(name) = fsck_tag_name(content)
                && !check_ref_format(name, true)
            {
                reporter.report_issue(
                    id,
                    config.bad_tag_name,
                    "badTagName",
                    &format!("invalid 'tag' name: {name}"),
                );
            }
            if let Ok(tagger) = fsck_tag_tagger(content) {
                reporter.report_tag_signature_issue(
                    id,
                    tagger,
                    config.bad_email,
                    commit_signature_has_bad_email,
                    "badEmail",
                    "bad email",
                );
                reporter.report_tag_signature_issue(
                    id,
                    tagger,
                    config.missing_email,
                    commit_signature_has_missing_email,
                    "missingEmail",
                    "missing email",
                );
                reporter.report_tag_signature_issue(
                    id,
                    tagger,
                    config.missing_name_before_email,
                    commit_signature_has_missing_name_before_email,
                    "missingNameBeforeEmail",
                    "missing space before email",
                );
                reporter.report_tag_signature_issue(
                    id,
                    tagger,
                    config.missing_space_before_email,
                    commit_signature_has_missing_space_before_email,
                    "missingSpaceBeforeEmail",
                    "missing space before email",
                );
                reporter.report_tag_signature_issue(
                    id,
                    tagger,
                    config.missing_space_before_date,
                    commit_signature_has_missing_space_before_date,
                    "missingSpaceBeforeDate",
                    "missing space before date",
                );
                reporter.report_tag_signature_issue(
                    id,
                    tagger,
                    config.zero_padded_date,
                    commit_signature_has_zero_padded_date,
                    "zeroPaddedDate",
                    "zero-padded date",
                );
                reporter.report_tag_signature_issue(
                    id,
                    tagger,
                    config.bad_date,
                    commit_signature_has_bad_date,
                    "badDate",
                    "bad date",
                );
            }
        }
        GitObjectKind::Blob => {}
    }
}

#[derive(Default)]
struct IndexPackFsckReporter {
    text: String,
    has_error: bool,
}

impl IndexPackFsckReporter {
    fn into_result(mut self) -> Result<()> {
        if self.has_error {
            self.text.push_str("fatal: fsck error in packed object\n");
            Err(CliError::Stderr {
                code: 128,
                text: self.text,
            })
        } else {
            if !self.text.is_empty() {
                eprint!("{}", self.text);
            }
            Ok(())
        }
    }

    fn report_missing_header(
        &mut self,
        id: &ObjectId,
        content: &[u8],
        severity: FsckMessageSeverity,
        header_prefix: &[u8],
        message_id: &str,
        message: &str,
    ) -> bool {
        if fsck_commit_has_header(content, header_prefix) {
            return false;
        }
        self.report_invalid_format(id, severity, message_id, message);
        severity == FsckMessageSeverity::Error
    }

    fn report_missing_tag_header(
        &mut self,
        id: &ObjectId,
        content: &[u8],
        severity: FsckMessageSeverity,
        header_prefix: &[u8],
        message_id: &str,
        message: &str,
    ) {
        if fsck_tag_has_header(content, header_prefix) {
            return;
        }
        self.report_invalid_format(id, severity, message_id, message);
    }

    fn report_commit_signature_issue(
        &mut self,
        id: &ObjectId,
        commit: &CommitObject,
        severity: FsckMessageSeverity,
        predicate: fn(&[u8]) -> bool,
        message_id: &str,
        message: &str,
    ) {
        if !predicate(&commit.author) && !predicate(&commit.committer) {
            return;
        }
        self.report_invalid_author_committer(id, severity, message_id, message);
    }

    fn report_tag_signature_issue(
        &mut self,
        id: &ObjectId,
        tagger: &[u8],
        severity: FsckMessageSeverity,
        predicate: fn(&[u8]) -> bool,
        message_id: &str,
        message: &str,
    ) {
        if !predicate(tagger) {
            return;
        }
        self.report_invalid_author_committer(id, severity, message_id, message);
    }

    fn report_tree_issue(
        &mut self,
        id: &ObjectId,
        found: bool,
        severity: FsckMessageSeverity,
        message_id: &str,
        message: &str,
    ) {
        if found {
            self.report_issue(id, severity, message_id, message);
        }
    }

    fn report_invalid_format(
        &mut self,
        id: &ObjectId,
        severity: FsckMessageSeverity,
        message_id: &str,
        message: &str,
    ) {
        self.report_issue(
            id,
            severity,
            message_id,
            &format!("invalid format - {message}"),
        );
    }

    fn report_invalid_author_committer(
        &mut self,
        id: &ObjectId,
        severity: FsckMessageSeverity,
        message_id: &str,
        message: &str,
    ) {
        self.report_issue(
            id,
            severity,
            message_id,
            &format!("invalid author/committer line - {message}"),
        );
    }

    fn report_issue(
        &mut self,
        id: &ObjectId,
        severity: FsckMessageSeverity,
        message_id: &str,
        message: &str,
    ) {
        match severity {
            FsckMessageSeverity::Error => {
                self.has_error = true;
                self.text.push_str(&format!(
                    "error: object {}: {message_id}: {message}\n",
                    id.to_hex()
                ));
            }
            FsckMessageSeverity::Warn => {
                self.text.push_str(&format!(
                    "warning: object {}: {message_id}: {message}\n",
                    id.to_hex()
                ));
            }
            FsckMessageSeverity::Ignore => {}
        }
    }
}

fn index_pack_verify_integrity_error(pack_path: &std::path::Path, error: io::Error) -> CliError {
    if error.kind() == io::ErrorKind::InvalidData
        && let Some(version) = unsupported_pack_file_version(&error)
    {
        return CliError::Fatal {
            code: 128,
            message: format!("pack version {version} unsupported"),
        };
    }
    if error.kind() == io::ErrorKind::InvalidData && error.to_string() == "pack checksum mismatch" {
        let idx_path = pack_path.with_extension("idx");
        let entry_count = pack_index_object_count(&idx_path).unwrap_or(1);
        let mut text = String::new();
        let pack_display = index_pack_verify_display_path(pack_path);
        for _ in 0..entry_count {
            text.push_str(&format!(
                "error: packfile {pack_display} does not match index\n"
            ));
        }
        text.push_str("fatal: pack is corrupted (SHA1 mismatch)\n");
        return CliError::Stderr { code: 128, text };
    }
    if error.kind() == io::ErrorKind::InvalidData {
        CliError::Fatal {
            code: 128,
            message: error.to_string(),
        }
    } else {
        CliError::Io(error)
    }
}

fn index_pack_verify_validation_error(path: &std::path::Path) -> CliError {
    CliError::Fatal {
        code: 128,
        message: format!("sha1 file '{}' validation error", path.display()),
    }
}

fn file_bytes_equal(path: &std::path::Path, expected: &[u8]) -> io::Result<bool> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut reader = io::BufReader::new(file);
    let mut offset = 0;
    let mut buffer = [0_u8; 64 * 1024];
    while offset < expected.len() {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(false);
        }
        let end = offset + read;
        if buffer[..read] != expected[offset..end] {
            return Ok(false);
        }
        offset = end;
    }
    Ok(true)
}

fn index_pack_verify_pack_index_mismatch_error(
    pack_path: &std::path::Path,
    entries: &[PackIndexEntry],
) -> CliError {
    let mut text = String::new();
    let pack_display = index_pack_verify_display_path(pack_path);
    for _ in 0..entries.len().max(1) {
        text.push_str(&format!(
            "error: packfile {pack_display} does not match index\n"
        ));
    }
    text.push_str("fatal: pack is corrupted (SHA1 mismatch)\n");
    CliError::Stderr { code: 128, text }
}

fn index_pack_verify_display_path(pack_path: &std::path::Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(canonical_cwd) = cwd.canonicalize()
        && let Ok(canonical_pack) = pack_path.canonicalize()
        && let Ok(relative) = canonical_pack.strip_prefix(&canonical_cwd)
    {
        return git_path_output_string(relative.display().to_string());
    }
    git_path_output_string(pack_path.display().to_string())
}

#[cfg(windows)]
fn git_path_output_string(value: String) -> String {
    value.replace('\\', "/")
}

#[cfg(not(windows))]
fn git_path_output_string(value: String) -> String {
    value
}

pub(crate) fn verify_pack(
    verbose: bool,
    stat_only: bool,
    object_format: Option<&str>,
    packs: Vec<PathBuf>,
) -> Result<()> {
    if object_format.is_some_and(|format| format != "sha1") {
        return Err(CliError::Fatal {
            code: 129,
            message: "only sha1 pack verification is supported".into(),
        });
    }
    if packs.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "verify-pack requires at least one .idx path".into(),
        });
    }
    for idx_path in packs {
        verify_pack_one(&idx_path, verbose, stat_only)?;
    }
    Ok(())
}

fn verify_pack_one(idx_path: &std::path::Path, verbose: bool, stat_only: bool) -> Result<()> {
    validate_pack_index_file(GitHashAlgorithm::Sha1, idx_path)
        .map_err(|error| verify_pack_index_error(idx_path, error))?;
    let pack_path = idx_path.with_extension("pack");
    let verified = zmin_git_core::verify_pack_file_matches_index(
        GitHashAlgorithm::Sha1,
        &pack_path,
        idx_path,
        verbose,
    )
    .map_err(verify_pack_integrity_error)?;
    if !verbose && !stat_only {
        return Ok(());
    }
    if stat_only {
        println!("non delta: {} objects", verified.objects);
        return Ok(());
    }
    let mut out = io::BufWriter::new(io::stdout().lock());
    let mut non_delta = 0usize;
    for entry in &verified.entries {
        non_delta += 1;
        entry.object_id.write_hex_io(&mut out)?;
        writeln!(
            out,
            " {:<6} {} {} {}",
            entry.kind.as_str(),
            entry.object_size,
            entry.packed_size,
            entry.offset
        )?;
    }
    writeln!(out, "non delta: {non_delta} objects")?;
    writeln!(out, "{}: ok", pack_path.display())?;
    out.flush()?;
    Ok(())
}

fn verify_pack_integrity_error(error: io::Error) -> CliError {
    if error.kind() == io::ErrorKind::InvalidData {
        if let Some(version) = unsupported_pack_file_version(&error) {
            return CliError::Fatal {
                code: 1,
                message: format!("pack version {version} unsupported"),
            };
        }
        CliError::Fatal {
            code: 1,
            message: error.to_string(),
        }
    } else {
        CliError::Io(error)
    }
}

fn verify_pack_index_error(idx_path: &std::path::Path, error: io::Error) -> CliError {
    if error.kind() == io::ErrorKind::InvalidData {
        if let Some(version) = unsupported_pack_index_version(&error) {
            return CliError::Stderr {
                code: 1,
                text: format!(
                    "error: index file {} is version {version} and is not supported by this binary (try upgrading GIT to a newer version)\nfatal: Cannot open existing pack idx file for '{}'\n",
                    idx_path.display(),
                    idx_path.display()
                ),
            };
        }
        CliError::Fatal {
            code: 1,
            message: format!("sha1 file '{}' validation error", idx_path.display()),
        }
    } else {
        CliError::Io(error)
    }
}

fn unsupported_pack_index_version(error: &io::Error) -> Option<String> {
    error
        .to_string()
        .strip_prefix("unsupported pack index version ")
        .map(str::to_owned)
}

fn unsupported_pack_file_version(error: &io::Error) -> Option<String> {
    error
        .to_string()
        .strip_prefix("unsupported pack file version ")
        .map(str::to_owned)
}

#[derive(Debug, Clone)]
struct PackRedundantEntry {
    pack_path: PathBuf,
    idx_path: PathBuf,
}

pub(crate) fn pack_redundant(
    _verbose: bool,
    alt_odb: bool,
    all: bool,
    packs: Vec<PathBuf>,
) -> Result<()> {
    if all && !packs.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "pack-redundant cannot combine --all with explicit packs".into(),
        });
    }
    if !all && packs.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message:
                "usage: git pack-redundant [--verbose] [--alt-odb] (--all | <pack-filename>...)"
                    .into(),
        });
    }
    let repo = find_repo_or_bare()?;
    let pack_paths = if all {
        collect_repo_pack_paths(&repo.objects_dir)?
    } else {
        packs
    };
    let mut entries = Vec::with_capacity(pack_paths.len());
    for pack_path in pack_paths {
        entries.push(pack_redundant_entry(pack_path));
    }
    let duplicate_objects = pack_redundant_duplicate_objects(&entries)?;

    let mut alternate_objects = HashSet::new();
    if alt_odb {
        for objects_dir in read_alternate_object_dirs(&repo.objects_dir)? {
            let store = PackedObjectStore::new(objects_dir, GitHashAlgorithm::Sha1);
            let mut insert_id = |id: &ObjectId| {
                alternate_objects.insert(id.clone());
                Ok(())
            };
            store.for_each_object_id(&mut insert_id)?;
        }
    }

    for idx in 0..entries.len() {
        if pack_redundant_entry_is_covered(&entries[idx], &duplicate_objects, &alternate_objects)? {
            println!("{}", git_relative_display(&repo, &entries[idx].idx_path)?);
            println!("{}", git_relative_display(&repo, &entries[idx].pack_path)?);
        }
    }
    Ok(())
}

fn pack_redundant_duplicate_objects(entries: &[PackRedundantEntry]) -> Result<HashSet<ObjectId>> {
    let mut seen = HashSet::new();
    let mut duplicates = HashSet::new();
    for entry in entries {
        let reserve = pack_redundant_initial_capacity(pack_index_object_count(&entry.idx_path)?);
        seen.reserve(reserve);
        let mut record_object = |object: &ObjectId| {
            if !seen.insert(object.clone()) {
                duplicates.insert(object.clone());
            }
            Ok(())
        };
        for_each_pack_index_object_id_from_path(
            GitHashAlgorithm::Sha1,
            &entry.idx_path,
            &mut record_object,
        )?;
    }
    Ok(duplicates)
}

fn pack_redundant_initial_capacity(count: usize) -> usize {
    count.min(PACK_REDUNDANT_INITIAL_CAPACITY_LIMIT).max(1)
}

fn pack_redundant_object_is_covered(
    object: &ObjectId,
    duplicate_objects: &HashSet<ObjectId>,
    alternate_objects: &HashSet<ObjectId>,
) -> bool {
    alternate_objects.contains(object) || duplicate_objects.contains(object)
}

fn pack_redundant_entry_is_covered(
    entry: &PackRedundantEntry,
    duplicate_objects: &HashSet<ObjectId>,
    alternate_objects: &HashSet<ObjectId>,
) -> Result<bool> {
    let mut is_covered = |object: &ObjectId| {
        Ok(pack_redundant_object_is_covered(
            object,
            duplicate_objects,
            alternate_objects,
        ))
    };
    Ok(pack_index_object_ids_all_from_path(
        GitHashAlgorithm::Sha1,
        &entry.idx_path,
        &mut is_covered,
    )?)
}

fn collect_repo_pack_paths(objects_dir: &std::path::Path) -> Result<Vec<PathBuf>> {
    let pack_dir = objects_dir.join("pack");
    let mut packs = Vec::new();
    match fs::read_dir(pack_dir) {
        Ok(entries) => {
            for entry in entries {
                let path = entry?.path();
                if path.extension().and_then(|value| value.to_str()) == Some("pack")
                    && path.with_extension("idx").is_file()
                {
                    packs.push(path);
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(CliError::Io(error)),
    }
    packs.sort();
    Ok(packs)
}

fn pack_redundant_entry(path: PathBuf) -> PackRedundantEntry {
    let pack_path = if path.extension().and_then(|value| value.to_str()) == Some("idx") {
        path.with_extension("pack")
    } else {
        path
    };
    let idx_path = pack_path.with_extension("idx");
    PackRedundantEntry {
        pack_path,
        idx_path,
    }
}

fn read_alternate_object_dirs(objects_dir: &std::path::Path) -> Result<Vec<PathBuf>> {
    let alternates_path = objects_dir.join("info/alternates");
    let file = match fs::File::open(&alternates_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let base = alternates_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut reader = io::BufReader::new(file);
    let mut line = String::new();
    let mut dirs = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let path = PathBuf::from(line);
        if path.is_absolute() {
            dirs.push(path);
        } else {
            dirs.push(base.join(path));
        }
    }
    Ok(dirs)
}

pub(crate) fn verify_commit(verbose: bool, raw: bool, commits: Vec<String>) -> Result<()> {
    if commits.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "verify-commit requires at least one commit".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
    let mut failed = false;
    for commit in commits {
        let id = resolve_objectish(&repo, &commit)?;
        let object = store.read_object(&id)?;
        if object.kind != GitObjectKind::Commit {
            eprintln!(
                "error: {commit}: cannot verify a non-commit object of type {}.",
                object.kind.as_str()
            );
            failed = true;
            continue;
        }
        let content = object.content.as_slice();
        let Some((signature, payload)) = commit_signature_payload(content)? else {
            failed = true;
            continue;
        };
        if !verify_gpg_signature(&repo, &signature, &payload, raw, verbose)? {
            failed = true;
        }
    }
    if failed {
        Err(CliError::Exit(1))
    } else {
        Ok(())
    }
}

pub(crate) fn verify_tag(verbose: bool, raw: bool, tags: Vec<String>) -> Result<()> {
    if tags.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "verify-tag requires at least one tag".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(&repo.objects_dir, GitHashAlgorithm::Sha1);
    let mut failed = false;
    for tag in tags {
        let id = resolve_objectish(&repo, &tag).map_err(|_| CliError::Stderr {
            code: 1,
            text: format!("error: tag '{tag}' not found.\n"),
        })?;
        let object = store.read_object(&id)?;
        if object.kind != GitObjectKind::Tag {
            eprintln!(
                "error: {tag}: cannot verify a non-tag object of type {}.",
                object.kind.as_str()
            );
            failed = true;
            continue;
        }
        let content = object.content.as_slice();
        if verbose && !raw {
            io::stdout().write_all(content).map_err(CliError::Io)?;
        }
        let Some((signature, payload)) = tag_signature_payload(content) else {
            eprintln!("error: no signature found");
            failed = true;
            continue;
        };
        if !verify_gpg_signature(&repo, signature, payload, raw, verbose)? {
            failed = true;
        }
    }
    if failed {
        Err(CliError::Exit(1))
    } else {
        Ok(())
    }
}

fn commit_signature_payload(content: &[u8]) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
    let header_end = content
        .windows(2)
        .position(|window| window == b"\n\n")
        .ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit missing header end".into(),
        })?;
    let headers = &content[..header_end];
    let message = &content[header_end + 2..];
    let mut payload = Vec::with_capacity(content.len());
    let mut signature = Vec::new();
    let mut in_signature = false;
    for line in headers.split(|byte| *byte == b'\n') {
        if let Some(value) = line.strip_prefix(b"gpgsig ") {
            if !signature.is_empty() {
                return Err(CliError::Fatal {
                    code: 128,
                    message: "commit has multiple gpgsig headers".into(),
                });
            }
            signature.extend_from_slice(value);
            signature.push(b'\n');
            in_signature = true;
            continue;
        }
        if in_signature && line.starts_with(b" ") {
            signature.extend_from_slice(&line[1..]);
            signature.push(b'\n');
            continue;
        }
        in_signature = false;
        payload.extend_from_slice(line);
        payload.push(b'\n');
    }
    payload.push(b'\n');
    payload.extend_from_slice(message);
    if signature.is_empty() {
        Ok(None)
    } else {
        Ok(Some((signature, payload)))
    }
}

fn tag_signature_payload(content: &[u8]) -> Option<(&[u8], &[u8])> {
    let marker = b"-----BEGIN PGP SIGNATURE-----";
    let start = content
        .windows(marker.len())
        .position(|window| window == marker)?;
    Some((&content[start..], &content[..start]))
}

fn verify_gpg_signature(
    repo: &GitRepo,
    signature: &[u8],
    payload: &[u8],
    raw: bool,
    verbose: bool,
) -> Result<bool> {
    let program = read_config_value(repo, "gpg.program")?.unwrap_or_else(|| "gpg".to_owned());
    let signature_path = write_verify_signature_input(signature)?;
    let mut child = ProcessCommand::new(&program)
        .arg("--keyid-format=long")
        .arg("--status-fd=1")
        .arg("--verify")
        .arg(&signature_path)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CliError::Fatal {
            code: 1,
            message: format!("cannot exec '{program}': {error}"),
        })?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| CliError::Fatal {
            code: 1,
            message: format!("cannot open stdin for '{program}'"),
        })?
        .write_all(payload)?;
    drop(child.stdin.take());
    let output = child.wait_with_output()?;
    let _ = fs::remove_file(&signature_path);
    if raw {
        io::stderr().write_all(&output.stdout)?;
    } else if verbose {
        io::stdout().write_all(payload)?;
    }
    io::stderr().write_all(&output.stderr)?;
    Ok(output.status.success() && gpg_status_is_good(&output.stdout))
}

fn gpg_status_is_good(status: &[u8]) -> bool {
    let mut newsig = false;
    let mut goodsig = false;
    let mut validsig = false;
    let mut trusted = false;
    for line in String::from_utf8_lossy(status).lines() {
        if line.starts_with("[GNUPG:] BADSIG")
            || line.starts_with("[GNUPG:] ERRSIG")
            || line.starts_with("[GNUPG:] EXPSIG")
            || line.starts_with("[GNUPG:] EXPKEYSIG")
            || line.starts_with("[GNUPG:] REVKEYSIG")
            || line.starts_with("[GNUPG:] NO_PUBKEY")
        {
            return false;
        }
        newsig |= line.starts_with("[GNUPG:] NEWSIG");
        goodsig |= line.starts_with("[GNUPG:] GOODSIG");
        validsig |= line.starts_with("[GNUPG:] VALIDSIG");
        trusted |= line.starts_with("[GNUPG:] TRUST_");
    }
    newsig && goodsig && validsig && trusted
}

fn write_verify_signature_input(signature: &[u8]) -> Result<PathBuf> {
    let unique = format!(
        "zmin-verify-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let dir = std::env::temp_dir();
    let signature_path = dir.join(format!("{unique}.sig"));
    fs::write(&signature_path, signature)?;
    Ok(signature_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmin_git_core::{GitObjectSink, InMemoryObjectStore};

    #[test]
    fn oid_fanout_uses_linear_bucket_prefix_counts() {
        let ids = [
            object_id_with_first_byte(0x00, 0x01),
            object_id_with_first_byte(0x7f, 0x01),
            object_id_with_first_byte(0x7f, 0x02),
            object_id_with_first_byte(0xff, 0x01),
        ];

        let fanout = oid_fanout(&ids, "test fanout overflow").expect("fanout");

        assert_eq!(fanout[0], 1);
        assert_eq!(fanout[0x7e], 1);
        assert_eq!(fanout[0x7f], 3);
        assert_eq!(fanout[0xfe], 3);
        assert_eq!(fanout[0xff], 4);
    }

    #[test]
    fn pack_object_id_reader_streams_lines_and_keeps_first_seen_order() {
        let first = object_id_with_first_byte(0x01, 0x01);
        let second = object_id_with_first_byte(0x02, 0x02);
        let input = format!(
            "\n{} first-path\n  {}\n{} duplicate\n",
            first.to_hex(),
            second.to_hex(),
            first.to_hex()
        );
        let mut reader = io::Cursor::new(input.into_bytes());

        let ids = collect_pack_object_ids_from_reader(&mut reader).expect("object ids");

        assert_eq!(ids, vec![first, second]);
    }

    #[test]
    fn pack_rev_arg_reader_streams_trimmed_non_empty_lines() {
        let mut reader = io::Cursor::new(b"\n HEAD \n^refs/tags/v1\n\n".to_vec());

        let revs = collect_pack_rev_args_from_reader(&mut reader).expect("rev args");

        assert_eq!(revs, vec!["HEAD", "^refs/tags/v1"]);
    }

    #[test]
    fn fsck_seen_initial_capacity_is_bounded_for_large_stores() {
        assert_eq!(fsck_seen_initial_capacity(usize::MAX, 1), 8192);
        assert_eq!(fsck_seen_initial_capacity(2, 4), 4);
        assert_eq!(fsck_seen_initial_capacity(0, 0), 1);
    }

    #[test]
    fn fsck_tree_entry_name_initial_capacity_is_bounded() {
        assert_eq!(
            fsck_tree_entry_name_initial_capacity(usize::MAX),
            FSCK_TREE_ENTRY_NAME_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(fsck_tree_entry_name_initial_capacity(2), 2);
        assert_eq!(fsck_tree_entry_name_initial_capacity(0), 1);
    }

    #[test]
    fn pack_redundant_initial_capacity_is_bounded() {
        assert_eq!(pack_redundant_initial_capacity(usize::MAX), 8192);
        assert_eq!(pack_redundant_initial_capacity(2), 2);
        assert_eq!(pack_redundant_initial_capacity(0), 1);
    }

    #[test]
    fn pack_rev_list_object_initial_capacity_is_bounded() {
        assert_eq!(
            pack_rev_list_object_initial_capacity(usize::MAX, 1),
            PACK_REV_LIST_OBJECT_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(pack_rev_list_object_initial_capacity(2, 3), 5);
        assert_eq!(pack_rev_list_object_initial_capacity(0, 0), 1);
    }

    #[test]
    fn multi_pack_index_initial_capacity_is_bounded() {
        assert_eq!(
            multi_pack_index_initial_capacity(usize::MAX),
            MULTI_PACK_INDEX_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(multi_pack_index_initial_capacity(2), 2);
        assert_eq!(multi_pack_index_initial_capacity(0), 1);
    }

    #[test]
    fn commit_graph_initial_capacity_is_bounded() {
        assert_eq!(
            commit_graph_initial_capacity(usize::MAX),
            COMMIT_GRAPH_INITIAL_CAPACITY_LIMIT
        );
        assert_eq!(commit_graph_initial_capacity(2), 2);
        assert_eq!(commit_graph_initial_capacity(0), 1);
    }

    #[test]
    fn multi_pack_index_deduplicates_without_losing_sorted_object_order() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let pack_dir = temp.path();
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let shared = store
            .write_object(GitObjectKind::Blob, b"shared midx object\n")
            .expect("write shared object");
        let left = store
            .write_object(GitObjectKind::Blob, b"left midx object\n")
            .expect("write left object");
        let right = store
            .write_object(GitObjectKind::Blob, b"right midx object\n")
            .expect("write right object");

        write_test_pack_index(pack_dir, "pack-left", &store, &[shared.clone(), left]);
        write_test_pack_index(pack_dir, "pack-right", &store, &[right, shared.clone()]);

        let bytes = encode_multi_pack_index(
            pack_dir,
            &["pack-left.idx".to_owned(), "pack-right.idx".to_owned()],
        )
        .expect("encode multi-pack-index");
        verify_multi_pack_index_bytes(&bytes).expect("verify multi-pack-index");

        let (object_ids, pack_ids) = multi_pack_index_object_ids_and_pack_ids(&bytes);
        let mut sorted = object_ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(object_ids, sorted);
        assert_eq!(object_ids.len(), 3);
        let shared_index = object_ids
            .iter()
            .position(|id| id.as_slice() == shared.as_bytes())
            .expect("shared object is present");
        assert_eq!(pack_ids[shared_index], 0);
    }

    #[test]
    fn object_id_chunk_sort_check_streams_without_collecting_windows() {
        assert!(object_id_chunks_are_strictly_sorted(&[], 2));
        assert!(object_id_chunks_are_strictly_sorted(&[0, 1, 0, 2, 1, 0], 2));
        assert!(!object_id_chunks_are_strictly_sorted(&[0, 1, 0, 1], 2));
        assert!(!object_id_chunks_are_strictly_sorted(&[0, 2, 0, 1], 2));
    }

    #[test]
    fn chunk_range_lookup_can_use_small_offset_table_without_hashmap() {
        let bytes = b"0123456789abcdef";
        let chunks = vec![(*b"AAAA", 2), (*b"BBBB", 6), (*b"CCCC", 11)];

        assert_eq!(
            commit_graph_chunk_range_from_offsets(bytes, &chunks, b"BBBB", bytes.len())
                .expect("chunk"),
            b"6789a"
        );
    }

    #[test]
    fn multi_pack_index_redundancy_checks_index_subsets_without_id_vectors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let pack_dir = temp.path();
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let first = store
            .write_object(GitObjectKind::Blob, b"first\n")
            .expect("write first");
        let second = store
            .write_object(GitObjectKind::Blob, b"second\n")
            .expect("write second");
        let third = store
            .write_object(GitObjectKind::Blob, b"third\n")
            .expect("write third");
        write_test_pack_index(pack_dir, "pack-a", &store, &[first.clone(), third.clone()]);
        write_test_pack_index(
            pack_dir,
            "pack-b",
            &store,
            &[first.clone(), second, third.clone()],
        );
        write_test_pack_index(pack_dir, "pack-c", &store, &[first, third]);
        let packs = vec![
            "pack-a.idx".to_owned(),
            "pack-b.idx".to_owned(),
            "pack-c.idx".to_owned(),
        ];
        let counts = multi_pack_index_pack_object_counts(pack_dir, &packs).expect("pack counts");

        assert!(
            multi_pack_index_pack_is_redundant(pack_dir, &packs, &counts, 0)
                .expect("pack-a redundancy")
        );
        assert!(
            !multi_pack_index_pack_is_redundant(pack_dir, &packs, &counts, 1)
                .expect("pack-b redundancy")
        );
        assert!(
            multi_pack_index_pack_is_redundant(pack_dir, &packs, &counts, 2)
                .expect("pack-c redundancy")
        );
    }

    #[test]
    fn pack_redundant_checks_stream_pack_indexes_without_entry_object_vectors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let pack_dir = temp.path();
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let shared = store
            .write_object(GitObjectKind::Blob, b"shared redundant object\n")
            .expect("write shared");
        let left = store
            .write_object(GitObjectKind::Blob, b"left redundant object\n")
            .expect("write left");
        let right = store
            .write_object(GitObjectKind::Blob, b"right redundant object\n")
            .expect("write right");
        write_test_pack_index(
            pack_dir,
            "pack-left",
            &store,
            &[shared.clone(), left.clone()],
        );
        write_test_pack_index(
            pack_dir,
            "pack-right",
            &store,
            &[shared.clone(), right.clone()],
        );
        write_test_pack_index(
            pack_dir,
            "pack-shared",
            &store,
            std::slice::from_ref(&shared),
        );
        let entries = vec![
            pack_redundant_entry(pack_dir.join("pack-left.pack")),
            pack_redundant_entry(pack_dir.join("pack-right.idx")),
            pack_redundant_entry(pack_dir.join("pack-shared.pack")),
        ];

        let duplicates = pack_redundant_duplicate_objects(&entries).expect("duplicates");
        let mut alternates = HashSet::new();

        assert!(duplicates.contains(&shared));
        assert!(
            !pack_redundant_entry_is_covered(&entries[0], &duplicates, &alternates)
                .expect("left not covered")
        );
        assert!(
            pack_redundant_entry_is_covered(&entries[2], &duplicates, &alternates)
                .expect("shared covered")
        );

        alternates.insert(left);
        assert!(
            pack_redundant_entry_is_covered(&entries[0], &duplicates, &alternates)
                .expect("left covered by alternate")
        );
        assert!(
            !pack_redundant_entry_is_covered(&entries[1], &duplicates, &alternates)
                .expect("right still not covered")
        );
    }

    #[test]
    fn alternate_object_dirs_reader_streams_lines_and_resolves_relative_paths() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let objects_dir = temp.path().join("objects");
        let info_dir = objects_dir.join("info");
        fs::create_dir_all(&info_dir).expect("create info dir");
        let absolute = temp.path().join("absolute-objects");
        fs::write(
            info_dir.join("alternates"),
            format!("\n{}\nrelative/objects\n\n", absolute.display()),
        )
        .expect("write alternates");

        let dirs = read_alternate_object_dirs(&objects_dir).expect("read alternates");

        assert_eq!(dirs, vec![absolute, info_dir.join("relative/objects")]);
    }

    #[test]
    fn commit_graph_positions_and_generations_use_object_ids_without_hex_keys() {
        let store = InMemoryObjectStore::new(GitHashAlgorithm::Sha1);
        let tree = store
            .write_object(
                GitObjectKind::Tree,
                &encode_tree(&[]).expect("encode empty tree"),
            )
            .expect("write tree");
        let root = write_test_commit(&store, &tree, &[], 1, b"root\n");
        let left = write_test_commit(&store, &tree, std::slice::from_ref(&root), 2, b"left\n");
        let right = write_test_commit(&store, &tree, std::slice::from_ref(&root), 3, b"right\n");
        let merge = write_test_commit(&store, &tree, &[left.clone(), right.clone()], 4, b"merge\n");
        let cache = CommitObjectCache::new(&store);

        let bytes = encode_commit_graph(
            &cache,
            &[merge.clone(), right.clone(), root.clone(), left.clone()],
        )
        .expect("encode commit graph");
        verify_commit_graph_bytes(&bytes).expect("verify commit graph");

        let graph = parsed_commit_graph_rows(&bytes);
        let merge_row = graph.get(merge.as_bytes()).expect("merge row");
        let left_row = graph.get(left.as_bytes()).expect("left row");
        let right_row = graph.get(right.as_bytes()).expect("right row");
        assert_eq!(merge_row.parent_one, left_row.position);
        assert_eq!(merge_row.parent_two, right_row.position);
        assert_eq!(merge_row.generation, 3);
    }

    fn write_test_pack_index(
        pack_dir: &std::path::Path,
        name: &str,
        store: &InMemoryObjectStore,
        ids: &[ObjectId],
    ) {
        let mut pack = Vec::new();
        write_pack_from_store_with_options(
            store,
            GitHashAlgorithm::Sha1,
            ids,
            PackEncodeOptions::UNDELTIFIED,
            &mut pack,
        )
        .expect("write pack");
        let indexed = index_pack_bytes(GitHashAlgorithm::Sha1, &pack).expect("index pack");
        fs::write(pack_dir.join(format!("{name}.pack")), pack).expect("write pack file");
        fs::write(pack_dir.join(format!("{name}.idx")), indexed.index).expect("write index file");
    }

    fn write_test_commit(
        store: &InMemoryObjectStore,
        tree: &ObjectId,
        parents: &[ObjectId],
        timestamp: i64,
        message: &[u8],
    ) -> ObjectId {
        let author = Signature::new("A", "a@example.test", timestamp, "+0000").expect("author");
        let committer =
            Signature::new("C", "c@example.test", timestamp, "+0000").expect("committer");
        let mut builder = CommitBuilder::new(tree.clone(), author, committer);
        for parent in parents {
            builder = builder.parent(parent.clone());
        }
        store
            .write_object(
                GitObjectKind::Commit,
                &builder
                    .message(message)
                    .expect("commit message")
                    .encode()
                    .expect("encode commit"),
            )
            .expect("write commit")
    }

    fn object_id_with_first_byte(first: u8, last: u8) -> ObjectId {
        let mut bytes = [0_u8; 20];
        bytes[0] = first;
        bytes[19] = last;
        ObjectId::from_hex(
            GitHashAlgorithm::Sha1,
            &bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .expect("object id")
    }

    #[derive(Debug)]
    struct CommitGraphRow {
        position: u32,
        parent_one: u32,
        parent_two: u32,
        generation: u32,
    }

    fn parsed_commit_graph_rows(bytes: &[u8]) -> HashMap<Vec<u8>, CommitGraphRow> {
        let digest_len = GitHashAlgorithm::Sha1.digest_len();
        let graph_data_end = bytes.len() - digest_len - bytes[7] as usize * digest_len;
        let chunk_count = bytes[6] as usize;
        let mut chunks = Vec::new();
        for idx in 0..chunk_count {
            let cursor = 8 + idx * 12;
            chunks.push((
                [
                    bytes[cursor],
                    bytes[cursor + 1],
                    bytes[cursor + 2],
                    bytes[cursor + 3],
                ],
                read_u64_be(&bytes[cursor + 4..cursor + 12]).expect("read chunk offset") as usize,
            ));
        }
        let oidf = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDF", graph_data_end)
            .expect("OIDF chunk");
        let oidl = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDL", graph_data_end)
            .expect("OIDL chunk");
        let cdat = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"CDAT", graph_data_end)
            .expect("CDAT chunk");
        let commit_count = read_u32_be(&oidf[255 * 4..256 * 4]).expect("read commit count");
        let commit_count = commit_count as usize;
        let mut rows = HashMap::new();
        for position in 0..commit_count {
            let id_start = position * digest_len;
            let data_start = position * (digest_len + 16);
            let generation_and_time_high =
                read_u32_be(&cdat[data_start + digest_len + 8..data_start + digest_len + 12])
                    .expect("read generation");
            rows.insert(
                oidl[id_start..id_start + digest_len].to_vec(),
                CommitGraphRow {
                    position: position as u32,
                    parent_one: read_u32_be(
                        &cdat[data_start + digest_len..data_start + digest_len + 4],
                    )
                    .expect("read first parent"),
                    parent_two: read_u32_be(
                        &cdat[data_start + digest_len + 4..data_start + digest_len + 8],
                    )
                    .expect("read second parent"),
                    generation: generation_and_time_high >> 2,
                },
            );
        }
        rows
    }

    fn multi_pack_index_object_ids_and_pack_ids(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<u32>) {
        let digest_len = GitHashAlgorithm::Sha1.digest_len();
        let graph_data_end = bytes.len() - digest_len;
        let chunk_count = bytes[6] as usize;
        let mut chunks = Vec::new();
        for idx in 0..chunk_count {
            let cursor = 12 + idx * 12;
            chunks.push((
                [
                    bytes[cursor],
                    bytes[cursor + 1],
                    bytes[cursor + 2],
                    bytes[cursor + 3],
                ],
                read_u64_be(&bytes[cursor + 4..cursor + 12]).expect("read chunk offset") as usize,
            ));
        }
        let oidf = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDF", graph_data_end)
            .expect("OIDF chunk");
        let oidl = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OIDL", graph_data_end)
            .expect("OIDL chunk");
        let ooff = commit_graph_chunk_range_from_offsets(bytes, &chunks, b"OOFF", graph_data_end)
            .expect("OOFF chunk");
        let object_count = read_u32_be(&oidf[255 * 4..256 * 4]).expect("read object count");
        let object_count = object_count as usize;
        let object_ids = oidl[..object_count * digest_len]
            .chunks_exact(digest_len)
            .map(|id| id.to_vec())
            .collect::<Vec<_>>();
        let pack_ids = ooff[..object_count * 8]
            .chunks_exact(8)
            .map(|entry| read_u32_be(&entry[..4]).expect("read pack id"))
            .collect::<Vec<_>>();
        (object_ids, pack_ids)
    }
}
