use super::*;

pub(crate) fn bisect(args: Vec<String>) -> Result<()> {
    let subcommand = args.first().map(String::as_str).unwrap_or("start");
    match subcommand {
        "-h" | "--help" => {
            bisect_help();
            Err(CliError::Exit(129))
        }
        "help" if cfg!(windows) => Err(CliError::Stderr {
            code: 129,
            text: bisect_usage().to_owned(),
        }),
        "start" => bisect_start(&args[1..]),
        "good" | "old" => bisect_mark(true, args.get(1).map(String::as_str)),
        "bad" | "new" => bisect_mark(false, args.get(1).map(String::as_str)),
        "next" => bisect_next_command(),
        "skip" => bisect_skip(&args[1..]),
        "terms" => bisect_terms(&args[1..]),
        "visualize" | "view" => bisect_visualize(&args[1..]),
        "replay" => bisect_replay(args.get(1).map(String::as_str)),
        "run" => bisect_run(&args[1..]),
        "reset" => bisect_reset(args.get(1).map(String::as_str)),
        "log" => bisect_log(),
        _ => {
            let repo = find_repo()?;
            if !repo.git_dir.join("BISECT_START").is_file() {
                return ensure_bisect_started(&repo);
            }
            let (bad_term, good_term) = bisect_terms_for_repo(&repo)?;
            if subcommand == good_term {
                bisect_mark(true, args.get(1).map(String::as_str))
            } else if subcommand == bad_term {
                bisect_mark(false, args.get(1).map(String::as_str))
            } else {
                Err(CliError::Stderr {
                    code: 129,
                    text: format!(
                        "error: Invalid command: you're currently in a bad/good bisect\n\
                         fatal: unknown command: '{subcommand}'\n\n\
                         {}",
                        bisect_usage()
                    ),
                })
            }
        }
    }
}

#[cfg(not(windows))]
fn bisect_usage() -> &'static str {
    concat!(
        "usage: git bisect start [--term-(new|bad)=<term> --term-(old|good)=<term>]    [--no-checkout] [--first-parent] [<bad> [<good>...]] [--]    [<pathspec>...]\n",
        "   or: git bisect (good|bad) [<rev>...]\n",
        "   or: git bisect terms [--term-good | --term-bad]\n",
        "   or: git bisect skip [(<rev>|<range>)...]\n",
        "   or: git bisect next\n",
        "   or: git bisect reset [<commit>]\n",
        "   or: git bisect visualize\n",
        "   or: git bisect replay <logfile>\n",
        "   or: git bisect log\n",
        "   or: git bisect run <cmd> [<arg>...]\n",
    )
}

#[cfg(windows)]
fn bisect_usage() -> &'static str {
    concat!(
        "usage: git bisect start [--term-(bad|new)=<term-new> --term-(good|old)=<term-old>]\n",
        "                        [--no-checkout] [--first-parent] [<bad> [<good>...]] [--] [<pathspec>...]\n",
        "   or: git bisect (bad|new|<term-new>) [<rev>]\n",
        "   or: git bisect (good|old|<term-old>) [<rev>...]\n",
        "   or: git bisect terms [--term-(good|old) | --term-(bad|new)]\n",
        "   or: git bisect skip [(<rev>|<range>)...]\n",
        "   or: git bisect next\n",
        "   or: git bisect reset [<commit>]\n",
        "   or: git bisect (visualize|view)\n",
        "   or: git bisect replay <logfile>\n",
        "   or: git bisect log\n",
        "   or: git bisect run <cmd> [<arg>...]\n",
        "   or: git bisect help\n",
    )
}

fn bisect_help() {
    print!("{}", bisect_usage());
}

pub(crate) fn rerere(args: Vec<String>) -> Result<()> {
    let autoupdate = args.iter().any(|arg| arg == "--rerere-autoupdate");
    let args = args
        .into_iter()
        .filter(|arg| {
            !matches!(
                arg.as_str(),
                "--rerere-autoupdate" | "--no-rerere-autoupdate"
            )
        })
        .collect::<Vec<_>>();
    if args.is_empty() {
        return rerere_record_resolutions(autoupdate);
    }
    let operation = args.first().map(String::as_str).unwrap_or("record");
    match operation {
        "status" | "remaining" => rerere_status(),
        "clear" => rerere_clear(),
        "diff" => rerere_diff(),
        "forget" => rerere_forget(&args[1..]),
        "gc" => rerere_gc(),
        "record" => rerere_usage(),
        _ => rerere_usage(),
    }
}

fn rerere_status() -> Result<()> {
    let repo = find_repo()?;
    if !config_bool_enabled(&repo, "rerere.enabled")? {
        return Ok(());
    }
    let index = read_repo_index(&repo)?;
    for path in merge_index_unmerged_paths(&index) {
        println!("{}", String::from_utf8_lossy(&path));
    }
    Ok(())
}

fn rerere_clear() -> Result<()> {
    let repo = find_repo()?;
    match fs::remove_dir_all(repo.git_dir.join("rr-cache")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn rerere_usage() -> Result<()> {
    Err(CliError::Stderr {
        code: 129,
        text: "usage: git rerere [clear | forget <pathspec>... | diff | status | remaining | gc]\n\n    --[no-]rerere-autoupdate\n                          register clean resolutions in index\n".into(),
    })
}

fn rerere_diff() -> Result<()> {
    let repo = find_repo()?;
    for entry in rerere_merge_rr_entries(&repo)? {
        if entry.cache_file(&repo, "postimage").is_file() {
            continue;
        }
        let preimage = entry.cache_file(&repo, "preimage");
        let Ok(old) = fs::read(&preimage) else {
            continue;
        };
        let worktree_path = repo.root.join(&entry.path);
        let Ok(new) = fs::read(&worktree_path) else {
            continue;
        };
        println!("--- a/{}", entry.path);
        println!("+++ b/{}", entry.path);
        print_unified_full_file_hunk(&old, &new, &entry.path)?;
    }
    Ok(())
}

fn rerere_record_resolutions(autoupdate: bool) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let mut index = read_repo_index(&repo)?;
    let mut index_updated = false;
    for entry in rerere_merge_rr_entries(&repo)? {
        let preimage = entry.cache_file(&repo, "preimage");
        if !preimage.is_file() {
            continue;
        }
        let worktree_path = repo.root.join(&entry.path);
        let Ok(content) = fs::read(&worktree_path) else {
            continue;
        };
        let postimage = entry.cache_file(&repo, "postimage");
        if postimage.is_file() {
            if rerere_content_has_conflict_markers(&content) {
                let postimage_content = fs::read(&postimage)?;
                fs::write(&worktree_path, postimage_content)?;
                if autoupdate {
                    stage_file(&repo, &store, &mut index, &worktree_path)?;
                    index_updated = true;
                    eprintln!("Staged '{}' using previous resolution.", entry.path);
                } else {
                    eprintln!("Resolved '{}' using previous resolution.", entry.path);
                }
            }
            continue;
        }
        if rerere_content_has_conflict_markers(&content) {
            continue;
        }
        fs::create_dir_all(entry.cache_dir(&repo))?;
        fs::write(postimage, content)?;
        eprintln!("Recorded resolution for '{}'.", entry.path);
    }
    if index_updated {
        index.write_to_path(&repo.index_path)?;
    }
    Ok(())
}

fn rerere_forget(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return rerere_usage();
    }
    let repo = find_repo()?;
    let entries = rerere_merge_rr_entries(&repo)?;
    let pathspecs = args
        .iter()
        .map(|pathspec| path_arg_to_repo_relative(&repo, Path::new(pathspec)))
        .collect::<Result<Vec<_>>>()?;
    for pathspec in &pathspecs {
        let wanted = String::from_utf8_lossy(pathspec).to_string();
        let mut found = false;
        for entry in entries
            .iter()
            .filter(|entry| pathspec_matches(entry.path.as_bytes(), std::slice::from_ref(pathspec)))
        {
            found = true;
            let postimage = entry.cache_file(&repo, "postimage");
            if postimage.is_file() {
                let worktree_path = repo.root.join(&entry.path);
                if let Ok(content) = fs::read(&worktree_path) {
                    fs::write(entry.cache_file(&repo, "thisimage"), content)?;
                    eprintln!("Updated preimage for '{}'", entry.path);
                }
                remove_file_if_exists(&postimage)?;
                eprintln!("Forgot resolution for '{}'", entry.path);
            } else {
                eprintln!("error: no remembered resolution for '{}'", entry.path);
            }
        }
        if !found {
            eprintln!("error: no remembered resolution for '{wanted}'");
        }
    }
    Ok(())
}

