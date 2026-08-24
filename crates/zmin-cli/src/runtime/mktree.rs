use std::cmp::Ordering;
use std::io::{self, BufReader, Read, Write};

use zmin_cli_runtime::RawStderrBytes;
use zmin_git_core::{GitHashAlgorithm, GitObjectKind, LooseObjectStore, ObjectId};

use super::{CliError, Result};

/// One record read from mktree's byte-oriented input stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MktreeRecord {
    pub(crate) bytes: Vec<u8>,
    pub(crate) terminated: bool,
}

/// Delimiter-aware reader used by mktree. It deliberately does not perform
/// text decoding or whitespace normalization.
pub(crate) struct MktreeRecordReader<R> {
    reader: BufReader<R>,
    delimiter: u8,
}

impl<R: Read> MktreeRecordReader<R> {
    pub(crate) fn new(reader: R, nul_terminated: bool) -> Self {
        Self {
            reader: BufReader::new(reader),
            delimiter: if nul_terminated { 0 } else { b'\n' },
        }
    }

    pub(crate) fn next_record(&mut self) -> io::Result<Option<MktreeRecord>> {
        let mut bytes = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            match self.reader.read(&mut byte)? {
                0 if bytes.is_empty() => return Ok(None),
                0 => {
                    return Ok(Some(MktreeRecord {
                        bytes,
                        terminated: false,
                    }));
                }
                _ if byte[0] == self.delimiter => {
                    return Ok(Some(MktreeRecord {
                        bytes,
                        terminated: true,
                    }));
                }
                _ => bytes.push(byte[0]),
            }
        }
    }
}

/// A parsed mktree entry. Unlike the general tree API, mode and names are
/// intentionally left unconstrained to match Git's plumbing command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MktreeEntry {
    pub(crate) mode: u32,
    pub(crate) name: Vec<u8>,
    pub(crate) id: ObjectId,
}

pub(crate) struct MktreeTreeWriter<'a> {
    store: &'a LooseObjectStore,
}

impl<'a> MktreeTreeWriter<'a> {
    pub(crate) fn new(store: &'a LooseObjectStore) -> Self {
        Self { store }
    }

    pub(crate) fn write<W: Write>(
        &self,
        entries: &mut Vec<MktreeEntry>,
        out: &mut W,
    ) -> Result<()> {
        entries.sort_by(mktree_entry_compare);

        let mut encoded = Vec::new();
        for entry in entries.iter() {
            encoded.extend_from_slice(format!("{:o}", entry.mode).as_bytes());
            encoded.push(b' ');
            encoded.extend_from_slice(&entry.name);
            encoded.push(0);
            encoded.extend_from_slice(entry.id.as_bytes());
        }
        let id = self
            .store
            .write_object(GitObjectKind::Tree, &encoded)
            .map_err(CliError::Io)?;
        id.write_hex_io(out).map_err(|_| {
            CliError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "failed to write mktree object id",
            ))
        })?;
        out.write_all(b"\n").map_err(CliError::Io)?;
        out.flush().map_err(CliError::Io)?;
        Ok(())
    }
}

pub(crate) fn run_mktree<R: Read, W: Write>(
    store: &LooseObjectStore,
    reader: R,
    output: &mut W,
    nul_terminated: bool,
    allow_missing: bool,
    batch: bool,
) -> Result<()> {
    let mut reader = MktreeRecordReader::new(reader, nul_terminated);
    let writer = MktreeTreeWriter::new(store);
    let mut entries = Vec::new();

    loop {
        let Some(record) = reader.next_record().map_err(CliError::Io)? else {
            if !batch || !entries.is_empty() {
                writer.write(&mut entries, output)?;
            }
            entries.clear();
            break;
        };

        // Git's strbuf is also a C string. In text mode an embedded NUL
        // terminates all subsequent C parsing and is never part of the name.
        let record_bytes = if nul_terminated {
            record.bytes.as_slice()
        } else {
            record
                .bytes
                .split(|byte| *byte == 0)
                .next()
                .unwrap_or_default()
        };

        if record_bytes.is_empty() {
            if !batch {
                return Err(mktree_raw_error(
                    b"input format error: (blank line only valid in batch mode)\n",
                ));
            }
            writer.write(&mut entries, output)?;
            entries.clear();
            continue;
        }

        entries.push(parse_mktree_record(
            store.algorithm(),
            store,
            record_bytes,
            nul_terminated,
            allow_missing,
        )?);
    }
    Ok(())
}

