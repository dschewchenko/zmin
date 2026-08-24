use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::{CliError, GitIndex, GitRepo, Result, matching_index_entries, path_exists};

static GLOBAL_PATHSPEC_OPTIONS: OnceLock<PathspecOptions> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub(crate) struct PathspecOptions {
    pub(crate) glob: bool,
    pub(crate) glob_explicit: bool,
    pub(crate) literal: bool,
    pub(crate) icase: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PathspecRule<'a> {
    pub(crate) pattern: &'a [u8],
    pub(crate) exclude: bool,
    pub(crate) options: PathspecOptions,
}

#[derive(Debug, Clone)]
pub(crate) struct HistoryPathQuery {
    rules: Vec<HistoryPathRule>,
    literal_rule_buckets: Vec<Vec<usize>>,
    fallback_rule_indices: Vec<usize>,
    has_positive_rule: bool,
    fingerprint: u64,
}

#[derive(Debug, Clone)]
struct HistoryPathRule {
    pattern: Vec<u8>,
    exclude: bool,
    options: PathspecOptions,
}

impl HistoryPathQuery {
    pub(crate) fn compile(pathspecs: &[Vec<u8>]) -> Self {
        let rules = pathspecs
            .iter()
            .map(|raw| {
                let parsed = parse_pathspec_rule(raw);
                HistoryPathRule {
                    pattern: parsed.pattern.to_vec(),
                    exclude: parsed.exclude,
                    options: parsed.options,
                }
            })
            .collect::<Vec<_>>();
        let mut literal_rule_buckets = (0..=u8::MAX).map(|_| Vec::new()).collect::<Vec<_>>();
        let mut fallback_rule_indices = Vec::new();
        let mut has_positive_rule = false;
        for (index, rule) in rules.iter().enumerate() {
            has_positive_rule |= !rule.exclude;
            let is_literal = !rule.options.glob
                && !rule.options.glob_explicit
                && !rule.options.icase
                && !rule.pattern.is_empty()
                && !rule
                    .pattern
                    .iter()
                    .any(|byte| matches!(*byte, b'*' | b'?' | b'['));
            if let Some(&first) = rule.pattern.first().filter(|_| is_literal) {
                literal_rule_buckets[usize::from(first)].push(index);
            } else {
                fallback_rule_indices.push(index);
            }
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for rule in &rules {
            rule.pattern.hash(&mut hasher);
            rule.exclude.hash(&mut hasher);
            rule.options.glob.hash(&mut hasher);
            rule.options.glob_explicit.hash(&mut hasher);
            rule.options.literal.hash(&mut hasher);
            rule.options.icase.hash(&mut hasher);
        }
        Self {
            rules,
            literal_rule_buckets,
            fallback_rule_indices,
            has_positive_rule,
            fingerprint: hasher.finish(),
        }
    }

    pub(crate) fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    pub(crate) fn matches_path(&self, path: &[u8]) -> bool {
        if self.rules.is_empty() {
            return true;
        }
        let mut matched_positive = false;
        let mut excluded = false;
        let fallback = self.fallback_rule_indices.iter().copied();
        let bucket = path
            .first()
            .map(|first| {
                self.literal_rule_buckets[usize::from(*first)]
                    .iter()
                    .copied()
            })
            .into_iter()
            .flatten();
        for rule_index in fallback.chain(bucket) {
            let rule = &self.rules[rule_index];
            if rule.pattern.is_empty() {
                if rule.exclude {
                    excluded = true;
                } else {
                    matched_positive = true;
                }
                continue;
            }
            let matches = history_path_rule_matches(path, rule);
            if rule.exclude {
                excluded |= matches;
            } else {
                matched_positive |= matches;
            }
        }
        (matched_positive || !self.has_positive_rule) && !excluded
    }

    pub(crate) fn may_match_prefix(&self, prefix: &[u8]) -> bool {
        if prefix.is_empty() || self.rules.is_empty() {
            return true;
        }
        if !self.has_positive_rule {
            return true;
        }
        let prefix = prefix.strip_suffix(b"/").unwrap_or(prefix);
        let fallback = self.fallback_rule_indices.iter().copied();
        let bucket = prefix
            .first()
            .map(|first| {
                self.literal_rule_buckets[usize::from(*first)]
                    .iter()
                    .copied()
            })
            .into_iter()
            .flatten();
        fallback.chain(bucket).any(|rule_index| {
            let rule = &self.rules[rule_index];
            if rule.exclude || rule.pattern.is_empty() {
                return false;
            }
            if rule
                .pattern
                .iter()
                .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
            {
                return true;
            }
            bytes_eq(&rule.pattern, prefix, rule.options.icase)
                || bytes_starts_with_path_separator(&rule.pattern, prefix, rule.options.icase)
                || bytes_starts_with_path_separator(prefix, &rule.pattern, rule.options.icase)
        })
    }
}

fn bytes_starts_with_path_separator(value: &[u8], prefix: &[u8], icase: bool) -> bool {
    value
        .get(..prefix.len().saturating_add(1))
        .is_some_and(|start| {
            start.last() == Some(&b'/') && bytes_eq(&start[..prefix.len()], prefix, icase)
        })
}

fn history_path_rule_matches(path: &[u8], rule: &HistoryPathRule) -> bool {
    if history_path_exact_or_prefix_matches(path, &rule.pattern, rule.options.icase) {
        return true;
    }
    if !rule.options.glob
        || !rule
            .pattern
            .iter()
            .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
    {
        return false;
    }
    let wildcard_matches_slash = if !rule.options.glob_explicit {
        true
    } else if rule.pattern.contains(&b'/') {
        false
    } else {
        !path.contains(&b'/')
    };
    wildcard_match_bytes_without_allocation(
        &rule.pattern,
        path,
        rule.options.icase,
        wildcard_matches_slash,
    )
}

fn history_path_exact_or_prefix_matches(path: &[u8], pattern: &[u8], icase: bool) -> bool {
    bytes_eq(path, pattern, icase)
        || path
            .get(..pattern.len())
            .is_some_and(|prefix| bytes_eq(prefix, pattern, icase))
            && path.get(pattern.len()) == Some(&b'/')
}

impl Default for PathspecOptions {
    fn default() -> Self {
        Self {
            glob: true,
            glob_explicit: false,
            literal: false,
            icase: false,
        }
    }
}

pub(crate) fn set_global_pathspec_options(options: PathspecOptions) {
    let _ = GLOBAL_PATHSPEC_OPTIONS.set(options);
}

pub(crate) fn pathspec_matches(path: &[u8], pathspecs: &[Vec<u8>]) -> bool {
    if pathspecs.is_empty() {
        return true;
    }
    let mut has_positive = false;
    let mut matched_positive = false;
    let mut excluded = false;
    for raw in pathspecs {
        let rule = parse_pathspec_rule(raw);
        if rule.pattern.is_empty() {
            if rule.exclude {
                excluded = true;
            } else {
                has_positive = true;
                matched_positive = true;
            }
            continue;
        }
        let matches = pathspec_rule_matches(path, rule);
        if rule.exclude {
            excluded |= matches;
        } else {
            has_positive = true;
            matched_positive |= matches;
        }
    }
    (matched_positive || !has_positive) && !excluded
}

pub(crate) fn parse_pathspec_rule(raw: &[u8]) -> PathspecRule<'_> {
    let mut options = GLOBAL_PATHSPEC_OPTIONS.get().copied().unwrap_or_default();
    let mut exclude = false;
    let mut pattern = raw;