fn rerere_gc() -> Result<()> {
    let repo = find_repo()?;
    let rr_cache = repo.git_dir.join("rr-cache");
    let resolved_days = rerere_gc_days(&repo, "gc.rerereResolved", 60)?;
    let unresolved_days = rerere_gc_days(&repo, "gc.rerereUnresolved", 15)?;
    let Ok(entries) = fs::read_dir(&rr_cache) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let has_preimage = path.join("preimage").is_file();
        let has_postimage = path.join("postimage").is_file();
        if !has_preimage && !has_postimage {
            fs::remove_dir_all(path)?;
            continue;
        }
        let expiry_days = if has_postimage {
            resolved_days
        } else {
            unresolved_days
        };
        let age_path = if has_postimage {
            path.join("postimage")
        } else {
            path.join("preimage")
        };
        if rerere_cache_file_older_than(&age_path, expiry_days)? {
            fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}

fn rerere_gc_days(repo: &GitRepo, name: &str, default: u64) -> Result<u64> {
    let Some(value) = read_config_value(repo, name)? else {
        return Ok(default);
    };
    value.trim().parse::<u64>().map_err(|_| CliError::Fatal {
        code: 128,
        message: format!("bad numeric config value '{value}' for '{name}'"),
    })
}

fn rerere_cache_file_older_than(path: &Path, days: u64) -> Result<bool> {
    if days == 0 {
        return Ok(false);
    }
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified().map_err(CliError::Io)?;
    let Ok(age) = std::time::SystemTime::now().duration_since(modified) else {
        return Ok(false);
    };
    Ok(age >= std::time::Duration::from_secs(days.saturating_mul(24 * 60 * 60)))
}

#[derive(Debug, Clone)]
struct RerereMergeEntry {
    cache_key: String,
    file_suffix: String,
    path: String,
}

impl RerereMergeEntry {
    fn cache_dir(&self, repo: &GitRepo) -> PathBuf {
        repo.git_dir.join("rr-cache").join(&self.cache_key)
    }

    fn cache_file(&self, repo: &GitRepo, name: &str) -> PathBuf {
        self.cache_dir(repo)
            .join(format!("{name}{}", self.file_suffix))
    }
}

fn rerere_merge_rr_entries(repo: &GitRepo) -> Result<Vec<RerereMergeEntry>> {
    let path = repo.git_dir.join("MERGE_RR");
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    let mut entries = Vec::new();
    for raw in bytes.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(raw);
        let Some((key, path)) = text.split_once('\t') else {
            continue;
        };
        let (cache_key, file_suffix) = rerere_cache_key_and_suffix(key);
        entries.push(RerereMergeEntry {
            cache_key,
            file_suffix,
            path: path.to_owned(),
        });
    }
    Ok(entries)
}

fn rerere_cache_key_and_suffix(key: &str) -> (String, String) {
    if key.len() > 40 && key.as_bytes().get(40) == Some(&b'.') {
        (key[..40].to_owned(), key[40..].to_owned())
    } else {
        (key.to_owned(), String::new())
    }
}

fn rerere_content_has_conflict_markers(content: &[u8]) -> bool {
    content.split(|byte| *byte == b'\n').any(|line| {
        line.starts_with(b"<<<<<<<") || line.starts_with(b"=======") || line.starts_with(b">>>>>>>")
    })
}

fn bisect_start(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let parsed = parse_bisect_start_args(args)?;
    let start_id = resolve_commitish(&repo, &store, "HEAD")?;
    fs::write(
        repo.git_dir.join("BISECT_START"),
        format!("{}\n", start_id.to_hex()),
    )?;
    fs::write(
        repo.git_dir.join("BISECT_TERMS"),
        format!("{}\n{}\n", parsed.bad_term, parsed.good_term),
    )?;
    fs::write(
        repo.git_dir.join("BISECT_NAMES"),
        bisect_names_content(&parsed.pathspecs),
    )?;
    if let Some(branch) = current_branch_ref_from_head_file(&repo.git_dir)? {
        fs::write(repo.git_dir.join("BISECT_START_REF"), format!("{branch}\n"))?;
    } else {
        remove_file_if_exists(&repo.git_dir.join("BISECT_START_REF"))?;
    }
    if parsed.no_checkout {
        fs::write(repo.git_dir.join("BISECT_NO_CHECKOUT"), b"1\n")?;
    } else {
        remove_file_if_exists(&repo.git_dir.join("BISECT_NO_CHECKOUT"))?;
    }
    if parsed.first_parent {
        fs::write(repo.git_dir.join("BISECT_FIRST_PARENT"), b"1\n")?;
    } else {
        remove_file_if_exists(&repo.git_dir.join("BISECT_FIRST_PARENT"))?;
    }
    bisect_clear_refs(&repo)?;
    bisect_append_start_log(&repo, args)?;
    if parsed.revs.is_empty() {
        return Ok(());
    }
    let bad = resolve_commitish(&repo, &store, &parsed.revs[0])?;
    refs.write_ref("refs/bisect/bad", &bad)?;
    for good in &parsed.revs[1..] {
        let good = resolve_commitish(&repo, &store, good)?;
        refs.write_ref(&bisect_good_ref(&good), &good)?;
    }
    match bisect_next(&repo, &store) {
        Ok(_) => Ok(()),
        Err(error) => {
            if matches!(&error, CliError::Stderr { code: 4, .. }) {
                bisect_clear_state(&repo)?;
            }
            Err(error)
        }
    }
}

fn current_branch_ref_from_head_file(git_dir: &Path) -> Result<Option<String>> {
    match fs::read_to_string(git_dir.join("HEAD")) {
        Ok(head) => Ok(head
            .trim()
            .strip_prefix("ref: ")
            .filter(|target| target.starts_with("refs/heads/"))
            .map(str::to_owned)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CliError::Io(error)),
    }
}

struct ParsedBisectStart {
    bad_term: String,
    good_term: String,
    no_checkout: bool,
    first_parent: bool,
    revs: Vec<String>,
    pathspecs: Vec<String>,
}

fn parse_bisect_start_args(args: &[String]) -> Result<ParsedBisectStart> {
    let mut bad_term = "bad".to_owned();
    let mut good_term = "good".to_owned();
    let mut no_checkout = false;
    let mut first_parent = false;
    let mut revs = Vec::new();
    let mut pathspecs = Vec::new();
    let mut end_of_options = false;
    for arg in args {
        if !end_of_options && arg == "--" {
            end_of_options = true;
            continue;
        }
        if end_of_options {
            pathspecs.push(arg.clone());
            continue;
        }
        if !end_of_options {
            if let Some(value) = arg
                .strip_prefix("--term-new=")
                .or_else(|| arg.strip_prefix("--term-bad="))
            {
                bad_term = validate_bisect_term(value)?;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--term-old=")
                .or_else(|| arg.strip_prefix("--term-good="))
            {
                good_term = validate_bisect_term(value)?;
                continue;
            }
            if arg == "--no-checkout" {
                no_checkout = true;
                continue;
            }
            if arg == "--first-parent" {
                first_parent = true;
                continue;
            }
        }
        revs.push(arg.clone());
    }
    if bad_term == good_term {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("please use two different terms: '{bad_term}' and '{good_term}'"),
        });
    }
    Ok(ParsedBisectStart {
        bad_term,
        good_term,
        no_checkout,
        first_parent,
        revs,
        pathspecs,
    })
}

fn validate_bisect_term(value: &str) -> Result<String> {
    if value.is_empty()
        || matches!(
            value,
            "help"
                | "start"
                | "terms"
                | "skip"
                | "next"
                | "reset"
                | "visualize"
                | "view"
                | "replay"
                | "log"
                | "run"
        )
    {
        return Err(CliError::Fatal {
            code: 1,
            message: format!("invalid term: {value}"),
        });
    }
    Ok(value.to_owned())
}

fn bisect_mark(good: bool, rev: Option<&str>) -> Result<()> {
    let repo = find_repo()?;
    ensure_bisect_started(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let id = if let Some(rev) = rev {
        resolve_commitish(&repo, &store, rev)?
    } else {
        bisect_current_target(&repo, &refs)?
    };
    if good {
        refs.write_ref(&bisect_good_ref(&id), &id)?;
    } else {
        refs.write_ref("refs/bisect/bad", &id)?;
    }
    bisect_append_log(&repo, if good { "good" } else { "bad" }, &id)?;
    bisect_next(&repo, &store).map(|_| ())
}

fn bisect_current_target(repo: &GitRepo, refs: &RefStore) -> Result<ObjectId> {
    match fs::read_to_string(repo.git_dir.join("BISECT_HEAD")) {
        Ok(value) => ObjectId::from_hex(GitHashAlgorithm::Sha1, value.trim()).map_err(CliError::Io),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            refs.resolve("HEAD").map_err(CliError::Io)
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn bisect_names_content(pathspecs: &[String]) -> String {
    if pathspecs.is_empty() {
        "\n".to_owned()
    } else {
        format!("{}\n", pathspecs.join("\n"))
    }
}

fn bisect_pathspecs(repo: &GitRepo) -> Result<Vec<Vec<u8>>> {
    let content = match fs::read_to_string(repo.git_dir.join("BISECT_NAMES")) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CliError::Io(error)),
    };
    content
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| path_arg_to_repo_relative_allow_root(repo, Path::new(line)))
        .collect()
}

fn bisect_no_checkout(repo: &GitRepo) -> bool {
    repo.git_dir.join("BISECT_NO_CHECKOUT").is_file()
}

fn bisect_first_parent(repo: &GitRepo) -> bool {
    repo.git_dir.join("BISECT_FIRST_PARENT").is_file()
}

