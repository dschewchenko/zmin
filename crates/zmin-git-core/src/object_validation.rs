use std::cmp::Ordering;
use std::collections::HashMap;
use std::io;

use crate::pack::{PackObjectValidationReason, PackValidationMode};
use crate::{GitHashAlgorithm, GitObjectKind, ObjectId, TreeMode};

const MAX_TREE_ENTRY_NAME_LEN: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeLinkMode {
    Tree,
    Blob,
    Gitlink,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedTreeMode {
    ordering_mode: TreeMode,
    link_mode: TreeLinkMode,
}

impl ParsedTreeMode {
    fn expected_kind(self) -> Option<GitObjectKind> {
        match self.link_mode {
            TreeLinkMode::Tree => Some(GitObjectKind::Tree),
            TreeLinkMode::Blob => Some(GitObjectKind::Blob),
            TreeLinkMode::Gitlink | TreeLinkMode::Invalid => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackObjectLink {
    pub id: ObjectId,
    pub expected_kind: GitObjectKind,
}

#[derive(Debug)]
pub(crate) struct PackSemanticValidationError {
    reason: PackObjectValidationReason,
    source: io::Error,
}

impl PackSemanticValidationError {
    fn new(reason: PackObjectValidationReason, source: io::Error) -> Self {
        Self { reason, source }
    }

    pub(crate) fn reason(&self) -> PackObjectValidationReason {
        self.reason
    }

    pub(crate) fn into_source(self) -> io::Error {
        self.source
    }
}

impl std::fmt::Display for PackSemanticValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for PackSemanticValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
pub(crate) struct PackObjectValidationState {
    algorithm: GitHashAlgorithm,
    mode: PackValidationMode,
    received: HashMap<ObjectId, GitObjectKind>,
    links: Vec<PackObjectLink>,
    validated_objects: usize,
}

impl PackObjectValidationState {
    pub(crate) fn new(algorithm: GitHashAlgorithm, mode: PackValidationMode) -> Self {
        Self {
            algorithm,
            mode,
            received: HashMap::new(),
            links: Vec::new(),
            validated_objects: 0,
        }
    }

    pub(crate) fn validate_object(
        &mut self,
        id: &ObjectId,
        kind: GitObjectKind,
        content: &[u8],
    ) -> io::Result<()> {
        if id.algorithm() != self.algorithm {
            return invalid("received object uses the wrong hash algorithm");
        }

        let semantic_result = match kind {
            GitObjectKind::Blob => validate_blob(content),
            GitObjectKind::Commit => self.validate_commit(content),
            GitObjectKind::Tree => self.validate_tree(content),
            GitObjectKind::Tag => self.validate_tag(content),
        };
        if let Err(error) = semantic_result {
            if matches!(
                kind,
                GitObjectKind::Commit | GitObjectKind::Tree | GitObjectKind::Tag
            ) {
                if error
                    .get_ref()
                    .and_then(|source| source.downcast_ref::<PackSemanticValidationError>())
                    .is_some()
                {
                    return Err(error);
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    PackSemanticValidationError::new(
                        PackObjectValidationReason::OtherSemantic,
                        error,
                    ),
                ));
            }
            return Err(error);
        }

        if self.mode == PackValidationMode::Strict {
            if self.received.insert(id.clone(), kind).is_some() {
                return invalid("received PACK contains duplicate object IDs");
            }
        }
        self.validated_objects = self.validated_objects.saturating_add(1);
        Ok(())
    }

    pub(crate) fn finish(&self, external_store: &dyn crate::GitObjectStore) -> io::Result<()> {
        if self.mode != PackValidationMode::Strict {
            return Ok(());
        }
        for link in &self.links {
            match self.received.get(&link.id) {
                Some(kind) if *kind == link.expected_kind => continue,
                Some(_) => return invalid("strict PACK link has the wrong object kind"),
                None => {
                    let object = external_store.read_object(&link.id).map_err(|error| {
                        if error.kind() == io::ErrorKind::NotFound {
                            invalid_io("strict PACK link points to a missing object")
                        } else {
                            error
                        }
                    })?;
                    if object.id != link.id || object.kind != link.expected_kind {
                        return invalid("strict PACK link has the wrong external object");
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validated_objects(&self) -> usize {
        self.validated_objects
    }

    fn add_link(&mut self, id: ObjectId, expected_kind: GitObjectKind) {
        if self.mode == PackValidationMode::Strict {
            self.links.push(PackObjectLink { id, expected_kind });
        }
    }

    fn validate_commit(&mut self, content: &[u8]) -> io::Result<()> {
        let tree_entry_len = self.algorithm.digest_len() * 2 + b"tree ".len();
        // Git v2.55's parse_commit_buffer reports "bogus commit object" only
        // when this first tree header has the required shape and newline.
        if content.len() <= tree_entry_len + 1
            || !content.starts_with(b"tree ")
            || content.get(tree_entry_len) != Some(&b'\n')
        {
            return Err(semantic_invalid(
                PackObjectValidationReason::CommitTreeHeader,
                "commit tree header is malformed",
            ));
        }
        if content.contains(&0) {
            return invalid("commit contains a NUL byte");
        }
        let (headers, _) = split_headers(content, "commit")?;
        let mut lines = headers.split(|byte| *byte == b'\n').peekable();
        let tree_line = lines
            .next()
            .ok_or_else(|| invalid_io("commit is missing its tree line"))?;
        let tree_id = parse_prefixed_id(self.algorithm, tree_line, b"tree ", "commit tree")?;
        self.add_link(tree_id, GitObjectKind::Tree);

        while lines
            .peek()
            .is_some_and(|line| line.starts_with(b"parent "))
        {
            let line = lines.next().expect("peeked parent line");
            if let Some(value) = line.strip_prefix(b"parent ") {
                let parent = parse_object_id(self.algorithm, value, "commit parent")?;
                self.add_link(parent, GitObjectKind::Commit);
            }
        }

        let author = lines
            .next()
            .ok_or_else(|| invalid_io("commit is missing its author line"))?;
        let author_value = author
            .strip_prefix(b"author ")
            .ok_or_else(|| invalid_io("commit author line is malformed"))?;
        validate_identity(author_value, "commit author")?;

        let committer = lines
            .next()
            .ok_or_else(|| invalid_io("commit is missing its committer line"))?;
        let committer_value = committer
            .strip_prefix(b"committer ")
            .ok_or_else(|| invalid_io("commit committer line is malformed"))?;
        validate_identity(committer_value, "commit committer")?;
        // Git's fsck commit parser stops after the validated committer. The
        // remaining header/body bytes are intentionally left to the object
        // message parser; only NUL rejection above applies to them here.
        Ok(())
    }

    fn validate_tree(&mut self, content: &[u8]) -> io::Result<()> {
        let mut cursor = 0;
        let mut previous: Option<(TreeMode, &[u8])> = None;
        let mut candidates: Vec<&[u8]> = Vec::new();
        while cursor < content.len() {
            let mode_end = content[cursor..]
                .iter()
                .position(|byte| *byte == b' ')
                .map(|offset| cursor + offset)
                .ok_or_else(|| invalid_io("tree mode is missing its separator"))?;
            let mode_bytes = &content[cursor..mode_end];
            let parsed_mode = parse_tree_mode(mode_bytes)?;
            if self.mode == PackValidationMode::Strict
                && parsed_mode.link_mode == TreeLinkMode::Invalid
            {
                return invalid("tree mode has an unknown file type");
            }
            let mode = parsed_mode.ordering_mode;
            cursor = mode_end + 1;
            let name_end = content[cursor..]
                .iter()
                .position(|byte| *byte == 0)
                .map(|offset| cursor + offset)
                .ok_or_else(|| invalid_io("tree name is missing its terminator"))?;
            let name = &content[cursor..name_end];
            if name.len() > MAX_TREE_ENTRY_NAME_LEN {
                return invalid("tree pathname exceeds 4096 bytes");
            }
            validate_tree_name(name)?;
            cursor = name_end + 1;
            let digest_end = cursor
                .checked_add(self.algorithm.digest_len())
                .ok_or_else(|| invalid_io("tree object ID offset overflows"))?;
            if digest_end > content.len() {
                return invalid("tree object ID is truncated");
            }
            let id = ObjectId::new(self.algorithm, &content[cursor..digest_end]);
            if id.as_bytes().iter().all(|byte| *byte == 0) {
                return invalid("tree contains a null object ID");
            }
            cursor = digest_end;

            if let Some((previous_mode, previous_name)) = previous {
                if previous_name == name {
                    return invalid("tree contains duplicate file or directory names");
                }
                match compare_tree_entries(
                    previous_mode,
                    previous_name,
                    mode,
                    name,
                    &mut candidates,
                ) {
                    Ordering::Less => {}
                    Ordering::Equal => return invalid("tree contains duplicate entries"),
                    Ordering::Greater => return invalid("tree entries are not canonically sorted"),
                }
            }
            previous = Some((mode, name));
            let expected_kind = parsed_mode.expected_kind();
            if let Some(expected_kind) = expected_kind {
                self.add_link(id, expected_kind);
            }
        }
        Ok(())
    }

    fn validate_tag(&mut self, content: &[u8]) -> io::Result<()> {
        let (headers, _) = split_headers(content, "tag")?;
        if content.contains(&0) {
            return invalid("tag contains a NUL byte");
        }
        let mut lines = headers.split(|byte| *byte == b'\n');
        let object_line = lines
            .next()
            .ok_or_else(|| invalid_io("tag is missing its object line"))?;
        let target = parse_prefixed_id(self.algorithm, object_line, b"object ", "tag target")?;
        let type_line = lines
            .next()
            .ok_or_else(|| invalid_io("tag is missing its type line"))?;
        let target_kind = parse_tag_type(type_line)?;
        let name_line = lines
            .next()
            .ok_or_else(|| invalid_io("tag is missing its name line"))?;
        let name = name_line
            .strip_prefix(b"tag ")
            .ok_or_else(|| invalid_io("tag name line is malformed"))?;
        // BAD_TAG_NAME is INFO in Git's default fsck policy. Evaluate the
        // same refname predicate, but do not reject without a severity
        // override (severity plumbing is deliberately deferred).
        let _valid_tag_name = is_valid_tag_refname(name);

        let mut next = lines.next();
        if let Some(line) = next.take() {
            if line.starts_with(b"tagger ") {
                let tagger = line
                    .strip_prefix(b"tagger ")
                    .ok_or_else(|| invalid_io("tagger line is malformed"))?;
                validate_identity(tagger, "tagger")?;
                next = lines.next();
            } else if line == b"tagger" {
                // A tagger line without its required space is an ignored
                // extra header, while the missing-tagger finding remains
                // INFO under Git's default fsck policy.
                next = lines.next();
            } else {
                next = Some(line);
            }
        }
        if let Some(line) = next
            && (line.starts_with(b"gpgsig ") || line.starts_with(b"gpgsig-sha256 "))
        {
            for continuation in &mut lines {
                if !continuation.starts_with(b" ") {
                    // EXTRA_HEADER_ENTRY is IGNORE by default. The first
                    // non-continuation line starts that ignored region.
                    break;
                }
            }
        }
        self.add_link(target, target_kind);
        Ok(())
    }
}

fn validate_blob(_content: &[u8]) -> io::Result<()> {
    Ok(())
}

fn parse_tree_mode(mode: &[u8]) -> io::Result<ParsedTreeMode> {
    if mode.first() == Some(&b'0') {
        return Err(invalid_io("tree mode is zero-padded (expected 40000)"));
    }
    if mode.is_empty() || !mode.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
        return Err(invalid_io("tree mode is invalid"));
    }
    let raw_mode = mode.iter().try_fold(0_u16, |value, byte| {
        value
            .checked_mul(8)
            .and_then(|value| value.checked_add(u16::from(byte - b'0')))
    });
    let raw_mode = raw_mode.ok_or_else(|| invalid_io("tree mode is invalid"))?;
    let (ordering_mode, link_mode) = match raw_mode & 0o170000 {
        0o040000 => (TreeMode::Tree, TreeLinkMode::Tree),
        0o160000 => (TreeMode::File, TreeLinkMode::Gitlink),
        0o100000 | 0o120000 => (TreeMode::File, TreeLinkMode::Blob),
        _ => (TreeMode::File, TreeLinkMode::Invalid),
    };
    Ok(ParsedTreeMode {
        ordering_mode,
        link_mode,
    })
}

fn split_headers<'a>(content: &'a [u8], kind: &str) -> io::Result<(&'a [u8], &'a [u8])> {
    if let Some(separator) = content.windows(2).position(|window| window == b"\n\n") {
        return Ok((&content[..separator], &content[separator + 2..]));
    }
    if content.last() == Some(&b'\n') {
        return Ok((&content[..content.len() - 1], &[]));
    }
    Err(invalid_io(&format!("{kind} has an unterminated header")))
}

fn parse_prefixed_id(
    algorithm: GitHashAlgorithm,
    line: &[u8],
    prefix: &[u8],
    label: &str,
) -> io::Result<ObjectId> {
    let value = line
        .strip_prefix(prefix)
        .ok_or_else(|| invalid_io(&format!("{label} line is missing")))?;
    parse_object_id(algorithm, value, label)
}

fn parse_object_id(algorithm: GitHashAlgorithm, value: &[u8], label: &str) -> io::Result<ObjectId> {
    ObjectId::from_hex_bytes(algorithm, value)
        .map_err(|_| invalid_io(&format!("{label} object ID is invalid")))
}

fn parse_tag_type(line: &[u8]) -> io::Result<GitObjectKind> {
    let value = line
        .strip_prefix(b"type ")
        .ok_or_else(|| invalid_io("tag type line is missing"))?;
    GitObjectKind::parse(value).ok_or_else(|| invalid_io("tag type is invalid"))
}

fn validate_identity(value: &[u8], label: &str) -> io::Result<()> {
    let email_start = value
        .iter()
        .position(|byte| *byte == b'<')
        .ok_or_else(|| invalid_io(&format!("{label} is missing an email")))?;
    if email_start == 0
        || value[email_start - 1] != b' '
        || value[..email_start - 1].contains(&b'>')
    {
        return invalid("identity name or email separator is malformed");
    }
    let email_end = value[email_start + 1..]
        .iter()
        .position(|byte| *byte == b'>')
        .map(|offset| email_start + 1 + offset)
        .ok_or_else(|| invalid_io(&format!("{label} email is malformed")))?;
    if email_end == email_start + 1 || value[email_start + 1..email_end].contains(&b'<') {
        return invalid("identity email is malformed");
    }
    let after_email = email_end + 1;
    if value.get(after_email) != Some(&b' ') {
        return invalid("identity is missing the space before its date");
    }
    let mut cursor = after_email + 1;
    while value.get(cursor) == Some(&b' ') || value.get(cursor) == Some(&b'\t') {
        cursor += 1;
    }
    let date_start = cursor;
    while value.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == date_start {
        return invalid("identity date is malformed");
    }
    if value[date_start] == b'0' && cursor - date_start > 1 {
        return invalid("identity date is zero-padded");
    }
    let date = std::str::from_utf8(&value[date_start..cursor])
        .ok()
        .and_then(|date| date.parse::<i64>().ok());
    if date.is_none() {
        return invalid("identity date overflows its integer range");
    }
    if value.get(cursor) != Some(&b' ') {
        return invalid("identity timezone separator is missing");
    }
    let timezone = &value[cursor + 1..];
    if timezone.len() != 5
        || !matches!(timezone[0], b'+' | b'-')
        || !timezone[1..].iter().all(u8::is_ascii_digit)
    {
        return invalid("identity timezone is malformed");
    }
    Ok(())
}

fn validate_tree_name(name: &[u8]) -> io::Result<()> {
    if name.is_empty() {
        return invalid("tree contains an empty name");
    }
    if name == b"." || name == b".." || name == b".git" {
        return invalid("tree contains a forbidden name");
    }
    if name.contains(&0) || name.contains(&b'/') || is_git_alias(name) {
        return invalid("tree name contains a forbidden byte");
    }
    Ok(())
}

fn is_valid_tag_refname(name: &[u8]) -> bool {
    if name.is_empty()
        || name.ends_with(b"/")
        || name.ends_with(b".")
        || name.windows(2).any(|window| window == b"//")
    {
        return false;
    }
    if name.split(|byte| *byte == b'/').any(|part| {
        part.is_empty()
            || part == b"."
            || part == b".."
            || part.starts_with(b".")
            || part.ends_with(b".lock")
    }) {
        return false;
    }
    !name.windows(2).any(|window| window == b"..")
        && !name.contains(&b'\\')
        && !name.windows(2).any(|window| window == b"@{")
        && !name.iter().any(|byte| *byte < 0x20 || *byte == 0x7f)
        && !name
            .iter()
            .any(|byte| matches!(*byte, b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'['))
}

fn is_git_alias(name: &[u8]) -> bool {
    is_hfs_dotgit(name) || is_ntfs_dotgit(name)
}

fn is_hfs_dotgit(name: &[u8]) -> bool {
    let Ok(name) = std::str::from_utf8(name) else {
        return false;
    };
    let mut chars = name.chars();
    if chars.next() != Some('.') {
        return false;
    }
    if !matches!(next_hfs_char(&mut chars), Some(char) if char.eq_ignore_ascii_case(&'g'))
        || !matches!(next_hfs_char(&mut chars), Some(char) if char.eq_ignore_ascii_case(&'i'))
        || !matches!(next_hfs_char(&mut chars), Some(char) if char.eq_ignore_ascii_case(&'t'))
    {
        return false;
    }
    matches!(next_hfs_char(&mut chars), None | Some('/'))
}

fn next_hfs_char(chars: &mut std::str::Chars<'_>) -> Option<char> {
    while let Some(char) = chars.next() {
        if matches!(
            char,
            '\u{200c}'
                | '\u{200d}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'
                | '\u{202b}'
                | '\u{202c}'
                | '\u{202d}'
                | '\u{202e}'
                | '\u{206a}'
                | '\u{206b}'
                | '\u{206c}'
                | '\u{206d}'
                | '\u{206e}'
                | '\u{206f}'
                | '\u{feff}'
        ) {
            continue;
        }
        return Some(char);
    }
    None
}

fn is_ntfs_dotgit(name: &[u8]) -> bool {
    let prefix_len = if name.len() >= 4
        && name[0] == b'.'
        && ascii_eq_ignore_case(name[1], b'g')
        && ascii_eq_ignore_case(name[2], b'i')
        && ascii_eq_ignore_case(name[3], b't')
    {
        4
    } else if name.len() >= 5
        && ascii_eq_ignore_case(name[0], b'g')
        && ascii_eq_ignore_case(name[1], b'i')
        && ascii_eq_ignore_case(name[2], b't')
        && name[3] == b'~'
        && (b'1'..=b'4').contains(&name[4])
    {
        5
    } else {
        return false;
    };
    for byte in &name[prefix_len..] {
        if *byte == b':' || *byte == b'/' || *byte == b'\\' {
            return true;
        }
        if *byte != b'.' && *byte != b' ' {
            return false;
        }
    }
    true
}

fn ascii_eq_ignore_case(left: u8, right: u8) -> bool {
    left.eq_ignore_ascii_case(&right)
}

fn compare_tree_entries<'a>(
    previous_mode: TreeMode,
    previous_name: &'a [u8],
    mode: TreeMode,
    name: &[u8],
    candidates: &mut Vec<&'a [u8]>,
) -> Ordering {
    let common = previous_name
        .iter()
        .zip(name)
        .take_while(|(left, right)| left == right)
        .count();
    let mut left = previous_name.get(common).copied();
    let mut right = name.get(common).copied();
    if left.is_none() && previous_mode == TreeMode::Tree {
        left = Some(b'/');
    }
    if right.is_none() && mode == TreeMode::Tree {
        right = Some(b'/');
    }
    if left.is_none() && right.is_none() {
        return Ordering::Equal;
    }
    if left.is_none() && right.is_some_and(|byte| byte < b'/') {
        candidates.push(previous_name);
    } else if right == Some(b'/') && left.is_some_and(|byte| byte < b'/') {
        while let Some(candidate) = candidates.pop() {
            if name.starts_with(candidate) {
                let suffix = &name[candidate.len()..];
                if suffix.is_empty() || suffix[0] < b'/' {
                    return Ordering::Equal;
                }
                candidates.push(candidate);
                break;
            }
        }
    }
    left.cmp(&right)
}

fn invalid(message: &str) -> io::Result<()> {
    Err(invalid_io(message))
}

fn semantic_invalid(reason: PackObjectValidationReason, message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        PackSemanticValidationError::new(reason, invalid_io(message)),
    )
}

fn invalid_io(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}
