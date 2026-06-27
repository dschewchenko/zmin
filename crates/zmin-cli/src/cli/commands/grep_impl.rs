use super::*;

pub(crate) fn grep(
    cached: bool,
    quiet: bool,
    ignore_case: bool,
    invert_match: bool,
    line_number: bool,
    files_with_matches: bool,
    name_only: bool,
    files_without_match: bool,
    count: bool,
    max_count: Option<usize>,
    after_context: Option<usize>,
    before_context: Option<usize>,
    context: Option<usize>,
    and: bool,
    or: bool,
    not: bool,
    patterns: Vec<String>,
    pattern_files: Vec<PathBuf>,
    with_filename: bool,
    no_filename: bool,
    null_terminated: bool,
    full_name: bool,
    heading: bool,
    break_groups: bool,
    basic_regexp: bool,
    extended_regexp: bool,
    fixed_strings: bool,
    text: bool,
    no_textconv: bool,
    word_regexp: bool,
    column: bool,
    only_matching: bool,
    pattern: Option<String>,
    args: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let grep_input = parse_grep_input(&repo, &store, &index, cached, args)?;
    let cwd_prefix = grep_cwd_prefix(&repo)?;
    let mut pathspecs = grep_input
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    if pathspecs.is_empty() && !cwd_prefix.is_empty() {
        pathspecs.push(cwd_prefix.clone());
    }
    let expression = GrepExpression::new(
        pattern,
        patterns,
        pattern_files,
        fixed_strings,
        ignore_case,
        word_regexp,
        and,
        or,
        not,
    )?;
    let files_with_matches = files_with_matches || name_only;
    let context = context.unwrap_or(0);
    let before_context = before_context.unwrap_or(context);
    let after_context = after_context.unwrap_or(context);
    let _accepted_parser_only = (basic_regexp, extended_regexp, text, no_textconv);
    let mut selected_any = false;
    let mut printed_group = false;

    for entry in grep_input
        .index
        .entries()
        .iter()
        .filter(|entry| entry.stage == 0)
    {
        if !pathspec_matches(&entry.path, &pathspecs) || entry.mode == IndexMode::Gitlink {
            continue;
        }
        let content = match grep_input.source {
            GrepSource::Worktree => {
                let path = repo
                    .root
                    .join(String::from_utf8_lossy(&entry.path).as_ref());
                match fs::read(path) {
                    Ok(content) => content,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(CliError::Io(error)),
                }
            }
            GrepSource::Index => read_index_entry_content(&store, entry)?,
        };
        let outcome = grep_file(
            &expression,
            grep_input.output_prefix.as_deref(),
            &entry.path,
            &cwd_prefix,
            &content,
            invert_match,
            line_number,
            files_with_matches,
            files_without_match,
            count,
            max_count,
            before_context,
            after_context,
            with_filename,
            no_filename,
            null_terminated,
            full_name,
            heading,
            break_groups,
            quiet,
            word_regexp,
            column,
            only_matching,
            printed_group,
        )?;
        selected_any |= if files_without_match {
            outcome.printed
        } else {
            outcome.matched
        };
        if outcome.printed {
            printed_group = true;
        }
    }

    if selected_any {
        Ok(())
    } else {
        Err(CliError::Exit(1))
    }
}