fn bisect_candidate_commits(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    bad: &ObjectId,
) -> Result<Vec<ObjectId>> {
    if !bisect_first_parent(repo) {
        return collect_commits_cached(repo, store, commit_cache, &[bad.to_hex()], None);
    }
    let mut commits = Vec::new();
    let mut current = bad.clone();
    loop {
        commits.push(current.clone());
        let commit = commit_cache.read_commit(&current)?;
        let Some(parent) = commit.parents.first() else {
            break;
        };
        current = parent.clone();
    }
    Ok(commits)
}

fn bisect_commit_touches_pathspecs(
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    id: &ObjectId,
    pathspecs: &[Vec<u8>],
) -> Result<bool> {
    if pathspecs.is_empty() {
        return Ok(true);
    }
    let tree_cache = TreeObjectCache::new(store);
    let commit = commit_cache.read_commit(id)?;
    let new_index = tree_cache.read_tree_to_index(&commit.tree)?;
    let old_index = if let Some(parent) = commit.parents.first() {
        let parent_commit = commit_cache.read_commit(parent)?;
        read_commit_tree_index_cached(&tree_cache, &parent_commit)?
    } else {
        GitIndex::new()
    };
    Ok(diff_indexes(&old_index, &new_index)?
        .into_iter()
        .any(|entry| pathspec_matches(&entry.path, pathspecs)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BisectStep {
    Continue,
    Done,
    SkippedOnly,
}

fn bisect_next(repo: &GitRepo, store: &LooseObjectStore) -> Result<BisectStep> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let bad = refs.resolve("refs/bisect/bad")?;
    let goods = bisect_good_ids(repo)?;
    let skipped = bisect_skipped_ids(repo)?;
    let pathspecs = bisect_pathspecs(repo)?;
    let commit_cache = CommitObjectCache::new(store);
    if goods.is_empty() {
        return Err(CliError::Fatal {
            code: 128,
            message: "bisect requires at least one good commit".into(),
        });
    }
    let mut candidates = bisect_candidate_commits(repo, store, &commit_cache, &bad)?
        .into_iter()
        .filter(|id| {
            !goods
                .iter()
                .any(|good| is_ancestor_commit_cached(&commit_cache, id, good).unwrap_or(false))
        })
        .collect::<Vec<_>>();
    if !pathspecs.is_empty() {
        candidates.retain(|id| {
            bisect_commit_touches_pathspecs(store, &commit_cache, id, &pathspecs).unwrap_or(true)
        });
        if candidates.is_empty() {
            return Err(CliError::Stderr {
                code: 4,
                text: "No testable commit found.\nMaybe you started with bad path arguments?\n"
                    .into(),
            });
        }
    }
    candidates.retain(|id| !goods.iter().any(|good| good == id));
    let mut possible_skipped = skipped.clone();
    candidates.retain(|id| !skipped.iter().any(|skipped| skipped == id));
    if !skipped.is_empty() && candidates.len() <= 1 {
        possible_skipped.extend(candidates.iter().cloned());
        dedup_object_ids(&mut possible_skipped);
        let mut log_possible = candidates.clone();
        log_possible.extend(skipped.iter().cloned());
        dedup_object_ids(&mut log_possible);
        bisect_report_only_skipped_left(repo, store, &possible_skipped, &log_possible)?;
        return Ok(BisectStep::SkippedOnly);
    }
    if candidates.len() <= 1 {
        let first_bad = candidates.first().unwrap_or(&bad);
        println!("{} is the first bad commit", first_bad.to_hex());
        return Ok(BisectStep::Done);
    }
    let next = if skipped.is_empty() {
        candidates[candidates.len() / 2].clone()
    } else {
        candidates
            .iter()
            .find(|candidate| {
                candidates.iter().all(|other| {
                    candidate == &other
                        || is_ancestor_commit_cached(&commit_cache, candidate, other)
                            .unwrap_or(false)
                })
            })
            .cloned()
            .unwrap_or_else(|| candidates[candidates.len() - 1].clone())
    };
    if bisect_no_checkout(repo) {
        fs::write(
            repo.git_dir.join("BISECT_HEAD"),
            format!("{}\n", next.to_hex()),
        )?;
    } else {
        checkout_worktree(repo, store, &next)?;
        refs.write_head_direct(&next)?;
    }
    fs::write(
        repo.git_dir.join("BISECT_EXPECTED_REV"),
        format!("{}\n", next.to_hex()),
    )?;
    println!(
        "Bisecting: {} revisions left to test after this",
        candidates.len().saturating_sub(2)
    );
    Ok(BisectStep::Continue)
}

fn bisect_next_command() -> Result<()> {
    let repo = find_repo()?;
    ensure_bisect_started(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    match bisect_next(&repo, &store)? {
        BisectStep::SkippedOnly => Err(CliError::Exit(2)),
        _ => Ok(()),
    }
}

fn bisect_skip(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    ensure_bisect_started(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let revs = if args.is_empty() {
        vec![bisect_current_target(&repo, &refs)?]
    } else {
        let mut revs = Vec::new();
        for arg in args {
            revs.extend(resolve_bisect_skip_arg(&repo, &store, arg)?);
        }
        revs
    };
    for id in revs {
        refs.write_ref(&bisect_skip_ref(&id), &id)?;
        bisect_append_log(&repo, "skip", &id)?;
    }
    match bisect_next(&repo, &store)? {
        BisectStep::SkippedOnly => Err(CliError::Exit(2)),
        _ => Ok(()),
    }
}

fn bisect_terms(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    if !repo.git_dir.join("BISECT_START").is_file() {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: no terms defined\n".into(),
        });
    }
    let (bad_term, good_term) = bisect_terms_for_repo(&repo)?;
    match args {
        [] => {
            println!("Your current terms are {good_term} for the old state");
            println!("and {bad_term} for the new state.");
        }
        [arg] if matches!(arg.as_str(), "--term-good" | "--term-old") => println!("{good_term}"),
        [arg] if matches!(arg.as_str(), "--term-bad" | "--term-new") => println!("{bad_term}"),
        _ => {
            return Err(CliError::Fatal {
                code: 129,
                message: "usage: git bisect terms [--term-good | --term-bad]".into(),
            });
        }
    }
    Ok(())
}

fn bisect_visualize(args: &[String]) -> Result<()> {
    let repo = find_repo()?;
    ensure_bisect_started(&repo)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let bad = refs.resolve("refs/bisect/bad")?;
    let mut revs = vec![bad.to_hex()];
    for good in bisect_good_ids(&repo)? {
        revs.push(format!("^{}", good.to_hex()));
    }
    let mut oneline = false;
    let mut parents = false;
    let mut reverse = false;
    let mut stat = false;
    let mut numstat = false;
    let mut shortstat = false;
    let mut raw = false;
    let mut summary = false;
    let mut name_only = false;
    let mut name_status = false;
    let mut format = None::<&str>;
    let mut max_count = None::<&str>;
    let mut since = None::<&str>;
    let mut pretty = None::<&str>;
    let mut idx = 0;
    while idx < args.len() {
        let arg = args[idx].as_str();
        match arg {
            "--oneline" => oneline = true,
            "--parents" => parents = true,
            "--reverse" => reverse = true,
            "--stat" => stat = true,
            "--numstat" => numstat = true,
            "--shortstat" => shortstat = true,
            "--raw" => raw = true,
            "--summary" => summary = true,
            "--name-only" => name_only = true,
            "--name-status" => name_status = true,
            "--max-count" | "-n" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("{arg} requires a value"),
                    });
                };
                max_count = Some(value.as_str());
            }
            "--since" | "--after" => {
                idx += 1;
                let Some(value) = args.get(idx) else {
                    return Err(CliError::Fatal {
                        code: 129,
                        message: format!("{arg} requires a value"),
                    });
                };
                since = Some(value.as_str());
            }
            value if value.starts_with("--max-count=") => {
                max_count = value.strip_prefix("--max-count=");
            }
            value if value.starts_with("-n") && value.len() > 2 => {
                max_count = Some(&value[2..]);
            }
            value if value.starts_with("--since=") => {
                since = value.strip_prefix("--since=");
            }
            value if value.starts_with("--after=") => {
                since = value.strip_prefix("--after=");
            }
            value if value.starts_with("--format=") => {
                format = value.strip_prefix("--format=");
            }
            value if value.starts_with("--pretty=") => {
                pretty = value.strip_prefix("--pretty=");
            }
            value if value.starts_with('-') => {
                return Err(CliError::Fatal {
                    code: 128,
                    message: format!("unrecognized argument: {value}"),
                });
            }
            value => revs.push(value.to_owned()),
        }
        idx += 1;
    }
    history_commands::log(history_commands::LogOptions {
        oneline,
        zero: false,
        all: false,
        parents,
        first_parent: false,
        no_diff_merges: false,
        diff_merges: None,
        separate_merges: false,
        dd: false,
        reverse,
        root: false,
        patch: false,
        patch_with_stat: false,
        combined: false,
        dense_combined: false,
        stat,
        numstat,
        shortstat,
        raw,
        summary,
        name_only,
        name_status,
        diff_required: false,
        pickaxe_string: None,
        pickaxe_regex: None,
        pickaxe_regex_mode: false,
        pickaxe_all: false,
        decorate: None,
        clear_decorations: false,
        ignore_matching_lines: Vec::new(),
        walk_reflogs: false,
        no_walk: false,
        format,
        max_count,
        since,
        date: None,
        pretty,
        revs,
    })
}

