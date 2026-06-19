use super::*;

pub(crate) fn grep(
    cached: bool,
    line_number: bool,
    files_with_matches: bool,
    fixed_strings: bool,
    pattern: &str,
    args: Vec<String>,
) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let index = read_repo_index(&repo)?;
    let grep_input = parse_grep_input(&repo, &store, &index, cached, args)?;
    let pathspecs = grep_input
        .paths
        .iter()
        .map(|path| path_arg_to_repo_relative(&repo, path))
        .collect::<Result<Vec<_>>>()?;
    let matcher = GrepMatcher::new(pattern, fixed_strings)?;
    let mut matched_any = false;

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
        if grep_file(
            &matcher,
            grep_input.output_prefix.as_deref(),
            &entry.path,
            &content,
            line_number,
            files_with_matches,
        )? {
            matched_any = true;
        }
    }

    if matched_any {
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
    fn new(pattern: &str, fixed_strings: bool) -> Result<Self> {
        if fixed_strings {
            return Ok(Self::Fixed(pattern.as_bytes().to_vec()));
        }
        Regex::new(pattern)
            .map(Self::Regex)
            .map_err(|error| CliError::Fatal {
                code: 128,
                message: format!("invalid grep pattern: {error}"),
            })
    }

    fn is_match(&self, line: &[u8]) -> bool {
        match self {
            Self::Fixed(pattern) => {
                pattern.is_empty() || line.windows(pattern.len()).any(|w| w == pattern)
            }
            Self::Regex(regex) => regex.is_match(line),
        }
    }
}

fn grep_file(
    matcher: &GrepMatcher,
    output_prefix: Option<&str>,
    path: &[u8],
    content: &[u8],
    line_number: bool,
    files_with_matches: bool,
) -> Result<bool> {
    let mut matched = false;
    for (idx, line) in grep_lines(content).enumerate() {
        if !matcher.is_match(line) {
            continue;
        }
        matched = true;
        let display_path = String::from_utf8_lossy(path);
        if files_with_matches {
            if let Some(prefix) = output_prefix {
                print!("{prefix}:");
            }
            println!("{display_path}");
            return Ok(true);
        }
        if let Some(prefix) = output_prefix {
            print!("{prefix}:");
        }
        if line_number {
            print!("{display_path}:{}:", idx + 1);
        } else {
            print!("{display_path}:");
        }
        io::stdout().write_all(line)?;
        println!();
    }
    Ok(matched)
}

fn grep_lines(content: &[u8]) -> impl Iterator<Item = &[u8]> {
    content.split(|byte| *byte == b'\n').map(|line| {
        if let Some(line) = line.strip_suffix(b"\r") {
            line
        } else {
            line
        }
    })
}