    if let Some(rest) = raw.strip_prefix(b":!") {
        exclude = true;
        pattern = rest;
    } else if let Some(rest) = raw.strip_prefix(b":^") {
        exclude = true;
        pattern = rest;
    } else if let Some(rest) = raw.strip_prefix(b":/") {
        pattern = rest;
    } else if let Some(rest) = raw.strip_prefix(b":(")
        && let Some(close) = rest.iter().position(|byte| *byte == b')')
    {
        let magic = &rest[..close];
        pattern = &rest[close + 1..];
        for token in magic.split(|byte| *byte == b',') {
            match token {
                b"exclude" | b"!" | b"^" => exclude = true,
                b"literal" => {
                    options.literal = true;
                    options.glob = false;
                    options.glob_explicit = false;
                }
                b"glob" => {
                    options.literal = false;
                    options.glob = true;
                    options.glob_explicit = true;
                }
                b"icase" => options.icase = true,
                b"top" => {}
                _ => {}
            }
        }
    }

    PathspecRule {
        pattern,
        exclude,
        options,
    }
}

pub(crate) fn pathspec_rule_matches(path: &[u8], rule: PathspecRule<'_>) -> bool {
    if pathspec_exact_or_prefix_matches(path, rule.pattern, rule.options.icase) {
        return true;
    }
    rule.options.glob && pathspec_glob_matches(path, rule.pattern, rule.options)
}

fn pathspec_exact_or_prefix_matches(path: &[u8], pathspec: &[u8], icase: bool) -> bool {
    if bytes_eq(path, pathspec, icase) {
        return true;
    }
    let mut prefix = pathspec.to_vec();
    prefix.push(b'/');
    bytes_starts_with(path, &prefix, icase)
}

