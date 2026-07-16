use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use crate::object::ObjectId;

#[derive(Debug, Clone, Default)]
pub struct GitAttributes {
    rules: Vec<AttributeRule>,
    macros: BTreeMap<String, Vec<AttributeAssignment>>,
    warnings: Vec<String>,
    ignore_case: bool,
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
    base: String,
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
        Self::parse_with_source(content, ".gitattributes")
    }

    pub fn parse_with_source(content: &str, source: &str) -> Self {
        Self::parse_with_base_and_source_and_case(content, "", source, false)
    }

    pub fn parse_with_base_and_source(content: &str, base: &str, source: &str) -> Self {
        Self::parse_with_base_and_source_and_case(content, base, source, false)
    }

    pub fn parse_with_base_and_source_and_case(
        content: &str,
        base: &str,
        source: &str,
        ignore_case: bool,
    ) -> Self {
        let mut rules = Vec::new();
        let mut macros = BTreeMap::new();
        let mut warnings = Vec::new();
        let base = base.trim_matches('/').replace('\\', "/");
        for (line_number, raw_line) in content.lines().enumerate() {
            if raw_line.len() >= 2048 {
                warnings.push(format!(
                    "warning: ignoring overly long attributes line {}",
                    line_number + 1
                ));
                continue;
            }
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let parts = split_attribute_tokens(line);
            let Some(pattern) = parts.first() else {
                continue;
            };
            if !pattern.starts_with("\\!") && pattern.starts_with('!') {
                warnings.push(
                    "Negative patterns are ignored in git attributes\nUse '\\!' for literal leading exclamation."
                        .to_owned(),
                );
                continue;
            }
            let pattern = unescape_attribute_pattern(pattern);
            let assignments = parts
                .iter()
                .skip(1)
                .filter_map(|value| parse_assignment(value, source, line_number + 1, &mut warnings))
                .collect::<Vec<_>>();
            if !assignments.is_empty() {
                if let Some(name) = pattern.strip_prefix("[attr]") {
                    if valid_attr_name(name) && !builtin_attr_name(name) {
                        macros.insert(name.to_owned(), assignments);
                    } else if !name.is_empty() {
                        warnings.push(format!(
                            "{name} is not a valid attribute name: {source}:{}",
                            line_number + 1
                        ));
                    }
                } else {
                    rules.push(AttributeRule {
                        base: base.clone(),
                        pattern: pattern.trim_start_matches('/').to_owned(),
                        assignments,
                    });
                }
            }
        }
        Self {
            rules,
            macros,
            warnings,
            ignore_case,
        }
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn set_ignore_case(&mut self, ignore_case: bool) {
        self.ignore_case = ignore_case;
    }

    pub fn append(&mut self, mut other: Self) {
        self.rules.append(&mut other.rules);
        self.warnings.append(&mut other.warnings);
        self.macros.append(&mut other.macros);
        self.ignore_case |= other.ignore_case;
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
        self.values_for_path_ordered(path)
            .into_iter()
            .filter(|(_, value)| value != &AttributeValue::Unspecified)
            .collect()
    }

    pub fn is_set(&self, path: &[u8], attr: &str) -> bool {
        self.values_for_path(path)
            .get(attr)
            .is_some_and(|value| value == &AttributeValue::Set)
    }

    fn values_for_path(&self, path: &[u8]) -> BTreeMap<String, AttributeValue> {
        self.values_for_path_ordered(path).into_iter().collect()
    }

    fn values_for_path_ordered(&self, path: &[u8]) -> Vec<(String, AttributeValue)> {
        let relative = String::from_utf8_lossy(path).replace('\\', "/");
        let mut values = BTreeMap::new();
        let mut order = Vec::new();
        for rule in &self.rules {
            let candidate = if rule.base.is_empty() {
                relative.as_str()
            } else if path_matches_rule_base(&rule.base, &relative, self.ignore_case) {
                path_strip_rule_base(&rule.base, &relative, self.ignore_case).unwrap_or_default()
            } else {
                continue;
            };
            let candidate_basename = candidate.rsplit('/').next().unwrap_or(candidate);
            if !attribute_pattern_matches(
                &rule.pattern,
                candidate,
                candidate_basename,
                self.ignore_case,
            ) {
                continue;
            }
            for assignment in &rule.assignments {
                apply_assignment(&mut values, &mut order, assignment, &self.macros);
            }
        }
        let mut rows = order
            .into_iter()
            .filter_map(|name| values.remove(&name).map(|value| (name, value)))
            .collect::<Vec<_>>();
        rows.sort_by_key(|(name, _)| check_attr_all_order_key(name));
        rows
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
    order: &mut Vec<String>,
    assignment: &AttributeAssignment,
    macros: &BTreeMap<String, Vec<AttributeAssignment>>,
) {
    let mut pending = vec![assignment.clone()];
    while let Some(current) = pending.pop() {
        if current.name == "binary" && current.value == AttributeValue::Set {
            for (name, value) in [
                ("binary", AttributeValue::Set),
                ("diff", AttributeValue::Unset),
                ("merge", AttributeValue::Unset),
                ("text", AttributeValue::Unset),
            ] {
                insert_attribute_value(values, order, name.to_owned(), value);
            }
            continue;
        }
        let macro_body = if current.value == AttributeValue::Set {
            macros.get(&current.name)
        } else {
            None
        };
        insert_attribute_value(values, order, current.name.clone(), current.value.clone());
        if let Some(body) = macro_body {
            for nested in body.iter().rev() {
                pending.push(nested.clone());
            }
        }
    }
}

fn insert_attribute_value(
    values: &mut BTreeMap<String, AttributeValue>,
    order: &mut Vec<String>,
    name: String,
    value: AttributeValue,
) {
    if !values.contains_key(&name) {
        order.push(name.clone());
    }
    values.insert(name, value);
}

fn check_attr_all_order_key(name: &str) -> usize {
    match name {
        "binary" => 0,
        "diff" => 1,
        "merge" => 2,
        "text" => 3,
        _ => 4,
    }
}

fn parse_assignment(
    value: &str,
    source: &str,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> Option<AttributeAssignment> {
    if let Some(name) = value.strip_prefix('-') {
        return validate_assignment_name(name, source, line_number, warnings).then(|| {
            AttributeAssignment {
                name: name.to_owned(),
                value: AttributeValue::Unset,
            }
        });
    }
    if let Some(name) = value.strip_prefix('!') {
        return validate_assignment_name(name, source, line_number, warnings).then(|| {
            AttributeAssignment {
                name: name.to_owned(),
                value: AttributeValue::Unspecified,
            }
        });
    }
    if let Some((name, attr_value)) = value.split_once('=') {
        return validate_assignment_name(name, source, line_number, warnings).then(|| {
            AttributeAssignment {
                name: name.to_owned(),
                value: AttributeValue::Value(attr_value.to_owned()),
            }
        });
    }
    validate_assignment_name(value, source, line_number, warnings).then(|| AttributeAssignment {
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

fn builtin_attr_name(name: &str) -> bool {
    name.starts_with("builtin_")
}

fn validate_assignment_name(
    name: &str,
    source: &str,
    line_number: usize,
    warnings: &mut Vec<String>,
) -> bool {
    let valid = valid_attr_name(name) && !builtin_attr_name(name);
    if !valid && !name.is_empty() {
        warnings.push(format!(
            "{name} is not a valid attribute name: {source}:{line_number}"
        ));
    }
    valid
}

fn attribute_pattern_matches(
    pattern: &str,
    relative: &str,
    basename: &str,
    ignore_case: bool,
) -> bool {
    if pattern.contains('/') {
        wildcard_match(pattern, relative, ignore_case)
    } else {
        wildcard_match(pattern, basename, ignore_case)
    }
}

fn wildcard_match(pattern: &str, text: &str, ignore_case: bool) -> bool {
    if let Some(rest) = pattern.strip_prefix("**/")
        && wildcard_match(rest, text, ignore_case)
    {
        return true;
    }
    let owned_pattern;
    let owned_text;
    let (pattern, text) = if ignore_case {
        owned_pattern = pattern.to_ascii_lowercase();
        owned_text = text.to_ascii_lowercase();
        (owned_pattern.as_bytes(), owned_text.as_bytes())
    } else {
        (pattern.as_bytes(), text.as_bytes())
    };
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

fn unescape_attribute_pattern(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn path_matches_rule_base(base: &str, relative: &str, ignore_case: bool) -> bool {
    if ignore_case {
        base.eq_ignore_ascii_case(relative)
            || relative.len() > base.len()
                && relative
                    .get(..base.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(base))
                && relative.as_bytes().get(base.len()) == Some(&b'/')
    } else {
        base == relative || relative.strip_prefix(&format!("{base}/")).is_some()
    }
}

fn path_strip_rule_base<'a>(base: &str, relative: &'a str, ignore_case: bool) -> Option<&'a str> {
    if ignore_case {
        if base.eq_ignore_ascii_case(relative) {
            return Some("");
        }
        if relative.len() > base.len()
            && relative
                .get(..base.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(base))
            && relative.as_bytes().get(base.len()) == Some(&b'/')
        {
            return relative.get(base.len() + 1..);
        }
        None
    } else if base == relative {
        Some("")
    } else {
        relative.strip_prefix(&format!("{base}/"))
    }
}

fn split_attribute_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if quoted || current.is_empty() {
                    quoted = !quoted;
                } else {
                    current.push(ch);
                }
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push('\\');
                    current.push(next);
                } else {
                    current.push(ch);
                }
            }
            ch if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                while chars.next_if(|next| next.is_whitespace()).is_some() {}
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
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
        assert_eq!(
            attrs.check_all(b"main.rs"),
            vec![
                ("diff".to_owned(), AttributeValue::Value("rust".to_owned())),
                ("text".to_owned(), AttributeValue::Set),
                ("custom".to_owned(), AttributeValue::Set),
            ]
        );
        assert_eq!(
            attrs.check_all(b"file.bin"),
            vec![
                ("binary".to_owned(), AttributeValue::Set),
                ("diff".to_owned(), AttributeValue::Unset),
                ("merge".to_owned(), AttributeValue::Unset),
                ("text".to_owned(), AttributeValue::Unset),
            ]
        );
    }

    #[test]
    fn expands_attribute_macros_iteratively() {
        let attrs = GitAttributes::parse("[attr]a0 a1\n[attr]a1 a2\n[attr]a2 -text\nfile a0\n");

        assert_eq!(
            attrs.check_all(b"file"),
            vec![
                ("text".to_owned(), AttributeValue::Unset),
                ("a0".to_owned(), AttributeValue::Set),
                ("a1".to_owned(), AttributeValue::Set),
                ("a2".to_owned(), AttributeValue::Set),
            ]
        );
    }

    #[test]
    fn reports_builtin_attribute_name_warnings() {
        let attrs = GitAttributes::parse("foo* builtin_foo builtin_objectmode=100644\n");

        assert_eq!(
            attrs.warnings(),
            &[
                "builtin_foo is not a valid attribute name: .gitattributes:1".to_owned(),
                "builtin_objectmode is not a valid attribute name: .gitattributes:1".to_owned(),
            ]
        );
        assert!(attrs.check_all(b"foo.txt").is_empty());
    }

    #[test]
    fn ignores_overly_long_attribute_lines_with_warning() {
        let attrs = GitAttributes::parse(&format!("path {:02043}\n", 1));

        assert_eq!(
            attrs.warnings(),
            &["warning: ignoring overly long attributes line 1".to_owned()]
        );
        assert!(attrs.check_all(b"path").is_empty());
    }

    #[test]
    fn parses_quoted_patterns_and_escaped_quotes() {
        let attrs = GitAttributes::parse("\" d \" test=d\n e\\\" test=e\n");

        assert_eq!(
            attrs.check(b" d ", &["test".to_owned()]),
            vec![("test".to_owned(), AttributeValue::Value("d".to_owned()))]
        );
        assert_eq!(
            attrs.check(b"e\"", &["test".to_owned()]),
            vec![("test".to_owned(), AttributeValue::Value("e".to_owned()))]
        );
        assert!(attrs.warnings().is_empty());
    }

    #[test]
    fn keeps_literal_quote_inside_unquoted_pattern() {
        let attrs = GitAttributes::parse(" e\" test=e\n");

        assert_eq!(
            attrs.check(b"e\"", &["test".to_owned()]),
            vec![("test".to_owned(), AttributeValue::Value("e".to_owned()))]
        );
        assert!(attrs.warnings().is_empty());
    }
}