fn bisect_replay(path: Option<&str>) -> Result<()> {
    let Some(path) = path else {
        return Err(CliError::Fatal {
            code: 129,
            message: "bisect replay requires a log file".into(),
        });
    };
    let content = fs::read_to_string(path)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix("git bisect ") else {
            continue;
        };
        let mut parts = rest.split_whitespace();
        let Some(command) = parts.next() else {
            continue;
        };
        match command {
            "start" => {
                let args = parse_bisect_log_args(rest.strip_prefix("start").unwrap_or_default());
                bisect_start(&args)?;
            }
            "good" | "old" => {
                let rev = parts.next().map(unquote_bisect_log_arg);
                bisect_mark(true, rev.as_deref())?;
            }
            "bad" | "new" => {
                let rev = parts.next().map(unquote_bisect_log_arg);
                bisect_mark(false, rev.as_deref())?;
            }
            "skip" => {
                let rev = parts.next().map(unquote_bisect_log_arg);
                let args = rev.into_iter().collect::<Vec<_>>();
                bisect_skip(&args)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn bisect_run(command: &[String]) -> Result<()> {
    if command.is_empty() {
        return Err(CliError::Fatal {
            code: 129,
            message: "bisect run failed: no command provided".into(),
        });
    }
    ensure_bisect_started(&find_repo()?)?;
    loop {
        let repo = find_repo()?;
        println!("running {}", shell_quoted_args(command));
        let status = std::process::Command::new(&command[0])
            .args(&command[1..])
            .current_dir(&repo.root)
            .status()
            .map_err(CliError::Io)?;
        let code = status.code().unwrap_or(128);
        match code {
            0 => bisect_mark(true, None)?,
            125 => {
                if bisect_skip_current_for_run()? == BisectStep::SkippedOnly {
                    return Err(CliError::Stderr {
                        code: 2,
                        text: "error: bisect run cannot continue any more\n".into(),
                    });
                }
            }
            1..=127 => bisect_mark(false, None)?,
            _ => {
                return Err(CliError::Stderr {
                    code: bisect_run_abort_exit_code(code),
                    text: format!(
                        "error: bisect run failed: exit code {code} from {} is < 0 or >= 128\n",
                        shell_quoted_args(command)
                    ),
                });
            }
        }
        if matches!(code, 126 | 127) && bisect_remaining_candidates(&find_repo()?)? <= 1 {
            bisect_validate_known_good_for_run(command, code)?;
        }
        let repo = find_repo()?;
        if !repo.git_dir.join("BISECT_START").is_file() || bisect_remaining_candidates(&repo)? <= 1
        {
            break;
        }
    }
    Ok(())
}

fn bisect_skip_current_for_run() -> Result<BisectStep> {
    let repo = find_repo()?;
    ensure_bisect_started(&repo)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let id = bisect_current_target(&repo, &refs)?;
    refs.write_ref(&bisect_skip_ref(&id), &id)?;
    bisect_append_log(&repo, "skip", &id)?;
    bisect_next(&repo, &store)
}

fn bisect_validate_known_good_for_run(command: &[String], bad_code: i32) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let current = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1).resolve("HEAD")?;
    let Some(good) = bisect_good_ids(&repo)?.into_iter().next() else {
        return Ok(());
    };
    checkout_worktree(&repo, &store, &good)?;
    RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1).write_head_direct(&good)?;
    print_bisect_commit_summary(&store, &good)?;
    println!("running {}", shell_quoted_args(command));
    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .current_dir(&repo.root)
        .status()
        .map_err(CliError::Io)?;
    let good_code = status.code().unwrap_or(128);
    checkout_worktree(&repo, &store, &current)?;
    RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1).write_head_direct(&current)?;
    print_bisect_commit_summary(&store, &current)?;
    if good_code == bad_code {
        return Err(CliError::Stderr {
            code: 1,
            text: format!("error: bogus exit code {bad_code} for good revision\n"),
        });
    }
    Ok(())
}

fn bisect_run_abort_exit_code(code: i32) -> i32 {
    if code == 128 {
        128
    } else {
        256_i32.saturating_sub(code).clamp(1, 127)
    }
}

fn shell_quoted_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn bisect_reset(target: Option<&str>) -> Result<()> {
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let target_id = if let Some(target) = target {
        resolve_commitish(&repo, &store, target)
            .map_err(|_| CliError::Message(format!("'{target}' is not a valid commit")))?
    } else {
        let raw = fs::read_to_string(repo.git_dir.join("BISECT_START"))?;
        ObjectId::from_hex(GitHashAlgorithm::Sha1, raw.trim()).map_err(CliError::Io)?
    };
    checkout_worktree(&repo, &store, &target_id)?;
    if target.is_none() {
        match fs::read_to_string(repo.git_dir.join("BISECT_START_REF")) {
            Ok(branch) => refs.write_head_symbolic(branch.trim())?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                refs.write_head_direct(&target_id)?
            }
            Err(error) => return Err(CliError::Io(error)),
        }
    } else {
        refs.write_head_direct(&target_id)?;
    }
    bisect_clear_state(&repo)?;
    Ok(())
}

fn ensure_bisect_started(repo: &GitRepo) -> Result<()> {
    if repo.git_dir.join("BISECT_START").is_file() {
        return Ok(());
    }
    Err(CliError::Stderr {
        code: 1,
        text: "You need to start by \"git bisect start\"\n\n".to_owned(),
    })
}

fn bisect_log() -> Result<()> {
    let repo = find_repo()?;
    match fs::read_to_string(repo.git_dir.join("BISECT_LOG")) {
        Ok(log) => {
            print!("{log}");
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

fn bisect_good_ids(repo: &GitRepo) -> Result<Vec<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut ids = Vec::new();
    refs.for_each_resolved_ref("refs/bisect/good/", |_, id| {
        ids.push(id.clone());
        Ok::<(), CliError>(())
    })?;
    Ok(ids)
}

fn bisect_good_ref(id: &ObjectId) -> String {
    format!("refs/bisect/good/{}", id.to_hex())
}

fn resolve_bisect_skip_arg(
    repo: &GitRepo,
    store: &LooseObjectStore,
    arg: &str,
) -> Result<Vec<ObjectId>> {
    let Some((left, right)) = arg.split_once("..") else {
        return Ok(vec![resolve_commitish(repo, store, arg)?]);
    };
    if right.is_empty() {
        return Ok(vec![resolve_commitish(repo, store, arg)?]);
    }
    let commit_cache = CommitObjectCache::new(store);
    let right_id = resolve_commitish(repo, store, right)?;
    let left_id = (!left.is_empty())
        .then(|| resolve_commitish(repo, store, left))
        .transpose()?;
    let mut ids = collect_commits_cached(repo, store, &commit_cache, &[right_id.to_hex()], None)?;
    if let Some(left_id) = left_id {
        ids.retain(|id| !is_ancestor_commit_cached(&commit_cache, id, &left_id).unwrap_or(false));
    }
    Ok(ids)
}

fn bisect_skipped_ids(repo: &GitRepo) -> Result<Vec<ObjectId>> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let mut ids = Vec::new();
    refs.for_each_resolved_ref("refs/bisect/skip/", |_, id| {
        ids.push(id.clone());
        Ok::<(), CliError>(())
    })?;
    Ok(ids)
}

fn bisect_skip_ref(id: &ObjectId) -> String {
    format!("refs/bisect/skip/{}", id.to_hex())
}

fn bisect_remaining_candidates(repo: &GitRepo) -> Result<usize> {
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let bad = refs.resolve("refs/bisect/bad")?;
    let goods = bisect_good_ids(repo)?;
    let skipped = bisect_skipped_ids(repo)?;
    let pathspecs = bisect_pathspecs(repo)?;
    let commit_cache = CommitObjectCache::new(&store);
    let mut candidates = bisect_candidate_commits(repo, &store, &commit_cache, &bad)?
        .into_iter()
        .filter(|id| {
            !goods
                .iter()
                .any(|good| is_ancestor_commit_cached(&commit_cache, id, good).unwrap_or(false))
        })
        .collect::<Vec<_>>();
    if !pathspecs.is_empty() {
        candidates.retain(|id| {
            bisect_commit_touches_pathspecs(&store, &commit_cache, id, &pathspecs).unwrap_or(true)
        });
    }
    candidates.retain(|id| !goods.iter().any(|good| good == id));
    candidates.retain(|id| !skipped.iter().any(|skipped| skipped == id));
    Ok(candidates.len())
}

fn bisect_report_only_skipped_left(
    repo: &GitRepo,
    store: &LooseObjectStore,
    stdout_possible: &[ObjectId],
    log_possible: &[ObjectId],
) -> Result<()> {
    println!("There are only 'skip'ped commits left to test.");
    println!("The first bad commit could be any of:");
    for id in stdout_possible {
        println!("{}", id.to_hex());
    }
    println!("We cannot bisect more!");

    use std::fs::OpenOptions;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(repo.git_dir.join("BISECT_LOG"))?;
    writeln!(file, "# only skipped commits left to test")?;
    for id in log_possible {
        writeln!(
            file,
            "# possible first bad commit: [{}] {}",
            id.to_hex(),
            bisect_commit_subject(store, id)?
        )?;
    }
    Ok(())
}

fn dedup_object_ids(ids: &mut Vec<ObjectId>) {
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.to_hex()));
}