fn pathspec_glob_matches(path: &[u8], pathspec: &[u8], options: PathspecOptions) -> bool {
    if !pathspec
        .iter()
        .any(|byte| matches!(*byte, b'*' | b'?' | b'['))
    {
        return false;
    }
    let path = String::from_utf8_lossy(path).replace('\\', "/");
    let pattern = String::from_utf8_lossy(pathspec).replace('\\', "/");
    if !options.glob_explicit {
        wildcard_match_pathspec(&pattern, &path, options.icase, true)
    } else if pattern.contains('/') {
        wildcard_match_pathspec(&pattern, &path, options.icase, !options.glob_explicit)
    } else if options.glob_explicit {
        !path.contains('/') && wildcard_match_pathspec(&pattern, &path, options.icase, false)
    } else {
        false
    }
}

pub(crate) fn bytes_eq(left: &[u8], right: &[u8], icase: bool) -> bool {
    if icase {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

pub(crate) fn bytes_starts_with(value: &[u8], prefix: &[u8], icase: bool) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|start| bytes_eq(start, prefix, icase))
}

pub(crate) fn wildcard_match_pathspec(
    pattern: &str,
    text: &str,
    icase: bool,
    wildcard_matches_slash: bool,
) -> bool {
    let pattern = if icase {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_owned()
    };
    let text = if icase {
        text.to_ascii_lowercase()
    } else {
        text.to_owned()
    };
    wildcard_match_bytes_with_slash(pattern.as_bytes(), text.as_bytes(), wildcard_matches_slash)
}

pub(crate) fn wildcard_match(pattern: &str, value: &str) -> bool {
    wildcard_match_bytes(pattern.as_bytes(), value.as_bytes())
}

fn wildcard_match_bytes(pattern: &[u8], value: &[u8]) -> bool {
    wildcard_match_bytes_with_slash(pattern, value, true)
}

fn wildcard_match_bytes_without_allocation(
    pattern: &[u8],
    value: &[u8],
    icase: bool,
    wildcard_matches_slash: bool,
) -> bool {
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_value_index = 0;
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && pattern_byte_matches(
                pattern,
                value,
                pattern_index,
                value_index,
                icase,
                wildcard_matches_slash,
            )
        {
            let consumed = if pattern[pattern_index] == b'[' {
                wildcard_class_end(&pattern[pattern_index + 1..]).unwrap_or(0) + 2
            } else {
                1
            };
            pattern_index += consumed;
            value_index += 1;
            continue;
        }
        if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            star_value_index = value_index;
            pattern_index += 1;
            continue;
        }
        if let Some(star_index) = star_index {
            if star_value_index < value.len()
                && (wildcard_matches_slash || value[star_value_index] != b'/')
            {
                star_value_index += 1;
                value_index = star_value_index;
                pattern_index = star_index + 1;
                continue;
            }
        }
        return false;
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn pattern_byte_matches(
    pattern: &[u8],
    value: &[u8],
    pattern_index: usize,
    value_index: usize,
    icase: bool,
    wildcard_matches_slash: bool,
) -> bool {
    let pattern_byte = pattern[pattern_index];
    let value_byte = value[value_index];
    match pattern_byte {
        b'?' => wildcard_matches_slash || value_byte != b'/',
        b'[' => {
            wildcard_class_matches(&pattern[pattern_index + 1..], Some(&value_byte))
                .is_some_and(|(_, matched)| matched)
                && (wildcard_matches_slash || value_byte != b'/')
        }
        b'*' => false,
        literal => bytes_eq(&[literal], &[value_byte], icase),
    }
}

fn wildcard_class_end(class: &[u8]) -> Option<usize> {
    class.iter().position(|byte| *byte == b']')
}

fn wildcard_match_bytes_with_slash(
    pattern: &[u8],
    value: &[u8],
    wildcard_matches_slash: bool,
) -> bool {
    let mut memo = vec![None; (pattern.len() + 1) * (value.len() + 1)];
    wildcard_match_memo(pattern, value, wildcard_matches_slash, 0, 0, &mut memo)
}

