use std::fs;
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
    if pattern.contains('/') {
        wildcard_match_pathspec(&pattern, &path, options.icase, !options.glob_explicit)
    } else if options.glob_explicit {
        !path.contains('/') && wildcard_match_pathspec(&pattern, &path, options.icase, false)
    } else if pathspec
        .first()
        .is_some_and(|byte| matches!(*byte, b'*' | b'?' | b'['))
    {
        let basename = path.rsplit('/').next().unwrap_or(&path);
        wildcard_match_pathspec(&pattern, basename, options.icase, true)
    } else {
        !path.contains('/') && wildcard_match_pathspec(&pattern, &path, options.icase, true)
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