fn print_bisect_commit_summary(store: &LooseObjectStore, id: &ObjectId) -> Result<()> {
    let commit_cache = CommitObjectCache::new(store);
    let commit = commit_cache.read_commit(id)?;
    println!(
        "[{}] {}",
        id.to_hex(),
        commit_message_subject(&commit.message)
    );
    Ok(())
}

fn bisect_commit_subject(store: &LooseObjectStore, id: &ObjectId) -> Result<String> {
    let commit_cache = CommitObjectCache::new(store);
    let commit = commit_cache.read_commit(id)?;
    Ok(commit_message_subject(&commit.message))
}

fn commit_message_subject(message: &[u8]) -> String {
    String::from_utf8_lossy(message)
        .lines()
        .next()
        .unwrap_or("")
        .to_owned()
}

fn bisect_append_log(repo: &GitRepo, term: &str, id: &ObjectId) -> Result<()> {
    use std::fs::OpenOptions;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(repo.git_dir.join("BISECT_LOG"))?;
    writeln!(file, "git bisect {term} {}", id.to_hex())?;
    Ok(())
}

fn bisect_append_start_log(repo: &GitRepo, args: &[String]) -> Result<()> {
    use std::fs::OpenOptions;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(repo.git_dir.join("BISECT_LOG"))?;
    if args.is_empty() {
        writeln!(file, "git bisect start")?;
    } else {
        writeln!(file, "git bisect start {}", args.join(" "))?;
    }
    Ok(())
}

fn bisect_terms_for_repo(repo: &GitRepo) -> Result<(String, String)> {
    match fs::read_to_string(repo.git_dir.join("BISECT_TERMS")) {
        Ok(content) => {
            let mut lines = content.lines();
            let bad = lines.next().unwrap_or("bad").to_owned();
            let good = lines.next().unwrap_or("good").to_owned();
            Ok((bad, good))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(("bad".to_owned(), "good".to_owned()))
        }
        Err(error) => Err(CliError::Io(error)),
    }
}

fn parse_bisect_log_args(value: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = value.trim().chars().peekable();
    let mut in_single = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => in_single = !in_single,
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ch if ch.is_whitespace() && !in_single => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn unquote_bisect_log_arg(value: &str) -> String {
    value.trim_matches('\'').to_owned()
}

fn bisect_clear_state(repo: &GitRepo) -> Result<()> {
    bisect_clear_refs(repo)?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_START"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_START_REF"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_TERMS"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_NAMES"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_NO_CHECKOUT"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_FIRST_PARENT"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_HEAD"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_EXPECTED_REV"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_ANCESTORS_OK"))?;
    remove_file_if_exists(&repo.git_dir.join("BISECT_LOG"))
}

fn bisect_clear_refs(repo: &GitRepo) -> Result<()> {
    let path = repo.git_dir.join("refs/bisect");
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CliError::Io(error)),
    }
}

pub(crate) fn sequencer_command(
    command: &str,
    abort: bool,
    continue_: bool,
    no_commit: bool,
    mainline: Option<usize>,
    commits: Vec<String>,
) -> Result<()> {
    if abort && continue_ {
        return Err(CliError::Fatal {
            code: 129,
            message: format!("cannot use --abort and --continue with {command}"),
        });
    }
    if abort {
        return Err(CliError::Stderr {
            code: 128,
            text: format!("error: no cherry-pick or revert in progress\nfatal: {command} failed\n"),
        });
    }
    if continue_ {
        return Err(CliError::Stderr {
            code: 128,
            text: format!("error: no cherry-pick or revert in progress\nfatal: {command} failed\n"),
        });
    }
    sequencer_pick(command == "revert", no_commit, mainline, commits)
}

pub(crate) fn sequencer_pick(
    revert: bool,
    no_commit: bool,
    mainline: Option<usize>,
    commits: Vec<String>,
) -> Result<()> {
    if commits.len() != 1 {
        return Err(CliError::Fatal {
            code: 129,
            message: "currently exactly one commit is supported".into(),
        });
    }
    let repo = find_repo()?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    let tree_cache = TreeObjectCache::new(&store);
    if !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 1,
            message: "local changes would be overwritten".into(),
        });
    }
    let picked_id = resolve_commitish_or_bad_revision(&repo, &store, &commits[0])?;
    let picked = commit_cache.read_commit(&picked_id)?;
    let parent_id = sequencer_parent_for_pick(&picked, mainline)?;
    let base_index = if revert {
        tree_cache.read_tree_to_index(&picked.tree)?
    } else {
        read_treeish_index_cached(&repo, &store, &tree_cache, &parent_id.to_hex())?
    };
    let patch_index = if revert {
        read_treeish_index_cached(&repo, &store, &tree_cache, &parent_id.to_hex())?
    } else {
        tree_cache.read_tree_to_index(&picked.tree)?
    };
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let current_index = read_head_index_with_caches(&repo, &commit_cache, &tree_cache)?;
    let new_index = apply_tree_delta(&base_index, &patch_index, &current_index)?;
    remove_tracked_paths_missing_from_target(&repo, &current_index, &new_index)?;
    new_index.write_to_path(&repo.index_path)?;
    checkout_index(
        &store,
        &new_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    let message = if revert {
        revert_message(&picked_id, &picked)
    } else {
        picked.message.clone()
    };
    if no_commit {
        let auto_merge_tree = write_tree_from_index(&store, &new_index)?;
        fs::write(repo.git_dir.join("AUTO_MERGE"), auto_merge_tree.to_hex() + "\n")?;
        fs::write(repo.git_dir.join("MERGE_MSG"), &message)?;
        fs::write(repo.git_dir.join("COMMIT_EDITMSG"), &message)?;
        return Ok(());
    }
    let tree = write_tree_from_index(&store, &new_index)?;
    let current_head = commit_cache.read_commit(&head_id)?;
    if current_head.tree == tree {
        return Err(CliError::Message("nothing to commit".into()));
    }
    let author = if revert {
        signature_from_identity(&repo, "GIT_AUTHOR")?
    } else {
        signature_from_commit_bytes(&picked.author)?
    };
    let committer = signature_from_identity(&repo, "GIT_COMMITTER")?;
    let commit = CommitBuilder::new(tree.clone(), author.clone(), committer)
        .parent(head_id)
        .message(message.clone())?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    fs::write(repo.git_dir.join("AUTO_MERGE"), tree.to_hex() + "\n")?;
    print_sequencer_commit_summary(
        &repo,
        &store,
        &id,
        &message,
        &author,
        &current_head.tree,
        &tree,
    )?;
    Ok(())
}

fn print_sequencer_commit_summary(
    repo: &GitRepo,
    store: &LooseObjectStore,
    id: &ObjectId,
    message: &[u8],
    author: &Signature,
    parent_tree: &ObjectId,
    tree: &ObjectId,
) -> Result<()> {
    let branch = sequencer_summary_branch(repo)?;
    println!(
        "[{branch} {}] {}",
        short_object_id(id),
        commit_subject(message)
    );
    println!(" Date: {}", sequencer_signature_summary_date(author)?);
    let tree_cache = TreeObjectCache::new(store);
    let old_index = tree_cache.read_tree_to_index(parent_tree)?;
    let new_index = tree_cache.read_tree_to_index(tree)?;
    let entries = diff_indexes(&old_index, &new_index)?;
    let context = DiffIndexContext {
        repo,
        store,
        old_index: &old_index,
        new_index: &new_index,
        old_source: DiffSideSource::Index,
        new_source: DiffSideSource::Index,
    };
    let rows = diff_stat_rows_with_whitespace(
        &context,
        &entries,
        DiffStatOptions {
            whitespace_mode: DiffWhitespaceMode::None,
            relative_prefix: None,
            ignore_matching_lines: &[],
            ignore_blank_lines: false,
            compact_summary: false,
        },
    )?;
    if !rows.is_empty() {
        print_diff_stat_summary(&rows);
    }
    print_summary_entries(&old_index, &new_index, &entries, None)?;
    Ok(())
}

fn sequencer_summary_branch(repo: &GitRepo) -> Result<String> {
    let raw = fs::read_to_string(repo.git_dir.join("HEAD")).unwrap_or_default();
    if let Some(name) = raw
        .trim_end_matches('\n')
        .strip_prefix("ref: ")
        .map(str::to_owned)
    {
        return Ok(name
            .strip_prefix("refs/heads/")
            .unwrap_or(name.as_str())
            .to_owned());
    }
    if !raw.trim().is_empty() {
        Ok("detached HEAD".to_owned())
    } else {
        Ok("HEAD".to_owned())
    }
}

fn sequencer_signature_summary_date(signature: &Signature) -> Result<String> {
    let offset = parse_timezone_offset(&signature.timezone).ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "commit has invalid author timezone".into(),
    })?;
    let utc =
        chrono::DateTime::from_timestamp(signature.timestamp, 0).ok_or_else(|| CliError::Fatal {
            code: 128,
            message: "commit author timestamp is out of range".into(),
        })?;
    Ok(utc
        .with_timezone(&offset)
        .format("%a %b %-d %H:%M:%S %Y %z")
        .to_string())
}

