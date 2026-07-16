#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeFileLabels {
    pub current: String,
    pub ancestor: String,
    pub other: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeFileResult {
    pub content: Vec<u8>,
    pub conflicts: usize,
}

pub fn merge_file(
    current: &[u8],
    ancestor: &[u8],
    other: &[u8],
    labels: &MergeFileLabels,
) -> MergeFileResult {
    if current == other {
        return MergeFileResult {
            content: current.to_vec(),
            conflicts: 0,
        };
    }
    if current == ancestor {
        return MergeFileResult {
            content: other.to_vec(),
            conflicts: 0,
        };
    }
    if other == ancestor {
        return MergeFileResult {
            content: current.to_vec(),
            conflicts: 0,
        };
    }

    let current_lines = split_lines(current);
    let ancestor_lines = split_lines(ancestor);
    let other_lines = split_lines(other);
    let mut prefix_len = 0usize;
    while prefix_len < current_lines.len()
        && prefix_len < ancestor_lines.len()
        && prefix_len < other_lines.len()
        && current_lines[prefix_len] == ancestor_lines[prefix_len]
        && ancestor_lines[prefix_len] == other_lines[prefix_len]
    {
        prefix_len += 1;
    }

    let mut suffix_len = 0usize;
    while prefix_len + suffix_len < current_lines.len()
        && prefix_len + suffix_len < ancestor_lines.len()
        && prefix_len + suffix_len < other_lines.len()
        && current_lines[current_lines.len() - suffix_len - 1]
            == ancestor_lines[ancestor_lines.len() - suffix_len - 1]
        && ancestor_lines[ancestor_lines.len() - suffix_len - 1]
            == other_lines[other_lines.len() - suffix_len - 1]
    {
        suffix_len += 1;
    }

    let current_mid = &current_lines[prefix_len..current_lines.len() - suffix_len];
    let ancestor_mid = &ancestor_lines[prefix_len..ancestor_lines.len() - suffix_len];
    let other_mid = &other_lines[prefix_len..other_lines.len() - suffix_len];

    let resolved_mid = if current_mid == ancestor_mid {
        Some(other_mid)
    } else if other_mid == ancestor_mid || current_mid == other_mid {
        Some(current_mid)
    } else {
        None
    };

    let mut out = Vec::new();
    append_lines(&mut out, &current_lines[..prefix_len]);
    if let Some(resolved_mid) = resolved_mid {
        append_lines(&mut out, resolved_mid);
        append_lines(&mut out, &current_lines[current_lines.len() - suffix_len..]);
        return MergeFileResult {
            content: out,
            conflicts: 0,
        };
    }

    append_conflict(&mut out, current_mid, other_mid, labels);
    append_lines(&mut out, &current_lines[current_lines.len() - suffix_len..]);
    MergeFileResult {
        content: out,
        conflicts: 1,
    }
}

pub fn merge_file_diff3(
    current: &[u8],
    ancestor: &[u8],
    other: &[u8],
    labels: &MergeFileLabels,
) -> MergeFileResult {
    let mut result = merge_file(current, ancestor, other, labels);
    if result.conflicts == 0 {
        return result;
    }

    let current_lines = split_lines(current);
    let ancestor_lines = split_lines(ancestor);
    let other_lines = split_lines(other);
    let (prefix_len, suffix_len) = common_edges(&current_lines, &ancestor_lines, &other_lines);
    let mut out = Vec::new();
    append_lines(&mut out, &current_lines[..prefix_len]);
    append_marker(&mut out, b'<', &labels.current);
    append_lines(
        &mut out,
        &current_lines[prefix_len..current_lines.len() - suffix_len],
    );
    append_marker(&mut out, b'|', &labels.ancestor);
    append_lines(
        &mut out,
        &ancestor_lines[prefix_len..ancestor_lines.len() - suffix_len],
    );
    out.extend_from_slice(b"=======\n");
    append_lines(
        &mut out,
        &other_lines[prefix_len..other_lines.len() - suffix_len],
    );
    append_marker(&mut out, b'>', &labels.other);
    append_lines(&mut out, &current_lines[current_lines.len() - suffix_len..]);
    result.content = out;
    result
}