fn wildcard_match_memo(
    pattern: &[u8],
    value: &[u8],
    wildcard_matches_slash: bool,
    pattern_index: usize,
    value_index: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let width = value.len() + 1;
    let memo_index = pattern_index * width + value_index;
    if let Some(result) = memo[memo_index] {
        return result;
    }
    let result = if pattern_index == pattern.len() {
        value_index == value.len()
    } else {
        match pattern[pattern_index] {
            b'*' => {
                wildcard_match_memo(
                    pattern,
                    value,
                    wildcard_matches_slash,
                    pattern_index + 1,
                    value_index,
                    memo,
                ) || (value_index < value.len()
                    && (wildcard_matches_slash || value[value_index] != b'/')
                    && wildcard_match_memo(
                        pattern,
                        value,
                        wildcard_matches_slash,
                        pattern_index,
                        value_index + 1,
                        memo,
                    ))
            }
            b'?' => {
                value_index < value.len()
                    && (wildcard_matches_slash || value[value_index] != b'/')
                    && wildcard_match_memo(
                        pattern,
                        value,
                        wildcard_matches_slash,
                        pattern_index + 1,
                        value_index + 1,
                        memo,
                    )
            }
            b'[' => {
                if let Some((class_end, matched)) =
                    wildcard_class_matches(&pattern[pattern_index + 1..], value.get(value_index))
                {
                    matched
                        && (wildcard_matches_slash || value[value_index] != b'/')
                        && wildcard_match_memo(
                            pattern,
                            value,
                            wildcard_matches_slash,
                            pattern_index + class_end + 2,
                            value_index + 1,
                            memo,
                        )
                } else {
                    value.get(value_index) == Some(&b'[')
                        && wildcard_match_memo(
                            pattern,
                            value,
                            wildcard_matches_slash,
                            pattern_index + 1,
                            value_index + 1,
                            memo,
                        )
                }
            }
            literal => {
                value.get(value_index) == Some(&literal)
                    && wildcard_match_memo(
                        pattern,
                        value,
                        wildcard_matches_slash,
                        pattern_index + 1,
                        value_index + 1,
                        memo,
                    )
            }
        }
    };
    memo[memo_index] = Some(result);
    result
}

fn wildcard_class_matches(class: &[u8], value: Option<&u8>) -> Option<(usize, bool)> {
    let value = *value?;
    let mut index = 0;
    let negated = matches!(class.first(), Some(b'!' | b'^'));
    if negated {
        index += 1;
    }
    let mut matched = false;
    let mut previous = None;
    while index < class.len() {
        let byte = class[index];
        if byte == b']' && previous.is_some() {
            return Some((index, if negated { !matched } else { matched }));
        }
        if byte == b'-'
            && let Some(start) = previous
            && let Some(end) = class.get(index + 1).copied()
            && end != b']'
        {
            if start <= value && value <= end {
                matched = true;
            }
            previous = Some(end);
            index += 2;
            continue;
        }
        if byte == value {
            matched = true;
        }
        previous = Some(byte);
        index += 1;
    }
    None
}
pub(crate) fn read_pathspec_file(path: &Path, nul: bool) -> Result<Vec<PathBuf>> {
    let content = if path == Path::new("-") {
        let mut content = Vec::new();
        io::stdin().read_to_end(&mut content)?;
        content
    } else {
        fs::read(path).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                CliError::Fatal {
                    code: 128,
                    message: format!(
                        "could not open '{}' for reading: No such file or directory",
                        path.display()
                    ),
                }
            } else {
                CliError::Io(error)
            }
        })?
    };
    let parts = if nul {
        content
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| PathBuf::from(String::from_utf8_lossy(part).into_owned()))
            .collect()
    } else {
        String::from_utf8_lossy(&content)
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect()
    };
    Ok(parts)
}

pub(crate) fn ensure_add_pathspecs_match(
    repo: &GitRepo,
    index: &GitIndex,
    pathspecs: &[Vec<u8>],
) -> Result<()> {
    for pathspec in pathspecs {
        if pathspec.is_empty() {
            continue;
        }
        let index_matches = matching_index_entries(index, pathspec);
        let absolute = repo.root.join(String::from_utf8_lossy(pathspec).as_ref());
        if index_matches.is_empty() && !path_exists(&absolute) {
            return Err(CliError::Fatal {
                code: 128,
                message: format!(
                    "pathspec '{}' did not match any files",
                    String::from_utf8_lossy(pathspec)
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_glob_matches_directory_and_its_descendants() {
        let rule = parse_pathspec_rule(b"untracked_*");

        assert!(pathspec_rule_matches(b"untracked_dir", rule));
        assert!(pathspec_rule_matches(b"untracked_dir/file", rule));
    }

    #[test]
    fn history_query_matches_raw_bytes_and_nested_prefixes() {
        let query = HistoryPathQuery::compile(&[b"dir/\x80name".to_vec()]);

        assert!(query.matches_path(b"dir/\x80name"));
        assert!(query.matches_path(b"dir/\x80name/child"));
        assert!(!query.matches_path("dir/�name".as_bytes()));
        assert!(query.may_match_prefix(b"dir/"));

        let glob = HistoryPathQuery::compile(&[b"dir/*\x80*".to_vec()]);
        assert!(glob.matches_path(b"dir/a\x80b"));
        assert!(!glob.matches_path(b"dir/ab"));
    }

    #[test]
    fn history_query_literal_bucket_keeps_global_positive_rule_state() {
        let query = HistoryPathQuery::compile(&[b":(literal)path.txt".to_vec()]);

        assert!(query.matches_path(b"path.txt"));
        assert!(!query.matches_path(b"noise.txt"));
    }
}