fn parse_mktree_record(
    algorithm: GitHashAlgorithm,
    store: &LooseObjectStore,
    record: &[u8],
    nul_terminated: bool,
    allow_missing: bool,
) -> Result<MktreeEntry> {
    // strtoul stops at the first non-octal byte, not at arbitrary C
    // whitespace. The separator must then be exactly one literal space.
    let (mode, mode_stop) = parse_octal_mode(record);
    if mode_stop == 0 || mode_stop >= record.len() || record[mode_stop] != b' ' {
        return Err(input_format_error(record));
    }

    let type_start = mode_stop + 1;
    let Some(type_relative_end) = record[type_start..].iter().position(|byte| *byte == b' ') else {
        return Err(input_format_error(record));
    };
    let type_end = type_start + type_relative_end;
    let object_type = &record[type_start..type_end];
    let oid_start = type_end + 1;
    let digest_hex_len = algorithm.digest_len() * 2;
    if oid_start + digest_hex_len > record.len() {
        return Err(input_format_error(record));
    }
    let oid_end = oid_start + digest_hex_len;
    let id = ObjectId::from_hex_bytes(algorithm, &record[oid_start..oid_end])
        .map_err(|_| input_format_error(record))?;
    if oid_end >= record.len() || record[oid_end] != b'\t' {
        return Err(input_format_error(record));
    }

    let mode_kind = mktree_mode_kind(mode);
    let Some(type_kind) = GitObjectKind::parse(object_type) else {
        return Err(mktree_raw_error_with_parts(&[
            b"invalid object type \"",
            object_type,
            b"\"\n",
        ]));
    };
    if mode_kind != type_kind {
        let path = parse_mktree_name(&record[oid_end + 1..], nul_terminated)?;
        return Err(mktree_raw_error_with_parts(&[
            b"entry '",
            &path,
            b"' object type (",
            type_kind.as_bytes(),
            b") doesn't match mode type (",
            mode_kind.as_bytes(),
            b")\n",
        ]));
    }

    let path = parse_mktree_name(&record[oid_end + 1..], nul_terminated)?;
    let is_gitlink = mode_kind == GitObjectKind::Commit;
    match store.read_object(&id) {
        Ok(object) if object.kind != mode_kind => {
            return Err(mktree_raw_error_with_parts(&[
                b"entry '",
                &path,
                b"' object ",
                id.to_hex().as_bytes(),
                b" is a ",
                object.kind.as_bytes(),
                b" but specified type was (",
                mode_kind.as_bytes(),
                b")\n",
            ]));
        }
        Ok(_) => {}
        Err(_) if allow_missing || is_gitlink => {}
        Err(_) => {
            return Err(mktree_raw_error_with_parts(&[
                b"entry '",
                &path,
                b"' object ",
                id.to_hex().as_bytes(),
                b" is unavailable\n",
            ]));
        }
    }

    if path.contains(&b'/') {
        return Err(mktree_raw_error_with_parts(&[
            b"path ",
            &path,
            b" contains slash\n",
        ]));
    }
    Ok(MktreeEntry {
        mode,
        name: path,
        id,
    })
}

fn parse_mktree_name(raw: &[u8], nul_terminated: bool) -> Result<Vec<u8>> {
    let mut path = if !nul_terminated && raw.first() == Some(&b'"') {
        c_unquote(raw).ok_or_else(|| mktree_raw_error(b"invalid quoting\n"))?
    } else {
        raw.to_vec()
    };
    // append_to_tree() uses strlen(), so an octal/escaped NUL terminates a
    // decoded name just as an input NUL terminates a text record.
    if let Some(nul) = path.iter().position(|byte| *byte == 0) {
        path.truncate(nul);
    }
    Ok(path)
}