fn common_edges<'a>(
    current: &[&'a [u8]],
    ancestor: &[&'a [u8]],
    other: &[&'a [u8]],
) -> (usize, usize) {
    let mut prefix_len = 0usize;
    while prefix_len < current.len()
        && prefix_len < ancestor.len()
        && prefix_len < other.len()
        && current[prefix_len] == ancestor[prefix_len]
        && ancestor[prefix_len] == other[prefix_len]
    {
        prefix_len += 1;
    }
    let mut suffix_len = 0usize;
    while prefix_len + suffix_len < current.len()
        && prefix_len + suffix_len < ancestor.len()
        && prefix_len + suffix_len < other.len()
        && current[current.len() - suffix_len - 1] == ancestor[ancestor.len() - suffix_len - 1]
        && ancestor[ancestor.len() - suffix_len - 1] == other[other.len() - suffix_len - 1]
    {
        suffix_len += 1;
    }
    (prefix_len, suffix_len)
}

fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (idx, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            lines.push(&bytes[start..=idx]);
            start = idx + 1;
        }
    }
    if start < bytes.len() {
        lines.push(&bytes[start..]);
    }
    lines
}

fn append_lines(out: &mut Vec<u8>, lines: &[&[u8]]) {
    for line in lines {
        out.extend_from_slice(line);
    }
}

fn append_conflict(
    out: &mut Vec<u8>,
    current: &[&[u8]],
    other: &[&[u8]],
    labels: &MergeFileLabels,
) {
    append_marker(out, b'<', &labels.current);
    append_lines(out, current);
    out.extend_from_slice(b"=======\n");
    append_lines(out, other);
    append_marker(out, b'>', &labels.other);
}

fn append_marker(out: &mut Vec<u8>, marker: u8, label: &str) {
    out.extend(std::iter::repeat_n(marker, 7));
    if !label.is_empty() {
        out.push(b' ');
        out.extend_from_slice(label.as_bytes());
    }
    out.push(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_clean_side_changes() {
        let labels = labels();
        assert_eq!(
            merge_file(b"a\nours\nc\n", b"a\nb\nc\n", b"a\nb\nc\n", &labels),
            MergeFileResult {
                content: b"a\nours\nc\n".to_vec(),
                conflicts: 0,
            }
        );
        assert_eq!(
            merge_file(b"a\nb\nc\n", b"a\nb\nc\n", b"a\ntheirs\nc\n", &labels),
            MergeFileResult {
                content: b"a\ntheirs\nc\n".to_vec(),
                conflicts: 0,
            }
        );
    }

    #[test]
    fn emits_standard_conflict_markers() {
        assert_eq!(
            merge_file(b"a\nours\nc\n", b"a\nb\nc\n", b"a\ntheirs\nc\n", &labels()),
            MergeFileResult {
                content: b"a\n<<<<<<< ours\nours\n=======\ntheirs\n>>>>>>> theirs\nc\n".to_vec(),
                conflicts: 1,
            }
        );
    }

    #[test]
    fn emits_diff3_and_empty_label_markers() {
        assert_eq!(
            merge_file_diff3(
                b"ours\n",
                b"base\n",
                b"theirs\n",
                &MergeFileLabels {
                    current: String::new(),
                    ancestor: "base".to_owned(),
                    other: String::new(),
                },
            ),
            MergeFileResult {
                content: b"<<<<<<<\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>>\n".to_vec(),
                conflicts: 1,
            }
        );
    }

    fn labels() -> MergeFileLabels {
        MergeFileLabels {
            current: "ours".to_owned(),
            ancestor: "base".to_owned(),
            other: "theirs".to_owned(),
        }
    }
}