pub(crate) fn sequencer_parent_for_pick(
    commit: &zmin_git_core::CommitObject,
    mainline: Option<usize>,
) -> Result<ObjectId> {
    match (commit.parents.len(), mainline) {
        (1, None) => Ok(commit.parents[0].clone()),
        (1, Some(_)) => Err(CliError::Fatal {
            code: 128,
            message: "mainline was specified but commit is not a merge.".into(),
        }),
        (count, Some(mainline)) if count > 1 && (1..=count).contains(&mainline) => {
            Ok(commit.parents[mainline - 1].clone())
        }
        (count, Some(mainline)) if count > 1 => Err(CliError::Fatal {
            code: 128,
            message: format!("commit does not have parent {mainline}"),
        }),
        (count, None) if count > 1 => Err(CliError::Fatal {
            code: 128,
            message: "commit is a merge but no -m option was given".into(),
        }),
        _ => Err(CliError::Fatal {
            code: 128,
            message: "cannot cherry-pick a root commit".into(),
        }),
    }
}

pub(crate) fn apply_tree_delta(
    base_index: &GitIndex,
    patch_index: &GitIndex,
    current_index: &GitIndex,
) -> Result<GitIndex> {
    let mut next = current_index.clone();
    for diff in diff_indexes(base_index, patch_index)? {
        let base = find_index_entry(base_index, &diff.path);
        let patch = find_index_entry(patch_index, &diff.path);
        let current = find_index_entry(current_index, &diff.path);
        if !merge_tree_same_entry(current, base) && !merge_tree_same_entry(current, patch) {
            return Err(CliError::Fatal {
                code: 1,
                message: format!(
                    "could not apply changes to {}",
                    String::from_utf8_lossy(&diff.path)
                ),
            });
        }
        match patch {
            Some(entry) => next.upsert(entry.clone())?,
            None => {
                next.remove_path(&diff.path)?;
            }
        }
    }
    Ok(next)
}

fn revert_message(id: &ObjectId, commit: &zmin_git_core::CommitObject) -> Vec<u8> {
    let subject = commit_subject(&commit.message);
    format!(
        "Revert \"{subject}\"\n\nThis reverts commit {}.\n",
        id.to_hex()
    )
    .into_bytes()
}

pub(crate) fn rebase(
    abort: bool,
    continue_: bool,
    onto: Option<&str>,
    args: Vec<String>,
    preserve_merges: bool,
    interactive: bool,
) -> Result<()> {
    if abort && continue_ {
        return Err(CliError::Fatal {
            code: 129,
            message: "cannot use --abort and --continue with rebase".into(),
        });
    }
    if abort {
        return rebase_abort();
    }
    if continue_ {
        return rebase_continue();
    }
    if abort || continue_ {
        return Err(CliError::Fatal {
            code: 128,
            message: "no rebase in progress".into(),
        });
    }
    if args.len() > 2 {
        return Err(CliError::Fatal {
            code: 129,
            message: "usage: git rebase [--onto <newbase>] [<upstream> [<branch>]]".into(),
        });
    }
    let repo = find_repo()?;
    let branch = args.get(1).cloned();
    let upstream = match args.first().map(String::as_str) {
        Some(upstream) => upstream.to_owned(),
        None => rebase_configured_upstream(&repo)?,
    };
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    if !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 1,
            message: "cannot rebase with local changes".into(),
        });
    }
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    if let Some(branch) = branch.as_deref() {
        rebase_switch_branch_without_checkout(&refs, branch)?;
    }
    let head = refs.resolve("HEAD")?;
    let upstream_id = resolve_commitish(&repo, &store, &upstream)?;
    let new_base = onto.unwrap_or(&upstream);
    let new_base_id = resolve_commitish(&repo, &store, new_base)?;
    let commit_cache = CommitObjectCache::new(&store);
    if onto.is_none() && is_ancestor_commit_cached(&commit_cache, &head, &upstream_id)? {
        checkout_worktree(&repo, &store, &upstream_id)?;
        update_head_to_commit(&refs, &upstream_id)?;
        println!("Fast-forwarded to {upstream}");
        return Ok(());
    }
    let revs = RevListRevs {
        include: vec![head.to_hex()],
        exclude: vec![upstream_id.to_hex()],
        extra_objects: Vec::new(),
    };
    let mut commits =
        collect_commits_with_exclusions_cached(&repo, &store, &commit_cache, &revs, None)?;
    commits.reverse();
    let interactive_todo = if interactive {
        checkout_worktree(&repo, &store, &new_base_id)?;
        update_head_to_commit(&refs, &new_base_id)?;
        Some(edit_interactive_rebase_todo(
            &repo,
            &commit_cache,
            &commits,
            &head,
        )?)
    } else {
        None
    };
    if !interactive {
        checkout_worktree(&repo, &store, &new_base_id)?;
        update_head_to_commit(&refs, &new_base_id)?;
    }
    if preserve_merges {
        return rebase_commits_preserving_merges(
            &repo,
            &store,
            &refs,
            &commit_cache,
            &new_base_id,
            commits,
        );
    }
    if let Some(todo) = interactive_todo {
        for (index, item) in todo.iter().enumerate() {
            match item.command {
                RebaseTodoCommand::Pick => {
                    sequencer_pick(false, false, None, vec![item.commit.to_hex()])?
                }
                RebaseTodoCommand::Reword => {
                    let message = edit_rebase_commit_message(&repo, &commit_cache, &item.commit)?;
                    rebase_pick_commit_with_message(
                        &repo,
                        &store,
                        &commit_cache,
                        &item.commit,
                        Some(message),
                        true,
                    )?;
                }
                RebaseTodoCommand::Squash => {
                    rebase_squash_commit(&repo, &store, &commit_cache, &item.commit, true)?;
                }
                RebaseTodoCommand::Fixup => {
                    rebase_squash_commit(&repo, &store, &commit_cache, &item.commit, false)?;
                }
                RebaseTodoCommand::Edit => {
                    rebase_pick_commit_with_message(
                        &repo,
                        &store,
                        &commit_cache,
                        &item.commit,
                        None,
                        true,
                    )?;
                    write_rebase_edit_state(&repo, &head, &todo[index + 1..])?;
                    eprintln!(
                        "Stopped at {}...  # {}",
                        short_object_id(&item.commit),
                        commit_subject(&commit_cache.read_commit(&item.commit)?.message)
                    );
                    eprintln!("You can amend the commit now, with\n");
                    eprintln!("  git commit --amend \n");
                    eprintln!("Once you are satisfied with your changes, run\n");
                    eprintln!("  git rebase --continue");
                    return Ok(());
                }
                RebaseTodoCommand::Drop => {}
            }
        }
    } else {
        let mut rebased_head = None;
        for commit in commits {
            rebased_head = Some(rebase_pick_commit_with_message(
                &repo,
                &store,
                &commit_cache,
                &commit,
                None,
                false,
            )?);
        }
        if let Some(rebased_head) = rebased_head {
            let checkout_metadata = WorktreeCheckoutMetadata {
                ref_name: current_branch_ref(&refs)?,
                treeish: Some(rebased_head.clone()),
            };
            let mut final_index = read_repo_index(&repo)?;
            checkout_worktree_updates_to_index_with_metadata(
                &repo,
                &store,
                &final_index,
                &checkout_metadata,
            )?;
            refresh_tracked_index_metadata_matching(&repo, &mut final_index, &[])?;
            final_index.write_to_path(&repo.index_path)?;
        }
    }
    println!("Successfully rebased and updated HEAD.");
    Ok(())
}

fn rebase_switch_branch_without_checkout(refs: &RefStore, branch: &str) -> Result<()> {
    let Some(ref_name) = branch_checkout_ref(refs, branch)? else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("invalid branch '{branch}'"),
        });
    };
    refs.write_symbolic_ref("HEAD", &ref_name)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RebaseTodoCommand {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

#[derive(Debug, Clone)]
struct RebaseTodoItem {
    command: RebaseTodoCommand,
    commit: ObjectId,
}

fn edit_interactive_rebase_todo(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
    orig_head: &ObjectId,
) -> Result<Vec<RebaseTodoItem>> {
    let rebase_dir = repo.git_dir.join("rebase-merge");
    fs::create_dir_all(&rebase_dir)?;
    fs::write(
        rebase_dir.join("orig-head"),
        format!("{}\n", orig_head.to_hex()),
    )?;
    let todo_path = rebase_dir.join("git-rebase-todo");
    let mut todo = String::new();
    for commit_id in commits {
        let commit = commit_cache.read_commit(commit_id)?;
        todo.push_str(&format!(
            "pick {} # {}\n",
            short_object_id(commit_id),
            commit_subject(&commit.message)
        ));
    }
    todo.push_str("\n# Rebase ");
    todo.push_str(&commits.len().to_string());
    todo.push_str(" commit");
    if commits.len() != 1 {
        todo.push('s');
    }
    todo.push_str("\n#\n# Commands:\n# p, pick <commit> = use commit\n# r, reword <commit> = use commit, but edit the commit message\n# e, edit <commit> = use commit, but stop for amending\n# s, squash <commit> = use commit, but meld into previous commit\n# f, fixup <commit> = like \"squash\" but keep only the previous commit's message\n# d, drop <commit> = remove commit\n");
    fs::write(&todo_path, todo)?;
    run_sequence_editor(repo, &todo_path)?;
    let edited = fs::read_to_string(&todo_path)?;
    let parsed = parse_interactive_rebase_todo(repo, commit_cache, commits, &edited)?;
    remove_path_if_exists(&rebase_dir)?;
    Ok(parsed)
}

fn run_sequence_editor(repo: &GitRepo, path: &Path) -> Result<()> {
    let Some(editor) = git_sequence_editor(repo)? else {
        return Err(CliError::Stderr {
            code: 1,
            text: "error: Terminal is dumb, but EDITOR unset\n".into(),
        });
    };
    run_shell_command_with_path(&editor, path)
}

fn run_shell_command_with_path(command: &str, path: &Path) -> Result<()> {
    let status = run_editor_command_with_path(command, path)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Fatal {
            code: status.code().unwrap_or(1),
            message: format!("editor command failed: {command}"),
        })
    }
}