fn c_unquote(raw: &[u8]) -> Option<Vec<u8>> {
    if raw.first().copied() != Some(b'"') {
        return None;
    }
    let mut output = Vec::new();
    let mut cursor = 1;
    loop {
        let start = cursor;
        while cursor < raw.len() && raw[cursor] != b'"' && raw[cursor] != b'\\' {
            cursor += 1;
        }
        output.extend_from_slice(&raw[start..cursor]);
        let marker = *raw.get(cursor)?;
        cursor += 1;
        match marker {
            b'"' => return Some(output),
            b'\\' => {}
            _ => return None,
        }
        let escaped = *raw.get(cursor)?;
        cursor += 1;
        let decoded = match escaped {
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => 0x0a,
            b'r' => 0x0d,
            b't' => 0x09,
            b'v' => 0x0b,
            b'\\' | b'"' => escaped,
            b'0'..=b'3' => {
                let second = *raw.get(cursor)?;
                let third = *raw.get(cursor + 1)?;
                if !(b'0'..=b'7').contains(&second) || !(b'0'..=b'7').contains(&third) {
                    return None;
                }
                cursor += 2;
                ((escaped - b'0') << 6) | ((second - b'0') << 3) | (third - b'0')
            }
            _ => return None,
        };
        output.push(decoded);
    }
}

fn parse_octal_mode(record: &[u8]) -> (u32, usize) {
    let mut cursor = 0;
    while record
        .get(cursor)
        .is_some_and(|byte| is_c_whitespace(*byte))
    {
        cursor += 1;
    }
    let negative = match record.get(cursor) {
        Some(b'+') => {
            cursor += 1;
            false
        }
        Some(b'-') => {
            cursor += 1;
            true
        }
        _ => false,
    };
    let first_digit = cursor;
    let mut value = 0_u64;
    let mut overflow = false;
    while let Some(byte @ b'0'..=b'7') = record.get(cursor).copied() {
        let digit = u64::from(byte - b'0');
        value = match value
            .checked_mul(8)
            .and_then(|value| value.checked_add(digit))
        {
            Some(value) => value,
            None => {
                overflow = true;
                u64::MAX
            }
        };
        cursor += 1;
    }
    if cursor == first_digit {
        return (0, 0);
    }
    if overflow {
        value = u64::MAX;
    }
    // strtoul saturates at ULONG_MAX on overflow before the sign is applied;
    // glibc therefore yields the same ULONG_MAX result for either sign.
    if negative && !overflow {
        value = 0_u64.wrapping_sub(value);
    }
    (value as u32, cursor)
}

fn is_c_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn mktree_mode_kind(mode: u32) -> GitObjectKind {
    match mode & 0o170000 {
        0o040000 => GitObjectKind::Tree,
        0o160000 => GitObjectKind::Commit,
        _ => GitObjectKind::Blob,
    }
}

fn is_tree_mode(mode: u32) -> bool {
    mode & 0o170000 == 0o040000
}

fn mktree_entry_compare(left: &MktreeEntry, right: &MktreeEntry) -> Ordering {
    let common_len = left.name.len().min(right.name.len());
    let ordering = left.name[..common_len].cmp(&right.name[..common_len]);
    if ordering != Ordering::Equal {
        return ordering;
    }
    let mut left_next = left.name.get(common_len).copied().unwrap_or(0);
    let mut right_next = right.name.get(common_len).copied().unwrap_or(0);
    if left_next == 0 && is_tree_mode(left.mode) {
        left_next = b'/';
    }
    if right_next == 0 && is_tree_mode(right.mode) {
        right_next = b'/';
    }
    left_next.cmp(&right_next)
}

fn input_format_error(record: &[u8]) -> CliError {
    mktree_raw_error_with_parts(&[b"input format error: ", record, b"\n"])
}

fn mktree_raw_error(message: &[u8]) -> CliError {
    mktree_raw_error_with_parts(&[message])
}

fn mktree_raw_error_with_parts(parts: &[&[u8]]) -> CliError {
    let size: usize = parts.iter().map(|part| part.len()).sum();
    let mut bytes = Vec::with_capacity(size + b"fatal: ".len());
    bytes.extend_from_slice(b"fatal: ");
    for part in parts {
        // Git's vfreportf sanitizes control bytes in every die() message,
        // except horizontal tab and line feed.
        for byte in part.iter().copied() {
            if byte.is_ascii_control() && byte != b'\t' && byte != b'\n' {
                bytes.push(b'?');
            } else {
                bytes.push(byte);
            }
        }
    }
    CliError::Io(io::Error::new(
        io::ErrorKind::InvalidData,
        RawStderrBytes { code: 128, bytes },
    ))
}