struct GrepInput {
    index: GitIndex,
    source: GrepSource,
    output_prefix: Option<String>,
    paths: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
enum GrepSource {
    Worktree,
    Index,
}

fn parse_grep_input(
    repo: &GitRepo,
    store: &LooseObjectStore,
    index: &GitIndex,
    cached: bool,
    args: Vec<String>,
) -> Result<GrepInput> {
    let (treeish, paths) = split_grep_treeish_and_paths(repo, store, args)?;
    if cached && treeish.is_some() {
        return Err(CliError::Fatal {
            code: 128,
            message: "git grep --cached does not accept a treeish".into(),
        });
    }
    let paths = paths.into_iter().map(PathBuf::from).collect();
    if let Some(treeish) = treeish {
        return Ok(GrepInput {
            index: read_treeish_index(repo, store, &treeish)?,
            source: GrepSource::Index,
            output_prefix: Some(treeish),
            paths,
        });
    }
    Ok(GrepInput {
        index: index.clone(),
        source: if cached {
            GrepSource::Index
        } else {
            GrepSource::Worktree
        },
        output_prefix: None,
        paths,
    })
}

fn split_grep_treeish_and_paths(
    repo: &GitRepo,
    store: &LooseObjectStore,
    args: Vec<String>,
) -> Result<(Option<String>, Vec<String>)> {
    let Some(first) = args.first() else {
        return Ok((None, Vec::new()));
    };
    if first == "--" {
        return Ok((None, args.into_iter().skip(1).collect()));
    }
    if resolve_treeish(repo, store, first).is_ok() {
        let treeish = first.clone();
        let mut paths = args.into_iter().skip(1).collect::<Vec<_>>();
        if paths.first().is_some_and(|arg| arg == "--") {
            paths.remove(0);
        }
        return Ok((Some(treeish), paths));
    }
    if !repo.root.join(std::path::Path::new(first)).exists() {
        return Err(ambiguous_revision_error(first));
    }
    Ok((None, args))
}

enum GrepMatcher {
    Fixed(Vec<u8>),
    Regex(Regex),
}

impl GrepMatcher {
    fn new(pattern: &str, fixed_strings: bool, ignore_case: bool) -> Result<Self> {
        if fixed_strings {
            let pattern = if ignore_case {
                pattern.bytes().map(lower_ascii).collect()
            } else {
                pattern.as_bytes().to_vec()
            };
            return Ok(Self::Fixed(pattern));
        }
        regex::bytes::RegexBuilder::new(pattern)
            .case_insensitive(ignore_case)
            .build()
            .map(Self::Regex)
            .map_err(|error| CliError::Fatal {
                code: 128,
                message: format!("invalid grep pattern: {error}"),
            })
    }

    fn match_ranges(&self, line: &[u8], word_regexp: bool) -> Vec<(usize, usize)> {
        match self {
            Self::Fixed(pattern) => {
                if pattern.is_empty() {
                    return vec![(0, 0)];
                }
                let owned;
                let haystack = if pattern.iter().any(|byte| byte.is_ascii_uppercase()) {
                    line
                } else {
                    owned = line.iter().copied().map(lower_ascii).collect::<Vec<_>>();
                    owned.as_slice()
                };
                let mut ranges = Vec::new();
                let mut start = 0usize;
                while start + pattern.len() <= haystack.len() {
                    if &haystack[start..start + pattern.len()] == pattern.as_slice() {
                        ranges.push((start, start + pattern.len()));
                        start += pattern.len().max(1);
                    } else {
                        start += 1;
                    }
                }
                if word_regexp {
                    filter_word_ranges(line, ranges)
                } else {
                    ranges
                }
            }
            Self::Regex(regex) => {
                let ranges = regex
                    .find_iter(line)
                    .map(|m| (m.start(), m.end()))
                    .collect();
                if word_regexp {
                    filter_word_ranges(line, ranges)
                } else {
                    ranges
                }
            }
        }
    }
}

enum GrepExpressionMode {
    Any,
    All,
    AllButLast,
}

struct GrepExpression {
    mode: GrepExpressionMode,
    matchers: Vec<GrepMatcher>,
}

impl GrepExpression {
    fn new(
        pattern: Option<String>,
        patterns: Vec<String>,
        pattern_files: Vec<PathBuf>,
        fixed_strings: bool,
        ignore_case: bool,
        word_regexp: bool,
        and: bool,
        or: bool,
        not: bool,
    ) -> Result<Self> {
        let mut all_patterns = Vec::new();
        if let Some(pattern) = pattern {
            all_patterns.push(pattern);
        }
        all_patterns.extend(patterns);
        for path in pattern_files {
            let content = fs::read_to_string(path)?;
            all_patterns.extend(content.lines().map(str::to_owned));
        }
        if all_patterns.is_empty() {
            return Err(CliError::Fatal {
                code: 128,
                message: "no pattern given".into(),
            });
        }
        if not && (!and || all_patterns.len() != 2) {
            return Err(CliError::Fatal {
                code: 128,
                message: "git grep --not is only modeled in a two-pattern --and expression".into(),
            });
        }
        if and && or {
            return Err(CliError::Fatal {
                code: 128,
                message: "git grep does not support mixing modeled --and and --or in one helper-free lane".into(),
            });
        }
        let matchers = all_patterns
            .iter()
            .map(|pattern| GrepMatcher::new(pattern, fixed_strings, ignore_case))
            .collect::<Result<Vec<_>>>()?;
        let mode = if and && not {
            GrepExpressionMode::AllButLast
        } else if and {
            GrepExpressionMode::All
        } else {
            let _ = or;
            GrepExpressionMode::Any
        };
        let _ = word_regexp;
        Ok(Self { mode, matchers })
    }