fn parse_interactive_rebase_todo(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
    input: &str,
) -> Result<Vec<RebaseTodoItem>> {
    let mut out = Vec::new();
    for (line_number, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let command = parts.next().unwrap_or_default();
        let commit = parts.next().ok_or_else(|| CliError::Fatal {
            code: 1,
            message: format!("missing commit in rebase todo line {}", line_number + 1),
        })?;
        let command = match command {
            "pick" | "p" => RebaseTodoCommand::Pick,
            "reword" | "r" => RebaseTodoCommand::Reword,
            "edit" | "e" => RebaseTodoCommand::Edit,
            "squash" | "s" => RebaseTodoCommand::Squash,
            "fixup" | "f" => RebaseTodoCommand::Fixup,
            "drop" | "d" => RebaseTodoCommand::Drop,
            other => {
                return Err(invalid_interactive_rebase_command(
                    other,
                    line_number + 1,
                    line,
                ));
            }
        };
        let commit = resolve_rebase_todo_commit(repo, commit_cache, commits, commit)?;
        out.push(RebaseTodoItem { command, commit });
    }
    Ok(out)
}

fn invalid_interactive_rebase_command(command: &str, line_number: usize, line: &str) -> CliError {
    CliError::Stderr {
        code: 1,
        text: format!(
            "error: invalid command '{command}'\n\
             error: invalid line {line_number}: {line}\n\
             You can fix this with 'git rebase --edit-todo' and then run 'git rebase --continue'.\n\
             Or you can abort the rebase with 'git rebase --abort'.\n"
        ),
    }
}

fn resolve_rebase_todo_commit(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commits: &[ObjectId],
    value: &str,
) -> Result<ObjectId> {
    if let Ok(index) = value.parse::<usize>()
        && !commits.is_empty()
    {
        let index = index
            .checked_sub(1)
            .unwrap_or(0)
            .min(commits.len().saturating_sub(1));
        return Ok(commits[index].clone());
    }
    if let Some(commit) = commits
        .iter()
        .find(|commit| commit.to_hex().starts_with(value))
        .cloned()
    {
        return Ok(commit);
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit = resolve_commitish(repo, &store, value)?;
    let _ = commit_cache.read_commit(&commit)?;
    Ok(commit)
}

fn rebase_continue() -> Result<()> {
    let repo = find_repo()?;
    let rebase_dir = repo.git_dir.join("rebase-merge");
    let todo_path = rebase_dir.join("git-rebase-todo");
    if !todo_path.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: "no rebase in progress".into(),
        });
    }
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let commit_cache = CommitObjectCache::new(&store);
    if !worktree_clean(&repo, &store)? {
        return Err(CliError::Fatal {
            code: 1,
            message: "cannot continue rebase with local changes".into(),
        });
    }
    let todo = fs::read_to_string(&todo_path)?;
    let items = parse_interactive_rebase_todo(&repo, &commit_cache, &[], &todo)?;
    remove_path_if_exists(&rebase_dir)?;
    for item in items {
        match item.command {
            RebaseTodoCommand::Pick | RebaseTodoCommand::Edit => {
                sequencer_pick(false, false, None, vec![item.commit.to_hex()])?;
            }
            RebaseTodoCommand::Reword => {
                let message = edit_rebase_commit_message(&repo, &commit_cache, &item.commit)?;
                rebase_pick_commit_with_message(
                    &repo,
                    &store,
                    &commit_cache,
                    &item.commit,
                    Some(message),
                    true,
                )?;
            }
            RebaseTodoCommand::Squash => {
                rebase_squash_commit(&repo, &store, &commit_cache, &item.commit, true)?;
            }
            RebaseTodoCommand::Fixup => {
                rebase_squash_commit(&repo, &store, &commit_cache, &item.commit, false)?;
            }
            RebaseTodoCommand::Drop => {}
        }
    }
    println!("Successfully rebased and updated HEAD.");
    Ok(())
}

fn rebase_abort() -> Result<()> {
    let repo = find_repo()?;
    let rebase_dir = repo.git_dir.join("rebase-merge");
    if !rebase_dir.exists() {
        return Err(CliError::Fatal {
            code: 128,
            message: "no rebase in progress".into(),
        });
    }
    let orig_head = fs::read_to_string(rebase_dir.join("orig-head"))?;
    let orig_head =
        ObjectId::from_hex(GitHashAlgorithm::Sha1, orig_head.trim()).map_err(CliError::Io)?;
    let store = LooseObjectStore::new(repo.objects_dir.clone(), GitHashAlgorithm::Sha1);
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    checkout_worktree(&repo, &store, &orig_head)?;
    update_head_to_commit(&refs, &orig_head)?;
    remove_path_if_exists(&rebase_dir)
}

fn write_rebase_edit_state(
    repo: &GitRepo,
    orig_head: &ObjectId,
    remaining: &[RebaseTodoItem],
) -> Result<()> {
    let rebase_dir = repo.git_dir.join("rebase-merge");
    fs::create_dir_all(&rebase_dir)?;
    let mut todo = String::new();
    for item in remaining {
        todo.push_str(&format!(
            "{} {}\n",
            rebase_todo_command_name(item.command),
            item.commit.to_hex()
        ));
    }
    fs::write(rebase_dir.join("git-rebase-todo"), todo).map_err(CliError::Io)?;
    fs::write(
        rebase_dir.join("orig-head"),
        format!("{}\n", orig_head.to_hex()),
    )
    .map_err(CliError::Io)
}

fn rebase_todo_command_name(command: RebaseTodoCommand) -> &'static str {
    match command {
        RebaseTodoCommand::Pick => "pick",
        RebaseTodoCommand::Reword => "reword",
        RebaseTodoCommand::Edit => "edit",
        RebaseTodoCommand::Squash => "squash",
        RebaseTodoCommand::Fixup => "fixup",
        RebaseTodoCommand::Drop => "drop",
    }
}

fn rebase_squash_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    picked_id: &ObjectId,
    edit_message: bool,
) -> Result<ObjectId> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let picked = commit_cache.read_commit(picked_id)?;
    let tree_cache = TreeObjectCache::new(store);
    let parent_id = sequencer_parent_for_pick(&picked, None)?;
    let base_index = read_treeish_index_cached(repo, store, &tree_cache, &parent_id.to_hex())?;
    let patch_index = tree_cache.read_tree_to_index(&picked.tree)?;
    let current_index = read_head_index_with_caches(repo, commit_cache, &tree_cache)?;
    let new_index = apply_tree_delta(&base_index, &patch_index, &current_index)?;
    remove_tracked_paths_missing_from_target(repo, &current_index, &new_index)?;
    new_index.write_to_path(&repo.index_path)?;
    checkout_index(
        store,
        &new_index,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    smudge_worktree_filter_entries(repo, &new_index)?;
    let tree = write_tree_from_index(store, &new_index)?;
    let message = if edit_message {
        edit_rebase_squash_message(repo, &head_commit.message, &picked.message)?
    } else {
        head_commit.message.clone()
    };
    let author = signature_from_commit_bytes(&head_commit.author)?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let mut builder = CommitBuilder::new(tree, author, committer);
    for parent in &head_commit.parents {
        builder = builder.parent(parent.clone());
    }
    let commit = builder.message(message.clone())?.encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    println!("[{}] {}", short_object_id(&id), commit_subject(&message));
    Ok(id)
}

fn edit_rebase_squash_message(
    repo: &GitRepo,
    previous_message: &[u8],
    squashed_message: &[u8],
) -> Result<Vec<u8>> {
    let path = repo.git_dir.join("COMMIT_EDITMSG");
    let mut message = Vec::new();
    message.extend_from_slice(previous_message);
    if !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    message.push(b'\n');
    message.extend_from_slice(squashed_message);
    if !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    fs::write(&path, message)?;
    run_commit_editor(repo, &path)?;
    let mut edited = fs::read(&path)?;
    if !edited.ends_with(b"\n") {
        edited.push(b'\n');
    }
    Ok(cleanup_commit_message(edited, CommitCleanupMode::Default))
}

