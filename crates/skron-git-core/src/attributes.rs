use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

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
