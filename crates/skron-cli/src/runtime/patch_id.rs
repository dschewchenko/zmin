use skron_git_core::{GitHashAlgorithm, GitObjectHash};

use super::reference_commands;

pub(crate) fn patch_id_generate(
    input: &[u8],
    mode: reference_commands::PatchIdMode,
) -> Vec<([u8; 20], String)> {
    let lines = split_patch_lines(input);
    let mut cursor = 0usize;
    let mut oid = "0000000000000000000000000000000000000000".to_owned();
    let mut out = Vec::new();
    while cursor < lines.len() {
        let (patchlen, next_oid, result) = patch_id_one(&lines, &mut cursor, mode);
        if patchlen > 0 {
            out.push((result, oid.clone()));
        }
        oid = next_oid.unwrap_or_else(|| "0000000000000000000000000000000000000000".to_owned());
    }
    out
}

fn split_patch_lines(input: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (index, byte) in input.iter().enumerate() {
        if *byte == b'\n' {
            lines.push(&input[start..=index]);
            start = index + 1;
        }
    }
    if start < input.len() {
        lines.push(&input[start..]);
    }
    lines
}

fn patch_id_one(
    lines: &[&[u8]],
    cursor: &mut usize,
    mode: reference_commands::PatchIdMode,
) -> (usize, Option<String>, [u8; 20]) {
    let mut patchlen = 0usize;
    let mut before = -1isize;
    let mut after = -1isize;
    let mut hasher = GitObjectHash::new(GitHashAlgorithm::Sha1);
    let mut result = [0u8; 20];
    let mut next_oid = None;

    while *cursor < lines.len() {
        let line = lines[*cursor];
        *cursor += 1;
        let mut oid_scan = line;
        let has_oid_prefix = if let Some(rest) = strip_bytes_prefix(line, b"diff-tree ") {
            oid_scan = rest;
            true
        } else if let Some(rest) = strip_bytes_prefix(line, b"commit ") {
            oid_scan = rest;
            true
        } else if let Some(rest) = strip_bytes_prefix(line, b"From ") {
            oid_scan = rest;
            true
        } else {
            false
        };
        if !has_oid_prefix && line.starts_with(b"\\ ") && line.len() > 12 {
            continue;
        }
        if let Some(hex_id) = parse_patch_id_hex(oid_scan) {
            next_oid = Some(hex_id);
            break;
        }

        if patchlen == 0 && !line.starts_with(b"diff ") {
            continue;
        }

        if before == -1 {
            if line.starts_with(b"index ") {
                continue;
            } else if line.starts_with(b"--- ") {
                before = 1;
                after = 1;
            } else if !line.first().is_some_and(u8::is_ascii_alphabetic) {
                break;
            }
        }

        if before == 0 && after == 0 {
            if line.starts_with(b"@@ -") {
                if let Some((parsed_before, parsed_after)) = scan_patch_hunk_header(line) {
                    before = parsed_before;
                    after = parsed_after;
                }
                continue;
            }
            if !line.starts_with(b"diff ") {
                break;
            }
            if mode == reference_commands::PatchIdMode::Stable {
                patch_id_flush(&mut result, &mut hasher);
            }
            before = -1;
            after = -1;
        }

        if line.first().is_some_and(|byte| matches!(byte, b'-' | b' ')) {
            before -= 1;
        }
        if line.first().is_some_and(|byte| matches!(byte, b'+' | b' ')) {
            after -= 1;
        }

        let normalized = patch_id_normalize_line(line, mode);
        patchlen += normalized.len();
        hasher.update(&normalized);
    }

    patch_id_flush(&mut result, &mut hasher);
    (patchlen, next_oid, result)
}

fn strip_bytes_prefix<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    line.starts_with(prefix).then(|| &line[prefix.len()..])
}

fn parse_patch_id_hex(line: &[u8]) -> Option<String> {
    let candidate = line.get(..40)?;
    candidate
        .iter()
        .all(u8::is_ascii_hexdigit)
        .then(|| String::from_utf8_lossy(candidate).into_owned())
}

fn scan_patch_hunk_header(line: &[u8]) -> Option<(isize, isize)> {
    let text = std::str::from_utf8(line).ok()?;
    let rest = text.strip_prefix("@@ -")?;
    let (before_text, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix('+')?;
    let after_text = rest.split_whitespace().next()?;
    Some((
        parse_patch_hunk_count(before_text)?,
        parse_patch_hunk_count(after_text)?,
    ))
}

fn parse_patch_hunk_count(text: &str) -> Option<isize> {
    let count = text.split_once(',').map_or("1", |(_, count)| count);
    count.parse().ok()
}

pub(crate) fn patch_id_normalize_line(
    line: &[u8],
    mode: reference_commands::PatchIdMode,
) -> Vec<u8> {
    if mode == reference_commands::PatchIdMode::Verbatim {
        return line.to_vec();
    }
    line.iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect()
}

fn patch_id_flush(result: &mut [u8; 20], hasher: &mut GitObjectHash) {
    let old = std::mem::replace(hasher, GitObjectHash::new(GitHashAlgorithm::Sha1));
    let digest = old.finalize();
    let mut carry = 0u16;
    for (out, byte) in result.iter_mut().zip(digest.as_bytes()) {
        carry += u16::from(*out) + u16::from(*byte);
        *out = carry as u8;
        carry >>= 8;
    }
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
