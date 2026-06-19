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
    Glob(String),
    Path(String),
}

impl GitIgnore {
    pub fn load_from_root(root: &Path) -> io::Result<Self> {
        let mut ignore = Self::default();
        ignore.load_from_dir(root, root)?;
        Ok(ignore)
    }

    fn load_from_dir(&mut self, root: &Path, dir: &Path) -> io::Result<()> {
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
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                self.load_from_dir(root, &entry.path())?;
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
            let pattern = line_pattern.trim_start_matches('/').to_owned();
            if pattern.is_empty() {
                continue;
            }
            let kind = if let Some(directory) = pattern.strip_suffix('/') {
                IgnorePattern::Directory(directory.to_owned())
            } else if pattern.contains('*') || pattern.contains('?') {
                IgnorePattern::Glob(pattern.clone())
            } else if pattern.contains('/') {
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
            let matches = match &entry.kind {
                IgnorePattern::Component(component) => candidate
                    .split('/')
                    .any(|path_component| path_component == component),
                IgnorePattern::Directory(directory) => {
                    (is_dir && candidate == *directory)
                        || candidate.starts_with(&format!("{directory}/"))
                }
                IgnorePattern::Glob(pattern) if pattern.contains('/') => {
                    wildcard_match(pattern, candidate)
                }
                IgnorePattern::Glob(pattern) => wildcard_match(pattern, basename),
                IgnorePattern::Path(pattern) => candidate == pattern,
            };
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
