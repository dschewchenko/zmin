use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct GitIgnore {
    patterns: Vec<IgnorePatternEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIgnoreMatch {
    pub line_number: usize,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IgnorePatternEntry {
    base: String,
    line_number: usize,
    pattern: String,
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
        let path = root.join(".gitignore");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        Ok(Self::parse(&content))
    }

    pub fn parse(content: &str) -> Self {
        Self::parse_with_base(content, "")
    }

    pub fn parse_with_base(content: &str, base: &str) -> Self {
        let mut patterns = Vec::new();
        let base = base.trim_matches('/').replace('\\', "/");
        for (line_number, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                continue;
            }
            let display_pattern = line.to_owned();
            let pattern = line.trim_start_matches('/').to_owned();
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
                line_number: line_number + 1,
                pattern: display_pattern,
                kind,
            });
        }
        Self { patterns }
    }

    pub fn append(&mut self, mut other: Self) {
        self.patterns.append(&mut other.patterns);
    }

    pub fn is_ignored(&self, path: &[u8], is_dir: bool) -> bool {
        self.match_path(path, is_dir).is_some()
    }

    pub fn match_path(&self, path: &[u8], is_dir: bool) -> Option<GitIgnoreMatch> {
        let relative = String::from_utf8_lossy(path).replace('\\', "/");
        self.patterns.iter().find_map(|entry| {
            let candidate = if entry.base.is_empty() {
                relative.as_str()
            } else if relative == entry.base {
                ""
            } else {
                relative.strip_prefix(&format!("{}/", entry.base))?
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
            matches.then(|| GitIgnoreMatch {
                line_number: entry.line_number,
                pattern: entry.pattern.clone(),
            })
        })
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