    fn match_ranges(&self, line: &[u8], word_regexp: bool) -> Vec<(usize, usize)> {
        match self.mode {
            GrepExpressionMode::Any => self
                .matchers
                .iter()
                .flat_map(|matcher| matcher.match_ranges(line, word_regexp))
                .collect(),
            GrepExpressionMode::All => {
                let mut ranges = Vec::new();
                for matcher in &self.matchers {
                    let matcher_ranges = matcher.match_ranges(line, word_regexp);
                    if matcher_ranges.is_empty() {
                        return Vec::new();
                    }
                    ranges.extend(matcher_ranges);
                }
                ranges
            }
            GrepExpressionMode::AllButLast => {
                let mut head_ranges = Vec::new();
                for matcher in &self.matchers[..self.matchers.len() - 1] {
                    let matcher_ranges = matcher.match_ranges(line, word_regexp);
                    if matcher_ranges.is_empty() {
                        return Vec::new();
                    }
                    head_ranges.extend(matcher_ranges);
                }
                if self.matchers[self.matchers.len() - 1]
                    .match_ranges(line, word_regexp)
                    .is_empty()
                {
                    head_ranges
                } else {
                    Vec::new()
                }
            }
        }
    }
}

struct GrepFileOutcome {
    matched: bool,
    printed: bool,
}

fn grep_cwd_prefix(repo: &GitRepo) -> Result<Vec<u8>> {
    let mut prefix = repo_relative_path(&repo.root, &std::env::current_dir()?)?;
    while prefix.ends_with(b"/") {
        prefix.pop();
    }
    Ok(prefix)
}

fn grep_file(
    expression: &GrepExpression,
    output_prefix: Option<&str>,
    path: &[u8],
    cwd_prefix: &[u8],
    content: &[u8],
    invert_match: bool,
    line_number: bool,
    files_with_matches: bool,
    files_without_match: bool,
    count: bool,
    max_count: Option<usize>,
    before_context: usize,
    after_context: usize,
    with_filename: bool,
    no_filename: bool,
    null_terminated: bool,
    full_name: bool,
    heading: bool,
    break_groups: bool,
    quiet: bool,
    word_regexp: bool,
    column: bool,
    only_matching: bool,
    printed_group: bool,
) -> Result<GrepFileOutcome> {
    let mut emitted = false;
    let display_path = grep_display_path(path, cwd_prefix, full_name);
    let display_path = String::from_utf8_lossy(&display_path);
    let filename_prefix = !no_filename && (with_filename || output_prefix.is_some() || !heading);
    let lines = grep_lines(content).collect::<Vec<_>>();
    let mut line_ranges = Vec::with_capacity(lines.len());
    let mut matching_lines = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let ranges = expression.match_ranges(line, word_regexp);
        let is_match = !ranges.is_empty();
        line_ranges.push(ranges);
        if is_match != invert_match {
            matching_lines.push(idx);
        }
    }
    if let Some(limit) = max_count {
        matching_lines.truncate(limit);
    }
    let matched = !matching_lines.is_empty();
    let match_count = matching_lines.len();
    if quiet {
        return Ok(GrepFileOutcome {
            matched,
            printed: false,
        });
    }
    if files_without_match {
        if matched {
            return Ok(GrepFileOutcome {
                matched: true,
                printed: false,
            });
        }
        if break_groups && printed_group {
            println!();
        }
        if let Some(prefix) = output_prefix {
            print!("{prefix}:");
        }
        println!("{display_path}");
        return Ok(GrepFileOutcome {
            matched: false,
            printed: true,
        });
    }
    if files_with_matches {
        if !matched {
            return Ok(GrepFileOutcome {
                matched: false,
                printed: false,
            });
        }
        if break_groups && printed_group {
            println!();
        }
        if let Some(prefix) = output_prefix {
            print!("{prefix}:");
        }
        if null_terminated {
            print!("{display_path}\0");
        } else {
            println!("{display_path}");
        }
        return Ok(GrepFileOutcome {
            matched: true,
            printed: true,
        });
    }
    if count {
        if matched {
            if break_groups && printed_group {
                println!();
            }
            if let Some(prefix) = output_prefix {
                print!("{prefix}:");
            }
            println!("{display_path}:{match_count}");
        }
        return Ok(GrepFileOutcome {
            matched,
            printed: matched,
        });
    }
    if before_context > 0 || after_context > 0 {
        let groups = build_context_groups(lines.len(), &matching_lines, before_context, after_context);
        for (group_idx, (start, end)) in groups.iter().enumerate() {
            if printed_group || group_idx > 0 {
                println!("--");
            }
            for line_idx in *start..=*end {
                let is_match = matching_lines.binary_search(&line_idx).is_ok();
                let line = lines[line_idx];
                let ranges = &line_ranges[line_idx];
                if let Some(prefix) = output_prefix {
                    let separator = if is_match { ':' } else { '-' };
                    print!("{prefix}{separator}");
                }
                if filename_prefix {
                    let separator = if is_match { ':' } else { '-' };
                    print!("{display_path}{separator}");
                    if line_number {
                        print!("{}{separator}", line_idx + 1);
                    }
                } else if line_number {
                    let separator = if is_match { ':' } else { '-' };
                    print!("{}{separator}", line_idx + 1);
                }
                if is_match && column {
                    print!("{}:", ranges[0].0 + 1);
                }
                io::stdout().write_all(line)?;
                println!();
                emitted = true;
            }
        }
        return Ok(GrepFileOutcome {
            matched,
            printed: emitted,
        });
    }
    for idx in matching_lines {
        let line = lines[idx];
        let ranges = &line_ranges[idx];
        if !emitted {
            if break_groups && printed_group {
                println!();
            }
            if heading {
                println!("{display_path}");
            }
        }
        if only_matching {
            for &(start, end) in ranges {
                if let Some(prefix) = output_prefix {
                    print!("{prefix}:");
                }
                if filename_prefix {
                    print!("{display_path}:");
                }
                if line_number {
                    print!("{}:", idx + 1);
                }
                if column {
                    print!("{}:", start + 1);
                }
                println!("{}", String::from_utf8_lossy(&line[start..end]));
                emitted = true;
            }
            continue;
        }
        if let Some(prefix) = output_prefix {
            print!("{prefix}:");
        }
        if filename_prefix {
            if line_number {
                print!("{display_path}:{}:", idx + 1);
            } else {
                print!("{display_path}:");
            }
        } else if line_number {
            print!("{}:", idx + 1);
        }
        if column {
            print!("{}:", ranges[0].0 + 1);
        }
        io::stdout().write_all(line)?;
        println!();
        emitted = true;
    }
    Ok(GrepFileOutcome {
        matched,
        printed: emitted,
    })
}