fn edit_rebase_commit_message(
    repo: &GitRepo,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    commit_id: &ObjectId,
) -> Result<Vec<u8>> {
    let commit = commit_cache.read_commit(commit_id)?;
    let path = repo.git_dir.join("COMMIT_EDITMSG");
    fs::write(&path, &commit.message)?;
    run_commit_editor(repo, &path)?;
    let mut message = fs::read(&path)?;
    if !message.ends_with(b"\n") {
        message.push(b'\n');
    }
    Ok(cleanup_commit_message(message, CommitCleanupMode::Default))
}

fn run_commit_editor(repo: &GitRepo, path: &Path) -> Result<()> {
    let Some(editor) = git_editor(repo)? else {
        return Err(editor_required_message_error());
    };
    run_shell_command_with_path(&editor, path)
}

fn rebase_pick_commit_with_message(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    picked_id: &ObjectId,
    message_override: Option<Vec<u8>>,
    update_worktree: bool,
) -> Result<ObjectId> {
    let tree_cache = TreeObjectCache::new(store);
    if update_worktree && !worktree_clean(repo, store)? {
        return Err(CliError::Fatal {
            code: 1,
            message: "local changes would be overwritten".into(),
        });
    }
    let picked = commit_cache.read_commit(picked_id)?;
    let parent_id = sequencer_parent_for_pick(&picked, None)?;
    let base_index = read_treeish_index_cached(repo, store, &tree_cache, &parent_id.to_hex())?;
    let patch_index = tree_cache.read_tree_to_index(&picked.tree)?;
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let head_id = refs.resolve("HEAD")?;
    let current_index = read_head_index_with_caches(repo, commit_cache, &tree_cache)?;
    let new_index = apply_tree_delta(&base_index, &patch_index, &current_index)?;
    remove_tracked_paths_missing_from_target(repo, &current_index, &new_index)?;
    new_index.write_to_path(&repo.index_path)?;
    if update_worktree {
        checkout_index(
            store,
            &new_index,
            &repo.root,
            CheckoutIndexOptions { force: true },
        )?;
        smudge_worktree_filter_entries(repo, &new_index)?;
    }
    let tree = write_tree_from_index(store, &new_index)?;
    let current_head = commit_cache.read_commit(&head_id)?;
    if current_head.tree == tree {
        return Err(CliError::Message("nothing to commit".into()));
    }
    let author = signature_from_commit_bytes(&picked.author)?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let message = message_override.unwrap_or_else(|| picked.message.clone());
    let commit = CommitBuilder::new(tree, author, committer)
        .parent(head_id)
        .message(message.clone())?
        .encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&refs, &id)?;
    println!("[{}] {}", short_object_id(&id), commit_subject(&message));
    Ok(id)
}

fn rebase_commits_preserving_merges(
    repo: &GitRepo,
    store: &LooseObjectStore,
    refs: &RefStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    new_base_id: &ObjectId,
    commits: Vec<ObjectId>,
) -> Result<()> {
    let mut rewritten = HashMap::<String, ObjectId>::new();
    for commit_id in commits {
        let commit = commit_cache.read_commit(&commit_id)?;
        if commit.parents.len() <= 1 {
            sequencer_pick(false, false, None, vec![commit_id.to_hex()])?;
            rewritten.insert(commit_id.to_hex(), refs.resolve("HEAD")?);
            continue;
        }

        let first_parent = rewritten_parent(&rewritten, new_base_id, &commit.parents[0]);
        if refs.resolve("HEAD")? != first_parent {
            checkout_worktree(repo, store, &first_parent)?;
            update_head_to_commit(refs, &first_parent)?;
        }

        let mut parents = vec![first_parent];
        for parent in commit.parents.iter().skip(1) {
            parents.push(rewritten_parent(&rewritten, new_base_id, parent));
        }
        let rebased = rebase_merge_commit(repo, store, commit_cache, &commit, &parents)?;
        rewritten.insert(commit_id.to_hex(), rebased);
    }
    println!("Successfully rebased and updated HEAD.");
    Ok(())
}

fn rewritten_parent(
    rewritten: &HashMap<String, ObjectId>,
    new_base_id: &ObjectId,
    parent: &ObjectId,
) -> ObjectId {
    rewritten
        .get(&parent.to_hex())
        .cloned()
        .unwrap_or_else(|| new_base_id.clone())
}

fn rebase_merge_commit(
    repo: &GitRepo,
    store: &LooseObjectStore,
    commit_cache: &CommitObjectCache<'_, LooseObjectStore>,
    original: &zmin_git_core::CommitObject,
    parents: &[ObjectId],
) -> Result<ObjectId> {
    if parents.len() < 2 {
        return Err(CliError::Fatal {
            code: 128,
            message: "cannot recreate merge commit without at least two parents".into(),
        });
    }
    let tree_cache = TreeObjectCache::new(store);
    let head_id = parents[0].clone();
    let target_id = parents[1].clone();
    let Some(base_id) = best_merge_base_cached(commit_cache, &head_id, &target_id)? else {
        return Err(CliError::Fatal {
            code: 128,
            message: "refusing to merge unrelated histories".into(),
        });
    };
    let base_commit = commit_cache.read_commit(&base_id)?;
    let head_commit = commit_cache.read_commit(&head_id)?;
    let target_commit = commit_cache.read_commit(&target_id)?;
    let base = read_commit_tree_index_cached(&tree_cache, &base_commit)?;
    let ours = read_commit_tree_index_cached(&tree_cache, &head_commit)?;
    let theirs = read_commit_tree_index_cached(&tree_cache, &target_commit)?;
    let merge_result = merge_indexes(store, &base, &ours, &theirs, &target_id.to_hex())?;
    let merged = match merge_result {
        MergeIndexResult::Clean(merged) => merged,
        MergeIndexResult::Conflicted { index, files } => {
            remove_tracked_paths_missing_from_target(repo, &ours, &index)?;
            checkout_merged_stage_zero(repo, store, &index)?;
            for file in files {
                println!("Auto-merging {}", String::from_utf8_lossy(&file.path));
                eprintln!(
                    "CONFLICT (content): Merge conflict in {}",
                    String::from_utf8_lossy(&file.path)
                );
            }
            index.write_to_path(&repo.index_path)?;
            eprintln!("Automatic merge failed; fix conflicts and then commit the result.");
            return Err(CliError::Exit(1));
        }
    };
    remove_tracked_paths_missing_from_target(repo, &ours, &merged)?;
    merged.write_to_path(&repo.index_path)?;
    checkout_index(
        store,
        &merged,
        &repo.root,
        CheckoutIndexOptions { force: true },
    )?;
    let tree = write_tree_from_index(store, &merged)?;
    let author = signature_from_commit_bytes(&original.author)?;
    let committer = signature_from_identity(repo, "GIT_COMMITTER")?;
    let mut builder = CommitBuilder::new(tree, author, committer);
    for parent in parents {
        builder = builder.parent(parent.clone());
    }
    let commit = builder.message(original.message.clone())?.encode()?;
    let id = store.write_object(GitObjectKind::Commit, &commit)?;
    update_head_to_commit(&RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1), &id)?;
    println!(
        "[{}] {}",
        short_object_id(&id),
        commit_subject(&original.message)
    );
    Ok(id)
}

fn rebase_configured_upstream(repo: &GitRepo) -> Result<String> {
    let refs = RefStore::new(&repo.git_dir, GitHashAlgorithm::Sha1);
    let branch_ref = current_branch_ref(&refs)?.ok_or_else(|| CliError::Fatal {
        code: 128,
        message: "cannot rebase detached HEAD without an explicit upstream".into(),
    })?;
    let branch = branch_display_name(&branch_ref);
    let remote = read_config_section_value(repo, "branch", &branch, "remote")?;
    let merge = read_config_section_value(repo, "branch", &branch, "merge")?;
    let (Some(remote), Some(merge)) = (remote, merge) else {
        return Err(CliError::Fatal {
            code: 128,
            message: format!("no upstream configured for branch '{branch}'"),
        });
    };
    let merge_branch = short_ref_name(&merge);
    if remote == "." {
        Ok(merge_branch)
    } else {
        Ok(format!("refs/remotes/{remote}/{merge_branch}"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn bisect_good_ids_use_loose_ref_over_stale_packed_ref() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(git_dir.join("objects")).expect("objects dir");
        let stale_id = ObjectId::new(GitHashAlgorithm::Sha1, &[1; 20]);
        let live_id = ObjectId::new(GitHashAlgorithm::Sha1, &[2; 20]);
        fs::write(
            git_dir.join("packed-refs"),
            format!(
                "{} refs/bisect/good/{}\n",
                stale_id.to_hex(),
                live_id.to_hex()
            ),
        )
        .expect("write packed refs");
        let refs = RefStore::new(&git_dir, GitHashAlgorithm::Sha1);
        refs.write_ref(&bisect_good_ref(&live_id), &live_id)
            .expect("write loose ref");
        let repo = GitRepo {
            root: dir.path().to_path_buf(),
            git_dir,
            objects_dir: dir.path().join(".git/objects"),
            index_path: dir.path().join(".git/index"),
        };

        let ids = bisect_good_ids(&repo).expect("good ids");

        assert_eq!(ids, vec![live_id]);
    }
}
