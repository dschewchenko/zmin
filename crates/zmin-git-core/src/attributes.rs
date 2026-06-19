use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::object::ObjectId;

#[derive(Debug, Clone, Default)]
pub struct GitAttributes {
    rules: Vec<AttributeRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeValue {
    Set,
    Unset,
    Unspecified,
    Value(String),
}

impl AttributeValue {
    pub fn as_check_attr_value(&self) -> &str {
        match self {
            Self::Set => "set",
            Self::Unset => "unset",
            Self::Unspecified => "unspecified",
            Self::Value(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttributeRule {
    pattern: String,
    assignments: Vec<AttributeAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttributeAssignment {
    name: String,
    value: AttributeValue,
}

impl GitAttributes {
    pub fn load_from_root(root: &Path) -> io::Result<Self> {
        let path = root.join(".gitattributes");
        match fs::read_to_string(path) {
            Ok(content) => Ok(Self::parse(&content)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn parse(content: &str) -> Self {
        let mut rules = Vec::new();
        for line in content.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(pattern) = parts.next() else {
                continue;
            };
            let assignments = parts.filter_map(parse_assignment).collect::<Vec<_>>();
            if !assignments.is_empty() {
                rules.push(AttributeRule {
                    pattern: pattern.trim_start_matches('/').to_owned(),
                    assignments,
                });
            }
        }
        Self { rules }
    }

    pub fn check(&self, path: &[u8], attrs: &[String]) -> Vec<(String, AttributeValue)> {
        attrs
            .iter()
            .map(|attr| {
                (
                    attr.clone(),
                    self.values_for_path(path)
                        .remove(attr)
                        .unwrap_or(AttributeValue::Unspecified),
                )
            })
            .collect()
    }

    pub fn check_all(&self, path: &[u8]) -> Vec<(String, AttributeValue)> {
        let values = self.values_for_path(path);
        let names = values.keys().cloned().collect::<BTreeSet<_>>();
        names
            .iter()
            .filter_map(|name| {
                let value = values.get(name)?;
                if value == &AttributeValue::Unspecified {
                    None
                } else {
                    Some((name.clone(), value.clone()))
                }
            })
            .collect()
    }

    pub fn is_set(&self, path: &[u8], attr: &str) -> bool {
        self.values_for_path(path)
            .get(attr)
            .is_some_and(|value| value == &AttributeValue::Set)
    }

    fn values_for_path(&self, path: &[u8]) -> BTreeMap<String, AttributeValue> {
        let relative = String::from_utf8_lossy(path).replace('\\', "/");
        let basename = relative.rsplit('/').next().unwrap_or(&relative);
        let mut values = BTreeMap::new();
        for rule in &self.rules {
            if !attribute_pattern_matches(&rule.pattern, &relative, basename) {
                continue;
            }
            for assignment in &rule.assignments {
                apply_assignment(&mut values, assignment);
            }
        }
        values
    }
}

pub fn apply_ident_clean(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    let mut cursor = 0usize;
    while let Some(relative_start) = find_subslice(&content[cursor..], b"$Id:") {
        let start = cursor + relative_start;
        out.extend_from_slice(&content[cursor..start]);
        let search_start = start + b"$Id:".len();
        if let Some(relative_end) = find_ident_clean_terminator(&content[search_start..]) {
            let value = &content[search_start..search_start + relative_end];
            if ident_clean_value_is_collapsible(value) {
                out.extend_from_slice(b"$Id$");
            } else {
                out.extend_from_slice(&content[start..search_start + relative_end + 1]);
            }
            cursor = search_start + relative_end + 1;
        } else {
            out.extend_from_slice(&content[start..search_start]);
            cursor = search_start;
        }
    }
    out.extend_from_slice(&content[cursor..]);
    out
}

fn find_ident_clean_terminator(content: &[u8]) -> Option<usize> {
    for (idx, byte) in content.iter().enumerate() {
        match *byte {
            b'$' => return Some(idx),
            b'\n' | b'\r' => return None,
            _ => {}
        }
    }
    None
}

fn ident_clean_value_is_collapsible(value: &[u8]) -> bool {
    let value = trim_ascii_whitespace(value);
    value.iter().all(|byte| !byte.is_ascii_whitespace())
}

fn trim_ascii_whitespace(mut value: &[u8]) -> &[u8] {
    while let Some((first, rest)) = value.split_first()
        && first.is_ascii_whitespace()
    {
        value = rest;
    }
    while let Some((last, rest)) = value.split_last()
        && last.is_ascii_whitespace()
    {
        value = rest;
    }
    value
}

pub fn apply_ident_smudge(content: &[u8], id: &ObjectId) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + id.hex_len() + 3);
    let mut cursor = 0usize;
    while let Some(relative_start) = find_subslice(&content[cursor..], b"$Id") {
        let start = cursor + relative_start;
        out.extend_from_slice(&content[cursor..start]);
        match content.get(start + b"$Id".len()).copied() {
            Some(b'$') => {
                append_ident_smudge_marker(&mut out, id);
                cursor = start + b"$Id$".len();
            }
            Some(b':') => {
                let search_start = start + b"$Id:".len();
                if let Some(relative_end) = find_ident_clean_terminator(&content[search_start..]) {
                    let value = &content[search_start..search_start + relative_end];
                    if ident_clean_value_is_collapsible(value) {
                        append_ident_smudge_marker(&mut out, id);
                    } else {
                        out.extend_from_slice(&content[start..search_start + relative_end + 1]);
                    }
                    cursor = search_start + relative_end + 1;
                } else {
                    out.extend_from_slice(&content[start..search_start]);
                    cursor = search_start;
                }
            }
            _ => {
                out.extend_from_slice(b"$Id");
                cursor = start + b"$Id".len();
            }
        }
    }
    out.extend_from_slice(&content[cursor..]);
    out
}

pub fn apply_eol_clean_to_lf(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    let mut cursor = 0usize;
    while let Some(relative) = find_subslice(&content[cursor..], b"\r\n") {
        let start = cursor + relative;
        out.extend_from_slice(&content[cursor..start]);
        out.push(b'\n');
        cursor = start + 2;
    }
    out.extend_from_slice(&content[cursor..]);
    out
}

pub fn apply_eol_smudge_to_crlf(content: &[u8]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(content.len() + content.iter().filter(|byte| **byte == b'\n').count());
    let mut previous = None;
    for byte in content {
        if *byte == b'\n' && previous != Some(b'\r') {
            out.push(b'\r');
        }
        out.push(*byte);
        previous = Some(*byte);
    }
    out
}

fn append_ident_smudge_marker(out: &mut Vec<u8>, id: &ObjectId) {
    out.extend_from_slice(b"$Id: ");
    out.extend_from_slice(id.to_hex().as_bytes());
    out.extend_from_slice(b" $");
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn apply_assignment(
    values: &mut BTreeMap<String, AttributeValue>,
    assignment: &AttributeAssignment,
) {
    if assignment.name == "binary" && assignment.value == AttributeValue::Set {
        values.insert("binary".to_owned(), AttributeValue::Set);
        values.insert("diff".to_owned(), AttributeValue::Unset);
        values.insert("merge".to_owned(), AttributeValue::Unset);
        values.insert("text".to_owned(), AttributeValue::Unset);
        return;
    }
    values.insert(assignment.name.clone(), assignment.value.clone());
}

fn parse_assignment(value: &str) -> Option<AttributeAssignment> {
    if let Some(name) = value.strip_prefix('-') {
        return valid_attr_name(name).then(|| AttributeAssignment {
            name: name.to_owned(),
            value: AttributeValue::Unset,
        });
    }
    if let Some(name) = value.strip_prefix('!') {
        return valid_attr_name(name).then(|| AttributeAssignment {
            name: name.to_owned(),
            value: AttributeValue::Unspecified,
        });
    }
    if let Some((name, attr_value)) = value.split_once('=') {
        return valid_attr_name(name).then(|| AttributeAssignment {
            name: name.to_owned(),
            value: AttributeValue::Value(attr_value.to_owned()),
        });
    }
    valid_attr_name(value).then(|| AttributeAssignment {
        name: value.to_owned(),
        value: AttributeValue::Set,
    })
}

fn valid_attr_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn attribute_pattern_matches(pattern: &str, relative: &str, basename: &str) -> bool {
    if pattern.contains('/') {
        wildcard_match(pattern, relative)
    } else {
        wildcard_match(pattern, basename)
    }
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut pattern_idx, mut text_idx) = (0, 0);
    let mut star_idx = None;
    let mut star_text_idx = 0;

    while text_idx < text.len() {
        if pattern_idx < pattern.len()
            && (pattern[pattern_idx] == b'?' || pattern[pattern_idx] == text[text_idx])
        {
            pattern_idx += 1;
            text_idx += 1;
        } else if pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
            star_idx = Some(pattern_idx);
            star_text_idx = text_idx;
            pattern_idx += 1;
        } else if let Some(star) = star_idx {
            pattern_idx = star + 1;
            star_text_idx += 1;
            text_idx = star_text_idx;
        } else {
            return false;
        }
    }

    while pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
        pattern_idx += 1;
    }
    pattern_idx == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_attribute_assignments() {
        let attrs = GitAttributes::parse(
            "*.rs text diff=rust custom\n*.bin -text binary\n/docs/** linguist-documentation\n*.md !diff\n",
        );

        assert_eq!(
            attrs.check(
                b"main.rs",
                &["text".to_owned(), "diff".to_owned(), "custom".to_owned()]
            ),
            vec![
                ("text".to_owned(), AttributeValue::Set),
                ("diff".to_owned(), AttributeValue::Value("rust".to_owned())),
                ("custom".to_owned(), AttributeValue::Set),
            ]
        );
        assert_eq!(
            attrs.check(b"file.bin", &["text".to_owned(), "binary".to_owned()]),
            vec![
                ("text".to_owned(), AttributeValue::Unset),
                ("binary".to_owned(), AttributeValue::Set),
            ]
        );
        assert_eq!(
            attrs.check(b"file.bin", &["diff".to_owned(), "merge".to_owned()]),
            vec![
                ("diff".to_owned(), AttributeValue::Unset),
                ("merge".to_owned(), AttributeValue::Unset),
            ]
        );
        assert_eq!(
            attrs.check(b"docs/a.md", &["linguist-documentation".to_owned()]),
            vec![("linguist-documentation".to_owned(), AttributeValue::Set)]
        );
        assert_eq!(
            attrs.check(b"readme.md", &["diff".to_owned()]),
            vec![("diff".to_owned(), AttributeValue::Unspecified)]
        );
    }
}
