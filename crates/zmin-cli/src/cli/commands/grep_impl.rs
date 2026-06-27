use super::*;

pub(crate) fn grep(
    cached: bool,
    ignore_case: bool,
    invert_match: bool,
    line_number: bool,
    files_with_matches: bool,
    files_without_match: bool,
    count: bool,
    max_count: Option<usize>,
    with_filename: bool,
    full_name: bool,
    heading: bool,
    break_groups: bool,
    fixed_strings: bool,
    pattern: &str,
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
    let matcher = GrepMatcher::new(pattern, fixed_strings, ignore_case)?;
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
            &matcher,
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
            with_filename,
            full_name,
            heading,
            break_groups,
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

    fn is_match(&self, line: &[u8]) -> bool {
        match self {
            Self::Fixed(pattern) => {
                let owned;
                let haystack = if pattern
                    .iter()
                    .any(|byte| byte.is_ascii_uppercase())
                {
                    line
                } else {
                    owned = line.iter().copied().map(lower_ascii).collect::<Vec<_>>();
                    owned.as_slice()
                };
                pattern.is_empty() || haystack.windows(pattern.len()).any(|w| w == pattern)
            }
            Self::Regex(regex) => regex.is_match(line),
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
    matcher: &GrepMatcher,
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
    with_filename: bool,
    full_name: bool,
    heading: bool,
    break_groups: bool,
    printed_group: bool,
) -> Result<GrepFileOutcome> {
    let mut matched = false;
    let mut match_count = 0usize;
    let mut emitted = false;
    let display_path = grep_display_path(path, cwd_prefix, full_name);
    let display_path = String::from_utf8_lossy(&display_path);
    let filename_prefix = with_filename || output_prefix.is_some() || !heading;
    for (idx, line) in grep_lines(content).enumerate() {
        let is_match = matcher.is_match(line);
        if is_match == invert_match {
            continue;
        }
        matched = true;
        match_count += 1;
        if files_without_match {
            return Ok(GrepFileOutcome {
                matched: true,
                printed: false,
            });
        }
        if files_with_matches {
            if break_groups && printed_group {
                println!();
            }
            if let Some(prefix) = output_prefix {
                print!("{prefix}:");
            }
            println!("{display_path}");
            return Ok(GrepFileOutcome {
                matched: true,
                printed: true,
            });
        }
        if count {
            if max_count.is_some_and(|limit| match_count >= limit) {
                break;
            }
            continue;
        }
        if !emitted {
            if break_groups && printed_group {
                println!();
            }
            if heading {
                println!("{display_path}");
            }
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
        io::stdout().write_all(line)?;
        println!();
        emitted = true;
        if max_count.is_some_and(|limit| match_count >= limit) {
            break;
        }
    }
    if count && matched {
        if break_groups && printed_group {
            println!();
        }
        if let Some(prefix) = output_prefix {
            print!("{prefix}:");
        }
        println!("{display_path}:{match_count}");
        return Ok(GrepFileOutcome {
            matched: true,
            printed: true,
        });
    }
    if files_without_match && !matched {
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
    Ok(GrepFileOutcome {
        matched,
        printed: emitted,
    })
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