fn filter_word_ranges(line: &[u8], ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges
        .into_iter()
        .filter(|(start, end)| {
            let left = *start == 0 || !is_word_byte(line[start.saturating_sub(1)]);
            let right = *end == line.len() || !is_word_byte(line[*end]);
            left && right
        })
        .collect()
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn build_context_groups(
    total_lines: usize,
    matching_lines: &[usize],
    before_context: usize,
    after_context: usize,
) -> Vec<(usize, usize)> {
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for line_idx in matching_lines {
        let start = line_idx.saturating_sub(before_context);
        let end = (line_idx + after_context).min(total_lines.saturating_sub(1));
        if let Some((_, previous_end)) = groups.last_mut() {
            if start <= *previous_end + 1 {
                *previous_end = (*previous_end).max(end);
                continue;
            }
        }
        groups.push((start, end));
    }
    groups
}

fn grep_lines(content: &[u8]) -> impl Iterator<Item = &[u8]> {
    content
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty() || !content.ends_with(b"\n"))
        .map(|line| {
        if let Some(line) = line.strip_suffix(b"\r") {
            line
        } else {
            line
        }
    })
}

fn grep_display_path(path: &[u8], cwd_prefix: &[u8], full_name: bool) -> Vec<u8> {
    if full_name || cwd_prefix.is_empty() {
        return path.to_vec();
    }
    if path == cwd_prefix {
        return Vec::new();
    }
    if let Some(rest) = path
        .strip_prefix(cwd_prefix)
        .and_then(|rest| rest.strip_prefix(b"/"))
    {
        return rest.to_vec();
    }
    relative_pathspec_bytes(cwd_prefix, path)
}

fn relative_pathspec_bytes(from: &[u8], to: &[u8]) -> Vec<u8> {
    let from_components = from
        .split(|byte| *byte == b'/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    let to_components = to
        .split(|byte| *byte == b'/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    let mut out = Vec::new();
    for _ in common..from_components.len() {
        if !out.is_empty() {
            out.push(b'/');
        }
        out.extend_from_slice(b"..");
    }
    for component in &to_components[common..] {
        if !out.is_empty() {
            out.push(b'/');
        }
        out.extend_from_slice(component);
    }
    out
}

fn lower_ascii(byte: u8) -> u8 {
    byte.to_ascii_lowercase()
}
