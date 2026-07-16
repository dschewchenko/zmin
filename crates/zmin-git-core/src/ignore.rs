use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct GitIgnore {
    patterns: Vec<IgnorePatternEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIgnoreMatch {
    pub source: String,
    pub line_number: usize,
    pub pattern: String,
    pub is_negation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IgnorePatternEntry {
    base: String,
    source: String,
    line_number: usize,
    pattern: String,
    is_negation: bool,
    kind: IgnorePattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IgnorePattern {
    Component(String),
    Directory(String),
    DirectoryGlob(String),
    DirectoryPath(String),
    DirectoryPathGlob(String),
    Glob(String),
    PathGlob(String),
    Path(String),
}

impl GitIgnore {
    pub fn load_from_root(root: &Path) -> io::Result<Self> {
        let mut ignore = Self::default();
        ignore.load_from_dir(root, root)?;
        Ok(ignore)
    }

    pub fn load_from_root_ignore_errors(root: &Path) -> io::Result<Self> {
        let mut ignore = Self::default();
        ignore.load_from_dir_with_options(root, root, true)?;
        Ok(ignore)
    }

    fn load_from_dir(&mut self, root: &Path, dir: &Path) -> io::Result<()> {
        self.load_from_dir_with_options(root, dir, false)
    }

    fn load_from_dir_with_options(
        &mut self,
        root: &Path,
        dir: &Path,
        ignore_errors: bool,
    ) -> io::Result<()> {
        let ignore_path = dir.join(".gitignore");
        if ignore_path.exists() {
            let content = fs::read_to_string(&ignore_path)?;
            let base = ignore_base(root, dir);
            self.append(Self::parse_with_base_and_source(
                &content,
                &base,
                &ignore_path.to_string_lossy(),
            ));
        }
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if ignore_errors => {
                let _ = error;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if ignore_errors => {
                    let _ = error;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if entry.file_name() == ".git" {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if ignore_errors => {
                    let _ = error;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if metadata.is_dir() {
                self.load_from_dir_with_options(root, &entry.path(), ignore_errors)?;
            }
        }
        Ok(())
    }

    pub fn parse(content: &str) -> Self {
        Self::parse_with_base(content, "")
    }

    pub fn parse_with_base(content: &str, base: &str) -> Self {
        Self::parse_with_base_and_source(content, base, ".gitignore")
    }

    pub fn parse_with_base_and_source(content: &str, base: &str, source: &str) -> Self {
        let mut patterns = Vec::new();
        let base = base.trim_matches('/').replace('\\', "/");
        for (line_number, raw_line) in content.lines().enumerate() {
            let Some((display_pattern, line)) = parse_ignore_line(raw_line) else {
                continue;
            };
            let (is_negation, line_pattern) = line
                .strip_prefix('!')
                .map_or((false, line.as_str()), |pattern| (true, pattern));
            let anchored = line_pattern.starts_with('/');
            let pattern = line_pattern.trim_start_matches('/').to_owned();
            if pattern.is_empty() {
                continue;
            }
            let kind = if let Some(directory) = pattern.strip_suffix('/') {
                let has_wildcard =
                    directory.contains('*') || directory.contains('?') || directory.contains('[');
                if has_wildcard && (anchored || directory.contains('/')) {
                    IgnorePattern::DirectoryPathGlob(directory.to_owned())
                } else if has_wildcard {
                    IgnorePattern::DirectoryGlob(directory.to_owned())
                } else if anchored || directory.contains('/') {
                    IgnorePattern::DirectoryPath(directory.to_owned())
                } else {
                    IgnorePattern::Directory(directory.to_owned())
                }
            } else if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
                if anchored || pattern.contains('/') {
                    IgnorePattern::PathGlob(pattern.clone())
                } else {
                    IgnorePattern::Glob(pattern.clone())
                }
            } else if anchored || pattern.contains('/') {
                IgnorePattern::Path(pattern.clone())
            } else {
                IgnorePattern::Component(pattern.clone())
            };
            patterns.push(IgnorePatternEntry {
                base: base.clone(),
                source: source.to_owned(),
                line_number: line_number + 1,
                pattern: display_pattern,
                is_negation,
                kind,
            });
        }
        Self { patterns }
    }

    pub fn append(&mut self, mut other: Self) {
        self.patterns.append(&mut other.patterns);
    }

    pub fn is_ignored(&self, path: &[u8], is_dir: bool) -> bool {
        self.match_path(path, is_dir)
            .is_some_and(|ignore_match| !ignore_match.is_negation)
    }

    pub fn match_path(&self, path: &[u8], is_dir: bool) -> Option<GitIgnoreMatch> {
        let relative = String::from_utf8_lossy(path);
        let mut matched = None;
        for entry in &self.patterns {
            let candidate = if entry.base.is_empty() {
                relative.as_ref()
            } else if relative == entry.base {
                ""
            } else {
                match relative.strip_prefix(&format!("{}/", entry.base)) {
                    Some(candidate) => candidate,
                    None => continue,
                }
            };
            let basename = candidate.rsplit('/').next().unwrap_or(candidate);
            let negated_directory_pattern_on_file = entry.is_negation
                && !is_dir
                && matches!(
                    &entry.kind,
                    IgnorePattern::Directory(_)
                        | IgnorePattern::DirectoryGlob(_)
                        | IgnorePattern::DirectoryPath(_)
                        | IgnorePattern::DirectoryPathGlob(_)
                );
            let matches = match &entry.kind {
                IgnorePattern::Component(component) => candidate
                    .split('/')
                    .any(|path_component| path_component == component),
                IgnorePattern::Directory(directory) => {
                    let directory_candidate = if is_dir {
                        candidate
                    } else {
                        candidate
                            .rsplit_once('/')
                            .map_or("", |(parent, _basename)| parent)
                    };
                    directory_candidate
                        .split('/')
                        .any(|component| component == directory)
                }
                IgnorePattern::DirectoryGlob(pattern) => {
                    let directory_candidate = if is_dir {
                        candidate
                    } else {
                        candidate
                            .rsplit_once('/')
                            .map_or("", |(parent, _basename)| parent)
                    };
                    !directory_candidate.is_empty()
                        && directory_candidate
                            .split('/')
                            .any(|component| wildcard_match(pattern, component))
                }
                IgnorePattern::DirectoryPath(directory) => {
                    (is_dir && candidate == directory)
                        || candidate.starts_with(&format!("{directory}/"))
                }
                IgnorePattern::DirectoryPathGlob(pattern) => {
                    let directory_candidate = if is_dir {
                        candidate
                    } else {
                        candidate
                            .rsplit_once('/')
                            .map_or("", |(parent, _basename)| parent)
                    };
                    !directory_candidate.is_empty() && wildcard_match(pattern, directory_candidate)
                }
                IgnorePattern::Glob(pattern) => wildcard_match(pattern, basename),
                IgnorePattern::PathGlob(pattern) => wildcard_match(pattern, candidate),
                IgnorePattern::Path(pattern) => candidate == pattern,
            } && !negated_directory_pattern_on_file;
            if matches {
                matched = Some(GitIgnoreMatch {
                    source: entry.source.clone(),
                    line_number: entry.line_number,
                    pattern: entry.pattern.clone(),
                    is_negation: entry.is_negation,
                });
            }
        }
        matched
    }
}

fn ignore_base(root: &Path, dir: &Path) -> String {
    dir.strip_prefix(root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn parse_ignore_line(raw_line: &str) -> Option<(String, String)> {
    let mut line = raw_line.trim_end_matches('\r').to_owned();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    while line.ends_with(' ') && !last_space_is_escaped(&line) {
        line.pop();
    }
    let pattern = unescape_ignore_pattern(&line);
    if pattern.is_empty() {
        return None;
    }
    Some((line, pattern))
}

fn last_space_is_escaped(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.last() != Some(&b' ') {
        return false;
    }
    let mut backslashes = 0usize;
    let mut index = bytes.len() - 1;
    while index > 0 && bytes[index - 1] == b'\\' {
        backslashes += 1;
        index -= 1;
    }
    backslashes % 2 == 1
}

fn unescape_ignore_pattern(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(next) = chars.peek().copied()
            && matches!(next, ' ' | '\\' | '#' | '!')
        {
            out.push(next);
            chars.next();
            continue;
        }
        out.push(ch);
    }
    out
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let mut memo = vec![None; (pattern.len() + 1) * (text.len() + 1)];
    wildcard_match_memo(pattern, text, 0, 0, &mut memo)
}

fn wildcard_match_memo(
    pattern: &[u8],
    text: &[u8],
    pattern_index: usize,
    text_index: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let width = text.len() + 1;
    let memo_index = pattern_index * width + text_index;
    if let Some(result) = memo[memo_index] {
        return result;
    }
    let double_star_left_boundary = pattern_index == 0 || pattern[pattern_index - 1] == b'/';
    let double_star_right_boundary =
        pattern_index + 2 == pattern.len() || pattern.get(pattern_index + 2) == Some(&b'/');
    let result = if pattern_index == pattern.len() {
        text_index == text.len()
    } else if pattern[pattern_index] == b'\\'
        && let Some(literal) = pattern.get(pattern_index + 1)
    {
        text.get(text_index) == Some(literal)
            && wildcard_match_memo(pattern, text, pattern_index + 2, text_index + 1, memo)
    } else if double_star_left_boundary && pattern[pattern_index..].starts_with(b"**/") {
        wildcard_match_memo(pattern, text, pattern_index + 3, text_index, memo)
            || (text_index < text.len()
                && wildcard_match_memo(pattern, text, pattern_index, text_index + 1, memo))
    } else if double_star_left_boundary
        && double_star_right_boundary
        && pattern[pattern_index..].starts_with(b"**")
    {
        wildcard_match_memo(pattern, text, pattern_index + 2, text_index, memo)
            || (text_index < text.len()
                && wildcard_match_memo(pattern, text, pattern_index, text_index + 1, memo))
    } else {
        match pattern[pattern_index] {
            b'*' => {
                wildcard_match_memo(pattern, text, pattern_index + 1, text_index, memo)
                    || (text.get(text_index).is_some_and(|byte| *byte != b'/')
                        && wildcard_match_memo(pattern, text, pattern_index, text_index + 1, memo))
            }
            b'?' => {
                text.get(text_index).is_some_and(|byte| *byte != b'/')
                    && wildcard_match_memo(pattern, text, pattern_index + 1, text_index + 1, memo)
            }
            b'[' => wildcard_class_match(pattern, text, pattern_index, text_index, memo),
            literal => {
                text.get(text_index) == Some(&literal)
                    && wildcard_match_memo(pattern, text, pattern_index + 1, text_index + 1, memo)
            }
        }
    };
    memo[memo_index] = Some(result);
    result
}

fn wildcard_class_match(
    pattern: &[u8],
    text: &[u8],
    pattern_index: usize,
    text_index: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let Some(&value) = text.get(text_index).filter(|byte| **byte != b'/') else {
        return false;
    };
    let Some(relative_end) = pattern[pattern_index + 1..]
        .iter()
        .position(|byte| *byte == b']')
    else {
        return value == b'['
            && wildcard_match_memo(pattern, text, pattern_index + 1, text_index + 1, memo);
    };
    let class_end = pattern_index + 1 + relative_end;
    let mut class = &pattern[pattern_index + 1..class_end];
    let negated = class
        .first()
        .is_some_and(|byte| matches!(*byte, b'!' | b'^'));
    if negated {
        class = &class[1..];
    }
    let mut matched = false;
    let mut index = 0;
    while index < class.len() {
        if index + 2 < class.len() && class[index + 1] == b'-' {
            matched |= class[index] <= value && value <= class[index + 2];
            index += 3;
        } else {
            matched |= class[index] == value;
            index += 1;
        }
    }
    (matched != negated) && wildcard_match_memo(pattern, text, class_end + 1, text_index + 1, memo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_root_ignore_subset_for_common_patterns() {
        let ignore = GitIgnore::parse("# comment\ntarget/\n*.log\nbuild/*.tmp\n/dist\n!keep.log\n");

        assert!(ignore.is_ignored(b"target", true));
        assert!(ignore.is_ignored(b"target/generated.txt", false));
        assert!(ignore.is_ignored(b"src/debug.log", false));
        assert!(ignore.is_ignored(b"build/cache.tmp", false));
        assert!(ignore.is_ignored(b"dist", false));
        assert!(!ignore.is_ignored(b"build/cache.txt", false));
        assert!(!ignore.is_ignored(b"keep.txt", false));
    }

    #[test]
    fn double_star_matches_zero_or_multiple_directories() {
        let ignore = GitIgnore::parse("**/a.1\n");

        assert!(ignore.is_ignored(b"a.1", false));
        assert!(ignore.is_ignored(b"one/a.1", false));
        assert!(ignore.is_ignored(b"one/two/a.1", false));
        assert!(!ignore.is_ignored(b"one/a.2", false));
    }

    #[test]
    fn negated_directory_glob_does_not_unignore_files() {
        let ignore = GitIgnore::parse("data/**\n!data/**/\n!data/**/*.txt\n");

        assert!(ignore.is_ignored(b"data/file", false));
        assert!(ignore.is_ignored(b"data/data1/file1", false));
        assert!(!ignore.is_ignored(b"data/data1/file1.txt", false));
        assert!(!ignore.is_ignored(b"data/data1", true));
    }

    #[test]
    fn anchored_glob_only_matches_from_ignore_base() {
        let ignore = GitIgnore::parse("/*.3\n");

        assert!(ignore.is_ignored(b"a.3", false));
        assert!(!ignore.is_ignored(b"one/a.3", false));
    }

    #[test]
    fn unanchored_directory_matches_at_any_depth_but_not_same_named_file() {
        let ignore = GitIgnore::parse("cache/\n");

        assert!(ignore.is_ignored(b"cache", true));
        assert!(ignore.is_ignored(b"src/cache", true));
        assert!(ignore.is_ignored(b"src/cache/file", false));
        assert!(!ignore.is_ignored(b"cache", false));
    }

    #[test]
    fn embedded_double_star_does_not_consume_its_following_slash() {
        let ignore = GitIgnore::parse("foo**/bar\n");

        assert!(ignore.is_ignored(b"foo/bar", false));
        assert!(!ignore.is_ignored(b"foobar", false));
    }

    #[test]
    fn scoped_rules_apply_only_below_base_directory() {
        let ignore = GitIgnore::parse_with_base("a.tmp\nnested/\n", "dir");

        assert!(ignore.is_ignored(b"dir/a.tmp", false));
        assert!(ignore.is_ignored(b"dir/sub/a.tmp", false));
        assert!(ignore.is_ignored(b"dir/nested", true));
        assert!(ignore.is_ignored(b"dir/nested/file", false));
        assert!(!ignore.is_ignored(b"a.tmp", false));
        assert!(!ignore.is_ignored(b"other/a.tmp", false));
    }

    #[test]
    fn wildcard_match_handles_star_and_question_mark() {
        assert!(wildcard_match("*.rs", "lib.rs"));
        assert!(wildcard_match("file-?.txt", "file-a.txt"));
        assert!(wildcard_match("build/*.tmp", "build/cache.tmp"));
        assert!(!wildcard_match("file-?.txt", "file-long.txt"));
        assert!(!wildcard_match("build/*.tmp", "src/cache.tmp"));
    }
}
